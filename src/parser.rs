use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeZone};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::LazyLock;

use crate::config::{AnalyzerConfig, LogFormat, ParserRules, contains_any_marker, default_config};

mod entities;

pub use entities::{
    CommandLogParams, EventDirection, EventLogParams, LogEntry, LogEntryBase, LogEntryKind,
    RequestDirection, RequestLogParams, create_command_log, create_event_log, create_generic_log,
    create_request_log,
};

static CLASSIC_ENTRY_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[\w-]+(?:\s+\([^)]*\))?\s+\|\s+\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}")
        .expect("valid classic log entry start regex")
});
static RUST_TRACING_ENTRY_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\d{4}-\d{2}-\d{2}[T ][0-9:.+-]+(?:Z|[+-]\d{2}:?\d{2})?\s+(?i:trace|debug|info|warn|warning|error|fatal)\b",
    )
    .expect("valid rust tracing start regex")
});
static RUST_TRACING_ENTRY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<timestamp>\d{4}-\d{2}-\d{2}[T ][0-9:.+-]+(?:Z|[+-]\d{2}:?\d{2})?)\s+(?P<level>(?i:trace|debug|info|warn|warning|error|fatal))\s+(?P<module>[A-Za-z0-9_:.:-]+):\s*(?P<rest>[\s\S]*)$",
    )
    .expect("valid rust tracing regex")
});
static SYSLOG_ENTRY_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:\d{4}-\d{2}-\d{2}[T ][0-9:.+-]+(?:Z|[+-]\d{2}:?\d{2})?|[A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2})\s+\S+\s+\S+(?:\[\d+\])?:",
    )
    .expect("valid syslog start regex")
});
static SYSLOG_ENTRY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<timestamp>(?:\d{4}-\d{2}-\d{2}[T ][0-9:.+-]+(?:Z|[+-]\d{2}:?\d{2})?)|(?:[A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}))\s+(?P<host>\S+)\s+(?P<process>[^\s:\[]+)(?:\[(?P<pid>\d+)\])?:\s*(?P<message>[\s\S]*)$",
    )
    .expect("valid syslog regex")
});
static LEVEL_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<level>(?i:trace|debug|info|warn|warning|error|fatal))\b")
        .expect("valid level prefix regex")
});
static FIELD_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z_][A-Za-z0-9_.:-]*$").expect("valid structured field key regex")
});

/// Parse error types
#[derive(Debug)]
pub enum ParseError {
    IoError(std::io::Error),
    InvalidLogFormat(String),
    JsonParseError(String),
}

impl From<std::io::Error> for ParseError {
    fn from(err: std::io::Error) -> Self {
        ParseError::IoError(err)
    }
}

/// Parses a log file into a vector of LogEntry structs
pub fn parse_log_file(path: impl AsRef<Path>) -> Result<Vec<LogEntry>, ParseError> {
    parse_log_file_with_config(path, default_config())
}

/// Detect the most likely log format for a file using the active parser config.
pub fn detect_log_format(
    path: impl AsRef<Path>,
    config: &AnalyzerConfig,
) -> Result<LogFormat, ParseError> {
    if config.parser.format != LogFormat::Auto {
        return Ok(config.parser.format);
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut samples = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        samples.push(line);
        if samples.len() >= 10 {
            break;
        }
    }

    Ok(detect_format_from_lines(
        samples.iter().map(String::as_str),
        config.parser.format,
    ))
}

/// Parses a log file into a vector of LogEntry structs using explicit analyzer config
pub fn parse_log_file_with_config(
    path: impl AsRef<Path>,
    config: &AnalyzerConfig,
) -> Result<Vec<LogEntry>, ParseError> {
    let path = path.as_ref();
    let format = detect_log_format(path, config)?;
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut logs = Vec::new();

    if format == LogFormat::JsonLines {
        for (index, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            match parse_log_entry_in_format(&line, index + 1, config, format) {
                Ok(entry) => logs.push(entry),
                Err(ParseError::InvalidLogFormat(_)) => {}
                Err(err) => return Err(err),
            }
        }

        return Ok(logs);
    }

    let mut current_log: Option<String> = None;
    let mut current_line_number = 0usize;

    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line?;

        if line_starts_entry(&line, format) {
            if let Some(log_text) = current_log.take() {
                match parse_log_entry_in_format(&log_text, current_line_number, config, format) {
                    Ok(entry) => logs.push(entry),
                    Err(ParseError::InvalidLogFormat(_)) => {}
                    Err(err) => return Err(err),
                }
            }

            current_log = Some(line);
            current_line_number = line_number;
        } else if let Some(ref mut log_text) = current_log {
            log_text.push('\n');
            log_text.push_str(&line);
        }
    }

    if let Some(log_text) = current_log
        && let Ok(entry) = parse_log_entry_in_format(&log_text, current_line_number, config, format)
    {
        logs.push(entry);
    }

    Ok(logs)
}

