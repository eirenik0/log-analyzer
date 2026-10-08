//! Presentation-only redaction. Analysis always sees original entries.
use regex::Regex;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write};
use std::sync::LazyLock;

const SECRET_FIELDS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "access_token",
    "refresh_token",
    "api_key",
    "apikey",
    "client_secret",
    "authorization",
    "auth",
    "cookie",
    "set_cookie",
    "credentials",
    "private_key",
];

#[derive(Default)]
struct OutputState {
    redact: bool,
    mask_ids: Vec<String>,
    masked_values: BTreeMap<String, String>,
    stdout: String,
}

thread_local! {
    static STATE: RefCell<Option<OutputState>> = const { RefCell::new(None) };
}

/// CLI-scoped buffering lets JSON receive metadata once and text redact across fragments.
pub struct OutputGuard;
impl OutputGuard {
    pub fn new(redact: bool, mask_ids: &[String]) -> Self {
        STATE.with(|state| {
            *state.borrow_mut() = Some(OutputState {
                redact,
                mask_ids: mask_ids.to_vec(),
                ..OutputState::default()
            });
        });
        Self
    }
}
impl Drop for OutputGuard {
    fn drop(&mut self) {
        let rendered = STATE.with(|state| {
            let mut state = state.borrow_mut().take()?;
            let text = std::mem::take(&mut state.stdout);
            Some(state.report(&text))
        });
        if let Some(rendered) = rendered {
            let _ = io::stdout().lock().write_all(rendered.as_bytes());
        }
    }
}

pub fn print(args: fmt::Arguments<'_>) {
    let text = args.to_string();
    let captured = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Some(state) = state.as_mut().filter(|s| s.redact) {
            state.stdout.push_str(&text);
            true
        } else {
            false
        }
    });
    if !captured {
        std::print!("{text}");
    }
}

pub fn format_report(text: &str) -> String {
    STATE.with(|state| match state.borrow_mut().as_mut() {
        Some(state) => state.report(text),
        None => text.to_string(),
    })
}

pub fn diagnostic(text: &str) -> String {
    STATE.with(
        |state| match state.borrow_mut().as_mut().filter(|s| s.redact) {
            Some(state) => state.text(text),
            None => text.to_string(),
        },
    )
}

fn canonical(field: &str) -> String {
    field
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(value) = u8::from_str_radix(
                std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""),
                16,
            )
        {
            decoded.push(value);
            index += 3;
            continue;
        }
        decoded.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| text.to_string())
}

impl OutputState {
    fn replacement(&mut self, key: &str, value: &str) -> Option<String> {
        if let Some(replacement) = self.masked_values.get(value) {
            return Some(replacement.clone());
        }
        let decoded = percent_decode(key);
        let key = canonical(&decoded);
        let leaf = canonical(decoded.rsplit('.').next().unwrap_or(&decoded));
        if self
            .mask_ids
            .iter()
            .any(|field| canonical(field) == key || canonical(field) == leaf)
        {
            if value == "null" || value.is_empty() {
                return None;
            }
            if value.starts_with("[MASKED_ID:") {
                return Some(value.to_string());
            }
            let next = self.masked_values.len() + 1;
            return Some(
                self.masked_values
                    .entry(value.to_string())
                    .or_insert_with(|| format!("[MASKED_ID:{next}]"))
                    .clone(),
            );
        }
        SECRET_FIELDS
            .iter()
            .any(|field| canonical(field) == leaf)
            .then(|| "[REDACTED]".into())
    }

