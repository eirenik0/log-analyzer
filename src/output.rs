//! Presentation-only redaction. Analysis always sees original entries.
use aho_corasick::{AhoCorasick, MatchKind};
use regex::Regex;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
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
    "credential",
    "signature",
    "private_key",
];

#[derive(Default)]
struct OutputState {
    redact: bool,
    mask_ids: Vec<String>,
    masked_values: BTreeMap<String, String>,
    generated: HashSet<String>,
    next_id: usize,
    matcher: Option<(AhoCorasick, Vec<String>)>,
    preserve_numeric_metadata: bool,
    stdout: String,
    prepared: bool,
    compact: bool,
}

thread_local! {
    static STATE: RefCell<Option<OutputState>> = const { RefCell::new(None) };
}

/// CLI-scoped buffering lets JSON receive metadata once and text redact across fragments.
pub struct OutputGuard;
impl OutputGuard {
    pub fn new(redact: bool, mask_ids: &[String], compact: bool) -> Self {
        STATE.with(|state| {
            *state.borrow_mut() = Some(OutputState {
                redact,
                compact,
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

pub(crate) fn canonical(field: &str) -> String {
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

fn field_matches(key: &str, field: &str) -> bool {
    let decoded = percent_decode(key);
    let segments: Vec<_> = decoded.split('.').collect();
    (0..segments.len()).any(|start| canonical(&segments[start..].join(".")) == canonical(field))
}

impl OutputState {
    fn replacement(&mut self, key: &str, value: &str) -> Option<String> {
        let decoded = percent_decode(key);
        let leaf = canonical(decoded.rsplit('.').next().unwrap_or(&decoded));

        if (leaf == "id" || leaf == "correlationid")
            && let Some(replacement) = self.masked_values.get(value)
        {
            return Some(replacement.clone());
        }
        if self.mask_ids.iter().any(|field| field_matches(key, field)) {
            if value == "null" || value.is_empty() {
                return None;
            }
            if self.generated.contains(value) {
                return Some(value.to_string());
            }
            if let Some(mask) = self.masked_values.get(value) {
                return Some(mask.clone());
            }
            self.next_id += 1;
            while format!("[MASKED_ID:{}]", self.next_id) == value {
                self.next_id += 1;
            }
            let mask = format!("[MASKED_ID:{}]", self.next_id);
            self.generated.insert(mask.clone());
            self.masked_values.insert(value.to_string(), mask.clone());
            self.matcher = None;
            return Some(mask);
        }
        SECRET_FIELDS
            .iter()
            .any(|field| field_matches(key, field))
            .then(|| "[REDACTED]".into())
    }

    fn value(&mut self, value: &Value) -> Value {
        self.value_at(value, "")
    }

    fn value_at(&mut self, value: &Value, path: &str) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(key, value)| {
                        let rendered = value
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| value.to_string());
                        let field_path = if path.is_empty() {
                            key.clone()
                        } else {
                            format!("{path}.{key}")
                        };
                        let value = self
                            .replacement(&field_path, &rendered)
                            .map(Value::String)
                            .unwrap_or_else(|| self.value_at(value, &field_path));
                        (key.clone(), value)
                    })
                    .collect(),
            ),
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| self.value_at(v, path)).collect())
            }
            Value::String(text) => {
                let leaf = path.rsplit('.').next().unwrap_or(path);
                let metadata = matches!(
                    leaf,
                    "timestamp"
                        | "ts"
                        | "start"
                        | "end"
                        | "time_range"
                        | "capture_window"
                        | "start_time"
                        | "end_time"
                        | "first_timestamp"
                        | "last_timestamp"
                        | "file"
                        | "source_file"
                        | "source_row_path"
                        | "row_path"
                        | "parser"
                        | "format"
                        | "field"
                        | "op_type"
                        | "boundary"
                        | "severity"
                        | "level"
                );
                let previous = self.preserve_numeric_metadata;
                self.preserve_numeric_metadata = metadata;
                let rendered = self.text(text);
                self.preserve_numeric_metadata = previous;
                Value::String(rendered)
            }
            value => value.clone(),
        }
    }

    fn text(&mut self, text: &str) -> String {
        if !(self.preserve_numeric_metadata && text.chars().all(|c| c.is_ascii_digit()))
            && let Some(replacement) = self.masked_values.get(text)
        {
            return replacement.clone();
        }
        // Keep parsed JSON separate from plain fragments so ID masking never changes keys.
        let mut out = String::new();
        let mut raw_start = 0;
        let mut search = 0;
        while search < text.len() {
            let Some((relative, ch)) = text[search..]
                .char_indices()
                .find(|(_, c)| *c == '{' || *c == '[')
            else {
                break;
            };
            let start = search + relative;
            let mut stream =
                serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>();
            if let Some(Ok(value)) = stream.next() {
                let end = start + stream.byte_offset();
                let prefix = &text[raw_start..start];
                static FIELD: LazyLock<Regex> = LazyLock::new(|| {
                    Regex::new(r#"([A-Za-z_][A-Za-z0-9_.-]*)(["']?\s*[:=]\s*)$"#).unwrap()
                });
                let sensitive_prefix = FIELD.captures(prefix).is_some_and(|captures| {
                    SECRET_FIELDS
                        .iter()
                        .any(|field| field_matches(&captures[1], field))
                        || self
                            .mask_ids
                            .iter()
                            .any(|field| field_matches(&captures[1], field))
                });
                if sensitive_prefix
                    && text[end..]
                        .chars()
                        .next()
                        .is_some_and(|ch| !ch.is_whitespace())
                {
                    // A JSON-looking prefix with attached characters is still one scalar token.
                    search = start + ch.len_utf8();
                    continue;
                }
                let replacement = FIELD.captures(prefix).and_then(|captures| {
                    self.replacement(&captures[1], &value.to_string())
                        .map(|replacement| {
                            (
                                captures.get(0).unwrap().start(),
                                captures[1].to_string(),
                                captures[2].to_string(),
                                replacement,
                            )
                        })
                });
                if let Some((offset, key, separator, replacement)) = replacement {
                    out.push_str(&self.fragment(&prefix[..offset]));
                    out.push_str(&format!("{key}{separator}{}", json!(replacement)));
                } else {
                    out.push_str(&self.fragment(prefix));
                    out.push_str(&self.value(&value).to_string());
                }
                search = end;
                raw_start = end;
            } else {
                search = start + ch.len_utf8();
            }
        }
        out.push_str(&self.fragment(&text[raw_start..]));
        out
    }

    fn fragment(&mut self, text: &str) -> String {
        static QUERY: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r#"([?&])([^&=\s#]+)=([^&\s#"'<>}]*)"#).unwrap());
        let query = &*QUERY;
        let out = query.replace_all(text, |captures: &regex::Captures<'_>| {
            if let Some(replacement) = self.replacement(&captures[2], &percent_decode(&captures[3]))
            {
                format!("{}{}={}", &captures[1], &captures[2], replacement)
            } else {
                captures[0].to_string()
            }
        });
        // Also handle log-style key=value and JSON5-style key: value fragments.
        static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?i)([A-Za-z_][A-Za-z0-9_.-]*)([\"']?\s*[:=]\s*)(\"(?:\\.|[^\"\\])*\"|'(?:\\.|[^'\\])*'|[^\s]+)"#).unwrap()
        });
        static HEADER: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?i)\b(authorization|auth|cookie|set[_.-]?cookie)(["']?[ \t]*[:=][ \t]*)([^\r\n]+(?:\r?\n[ \t]+[^\r\n]+)*)"#).unwrap()
        });
        let out = HEADER.replace_all(&out, |captures: &regex::Captures<'_>| {
            format!("{}{}[REDACTED]", &captures[1], &captures[2])
        });
        let assignment = &*ASSIGNMENT;
        let text = assignment
            .replace_all(&out, |captures: &regex::Captures<'_>| {
                let raw = &captures[3];
                let start = captures.get(0).unwrap().start();
                let previous = out[..start].chars().next_back();
                // URL query values were already handled with their own delimiters.
                if matches!(previous, Some('?') | Some('&'))
                    || raw == "[REDACTED]"
                    || self.generated.contains(raw)
                {
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
            .into_owned();
        self.mask_plain(&text)
    }

    fn mask_plain(&mut self, text: &str) -> String {
        if self.masked_values.is_empty() {
            return text.to_string();
        }
        if self.matcher.is_none() {
            let originals: Vec<_> = self.masked_values.keys().cloned().collect();
            let matcher = AhoCorasick::builder()
                .match_kind(MatchKind::LeftmostLongest)
                .build(&originals)
                .expect("identifier patterns are valid");
            self.matcher = Some((matcher, originals));
        }
        static PLACEHOLDER: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"\[(?:MASKED_ID:\d+|REDACTED)\]").unwrap());
        let placeholders: Vec<_> = PLACEHOLDER.find_iter(text).map(|m| m.range()).collect();
        let (matcher, originals) = self.matcher.as_ref().unwrap();
        let mut out = String::new();
        let mut cursor = 0;
        let mut placeholder_index = 0;
        for found in matcher.find_iter(text) {
            let start = found.start();
            let end = found.end();
            while placeholder_index < placeholders.len()
                && placeholders[placeholder_index].end <= start
            {
                placeholder_index += 1;
            }
            let original = &originals[found.pattern().as_usize()];
            let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
            if text[..start].chars().next_back().is_some_and(is_word)
                || text[end..].chars().next().is_some_and(is_word)
                || placeholders
                    .get(placeholder_index)
                    .is_some_and(|p| p.contains(&start))
                || (self.preserve_numeric_metadata && original.chars().all(|c| c.is_ascii_digit()))
            {
                continue;
            }
            out.push_str(&text[cursor..start]);
            out.push_str(&self.masked_values[original]);
            cursor = end;
        }
        out.push_str(&text[cursor..]);
        out
    }

    // Collection deliberately does not redact prose or scan previously collected IDs.
    fn collect_value(&mut self, value: &Value, path: &str) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    let path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    if self
                        .mask_ids
                        .iter()
                        .any(|field| field_matches(&path, field))
                    {
                        let rendered = value
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| value.to_string());
                        self.replacement(&path, &rendered);
                    }
                    self.collect_value(value, &path);
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.collect_value(item, path);
                }
            }
            Value::String(text) => self.collect_text(text),
            _ => (),
        }
    }

    fn collect_text(&mut self, text: &str) {
        static NAMED: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"([A-Za-z_][A-Za-z0-9_.%-]*)(?:["']?\s*[:=]\s*)("(?:\\.|[^"\\])*"|'[^']*'|[^\s&]+)"#).unwrap()
        });
        for captures in NAMED.captures_iter(text) {
            if self
                .mask_ids
                .iter()
                .any(|field| field_matches(&captures[1], field))
            {
                let raw = &captures[2];
                let value = serde_json::from_str::<String>(raw)
                    .unwrap_or_else(|_| percent_decode(raw.trim_matches('\'')));
                self.replacement(&captures[1], &value);
            }
        }
        for (start, ch) in text
            .char_indices()
            .filter(|(_, ch)| *ch == '{' || *ch == '[')
        {
            let mut stream =
                serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>();
            if let Some(Ok(value)) = stream.next() {
                self.collect_value(&value, "");
            }
            let _ = ch;
        }
    }

    fn report(&mut self, text: &str) -> String {
        if !self.redact || text.is_empty() {
            return text.to_string();
        }
        if let Ok(value) = serde_json::from_str::<Value>(text)
            && value.is_object()
        {
            let mut value = if self.prepared {
                value
            } else {
                self.value(&value)
            };
            // A second pass covers generic ID labels encountered before their named fields.
            if !self.prepared {
                value = self.value(&value);
            }
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
                            &field,
                            &value
                                .as_str()
                                .map(str::to_string)
                                .unwrap_or_else(|| value.to_string()),
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
            let serialized = if self.compact {
                serde_json::to_string(&value)
            } else {
                serde_json::to_string_pretty(&value)
            }
            .unwrap();
            return format!("{serialized}\n");
        }
        if self.prepared {
            return text.to_string();
        }
        let marker = if text.starts_with("# Generated by log-analyzer") {
            "# [REDACTED OUTPUT]"
        } else {
            "[REDACTED OUTPUT]"
        };
        if serde_json::from_str::<Value>(text).is_ok_and(|value| value.is_number()) {
            return format!("{marker}\n{text}");
        }
        self.preserve_numeric_metadata = true;
        let text = self.text(text);
        let rendered = format!("{marker}\n{}", self.text(&text));
        self.preserve_numeric_metadata = false;
        rendered
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
            for text in [&mut comparison.text1, &mut comparison.text2]
                .into_iter()
                .flatten()
            {
                *text = state.text(text);
            }
            for diff in &mut comparison.json_differences {
                for value in [&mut diff.value1, &mut diff.value2] {
                    let rendered = value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string());
                    let replacement = state.replacement(&diff.path, &rendered).or_else(|| {
                        diff.path
                            .split('.')
                            .find_map(|key| state.replacement(key, &rendered))
                    });
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
            state.replacement("component_id", &entry.component_id);
            for payload in [entry.payload(), entry.envelope_payload.as_ref()]
                .into_iter()
                .flatten()
            {
                state.collect_value(payload, "");
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
            state.collect_text(&entry.raw_logline);
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

pub fn register_selector(value: &str) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state
            .as_mut()
            .filter(|s| s.redact && !s.mask_ids.is_empty())
        else {
            return;
        };
        let field = state.mask_ids[0].clone();
        state.replacement(&field, value);
    });
}