/// Parses a single log entry string into a LogEntry struct
pub fn parse_log_entry(log_text: &str, source_line_number: usize) -> Result<LogEntry, ParseError> {
    parse_log_entry_with_config(log_text, source_line_number, default_config())
}

/// Parses a single log entry string into a LogEntry struct using explicit analyzer config
pub fn parse_log_entry_with_config(
    log_text: &str,
    source_line_number: usize,
    config: &AnalyzerConfig,
) -> Result<LogEntry, ParseError> {
    let format = detect_format_from_lines(log_text.lines(), config.parser.format);
    parse_log_entry_in_format(log_text, source_line_number, config, format)
}

fn detect_format_from_lines<'a>(
    lines: impl IntoIterator<Item = &'a str>,
    configured_format: LogFormat,
) -> LogFormat {
    if configured_format != LogFormat::Auto {
        return configured_format;
    }

    let mut classic = 0usize;
    let mut rust_tracing = 0usize;
    let mut syslog = 0usize;
    let mut json_lines = 0usize;

    for line in lines
        .into_iter()
        .filter(|line| !line.trim().is_empty())
        .take(10)
    {
        let trimmed = line.trim();

        if CLASSIC_ENTRY_START.is_match(line) {
            classic += 1;
        }
        if RUST_TRACING_ENTRY.is_match(line) {
            rust_tracing += 1;
        }
        if SYSLOG_ENTRY.is_match(line) {
            syslog += 1;
        }
        if looks_like_json_line(trimmed) {
            json_lines += 1;
        }
    }

    let candidates = [
        (LogFormat::Classic, classic),
        (LogFormat::RustTracing, rust_tracing),
        (LogFormat::Syslog, syslog),
        (LogFormat::JsonLines, json_lines),
    ];

    let (format, score) = candidates
        .into_iter()
        .max_by(|(format_a, score_a), (format_b, score_b)| {
            score_a
                .cmp(score_b)
                .then_with(|| format_priority(*format_a).cmp(&format_priority(*format_b)))
        })
        .unwrap_or((LogFormat::Classic, 0));

    if score == 0 {
        LogFormat::Classic
    } else {
        format
    }
}

fn format_priority(format: LogFormat) -> u8 {
    match format {
        LogFormat::Classic => 4,
        LogFormat::RustTracing => 3,
        LogFormat::Syslog => 2,
        LogFormat::JsonLines => 1,
        LogFormat::Auto => 0,
    }
}

fn line_starts_entry(line: &str, format: LogFormat) -> bool {
    match format {
        LogFormat::Classic => CLASSIC_ENTRY_START.is_match(line),
        LogFormat::RustTracing => RUST_TRACING_ENTRY_START.is_match(line),
        LogFormat::Syslog => SYSLOG_ENTRY_START.is_match(line),
        LogFormat::JsonLines => looks_like_json_line(line.trim()),
        LogFormat::Auto => false,
    }
}

fn looks_like_json_line(trimmed: &str) -> bool {
    trimmed.starts_with('{')
        && serde_json::from_str::<Value>(trimmed)
            .ok()
            .is_some_and(|value| value.is_object())
}

fn parse_log_entry_in_format(
    log_text: &str,
    source_line_number: usize,
    config: &AnalyzerConfig,
    format: LogFormat,
) -> Result<LogEntry, ParseError> {
    match format {
        LogFormat::Classic => parse_classic_log_entry(log_text, source_line_number, config),
        LogFormat::RustTracing => {
            parse_rust_tracing_log_entry(log_text, source_line_number, config)
        }
        LogFormat::Syslog => parse_syslog_log_entry(log_text, source_line_number, config),
        LogFormat::JsonLines => parse_json_line_entry(log_text, source_line_number, config),
        LogFormat::Auto => parse_classic_log_entry(log_text, source_line_number, config),
    }
}

