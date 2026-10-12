//! Shared role selectors and scoped join decisions; no timestamp or name guessing.
use crate::{
    event_rules::ClassifiedRecord,
    investigation_policy::{Cardinality, Chain, ChainStep, Relationship, Role},
    parser::LogEntry,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) fn rules(entry: &LogEntry) -> Vec<String> {
    match &entry.classification {
        Some(
            ClassifiedRecord::Event { rule_ids, .. } | ClassifiedRecord::Conflict { rule_ids, .. },
        ) => rule_ids.clone(),
        Some(ClassifiedRecord::Invalid { diagnostics, .. }) => diagnostics
            .iter()
            .filter_map(|d| d.rule_id.clone())
            .collect(),
        _ => Vec::new(),
    }
}
pub(crate) fn scalar_identity(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}
pub(crate) fn cardinality_supported(
    cardinality: Cardinality,
    sources: usize,
    targets: usize,
) -> bool {
    match cardinality {
        Cardinality::OneToOne => sources == 1 && targets == 1,
        Cardinality::OneToMany => sources == 1,
        Cardinality::ManyToOne => targets == 1,
    }
}
pub(crate) fn field(entry: &LogEntry, selector: &str) -> Option<Value> {
    if let Some(alternatives) = selector.strip_prefix("coalesce:") {
        let fields: Vec<_> = alternatives.split('|').collect();
        if selector.len() > 4096
            || fields.len() > 16
            || fields
                .iter()
                .any(|f| f.is_empty() || f.starts_with("coalesce:"))
        {
            return None;
        }
        let mut value = None;
        for candidate in fields.into_iter().filter_map(|f| field(entry, f)) {
            if candidate.is_null() || candidate == "" {
                continue;
            }
            if value
                .as_ref()
                .is_some_and(|previous| previous != &candidate)
            {
                return None;
            }
            value = Some(candidate);
        }
        return value;
    }
    if let Some(prefix) = selector.strip_prefix("component_id.ends_at_first:") {
        if prefix.is_empty() || entry.component_id.len() > 4096 {
            return None;
        }
        let retained = field(entry, &format!("component_id.prefix:{prefix}"))?;
        return Some(json!(
            entry
                .component_id
                .split(" & ")
                .all(|branch| Some(branch) == retained.as_str())
        ));
    }
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
pub(crate) fn key(entry: &LogEntry, fields: &[String]) -> Option<Vec<String>> {
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
            scalar_identity(&value)
        })
        .collect()
}
pub(crate) struct Matcher<'a> {
    pub(crate) role: &'a Role,
    pub(crate) pattern: Option<regex::Regex>,
}
impl<'a> Matcher<'a> {
    pub(crate) fn new(role: &'a Role) -> Self {
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
    pub(crate) fn matches(&self, entry: &LogEntry) -> bool {
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

pub(crate) trait Observer {
    fn work_units(&self) -> u64;
    fn chain_step(
        &mut self,
        chain: &Chain,
        step: &ChainStep,
        roots: &[&LogEntry],
        sources: &[&LogEntry],
    );
    fn chain_group(
        &mut self,
        chain: &Chain,
        step: &ChainStep,
        roots: &[&LogEntry],
        sources: &[&LogEntry],
        targets: &[&LogEntry],
        supported: bool,
    );
    fn chain_later(&mut self, chain: &Chain, roots: &[&LogEntry], activity: Option<&[&LogEntry]>);
    fn relationship_group(
        &mut self,
        relationship: &Relationship,
        sources: &[&LogEntry],
        targets: &[&LogEntry],
        supported: bool,
    );
}