pub fn report_prefix() -> &'static str {
    STATE.with(|state| {
        if state.borrow().as_ref().is_some_and(|s| s.redact) {
            "[REDACTED OUTPUT]\n"
        } else {
            ""
        }
    })
}

pub fn prepare_errors(report: &mut crate::errors::ErrorAnalysisReport) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut().filter(|s| s.redact) else {
            return;
        };
        for cluster in &mut report.clusters {
            cluster.severity = state.text(&cluster.severity);
            cluster.pattern = state.text(&cluster.pattern);
            cluster.sample_message = state.text(&cluster.sample_message);
            for component in &mut cluster.components {
                *component = state.text(component);
            }
            for session in &mut cluster.affected_sessions {
                session.session_path = state.text(&session.session_path);
            }
        }
        if let Some(longest) = &mut report.longest_blocking {
            longest.severity = state.text(&longest.severity);
            longest.pattern = state.text(&longest.pattern);
            longest.session_path = state.text(&longest.session_path);
        }
    });
}

pub fn prepare_bounded_output(text: &str, json_output: bool) -> String {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut().filter(|s| s.redact) else {
            return text.to_string();
        };
        let text = if json_output {
            let mut value: Value = serde_json::from_str(text).expect("formatted report is JSON");
            if let Some(coverage) = value.get_mut("coverage") {
                *coverage = state.value(coverage);
            }
            value.to_string()
        } else {
            text.to_string()
        };
        state.prepared = true;
        state.report(&text)
    })
}

pub fn redaction_enabled() -> bool {
    STATE.with(|state| state.borrow().as_ref().is_some_and(|s| s.redact))
}

/// Preserve typed counts/timestamps while preparing source strings for text rendering.
pub fn prepare_performance(results: &mut crate::perf_analyzer::PerfAnalysisResults) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Some(state) = state.as_mut().filter(|s| s.redact) {
            let value = serde_json::to_value(&*results).expect("performance results serialize");
            *results = serde_json::from_value(state.value(&value))
                .expect("redaction preserves typed performance fields");
        }
    });
}

pub(crate) fn byte_prefix(text: &str, max_bytes: usize) -> &str {
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
