use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeZone};
use regex::Regex;
use serde::Serialize;
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
static CONSOLE_SOURCE_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<source>\S+:\d+(?::\d+)?)\s+")
        .expect("valid browser console source prefix regex")
});

fn split_console_source(line: &str) -> (&str, Option<&str>) {
    if let Some(captures) = CONSOLE_SOURCE_PREFIX.captures(line) {
        let prefix = captures.get(0).expect("matched prefix");
        return (
            &line[prefix.end()..],
            captures.name("source").map(|value| value.as_str()),
        );
    }
    (line, None)
}

fn classic_entry_start(line: &str) -> bool {
    CLASSIC_ENTRY_START.is_match(split_console_source(line).0)
}

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
    NoRecognizedEntries(ParseCoverage),
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

/// Coverage is measured before filters or severity selection.
#[derive(Debug, Clone, Serialize)]
pub struct ParseCoverage {
    pub file: String,
    pub profile: String,
    pub configured_parser: LogFormat,
    pub selected_parser: LogFormat,
    pub input_bytes: u64,
    pub nonempty_lines: usize,
    pub parsed_entries: usize,
    pub rejected_candidates: usize,
    pub normalization_diagnostics: Vec<crate::normalize::RowDiagnostic>,
}

impl ParseCoverage {
    pub fn is_unparsed(&self) -> bool {
        self.nonempty_lines > 0 && self.parsed_entries == 0
    }
}

#[derive(Debug)]
pub struct ParsedLogFile {
    pub entries: Vec<LogEntry>,
    pub coverage: ParseCoverage,
}

/// Parse with coverage, retaining diagnostics even when no entries are recognized.
pub fn parse_log_file_report(
    path: impl AsRef<Path>,
    config: &AnalyzerConfig,
) -> Result<ParsedLogFile, ParseError> {
    let path = path.as_ref();
    let format = if config.normalization.is_some() {
        LogFormat::JsonLines
    } else {
        detect_log_format(path, config)?
    };
    let file = File::open(path)?;
    let mut coverage = ParseCoverage {
        file: path.display().to_string(),
        profile: config.profile_name.clone(),
        configured_parser: config.parser.format,
        selected_parser: format,
        input_bytes: file.metadata()?.len(),
        nonempty_lines: 0,
        parsed_entries: 0,
        rejected_candidates: 0,
        normalization_diagnostics: Vec::new(),
    };
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    let mut current_log: Option<String> = None;
    let mut current_line_number = 0;

    let mut finish = |text: &str, line_number: usize| {
        if let Some(rules) = &config.normalization {
            for (row_path, row) in crate::normalize::normalize(text, line_number, rules) {
                let parsed = row.and_then(|value| {
                    parse_json_line_entry(&value.to_string(), line_number, config).map_err(|_| {
                        crate::normalize::RowDiagnostic {
                            line: line_number,
                            row_path: row_path.clone(),
                            field: "timestamp_or_message".into(),
                            reason: "invalid_normalized_entry".into(),
                        }
                    })
                });
                match parsed {
                    Ok(mut entry) => {
                        entry.normalized_record = Some(entry.raw_logline.clone());
                        entry.source_file = Some(path.display().to_string());
                        entry.source_row_path = Some(row_path);
                        entry.raw_logline = text.to_string();
                        entries.push(entry);
                    }
                    Err(diagnostic) => {
                        coverage.rejected_candidates += 1;
                        coverage.normalization_diagnostics.push(diagnostic);
                    }
                }
            }
            return;
        }

        if format != LogFormat::JsonLines && !line_starts_entry(text, format) {
            coverage.rejected_candidates += 1;
            return;
        }
        match parse_log_entry_in_format(text, line_number, config, format) {
            Ok(mut entry) => {
                entry.source_file = Some(path.display().to_string());
                entries.push(entry);
            }
            Err(_) => coverage.rejected_candidates += 1,
        }
    };

    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if !line.trim().is_empty() {
            coverage.nonempty_lines += 1;
        }
        if format == LogFormat::JsonLines {
            if !line.trim().is_empty() {
                finish(&line, index + 1);
            }
        } else if line_starts_entry(&line, format) || looks_like_entry_candidate(&line) {
            if let Some(text) = current_log.take() {
                finish(&text, current_line_number);
            }
            current_log = Some(line);
            current_line_number = index + 1;
        } else if let Some(text) = &mut current_log {
            text.push('\n');
            text.push_str(&line);
        } else if !line.trim().is_empty() {
            // An unrecognized leading block is one candidate, not one per stack frame.
            current_log = Some(line);
            current_line_number = index + 1;
        }
    }
    if let Some(text) = current_log {
        finish(&text, current_line_number);
    }
    coverage.parsed_entries = entries.len();
    Ok(ParsedLogFile { entries, coverage })
}