fn parse_classic_log_entry(
    log_text: &str,
    source_line_number: usize,
    config: &AnalyzerConfig,
) -> Result<LogEntry, ParseError> {
    let mut parts = log_text.splitn(2, " | ");

    let component_part = parts
        .next()
        .ok_or_else(|| ParseError::InvalidLogFormat("Missing component section".to_string()))?;
    let (component, component_id) = extract_component_info(component_part);

    let rest = parts
        .next()
        .ok_or_else(|| ParseError::InvalidLogFormat("Missing log message section".to_string()))?;

    let (timestamp_str, level, message) = extract_log_parts(rest)
        .ok_or_else(|| ParseError::InvalidLogFormat("Invalid classic log format".to_string()))?;

    build_log_entry(
        component.to_string(),
        component_id.to_string(),
        parse_timestamp(timestamp_str)?,
        normalize_level(level),
        message.to_string(),
        log_text.to_string(),
        source_line_number,
        &config.parser,
        HashMap::new(),
        None,
        None,
    )
}

fn parse_rust_tracing_log_entry(
    log_text: &str,
    source_line_number: usize,
    config: &AnalyzerConfig,
) -> Result<LogEntry, ParseError> {
    let captures = RUST_TRACING_ENTRY.captures(log_text).ok_or_else(|| {
        ParseError::InvalidLogFormat("Invalid rust tracing log format".to_string())
    })?;

    let timestamp = captures
        .name("timestamp")
        .map(|m| m.as_str())
        .ok_or_else(|| ParseError::InvalidLogFormat("Missing timestamp".to_string()))?;
    let level = captures
        .name("level")
        .map(|m| m.as_str())
        .ok_or_else(|| ParseError::InvalidLogFormat("Missing level".to_string()))?;
    let module_path = captures
        .name("module")
        .map(|m| m.as_str())
        .ok_or_else(|| ParseError::InvalidLogFormat("Missing module path".to_string()))?;
    let rest = captures
        .name("rest")
        .map(|m| m.as_str())
        .unwrap_or_default();

    let (message, structured_fields) = split_tracing_message_and_fields(rest);
    let component = map_module_path_to_component(module_path, &config.parser);

    build_log_entry(
        component,
        String::new(),
        parse_timestamp(timestamp)?,
        normalize_level(level),
        message,
        log_text.to_string(),
        source_line_number,
        &config.parser,
        structured_fields,
        Some(module_path.to_string()),
        None,
    )
}

fn parse_syslog_log_entry(
    log_text: &str,
    source_line_number: usize,
    config: &AnalyzerConfig,
) -> Result<LogEntry, ParseError> {
    let captures = SYSLOG_ENTRY
        .captures(log_text)
        .ok_or_else(|| ParseError::InvalidLogFormat("Invalid syslog log format".to_string()))?;

    let timestamp = captures
        .name("timestamp")
        .map(|m| m.as_str())
        .ok_or_else(|| ParseError::InvalidLogFormat("Missing timestamp".to_string()))?;
    let host = captures
        .name("host")
        .map(|m| m.as_str())
        .ok_or_else(|| ParseError::InvalidLogFormat("Missing host".to_string()))?;
    let process = captures
        .name("process")
        .map(|m| m.as_str())
        .ok_or_else(|| ParseError::InvalidLogFormat("Missing process".to_string()))?;
    let pid = captures.name("pid").map(|m| m.as_str()).unwrap_or_default();
    let message = captures
        .name("message")
        .map(|m| m.as_str())
        .unwrap_or_default()
        .to_string();

    let mut structured_fields = HashMap::new();
    structured_fields.insert("host".to_string(), host.to_string());
    if !pid.is_empty() {
        structured_fields.insert("pid".to_string(), pid.to_string());
    }

    build_log_entry(
        process.to_string(),
        pid.to_string(),
        parse_timestamp(timestamp)?,
        infer_level_from_text(&message),
        message,
        log_text.to_string(),
        source_line_number,
        &config.parser,
        structured_fields,
        None,
        None,
    )
}

