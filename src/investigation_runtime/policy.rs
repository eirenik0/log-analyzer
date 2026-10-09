use super::{
    Selector,
    findings::{excerpt, fact, occurrence, population, rules},
    selected,
};
use crate::{
    evidence,
    investigation_policy::{Cardinality, Grouping, InvestigationPolicy, Role},
    parser::LogEntry,
    processing::Budget,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn has_role(entry: &LogEntry, role: &Role) -> bool {
    matches!(
        entry.classification,
        Some(crate::event_rules::ClassifiedRecord::Event { .. })
    ) && rules(entry).iter().any(|rule| role.rule_ids.contains(rule))
}
fn field(entry: &LogEntry, selector: &str) -> Option<String> {
    if let Some(field) = selector.strip_prefix("payload.") {
        match crate::extract::extract_payload_field(entry, field)? {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            Value::Bool(value) => Some(value.to_string()),
            Value::Null | Value::Array(_) | Value::Object(_) => None,
        }
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
pub(super) fn calculate(
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
    memberships: &mut Vec<Value>,
) {
    let roles: BTreeMap<_, _> = policy
        .roles
        .iter()
        .map(|role| (role.id.as_str(), role))
        .collect();
    for (index, declaration) in policy.populations.iter().enumerate() {
        let mut groups: BTreeMap<Vec<String>, Vec<&LogEntry>> = BTreeMap::new();
        let mut missing = Vec::new();
        for entry in entries
            .iter()
            .filter(|entry| selected(entry, selectors, config))
        {
            if !budget.checkpoint(
                "calculation",
                (declaration.roles.len() + declaration.identity_fields.len()) as u64,
            ) {
                return;
            }
            if !declaration
                .roles
                .iter()
                .any(|id| has_role(entry, roles[id.as_str()]))
            {
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
                return;
            }
            groups.entry(key).or_default().push(entry);
        }
        let members=groups.iter().enumerate().map(|(member_index,(key,entries))| {
            json!({"kind":match declaration.entity{crate::investigation_policy::Entity::Events=>"event",crate::investigation_policy::Entity::Attempts=>"attempt",crate::investigation_policy::Entity::Resources=>"resource"},"id":format!("{scope}-policy-{index}-{member_index}"),"identity":declaration.identity_fields.iter().chain(std::iter::once(&"$correlation_scope".to_owned())).zip(key).map(|(field,value)|json!({"field":field,"value":value})).collect::<Vec<_>>(),"source_occurrences":entries.iter().map(|entry|occurrence(entry,context,snapshot)).collect::<Vec<_>>(),"measurement_ids":[]})
        }).collect();
        let rule_ids = declaration
            .roles
            .iter()
            .flat_map(|id| roles[id.as_str()].rule_ids.clone())
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
        );
        populations.last_mut().unwrap()["exclusions"] = json!([{"reason":"Missing declared identity fields; excluded from this population, never guessed","count":missing.len()}]);
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
        for entry in entries
            .iter()
            .filter(|entry| selected(entry, selectors, config))
        {
            if !budget.checkpoint("calculation", fields.len() as u64 + 2) {
                return;
            }
            let source = has_role(entry, roles[declaration.source_role.as_str()]);
            let target = has_role(entry, roles[declaration.target_role.as_str()]);
            if !source && !target {
                continue;
            }
            let Some(mut key) = identity(entry, &fields) else {
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
                return;
            }
            if source {
                sources.entry(key.clone()).or_default().push(entry);
            }
            if target {
                targets.entry(key).or_default().push(entry);
            }
        }
        let mut joins = 0usize;
        let mut ambiguous = 0usize;
        for (key, from) in sources {
            let Some(to) = targets.get(&key) else {
                missing += from.len();
                continue;
            };
            let supported = match declaration.cardinality {
                Cardinality::OneToOne => from.len() == 1 && to.len() == 1,
                Cardinality::ManyToOne => to.len() == 1,
                Cardinality::OneToMany => from.len() == 1,
            };
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
                        return;
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
}