    fn value(&mut self, value: &Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(key, value)| {
                        let rendered = value
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| value.to_string());
                        let value = self
                            .replacement(key, &rendered)
                            .map(Value::String)
                            .unwrap_or_else(|| self.value(value));
                        (key.clone(), value)
                    })
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(|v| self.value(v)).collect()),
            Value::String(text) => Value::String(self.text(text)),
            value => value.clone(),
        }
    }

    fn text(&mut self, text: &str) -> String {
        if let Some(replacement) = self.masked_values.get(text) {
            return replacement.clone();
        }
        // Redact embedded JSON, including JSON stored inside a message or raw evidence string.
        let mut out = String::new();
        let mut position = 0;
        while position < text.len() {
            let Some((relative, _)) = text[position..]
                .char_indices()
                .find(|(_, c)| *c == '{' || *c == '[')
            else {
                out.push_str(&text[position..]);
                break;
            };
            let start = position + relative;
            out.push_str(&text[position..start]);
            let mut stream =
                serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>();
            if let Some(Ok(value)) = stream.next() {
                let end = start + stream.byte_offset();
                out.push_str(&self.value(&value).to_string());
                position = end;
            } else {
                let ch = text[start..].chars().next().unwrap();
                out.push(ch);
                position = start + ch.len_utf8();
            }
        }
        static QUERY: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r#"([?&])([^&=\s#]+)=([^&\s#"'<>}]*)"#).unwrap());
        let query = &*QUERY;
        let out = query.replace_all(&out, |captures: &regex::Captures<'_>| {
            if let Some(replacement) = self.replacement(&captures[2], &percent_decode(&captures[3]))
            {
                format!("{}{}={}", &captures[1], &captures[2], replacement)
            } else {
                captures[0].to_string()
            }
        });
        // Also handle log-style key=value and JSON5-style key: value fragments.
        static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?i)([A-Za-z_][A-Za-z0-9_.-]*)([\"']?\s*[:=]\s*)(\"(?:\\.|[^\"\\])*\"|'(?:\\.|[^'\\])*'|[^\s&,}\]]+)"#).unwrap()
        });
        static AUTH: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(
                r#"(?i)\b(authorization|auth)(["']?\s*[:=]\s*)(?:Bearer|Basic)\s+[^\s'",}\]]+"#,
            )
            .unwrap()
        });
        let out = AUTH.replace_all(&out, |captures: &regex::Captures<'_>| {
            format!("{}{}[REDACTED]", &captures[1], &captures[2])
        });
        let assignment = &*ASSIGNMENT;
        assignment
            .replace_all(&out, |captures: &regex::Captures<'_>| {
                let raw = &captures[3];
                if raw.starts_with("[REDACTED") || raw.starts_with("[MASKED_ID:") {
                    return captures[0].to_string();
                }
                let value = serde_json::from_str::<Value>(raw)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_else(|| raw.trim_matches('\'').to_string());
                if let Some(replacement) = self.replacement(&captures[1], &value) {
                    let replacement = if raw.starts_with('"') || raw.starts_with('\'') {
                        json!(replacement).to_string()
                    } else {
                        replacement
                    };
                    format!("{}{}{}", &captures[1], &captures[2], replacement)
                } else {
                    captures[0].to_string()
                }
            })
            .into_owned()
    }

    fn report(&mut self, text: &str) -> String {
        if !self.redact || text.is_empty() {
            return text.to_string();
        }
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            let mut value = self.value(&value);
            // A second pass covers generic ID labels encountered before their named fields.
            value = self.value(&value);
            // Aggregate extraction puts a selected field's values under generic `value` keys.
            if let Some(extract) = value.get_mut("extract")
                && let Some(field) = extract
                    .get("field")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                && let Some(groups) = extract.get_mut("groups").and_then(Value::as_array_mut)
            {
                for group in groups {
                    if let Some(value) = group.get_mut("value")
                        && let Some(replacement) = self.replacement(
                            field.rsplit('.').next().unwrap_or(&field),
                            &value.to_string(),
                        )
                    {
                        *value = Value::String(replacement);
                    }
                }
            }
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "redaction".into(),
                    json!({"applied":true,"masked_id_fields":self.mask_ids,"scope":"report"}),
                );
            }
            return format!("{}\n", serde_json::to_string_pretty(&value).unwrap());
        }
        let marker = if text.starts_with("# Generated by log-analyzer") {
            "# [REDACTED OUTPUT]"
        } else {
            "[REDACTED OUTPUT]"
        };
        let text = self.text(text);
        format!("{marker}\n{}", self.text(&text))
    }
}

/// Diff values use generic field names in both text and JSON, so preserve their
/// path context while redacting the already computed result.
pub fn redact_comparison(results: &mut crate::comparator::ComparisonResults) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut().filter(|s| s.redact) else {
            return;
        };
        for comparison in &mut results.shared_comparisons {
            for diff in &mut comparison.json_differences {
                for value in [&mut diff.value1, &mut diff.value2] {
                    let rendered = value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string());
                    let replacement = diff
                        .path
                        .split('.')
                        .find_map(|key| state.replacement(key, &rendered));
                    *value = replacement
                        .map(Value::String)
                        .unwrap_or_else(|| state.value(value));
                }
            }
            for payload in [&mut comparison.log1_payload, &mut comparison.log2_payload]
                .into_iter()
                .flatten()
            {
                *payload = state.value(payload);
            }
        }
    });
}

/// Learn configured identifiers without changing entries or analysis keys.
pub fn observe_entries(entries: &[crate::parser::LogEntry]) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state
            .as_mut()
            .filter(|s| s.redact && !s.mask_ids.is_empty())
        else {
            return;
        };
        for entry in entries {
            for payload in [entry.payload(), entry.envelope_payload.as_ref()]
                .into_iter()
                .flatten()
            {
                state.value(payload);
            }
            for (key, value) in &entry.structured_fields {
                state.replacement(key, value);
            }
            if let crate::parser::LogEntryKind::Request {
                request_id: Some(id),
                ..
            } = &entry.kind
            {
                state.replacement("request_id", id);
            }
            state.text(&entry.raw_logline);
        }
    });
}

pub fn identifier(value: &str) -> String {
    STATE.with(|state| {
        state
            .borrow()
            .as_ref()
            .and_then(|s| s.masked_values.get(value))
            .cloned()
            .unwrap_or_else(|| value.to_string())
    })
}

/// Process compaction is presentation too; redact before truncating names/values.
pub fn prepare_process_entries(entries: &mut [crate::parser::LogEntry]) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut().filter(|s| s.redact) else {
            return;
        };
        for entry in entries {
            entry.message = state.text(&entry.message);
            match &mut entry.kind {
                crate::parser::LogEntryKind::Event { payload, .. }
                | crate::parser::LogEntryKind::Request { payload, .. }
                | crate::parser::LogEntryKind::Generic { payload } => {
                    if let Some(payload) = payload {
                        *payload = state.value(payload);
                    }
                }
                crate::parser::LogEntryKind::Command { settings, .. } => {
                    if let Some(settings) = settings {
                        *settings = state.value(settings);
                    }
                }
            }
        }
    });
}