fn parse_json_line_entry(
    log_text: &str,
    source_line_number: usize,
    config: &AnalyzerConfig,
) -> Result<LogEntry, ParseError> {
    let value: Value = serde_json::from_str(log_text)
        .map_err(|err| ParseError::JsonParseError(err.to_string()))?;
    let object = value.as_object().ok_or_else(|| {
        ParseError::InvalidLogFormat("JSON log line must be an object".to_string())
    })?;

    let timestamp = json_string_field(object, &["timestamp", "@timestamp", "ts", "time"])
        .ok_or_else(|| {
            ParseError::InvalidLogFormat("JSON log line missing timestamp".to_string())
        })?;
    let level = json_scalar_field(object, &["level", "lvl", "severity"])
        .unwrap_or_else(|| "INFO".to_string());
    let module_path = json_string_field(object, &["module_path", "module", "target", "logger"]);
    let component = json_string_field(object, &["component", "service", "source"])
        .or_else(|| {
            module_path
                .as_deref()
                .map(|path| map_module_path_to_component(path, &config.parser))
        })
        .unwrap_or_else(|| "json".to_string());
    let component_id =
        json_string_field(object, &["component_id", "session_id"]).unwrap_or_default();
    let payload = object
        .get("payload")
        .cloned()
        .or_else(|| object.get("fields").cloned());

    let message = json_string_field(object, &["message", "msg", "event"])
        .or_else(|| {
            payload
                .as_ref()
                .and_then(|value| serde_json::to_string(value).ok())
        })
        .unwrap_or_else(|| log_text.to_string());

    let mut structured_fields = HashMap::new();
    for (key, value) in object {
        if matches!(
            key.as_str(),
            "timestamp"
                | "@timestamp"
                | "ts"
                | "time"
                | "level"
                | "lvl"
                | "severity"
                | "message"
                | "msg"
                | "event"
                | "component"
                | "component_id"
                | "service"
                | "source"
                | "module_path"
                | "module"
                | "target"
                | "logger"
                | "payload"
                | "fields"
        ) {
            continue;
        }

        if let Some(value) = json_to_field_string(value) {
            structured_fields.insert(key.clone(), value);
        }
    }

    build_log_entry(
        component,
        component_id,
        parse_timestamp(&timestamp)?,
        normalize_level(&level),
        message,
        log_text.to_string(),
        source_line_number,
        &config.parser,
        structured_fields,
        module_path,
        payload,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_log_entry(
    component: String,
    component_id: String,
    timestamp: DateTime<Local>,
    level: String,
    message: String,
    raw_logline: String,
    source_line_number: usize,
    parser_rules: &ParserRules,
    structured_fields: HashMap<String, String>,
    module_path: Option<String>,
    payload_override: Option<Value>,
) -> Result<LogEntry, ParseError> {
    let mut entry = determine_log_entry_kind(
        component,
        component_id,
        timestamp,
        level,
        message.clone(),
        raw_logline,
        &message,
        source_line_number,
        parser_rules,
    )?;

    if let Some(payload) = payload_override
        && let LogEntryKind::Generic { payload: existing } = &mut entry.kind
        && existing.is_none()
    {
        *existing = Some(payload);
    }

    entry.structured_fields = structured_fields;
    entry.module_path = module_path;
    Ok(entry)
}

fn extract_component_info(component_part: &str) -> (&str, &str) {
    if let Some(space_pos) = component_part.find(' ') {
        let component = &component_part[..space_pos];
        if component_part.len() > space_pos + 2
            && component_part.as_bytes()[space_pos + 1] == b'('
            && component_part.ends_with(')')
        {
            let component_id = &component_part[space_pos + 2..component_part.len() - 1];
            return (component, component_id);
        }
    }
    (component_part, "")
}

fn extract_log_parts(rest: &str) -> Option<(&str, &str, &str)> {
    let timestamp_end = rest.find('[')?;
    let timestamp = rest[..timestamp_end].trim();

    let level_start = timestamp_end + 1;
    let level_end = rest[level_start..].find(']')? + level_start;
    let level = &rest[level_start..level_end].trim();

    let message_start = level_end + 2;
    let message = if message_start < rest.len() {
        &rest[message_start..]
    } else {
        ""
    };

    Some((timestamp, level, message))
}

fn parse_timestamp(timestamp: &str) -> Result<DateTime<Local>, ParseError> {
    DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.with_timezone(&Local))
        .or_else(|_| timestamp.parse::<DateTime<Local>>())
        .or_else(|_| parse_local_naive(timestamp, "%Y-%m-%d %H:%M:%S%.f").ok_or(()))
        .or_else(|_| parse_local_naive(timestamp, "%Y-%m-%dT%H:%M:%S%.f").ok_or(()))
        .or_else(|_| parse_local_naive(timestamp, "%Y-%m-%d %H:%M:%S").ok_or(()))
        .or_else(|_| parse_local_naive(timestamp, "%Y-%m-%dT%H:%M:%S").ok_or(()))
        .or_else(|_| parse_syslog_timestamp(timestamp).ok_or(()))
        .map_err(|_| ParseError::InvalidLogFormat(format!("Invalid timestamp '{}'", timestamp)))
}

fn parse_local_naive(timestamp: &str, format: &str) -> Option<DateTime<Local>> {
    let naive = NaiveDateTime::parse_from_str(timestamp, format).ok()?;
    localize_naive_datetime(&naive)
}

fn parse_syslog_timestamp(timestamp: &str) -> Option<DateTime<Local>> {
    let year = Local::now().year();
    let naive =
        NaiveDateTime::parse_from_str(&format!("{year} {timestamp}"), "%Y %b %e %H:%M:%S").ok()?;
    localize_naive_datetime(&naive)
}

fn localize_naive_datetime(naive: &NaiveDateTime) -> Option<DateTime<Local>> {
    Local
        .from_local_datetime(naive)
        .single()
        .or_else(|| Local.from_local_datetime(naive).earliest())
}

fn normalize_level(level: &str) -> String {
    level.trim().to_ascii_uppercase()
}

fn infer_level_from_text(message: &str) -> String {
    LEVEL_PREFIX_RE
        .captures(message.trim_start())
        .and_then(|caps| {
            caps.name("level")
                .map(|value| value.as_str().to_ascii_uppercase())
        })
        .unwrap_or_else(|| "INFO".to_string())
}

fn map_module_path_to_component(module_path: &str, parser_rules: &ParserRules) -> String {
    let mut segments: Vec<String> = module_path
        .split("::")
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect();

    if let Some(first) = segments.first_mut()
        && !parser_rules.module_strip_prefix.is_empty()
        && let Some(stripped) = first.strip_prefix(&parser_rules.module_strip_prefix)
    {
        *first = stripped.to_string();
    }

    segments.retain(|segment| !segment.is_empty());
    if segments.is_empty() {
        return module_path.to_string();
    }

    let depth = parser_rules.module_depth.max(1);
    if segments.len() > depth {
        segments = segments[segments.len() - depth..].to_vec();
    }

    segments.join("::")
}

fn split_tracing_message_and_fields(rest: &str) -> (String, HashMap<String, String>) {
    let trimmed = rest.trim_end();
    if trimmed.is_empty() {
        return (String::new(), HashMap::new());
    }

    let mut boundaries = vec![0usize];
    boundaries.extend(
        trimmed
            .char_indices()
            .filter_map(|(index, ch)| ch.is_whitespace().then_some(index + ch.len_utf8())),
    );

    for boundary in boundaries {
        let suffix = trimmed[boundary..].trim_start();
        if suffix.is_empty() {
            continue;
        }

        if let Some(fields) = parse_structured_fields(suffix) {
            let message = trimmed[..boundary].trim_end().to_string();
            return (message, fields);
        }
    }

    (trimmed.to_string(), HashMap::new())
}

fn parse_structured_fields(input: &str) -> Option<HashMap<String, String>> {
    let mut fields = HashMap::new();
    let mut remaining = input.trim();
    let mut parsed_any = false;

    while !remaining.is_empty() {
        let separator = remaining.find('=')?;
        let key = remaining[..separator].trim();
        if !FIELD_KEY_RE.is_match(key) {
            return None;
        }

        let (value, consumed) = parse_field_value(&remaining[separator + 1..])?;
        fields.insert(key.to_string(), value);
        parsed_any = true;

        if separator + 1 + consumed >= remaining.len() {
            remaining = "";
        } else {
            remaining = remaining[separator + 1 + consumed..].trim_start();
        }
    }

    parsed_any.then_some(fields)
}

fn parse_field_value(input: &str) -> Option<(String, usize)> {
    let mut chars = input.char_indices();
    let (_, first) = chars.next()?;

    match first {
        '"' | '\'' => parse_quoted_field_value(input, first),
        '{' | '[' | '(' => parse_balanced_field_value(input, first),
        _ => {
            let end = input
                .char_indices()
                .find_map(|(index, ch)| ch.is_whitespace().then_some(index))
                .unwrap_or(input.len());
            Some((input[..end].to_string(), end))
        }
    }
}

fn parse_quoted_field_value(input: &str, quote: char) -> Option<(String, usize)> {
    let mut escape_next = false;
    for (index, ch) in input.char_indices().skip(1) {
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' {
            escape_next = true;
            continue;
        }
        if ch == quote {
            return Some((
                unescape_quoted_value(&input[1..index], quote),
                index + quote.len_utf8(),
            ));
        }
    }
    None
}

