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
    Regex::new(r"^[\w/-]+(?:\s+\([^)]*\))?\s+\|\s+\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}")
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
    NoRecognizedEntries(Box<ParseCoverage>),
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

/// Structural observations do not establish event semantics or capture completeness.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FormatMatches {
    pub classic: usize,
    pub rust_tracing: usize,
    pub syslog: usize,
    pub json_lines: usize,
}
impl FormatMatches {
    fn observe(&mut self, line: &str) {
        self.classic += usize::from(classic_entry_start(line));
        self.rust_tracing += usize::from(RUST_TRACING_ENTRY.is_match(line));
        self.syslog += usize::from(SYSLOG_ENTRY.is_match(line));
        self.json_lines += usize::from(looks_like_json_line(line.trim()));
    }
    fn status(&self) -> &'static str {
        let scores = [
            self.classic,
            self.rust_tracing,
            self.syslog,
            self.json_lines,
        ];
        let max = scores.into_iter().max().unwrap_or(0);
        if max == 0 {
            "no_match"
        } else if scores.into_iter().filter(|score| *score == max).count() > 1 {
            "tied"
        } else if scores.into_iter().filter(|score| *score > 0).count() > 1 {
            "mixed"
        } else {
            "single_format"
        }
    }
    fn any(&self) -> bool {
        self.classic + self.rust_tracing + self.syslog + self.json_lines > 0
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StructuralDiagnostic {
    pub line: usize,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct StructuralDiagnostics {
    pub version: u32,
    pub selection: &'static str,
    pub sampled_nonempty_lines: usize,
    pub sample_format_matches: FormatMatches,
    pub sample_status: &'static str,
    pub observed_format_matches: FormatMatches,
    pub observed_status: &'static str,
    pub unsupported_python_headers: usize,
    pub physical_candidate_blocks: usize,
    pub attached_nonempty_lines: usize,
    pub blank_lines: usize,
    pub diagnostics: Vec<StructuralDiagnostic>,
    pub diagnostic_count: usize,
    pub omitted_diagnostics: usize,
    pub limitations: Vec<&'static str>,
}
impl StructuralDiagnostics {
    fn new(selection: &'static str, samples: &[String]) -> Self {
        let mut sample_format_matches = FormatMatches::default();
        let mut sampled_nonempty_lines = 0;
        for line in samples.iter().filter(|line| !line.trim().is_empty()) {
            sample_format_matches.observe(line);
            sampled_nonempty_lines += 1;
        }
        let sample_status = if selection == "automatic_sample" {
            sample_format_matches.status()
        } else {
            "not_sampled"
        };
        Self {
            version: 1,
            selection,
            sampled_nonempty_lines,
            sample_status,
            sample_format_matches,
            observed_format_matches: FormatMatches::default(),
            observed_status: "no_match",
            unsupported_python_headers: 0,
            physical_candidate_blocks: 0,
            attached_nonempty_lines: 0,
            blank_lines: 0,
            diagnostics: Vec::new(),
            diagnostic_count: 0,
            omitted_diagnostics: 0,
            limitations: vec![
                "structure_does_not_establish_event_semantics",
                "sample_does_not_establish_capture_completeness",
                "attached_lines_are_not_validated_continuations",
            ],
        }
    }
    fn reject(&mut self, line: usize, reason: &'static str) {
        self.diagnostic_count += 1;
        if self.diagnostics.len() < 20 {
            self.diagnostics.push(StructuralDiagnostic { line, reason });
        } else {
            self.omitted_diagnostics += 1;
        }
    }
}

fn unsupported_python_header(line: &str) -> bool {
    static PYTHON_HEADER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
        r"^\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}(?:[.,]\d+)?(?:Z|[+-]\d{2}:?\d{2})?\s+(?:\[(?i:trace|debug|info|warn|warning|error|critical|fatal)\]|(?i:trace|debug|info|warn|warning|error|critical|fatal))\s+\S"
    ).expect("valid unsupported Python-style header regex")
    });
    PYTHON_HEADER.is_match(line)
        && !RUST_TRACING_ENTRY.is_match(line)
        && !SYSLOG_ENTRY.is_match(line)
}

