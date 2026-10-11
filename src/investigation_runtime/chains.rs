//! Explicit role graphs. Source order is context, never a join key or proof of completion.
use super::{
    Selector,
    findings::{excerpt, fact, occurrence, rules},
    selected,
};
use crate::{
    config::AnalyzerConfig,
    event_rules::ClassifiedRecord,
    evidence,
    investigation_policy::{Cardinality, InvestigationPolicy, Role},
    parser::LogEntry,
    processing::Budget,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn field(entry: &LogEntry, selector: &str) -> Option<Value> {
    if let Some(prefix) = selector.strip_prefix("component_id.prefix:") {
        if entry.component_id.len() > 4096 {
            return None;
        }
        let mut values = BTreeSet::new();
        for branch in entry.component_id.split(" & ") {
            let segments: Vec<_> = branch.split('/').collect();
            let position = segments
                .iter()
                .position(|segment| segment.starts_with(prefix))?;
            values.insert(segments[..=position].join("/"));
        }
        return (values.len() == 1).then(|| json!(values.first().unwrap()));
    }
    if let Some(key) = selector.strip_prefix("event.") {
        let Some(ClassifiedRecord::Event { semantics, .. }) = &entry.classification else {
            return None;
        };
        return match key {
            "name" => Some(json!(semantics.name)),
            "kind" => serde_json::to_value(semantics.kind).ok(),
            "phase" => semantics.phase.and_then(|v| serde_json::to_value(v).ok()),
            "outcome" => semantics.outcome.and_then(|v| serde_json::to_value(v).ok()),
            "correlation_id" => semantics.correlation_id.as_ref().map(|v| json!(v)),
            "direction" => semantics.direction.as_ref().map(|v| json!(v)),
            "endpoint" => semantics.endpoint.as_ref().map(|v| json!(v)),
            "end_expected" => Some(json!(semantics.end_expected)),
            _ => None,
        };
    }
    if let Some(key) = selector.strip_prefix("payload.") {
        return crate::extract::extract_payload_field(entry, key)
            .filter(|v| !v.is_array() && !v.is_object() && v.to_string().len() <= 4096)
            .cloned();
    }
    let value = match selector {
        "component_id" => Some(entry.component_id.as_str()),
        "component" => Some(entry.component.as_str()),
        "level" => Some(entry.level.as_str()),
        _ => entry.structured_field(selector),
    };
    value.filter(|v| v.len() <= 4096).map(|v| json!(v))
}
fn key(entry: &LogEntry, fields: &[String]) -> Option<Vec<String>> {
    fields
        .iter()
        .map(|selector| {
            let value = field(entry, selector)?;
            if value.is_null()
                || value.is_array()
                || value.is_object()
                || value == ""
                || value.to_string().len() > 4096
            {
                return None;
            }
            Some(value.to_string())
        })
        .collect()
}
pub(super) struct Matcher<'a> {
    role: &'a Role,
    pattern: Option<regex::Regex>,
}
impl<'a> Matcher<'a> {
    pub(super) fn new(role: &'a Role) -> Self {
        Self {
            role,
            pattern: role.message_pattern.as_ref().map(|p| {
                regex::RegexBuilder::new(p)
                    .size_limit(1_048_576)
                    .build()
                    .expect("validated role pattern")
            }),
        }
    }
    pub(super) fn matches(&self, entry: &LogEntry) -> bool {
        rules(entry)
            .iter()
            .any(|id| self.role.rule_ids.contains(id))
            && self
                .role
                .select
                .iter()
                .all(|c| field(entry, &c.field).as_ref() == Some(&c.equals))
            && self
                .pattern
                .as_ref()
                .is_none_or(|pattern| pattern.is_match(&entry.message))
    }
}
fn identities(entry: &LogEntry, fields: &[String]) -> Vec<Value> {
    fields
        .iter()
        .map(|f| json!({"field":f,"value":field(entry,f)}))
        .collect()
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
#[allow(clippy::too_many_arguments)]
pub(super) fn calculate(
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
            if matcher.matches(entry) {
                if !budget.reserve("calculation", 32) {
                    return;
                }
                roles.entry(id).or_default().push(entry);
            }
        }
    }
    for (chain_index, chain) in policy.chains.iter().enumerate() {
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
        ordered_roots.sort_by_key(|events| {
            (
                events[0].source_line_number,
                events[0].source_row_path.as_deref(),
            )
        });
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
            let complete = input_complete
                && !unresolved
                && missing_roots.is_empty()
                && root_entries.len() == 1;
            let mut node_completeness = BTreeMap::from([("$root", complete)]);
            for (step, (fields, index, missing)) in chain.steps.iter().zip(&indices) {
                let sources = &selected_nodes[step.from.as_str()];
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
                    let supported = match step.cardinality {
                        Cardinality::OneToOne => from.len() == 1 && to.len() == 1,
                        Cardinality::OneToMany => from.len() == 1,
                        Cardinality::ManyToOne => to.len() == 1,
                    };
                    if !supported || to.iter().any(|t| from.iter().any(|s| std::ptr::eq(*s, *t))) {
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
                targets.sort_by_key(|e| (e.source_line_number, e.source_row_path.as_deref()));
                let sampled = step_samples(&targets, step.outcome_field.as_deref());
                let joins: Vec<_> = sampled.iter().map(|target| {
                    let (source,source_count,target_count)=joins_by_target[&(target.source_line_number,target.source_row_path.as_deref())];
                    json!({"source":occurrence(source,context,snapshot),"target":occurrence(target,context,snapshot),"identity":identities(source,fields),"source_count":source_count,"target_count":target_count})
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
                steps.push(json!({"id":step.id,"label":step.label,"event_type":step.event_type,"from":step.from,"observed_count":targets.len()+ambiguous,"linked_count":targets.len(),"status":if ambiguous>0{"ambiguous"}else if sources.is_empty() || missing_source_identity {"unavailable"}else if targets.is_empty(){"not_observed"}else{"observed"},"count_completeness":if step_complete{"complete"}else{"partial"},"join_fields":fields,"cardinality":step.cardinality,"join_samples":joins,"ambiguity":{"excluded_target_occurrences":ambiguous,"role_occurrences_missing_identity":missing,"source_occurrences_missing_identity":missing_source},"evidence":witnesses,"ambiguous_evidence":ambiguous_samples,"omitted_occurrences":targets.len().saturating_sub(sampled.len()),"outcomes":outcomes,"omitted_outcomes":if step.outcome_field.is_some(){targets.len().saturating_sub(sampled.len())}else{0}}));
                node_completeness.insert(step.id.as_str(), step_complete);
                selected_nodes.insert(step.id.as_str(), targets);
            }
            let last_line = selected_nodes
                .values()
                .flatten()
                .map(|e| e.source_line_number)
                .max()
                .unwrap_or(root.source_line_number);
            let activity = key(root, &chain.context_fields).and_then(|k| later_index.get(&k));
            let activity_count = activity.map(|events| {
                events
                    .len()
                    .saturating_sub(events.partition_point(|e| e.source_line_number <= last_line))
            });
            let later: Vec<_> = activity
                .into_iter()
                .flatten()
                .rev()
                .take(3)
                .filter(|e| e.source_line_number > last_line)
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
            let details = json!({"supporting_occurrences":support,"event_chain":{"id":chain.id,"title":chain.title,"root":occurrence(root,context,snapshot),"identity":identities(root,&root_fields),"scope_fields":chain.scope_fields,"context_identity":identities(root,&chain.context_fields),"root_occurrences":root_entries.len(),"ambiguity":if root_entries.len()>1{"reused_root_identity"}else{"none"},"steps":steps,"later_activity":later,"later_activity_observed_count":activity_count,"omitted_later_activity":activity_count.map(|n|n.saturating_sub(later.len())),"completeness":if complete && node_completeness.values().all(|v|*v) {"complete"}else{"partial"},"evidence_policy":"First and last occurrence per role, up to two additional distinct outcomes and two ambiguous candidates, plus last three selected same-context records after the last linked event. Terminal and contrary roles must be explicitly configured; absence is not proof of failure. Outcomes at different levels are independent; source order implies no causality."}});
            findings.push(fact(format!("{scope}-chain-{chain_index}-{root_index}"),scope,"observation","Connected event chain from explicit profile roles and exact identity/scope joins; outcome levels remain independent.",bundle,details));
        }
        if let Some(root) = missing_roots.first() {
            findings.push(fact(format!("{scope}-chain-{chain_index}-missing"),scope,"unknown","Some chain roots lack declared identity or scope; no relationship is guessed.",vec![excerpt(root,context,snapshot)],json!({"reason":format!("{} root occurrences excluded",missing_roots.len()),"supporting_occurrences":[occurrence(root,context,snapshot)]})));
        }
    }
}
