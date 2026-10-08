mod display;
mod entities;

pub use display::{
    display_perf_results, format_perf_results_json, format_perf_results_json_with_options,
    format_perf_results_text, truncate_string,
};
pub use entities::{
    AmbiguousGroup, OperationStats, OrphanOperation, PerfAnalysisResults, SourceLocation,
    TimedOperation, UnmatchedEvent,
};

use crate::comparator::LogFilter;
use crate::config::{AnalyzerConfig, PerfRules, contains_any_marker, default_config};
use crate::parser::{EventDirection, LogEntry, LogEntryKind, RequestDirection};

/// Extracts the request ID from a log message containing [request_id] pattern
/// The pattern is: Request "name" [id] where id contains "--" (e.g., "0--uuid" or "0--uuid#2")
pub fn extract_request_id(message: &str) -> Option<String> {
    // Look for pattern: Request "name" [id] where id contains "--"
    let req_prefix = r#"Request ""#;
    if let Some(start_idx) = message.find(req_prefix) {
        let after_prefix = start_idx + req_prefix.len();
        // Find closing quote of request name
        if let Some(name_end) = message[after_prefix..].find('"') {
            let after_name = after_prefix + name_end + 1;
            if after_name < message.len() {
                let rest = &message[after_name..];
                // Request ID should be immediately after: " [id]"
                if rest.starts_with(" [")
                    && let Some(id_end) = rest[2..].find(']')
                {
                    let potential_id = &rest[2..2 + id_end];
                    // Validate it looks like a request ID (contains --)
                    if potential_id.contains("--") && !potential_id.contains(' ') {
                        return Some(potential_id.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Extracts event key from event payload
pub fn extract_event_key(payload: &serde_json::Value) -> Option<String> {
    extract_event_key_with_rules(payload, &default_config().perf)
}

fn extract_event_key_with_rules(payload: &serde_json::Value, rules: &PerfRules) -> Option<String> {
    rules.event_correlation_keys.iter().find_map(|key| {
        payload
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    })
}

/// Analyzes logs for performance bottlenecks by tracking paired operations
pub fn analyze_performance(
    logs: &[LogEntry],
    filter: &LogFilter,
    op_type_filter: Option<&str>,
) -> PerfAnalysisResults {
    analyze_performance_with_config(logs, filter, op_type_filter, default_config())
}

/// Analyzes logs for performance bottlenecks using explicit analyzer config.
pub fn analyze_performance_with_config(
    logs: &[LogEntry],
    filter: &LogFilter,
    op_type_filter: Option<&str>,
    config: &AnalyzerConfig,
) -> PerfAnalysisResults {
    let mut results = PerfAnalysisResults::new();

    let filtered: Vec<_> = logs.iter().filter(|entry| filter.matches(entry)).collect();
    results.total_entries = filtered.len();
    results.time_range = filtered
        .iter()
        .map(|entry| entry.timestamp)
        .min()
        .zip(filtered.iter().map(|entry| entry.timestamp).max());
    let track_commands = filtered.iter().any(|entry| {
        matches!(entry.kind, LogEntryKind::Command { .. })
            && contains_any_marker(&entry.message, &config.perf.command_completion_markers)
    });
    let mut groups: std::collections::BTreeMap<CorrelationKey, Vec<BoundaryEvent<'_>>> =
        std::collections::BTreeMap::new();
    for entry in filtered {
        let (name, id, start, end) = match &entry.kind {
            LogEntryKind::Request {
                request,
                request_id,
                direction,
                ..
            } => (
                request.as_str(),
                request_id
                    .clone()
                    .or_else(|| extract_request_id(&entry.message)),
                *direction == RequestDirection::Send,
                *direction == RequestDirection::Receive,
            ),
            LogEntryKind::Event {
                event_type,
                payload,
                direction,
            } => (
                event_type.as_str(),
                payload
                    .as_ref()
                    .and_then(|p| extract_event_key_with_rules(p, &config.perf)),
                *direction == EventDirection::Receive,
                *direction == EventDirection::Emit,
            ),
            LogEntryKind::Command { command, .. } if track_commands => (
                command.as_str(),
                Some(command.clone()),
                contains_any_marker(&entry.message, &config.perf.command_start_markers),
                contains_any_marker(&entry.message, &config.perf.command_completion_markers),
            ),
            _ => continue,
        };
        let op_type = entry.entry_type();
        if op_type_filter.is_some_and(|selected| selected != op_type) || (!start && !end) {
            continue;
        }
        let event = BoundaryEvent {
            entry,
            name,
            id,
            start,
        };
        if start && end {
            results
                .unmatched_events
                .push(event.unmatched(Vec::new(), "ambiguous_boundary"));
            continue;
        }
        let Some(id) = event.id.clone().filter(|id| !id.is_empty()) else {
            results
                .unmatched_events
                .push(event.unmatched(Vec::new(), "missing_correlation_key"));
            continue;
        };
        let scope = config
            .perf
            .correlation_scope_fields
            .iter()
            .map(|field| match field.as_str() {
                "component_id" => {
                    (!entry.component_id.trim().is_empty()).then(|| entry.component_id.clone())
                }
                "component" => Some(entry.component.clone()),
                _ => entry
                    .structured_field(field)
                    .map(str::to_owned)
                    .or_else(|| {
                        entry
                            .envelope_payload
                            .as_ref()
                            .and_then(|p| p.get(field))
                            .or_else(|| entry.payload().and_then(|p| p.get(field)))
                            .filter(|value| !value.is_null())
                            .map(|value| {
                                value
                                    .as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| value.to_string())
                            })
                    }),
            })
            .map(|value| value.filter(|value| !value.trim().is_empty() && value != "null"))
            .collect::<Option<Vec<_>>>();
        let Some(scope) = scope else {
            results
                .unmatched_events
                .push(event.unmatched(Vec::new(), "missing_scope_field"));
            continue;
        };
        groups
            .entry(CorrelationKey {
                op_type: op_type.to_string(),
                name: name.to_string(),
                id,
                scope,
            })
            .or_default()
            .push(event);
    }
    for (key, mut events) in groups {
        events.sort_by_key(|event| event.entry.timestamp);
        let mut active = false;
        let ambiguous = events.iter().any(|event| {
            if event.start {
                if active {
                    return true;
                }
                active = true;
            } else {
                active = false;
            }
            false
        });
        if ambiguous {
            let preserved: Vec<_> = events
                .iter()
                .map(|event| event.unmatched(key.scope.clone(), "overlapping_starts"))
                .collect();
            for event in &events {
                if event.start {
                    results.orphans.push(event.orphan());
                }
            }
            results.unmatched_events.extend(preserved.clone());
            results.ambiguous_groups.push(AmbiguousGroup {
                op_type: key.op_type,
                name: key.name,
                correlation_id: key.id,
                scope: key.scope,
                events: preserved,
            });
            continue;
        }
        let mut pending: Option<BoundaryEvent<'_>> = None;
        for event in events {
            if event.start {
                pending = Some(event);
            } else if let Some(start) = pending.take() {
                let entry = event.entry;
                results.operations.push(TimedOperation {
                    op_type: key.op_type.clone(),
                    name: key.name.clone(),
                    correlation_id: Some(key.id.clone()),
                    start_time: start.entry.timestamp,
                    end_time: entry.timestamp,
                    duration_ms: entry
                        .timestamp
                        .signed_duration_since(start.entry.timestamp)
                        .num_milliseconds(),
                    start_component: start.entry.component.clone(),
                    end_component: entry.component.clone(),
                    start_source: source(start.entry),
                    end_source: source(entry),
                    scope: key.scope.clone(),
                    endpoint: match &entry.kind {
                        LogEntryKind::Request { endpoint, .. } => endpoint.clone(),
                        _ => None,
                    },
                    status: entry
                        .payload()
                        .and_then(|p| p.get("statusCode"))
                        .and_then(|v| v.as_i64())
                        .map(|v| v.to_string()),
                });
            } else {
                results
                    .unmatched_events
                    .push(event.unmatched(key.scope.clone(), "missing_start"));
            }
        }
        if let Some(event) = pending {
            results.orphans.push(event.orphan());
            results
                .unmatched_events
                .push(event.unmatched(key.scope, "missing_end"));
        }
    }
    results.calculate_stats();
    results
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CorrelationKey {
    op_type: String,
    name: String,
    id: String,
    scope: Vec<String>,
}

struct BoundaryEvent<'a> {
    entry: &'a LogEntry,
    name: &'a str,
    id: Option<String>,
    start: bool,
}

fn source(entry: &LogEntry) -> SourceLocation {
    SourceLocation {
        file: entry.source_file.clone(),
        line: entry.source_line_number,
        row_path: entry.source_row_path.clone(),
    }
}

impl BoundaryEvent<'_> {
    fn unmatched(&self, scope: Vec<String>, reason: &str) -> UnmatchedEvent {
        UnmatchedEvent {
            op_type: self.entry.entry_type().to_string(),
            name: self.name.to_string(),
            correlation_id: self.id.clone(),
            scope,
            boundary: if self.start { "start" } else { "end" }.to_string(),
            reason: reason.to_string(),
            timestamp: self.entry.timestamp,
            component: self.entry.component.clone(),
            source: source(self.entry),
            context: self.entry.raw_logline.clone(),
        }
    }
    fn orphan(&self) -> OrphanOperation {
        OrphanOperation {
            op_type: self.entry.entry_type().to_string(),
            name: self.name.to_string(),
            correlation_id: self.id.clone(),
            start_time: self.entry.timestamp,
            component: self.entry.component.clone(),
            component_id: (!self.entry.component_id.is_empty())
                .then(|| self.entry.component_id.clone()),
            source: source(self.entry),
            context: self.entry.message.clone(),
        }
    }
}