/// Reject nonempty files with no recognized entries for all parser callers.
pub fn parse_log_file_with_config(
    path: impl AsRef<Path>,
    config: &AnalyzerConfig,
) -> Result<Vec<LogEntry>, ParseError> {
    let parsed = parse_log_file_report(path.as_ref(), config)?;
    if parsed.coverage.is_unparsed() {
        return Err(ParseError::NoRecognizedEntries(parsed.coverage));
    }
    for diagnostic in &parsed.coverage.normalization_diagnostics {
        report_eprintln!(
            "Normalization skipped {}:{} row {} field {}: {}",
            path.as_ref().display(),
            diagnostic.line,
            diagnostic.row_path,
            diagnostic.field,
            diagnostic.reason
        );
    }
    Ok(parsed.entries)
}

fn looks_like_entry_candidate(line: &str) -> bool {
    static STRUCTURED_CANDIDATE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"^(?:\S+[:#]\S+\s+)?[\w-]+(?:\s+\([^)]*\))?\s+\|\s+\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}",
        )
        .expect("valid entry candidate regex")
    });
    STRUCTURED_CANDIDATE.is_match(line)
        || RUST_TRACING_ENTRY_START.is_match(line)
        || SYSLOG_ENTRY_START.is_match(line)
        || (line.starts_with('{')
            && serde_json::from_str::<Value>(line)
                .ok()
                .is_some_and(|value| {
                    ["timestamp", "@timestamp", "ts", "time"]
                        .iter()
                        .any(|key| value.get(key).is_some())
                }))
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

        if classic_entry_start(line) {
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
        LogFormat::Classic => classic_entry_start(line),
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
    let (first_line, console_source) =
        split_console_source(log_text.lines().next().unwrap_or_default());
    let normalized;
    let classic_text = if console_source.is_some() {
        // Normalize continuation prefixes for payload parsing; raw text stays untouched.
        normalized = std::iter::once(first_line)
            .chain(
                log_text
                    .lines()
                    .skip(1)
                    .map(|line| split_console_source(line).0),
            )
            .collect::<Vec<_>>()
            .join("\n");
        normalized.as_str()
    } else {
        log_text
    };
    let mut parts = classic_text.splitn(2, " | ");

    let component_part = parts
        .next()
        .ok_or_else(|| ParseError::InvalidLogFormat("Missing component section".to_string()))?;
    let (component, component_id) = extract_component_info(component_part);

    let rest = parts
        .next()
        .ok_or_else(|| ParseError::InvalidLogFormat("Missing log message section".to_string()))?;

    let (timestamp_str, level, message) = extract_log_parts(rest)
        .ok_or_else(|| ParseError::InvalidLogFormat("Invalid classic log format".to_string()))?;

    let mut structured_fields = HashMap::new();
    if let Some(source) = console_source {
        structured_fields.insert("console_source".to_string(), source.to_string());
    }

    build_log_entry(
        component.to_string(),
        component_id.to_string(),
        parse_timestamp(timestamp_str)?,
        timestamp_str,
        normalize_level(level),
        message.to_string(),
        log_text.to_string(),
        source_line_number,
        config,
        structured_fields,
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
        timestamp,
        normalize_level(level),
        message,
        log_text.to_string(),
        source_line_number,
        config,
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
        timestamp,
        infer_level_from_text(&message),
        message,
        log_text.to_string(),
        source_line_number,
        config,
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
        &timestamp,
        normalize_level(&level),
        message,
        log_text.to_string(),
        source_line_number,
        config,
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
    source_timestamp: &str,
    level: String,
    message: String,
    raw_logline: String,
    source_line_number: usize,
    config: &AnalyzerConfig,
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
        config,
    )?;

    entry.source_timestamp = DateTime::parse_from_rfc3339(source_timestamp).ok();
    entry.timestamp_year_inferred = !source_timestamp
        .trim()
        .as_bytes()
        .get(..4)
        .is_some_and(|year| year.iter().all(u8::is_ascii_digit));
    entry.envelope_payload = payload_override.clone();
    if let Some(payload) = payload_override {
        let existing = match &mut entry.kind {
            LogEntryKind::Generic { payload }
            | LogEntryKind::Event { payload, .. }
            | LogEntryKind::Request { payload, .. } => payload,
            LogEntryKind::Command { settings, .. } => settings,
        };
        if existing.is_none() {
            *existing = Some(payload);
        }
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

pub(crate) fn lifecycle_word_char(ch: char) -> bool {
    if ch.is_alphanumeric() || ch == '_' {
        return true;
    }
    if ch.is_ascii() {
        return false;
    }
    static WORD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\w$").expect("word character regex"));
    let mut encoded = [0; 4];
    WORD.is_match(ch.encode_utf8(&mut encoded))
}

fn command_prefix_boundary(message: &str, start: usize, prefix: &str) -> bool {
    !prefix.chars().next().is_some_and(lifecycle_word_char)
        || !message[..start]
            .chars()
            .next_back()
            .is_some_and(lifecycle_word_char)
}

// Command identity is independent of lifecycle wording. Only quoted names have
// an unambiguous end on completion lines; retain legacy start-delimited names.
fn extract_command_name(message: &str, config: &AnalyzerConfig) -> Option<(String, usize)> {
    let rules = &config.parser;
    let prefix = rules.command_prefix.as_str();
    if prefix.is_empty() {
        return None;
    }
    let (spans, quotes) = opaque_spans(message);
    // Subject quotes are parsed by the candidate parser, which can recover from
    // an earlier malformed name. Other quoted context cannot contain subjects.
    let subject_quotes: Vec<_> = message
        .match_indices(prefix)
        .filter(|(start, _)| command_prefix_boundary(message, *start, prefix))
        .filter_map(|(start, _)| {
            let end = start + prefix.len();
            if prefix.ends_with(['"', '\'']) {
                Some(end - 1)
            } else {
                let remaining = message[end..].trim_start();
                remaining
                    .starts_with(['"', '\''])
                    .then_some(message.len() - remaining.len())
            }
        })
        .collect();
    let assignments = finish_spans(
        metadata_assignment_spans(message, &quotes)
            .into_iter()
            .filter_map(|span| {
                let field = &message[span.clone()];
                let separator = field.find(['=', ':'])?;
                let value = field[separator + 1..].trim_start();
                Some(span.end - value.len()..span.end)
            })
            .collect(),
        true,
    );
    let unfinished_payload = unfinished_payload_start(message, rules, &spans, &quotes);
    let request_subject = (!rules.request_prefix.is_empty())
        .then(|| {
            message
                .match_indices(rules.request_prefix.as_str())
                .filter(|(start, _)| {
                    command_prefix_boundary(message, *start, &rules.request_prefix)
                })
                .find_map(|(start, _)| {
                    if unfinished_payload.is_some_and(|boundary| start >= boundary)
                        || [&spans, &quotes, &assignments].iter().any(|ranges| {
                            let index = ranges.partition_point(|span| span.end <= start);
                            ranges.get(index).is_some_and(|span| span.contains(&start))
                        })
                    {
                        return None;
                    }
                    let end = start + rules.request_prefix.len();
                    let name = if rules.request_prefix.ends_with('"') {
                        &message[end - 1..]
                    } else {
                        message[end..].trim_start()
                    };
                    let (name, _) = parse_quoted_field_value(name, '"')?;
                    (!name.trim().is_empty()).then_some(start)
                })
        })
        .flatten();
    let mut candidates = message
        .match_indices(prefix)
        .filter(|(start, _)| command_prefix_boundary(message, *start, prefix))
        .filter_map(|(start, _)| {
            if unfinished_payload.is_some_and(|boundary| start >= boundary)
                || request_subject.is_some_and(|request| request < start)
            {
                return None;
            }
            let index = spans.partition_point(|span| span.end <= start);
            if spans.get(index).is_some_and(|span| span.contains(&start)) {
                return None;
            }
            let index = quotes.partition_point(|span| span.end <= start);
            if quotes.get(index).is_some_and(|span| {
                span.contains(&start) && subject_quotes.binary_search(&span.start).is_err()
            }) {
                return None;
            }
            let index = assignments.partition_point(|span| span.end <= start);
            if assignments
                .get(index)
                .is_some_and(|span| span.contains(&start))
            {
                return None;
            }
            parse_command_candidate(message, start + prefix.len(), config)
        });
    let candidate = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    Some(candidate)
}

fn unfinished_payload_start(
    message: &str,
    rules: &ParserRules,
    spans: &[std::ops::Range<usize>],
    quotes: &[std::ops::Range<usize>],
) -> Option<usize> {
    let openers: Vec<_> = message
        .char_indices()
        .filter_map(|(index, ch)| {
            let quote = quotes.partition_point(|span| span.end <= index);
            (matches!(ch, '{' | '[')
                && !quotes.get(quote).is_some_and(|span| span.contains(&index)))
            .then_some(index)
        })
        .collect();
    let mut boundary = None;
    for marker in rules
        .command_payload_markers
        .iter()
        .chain(&rules.request_payload_markers)
        .filter(|marker| !marker.is_empty())
    {
        for (start, _) in message.match_indices(marker) {
            let payload = spans.partition_point(|span| span.end <= start);
            if spans.get(payload).is_some_and(|span| span.contains(&start)) {
                continue;
            }
            let quote = quotes.partition_point(|span| span.end <= start);
            if quotes.get(quote).is_some_and(|span| span.contains(&start)) {
                continue;
            }
            let probe = start + marker.len() - usize::from(marker.ends_with(['{', '[']));
            let index = openers.partition_point(|opener| *opener < probe);
            if let Some(&opener) = openers.get(index) {
                let index = spans.partition_point(|span| span.end <= opener);
                if !spans.get(index).is_some_and(|span| span.contains(&opener)) {
                    boundary =
                        Some(boundary.map_or(opener, |previous: usize| previous.min(opener)));
                }
            }
        }
    }
    boundary
}

fn parse_command_candidate(
    message: &str,
    name_start: usize,
    config: &AnalyzerConfig,
) -> Option<(String, usize)> {
    let rules = &config.parser;
    let prefix = rules.command_prefix.as_str();
    let (command, name_end, quoted) = if let Some(quote @ ('"' | '\'')) = prefix.chars().next_back()
    {
        let quote_start = name_start - quote.len_utf8();
        let (name, consumed) = parse_quoted_field_value(&message[quote_start..], quote)?;
        (name, quote_start + consumed, true)
    } else {
        let remaining = message[name_start..].trim_start();
        let offset = message.len() - remaining.len();
        if let Some(quote @ ('"' | '\'')) = remaining.chars().next() {
            let (name, consumed) = parse_quoted_field_value(remaining, quote)?;
            (name, offset + consumed, true)
        } else {
            if rules.command_start_marker.is_empty() {
                return None;
            }
            let end = remaining.find(&rules.command_start_marker)?;
            (remaining[..end].trim_end().to_string(), offset + end, false)
        }
    };
    let after_name = &message[name_end..];
    if quoted && after_name.chars().next().is_some_and(lifecycle_word_char) {
        let adjacent_marker = config
            .perf
            .command_start_markers
            .iter()
            .chain(&config.perf.command_completion_markers)
            .map(String::as_str)
            .chain(std::iter::once(rules.command_start_marker.as_str()))
            .filter(|marker| !marker.is_empty())
            .any(|marker| {
                after_name.strip_prefix(marker).is_some_and(|remaining| {
                    !marker.chars().next_back().is_some_and(lifecycle_word_char)
                        || !remaining.chars().next().is_some_and(lifecycle_word_char)
                })
            });
        if !adjacent_marker
            || after_name
                .split_whitespace()
                .next()
                .is_some_and(|word| word.contains(['"', '\'']))
        {
            return None;
        }
    }
    (!command.trim().is_empty()).then_some((command, name_end))
}

pub(crate) fn command_lifecycle_message<'a>(
    message: &'a str,
    config: &AnalyzerConfig,
) -> std::borrow::Cow<'a, str> {
    let body = extract_command_name(message, config)
        .map(|(_, end)| &message[end..])
        .unwrap_or(message);
    // Payload syntax is opaque even when malformed: its words cannot prove a
    // lifecycle boundary. Do not depend on successful JSON decoding here.
    let (_, quotes) = opaque_spans(body);
    let end = body
        .char_indices()
        .find_map(|(index, ch)| {
            let quote = quotes.partition_point(|span| span.end <= index);
            (matches!(ch, '{' | '[')
                && !quotes.get(quote).is_some_and(|span| span.contains(&index)))
            .then_some(index)
        })
        .unwrap_or(body.len());
    let payload_end = end;
    let end = body[..end]
        .char_indices()
        .find_map(|(index, ch)| {
            let quote = quotes.partition_point(|span| span.end <= index);
            if ch != ':' || quotes.get(quote).is_some_and(|span| span.contains(&index)) {
                return None;
            }
            let before = body[..index].trim_end();
            let value = body[index + 1..].split_whitespace().next().unwrap_or("");
            let value = value
                .trim_matches(|ch: char| {
                    !ch.is_alphanumeric() && !matches!(ch, '.' | '-' | '+' | '_')
                })
                .to_ascii_lowercase();
            if matches!(value.as_str(), "true" | "false" | "null") || value.parse::<f64>().is_ok() {
                return None;
            }
            config
                .perf
                .command_start_markers
                .iter()
                .chain(&config.perf.command_completion_markers)
                .any(|marker| {
                    !marker.is_empty()
                        && before.strip_suffix(marker).is_some_and(|prefix| {
                            prefix.chars().next_back().is_none_or(char::is_whitespace)
                        })
                })
                .then_some(index)
        })
        .unwrap_or(end);
    let question_tail = end < payload_end && lifecycle_question(&body[end..payload_end]);
    let body = &body[..end];
    let excluded = finish_spans(
        quotes
            .iter()
            .filter(|span| span.start < end)
            .cloned()
            .chain(metadata_assignment_spans(body, &quotes))
            .collect(),
        true,
    );
    if excluded.is_empty() {
        return if question_tail {
            std::borrow::Cow::Owned(format!("{body} ?"))
        } else {
            std::borrow::Cow::Borrowed(body)
        };
    }
    let mut visible = String::with_capacity(end);
    let mut previous = 0;
    for quote in &excluded {
        visible.push_str(&body[previous..quote.start]);
        visible.push(' ');
        previous = quote.end.min(end);
    }
    visible.push_str(&body[previous..]);
    if question_tail {
        visible.push_str(" ?");
    }
    std::borrow::Cow::Owned(visible)
}

