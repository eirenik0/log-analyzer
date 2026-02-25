use crate::parser::LogEntry;
use serde_json::{Value, json};
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

#[derive(Debug, Clone)]
struct ExtractGroup {
    value_key: String,
    value: Value,
    count: usize,
}

#[derive(Debug, Clone)]
struct ExtractSummary {
    matches: usize,
    extracted: usize,
    missing_payload: usize,
    missing_field: usize,
    groups: Vec<ExtractGroup>,
}

pub fn format_extract_text(logs: &[LogEntry], match_indices: &[usize], field_path: &str) -> String {
    let summary = build_extract_summary(logs, match_indices, field_path);

    if summary.groups.is_empty() {
        return format!(
            "No values found for field '{}' in {} matching entr{}.\n",
            field_path,
            summary.matches,
            if summary.matches == 1 { "y" } else { "ies" }
        );
    }

    let mut out = String::new();
    for group in summary.groups {
        let value = serde_json::to_string(&group.value)
            .unwrap_or_else(|_| "\"<failed to serialize value>\"".to_string());
        let _ = writeln!(
            out,
            "{}={} ({} occurrence{})",
            field_path,
            value,
            group.count,
            if group.count == 1 { "" } else { "s" }
        );
    }

    out
}

pub fn format_extract_json(
    file: &Path,
    logs: &[LogEntry],
    match_indices: &[usize],
    field_path: &str,
) -> String {
    let summary = build_extract_summary(logs, match_indices, field_path);

    serde_json::to_string_pretty(&json!({
        "extract": {
            "file": file.display().to_string(),
            "field": field_path,
            "matches": summary.matches,
            "extracted": summary.extracted,
            "missing_payload": summary.missing_payload,
            "missing_field": summary.missing_field,
            "groups": summary.groups.iter().map(|group| json!({
                "value": group.value,
                "count": group.count,
            })).collect::<Vec<_>>(),
        }
    }))
    .unwrap_or_else(|_| "{\"extract\":{\"error\":\"failed to serialize extract output\"}}".into())
}

fn build_extract_summary(
    logs: &[LogEntry],
    match_indices: &[usize],
    field_path: &str,
) -> ExtractSummary {
    let mut grouped: BTreeMap<String, (Value, usize)> = BTreeMap::new();
    let mut extracted = 0usize;
    let mut missing_payload = 0usize;
    let mut missing_field = 0usize;

    for &idx in match_indices {
        let Some(payload) = logs[idx].payload() else {
            missing_payload += 1;
            continue;
        };

        let Some(value) = extract_field_value(payload, field_path) else {
            missing_field += 1;
            continue;
        };

        extracted += 1;
        let key = serde_json::to_string(value)
            .unwrap_or_else(|_| "\"<failed to serialize value>\"".to_string());
        grouped
            .entry(key)
            .and_modify(|(_, count)| *count += 1)
            .or_insert_with(|| (value.clone(), 1));
    }

    let mut groups: Vec<_> = grouped
        .into_iter()
        .map(|(value_key, (value, count))| ExtractGroup {
            value_key,
            value,
            count,
        })
        .collect();
    groups.sort_by_key(|group| (Reverse(group.count), group.value_key.clone()));

    ExtractSummary {
        matches: match_indices.len(),
        extracted,
        missing_payload,
        missing_field,
        groups,
    }
}

fn extract_field_value<'a>(value: &'a Value, field_path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in field_path.split('.') {
        if segment.is_empty() {
            return None;
        }

        current = match current {
            Value::Object(map) => map.get(segment)?,
            Value::Array(items) => {
                let index = segment.parse::<usize>().ok()?;
                items.get(index)?
            }
            _ => return None,
        };
    }

    Some(current)
}

#[cfg(test)]
mod tests {
    use super::extract_field_value;
    use serde_json::json;

    #[test]
    fn extracts_nested_object_and_array_paths() {
        let payload = json!({
            "settings": {
                "retries": [
                    { "timeout": 1000 },
                    { "timeout": 2000 }
                ]
            }
        });

        assert_eq!(
            extract_field_value(&payload, "settings.retries.1.timeout"),
            Some(&json!(2000))
        );
        assert_eq!(extract_field_value(&payload, "settings.missing"), None);
    }
}