#[allow(clippy::too_many_arguments)]
fn finish_candidate(
    text: &str,
    line_number: usize,
    format: LogFormat,
    path: &Path,
    config: &AnalyzerConfig,
    entries: &mut Vec<LogEntry>,
    coverage: &mut ParseCoverage,
    controls: &mut Option<&mut crate::processing::Budget>,
) {
    if let Some(budget) = controls.as_deref_mut()
        && !budget.record(
            text.len(),
            config.normalization.as_ref().map_or(0, |r| r.fields.len()),
            false,
        )
    {
        return;
    }
    coverage.structural_diagnostics.physical_candidate_blocks += 1;
    if let Some(rules) = &config.normalization {
        let mut first_row = true;
        let controls = std::cell::RefCell::new(controls.as_deref_mut());
        crate::normalize::visit_normalized(
            text,
            line_number,
            rules,
            |_, _| {
                controls.borrow_mut().as_deref_mut().is_none_or(|budget| {
                    if first_row {
                        // Early normalization failures retain the physical record charge.
                        budget.records -= 1;
                        first_row = false;
                    }
                    budget.record(text.len(), rules.fields.len(), rules.expand_rows)
                })
            },
            |row_path, row| {
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
                        if controls.borrow_mut().as_deref_mut().is_some_and(|budget| {
                            !budget.retain_classification(&entry.classification)
                        }) {
                            return false;
                        }
                        entry.normalized_record = Some(entry.raw_logline.clone());
                        entry.source_file = Some(crate::evidence::path_label(path));
                        entry.source_row_path = Some(row_path);
                        entry.raw_logline = text.to_string();
                        entries.push(entry);
                    }
                    Err(diagnostic) => {
                        coverage.rejected_candidates += 1;
                        coverage.normalization_diagnostics.push(diagnostic);
                    }
                }
                true
            },
        );
        return;
    }
    if format != LogFormat::JsonLines && !line_starts_entry(text, format) {
        coverage.rejected_candidates += 1;
        let first_line = text.lines().next().unwrap_or_default();
        let mut matches = FormatMatches::default();
        matches.observe(first_line);
        let reason = if unsupported_python_header(first_line) {
            "unsupported_python_header"
        } else if matches.any() {
            "selected_parser_mismatch"
        } else if looks_like_entry_candidate(first_line) {
            "unsupported_header"
        } else {
            "unrecognized_leading_block"
        };
        coverage.structural_diagnostics.reject(line_number, reason);
        return;
    }
    match parse_log_entry_in_format(text, line_number, config, format) {
        Ok(mut entry) => {
            if controls
                .as_deref_mut()
                .is_some_and(|budget| !budget.retain_classification(&entry.classification))
            {
                return;
            }
            entry.source_file = Some(crate::evidence::path_label(path));
            entries.push(entry);
        }
        Err(_) => {
            coverage.rejected_candidates += 1;
            coverage
                .structural_diagnostics
                .reject(line_number, "invalid_selected_record");
        }
    }
}

/// Coverage is measured before filters or severity selection.
#[derive(Debug, Clone, Serialize)]
pub struct ParseCoverage {
    pub file: String,
    pub profile: String,
    pub configured_parser: LogFormat,
    pub selected_parser: LogFormat,
    pub input_bytes: u64,
    pub snapshot_sha256: String,
    pub nonempty_lines: usize,
    pub parsed_entries: usize,
    pub rejected_candidates: usize,
    pub normalization_diagnostics: Vec<crate::normalize::RowDiagnostic>,
    pub structural_diagnostics: StructuralDiagnostics,
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
    config.validate_event_rules().map_err(|reason| {
        ParseError::InvalidLogFormat(format!("invalid analyzer configuration: {reason}"))
    })?;
    let path = path.as_ref();
    let file = File::open(path)?;
    let mut reader = BufReader::new(crate::evidence::SnapshotReader::new(file));
    let lines = std::io::Read::by_ref(&mut reader)
        .lines()
        .map(|line| line.map(std::borrow::Cow::Owned));
    let mut parsed = parse_lines(lines, path, config, true, None)?;
    let (digest, bytes) = reader.into_inner().finish();
    parsed.coverage.snapshot_sha256 = digest;
    parsed.coverage.input_bytes = bytes;
    Ok(parsed)
}