// Assignment values are metadata, not evidence of an operation boundary.
fn metadata_assignment_spans(
    text: &str,
    quotes: &[std::ops::Range<usize>],
) -> Vec<std::ops::Range<usize>> {
    let mut spans = Vec::new();
    for (index, ch) in text
        .char_indices()
        .filter(|(_, ch)| matches!(ch, '=' | ':'))
    {
        let quote = quotes.partition_point(|span| span.end <= index);
        if quotes.get(quote).is_some_and(|span| span.contains(&index)) {
            continue;
        }
        let before = text[..index].trim_end();
        let key_start = before
            .char_indices()
            .rev()
            .find_map(|(start, ch)| {
                (!(ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.')))
                    .then_some(start + ch.len_utf8())
            })
            .unwrap_or(0);
        let key = &before[key_start..];
        if !key
            .chars()
            .next()
            .is_some_and(|ch| ch.is_alphabetic() || ch == '_')
        {
            continue;
        }
        let value = text[index + ch.len_utf8()..].trim_start();
        let value_start = text.len() - value.len();
        let quote = quotes.partition_point(|span| span.end <= value_start);
        let quoted_value = quotes.get(quote).filter(|span| span.contains(&value_start));
        let value_end = quoted_value.map_or_else(
            || {
                value
                    .char_indices()
                    .find_map(|(offset, ch)| {
                        (ch.is_whitespace() || matches!(ch, ',' | ';'))
                            .then_some(value_start + offset)
                    })
                    .unwrap_or(text.len())
            },
            |span| span.end.min(text.len()),
        );
        let first_word = value[..value_end - value_start]
            .trim_matches(|ch: char| !ch.is_alphanumeric() && !matches!(ch, '\'' | '’'))
            .to_lowercase();
        let value_end = if quoted_value.is_none() && value.starts_with('(') {
            let mut depth = 0usize;
            value
                .char_indices()
                .find_map(|(offset, ch)| {
                    let absolute = value_start + offset;
                    let quote = quotes.partition_point(|span| span.end <= absolute);
                    if quotes
                        .get(quote)
                        .is_some_and(|span| span.contains(&absolute))
                    {
                        return None;
                    }
                    match ch {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                return Some(absolute + ch.len_utf8());
                            }
                        }
                        _ => {}
                    }
                    None
                })
                .unwrap_or(text.len())
        } else if quoted_value.is_none()
            && lifecycle_words(&first_word).any(|word| {
                matches!(
                    word,
                    "not"
                        | "no"
                        | "to"
                        | "only"
                        | "anything"
                        | "all"
                        | "is"
                        | "was"
                        | "were"
                        | "are"
                        | "am"
                        | "has"
                        | "had"
                        | "have"
                        | "be"
                        | "been"
                        | "being"
                        | "got"
                        | "get"
                        | "gets"
                        | "getting"
                        | "became"
                        | "become"
                        | "becomes"
                        | "remains"
                        | "remain"
                ) || lifecycle_qualifier(word)
            })
        {
            // A qualified multiword value cannot expose a later phase word.
            value
                .char_indices()
                .find(|&(offset, ch)| {
                    matches!(ch, ',' | ';' | '?') || lifecycle_sentence_boundary(value, offset, ch)
                })
                .map_or(text.len(), |(offset, _)| value_start + offset)
        } else {
            value_end
        };
        spans.push(key_start..value_end);
    }
    spans
}

