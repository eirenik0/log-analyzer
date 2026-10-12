//! Bounded starting-profile diagnostics; observed fields are not inferred semantics.
use crate::{
    comparator::LogFilter,
    config::AnalyzerConfig,
    event_rules::ClassifiedRecord,
    evidence,
    parser::LogEntry,
    relationship_rules::{Matcher, field, rules},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn witness(entry: &LogEntry) -> Value {
    json!({"source":evidence::source(entry)})
}
fn add_sample(samples: &mut Vec<Value>, entry: &LogEntry) {
    if samples.len() < 2 {
        samples.push(witness(entry));
    }
}
fn shapes(
    value: &Value,
    path: &str,
    depth: usize,
    result: &mut BTreeMap<String, BTreeSet<&'static str>>,
) {
    if depth > 8 || result.len() >= 128 || path.len() > 512 {
        return;
    }
    let kind = match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    result.entry(path.to_owned()).or_default().insert(kind);
    match value {
        Value::Object(fields) => {
            for (field, value) in fields {
                shapes(
                    value,
                    &format!("{path}/{}", field.replace('~', "~0").replace('/', "~1")),
                    depth + 1,
                    result,
                );
            }
        }
        Value::Array(items) => {
            for item in items.iter().take(3) {
                shapes(item, &format!("{path}/*"), depth + 1, result);
            }
        }
        _ => (),
    }
}
pub(crate) fn report(
    inputs: &[Vec<LogEntry>],
    config: &AnalyzerConfig,
    filter: &LogFilter,
    limit: usize,
) -> Value {
    let mut input_reports = Vec::new();
    let mut role_reports = Vec::new();
    for (input, entries) in inputs.iter().enumerate() {
        let selected: Vec<_> = entries.iter().filter(|e| filter.matches(e)).collect();
        let mut shape_inventory = BTreeMap::new();
        let mut positions = BTreeSet::new();
        if !selected.is_empty() {
            positions.insert(0);
            if limit > 1 {
                positions.insert(selected.len() - 1);
            }
        }
        for (i, entry) in selected.iter().enumerate() {
            if let Some(payload) = &entry.envelope_payload {
                shapes(payload, "", 0, &mut shape_inventory);
            }
            if let Some(payload) = entry.payload() {
                shapes(payload, "/payload", 0, &mut shape_inventory);
            }
            for name in entry.structured_fields.keys() {
                if shape_inventory.len() < 128 && name.len() <= 512 {
                    shape_inventory
                        .entry(format!("structured:{name}"))
                        .or_insert_with(BTreeSet::new)
                        .insert("string");
                }
            }
            if i == 0
                || i + 1 == selected.len()
                || entry.level == "ERROR"
                || entry.level == "FATAL"
                || !matches!(entry.classification, Some(ClassifiedRecord::Event { .. }))
            {
                // Keep first and final context, then representative unclassified/error records.
                if positions.len() < limit.max(1) {
                    positions.insert(i);
                }
            }
        }
        let records:Vec<_> = positions.into_iter().take(limit.max(1)).map(|i| {
            let entry = selected[i];
            json!({"source":evidence::source(entry),"timestamp":entry.source_timestamp.map(|t|t.to_rfc3339()),"component":entry.component,"component_id":entry.component_id,"level":entry.level,"text":entry.message.chars().take(512).collect::<String>(),"omitted_characters":entry.message.chars().count().saturating_sub(512),"classification":entry.classification})
        }).collect();
        input_reports.push(json!({"input":input,"selected_records":selected.len(),"field_shapes":shape_inventory.into_iter().map(|(path,types)|json!({"path":path,"types":types})).collect::<Vec<_>>(),"shape_policy":"At most 128 paths, depth 8, path length 512, first three array members; this is a shape sample, not an exhaustive schema","records":records,"omitted_records":selected.len().saturating_sub(records.len())}));
        if let Some(policy) = &config.investigation {
            for role in &policy.roles {
                let matcher = Matcher::new(role);
                let mut rule_matches = 0;
                let mut matched = 0;
                let mut unresolved = 0;
                let mut pattern_failed = 0;
                let mut witnesses = Vec::new();
                let mut failures: Vec<_> = role
                    .select
                    .iter()
                    .map(|_| (0usize, 0usize, Vec::new()))
                    .collect();
                let pattern = role
                    .message_pattern
                    .as_ref()
                    .map(|p| regex::Regex::new(p).expect("validated role pattern"));
                for entry in &selected {
                    if !rules(entry).iter().any(|id| role.rule_ids.contains(id)) {
                        continue;
                    }
                    rule_matches += 1;
                    if !matches!(entry.classification, Some(ClassifiedRecord::Event { .. })) {
                        unresolved += 1;
                    }
                    for (condition, (failed, missing, samples)) in
                        role.select.iter().zip(&mut failures)
                    {
                        let observed = field(entry, &condition.field);
                        if observed.as_ref() != Some(&condition.equals) {
                            *failed += 1;
                            *missing += usize::from(observed.is_none());
                            add_sample(samples, entry);
                        }
                    }
                    if pattern
                        .as_ref()
                        .is_some_and(|p| !p.is_match(&entry.message))
                    {
                        pattern_failed += 1;
                    }
                    if matches!(entry.classification, Some(ClassifiedRecord::Event { .. }))
                        && matcher.matches(entry)
                    {
                        matched += 1;
                        add_sample(&mut witnesses, entry);
                    }
                }
                let keys: BTreeSet<_> = policy
                    .chains
                    .iter()
                    .flat_map(|chain| {
                        let root = if chain.root_role == role.id {
                            chain
                                .identity_fields
                                .iter()
                                .chain(&chain.scope_fields)
                                .chain(&chain.context_fields)
                                .cloned()
                                .collect::<Vec<_>>()
                        } else {
                            Vec::new()
                        };
                        root.into_iter().chain(
                            chain
                                .steps
                                .iter()
                                .filter(|step| {
                                    step.role == role.id
                                        || (step.from == "$root" && chain.root_role == role.id)
                                        || chain.steps.iter().any(|previous| {
                                            previous.id == step.from && previous.role == role.id
                                        })
                                })
                                .flat_map(|step| {
                                    step.join_fields.iter().chain(&chain.scope_fields).cloned()
                                }),
                        )
                    })
                    .chain(
                        policy
                            .relationships
                            .iter()
                            .filter(|r| r.source_role == role.id || r.target_role == role.id)
                            .flat_map(|r| {
                                r.join_fields
                                    .iter()
                                    .chain(&r.required_scope_fields)
                                    .cloned()
                            }),
                    )
                    .collect();
                let missing_keys: Vec<_> = keys
                    .into_iter()
                    .map(|selector| {
                        let mut count = 0;
                        let mut samples = Vec::new();
                        for entry in &selected {
                            if matcher.matches(entry)
                                && crate::relationship_rules::key(
                                    entry,
                                    std::slice::from_ref(&selector),
                                )
                                .is_none()
                            {
                                count += 1;
                                add_sample(&mut samples, entry);
                            }
                        }
                        json!({"field":selector,"missing_records":count,"witnesses":samples})
                    })
                    .collect();
                role_reports.push(json!({"input":input,"role":role.id,"rule_ids":role.rule_ids,"rule_matched_records":rule_matches,"matched_records":matched,"unresolved_records":unresolved,"failed_selectors":role.select.iter().zip(failures).map(|(condition,(failed,missing,witnesses))|json!({"field":condition.field,"equals":condition.equals,"failed_records":failed,"missing_records":missing,"witnesses":witnesses})).collect::<Vec<_>>(),"message_pattern":role.message_pattern,"message_pattern_failed_records":pattern_failed,"missing_join_keys":missing_keys,"witnesses":witnesses}));
            }
        }
    }
    let (relationships, _) = crate::profile_relationships::analyze(inputs, config, filter, &[]);
    json!({"version":1,"basis":"starting_profile_observations","profile_sha256":evidence::profile_digest(config).expect("profile serializes"),"activation":false,"inputs":input_reports,"roles":role_reports,"relationships":relationships,"questions":["Which source records independently establish each intended event role and outcome level?","Which fields identify the entity, request and containing run; can IDs be reused?","Which records bridge client and server identities?","Which recognized endpoints must join, and which must remain separate?","Do later records or cached outcomes contradict an apparent missing completion?"],"limitations":["Field shapes and wording similarity do not establish application meaning","A candidate needs source-backed assertions; do not copy expected facts from candidate output","Shape and witness samples are bounded; relationship processing limits are reported separately"]})
}