fn parse_lines<'a>(
    lines: impl Iterator<Item = std::io::Result<std::borrow::Cow<'a, str>>>,
    path: &Path,
    config: &AnalyzerConfig,
    physical_eof: bool,
    mut controls: Option<&mut crate::processing::Budget>,
) -> Result<ParsedLogFile, ParseError> {
    let mut lines = lines.enumerate();
    let mut samples = Vec::new();
    let mut sample_indices = Vec::new();
    let mut skipped_leading_blank_lines = 0;
    let mut sampling_stop = None;
    let selection = if config.normalization.is_some() {
        "normalization"
    } else if config.parser.format == LogFormat::Auto {
        "automatic_sample"
    } else {
        "explicit"
    };
    if selection == "automatic_sample" {
        let mut nonempty = 0;
        while nonempty < 10 {
            let Some((index, line)) = lines.next() else {
                break;
            };
            let line = line?;
            if controls
                .as_deref_mut()
                .is_some_and(|budget| !budget.physical_size(line.len()))
            {
                sampling_stop = Some((
                    index,
                    !line.trim().is_empty(),
                    controls
                        .as_deref()
                        .is_some_and(|budget| line.len() > budget.limits.record_bytes),
                ));
                break;
            }
            if line.trim().is_empty() && samples.is_empty() {
                skipped_leading_blank_lines += 1;
                continue;
            }
            nonempty += usize::from(!line.trim().is_empty());
            sample_indices.push(index);
            samples.push(line);
        }
    }
    let format = if config.normalization.is_some() {
        LogFormat::JsonLines
    } else {
        detect_format_from_lines(
            samples.iter().map(|line| line.as_ref()),
            config.parser.format,
        )
    };
    let mut structural_diagnostics = StructuralDiagnostics::new(
        selection,
        &samples.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    );
    structural_diagnostics.blank_lines = skipped_leading_blank_lines;
    let mut coverage = ParseCoverage {
        file: crate::evidence::path_label(path),
        profile: config.profile_name.clone(),
        configured_parser: config.parser.format,
        selected_parser: format,
        input_bytes: 0,
        snapshot_sha256: String::new(),
        nonempty_lines: 0,
        parsed_entries: 0,
        rejected_candidates: 0,
        normalization_diagnostics: Vec::new(),
        structural_diagnostics,
    };
    if let Some((index, nonempty, oversized)) = sampling_stop {
        coverage.nonempty_lines = samples
            .iter()
            .filter(|line| !line.trim().is_empty())
            .count()
            + usize::from(nonempty);
        coverage.structural_diagnostics.blank_lines +=
            samples.iter().filter(|line| line.trim().is_empty()).count() + usize::from(!nonempty);
        if nonempty {
            coverage.rejected_candidates += 1;
            coverage.structural_diagnostics.physical_candidate_blocks += 1;
            coverage.structural_diagnostics.reject(
                index + 1,
                if oversized {
                    "physical_record_limit"
                } else {
                    "processing_stopped_before_parse"
                },
            );
        }
        return Ok(ParsedLogFile {
            entries: Vec::new(),
            coverage,
        });
    }
    let mut entries = Vec::new();
    let mut current_log: Option<String> = None;
    let mut current_line_number = 0;

    let replay = sample_indices
        .into_iter()
        .zip(samples)
        .map(|(index, line)| (index, Ok(line)));
    for (index, line) in replay.chain(lines) {
        let line = line?;
        let nonempty = !line.trim().is_empty();
        coverage.nonempty_lines += usize::from(nonempty);
        coverage.structural_diagnostics.blank_lines += usize::from(!nonempty);
        if controls
            .as_deref_mut()
            .is_some_and(|budget| !budget.physical_size(line.len()))
        {
            if nonempty {
                coverage.rejected_candidates += 1;
                coverage.structural_diagnostics.physical_candidate_blocks += 1;
                coverage.structural_diagnostics.reject(
                    index + 1,
                    if controls
                        .as_deref()
                        .is_some_and(|budget| line.len() > budget.limits.record_bytes)
                    {
                        "physical_record_limit"
                    } else {
                        "processing_stopped_before_parse"
                    },
                );
            }
            break;
        }
        if nonempty {
            coverage
                .structural_diagnostics
                .observed_format_matches
                .observe(&line);
            coverage.structural_diagnostics.unsupported_python_headers +=
                usize::from(unsupported_python_header(&line));
        }
        if format == LogFormat::JsonLines {
            if !line.trim().is_empty() {
                finish_candidate(
                    &line,
                    index + 1,
                    format,
                    path,
                    config,
                    &mut entries,
                    &mut coverage,
                    &mut controls,
                );
            }
        } else if line_starts_entry(&line, format)
            || looks_like_entry_candidate(&line)
            || unsupported_python_header(&line)
        {
            if let Some(text) = current_log.take() {
                finish_candidate(
                    &text,
                    current_line_number,
                    format,
                    path,
                    config,
                    &mut entries,
                    &mut coverage,
                    &mut controls,
                );
            }
            if controls.as_deref().is_some_and(|budget| budget.halted) {
                break;
            }
            current_log = Some(line.into_owned());
            current_line_number = index + 1;
        } else if let Some(text) = &mut current_log {
            coverage.structural_diagnostics.attached_nonempty_lines +=
                usize::from(!line.trim().is_empty());
            if controls.as_deref_mut().is_some_and(|budget| {
                !budget.physical_size(text.len().saturating_add(1).saturating_add(line.len()))
            }) {
                coverage.rejected_candidates += 1;
                coverage.structural_diagnostics.physical_candidate_blocks += 1;
                coverage.structural_diagnostics.reject(
                    current_line_number,
                    if controls.as_deref().is_some_and(|budget| {
                        text.len().saturating_add(1).saturating_add(line.len())
                            > budget.limits.record_bytes
                    }) {
                        "multiline_record_limit"
                    } else {
                        "processing_stopped_before_parse"
                    },
                );
                break;
            }
            text.push('\n');
            text.push_str(&line);
        } else if !line.trim().is_empty() {
            // An unrecognized leading block is one candidate, not one per stack frame.
            if controls.as_deref().is_some_and(|budget| budget.halted) {
                break;
            }
            current_log = Some(line.into_owned());
            current_line_number = index + 1;
        }
    }
    if physical_eof
        && controls.as_deref().is_none_or(|budget| !budget.halted)
        && let Some(text) = current_log
    {
        finish_candidate(
            &text,
            current_line_number,
            format,
            path,
            config,
            &mut entries,
            &mut coverage,
            &mut controls,
        );
    }
    coverage.parsed_entries = entries.len();
    coverage.structural_diagnostics.observed_status = coverage
        .structural_diagnostics
        .observed_format_matches
        .status();
    Ok(ParsedLogFile { entries, coverage })
}