pub(crate) fn lifecycle_sentence_boundary(text: &str, index: usize, ch: char) -> bool {
    matches!(ch, '!' | '\n')
        || (ch == '.'
            && text[index + ch.len_utf8()..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace))
}

pub(crate) fn lifecycle_question(suffix: &str) -> bool {
    let (_, quotes) = opaque_spans(suffix);
    for (index, ch) in suffix.char_indices() {
        let quote = quotes.partition_point(|span| span.end <= index);
        if quotes.get(quote).is_some_and(|span| span.contains(&index)) {
            continue;
        }
        if ch == '?' {
            return true;
        }
        if lifecycle_sentence_boundary(suffix, index, ch) {
            break;
        }
    }
    false
}

pub(crate) fn lifecycle_words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|ch: char| !(ch.is_alphanumeric() || matches!(ch, '_' | '\'' | '’')))
        .filter(|word| !word.is_empty())
}

pub(crate) fn lifecycle_qualifier(word: &str) -> bool {
    matches!(
        word,
        "never"
            | "without"
            | "neither"
            | "nor"
            | "cannot"
            | "if"
            | "unless"
            | "whether"
            | "when"
            | "whenever"
            | "once"
            | "until"
            | "will"
            | "would"
            | "should"
            | "may"
            | "might"
            | "could"
            | "can"
            | "shall"
            | "must"
            | "yet"
            | "pending"
            | "awaiting"
            | "scheduled"
            | "planned"
            | "expected"
            | "being"
            | "almost"
            | "nearly"
            | "partially"
            | "partly"
            | "awaits"
            | "needs"
            | "requires"
    ) || word.ends_with("n't")
        || word.ends_with("n’t")
        || lifecycle_uncertainty(word)
}