fn parse_balanced_field_value(input: &str, opener: char) -> Option<(String, usize)> {
    let closer = match opener {
        '{' => '}',
        '[' => ']',
        '(' => ')',
        _ => return None,
    };

    let mut depth = 0usize;
    let mut in_string = false;
    let mut string_quote = '\0';
    let mut escape_next = false;

    for (index, ch) in input.char_indices() {
        if in_string {
            if escape_next {
                escape_next = false;
                continue;
            }
            if ch == '\\' {
                escape_next = true;
                continue;
            }
            if ch == string_quote {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' => {
                in_string = true;
                string_quote = ch;
            }
            c if c == opener => depth += 1,
            c if c == closer => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((input[..=index].to_string(), index + ch.len_utf8()));
                }
            }
            _ => {}
        }
    }

    None
}

fn unescape_quoted_value(value: &str, quote: char) -> String {
    value
        .replace("\\\\", "\\")
        .replace(&format!("\\{quote}"), &quote.to_string())
}

fn json_string_field(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(|value| value.as_str().map(ToString::to_string))
    })
}

fn json_scalar_field(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(json_to_field_string))
}

fn json_to_field_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("null".to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::String(value) => Some(value.clone()),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).ok(),
    }
}

/// Determines the type of log entry based on the message content
#[allow(clippy::too_many_arguments)]
fn determine_log_entry_kind(
    component: String,
    component_id: String,
    timestamp: DateTime<Local>,
    level: String,
    mut message_text: String,
    raw_logline: String,
    message: &str,
    source_line_number: usize,
    parser_rules: &ParserRules,
) -> Result<LogEntry, ParseError> {
    if contains_any_marker(message, &parser_rules.event_emit_markers) {
        let event_parts: Vec<&str> = message
            .splitn(2, &parser_rules.event_payload_separator)
            .collect();
        if event_parts.len() >= 2 {
            let event_type = extract_event_type(event_parts[0]).ok_or_else(|| {
                ParseError::InvalidLogFormat("Could not extract event type".to_string())
            })?;

            let payload_str = event_parts[1].trim();
            let payload = extract_json(payload_str, &parser_rules.json_indicators);

            message_text = format!(
                "{} {} [JSON removed]",
                event_parts[0], parser_rules.event_payload_separator
            );

            return Ok(create_event_log(EventLogParams {
                base: LogEntryBase {
                    component,
                    component_id,
                    timestamp,
                    level,
                    message: message_text,
                    raw_logline,
                    source_line_number,
                },
                event_type,
                direction: EventDirection::Emit,
                payload,
            }));
        }
    } else if contains_any_marker(message, &parser_rules.event_receive_markers) {
        let event_parts: Vec<&str> = message
            .splitn(2, &parser_rules.event_payload_separator)
            .collect();
        if event_parts.len() >= 2 {
            let event_type = extract_event_type(event_parts[0]).ok_or_else(|| {
                ParseError::InvalidLogFormat("Could not extract event type".to_string())
            })?;

            let payload_str = event_parts[1].trim();
            let payload = extract_json(payload_str, &parser_rules.json_indicators);

            message_text = format!(
                "{} {} [JSON removed]",
                event_parts[0], parser_rules.event_payload_separator
            );

            return Ok(create_event_log(EventLogParams {
                base: LogEntryBase {
                    component,
                    component_id,
                    timestamp,
                    level,
                    message: message_text,
                    raw_logline,
                    source_line_number,
                },
                event_type,
                direction: EventDirection::Receive,
                payload,
            }));
        }
    } else if message.contains(&parser_rules.command_prefix)
        && message.contains(&parser_rules.command_start_marker)
    {
        let cmd_prefix = parser_rules.command_prefix.as_str();
        let cmd_suffix = parser_rules.command_start_marker.as_str();

        if let Some(start_idx) = message.find(cmd_prefix) {
            let cmd_name_start = start_idx + cmd_prefix.len();
            if let Some(end_idx) = message[cmd_name_start..].find(cmd_suffix) {
                let command = message[cmd_name_start..cmd_name_start + end_idx].to_string();

                let mut settings = None;
                let mut cleaned_message = message.to_string();

                for indicator in &parser_rules.command_payload_markers {
                    if indicator.is_empty() {
                        continue;
                    }

                    if let Some(start_idx) = message.find(indicator.as_str()) {
                        let settings_start = start_idx + indicator.len() - 1;
                        let settings_str = &message[settings_start..];
                        settings = extract_json(settings_str, &parser_rules.json_indicators);

                        cleaned_message = message[..start_idx].to_string();
                        cleaned_message.push_str(indicator);
                        cleaned_message.push_str(" [JSON removed]");
                        break;
                    }
                }

                message_text = cleaned_message;
                return Ok(create_command_log(CommandLogParams {
                    base: LogEntryBase {
                        component,
                        component_id,
                        timestamp,
                        level,
                        message: message_text,
                        raw_logline,
                        source_line_number,
                    },
                    command,
                    settings,
                }));
            }
        }
    } else if message.contains(&parser_rules.request_prefix) {
        let (request_name, request_id, endpoint, direction, payload) =
            extract_request_info(message, parser_rules);

        if let Some(req_name) = request_name {
            let mut cleaned_message = message.to_string();
            for indicator in parser_rules
                .command_payload_markers
                .iter()
                .chain(parser_rules.request_payload_markers.iter())
            {
                if let Some(start_idx) = message.find(indicator.as_str()) {
                    cleaned_message = message[..start_idx].to_string();
                    cleaned_message.push_str(indicator);
                    cleaned_message.push_str(" [JSON removed]");
                    break;
                }
            }

            message_text = cleaned_message;

            return Ok(create_request_log(RequestLogParams {
                base: LogEntryBase {
                    component,
                    component_id,
                    timestamp,
                    level,
                    message: message_text,
                    raw_logline,
                    source_line_number,
                },
                request: req_name,
                request_id,
                endpoint,
                direction,
                payload,
            }));
        }
    }

    let payload = extract_json(message, &parser_rules.json_indicators);

    if payload.is_some() {
        let mut cleaned_message = String::new();
        for (index, ch) in message.char_indices() {
            if (ch == '{' || ch == '[') && extract_json_from_position(message, index).is_some() {
                cleaned_message = message[..index].to_string();
                cleaned_message.push_str("[JSON removed]");
                break;
            }
        }

        if !cleaned_message.is_empty() {
            message_text = cleaned_message;
        }
    }

    Ok(create_generic_log(
        component,
        component_id,
        timestamp,
        level,
        message_text,
        raw_logline,
        payload,
        source_line_number,
    ))
}