/// Parse the retained capture once. A cutoff never closes a pending physical record.
pub(crate) fn parse_capture(
    path: &Path,
    data: &[u8],
    physical_eof: bool,
    config: &AnalyzerConfig,
    budget: &mut crate::processing::Budget,
) -> Result<ParsedLogFile, ParseError> {
    config
        .validate_event_rules()
        .map_err(ParseError::InvalidLogFormat)?;
    let text = match std::str::from_utf8(data) {
        Ok(text) => text,
        Err(error) if !physical_eof && error.error_len().is_none() => {
            std::str::from_utf8(&data[..error.valid_up_to()]).expect("validated UTF-8 prefix")
        }
        Err(_) => return Err(ParseError::InvalidLogFormat("capture is not UTF-8".into())),
    };
    let lines = text
        .split_inclusive('\n')
        .filter(|line| physical_eof || line.ends_with('\n'))
        .map(|line| {
            Ok(std::borrow::Cow::Borrowed(
                line.strip_suffix('\n')
                    .unwrap_or(line)
                    .strip_suffix('\r')
                    .unwrap_or(line.strip_suffix('\n').unwrap_or(line)),
            ))
        });
    let mut parsed = parse_lines(lines, path, config, physical_eof, Some(budget))?;
    parsed.coverage.input_bytes = data.len() as u64;
    parsed.coverage.snapshot_sha256 = crate::evidence::digest(data);
    Ok(parsed)
}

