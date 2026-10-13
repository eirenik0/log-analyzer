use super::{
    Selector,
    findings::{excerpt, fact, occurrence, population},
    selected,
};
use crate::{
    evidence,
    investigation_policy::{Grouping, InvestigationPolicy},
    parser::LogEntry,
    processing::Budget,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub(super) fn scalar_identity(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}
fn field(entry: &LogEntry, selector: &str) -> Option<String> {
    if let Some(field) = selector.strip_prefix("payload.") {
        scalar_identity(crate::extract::extract_payload_field(entry, field)?)
    } else {
        entry.structured_field(selector).map(str::to_owned)
    }
}
fn identity(entry: &LogEntry, fields: &[String]) -> Option<Vec<String>> {
    fields
        .iter()
        .map(|selector| field(entry, selector).filter(|value| !value.is_empty()))
        .collect()
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn calculate(
    policy: &InvestigationPolicy,
    config: &crate::config::AnalyzerConfig,
    entries: &[LogEntry],
    selectors: &[Selector],
    scope: &str,
    context: &evidence::Context,
    snapshot: &Value,
    budget: &mut Budget,
    findings: &mut Vec<Value>,
    populations: &mut Vec<Value>,
    memberships: &mut dyn super::construction::MemberSink,
    mut observer: Option<&mut dyn crate::relationship_rules::Observer>,
) -> super::Result<()> {
    let roles: BTreeMap<_, _> = policy
        .roles
        .iter()
        .map(|role| (role.id.as_str(), super::chains::Matcher::new(role)))
        .collect();
    for (index, declaration) in policy.populations.iter().enumerate() {
        let mut groups: BTreeMap<Vec<String>, Vec<&LogEntry>> = BTreeMap::new();
        let mut missing = Vec::new();
        let mut unresolved = Vec::new();
        for entry in entries
            .iter()
            .filter(|entry| selected(entry, selectors, config))
        {
            if !budget.checkpoint(
                "calculation",
                (declaration.roles.len() + declaration.identity_fields.len()) as u64,
            ) {
                return Ok(());
            }
            if !declaration
                .roles
                .iter()
                .any(|id| roles[id.as_str()].matches(entry))
            {
                continue;
            }
            if !matches!(
                entry.classification,
                Some(crate::event_rules::ClassifiedRecord::Event { .. })
            ) {
                unresolved.push(entry);
                continue;
            }
            let Some(mut key) = identity(entry, &declaration.identity_fields) else {
                missing.push(entry);
                continue;
            };
            let Some(effective_scope) = crate::perf_analyzer::correlation_scope(entry, config)
            else {
                missing.push(entry);
                continue;
            };
            key.push(serde_json::to_string(&effective_scope).unwrap());
            if matches!(declaration.grouping, Grouping::Occurrence) {
                key.push(format!(
                    "{}:{}",
                    entry.source_line_number,
                    entry.source_row_path.as_deref().unwrap_or("")
                ));
            }
            if !budget.reserve("calculation", 4096) {
                return Ok(());
            }
            groups.entry(key).or_default().push(entry);
        }
        if let Some(entry) = unresolved.first() {
            findings.push(fact(format!("{scope}-policy-{index}-unavailable"),scope,"unknown","Some declared role matches have conflicting or invalid semantics.",vec![excerpt(entry,context,snapshot)],json!({"reason":format!("{} role-matching source records cannot establish membership; the number of excluded domain entities is unknown.", unresolved.len()),"supporting_occurrences":[occurrence(entry,context,snapshot)]})));
            if groups.is_empty() {
                continue;
            }
        }
        let members=groups.iter().enumerate().map(|(member_index,(key,entries))| {
            json!({"kind":match declaration.entity{crate::investigation_policy::Entity::Events=>"event",crate::investigation_policy::Entity::Attempts=>"attempt",crate::investigation_policy::Entity::Resources=>"resource"},"id":format!("{scope}-policy-{index}-{member_index}"),"identity":declaration.identity_fields.iter().chain(std::iter::once(&"$correlation_scope".to_owned())).zip(key).map(|(field,value)|json!({"field":field,"value":value})).collect::<Vec<_>>(),"source_occurrences":entries.iter().map(|entry|occurrence(entry,context,snapshot)).collect::<Vec<_>>(),"measurement_ids":[]})
        });
        let rule_ids = declaration
            .roles
            .iter()
            .flat_map(|id| {
                policy
                    .roles
                    .iter()
                    .find(|role| role.id == *id)
                    .unwrap()
                    .rule_ids
                    .clone()
            })
            .collect();
        population(
            scope,
            &format!("policy-{index}"),
            declaration.entity.label(),
            "Population grouped only by explicit profile role, identity fields and grouping policy; repeated starts are separate unless identity grouping is expressly declared.",
            declaration
                .identity_fields
                .iter()
                .cloned()
                .chain(std::iter::once("$correlation_scope".into()))
                .collect(),
            rule_ids,
            members,
            findings,
            populations,
            memberships,
        )?;
        populations.last_mut().unwrap()["exclusions"] = json!([{"reason":"Missing declared identity fields; excluded from this population, never guessed","count":missing.len()}]);
        if !unresolved.is_empty() {
            let population = populations.last_mut().unwrap();
            population["completeness"] = json!("partial");
            population["exclusions"].as_array_mut().unwrap().push(json!({"reason":"Conflicting or invalid declared role matches; domain entity membership is unavailable","count":null}));
        }
        if !missing.is_empty() {
            findings.push(fact(format!("{scope}-policy-{index}-missing"),scope,"unknown","Some role-matching events lack declared grouping identity.",vec![excerpt(missing[0],context,snapshot)],json!({"reason":"Missing declared identity fields prevent grouping; population exclusions give the exact count.","supporting_occurrences":[occurrence(missing[0],context,snapshot)]})));
        }
    }
    for (index, declaration) in policy.relationships.iter().enumerate() {
        let mut fields = declaration.join_fields.clone();
        for selector in &declaration.required_scope_fields {
            if !fields.contains(selector) {
                fields.push(selector.clone());
            }
        }
        let mut sources: BTreeMap<Vec<String>, Vec<&LogEntry>> = BTreeMap::new();
        let mut targets: BTreeMap<Vec<String>, Vec<&LogEntry>> = BTreeMap::new();
        let mut missing = 0usize;
        let mut unresolved = Vec::new();
        for entry in entries
            .iter()
            .filter(|entry| selected(entry, selectors, config))
        {
            if !budget.checkpoint("calculation", fields.len() as u64 + 2) {
                return Ok(());
            }
            let source = roles[declaration.source_role.as_str()].matches(entry);
            let target = roles[declaration.target_role.as_str()].matches(entry);
            if !source && !target {
                continue;
            }
            if !matches!(
                entry.classification,
                Some(crate::event_rules::ClassifiedRecord::Event { .. })
            ) {
                unresolved.push(entry);
                continue;
            }
            let Some(mut key) = crate::relationship_rules::key(entry, &fields) else {
                missing += 1;
                continue;
            };
            let Some(effective_scope) = crate::perf_analyzer::correlation_scope(entry, config)
            else {
                missing += 1;
                continue;
            };
            key.push(serde_json::to_string(&effective_scope).unwrap());
            if !budget.reserve("calculation", 8192) {
                return Ok(());
            }
            if source {
                sources.entry(key.clone()).or_default().push(entry);
            }
            if target {
                targets.entry(key).or_default().push(entry);
            }
        }
        if let Some(entry) = unresolved.first() {
            findings.push(fact(format!("{scope}-relationship-{index}-unavailable"),scope,"unknown","Unresolved declared role matches prevent establishing relationship cardinality.",vec![excerpt(entry,context,snapshot)],json!({"reason":format!("{} role-matching source records have conflicting or invalid semantics; no unique scoped joins are asserted.", unresolved.len()),"supporting_occurrences":[occurrence(entry,context,snapshot)]})));
            continue;
        }
        let mut joins = 0usize;
        let mut ambiguous = 0usize;
        for (key, from) in sources {
            let Some(to) = targets.get(&key) else {
                missing += from.len();
                continue;
            };
            let supported = crate::relationship_rules::cardinality_supported(
                declaration.cardinality,
                from.len(),
                to.len(),
            );
            if let Some(observer) = observer.as_deref_mut() {
                if !budget.checkpoint("calculation", observer.work_units()) {
                    return Ok(());
                }
                observer.relationship_group(declaration, &from, to, supported);
            }
            if !supported {
                ambiguous += from.len() + to.len();
                continue;
            }
            for source in &from {
                for target in to {
                    if std::ptr::eq(*source, *target) {
                        ambiguous += 1;
                        continue;
                    }
                    if !budget.checkpoint("calculation", 1)
                        || !budget.reserve(
                            "calculation",
                            (source.raw_logline.len() + target.raw_logline.len() + 8192) as u64 * 8,
                        )
                    {
                        return Ok(());
                    }
                    findings.push(fact(format!("{scope}-relationship-{index}-{joins}"),scope,"observation","Explicit role and exact join/scope fields support this source-target relationship; no causal or elapsed-time claim is made.",vec![excerpt(source,context,snapshot),excerpt(target,context,snapshot)],json!({"supporting_occurrences":[occurrence(source,context,snapshot),occurrence(target,context,snapshot)]})));
                    joins += 1;
                }
            }
        }
        if missing > 0 || ambiguous > 0 || joins == 0 {
            findings.push(fact(format!("{scope}-relationship-{index}-unavailable"),scope,"unknown","Some declared relationships cannot be established from the observed evidence.",Vec::new(),json!({"reason":format!("Declared scoped join: {joins} supported relationships; {missing} missing identities/targets; {ambiguous} ambiguous or self-joining occurrences. These diagnostics are not a distinct-resource or causal count."),"supporting_occurrences":[]})));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn calculate_stream(
    policy: &InvestigationPolicy,
    config: &crate::config::AnalyzerConfig,
    entries: &super::source_store::Entries,
    selectors: &[Selector],
    scope: &str,
    context: &evidence::Context,
    snapshot: &Value,
    budget: &mut Budget,
    findings: &mut dyn super::finding_store::FindingSink,
    populations: &mut Vec<Value>,
    memberships: &mut dyn super::construction::MemberSink,
) -> super::Result<()> {
    let result = calculate_stream_inner(
        policy,
        config,
        entries,
        selectors,
        scope,
        context,
        snapshot,
        budget,
        findings,
        populations,
        memberships,
    );
    if budget.halted { Ok(()) } else { result }
}

/// CLI construction replays keyed state instead of retaining matching sources.
#[allow(clippy::too_many_arguments)]
fn calculate_stream_inner(
    policy: &InvestigationPolicy,
    config: &crate::config::AnalyzerConfig,
    entries: &super::source_store::Entries,
    selectors: &[Selector],
    scope: &str,
    context: &evidence::Context,
    snapshot: &Value,
    budget: &mut Budget,
    findings: &mut dyn super::finding_store::FindingSink,
    populations: &mut Vec<Value>,
    memberships: &mut dyn super::construction::MemberSink,
) -> super::Result<()> {
    use super::{findings::population_stream, grouped::Groups};
    let roles: BTreeMap<_, _> = policy
        .roles
        .iter()
        .map(|role| (role.id.as_str(), super::chains::Matcher::new(role)))
        .collect();
    for (index, declaration) in policy.populations.iter().enumerate() {
        let mut groups = Groups::new(entries)?;
        let mut missing = 0usize;
        let mut missing_sample = None;
        let mut unresolved = 0usize;
        let mut unresolved_sample = None;
        for (ordinal, entry) in entries.iter()?.enumerate() {
            let entry = entry?;
            if !selected(&entry, selectors, config) {
                continue;
            }
            if !budget.checkpoint(
                "calculation",
                (declaration.roles.len() + declaration.identity_fields.len()) as u64,
            ) {
                return Ok(());
            }
            if !declaration
                .roles
                .iter()
                .any(|id| roles[id.as_str()].matches(&entry))
            {
                continue;
            }
            if !matches!(
                entry.classification,
                Some(crate::event_rules::ClassifiedRecord::Event { .. })
            ) {
                unresolved += 1;
                if unresolved_sample.is_none() {
                    unresolved_sample = Some(entry);
                }
                continue;
            }
            let group = identity(&entry, &declaration.identity_fields)
                .zip(crate::perf_analyzer::correlation_scope(&entry, config));
            let Some((mut key, effective_scope)) = group else {
                missing += 1;
                if missing_sample.is_none() {
                    missing_sample = Some(entry);
                }
                continue;
            };
            key.push(serde_json::to_string(&effective_scope)?);
            if matches!(declaration.grouping, Grouping::Occurrence) {
                key.push(format!(
                    "{}:{}",
                    entry.source_line_number,
                    entry.source_row_path.as_deref().unwrap_or("")
                ));
            }
            groups.insert("members", &key, ordinal)?;
        }
        if let Some(entry) = &unresolved_sample {
            findings.push(fact(format!("{scope}-policy-{index}-unavailable"),scope,"unknown","Some declared role matches have conflicting or invalid semantics.",vec![excerpt(entry,context,snapshot)],json!({"reason":format!("{unresolved} role-matching source records cannot establish membership; the number of excluded domain entities is unknown."),"supporting_occurrences":[occurrence(entry,context,snapshot)]})))?;
            if groups.keys("members", false).next().transpose()?.is_none() {
                continue;
            }
        }
        let members=groups.keys("members",false).enumerate().map(|(member_index,key)| {
            let key=key?;
            super::grouped::check(budget)?;
            // A membership is an atomic v1 item. Bound it before allocating its
            // citation array; an oversized item fails rather than losing witnesses.
            let mut sources=Vec::new(); let mut bytes=0u64;
            for entry in groups.entries(entries,"members",&key) {
                super::grouped::check(budget)?;
                let (_,entry)=entry?; let source=occurrence(&entry,context,snapshot);
                bytes=bytes.saturating_add(serde_json::to_vec(&source)?.len() as u64+128);
                if bytes>entries.item_bytes.min(8*1024*1024) {return Err("policy membership item byte limit exceeded".into());}
                sources.push(source);
            }
            Ok(json!({"kind":match declaration.entity{crate::investigation_policy::Entity::Events=>"event",crate::investigation_policy::Entity::Attempts=>"attempt",crate::investigation_policy::Entity::Resources=>"resource"},"id":format!("{scope}-policy-{index}-{member_index}"),"identity":declaration.identity_fields.iter().map(String::as_str).chain(std::iter::once("$correlation_scope")).zip(&key).map(|(field,value)|json!({"field":field,"value":value})).collect::<Vec<_>>(),"source_occurrences":sources,"measurement_ids":[]}))
        });
        let rule_ids = declaration
            .roles
            .iter()
            .flat_map(|id| {
                policy
                    .roles
                    .iter()
                    .find(|role| role.id == *id)
                    .unwrap()
                    .rule_ids
                    .clone()
            })
            .collect();
        population_stream(
            scope,
            &format!("policy-{index}"),
            declaration.entity.label(),
            "Population grouped only by explicit profile role, identity fields and grouping policy; repeated starts are separate unless identity grouping is expressly declared.",
            declaration
                .identity_fields
                .iter()
                .cloned()
                .chain(std::iter::once("$correlation_scope".into()))
                .collect(),
            rule_ids,
            members,
            findings,
            populations,
            memberships,
        )?;
        populations.last_mut().unwrap()["exclusions"] = json!([{"reason":"Missing declared identity fields; excluded from this population, never guessed","count":missing}]);
        if unresolved > 0 {
            let population = populations.last_mut().unwrap();
            population["completeness"] = json!("partial");
            population["exclusions"].as_array_mut().unwrap().push(json!({"reason":"Conflicting or invalid declared role matches; domain entity membership is unavailable","count":null}));
        }
        if let Some(entry) = &missing_sample {
            findings.push(fact(format!("{scope}-policy-{index}-missing"),scope,"unknown","Some role-matching events lack declared grouping identity.",vec![excerpt(entry,context,snapshot)],json!({"reason":"Missing declared identity fields prevent grouping; population exclusions give the exact count.","supporting_occurrences":[occurrence(entry,context,snapshot)]})))?;
        }
    }
    for (index, declaration) in policy.relationships.iter().enumerate() {
        let mut fields = declaration.join_fields.clone();
        for selector in &declaration.required_scope_fields {
            if !fields.contains(selector) {
                fields.push(selector.clone());
            }
        }
        let mut groups = Groups::new(entries)?;
        let mut missing = 0usize;
        let mut unresolved = 0usize;
        let mut unresolved_sample = None;
        for (ordinal, entry) in entries.iter()?.enumerate() {
            let entry = entry?;
            if !selected(&entry, selectors, config) {
                continue;
            }
            if !budget.checkpoint("calculation", fields.len() as u64 + 2) {
                return Ok(());
            }
            let source = roles[declaration.source_role.as_str()].matches(&entry);
            let target = roles[declaration.target_role.as_str()].matches(&entry);
            if !source && !target {
                continue;
            }
            if !matches!(
                entry.classification,
                Some(crate::event_rules::ClassifiedRecord::Event { .. })
            ) {
                unresolved += 1;
                if unresolved_sample.is_none() {
                    unresolved_sample = Some(entry);
                }
                continue;
            }
            let Some((mut key, effective_scope)) = crate::relationship_rules::key(&entry, &fields)
                .zip(crate::perf_analyzer::correlation_scope(&entry, config))
            else {
                missing += 1;
                continue;
            };
            key.push(serde_json::to_string(&effective_scope)?);
            if source {
                groups.insert("sources", &key, ordinal)?;
            }
            if target {
                groups.insert("targets", &key, ordinal)?;
            }
        }
        if let Some(entry) = &unresolved_sample {
            findings.push(fact(format!("{scope}-relationship-{index}-unavailable"),scope,"unknown","Unresolved declared role matches prevent establishing relationship cardinality.",vec![excerpt(entry,context,snapshot)],json!({"reason":format!("{unresolved} role-matching source records have conflicting or invalid semantics; no unique scoped joins are asserted."),"supporting_occurrences":[occurrence(entry,context,snapshot)]})))?;
            continue;
        }
        let mut joins = 0usize;
        let mut ambiguous = 0usize;
        for key in groups.keys("sources", false) {
            super::grouped::check(budget)?;
            let key = key?;
            let from = groups.count("sources", &key, budget)?;
            let to = groups.count("targets", &key, budget)?;
            if to == 0 {
                missing += from;
                continue;
            }
            if !crate::relationship_rules::cardinality_supported(declaration.cardinality, from, to)
            {
                ambiguous += from + to;
                continue;
            }
            for source in groups.entries(entries, "sources", &key) {
                let (source_ordinal, source) = source?;
                for target in groups.entries(entries, "targets", &key) {
                    let (target_ordinal, target) = target?;
                    if source_ordinal == target_ordinal {
                        ambiguous += 1;
                        continue;
                    }
                    if !budget.checkpoint("calculation", 1) {
                        return Ok(());
                    }
                    findings.push(fact(format!("{scope}-relationship-{index}-{joins}"),scope,"observation","Explicit role and exact join/scope fields support this source-target relationship; no causal or elapsed-time claim is made.",vec![excerpt(&source,context,snapshot),excerpt(&target,context,snapshot)],json!({"supporting_occurrences":[occurrence(&source,context,snapshot),occurrence(&target,context,snapshot)]})))?;
                    joins += 1;
                }
            }
        }
        if missing > 0 || ambiguous > 0 || joins == 0 {
            findings.push(fact(format!("{scope}-relationship-{index}-unavailable"),scope,"unknown","Some declared relationships cannot be established from the observed evidence.",Vec::new(),json!({"reason":format!("Declared scoped join: {joins} supported relationships; {missing} missing identities/targets; {ambiguous} ambiguous or self-joining occurrences. These diagnostics are not a distinct-resource or causal count."),"supporting_occurrences":[]})))?;
        }
    }
    Ok(())
}
