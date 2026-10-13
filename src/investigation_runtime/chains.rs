//! Explicit role graphs. Source order is context, never a join key or proof of completion.
use super::{
    Selector,
    findings::{excerpt, fact, occurrence, rules},
    selected,
};
use crate::{
    config::AnalyzerConfig, event_rules::ClassifiedRecord, evidence,
    investigation_policy::InvestigationPolicy, parser::LogEntry, processing::Budget,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) use crate::relationship_rules::{Matcher, field};
use crate::relationship_rules::{cardinality_supported, key};
fn identities(entry: &LogEntry, fields: &[String]) -> Vec<Value> {
    fields
        .iter()
        .map(|f| json!({"field":f,"value":field(entry,f)}))
        .collect()
}
// References into the same parsed slice retain physical and expansion order.
fn source_order(entry: &LogEntry) -> usize {
    std::ptr::from_ref(entry) as usize
}
fn samples<'a>(entries: &[&'a LogEntry]) -> Vec<&'a LogEntry> {
    let mut result = Vec::new();
    if let Some(first) = entries.first() {
        result.push(*first);
    }
    if let Some(last) = entries.last()
        && result
            .last()
            .is_none_or(|first| !std::ptr::eq(*first, *last))
    {
        result.push(*last);
    }
    result
}
fn step_samples<'a>(entries: &[&'a LogEntry], outcome: Option<&str>) -> Vec<&'a LogEntry> {
    let mut result = samples(entries);
    if let Some(selector) = outcome {
        let mut values: BTreeSet<_> = result
            .iter()
            .map(|e| field(e, selector).map(|v| v.to_string()))
            .collect();
        for entry in entries {
            if result.len() >= 4 {
                break;
            }
            if values.insert(field(entry, selector).map(|v| v.to_string())) {
                result.push(entry);
            }
        }
    }
    result
}
// These are endpoint intervals, never inferred operation durations or causes.
fn endpoint_interval(source: &LogEntry, target: &LogEntry, unique: bool) -> Value {
    let start = source
        .source_timestamp
        .filter(|_| !source.timestamp_year_inferred);
    let end = target
        .source_timestamp
        .filter(|_| !target.timestamp_year_inferred);
    let elapsed = if unique {
        start.zip(end).and_then(|(start, end)| {
            end.signed_duration_since(start)
                .num_nanoseconds()
                .filter(|n| *n >= 0)
        })
    } else {
        None
    };
    json!({"status":if elapsed.is_some(){"measured"}else{"unavailable"},
        "value":elapsed.map(|n| n as f64 / 1_000_000.0),"unit":"ms",
        "start_timestamp":start.map(|t| t.to_rfc3339()),"end_timestamp":end.map(|t| t.to_rfc3339()),
        "start_operation":field(source,"event.name"),"end_operation":field(target,"event.name"),
        "semantics":"selected_endpoint_interval_not_causality_or_operation_duration",
        "reason":if !unique {Some("ambiguous_endpoint_population")}else if start.is_none() || end.is_none(){Some("explicit_source_timestamps_unavailable")}else if elapsed.is_none(){Some("negative_or_out_of_range_interval")}else{None}})
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn calculate(
    policy: &InvestigationPolicy,
    config: &AnalyzerConfig,
    entries: &[LogEntry],
    selectors: &[Selector],
    scope: &str,
    context: &evidence::Context,
    snapshot: &Value,
    input_complete: bool,
    budget: &mut Budget,
    findings: &mut Vec<Value>,
    mut observer: Option<&mut dyn crate::relationship_rules::Observer>,
) {
    if policy.chains.is_empty()
        || !budget.reserve(
            "calculation",
            policy
                .roles
                .iter()
                .map(|role| {
                    if role.message_pattern.is_some() {
                        1_048_576
                    } else {
                        1024
                    }
                })
                .sum(),
        )
    {
        return;
    }
    let matchers: BTreeMap<_, _> = policy
        .roles
        .iter()
        .map(|r| (r.id.as_str(), Matcher::new(r)))
        .collect();
    let mut roles: BTreeMap<&str, Vec<&LogEntry>> = BTreeMap::new();
    // Independent selector counts explain recognition failures without guessing aliases.
    let mut diagnostics: BTreeMap<&str, Vec<usize>> = policy
        .roles
        .iter()
        .map(|role| (role.id.as_str(), vec![0; role.select.len() + 3]))
        .collect();
    let mut unresolved = false;
    for entry in entries {
        if !budget.checkpoint(
            "calculation",
            policy.roles.len() as u64 + entry.message.len() as u64,
        ) {
            return;
        }
        if !selected(entry, selectors, config) {
            continue;
        }
        unresolved |= matches!(
            entry.classification,
            Some(ClassifiedRecord::Conflict { .. } | ClassifiedRecord::Invalid { .. })
        );
        if !matches!(entry.classification, Some(ClassifiedRecord::Event { .. })) {
            continue;
        }
        for (id, matcher) in &matchers {
            if let Some(counts) = diagnostics.get_mut(id) {
                counts[0] += 1;
                if rules(entry)
                    .iter()
                    .any(|rule| matcher.role.rule_ids.contains(rule))
                {
                    counts[1] += 1;
                    for (index, condition) in matcher.role.select.iter().enumerate() {
                        if field(entry, &condition.field).as_ref() != Some(&condition.equals) {
                            counts[index + 2] += 1;
                        }
                    }
                    if matcher
                        .pattern
                        .as_ref()
                        .is_some_and(|pattern| !pattern.is_match(&entry.message))
                    {
                        counts[matcher.role.select.len() + 2] += 1;
                    }
                }
            }
            if matcher.matches(entry) {
                if !budget.reserve("calculation", 32) {
                    return;
                }
                roles.entry(id).or_default().push(entry);
            }
        }
    }
    for (chain_index, chain) in policy.chains.iter().enumerate() {
        if roles
            .get(chain.root_role.as_str())
            .is_none_or(Vec::is_empty)
        {
            if !budget.reserve("calculation", 262144) {
                return;
            }
            let matcher = &matchers[chain.root_role.as_str()];
            let counts = &diagnostics[chain.root_role.as_str()];
            let failed: Vec<_> = matcher
                .role
                .select
                .iter()
                .enumerate()
                .map(|(index, condition)| {
                    format!(
                        "{} equals {}: {} rule-matched records failed",
                        condition.field,
                        condition.equals,
                        counts[index + 2]
                    )
                })
                .collect();
            findings.push(fact(
                format!("{scope}-chain-{chain_index}-unrecognized"), scope, "unknown",
                "Configured chain found no matching roots; complete parsing does not establish relationship coverage.",
                Vec::new(), json!({"supporting_occurrences":[],"gap_classification":"recognition_unresolved","reason": format!("Chain {}; root role {}; {} selected classified events, {} matched rule IDs {:?}; selectors [{}]; message_pattern {:?}: {} rule-matched records failed; processed coverage {}. Counts are independent selector failures, not disjoint populations.", chain.id, chain.root_role, counts[0], counts[1], matcher.role.rule_ids, failed.join("; "), matcher.role.message_pattern, counts[matcher.role.select.len() + 2], if input_complete {"complete"} else {"partial"})}),
            ));
        }
        let root_fields: Vec<_> = chain
            .identity_fields
            .iter()
            .chain(&chain.scope_fields)
            .cloned()
            .collect();
        let mut roots: BTreeMap<Vec<String>, Vec<&LogEntry>> = BTreeMap::new();
        let mut missing_roots = Vec::new();
        for root in roles.get(chain.root_role.as_str()).into_iter().flatten() {
            if !budget.checkpoint("calculation", root_fields.len() as u64)
                || !budget.reserve(
                    "calculation",
                    ((root_fields.len() as u64).saturating_mul(8192)).saturating_add(64),
                )
            {
                return;
            }
            if let Some(key) = key(root, &root_fields) {
                roots.entry(key).or_default().push(root);
            } else {
                missing_roots.push(*root);
            }
        }
        // Index each step by its explicit join + scope fields once per declaration.
        let mut indices = Vec::new();
        for step in &chain.steps {
            let fields: Vec<_> = step
                .join_fields
                .iter()
                .chain(&chain.scope_fields)
                .cloned()
                .collect();
            let mut index: BTreeMap<Vec<String>, Vec<&LogEntry>> = BTreeMap::new();
            let mut missing = 0usize;
            for target in roles.get(step.role.as_str()).into_iter().flatten() {
                if !budget.checkpoint("calculation", fields.len() as u64)
                    || !budget.reserve(
                        "calculation",
                        ((fields.len() as u64).saturating_mul(8192)).saturating_add(64),
                    )
                {
                    return;
                }
                if let Some(key) = key(target, &fields) {
                    index.entry(key).or_default().push(target);
                } else {
                    missing += 1;
                }
            }
            indices.push((fields, index, missing));
        }
        let mut later_index: BTreeMap<Vec<String>, Vec<&LogEntry>> = BTreeMap::new();
        for entry in entries.iter().filter(|e| selected(e, selectors, config)) {
            if !budget.checkpoint("calculation", chain.context_fields.len() as u64)
                || !budget.reserve(
                    "calculation",
                    (chain.context_fields.len() as u64 * 8192).saturating_add(32),
                )
            {
                return;
            }
            if let Some(k) = key(entry, &chain.context_fields) {
                let tail = later_index.entry(k).or_default();
                tail.push(entry);
            }
        }
        let mut ordered_roots: Vec<_> = roots.values().collect();
        ordered_roots.sort_by_key(|events| source_order(events[0]));
        for (root_index, root_entries) in ordered_roots.into_iter().enumerate() {
            if !budget.checkpoint("calculation", 1)
                || !budget.reserve(
                    "calculation",
                    262144
                        + context.finding_fields_allowance() * (chain.steps.len() as u64 * 6 + 5),
                )
            {
                return;
            }
            let root = root_entries[0];
            let mut bundle: Vec<Value> = samples(root_entries)
                .iter()
                .map(|e| excerpt(e, context, snapshot))
                .collect();
            let mut selected_nodes = BTreeMap::from([("$root", root_entries.clone())]);
            let mut steps = Vec::new();
            let context_key = key(root, &chain.context_fields);
            let complete = input_complete
                && !unresolved
                && missing_roots.is_empty()
                && root_entries.len() == 1
                && context_key.is_some();
            let mut node_completeness = BTreeMap::from([("$root", complete)]);
            for (step, (fields, index, missing)) in chain.steps.iter().zip(&indices) {
                let sources = &selected_nodes[step.from.as_str()];
                if let Some(observer) = observer.as_deref_mut() {
                    if !budget.checkpoint("calculation", observer.work_units()) {
                        return;
                    }
                    observer.chain_step(chain, step, root_entries, sources);
                }
                let mut source_keys: BTreeMap<Vec<String>, Vec<&LogEntry>> = BTreeMap::new();
                let mut missing_source = 0usize;
                for source in sources {
                    if !budget.checkpoint("calculation", fields.len() as u64)
                        || !budget.reserve(
                            "calculation",
                            (fields.len() as u64 * 8192).saturating_add(64),
                        )
                    {
                        return;
                    }
                    if let Some(k) = key(source, fields) {
                        source_keys.entry(k).or_default().push(source);
                    } else {
                        missing_source += 1;
                    }
                }
                let missing_source_identity = source_keys.is_empty() && missing_source > 0;
                let mut targets: Vec<&LogEntry> = Vec::new();
                let mut target_ids = BTreeSet::new();
                let mut ambiguous = 0usize;
                let mut ambiguous_samples = Vec::new();
                let mut joins_by_target = BTreeMap::new();
                for (k, from) in source_keys {
                    let Some(to) = index.get(&k) else {
                        continue;
                    };
                    let supported = cardinality_supported(step.cardinality, from.len(), to.len());
                    let supported =
                        supported && !to.iter().any(|t| from.iter().any(|s| std::ptr::eq(*s, *t)));
                    if let Some(observer) = observer.as_deref_mut() {
                        if !budget.checkpoint("calculation", observer.work_units()) {
                            return;
                        }
                        observer.chain_group(chain, step, root_entries, &from, to, supported);
                    }
                    if !supported {
                        ambiguous += to.len();
                        for target in samples(to) {
                            if ambiguous_samples.len() < 2 {
                                ambiguous_samples.push(excerpt(target, context, snapshot));
                            }
                        }
                        continue;
                    }
                    if !budget.checkpoint("calculation", to.len() as u64)
                        || !budget.reserve("calculation", to.len() as u64 * 128)
                    {
                        return;
                    }
                    for target in to {
                        if target_ids
                            .insert((target.source_line_number, target.source_row_path.as_deref()))
                        {
                            targets.push(*target);
                            joins_by_target.insert(
                                (target.source_line_number, target.source_row_path.as_deref()),
                                (from[0], from.len(), to.len()),
                            );
                        }
                    }
                }
                targets.sort_by_key(|e| source_order(e));
                let sampled = step_samples(&targets, step.outcome_field.as_deref());
                let joins: Vec<_> = sampled.iter().map(|target| {
                    let (source,source_count,target_count)=joins_by_target[&(target.source_line_number,target.source_row_path.as_deref())];
                    json!({"source":occurrence(source,context,snapshot),"target":occurrence(target,context,snapshot),"identity":identities(source,fields),"source_count":source_count,"target_count":target_count,"endpoint_interval":({let mut interval=endpoint_interval(source,target,source_count==1 && target_count==1 && root_entries.len()==1); interval["source"]=occurrence(source,context,snapshot); interval["target"]=occurrence(target,context,snapshot); interval})})
                }).collect();
                let witnesses: Vec<_> = sampled
                    .iter()
                    .map(|e| excerpt(e, context, snapshot))
                    .collect();
                bundle.extend(witnesses.clone());
                bundle.extend(ambiguous_samples.clone());
                let outcomes: Vec<_> = step.outcome_field.as_ref().map(|f| sampled.iter().map(|e| json!({"level":step.outcome_level,"field":f,"value":field(e,f),"occurrence":occurrence(e,context,snapshot)})).collect()).unwrap_or_default();
                let step_complete = node_completeness[step.from.as_str()]
                    && *missing == 0
                    && missing_source == 0
                    && ambiguous == 0
                    && !sources.is_empty();
                let role = matchers[step.role.as_str()].role;
                let counts = &diagnostics[step.role.as_str()];
                let recognition = json!({"role":step.role,"rule_ids":role.rule_ids,
                    "rule_matched_records":counts[1],"role_matched_records":roles.get(step.role.as_str()).map_or(0,Vec::len),
                    "population":"selected_classified_records_in_this_input",
                    "failed_selectors":role.select.iter().enumerate().map(|(i,c)|json!({"field":c.field,"equals":c.equals,"failed_records":counts[i+2]})).collect::<Vec<_>>(),
                    "message_pattern_failed_records":counts[role.select.len()+2]});
                steps.push(json!({"recognition":recognition,"id":step.id,"label":step.label,"event_type":step.event_type,"from":step.from,"observed_count":targets.len()+ambiguous,"linked_count":targets.len(),"status":if ambiguous>0{"ambiguous"}else if sources.is_empty() || missing_source_identity {"unavailable"}else if targets.is_empty(){"not_observed"}else{"observed"},"count_completeness":if step_complete{"complete"}else{"partial"},"join_fields":fields,"cardinality":step.cardinality,"join_samples":joins,"ambiguity":{"excluded_target_occurrences":ambiguous,"role_occurrences_missing_identity":missing,"source_occurrences_missing_identity":missing_source},"evidence":witnesses,"ambiguous_evidence":ambiguous_samples,"omitted_occurrences":targets.len().saturating_sub(sampled.len()),"outcomes":outcomes,"omitted_outcomes":if step.outcome_field.is_some(){targets.len().saturating_sub(sampled.len())}else{0}}));
                node_completeness.insert(step.id.as_str(), step_complete);
                selected_nodes.insert(step.id.as_str(), targets);
            }
            let last_order = selected_nodes
                .values()
                .flatten()
                .map(|e| source_order(e))
                .max()
                .unwrap_or_else(|| source_order(root));
            let activity = context_key.and_then(|k| later_index.get(&k));
            let activity_count = activity.map(|events| {
                events
                    .len()
                    .saturating_sub(events.partition_point(|e| source_order(e) <= last_order))
            });
            if let Some(observer) = observer.as_deref_mut() {
                if !budget.checkpoint("calculation", observer.work_units()) {
                    return;
                }
                observer.chain_later(
                    chain,
                    root_entries,
                    activity.map(|events| {
                        &events[events.partition_point(|e| source_order(e) <= last_order)..]
                    }),
                );
            }
            let later: Vec<_> = activity
                .into_iter()
                .flatten()
                .rev()
                .take(3)
                .filter(|e| source_order(e) > last_order)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|e| excerpt(e, context, snapshot))
                .collect();
            bundle.extend(later.clone());
            // Deduplicate by occurrence, retaining row identity as well as line.
            let mut seen = BTreeSet::new();
            bundle.retain(|e| seen.insert(e["occurrence"].to_string()));
            let support: Vec<_> = bundle
                .iter()
                .map(|e| e["occurrence"].clone())
                .chain(steps.iter().flat_map(|step| {
                    step["join_samples"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .flat_map(|j| [j["source"].clone(), j["target"].clone()])
                }))
                .collect();
            let details = json!({"supporting_occurrences":support,"event_chain":{"id":chain.id,"title":chain.title,"root":occurrence(root,context,snapshot),"root_event_name":field(root,"event.name"),"identity":identities(root,&root_fields),"scope_fields":chain.scope_fields,"context_identity":identities(root,&chain.context_fields),"root_occurrences":root_entries.len(),"ambiguity":if root_entries.len()>1{"reused_root_identity"}else{"none"},"steps":steps,"later_activity":later,"later_activity_observed_count":activity_count,"omitted_later_activity":activity_count.map(|n|n.saturating_sub(later.len())),"completeness":if complete && node_completeness.values().all(|v|*v) {"complete"}else{"partial"},"evidence_policy":"First and last occurrence per role, up to two additional distinct outcomes and two ambiguous candidates, plus last three selected same-context records after the last linked event. Terminal and contrary roles must be explicitly configured; absence is not proof of failure. Outcomes at different levels are independent; source order implies no causality."}});
            findings.push(fact(format!("{scope}-chain-{chain_index}-{root_index}"),scope,"observation","Connected event chain from explicit profile roles and exact identity/scope joins; outcome levels remain independent.",bundle,details));
        }
        if let Some(root) = missing_roots.first() {
            let failed: Vec<_> = root_fields
                .iter()
                .map(|field| {
                    let count = missing_roots
                        .iter()
                        .filter(|entry| key(entry, std::slice::from_ref(field)).is_none())
                        .count();
                    format!("{field}: {count} roots missing a usable key")
                })
                .collect();
            findings.push(fact(format!("{scope}-chain-{chain_index}-missing"),scope,"unknown","Some chain roots lack declared identity or scope; no relationship is guessed.",vec![excerpt(root,context,snapshot)],json!({"gap_classification":"identity_or_scope_unavailable","reason":format!("{} root occurrences excluded; selectors [{}]",missing_roots.len(), failed.join("; ")),"supporting_occurrences":[occurrence(root,context,snapshot)]})));
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn calculate_stream(
    policy: &InvestigationPolicy,
    config: &AnalyzerConfig,
    entries: &super::source_store::Entries,
    selectors: &[Selector],
    scope: &str,
    context: &evidence::Context,
    snapshot: &Value,
    input_complete: bool,
    budget: &mut Budget,
    findings: &mut dyn super::finding_store::FindingSink,
) -> super::Result<()> {
    let result = calculate_stream_inner(
        policy,
        config,
        entries,
        selectors,
        scope,
        context,
        snapshot,
        input_complete,
        budget,
        findings,
    );
    if budget.halted { Ok(()) } else { result }
}

#[allow(clippy::too_many_arguments)]
fn calculate_stream_inner(
    policy: &InvestigationPolicy,
    config: &AnalyzerConfig,
    entries: &super::source_store::Entries,
    selectors: &[Selector],
    scope: &str,
    context: &evidence::Context,
    snapshot: &Value,
    input_complete: bool,
    budget: &mut Budget,
    findings: &mut dyn super::finding_store::FindingSink,
) -> super::Result<()> {
    use super::grouped::Groups;
    if policy.chains.is_empty() {
        return Ok(());
    }
    let matchers: BTreeMap<_, _> = policy
        .roles
        .iter()
        .map(|r| (r.id.as_str(), Matcher::new(r)))
        .collect();
    let mut roles = Groups::new(entries)?;
    let mut role_counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut diagnostics: BTreeMap<&str, Vec<usize>> = policy
        .roles
        .iter()
        .map(|role| (role.id.as_str(), vec![0; role.select.len() + 3]))
        .collect();
    let mut unresolved = false;
    for (ordinal, entry) in entries.iter()?.enumerate() {
        let entry = entry?;
        if !budget.checkpoint(
            "calculation",
            policy.roles.len() as u64 + entry.message.len() as u64,
        ) {
            return Ok(());
        }
        if !selected(&entry, selectors, config) {
            continue;
        }
        unresolved |= matches!(
            entry.classification,
            Some(ClassifiedRecord::Conflict { .. } | ClassifiedRecord::Invalid { .. })
        );
        if !matches!(entry.classification, Some(ClassifiedRecord::Event { .. })) {
            continue;
        }
        for (id, matcher) in &matchers {
            let counts = diagnostics.get_mut(id).unwrap();
            counts[0] += 1;
            if rules(&entry)
                .iter()
                .any(|rule| matcher.role.rule_ids.contains(rule))
            {
                counts[1] += 1;
                for (index, condition) in matcher.role.select.iter().enumerate() {
                    if field(&entry, &condition.field).as_ref() != Some(&condition.equals) {
                        counts[index + 2] += 1;
                    }
                }
                if matcher
                    .pattern
                    .as_ref()
                    .is_some_and(|pattern| !pattern.is_match(&entry.message))
                {
                    counts[matcher.role.select.len() + 2] += 1;
                }
            }
            if matcher.matches(&entry) {
                roles.insert(id, &[], ordinal)?;
                *role_counts.entry(id).or_default() += 1;
            }
        }
    }
    for (chain_index, chain) in policy.chains.iter().enumerate() {
        if role_counts
            .get(chain.root_role.as_str())
            .copied()
            .unwrap_or(0)
            == 0
        {
            let matcher = &matchers[chain.root_role.as_str()];
            let counts = &diagnostics[chain.root_role.as_str()];
            let failed: Vec<_> = matcher
                .role
                .select
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    format!(
                        "{} equals {}: {} rule-matched records failed",
                        c.field,
                        c.equals,
                        counts[i + 2]
                    )
                })
                .collect();
            findings.push(fact(format!("{scope}-chain-{chain_index}-unrecognized"),scope,"unknown","Configured chain found no matching roots; complete parsing does not establish relationship coverage.",Vec::new(),json!({"supporting_occurrences":[],"gap_classification":"recognition_unresolved","reason":format!("Chain {}; root role {}; {} selected classified events, {} matched rule IDs {:?}; selectors [{}]; message_pattern {:?}: {} rule-matched records failed; processed coverage {}. Counts are independent selector failures, not disjoint populations.",chain.id,chain.root_role,counts[0],counts[1],matcher.role.rule_ids,failed.join("; "),matcher.role.message_pattern,counts[matcher.role.select.len()+2],if input_complete{"complete"}else{"partial"})})))?;
        }
        let root_fields: Vec<_> = chain
            .identity_fields
            .iter()
            .chain(&chain.scope_fields)
            .cloned()
            .collect();
        let mut roots = Groups::new(entries)?;
        let mut missing_roots = 0usize;
        let mut missing_sample = None;
        let mut missing_fields = vec![0usize; root_fields.len()];
        for root in roles.entries(entries, &chain.root_role, &[]) {
            let (ordinal, root) = root?;
            if !budget.checkpoint("calculation", root_fields.len() as u64) {
                return Ok(());
            }
            if let Some(k) = key(&root, &root_fields) {
                roots.insert("roots", &k, ordinal)?;
            } else {
                missing_roots += 1;
                for (i, f) in root_fields.iter().enumerate() {
                    if key(&root, std::slice::from_ref(f)).is_none() {
                        missing_fields[i] += 1;
                    }
                }
                if missing_sample.is_none() {
                    missing_sample = Some(root);
                }
            }
        }
        let mut indices = Groups::new(entries)?;
        let mut step_indices = Vec::new();
        for (step_index, step) in chain.steps.iter().enumerate() {
            let fields: Vec<_> = step
                .join_fields
                .iter()
                .chain(&chain.scope_fields)
                .cloned()
                .collect();
            let collection = format!("step-{step_index}");
            let mut missing = 0usize;
            for target in roles.entries(entries, &step.role, &[]) {
                let (ordinal, target) = target?;
                if !budget.checkpoint("calculation", fields.len() as u64) {
                    return Ok(());
                }
                if let Some(k) = key(&target, &fields) {
                    indices.insert(&collection, &k, ordinal)?;
                } else {
                    missing += 1;
                }
            }
            step_indices.push((fields, collection, missing));
        }
        for (ordinal, entry) in entries.iter()?.enumerate() {
            let entry = entry?;
            if !selected(&entry, selectors, config) {
                continue;
            }
            if !budget.checkpoint("calculation", chain.context_fields.len() as u64) {
                return Ok(());
            }
            if let Some(k) = key(&entry, &chain.context_fields) {
                indices.insert_ranked("later", &k, ordinal)?;
            }
        }
        for (root_index, root_key) in roots.keys("roots", true).enumerate() {
            if !budget.checkpoint("calculation", 1) {
                return Ok(());
            }
            let root_key = root_key?;
            let root_count = roots.count("roots", &root_key, budget)?;
            let root_ordinal = roots
                .ordinals("roots", &root_key)
                .next()
                .transpose()?
                .ok_or("empty root population")?;
            let root = entries.get(root_ordinal)?;
            let mut bundle = Vec::new();
            let mut nodes = Groups::new(entries)?;
            let mut root_last = root_ordinal;
            for ordinal in roots.ordinals("roots", &root_key) {
                super::grouped::check(budget)?;
                let ordinal = ordinal?;
                nodes.node("$root", ordinal)?;
                root_last = ordinal;
            }
            bundle.push(excerpt(&root, context, snapshot));
            if root_last != root_ordinal {
                bundle.push(excerpt(&entries.get(root_last)?, context, snapshot));
            }
            let context_key = key(&root, &chain.context_fields);
            let complete = input_complete
                && !unresolved
                && missing_roots == 0
                && root_count == 1
                && context_key.is_some();
            let mut completeness = BTreeMap::from([("$root", complete)]);
            let mut node_counts = BTreeMap::from([("$root", root_count)]);
            let mut steps = Vec::new();
            let mut last_order = root_last;
            for (step, (fields, collection, missing)) in chain.steps.iter().zip(&step_indices) {
                let mut source_keys = Groups::new(entries)?;
                let mut missing_source = 0usize;
                for ordinal in nodes.node_ordinals(&step.from) {
                    let ordinal = ordinal?;
                    let source = entries.get(ordinal)?;
                    if !budget.checkpoint("calculation", fields.len() as u64) {
                        return Ok(());
                    }
                    if let Some(k) = key(&source, fields) {
                        source_keys.insert("sources", &k, ordinal)?;
                    } else {
                        missing_source += 1;
                    }
                }
                let mut has_source_key = false;
                let mut target_count = 0usize;
                let mut ambiguous = 0usize;
                let mut ambiguous_samples = Vec::new();
                for k in source_keys.keys("sources", false) {
                    super::grouped::check(budget)?;
                    let k = k?;
                    has_source_key = true;
                    let from = source_keys.count("sources", &k, budget)?;
                    let to = indices.count(collection, &k, budget)?;
                    if to == 0 {
                        continue;
                    }
                    let mut supported = cardinality_supported(step.cardinality, from, to);
                    if supported {
                        // Ordinals identify the same physical/expanded occurrence across replays.
                        let mut source_rows = source_keys.ordinals("sources", &k);
                        let mut source = source_rows.next().transpose()?;
                        for target in indices.ordinals(collection, &k) {
                            super::grouped::check(budget)?;
                            let target = target?;
                            while source.is_some_and(|ordinal| ordinal < target) {
                                super::grouped::check(budget)?;
                                source = source_rows.next().transpose()?;
                            }
                            if source == Some(target) {
                                supported = false;
                                break;
                            }
                        }
                    }
                    if !supported {
                        ambiguous += to;
                        let first = indices.ordinals(collection, &k).next().transpose()?;
                        let last = super::grouped::last(indices.ordinals(collection, &k), budget)?;
                        for ordinal in first
                            .into_iter()
                            .chain(last.filter(|last| Some(*last) != first))
                        {
                            if ambiguous_samples.len() < 2 {
                                ambiguous_samples.push(excerpt(
                                    &entries.get(ordinal)?,
                                    context,
                                    snapshot,
                                ));
                            }
                        }
                        continue;
                    }
                    if !budget.checkpoint("calculation", to as u64) {
                        return Ok(());
                    }
                    let first_source = source_keys
                        .ordinals("sources", &k)
                        .next()
                        .transpose()?
                        .ok_or("empty chain source group")?;
                    for ordinal in indices.ordinals(collection, &k) {
                        super::grouped::check(budget)?;
                        let ordinal = ordinal?;
                        nodes.node(&step.id, ordinal)?;
                        nodes.put(
                            &format!("{}:joins", step.id),
                            &ordinal.to_string(),
                            &(first_source, from, to),
                        )?;
                        target_count += 1;
                        last_order = last_order.max(ordinal);
                    }
                }
                let mut sampled = Vec::<(usize, LogEntry)>::new();
                let first = nodes.node_ordinals(&step.id).next().transpose()?;
                let last = super::grouped::last(nodes.node_ordinals(&step.id), budget)?;
                for ordinal in first
                    .into_iter()
                    .chain(last.filter(|last| Some(*last) != first))
                {
                    sampled.push((ordinal, entries.get(ordinal)?));
                }
                if let Some(selector) = &step.outcome_field {
                    let mut values: BTreeSet<_> = sampled
                        .iter()
                        .map(|(_, e)| field(e, selector).map(|v| v.to_string()))
                        .collect();
                    for ordinal in nodes.node_ordinals(&step.id) {
                        super::grouped::check(budget)?;
                        if sampled.len() >= 4 {
                            break;
                        }
                        let ordinal = ordinal?;
                        let entry = entries.get(ordinal)?;
                        if values.insert(field(&entry, selector).map(|v| v.to_string())) {
                            sampled.push((ordinal, entry));
                        }
                    }
                }
                let mut joins = Vec::new();
                for (ordinal, target) in &sampled {
                    let (source_ordinal, source_count, target_count): (usize, usize, usize) = nodes
                        .get(&format!("{}:joins", step.id), &ordinal.to_string())?
                        .ok_or("missing chain join")?;
                    let source = entries.get(source_ordinal)?;
                    let mut interval = endpoint_interval(
                        &source,
                        target,
                        source_count == 1 && target_count == 1 && root_count == 1,
                    );
                    interval["source"] = occurrence(&source, context, snapshot);
                    interval["target"] = occurrence(target, context, snapshot);
                    joins.push(json!({"source":occurrence(&source,context,snapshot),"target":occurrence(target,context,snapshot),"identity":identities(&source,fields),"source_count":source_count,"target_count":target_count,"endpoint_interval":interval}));
                }
                let witnesses: Vec<_> = sampled
                    .iter()
                    .map(|(_, e)| excerpt(e, context, snapshot))
                    .collect();
                bundle.extend(witnesses.clone());
                bundle.extend(ambiguous_samples.clone());
                let outcomes:Vec<_>=step.outcome_field.as_ref().map(|f|sampled.iter().map(|(_,e)|json!({"level":step.outcome_level,"field":f,"value":field(e,f),"occurrence":occurrence(e,context,snapshot)})).collect()).unwrap_or_default();
                let sources = node_counts[step.from.as_str()];
                let step_complete = completeness[step.from.as_str()]
                    && *missing == 0
                    && missing_source == 0
                    && ambiguous == 0
                    && sources > 0;
                let role = matchers[step.role.as_str()].role;
                let counts = &diagnostics[step.role.as_str()];
                let recognition = json!({"role":step.role,"rule_ids":role.rule_ids,"rule_matched_records":counts[1],"role_matched_records":role_counts.get(step.role.as_str()).copied().unwrap_or(0),"population":"selected_classified_records_in_this_input","failed_selectors":role.select.iter().enumerate().map(|(i,c)|json!({"field":c.field,"equals":c.equals,"failed_records":counts[i+2]})).collect::<Vec<_>>(),"message_pattern_failed_records":counts[role.select.len()+2]});
                steps.push(json!({"recognition":recognition,"id":step.id,"label":step.label,"event_type":step.event_type,"from":step.from,"observed_count":target_count+ambiguous,"linked_count":target_count,"status":if ambiguous>0{"ambiguous"}else if sources==0||!has_source_key&&missing_source>0{"unavailable"}else if target_count==0{"not_observed"}else{"observed"},"count_completeness":if step_complete{"complete"}else{"partial"},"join_fields":fields,"cardinality":step.cardinality,"join_samples":joins,"ambiguity":{"excluded_target_occurrences":ambiguous,"role_occurrences_missing_identity":missing,"source_occurrences_missing_identity":missing_source},"evidence":witnesses,"ambiguous_evidence":ambiguous_samples,"omitted_occurrences":target_count.saturating_sub(sampled.len()),"outcomes":outcomes,"omitted_outcomes":if step.outcome_field.is_some(){target_count.saturating_sub(sampled.len())}else{0}}));
                completeness.insert(&step.id, step_complete);
                node_counts.insert(&step.id, target_count);
            }
            super::grouped::check(budget)?;
            let suffix = context_key
                .map(|key| indices.suffix("later", &key, last_order))
                .transpose()?
                .flatten();
            let activity_count = suffix.as_ref().map(|(count, _)| *count);
            let later: Vec<_> = suffix
                .into_iter()
                .flat_map(|(_, tail)| tail)
                .map(|ordinal| {
                    entries
                        .get(ordinal)
                        .map(|entry| excerpt(&entry, context, snapshot))
                })
                .collect::<super::Result<_>>()?;
            bundle.extend(later.clone());
            let mut seen = BTreeSet::new();
            bundle.retain(|e| seen.insert(e["occurrence"].to_string()));
            let support: Vec<_> = bundle
                .iter()
                .map(|e| e["occurrence"].clone())
                .chain(steps.iter().flat_map(|step| {
                    step["join_samples"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .flat_map(|j| [j["source"].clone(), j["target"].clone()])
                }))
                .collect();
            let details = json!({"supporting_occurrences":support,"event_chain":{"id":chain.id,"title":chain.title,"root":occurrence(&root,context,snapshot),"root_event_name":field(&root,"event.name"),"identity":identities(&root,&root_fields),"scope_fields":chain.scope_fields,"context_identity":identities(&root,&chain.context_fields),"root_occurrences":root_count,"ambiguity":if root_count>1{"reused_root_identity"}else{"none"},"steps":steps,"later_activity":later,"later_activity_observed_count":activity_count,"omitted_later_activity":activity_count.map(|n|n.saturating_sub(later.len())),"completeness":if complete&&completeness.values().all(|v|*v){"complete"}else{"partial"},"evidence_policy":"First and last occurrence per role, up to two additional distinct outcomes and two ambiguous candidates, plus last three selected same-context records after the last linked event. Terminal and contrary roles must be explicitly configured; absence is not proof of failure. Outcomes at different levels are independent; source order implies no causality."}});
            findings.push(fact(format!("{scope}-chain-{chain_index}-{root_index}"),scope,"observation","Connected event chain from explicit profile roles and exact identity/scope joins; outcome levels remain independent.",bundle,details))?;
        }
        if let Some(root) = missing_sample {
            let failed: Vec<_> = root_fields
                .iter()
                .zip(missing_fields)
                .map(|(field, count)| format!("{field}: {count} roots missing a usable key"))
                .collect();
            findings.push(fact(format!("{scope}-chain-{chain_index}-missing"),scope,"unknown","Some chain roots lack declared identity or scope; no relationship is guessed.",vec![excerpt(&root,context,snapshot)],json!({"gap_classification":"identity_or_scope_unavailable","reason":format!("{missing_roots} root occurrences excluded; selectors [{}]",failed.join("; ")),"supporting_occurrences":[occurrence(&root,context,snapshot)]})))?;
        }
    }
    Ok(())
}