/// Reject nonempty files with no recognized entries for all parser callers.
pub fn parse_log_file_with_config(
    path: impl AsRef<Path>,
    config: &AnalyzerConfig,
) -> Result<Vec<LogEntry>, ParseError> {
    let parsed = parse_log_file_report(path.as_ref(), config)?;
    if parsed.coverage.is_unparsed() {
        return Err(ParseError::NoRecognizedEntries(Box::new(parsed.coverage)));
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
            r"^(?:\S+[:#]\S+\s+)?[^\s|]+(?:\s+\([^)]*\))?\s+\|\s+\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}",
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
    config.validate_event_rules().map_err(|reason| {
        ParseError::InvalidLogFormat(format!("invalid analyzer configuration: {reason}"))
    })?;
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
        Some(object),
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
    typed_fields: Option<&serde_json::Map<String, Value>>,
) -> Result<LogEntry, ParseError> {
    use crate::event_rules::{ClassifiedRecord, RecordInput, StructuredFields};
    let mut entry = if config.event_classifier().is_some() {
        create_generic_log(
            component,
            component_id,
            timestamp,
            level,
            message.clone(),
            raw_logline,
            None,
            source_line_number,
        )
    } else {
        determine_log_entry_kind(
            component,
            component_id,
            timestamp,
            level,
            message.clone(),
            raw_logline,
            &message,
            source_line_number,
            &config.parser,
        )?
    };
    let mut markers = config.parser.command_payload_markers.clone();
    if config.event_rules.is_some() {
        markers.extend(config.parser.request_payload_markers.iter().cloned());
        markers.push(config.parser.event_payload_separator.clone());
    }
    let (decoded_payload, cleaned) = if config.event_classifier().is_some()
        && message.len() <= crate::event_rules::MAX_MESSAGE_BYTES
    {
        explicit_payload(&message, &markers)
    } else {
        (None, message.clone())
    };
    let fields = typed_fields
        .map(StructuredFields::Json)
        .unwrap_or(StructuredFields::Flat(&structured_fields));
    let classification = config.event_classifier().map(|rules| {
        rules.classify_owned(
            &config.profile_name,
            RecordInput {
                record: &entry,
                original_message: &message,
                fields: if rules.schema().version >= 2 {
                    StructuredFields::WithPayload {
                        fields: &fields,
                        payload: decoded_payload.as_ref().or(payload_override.as_ref()),
                    }
                } else {
                    fields
                },
            },
        )
    });
    match &classification {
        Some(ClassifiedRecord::Event { semantics, .. }) => {
            use crate::event_rules::OperationKind;
            entry.message = cleaned;
            entry.kind = match semantics.kind {
                OperationKind::Command => LogEntryKind::Command {
                    command: semantics.name.clone(),
                    settings: decoded_payload,
                },
                OperationKind::Request => LogEntryKind::Request {
                    request: semantics.name.clone(),
                    request_id: semantics.correlation_id.clone(),
                    endpoint: semantics.endpoint.clone(),
                    direction: match semantics.direction.as_deref() {
                        Some("send") => RequestDirection::Send,
                        Some("receive") => RequestDirection::Receive,
                        _ => RequestDirection::Unknown,
                    },
                    payload: decoded_payload,
                },
                OperationKind::Event => LogEntryKind::Event {
                    event_type: semantics.name.clone(),
                    direction: match semantics.direction.as_deref() {
                        Some("emit") => EventDirection::Emit,
                        Some("receive") => EventDirection::Receive,
                        _ => EventDirection::Unknown,
                    },
                    payload: decoded_payload,
                },
            };
        }
        Some(ClassifiedRecord::Conflict { .. } | ClassifiedRecord::Invalid { .. }) => (),
        Some(ClassifiedRecord::Unclassified) => {
            entry = determine_log_entry_kind(
                entry.component,
                entry.component_id,
                timestamp,
                entry.level,
                message.clone(),
                entry.raw_logline,
                &message,
                source_line_number,
                &config.parser,
            )?;
        }
        None => (),
    }
    entry.classification = classification;

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
    let inherited_scope = record_correlation_scope(&entry, config).unwrap_or_default();
    if let Some(ClassifiedRecord::Event {
        semantics,
        legacy: false,
        ..
    }) = &mut entry.classification
        && semantics.scope.is_empty()
    {
        semantics.scope = inherited_scope;
    }
    attach_legacy_event_evidence(&mut entry, config);
    Ok(entry)
}

/// Legacy-only compatibility adapter. Phase searches happen once at parsing, never in perf.
/// Library callers assembling legacy Command records may attach the same evidence explicitly.
pub fn attach_legacy_command_evidence(entry: &mut LogEntry, config: &AnalyzerConfig) {
    attach_legacy_event_evidence(entry, config);
}

/// Legacy compatibility lives at the parsing seam; perf consumes cached evidence only.
pub fn attach_legacy_event_evidence(entry: &mut LogEntry, config: &AnalyzerConfig) {
    use crate::event_rules::{ClassifiedRecord, EventSemantics, OperationKind, Phase};
    if config.event_rules.is_some() {
        return;
    }
    let (kind, name, id, start, end, direction, endpoint) = match &entry.kind {
        LogEntryKind::Command { command, .. } if config.command_rules.is_none() => (
            OperationKind::Command,
            command.clone(),
            Some(command.clone()),
            contains_any_marker(&entry.message, &config.perf.command_start_markers),
            contains_any_marker(&entry.message, &config.perf.command_completion_markers),
            None,
            None,
        ),
        LogEntryKind::Request {
            request,
            request_id,
            direction,
            endpoint,
            ..
        } => (
            OperationKind::Request,
            request.clone(),
            request_id.clone(),
            *direction == RequestDirection::Send,
            *direction == RequestDirection::Receive,
            Some(direction.to_string().to_lowercase()),
            endpoint.clone(),
        ),
        LogEntryKind::Event {
            event_type,
            direction,
            payload,
        } => (
            OperationKind::Event,
            event_type.clone(),
            payload.as_ref().and_then(|p| {
                config
                    .perf
                    .event_correlation_keys
                    .iter()
                    .find_map(|key| p.get(key).and_then(Value::as_str).map(str::to_owned))
            }),
            *direction == EventDirection::Receive,
            *direction == EventDirection::Emit,
            Some(direction.to_string().to_lowercase()),
            None,
        ),
        LogEntryKind::Generic { .. } => {
            if entry.classification.is_none() {
                entry.classification = Some(ClassifiedRecord::Unclassified);
            }
            return;
        }
        _ => return,
    };
    entry.classification = Some(if start && end {
        ClassifiedRecord::Conflict {
            kinds: vec![kind],
            profile: config.profile_name.clone(),
            rule_ids: vec!["legacy-start".into(), "legacy-end".into()],
        }
    } else {
        ClassifiedRecord::Event {
            legacy: true,
            semantics: EventSemantics {
                kind,
                name,
                correlation_id: id,
                scope: Vec::new(),
                direction,
                endpoint,
                phase: if start {
                    Some(Phase::Start)
                } else if end {
                    Some(Phase::End)
                } else {
                    None
                },
                outcome: None,
                end_expected: true,
            },
            profile: config.profile_name.clone(),
            rule_ids: vec!["legacy-markers".into()],
        }
    });
}

fn explicit_payload(message: &str, payload_markers: &[String]) -> (Option<Value>, String) {
    // Record marker-eligible starts once; matching is independent of quoted command names.
    let mut eligible = vec![false; message.len()];
    let mut quote = None;
    let mut escaped = false;
    for (position, ch) in message.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if let Some(delimiter) = quote {
            if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '"' | '\'') {
            quote = Some(ch);
        } else {
            eligible[position] = true;
        }
    }
    let markers: Vec<_> = payload_markers.iter().filter(|s| !s.is_empty()).collect();
    if markers.is_empty() {
        return (None, message.to_string());
    }
    // A contiguous NFA bounds construction memory; overlapping search preserves marker priority
    // even when markers share prefixes or one eligible occurrence overlaps another.
    let Ok(matcher) = aho_corasick::AhoCorasickBuilder::new()
        .kind(Some(aho_corasick::AhoCorasickKind::ContiguousNFA))
        .build(markers)
    else {
        return (None, message.to_string());
    };
    let mut whitespace_range = (0, 0);
    let candidate = matcher
        .find_overlapping_iter(message)
        .filter_map(|matched| {
            if !eligible[matched.start()] {
                return None;
            }
            // Overlapping matches arrive in end-position order. Reuse whitespace runs so
            // space-only markers cannot repeatedly scan the same long suffix.
            if !(whitespace_range.0..=whitespace_range.1).contains(&matched.end()) {
                let rest = message[matched.end()..].trim_start();
                whitespace_range = (matched.end(), message.len() - rest.len());
            }
            let payload_start = whitespace_range.1;
            message[payload_start..].starts_with(['{', '[']).then_some((
                matched.start(),
                matched.pattern(),
                payload_start,
            ))
        })
        .min_by_key(|(start, pattern, _)| (*start, *pattern));
    if let Some((_, _, payload_start)) = candidate {
        let rest = &message[payload_start..];
        if let Some(end) = command_payload_end(rest)
            && command_payload_trivia(&rest[end..])
            && let Ok(payload) = json5::from_str::<Value>(&normalize_json5_undefined(&rest[..end]))
        {
            return (
                Some(payload),
                format!("{}[JSON removed]", &message[..payload_start]),
            );
        }
    }
    (None, message.to_string())
}

