use crate::parser::{LogEntry, LogEntryKind};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

#[derive(Serialize, Deserialize)]
pub struct LlmLogOutput {
    pub metadata: LlmMetadata,
    pub logs: Vec<LlmLogEntry>,
}

#[derive(Serialize, Deserialize)]
pub struct LlmMetadata {
    pub total_entries: usize,
    pub filtered_entries: usize,
    pub components: Vec<String>,
    pub levels: Vec<String>,
    pub entry_types: HashMap<String, usize>,
    pub time_range: Option<TimeRange>,
}

#[derive(Serialize, Deserialize)]
pub struct TimeRange {
    pub start: String,
    pub end: String,
}

#[derive(Serialize, Deserialize)]
pub struct LlmLogEntry {
    pub idx: usize,
    pub ts: String,
    pub comp: String,
    pub lvl: String,
    pub typ: String,
    pub msg: String,
    pub data: Option<Value>,
}

const SENSITIVE_FIELDS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "secret",
    "key",
    "token",
    "auth",
    "authorization",
    "session",
    "cookie",
    "credentials",
    "private",
    "confidential",
    "apikey",
    "api_key",
    "access_token",
    "refresh_token",
    "client_secret",
    "client_id",
    "user_id",
    "email",
    "phone",
    "address",
    "ssn",
    "credit_card",
    "card_number",
    "cvv",
    "pin",
    "hash",
    "signature",
    "encrypted",
    "cipher",
    "salt",
    "nonce",
    "iv",
    "certificate",
    "cert",
];

pub fn sanitize_json_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sanitized_map = Map::new();
            for (key, val) in map {
                let key_lower = key.to_lowercase();
                let is_sensitive_field = SENSITIVE_FIELDS
                    .iter()
                    .any(|&sensitive| key_lower.contains(sensitive));

                if is_sensitive_field {
                    // Only redact if the value could contain sensitive data
                    match val {
                        Value::String(s) if !s.is_empty() => {
                            sanitized_map.insert(key.clone(), json!("[REDACTED]"));
                        }
                        Value::Object(_) => {
                            sanitized_map.insert(key.clone(), json!("[REDACTED]"));
                        }
                        Value::Array(_) => {
                            sanitized_map.insert(key.clone(), json!("[REDACTED]"));
                        }
                        // Keep booleans, numbers, and null values as-is
                        _ => {
                            sanitized_map.insert(key.clone(), val.clone());
                        }
                    }
                } else {
                    sanitized_map.insert(key.clone(), sanitize_json_value(val));
                }
            }
            Value::Object(sanitized_map)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sanitize_json_value).collect()),
        Value::String(_) => value.clone(),
        _ => value.clone(),
    }
}

pub fn compact_json_value(value: &Value, max_depth: usize, current_depth: usize) -> Value {
    if current_depth >= max_depth {
        return json!("[TRUNCATED]");
    }

    match value {
        Value::Object(map) => {
            let mut compacted_map = Map::new();
            const MAX_FIELDS: usize = 20;

            for (count, (key, val)) in map.into_iter().enumerate() {
                if count >= MAX_FIELDS {
                    compacted_map.insert("_truncated_fields".to_string(), json!(map.len() - count));
                    break;
                }

                // Shorten long field names
                let compact_key = if key.len() > 30 {
                    format!("{}...", &key[0..27])
                } else {
                    key.clone()
                };

                compacted_map.insert(
                    compact_key,
                    compact_json_value(val, max_depth, current_depth + 1),
                );
            }
            Value::Object(compacted_map)
        }
        Value::Array(arr) => {
            const MAX_ARRAY_ITEMS: usize = 10;
            if arr.len() > MAX_ARRAY_ITEMS {
                let mut compacted_arr = arr
                    .iter()
                    .take(MAX_ARRAY_ITEMS)
                    .map(|v| compact_json_value(v, max_depth, current_depth + 1))
                    .collect::<Vec<_>>();
                compacted_arr.push(json!(format!(
                    "[...{} more items]",
                    arr.len() - MAX_ARRAY_ITEMS
                )));
                Value::Array(compacted_arr)
            } else {
                Value::Array(
                    arr.iter()
                        .map(|v| compact_json_value(v, max_depth, current_depth + 1))
                        .collect(),
                )
            }
        }
        Value::String(s) => {
            if s.len() > 100 {
                json!(format!("{}...", &s[0..97]))
            } else {
                value.clone()
            }
        }
        _ => value.clone(),
    }
}

