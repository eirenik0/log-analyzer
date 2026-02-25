use crate::comparator::LogFilter;
use crate::parser::{LogEntry, LogEntryKind};
use chrono::{SecondsFormat, Utc};
use serde_json::json;
use std::fmt::Write;

#[derive(Debug, Clone)]
pub enum TraceSelector {
    Id(String),
    Session(String),
}

impl TraceSelector {
    pub fn selector_type(&self) -> &'static str {
        match self {
            Self::Id(_) => "id",
            Self::Session(_) => "session",
        }
    }

    pub fn value(&self) -> &str {
        match self {
            Self::Id(value) | Self::Session(value) => value,
        }
    }

    fn matches(&self, entry: &LogEntry) -> bool {
        match self {
            Self::Id(needle) => matches_id(entry, needle),
            Self::Session(needle) => {
                !entry.component_id.is_empty() && entry.component_id.contains(needle)
            }
        }
    }
}

fn matches_id(entry: &LogEntry, needle: &str) -> bool {
    if entry.raw_logline.contains(needle) {
        return true;
    }

    matches!(
        &entry.kind,
        LogEntryKind::Request {
            request_id: Some(request_id),
            ..
        } if request_id.contains(needle)
    )
}

pub fn collect_trace_entries<'a>(
    logs: &'a [LogEntry],
    filter: &LogFilter,
    selector: &TraceSelector,
) -> Vec<&'a LogEntry> {
    let mut entries: Vec<&LogEntry> = logs
        .iter()
        .filter(|entry| filter.matches(entry) && selector.matches(entry))
        .collect();

    entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    entries
}

pub fn format_trace_text(entries: &[&LogEntry], selector: &TraceSelector) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "TRACE ({}) contains \"{}\"",
        selector.selector_type(),
        selector.value()
    );

    if entries.is_empty() {
        let _ = writeln!(out, "No matching log entries found.");
        return out;
    }

    let first_ts = entries[0].timestamp;
    let last_ts = entries
        .last()
        .map(|entry| entry.timestamp)
        .unwrap_or(first_ts);
    let total_ms = last_ts.signed_duration_since(first_ts).num_milliseconds();

    let _ = writeln!(
        out,
        "Matched {} entries across {} ms.\n",
        entries.len(),
        total_ms
    );

    let mut prev_ts = None;
    for entry in entries {
        let delta_ms = prev_ts
            .map(|ts| entry.timestamp.signed_duration_since(ts).num_milliseconds())
            .unwrap_or(0);
        let elapsed_ms = entry
            .timestamp
            .signed_duration_since(first_ts)
            .num_milliseconds();
        prev_ts = Some(entry.timestamp);

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
            "{}  +{delta_ms:>6}ms  T+{elapsed_ms:>6}ms  [{}] {} | {} (line {})",
            ts, entry.level, component_label, message, entry.source_line_number
        );
    }

    out
}

pub fn format_trace_json(entries: &[&LogEntry], selector: &TraceSelector) -> String {
    let first_ts = entries.first().map(|entry| entry.timestamp);
    let last_ts = entries.last().map(|entry| entry.timestamp);

    let mut prev_ts = None;
    let rows: Vec<_> = entries
        .iter()
        .map(|entry| {
            let delta_ms = prev_ts
                .map(|ts| entry.timestamp.signed_duration_since(ts).num_milliseconds())
                .unwrap_or(0);
            let elapsed_ms = first_ts
                .map(|ts| entry.timestamp.signed_duration_since(ts).num_milliseconds())
                .unwrap_or(0);
            prev_ts = Some(entry.timestamp);

            let request_id = match &entry.kind {
                LogEntryKind::Request {
                    request_id: Some(id),
                    ..
                } => Some(id.clone()),
                _ => None,
            };

            json!({
                "timestamp": entry
                    .timestamp
                    .with_timezone(&Utc)
                    .to_rfc3339_opts(SecondsFormat::Millis, true),
                "delta_ms": delta_ms,
                "elapsed_ms": elapsed_ms,
                "component": entry.component,
                "component_id": entry.component_id,
                "level": entry.level,
                "kind": entry.entry_type(),
                "log_key": entry.log_key(),
                "message": entry.message,
                "raw_logline": entry.raw_logline,
                "source_line_number": entry.source_line_number,
                "request_id": request_id,
            })
        })
        .collect();

    let total_duration_ms = match (first_ts, last_ts) {
        (Some(first), Some(last)) => last.signed_duration_since(first).num_milliseconds(),
        _ => 0,
    };

    serde_json::to_string_pretty(&json!({
        "trace": {
            "selector": {
                "type": selector.selector_type(),
                "value": selector.value(),
                "match_mode": "contains",
            },
            "count": entries.len(),
            "total_duration_ms": total_duration_ms,
            "entries": rows,
        }
    }))
    .unwrap_or_else(|_| "{\"trace\":{\"error\":\"failed to serialize trace output\"}}".to_string())
}