fn extract_request_info(
    message: &str,
    parser_rules: &ParserRules,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    RequestDirection,
    Option<Value>,
) {
    let mut request_name = None;
    let mut request_id = None;
    let mut endpoint = None;
    let mut direction = RequestDirection::Send;
    let mut payload = None;

    let req_prefix = parser_rules.request_prefix.as_str();
    if let Some(start_idx) = message.find(req_prefix) {
        let req_name_start = start_idx + req_prefix.len();
        if let Some(end_idx) = message[req_name_start..].find('"') {
            request_name = Some(message[req_name_start..req_name_start + end_idx].to_string());

            let after_name = req_name_start + end_idx + 1;
            if after_name < message.len() {
                let rest = &message[after_name..];
                if rest.starts_with(" [")
                    && let Some(id_end) = rest[2..].find(']')
                {
                    let potential_id = &rest[2..2 + id_end];
                    if potential_id.contains("--") && !potential_id.contains(' ') {
                        request_id = Some(potential_id.to_string());
                    }
                }
            }
        }
    }

    if let Some(addr_start) = message.find(&parser_rules.request_endpoint_marker) {
        let addr_content_start = addr_start + parser_rules.request_endpoint_marker.len();
        if let Some(addr_end) = message[addr_content_start..].find(']') {
            endpoint = Some(message[addr_content_start..addr_content_start + addr_end].to_string());
        }
    }

    if contains_any_marker(message, &parser_rules.request_receive_markers) {
        direction = RequestDirection::Receive;
    } else if contains_any_marker(message, &parser_rules.request_send_markers) {
        direction = RequestDirection::Send;
    }

    for indicator in &parser_rules.request_payload_markers {
        if let Some(start_idx) = message.find(indicator.as_str()) {
            let body_content = &message[start_idx + indicator.len()..];
            payload = extract_json(body_content, &parser_rules.json_indicators);
            break;
        }
    }

    (request_name, request_id, endpoint, direction, payload)
}