pub fn process_logs_for_llm(logs: &[LogEntry], limit: usize, sanitize: bool) -> LlmLogOutput {
    let total_entries = logs.len();
    let filtered_entries = if limit > 0 && limit < logs.len() {
        limit
    } else {
        logs.len()
    };

    let logs_to_process = if limit > 0 && limit < logs.len() {
        &logs[0..limit]
    } else {
        logs
    };

    // Collect metadata
    let mut components = logs_to_process
        .iter()
        .map(|log| log.component.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    components.sort();

    let mut levels = logs_to_process
        .iter()
        .map(|log| log.level.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    levels.sort();

    let mut entry_types = HashMap::new();
    for log in logs_to_process {
        let entry_type = match &log.kind {
            LogEntryKind::Event {
                event_type,
                direction,
                ..
            } => {
                format!("Event:{}:{}", direction, event_type)
            }
            LogEntryKind::Command { command, .. } => {
                format!("Command:{}", command)
            }
            LogEntryKind::Request {
                request, direction, ..
            } => {
                format!("Request:{}:{}", direction, request)
            }
            LogEntryKind::Generic { .. } => "Generic".to_string(),
        };
        *entry_types.entry(entry_type).or_insert(0) += 1;
    }

    let time_range = if !logs_to_process.is_empty() {
        Some(TimeRange {
            start: logs_to_process
                .first()
                .unwrap()
                .timestamp
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            end: logs_to_process
                .last()
                .unwrap()
                .timestamp
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
        })
    } else {
        None
    };

    let metadata = LlmMetadata {
        total_entries,
        filtered_entries,
        components,
        levels,
        entry_types,
        time_range,
    };

    // Process each log entry
    let processed_logs: Vec<LlmLogEntry> = logs_to_process
        .iter()
        .enumerate()
        .map(|(idx, log)| {
            let entry_type = match &log.kind {
                LogEntryKind::Event {
                    event_type,
                    direction,
                    ..
                } => {
                    format!("E:{}:{}", direction, event_type)
                }
                LogEntryKind::Command { command, .. } => {
                    format!("C:{}", command)
                }
                LogEntryKind::Request {
                    request, direction, ..
                } => {
                    format!("R:{}:{}", direction, request)
                }
                LogEntryKind::Generic { .. } => "G".to_string(),
            };

            let processed_payload = log.payload().map(|payload| {
                // First sanitize if requested
                let sanitized = if sanitize {
                    sanitize_json_value(payload)
                } else {
                    payload.clone()
                };

                // Then compact the sanitized data
                compact_json_value(&sanitized, 3, 0)
            });

            // Compact message text
            let compact_message = if log.message.len() > 200 {
                format!("{}...", &log.message[0..197])
            } else {
                log.message.clone()
            };

            LlmLogEntry {
                idx: idx + 1,
                ts: log.timestamp.format("%H:%M:%S%.3f").to_string(),
                comp: log.component.clone(),
                lvl: log.level.clone(),
                typ: entry_type,
                msg: compact_message,
                data: processed_payload,
            }
        })
        .collect();

    LlmLogOutput {
        metadata,
        logs: processed_logs,
    }
}

/// Sanitize a single log entry's payload in-place
pub fn sanitize_log_entry(log: &mut LogEntry) {
    match &mut log.kind {
        LogEntryKind::Event { payload, .. } => {
            if let Some(payload) = payload {
                *payload = sanitize_json_value(payload);
            }
        }
        LogEntryKind::Command { settings, .. } => {
            if let Some(settings) = settings {
                *settings = sanitize_json_value(settings);
            }
        }
        LogEntryKind::Request { payload, .. } => {
            if let Some(payload) = payload {
                *payload = sanitize_json_value(payload);
            }
        }
        LogEntryKind::Generic { payload } => {
            if let Some(payload) = payload {
                *payload = sanitize_json_value(payload);
            }
        }
    }
}

/// Sanitize a vector of log entries
pub fn sanitize_logs(logs: &mut [LogEntry]) {
    for log in logs.iter_mut() {
        sanitize_log_entry(log);
    }
}
