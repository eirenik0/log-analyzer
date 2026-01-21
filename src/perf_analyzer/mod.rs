mod display;
mod entities;

pub use display::{display_perf_results, format_perf_results_json};
pub use entities::{OperationStats, OrphanOperation, PerfAnalysisResults, TimedOperation};

use crate::comparator::LogFilter;
use crate::parser::{EventDirection, LogEntry, LogEntryKind, RequestDirection};
use std::collections::HashMap;

/// Extracts the request ID from a log message containing [request_id] pattern
/// The pattern is: Request "name" [id] where id contains "--" (e.g., "0--uuid" or "0--uuid#2")
fn extract_request_id(message: &str) -> Option<String> {
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
fn extract_event_key(payload: &serde_json::Value) -> Option<String> {
    // Try to get the "key" field from the event payload
    payload
        .get("key")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Extracts command correlation key from log entry
fn extract_command_key(entry: &LogEntry) -> Option<String> {
    // Use command name + component_id as correlation key
    if let LogEntryKind::Command { command, .. } = &entry.kind {
        return Some(format!("{}:{}", command, entry.component_id));
    }
    None
}

/// Checks if the logs contain any Command completion patterns
/// If not, Command tracking should be skipped since they would all appear as orphans
fn has_command_completion_patterns(logs: &[LogEntry]) -> bool {
    logs.iter().any(|entry| {
        if let LogEntryKind::Command { .. } = &entry.kind {
            // Check for "finished" pattern which indicates command completion
            entry.message.contains("finished")
                || entry.message.contains("returned")
                || entry.message.contains("completed")
        } else {
            false
        }
    })
}

/// Analyzes logs for performance bottlenecks by tracking paired operations
pub fn analyze_performance(
    logs: &[LogEntry],
    filter: &LogFilter,
    op_type_filter: Option<&str>,
) -> PerfAnalysisResults {
    let mut results = PerfAnalysisResults::new();

    // Track pending operations by correlation key
    let mut pending_requests: HashMap<String, &LogEntry> = HashMap::new();
    let mut pending_events: HashMap<String, &LogEntry> = HashMap::new();
    let mut pending_commands: HashMap<String, &LogEntry> = HashMap::new();

    // Check if we should track commands (only if completion patterns exist)
    let track_commands = has_command_completion_patterns(logs);

    // Filter logs first
    let filtered_logs: Vec<&LogEntry> = logs.iter().filter(|log| filter.matches(log)).collect();

    results.total_entries = filtered_logs.len();

    // Determine time range
    if !filtered_logs.is_empty() {
        let first_time = filtered_logs.first().unwrap().timestamp;
        let last_time = filtered_logs.last().unwrap().timestamp;
        results.time_range = Some((first_time, last_time));
    }

    // Process logs to find paired operations
    for entry in filtered_logs {
        match &entry.kind {
            LogEntryKind::Request {
                request,
                request_id,
                endpoint,
                direction,
                payload,
            } => {
                if op_type_filter.is_some() && op_type_filter != Some("Request") {
                    continue;
                }

                // Try to find correlation key
                let correlation_key = if let Some(req_id) = request_id {
                    Some(req_id.clone())
                } else {
                    extract_request_id(&entry.message)
                };

                match direction {
                    RequestDirection::Send => {
                        // This is a request start - store it
                        if let Some(key) = correlation_key {
                            pending_requests.insert(key, entry);
                        }
                    }
                    RequestDirection::Receive => {
                        // This is a request end - try to match with start
                        if let Some(key) = correlation_key
                            && let Some(start_entry) = pending_requests.remove(&key)
                        {
                            // Calculate duration
                            let duration = entry
                                .timestamp
                                .signed_duration_since(start_entry.timestamp)
                                .num_milliseconds();

                            // Extract status from payload
                            let status = payload
                                .as_ref()
                                .and_then(|p| p.get("statusCode"))
                                .and_then(|s| s.as_i64())
                                .map(|s| s.to_string());

                            results.operations.push(TimedOperation {
                                op_type: "Request".to_string(),
                                name: request.clone(),
                                correlation_id: Some(key),
                                start_time: start_entry.timestamp,
                                end_time: entry.timestamp,
                                duration_ms: duration,
                                start_component: start_entry.component.clone(),
                                end_component: entry.component.clone(),
                                endpoint: endpoint.clone(),
                                status,
                            });
                        }
                    }
                }
            }
            LogEntryKind::Event {
                event_type,
                direction,
                payload,
            } => {
                if op_type_filter.is_some() && op_type_filter != Some("Event") {
                    continue;
                }

                // Try to get correlation key from payload
                let correlation_key = payload.as_ref().and_then(extract_event_key);

                match direction {
                    EventDirection::Receive => {
                        // Event received - this is the start
                        if let Some(key) = correlation_key {
                            pending_events.insert(key, entry);
                        }
                    }
                    EventDirection::Emit => {
                        // Event emitted - this is the end
                        if let Some(key) = correlation_key
                            && let Some(start_entry) = pending_events.remove(&key)
                        {
                            let duration = entry
                                .timestamp
                                .signed_duration_since(start_entry.timestamp)
                                .num_milliseconds();

                            results.operations.push(TimedOperation {
                                op_type: "Event".to_string(),
                                name: event_type.clone(),
                                correlation_id: Some(key),
                                start_time: start_entry.timestamp,
                                end_time: entry.timestamp,
                                duration_ms: duration,
                                start_component: start_entry.component.clone(),
                                end_component: entry.component.clone(),
                                endpoint: None,
                                status: None,
                            });
                        }
                    }
                }
            }
            LogEntryKind::Command { command, .. } => {
                if op_type_filter.is_some() && op_type_filter != Some("Command") {
                    continue;
                }

                // Skip Command tracking if no completion patterns exist in the logs
                // This prevents showing all Commands as orphans when the SDK doesn't log completion
                if !track_commands {
                    continue;
                }

                // Check if this is a start or finish
                let is_start = entry.message.contains("is called");
                let is_finish = entry.message.contains("finished")
                    || entry.message.contains("returned")
                    || entry.message.contains("completed");

                if let Some(key) = extract_command_key(entry) {
                    if is_start {
                        pending_commands.insert(key, entry);
                    } else if is_finish && let Some(start_entry) = pending_commands.remove(&key) {
                        let duration = entry
                            .timestamp
                            .signed_duration_since(start_entry.timestamp)
                            .num_milliseconds();

                        results.operations.push(TimedOperation {
                            op_type: "Command".to_string(),
                            name: command.clone(),
                            correlation_id: Some(key),
                            start_time: start_entry.timestamp,
                            end_time: entry.timestamp,
                            duration_ms: duration,
                            start_component: start_entry.component.clone(),
                            end_component: entry.component.clone(),
                            endpoint: None,
                            status: None,
                        });
                    }
                }
            }
            LogEntryKind::Generic { .. } => {
                // Skip generic log entries for performance analysis
            }
        }
    }

    // Convert remaining pending operations to orphans
    for (key, entry) in pending_requests {
        if let LogEntryKind::Request { request, .. } = &entry.kind {
            results.orphans.push(OrphanOperation {
                op_type: "Request".to_string(),
                name: request.clone(),
                correlation_id: Some(key),
                start_time: entry.timestamp,
                component: entry.component.clone(),
                context: entry.message.clone(),
            });
        }
    }

    for (key, entry) in pending_events {
        if let LogEntryKind::Event { event_type, .. } = &entry.kind {
            results.orphans.push(OrphanOperation {
                op_type: "Event".to_string(),
                name: event_type.clone(),
                correlation_id: Some(key),
                start_time: entry.timestamp,
                component: entry.component.clone(),
                context: entry.message.clone(),
            });
        }
    }

    for (key, entry) in pending_commands {
        if let LogEntryKind::Command { command, .. } = &entry.kind {
            results.orphans.push(OrphanOperation {
                op_type: "Command".to_string(),
                name: command.clone(),
                correlation_id: Some(key),
                start_time: entry.timestamp,
                component: entry.component.clone(),
                context: entry.message.clone(),
            });
        }
    }

    // Calculate statistics
    results.calculate_stats();

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_request_id() {
        // Valid request ID patterns
        assert_eq!(
            extract_request_id(r#"Request "check" [0--abc123-def] will be sent"#),
            Some("0--abc123-def".to_string())
        );
        assert_eq!(
            extract_request_id(r#"Request "openEyes" [1--uuid-here#2] that was sent"#),
            Some("1--uuid-here#2".to_string())
        );

        // Invalid patterns - no request ID after name
        assert_eq!(extract_request_id("No brackets here"), None);
        assert_eq!(
            extract_request_id(r#"Request "check" called for target {"#),
            None
        );
        // Brackets in wrong place (JSON content)
        assert_eq!(
            extract_request_id(r#"Request "check" called for renders [1,2,3]"#),
            None
        );
    }

    #[test]
    fn test_extract_event_key() {
        let json = serde_json::json!({
            "key": "test-event-key",
            "data": "some data"
        });
        assert_eq!(extract_event_key(&json), Some("test-event-key".to_string()));

        let json_no_key = serde_json::json!({
            "data": "some data"
        });
        assert_eq!(extract_event_key(&json_no_key), None);
    }
}