fn extract_event_type(event_part: &str) -> Option<String> {
    if event_part.contains("\"name\":") {
        if let Some(start) = event_part.find('{')
            && let Some(end) = event_part.find('}')
        {
            let type_json = &event_part[start..=end];
            if let Ok(value) = serde_json::from_str::<Value>(type_json)
                && let Some(name) = value.get("name")
            {
                return Some(name.as_str().unwrap_or("").to_string());
            }
        }
    } else if let Some(start) = event_part.find('"')
        && let Some(end) = event_part[start + 1..].find('"')
    {
        return Some(event_part[start + 1..start + 1 + end].to_string());
    }

    None
}

fn extract_json(input: &str, json_indicators: &[String]) -> Option<Value> {
    for indicator in json_indicators {
        if indicator.is_empty() {
            continue;
        }
        if let Some(marker_pos) = input.find(indicator.as_str()) {
            let start_pos = marker_pos + indicator.len();

            let json_start = if indicator == "with body" || indicator == "with body " {
                let mut index = None;
                for (offset, ch) in input[start_pos..].char_indices() {
                    if ch == '[' || ch == '{' {
                        index = Some(start_pos + offset);
                        break;
                    }
                }
                index
            } else {
                Some(start_pos.saturating_sub(1))
            };

            if let Some(start_idx) = json_start
                && let Some(json_value) = extract_json_from_position(input, start_idx)
            {
                return Some(json_value);
            }
        }
    }

    for (index, ch) in input.char_indices() {
        if (ch == '{' || ch == '[')
            && let Some(json_value) = extract_json_from_position(input, index)
        {
            return Some(json_value);
        }
    }

    None
}

fn extract_json_from_position(input: &str, start_pos: usize) -> Option<Value> {
    if start_pos >= input.len() {
        return None;
    }

    let first_char = input[start_pos..].chars().next()?;
    if first_char != '{' && first_char != '[' {
        return None;
    }

    let mut brace_count = 0;
    let mut bracket_count = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (index, ch) in input[start_pos..].char_indices() {
        if in_string {
            if escape_next {
                escape_next = false;
                continue;
            }
            if ch == '\\' {
                escape_next = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => brace_count += 1,
            '}' => {
                brace_count -= 1;
                if brace_count == 0 && first_char == '{' && bracket_count == 0 {
                    let json_str =
                        input[start_pos..=start_pos + index].replace("undefined", "null");
                    return json5::from_str::<Value>(&json_str).ok();
                }
            }
            '[' => bracket_count += 1,
            ']' => {
                bracket_count -= 1;
                if bracket_count == 0 && first_char == '[' && brace_count == 0 {
                    let json_str =
                        input[start_pos..=start_pos + index].replace("undefined", "null");
                    return json5::from_str::<Value>(&json_str).ok();
                }
            }
            _ => {}
        }
    }

    None
}
