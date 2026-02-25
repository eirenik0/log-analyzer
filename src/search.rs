use crate::cli::SearchCountBy;
use crate::comparator::LogFilter;
use crate::parser::LogEntry;
use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write;
use std::path::Path;

#[derive(Debug, Clone)]
struct DisplayRow {
    idx: usize,
    is_match: bool,
    new_chunk: bool,
}

#[derive(Debug, Clone)]
struct CountGroup {
    key: String,
    count: usize,
}

pub fn collect_match_indices(logs: &[LogEntry], filter: &LogFilter) -> Vec<usize> {
    logs.iter()
        .enumerate()
        .filter_map(|(idx, entry)| filter.matches(entry).then_some(idx))
        .collect()
}

pub fn format_search_text(
    logs: &[LogEntry],
    match_indices: &[usize],
    context: usize,
    show_payloads: bool,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "SEARCH matched {} entr{}",
        match_indices.len(),
        if match_indices.len() == 1 { "y" } else { "ies" }
    );

    if match_indices.is_empty() {
        let _ = writeln!(out, "No matching log entries found.");
        return out;
    }

    if context > 0 {
        let _ = writeln!(out, "Context: {context} entries");
    }
    if show_payloads {
        let _ = writeln!(out, "Payloads: shown");
    }
    out.push('\n');

    for row in build_display_rows(logs, match_indices, context) {
        if row.new_chunk {
            let _ = writeln!(out, "--");
        }

        let entry = &logs[row.idx];
        let marker = if row.is_match { '>' } else { ' ' };
        let ts = entry
            .timestamp
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let component_label = if entry.component_id.is_empty() {
            entry.component.as_str().to_string()
        } else {
            format!("{} ({})", entry.component, entry.component_id)
        };
        let message = entry.message.replace('\n', "\\n");

        let _ = writeln!(
            out,
            "{marker}{:>6}: {} [{}] {} | {}",
            entry.source_line_number, ts, entry.level, component_label, message
        );

        if show_payloads && let Some(payload) = entry.payload() {
            let payload_text = serde_json::to_string(payload)
                .unwrap_or_else(|_| "\"<failed to serialize payload>\"".to_string());
            let _ = writeln!(out, "       payload: {payload_text}");
        }
    }

    out
}

pub fn format_search_json(
    file: &Path,
    logs: &[LogEntry],
    match_indices: &[usize],
    context: usize,
    show_payloads: bool,
) -> String {
    let rows = build_display_rows(logs, match_indices, context);
    let entries: Vec<_> = rows
        .iter()
        .map(|row| {
            let entry = &logs[row.idx];
            json!({
                "is_match": row.is_match,
                "source_line_number": entry.source_line_number,
                "timestamp": entry
                    .timestamp
                    .with_timezone(&Utc)
                    .to_rfc3339_opts(SecondsFormat::Millis, true),
                "component": entry.component,
                "component_id": entry.component_id,
                "level": entry.level,
                "kind": entry.entry_type(),
                "log_key": entry.log_key(),
                "message": entry.message,
                "raw_logline": entry.raw_logline,
                "payload": if show_payloads { entry.payload().cloned() } else { None },
            })
        })
        .collect();

    serde_json::to_string_pretty(&json!({
        "search": {
            "file": file.display().to_string(),
            "matches": match_indices.len(),
            "context": context,
            "show_payloads": show_payloads,
            "entries": entries,
        }
    }))
    .unwrap_or_else(|_| "{\"search\":{\"error\":\"failed to serialize search output\"}}".into())
}

pub fn format_search_count_text(
    logs: &[LogEntry],
    match_indices: &[usize],
    count_by: SearchCountBy,
) -> String {
    if count_by == SearchCountBy::Matches {
        return format!("{}\n", match_indices.len());
    }

    let groups = build_count_groups(logs, match_indices, count_by);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "SEARCH count by {} ({} entr{})",
        count_by_label(count_by),
        match_indices.len(),
        if match_indices.len() == 1 { "y" } else { "ies" }
    );

    if groups.is_empty() {
        return out;
    }

    out.push('\n');
    for group in groups {
        let _ = writeln!(out, "{:>6}  {}", group.count, group.key);
    }

    out
}

pub fn format_search_count_json(
    file: &Path,
    logs: &[LogEntry],
    match_indices: &[usize],
    count_by: SearchCountBy,
) -> String {
    let groups = build_count_groups(logs, match_indices, count_by);
    serde_json::to_string_pretty(&json!({
        "search": {
            "file": file.display().to_string(),
            "matches": match_indices.len(),
            "count_by": count_by_label(count_by),
            "groups": groups
                .iter()
                .map(|group| json!({
                    "key": group.key,
                    "count": group.count,
                }))
                .collect::<Vec<_>>(),
        }
    }))
    .unwrap_or_else(|_| {
        "{\"search\":{\"error\":\"failed to serialize search count output\"}}".into()
    })
}

fn build_display_rows(
    logs: &[LogEntry],
    match_indices: &[usize],
    context: usize,
) -> Vec<DisplayRow> {
    if logs.is_empty() || match_indices.is_empty() {
        return Vec::new();
    }

    let match_set: HashSet<usize> = match_indices.iter().copied().collect();
    let mut included = BTreeSet::new();

    for &idx in match_indices {
        let start = idx.saturating_sub(context);
        let end = idx
            .saturating_add(context)
            .min(logs.len().saturating_sub(1));
        for i in start..=end {
            included.insert(i);
        }
    }

    let mut rows = Vec::with_capacity(included.len());
    let mut prev_idx = None;
    for idx in included {
        let new_chunk = prev_idx.is_some_and(|prev| idx > prev + 1);
        rows.push(DisplayRow {
            idx,
            is_match: match_set.contains(&idx),
            new_chunk,
        });
        prev_idx = Some(idx);
    }

    rows
}

fn build_count_groups(
    logs: &[LogEntry],
    match_indices: &[usize],
    count_by: SearchCountBy,
) -> Vec<CountGroup> {
    let mut grouped: BTreeMap<String, usize> = BTreeMap::new();

    for &idx in match_indices {
        let key = match count_by {
            SearchCountBy::Matches => "matches".to_string(),
            SearchCountBy::Component => logs[idx].component.clone(),
            SearchCountBy::Level => logs[idx].level.clone(),
            SearchCountBy::Type => logs[idx].log_key(),
            SearchCountBy::Payload => logs[idx]
                .payload()
                .and_then(|payload| serde_json::to_string(payload).ok())
                .unwrap_or_else(|| "<none>".to_string()),
        };
        *grouped.entry(key).or_insert(0) += 1;
    }

    let mut groups: Vec<_> = grouped
        .into_iter()
        .map(|(key, count)| CountGroup { key, count })
        .collect();
    groups.sort_by_key(|group| (Reverse(group.count), group.key.clone()));
    groups
}

fn count_by_label(count_by: SearchCountBy) -> &'static str {
    match count_by {
        SearchCountBy::Matches => "matches",
        SearchCountBy::Component => "component",
        SearchCountBy::Level => "level",
        SearchCountBy::Type => "type",
        SearchCountBy::Payload => "payload",
    }
}
