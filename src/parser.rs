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
        &config.parser,
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
        timestamp,
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
        &timestamp,
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
    source_timestamp: &str,
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

// Command identity is independent of lifecycle wording. Only quoted names have
// an unambiguous end on completion lines; retain legacy start-delimited names.
fn extract_command_name(message: &str, rules: &ParserRules) -> Option<(String, usize)> {
    let prefix = rules.command_prefix.as_str();
    if prefix.is_empty() {
        return None;
    }
    let spans = valid_json_spans(message);
    message.match_indices(prefix).find_map(|(start, _)| {
        let index = spans.partition_point(|span| span.end <= start);
        if spans.get(index).is_some_and(|span| span.contains(&start)) {
            return None;
        }
        parse_command_candidate(message, start + prefix.len(), rules)
    })
}

fn parse_command_candidate(
    message: &str,
    name_start: usize,
    rules: &ParserRules,
) -> Option<(String, usize)> {
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
    if quoted
        && after_name
            .chars()
            .next()
            .is_some_and(|ch| ch.is_alphanumeric())
        && after_name
            .split_whitespace()
            .next()
            .is_some_and(|word| word.contains(['"', '\'']))
    {
        return None;
    }
    (!command.trim().is_empty()).then_some((command, name_end))
}

pub(crate) fn command_lifecycle_message<'a>(message: &'a str, rules: &ParserRules) -> &'a str {
    let body = extract_command_name(message, rules)
        .map(|(_, end)| &message[end..])
        .unwrap_or(message);
    // Payload syntax is opaque even when malformed: its words cannot prove a
    // lifecycle boundary. Do not depend on successful JSON decoding here.
    let end = body
        .char_indices()
        .find_map(|(index, ch)| matches!(ch, '{' | '[').then_some(index))
        .unwrap_or(body.len());
    &body[..end]
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
    } else if let Some((command, name_end)) = extract_command_name(message, parser_rules) {
        let mut settings = None;
        let mut cleaned_message = message.to_string();
        for indicator in &parser_rules.command_payload_markers {
            if indicator.is_empty() {
                continue;
            }
            if let Some(relative_start) = message[name_end..].find(indicator.as_str()) {
                let start_idx = name_end + relative_start;
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
    extract_json_span_from_position(input, start_pos).map(|(value, _)| value)
}

fn extract_json_span_from_position(input: &str, start_pos: usize) -> Option<(Value, usize)> {
    let remaining = input.get(start_pos..)?;
    let span = balanced_json_spans(remaining).into_iter().next()?;
    if span.start != 0 {
        return None;
    }
    let json = remaining[span.clone()].replace("undefined", "null");
    json5::from_str::<Value>(&json)
        .ok()
        .map(|value| (value, start_pos + span.end))
}

fn valid_json_spans(input: &str) -> Vec<std::ops::Range<usize>> {
    balanced_json_spans(input)
        .into_iter()
        .filter(|span| {
            json5::from_str::<Value>(&input[span.clone()].replace("undefined", "null")).is_ok()
        })
        .collect()
}

// Scan disjoint outer spans once; unmatched opening delimiters do not trigger
// repeated suffix scans. Quotes/comments are significant only inside a span.
fn balanced_json_spans(input: &str) -> Vec<std::ops::Range<usize>> {
    let mut spans = Vec::new();
    let mut delimiters = Vec::new();
    let mut root_start = 0;
    let mut string_quote = None;
    let mut escape_next = false;
    let mut characters = input.char_indices().peekable();
    let mut line_comment = false;
    let mut block_comment = false;
    while let Some((index, ch)) = characters.next() {
        if delimiters.is_empty() {
            if matches!(ch, '{' | '[') {
                root_start = index;
                delimiters.push(ch);
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
            '"' | '\'' => string_quote = Some(ch),
            '{' | '[' => delimiters.push(ch),
            '}' | ']' => {
                let expected = if ch == '}' { '{' } else { '[' };
                if delimiters.pop() != Some(expected) {
                    delimiters.clear();
                } else if delimiters.is_empty() {
                    spans.push(root_start..index + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    spans
}