pub(crate) fn lifecycle_uncertainty(word: &str) -> bool {
    matches!(
        word,
        "unlikely"
            | "likely"
            | "maybe"
            | "perhaps"
            | "possibly"
            | "probably"
            | "potentially"
            | "presumably"
            | "apparently"
            | "reportedly"
            | "allegedly"
            | "supposedly"
            | "uncertain"
            | "unconfirmed"
            | "seems"
            | "seemed"
            | "seemingly"
            | "appears"
            | "appeared"
            | "looks"
            | "believed"
            | "assumed"
            | "thought"
            | "suspected"
            | "think"
            | "thinks"
            | "believe"
            | "believes"
            | "assume"
            | "assumes"
            | "suspect"
            | "suspects"
            | "guess"
            | "guessed"
            | "estimate"
            | "estimated"
            | "infer"
            | "inferred"
            | "predict"
            | "predicted"
            | "presume"
            | "presumed"
            | "suppose"
            | "supposed"
            | "doubt"
            | "doubts"
            | "doubtful"
            | "unable"
            | "impossible"
            | "improbable"
            | "unsure"
            | "unclear"
            | "unverified"
            | "unproven"
    )
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
    config: &AnalyzerConfig,
) -> Result<LogEntry, ParseError> {
    let parser_rules = &config.parser;
    if !parser_rules.event_payload_separator.is_empty()
        && contains_any_marker(message, &parser_rules.event_emit_markers)
    {
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
    } else if !parser_rules.event_payload_separator.is_empty()
        && contains_any_marker(message, &parser_rules.event_receive_markers)
    {
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
    } else if let Some((command, name_end)) = extract_command_name(message, config) {
        let mut settings = None;
        let mut cleaned_message = message.to_string();
        let (_, quotes) = opaque_spans(message);
        for indicator in &parser_rules.command_payload_markers {
            if indicator.is_empty() {
                continue;
            }
            if let Some(start_idx) = message[name_end..]
                .match_indices(indicator.as_str())
                .map(|(relative_start, _)| name_end + relative_start)
                .find(|start| {
                    let quote = quotes.partition_point(|span| span.end <= *start);
                    if quotes.get(quote).is_some_and(|span| span.contains(start)) {
                        return false;
                    }
                    // Marker words alone do not justify truncating the body.
                    // Require an actual payload opener after the marker.
                    indicator.ends_with(['{', '['])
                        || message[*start + indicator.len()..]
                            .trim_start_matches(|ch: char| {
                                ch.is_whitespace() || matches!(ch, '=' | ':')
                            })
                            .starts_with(['{', '['])
                })
            {
                let settings_start =
                    start_idx + indicator.len() - indicator.chars().next_back().unwrap().len_utf8();
                settings = extract_json(&message[settings_start..], &parser_rules.json_indicators);
                cleaned_message = message[..start_idx].to_string();
                cleaned_message.push_str(indicator);
                cleaned_message.push_str(" [JSON removed]");
                break;
            }
        }
        return Ok(create_command_log(CommandLogParams {
            base: LogEntryBase {
                component,
                component_id,
                timestamp,
                level,
                message: cleaned_message,
                raw_logline,
                source_line_number,
            },
            command,
            settings,
        }));
    } else if !parser_rules.request_prefix.is_empty()
        && message.contains(&parser_rules.request_prefix)
    {
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

    if payload.is_some()
        && let Some((_, span)) = first_decodable_json_span(message)
    {
        message_text = format!("{}[JSON removed]", &message[..span.start]);
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

    if !parser_rules.request_endpoint_marker.is_empty()
        && let Some(addr_start) = message.find(&parser_rules.request_endpoint_marker)
    {
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

    first_decodable_json_span(input).map(|(value, _)| value)
}

fn first_decodable_json_span(input: &str) -> Option<(Value, std::ops::Range<usize>)> {
    let mut after_deep_payload = 0;
    for span in balanced_json_spans(input) {
        if span.start < after_deep_payload {
            continue;
        }
        if scan_payload_spans(&input[span.clone()], false).2 > 128 {
            // Do not decode a nested fragment of an unsupported outer payload.
            after_deep_payload = span.end;
            continue;
        }
        if let Some(value) = decode_json_span(input, span.clone()) {
            return Some((value, span));
        }
    }
    None
}

fn extract_json_from_position(input: &str, start_pos: usize) -> Option<Value> {
    extract_json_span_from_position(input, start_pos).map(|(value, _)| value)
}

fn extract_json_span_from_position(input: &str, start_pos: usize) -> Option<(Value, usize)> {
    let remaining = input.get(start_pos..)?;
    let span = balanced_json_spans(remaining).into_iter().next()?;
    if span.start != 0 {
        return None;
    }
    let end = start_pos + span.end;
    decode_json_span(remaining, span).map(|value| (value, end))
}

fn decode_json_span(input: &str, span: std::ops::Range<usize>) -> Option<Value> {
    let json = &input[span];
    // JSON5 decoding is recursive. Match serde_json's default depth limit
    // before handing deeply nested input to that decoder.
    if scan_payload_spans(json, false).2 > 128 {
        return None;
    }
    json5::from_str::<Value>(&json.replace("undefined", "null")).ok()
}

fn looks_like_json_start(input: &str) -> bool {
    let Some(opener) = input.chars().next() else {
        return false;
    };
    let rest = input[opener.len_utf8()..].trim_start();
    if opener == '[' {
        // An array opener is itself a valid first array value. Do not rescan
        // nested opener chains once for every unfinished delimiter.
        if rest.starts_with('[') {
            return true;
        }
        if rest.starts_with('{') {
            return looks_like_json_start(rest);
        }
        return rest.starts_with(['"', '\'', '+', '-', '.', '/', ']'])
            || rest.chars().next().is_some_and(|ch| ch.is_ascii_digit())
            || ["null", "true", "false", "undefined", "Infinity", "NaN"]
                .iter()
                .any(|value| rest.starts_with(value));
    }
    if rest.starts_with(['"', '\'', '/', '}']) {
        return true;
    }
    let end = rest
        .char_indices()
        .find_map(|(index, ch)| {
            (!(ch.is_alphanumeric() || matches!(ch, '_' | '$' | '\\'))).then_some(index)
        })
        .unwrap_or(rest.len());
    end > 0 && rest[end..].trim_start().starts_with(':')
}

// Scan disjoint outer spans once, retaining unfinished JSON-like payloads.
// Unmatched contextual opening delimiters do not trigger
// repeated suffix scans. Contextual quotes are retained separately from JSON.
fn balanced_json_spans(input: &str) -> Vec<std::ops::Range<usize>> {
    scan_payload_spans(input, false).0
}

fn opaque_spans(input: &str) -> (Vec<std::ops::Range<usize>>, Vec<std::ops::Range<usize>>) {
    let (spans, quotes, _) = scan_payload_spans(input, true);
    (spans, quotes)
}

fn scan_payload_spans(
    input: &str,
    include_unfinished: bool,
) -> (
    Vec<std::ops::Range<usize>>,
    Vec<std::ops::Range<usize>>,
    usize,
) {
    let mut max_depth = 0;
    let mut spans = Vec::new();
    let mut quotes = Vec::new();
    let mut quote_start = 0;
    let mut delimiters = Vec::new();
    let mut outside_quote = None;
    let mut outside_escape = false;
    let mut string_quote = None;
    let mut escape_next = false;
    let mut characters = input.char_indices().peekable();
    let mut line_comment = false;
    let mut block_comment = false;
    while let Some((index, ch)) = characters.next() {
        if delimiters.is_empty() {
            if let Some(quote) = outside_quote {
                if outside_escape {
                    outside_escape = false;
                } else if ch == '\\' {
                    outside_escape = true;
                } else if ch == quote {
                    quotes.push(quote_start..index + ch.len_utf8());
                    outside_quote = None;
                }
                continue;
            }
            if ch == '"'
                || (ch == '\''
                    && !input[..index]
                        .chars()
                        .next_back()
                        .is_some_and(|previous| previous.is_alphanumeric()))
            {
                quote_start = index;
                outside_quote = Some(ch);
                continue;
            }
            if matches!(ch, '{' | '[') {
                delimiters.push((ch, index));
                max_depth = max_depth.max(delimiters.len());
            }
            continue;
        }
        if line_comment {
            if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                line_comment = false;
            }
            continue;
        }
        if block_comment {
            if ch == '*' && characters.peek().is_some_and(|(_, next)| *next == '/') {
                characters.next();
                block_comment = false;
            }
            continue;
        }
        if let Some(quote) = string_quote {
            if escape_next {
                escape_next = false;
            } else if ch == '\\' {
                escape_next = true;
            } else if ch == quote {
                quotes.push(quote_start..index + ch.len_utf8());
                string_quote = None;
            }
            continue;
        }
        match ch {
            '/' if characters.peek().is_some_and(|(_, next)| *next == '/') => {
                characters.next();
                line_comment = true;
            }
            '/' if characters.peek().is_some_and(|(_, next)| *next == '*') => {
                characters.next();
                block_comment = true;
            }
            '\'' if input[..index]
                .chars()
                .next_back()
                .is_some_and(|previous| previous.is_alphanumeric()) => {}
            '"' | '\'' => {
                quote_start = index;
                string_quote = Some(ch);
            }
            '{' | '[' => {
                delimiters.push((ch, index));
                max_depth = max_depth.max(delimiters.len());
            }
            '}' | ']' => {
                let expected = if ch == '}' { '{' } else { '[' };
                let Some((opener, start)) = delimiters.pop() else {
                    continue;
                };
                if opener != expected {
                    if let Some(opaque_start) = delimiters
                        .iter()
                        .map(|(_, start)| *start)
                        .chain(std::iter::once(start))
                        .find(|start| looks_like_json_start(&input[*start..]))
                    {
                        if include_unfinished {
                            spans.push(opaque_start..input.len());
                        }
                        return (finish_spans(spans, include_unfinished), quotes, max_depth);
                    }
                    delimiters.clear();
                } else {
                    // Retain nested balanced regions even if their enclosing
                    // stray opener never closes or is not JSON-like.
                    spans.push(start..index + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    if let Some(start) = delimiters
        .iter()
        .map(|(_, start)| *start)
        .find(|start| looks_like_json_start(&input[*start..]))
        && include_unfinished
    {
        spans.push(start..input.len());
    }
    if outside_quote.is_some() || string_quote.is_some() {
        quotes.push(quote_start..input.len());
    }
    (finish_spans(spans, include_unfinished), quotes, max_depth)
}

fn finish_spans(
    mut spans: Vec<std::ops::Range<usize>>,
    merge: bool,
) -> Vec<std::ops::Range<usize>> {
    spans.sort_unstable_by_key(|span| span.start);
    if !merge {
        return spans;
    }
    let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
    for span in spans {
        if let Some(previous) = merged
            .last_mut()
            .filter(|previous| span.start <= previous.end)
        {
            previous.end = previous.end.max(span.end);
        } else {
            merged.push(span);
        }
    }
    merged
}