// Convert legacy JSON5 undefined values without rewriting string identities or property names.
fn normalize_json5_undefined(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut copied = 0;
    let mut containers = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut previous = None;
    let mut chars = input.char_indices().peekable();
    while let Some((position, ch)) = chars.next() {
        if line_comment {
            line_comment = !is_json5_line_terminator(ch);
            continue;
        }
        if block_comment {
            if ch == '*' && chars.peek().is_some_and(|(_, next)| *next == '/') {
                chars.next();
                block_comment = false;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
                previous = Some(ch);
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '/' if chars.peek().is_some_and(|(_, next)| *next == '/') => {
                chars.next();
                line_comment = true;
                continue;
            }
            '/' if chars.peek().is_some_and(|(_, next)| *next == '*') => {
                chars.next();
                block_comment = true;
                continue;
            }
            '{' | '[' => containers.push(ch),
            '}' | ']' => {
                containers.pop();
            }
            'u' if (previous == Some(':')
                || previous == Some('[')
                || (previous == Some(',') && containers.last() == Some(&'[')))
                && input[position..].starts_with("undefined")
                && input[position + 9..].chars().next().is_none_or(|next| {
                    next.is_whitespace() || matches!(next, ',' | ']' | '}' | '/')
                }) =>
            {
                output.push_str(&input[copied..position]);
                output.push_str("null");
                copied = position + 9;
                for _ in 0..8 {
                    chars.next();
                }
            }
            _ => (),
        }
        if !ch.is_whitespace() {
            previous = Some(ch);
        }
    }
    output.push_str(&input[copied..]);
    output
}

fn is_json5_line_terminator(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

pub(crate) fn complete_container_suffix(input: &str) -> bool {
    input.starts_with(['{', '['])
        && command_payload_end(input).is_some_and(|end| command_payload_trivia(&input[end..]))
}

// Only whitespace and complete JSON5 comments may follow the bounded root value.
fn command_payload_trivia(mut input: &str) -> bool {
    loop {
        input = input.trim_start();
        if input.is_empty() {
            return true;
        }
        if let Some(comment) = input.strip_prefix("//") {
            input = comment
                .find(is_json5_line_terminator)
                .map_or("", |end| &comment[end..]);
        } else if let Some(comment) = input.strip_prefix("/*") {
            let Some(end) = comment.find("*/") else {
                return false;
            };
            input = &comment[end + 2..];
        } else {
            return false;
        }
    }
}

// Bound the new command decoding path without changing legacy payload extraction.
fn command_payload_end(input: &str) -> Option<usize> {
    let mut stack = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut chars = input.char_indices().peekable();
    while let Some((position, ch)) = chars.next() {
        if line_comment {
            line_comment = !is_json5_line_terminator(ch);
            continue;
        }
        if block_comment {
            if ch == '*' && chars.peek().is_some_and(|(_, next)| *next == '/') {
                chars.next();
                block_comment = false;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '/' if chars.peek().is_some_and(|(_, next)| *next == '/') => {
                chars.next();
                line_comment = true;
            }
            '/' if chars.peek().is_some_and(|(_, next)| *next == '*') => {
                chars.next();
                block_comment = true;
            }
            '{' | '[' => {
                if stack.len() == 128 {
                    return None;
                }
                stack.push(ch);
            }
            '}' | ']' => {
                if stack.pop()? != if ch == '}' { '{' } else { '[' } {
                    return None;
                }
                if stack.is_empty() {
                    return Some(position + ch.len_utf8());
                }
            }
            _ => (),
        }
    }
    None
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
    if !trimmed.contains('=') {
        return (trimmed.to_string(), HashMap::new());
    }

    let boundaries = std::iter::once(0).chain(
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
        // Reject prose at its first token instead of rescanning the entire suffix
        // for '=' at every word boundary in a long tracing message.
        let key_end = remaining
            .find(|ch: char| ch == '=' || ch.is_whitespace())
            .unwrap_or(remaining.len());
        let key = &remaining[..key_end];
        if !FIELD_KEY_RE.is_match(key) {
            return None;
        }

        let value_input = remaining[key_end..].trim_start().strip_prefix('=')?;
        let (value, consumed) = parse_field_value(value_input)?;
        fields.insert(key.to_string(), value);
        parsed_any = true;

        remaining = value_input[consumed..].trim_start();
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
    } else if !parser_rules.command_prefix.is_empty()
        && !parser_rules.command_start_marker.is_empty()
        && message.contains(&parser_rules.command_prefix)
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

pub(crate) fn record_correlation_scope(
    entry: &LogEntry,
    config: &AnalyzerConfig,
) -> Option<Vec<String>> {
    config
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
        .map(|value| {
            value
                .filter(|value| !value.trim().is_empty() && value != "null")
                .map(|value| crate::event_rules::bounded_scope_value(&value))
        })
        .collect::<Option<Vec<_>>>()
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    #[test]
    fn classification_memory_cutoff_applies_to_native_and_normalized_records() {
        for normalized in [false, true] {
            let mut config = crate::config::load_config_from_path(Path::new(
                "examples/investigations/profile.toml",
            ))
            .unwrap();
            config.profile_name = "p".repeat(4096);
            if normalized {
                config.normalization = Some(crate::normalize::NormalizationRules::default());
            }
            let text = concat!(
                "{\"timestamp\":\"2026-01-01T00:00:00Z\",\"message\":\"start\",",
                "\"phase\":\"start\",\"operation\":\"run\",\"id\":\"one\",\"session\":\"scope\"}\n"
            );
            let mut budget = crate::processing::test_budget();
            budget.limits.memory_bytes = 100 * 1024;
            let parsed = parse_capture(
                Path::new("source"),
                text.as_bytes(),
                true,
                &config,
                &mut budget,
            )
            .unwrap();
            assert!(parsed.entries.is_empty());
            assert_eq!(parsed.coverage.nonempty_lines, 1);
            assert!(budget.memory_bytes <= budget.limits.memory_bytes);
            let stop = budget.stop.unwrap();
            assert_eq!(stop["stage"], "classification");
            assert_eq!(stop["reason"], "memory_limit");

            let mut budget = crate::processing::test_budget();
            let parsed = parse_capture(
                Path::new("source"),
                text.as_bytes(),
                true,
                &config,
                &mut budget,
            )
            .unwrap();
            assert_eq!(parsed.entries.len(), 1);
            assert!(budget.stop.is_none());
        }
    }

    #[test]
    fn cutoff_does_not_close_multiline_candidate_or_partial_json_scalar() {
        let config =
            crate::config::load_config_from_path(Path::new("examples/investigations/profile.toml"))
                .unwrap();
        let data = include_bytes!("../examples/investigations/slow.jsonl");
        let first = data.iter().position(|byte| *byte == b'\n').unwrap() + 1;
        let mut budget = crate::processing::test_budget();
        let captured = parse_capture(
            Path::new("source"),
            &data[..first + 5],
            false,
            &config,
            &mut budget,
        )
        .unwrap();
        assert_eq!(captured.entries.len(), 1);
        let classic = crate::config::default_config();
        let text = b"worker | 2026-01-01T00:00:00+02:00 [INFO ] first\ncontinuation\n";
        let mut budget = crate::processing::test_budget();
        assert!(
            parse_capture(Path::new("source"), text, false, classic, &mut budget)
                .unwrap()
                .entries
                .is_empty()
        );
        let mut budget = crate::processing::test_budget();
        assert_eq!(
            parse_capture(Path::new("source"), text, true, classic, &mut budget)
                .unwrap()
                .entries
                .len(),
            1
        );
    }
    #[test]
    fn bounded_capture_and_unlimited_wrapper_share_records_and_coverage() {
        let config =
            crate::config::load_config_from_path(Path::new("examples/investigations/profile.toml"))
                .unwrap();
        let path = Path::new("examples/investigations/slow.jsonl");
        let unlimited = parse_log_file_report(path, &config).unwrap();
        let mut budget = crate::processing::test_budget();
        let bounded = parse_capture(
            path,
            &std::fs::read(path).unwrap(),
            true,
            &config,
            &mut budget,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(unlimited.coverage).unwrap(),
            serde_json::to_value(bounded.coverage).unwrap()
        );
        for (left, right) in unlimited.entries.iter().zip(bounded.entries.iter()) {
            assert_eq!(left.raw_logline, right.raw_logline);
            assert_eq!(
                serde_json::to_value(&left.classification).unwrap(),
                serde_json::to_value(&right.classification).unwrap()
            );
            assert_eq!(left.source_timestamp, right.source_timestamp);
        }
    }
}
