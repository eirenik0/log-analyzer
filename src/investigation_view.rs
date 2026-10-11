//! Declarative views over findings; no application-specific grouping in the core.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InvestigationView {
    pub source_fields: Vec<String>,
    pub groups: Vec<FindingGroup>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindingGroup {
    pub id: String,
    pub title: String,
    pub select: Vec<Condition>,
    pub by: Vec<Dimension>,
    #[serde(default = "default_groups")]
    pub max_groups: usize,
    #[serde(default)]
    pub measurements: Vec<Measurement>,
}
fn default_groups() -> usize {
    20
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub path: String,
    pub equals: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dimension {
    pub label: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    pub label: String,
    pub path: String,
    pub unit_path: String,
    pub unit: String,
    pub aggregate: Aggregate,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Aggregate {
    Min,
    Max,
    Mean,
    Sum,
}
impl InvestigationView {
    pub fn validate(&self) -> Result<(), String> {
        fn text(value: &str) -> bool {
            !value.is_empty() && value.len() <= 256
        }
        fn pointer(value: &str) -> bool {
            text(value)
                && value.starts_with('/')
                && value.split('/').all(|part| {
                    let mut chars = part.chars();
                    while let Some(ch) = chars.next() {
                        if ch == '~' && !matches!(chars.next(), Some('0' | '1')) {
                            return false;
                        }
                    }
                    true
                })
        }
        fn template(value: &str) -> bool {
            if !text(value) {
                return false;
            }
            let mut rest = value;
            while let Some((prefix, token)) = rest.split_once('{') {
                if prefix.contains('}') {
                    return false;
                }
                let Some((path, suffix)) = token.split_once('}') else {
                    return false;
                };
                if !pointer(path) {
                    return false;
                }
                rest = suffix;
            }
            !rest.contains('}')
        }
        let mut ids = BTreeSet::new();
        let fields: BTreeSet<_> = self.source_fields.iter().collect();
        if self.source_fields.len() > 8
            || fields.len() != self.source_fields.len()
            || self.source_fields.iter().any(|field| !text(field))
            || self.groups.len() > 8
        {
            return Err("Investigation views support at most 8 unique bounded source fields and 8 group definitions".into());
        }
        for group in &self.groups {
            if !text(&group.id)
                || !text(&group.title)
                || !ids.insert(&group.id)
                || !(1..=20).contains(&group.max_groups)
                || group.select.is_empty()
                || group.select.len() > 8
                || group.by.is_empty()
                || group.by.len() > 8
                || group.select.iter().any(|condition| {
                    !pointer(&condition.path)
                        || condition.equals.is_array()
                        || condition.equals.is_object()
                        || condition
                            .equals
                            .as_str()
                            .is_some_and(|value| value.len() > 4096)
                })
                || group.by.iter().any(|dimension| {
                    !text(&dimension.label)
                        || !pointer(&dimension.path)
                        || dimension
                            .template
                            .as_ref()
                            .is_some_and(|value| !template(value))
                })
                || group.measurements.len() > 4
                || group.measurements.iter().any(|metric| {
                    !text(&metric.label)
                        || !pointer(&metric.path)
                        || !pointer(&metric.unit_path)
                        || !text(&metric.unit)
                })
            {
                return Err("Investigation view groups require unique IDs, scalar selectors, valid JSON pointers, 1–8 dimensions and a limit of 1–20 groups".into());
            }
        }
        Ok(())
    }
}
