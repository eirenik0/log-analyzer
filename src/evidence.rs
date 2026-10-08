//! Snapshot-scoped identities and additive report semantics for consuming agents.
use crate::{
    cli::Cli,
    config::AnalyzerConfig,
    parser::{LogEntry, ParseCoverage},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{self, Read};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReference {
    pub reference_id: String,
    #[serde(default)]
    pub location_redacted: bool,
    pub input_id: String,
    pub line: usize,
    pub row_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expansion: Option<ExpansionReference>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpansionReference {
    pub path: String,
    pub index: usize,
}

pub const CONTRACT_VERSION: u32 = 1;

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Non-UTF-8 source labels retain a readable display and a distinct byte identity.
/// Escape valid labels beginning with @ so they cannot impersonate encoded labels.
pub(crate) fn path_label(path: &std::path::Path) -> String {
    match path.to_str() {
        Some(text) if text.starts_with('@') => format!("@utf8:{text}"),
        Some(text) => text.into(),
        None => format!(
            "@os-bytes:{}:{}",
            digest(path.as_os_str().as_encoded_bytes()),
            path.to_string_lossy()
        ),
    }
}

/// Preserve normal paths; non-UTF-8 paths expose a safe display and byte identity.
fn path_value(path: &std::path::Path) -> Value {
    match path.to_str() {
        Some(path) => json!(path),
        None => {
            json!({"file":path_label(path),"os_bytes_sha256":digest(path.as_os_str().as_encoded_bytes())})
        }
    }
}

pub(crate) fn serialize_path<S: serde::Serializer>(
    path: &std::path::Path,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serde::Serialize::serialize(&path_value(path), serializer)
}

pub(crate) fn serialize_paths<S: serde::Serializer>(
    paths: &[std::path::PathBuf],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serde::Serialize::serialize(
        &paths
            .iter()
            .map(|path| path_value(path))
            .collect::<Vec<_>>(),
        serializer,
    )
}

pub(crate) fn serialize_optional_path<S: serde::Serializer>(
    path: &Option<std::path::PathBuf>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serde::Serialize::serialize(&path.as_deref().map(path_value), serializer)
}

/// Hash exactly the byte stream consumed by the parser, including line endings.
pub(crate) struct SnapshotReader<R> {
    inner: R,
    hash: Sha256,
    bytes: u64,
}
impl<R> SnapshotReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            hash: Sha256::new(),
            bytes: 0,
        }
    }
    pub fn finish(self) -> (String, u64) {
        (format!("{:x}", self.hash.finalize()), self.bytes)
    }
}
impl<R: Read> Read for SnapshotReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buf)?;
        self.hash.update(&buf[..count]);
        self.bytes += count as u64;
        Ok(count)
    }
}

pub fn source(entry: &LogEntry) -> crate::perf_analyzer::SourceLocation {
    crate::perf_analyzer::SourceLocation {
        evidence_ref: None,
        file: entry.source_file.clone(),
        line: entry.source_line_number,
        row_path: entry.source_row_path.clone(),
    }
}

#[derive(Default)]
pub(crate) struct Context {
    profile_digest: String,
    query: Value,
    inputs: Vec<Value>,
    filter: crate::comparator::LogFilter,
    records: Vec<(chrono::DateTime<chrono::Local>, usize, usize, Value)>,
    collect_records: bool,
    trace_selector: Option<crate::trace::TraceSelector>,
    legacy_sanitize_records: bool,
}
impl Context {
    pub fn new(cli: &Cli, config: &AnalyzerConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let mut query = serde_json::to_value(cli)?;
        // Rendering and destination do not change the selected evidence.
        for key in [
            "output",
            "color",
            "verbose",
            "quiet",
            "format",
            "json",
            "compact",
            "config",
            "preset",
            "redact",
            "mask_id",
            "report_max_chars",
            "report_max_bytes",
            "report_max_items",
            "report_cursor",
            "complete_output",
        ] {
            query.as_object_mut().unwrap().remove(key);
        }
        let filter = if let Some(text) = &cli.filter {
            crate::filter::to_log_filter(&crate::filter::FilterExpression::parse(text)?)
        } else {
            crate::comparator::LogFilter::new()
        };
        Ok(Self {
            profile_digest: digest(&serde_json::to_vec(&serde_json::to_value(config)?)?),
            query,
            filter,
            collect_records: cli.common_reports(),
            legacy_sanitize_records: !cli.redact
                && matches!(
                    &cli.command,
                    crate::cli::Commands::Process {
                        no_sanitize: false,
                        ..
                    } | crate::cli::Commands::LlmDiff {
                        no_sanitize: false,
                        ..
                    }
                ),
            trace_selector: match &cli.command {
                crate::cli::Commands::Trace { id: Some(id), .. } => {
                    Some(crate::trace::TraceSelector::Id(id.clone()))
                }
                crate::cli::Commands::Trace {
                    session: Some(session),
                    ..
                } => Some(crate::trace::TraceSelector::Session(session.clone())),
                _ => None,
            },
            ..Self::default()
        })
    }
    pub fn observe(&mut self, coverage: &ParseCoverage, entries: &[LogEntry]) {
        let input_id =
            digest(&serde_json::to_vec(&json!([coverage.file, coverage.snapshot_sha256])).unwrap());
        self.inputs.push(json!({"input_id": input_id, "file": coverage.file, "sha256": coverage.snapshot_sha256, "bytes": coverage.input_bytes, "coverage": coverage, "selected_entries": entries.iter().filter(|entry| self.filter.matches(entry)).count()}));
        if self.collect_records {
            let input_ordinal = self.inputs.len() - 1;
            for entry in entries.iter().filter(|entry| {
                self.filter.matches(entry)
                    && self
                        .trace_selector
                        .as_ref()
                        .is_none_or(|selector| selector.matches(entry))
            }) {
                let row = json!(entry.source_row_path);
                let reference = self
                    .reference(&coverage.file, &json!(entry.source_line_number), &row)
                    .unwrap();
                let mut record = json!({"evidence_ref":reference,"source_file":entry.source_file,
                    "source_line_number":entry.source_line_number,"source_row_path":entry.source_row_path,
                    "timestamp":entry.source_timestamp.map(|t|t.to_rfc3339()).unwrap_or_else(||entry.timestamp.to_rfc3339()),
                    "timestamp_year_inferred":entry.timestamp_year_inferred,"timestamp_offset_source":if entry.source_timestamp.is_some(){"source"}else{"local_assumption"},
                    "component":entry.component,"component_id":entry.component_id,"level":entry.level,
                    "message":entry.message,"raw_logline":entry.raw_logline,
                    "payload":entry.payload().or(entry.envelope_payload.as_ref()),"structured_fields":entry.structured_fields,
                    "classification":entry.classification,"input_ordinal":input_ordinal});
                if self.legacy_sanitize_records {
                    record["payload"] =
                        crate::llm_processor::sanitize_json_value(&record["payload"]);
                    record["structured_fields"] =
                        crate::llm_processor::sanitize_json_value(&record["structured_fields"]);
                    record["raw_logline"] = json!("[OMITTED LEGACY SANITIZATION]");
                    record["raw_logline_omitted"] = json!(true);
                }
                self.records
                    .push((entry.timestamp, input_ordinal, self.records.len(), record));
            }
        }
    }
    pub fn records(&self) -> Value {
        let mut records: Vec<_> = self.records.iter().collect();
        records.sort_by_key(|(timestamp, input, ordinal, _)| (*timestamp, *input, *ordinal));
        Value::Array(
            records
                .into_iter()
                .map(|(_, _, _, value)| value.clone())
                .collect(),
        )
    }
    fn reference(&self, file: &str, line: &Value, row: &Value) -> Option<Value> {
        if line.as_u64()? == 0 {
            return None;
        }
        let input = self.inputs.iter().find(|input| input["file"] == file)?;
        Some(
            json!({"reference_id":digest(&serde_json::to_vec(&json!([input["input_id"],line,row,null])).unwrap()),"location_redacted":false,"input_id": input["input_id"], "line": line, "row_path": row}),
        )
    }
    pub fn attach_source(&self, source: &mut crate::perf_analyzer::SourceLocation) {
        if let Some(file) = &source.file {
            if source.evidence_ref.is_some() {
                return;
            }
            source.evidence_ref = self
                .reference(file, &json!(source.line), &json!(source.row_path))
                .map(|value| serde_json::from_value(value).unwrap());
        }
    }
    /// Annotate only generated report structures; payload keys never become provenance.
    pub fn annotate(&self, value: &mut Value) {
        let section = value.get_mut("logs");
        if let Some(section) = section {
            self.walk(section, None);
        }
        for key in [
            "profile_validation",
            "search",
            "extract",
            "trace",
            "operations",
            "orphans",
            "unmatched_events",
            "ambiguous_groups",
            "threshold_violations",
            "event_timeline",
            "comparisons",
            "errors",
        ] {
            if let Some(section) = value.get_mut(key) {
                self.walk(section, None);
            }
        }
        if let Some(trace) = value.get_mut("trace") {
            let refs: Vec<_> = trace["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.get("evidence_ref").cloned())
                .collect();
            if let (Some(first), Some(last)) = (refs.first(), refs.last()) {
                let available = trace["entries"].as_array().unwrap().iter().all(|entry| {
                    entry["timestamp_year_source"] == "source"
                        && entry["timestamp_offset_source"] == "source"
                });
                trace["span_boundaries"] = json!({"start":first,"end":last,"kind":if available {"measurement"} else {"unavailable"},"measurement_ms":if available {trace["total_duration_ms"].clone()} else {Value::Null},"semantics":"elapsed_span_of_matches","reason":if available {Value::Null} else {json!("timestamp_year_or_offset_assumed")}});
            }
        }
    }
    fn walk(&self, value: &mut Value, inherited_file: Option<&str>) {
        match value {
            Value::Array(items) => {
                for item in items {
                    self.walk(item, inherited_file);
                }
            }
            Value::Object(map) => {
                let file = map
                    .get("source_file")
                    .or_else(|| map.get("file"))
                    .and_then(Value::as_str)
                    .or(inherited_file)
                    .map(str::to_owned);
                let line = map
                    .get("source_line_number")
                    .or_else(|| map.get("source_line"))
                    .or_else(|| map.get("line"));
                let row = map
                    .get("source_row_path")
                    .or_else(|| map.get("row_path"))
                    .cloned()
                    .unwrap_or(Value::Null);
                if !map.contains_key("evidence_ref")
                    && let (Some(file), Some(line)) = (&file, line)
                    && let Some(reference) = self.reference(file, line, &row)
                {
                    map.insert("evidence_ref".into(), reference);
                }
                for (key, child) in map.iter_mut() {
                    if !matches!(
                        key.as_str(),
                        "payload"
                            | "data"
                            | "values"
                            | "structured_fields"
                            | "envelope_payload"
                            | "correlation_ids"
                            | "differences"
                            | "evidence_ref"
                            | "paths"
                            | "key"
                    ) {
                        self.walk(child, file.as_deref());
                    }
                }
                if let (Some(path), Some(index)) = (
                    map.get("array_path")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    map.get("array_index").and_then(Value::as_u64),
                ) && let Some(reference) = map
                    .get_mut("source")
                    .and_then(|s| s.get_mut("evidence_ref"))
                    && reference.get("expansion").is_none()
                {
                    reference["expansion"] = json!({"path":path,"index":index});
                    reference["reference_id"] = json!(digest(
                        &serde_json::to_vec(&json!([
                            reference["input_id"],
                            reference["line"],
                            reference["row_path"],
                            reference["expansion"]
                        ]))
                        .unwrap()
                    ));
                }
            }
            _ => (),
        }
    }
    pub fn metadata(&self, report: &Value, redacted: bool, masked: &[String]) -> Value {
        let parsed: u64 = self
            .inputs
            .iter()
            .filter_map(|input| {
                input
                    .pointer("/coverage/parsed_entries")
                    .and_then(Value::as_u64)
            })
            .sum();
        let selected: u64 = self
            .inputs
            .iter()
            .filter_map(|input| input["selected_entries"].as_u64())
            .sum();
        let unparsed = self.inputs.iter().any(|input| {
            input
                .pointer("/coverage/nonempty_lines")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                > 0
                && input.pointer("/coverage/parsed_entries") == Some(&json!(0))
        });
        let status = if self.inputs.is_empty() {
            "not_applicable"
        } else if unparsed {
            "unparsed_input"
        } else if parsed == 0 {
            "empty_input"
        } else if selected == 0 {
            "zero_filter_matches"
        } else {
            "parsed"
        };
        let mut omissions = json!({"records": null, "details": "command_specific", "retrieval": "rerun_without_display_limits"});
        if report.get("logs").is_some() {
            omissions["records"] = json!(
                selected.saturating_sub(report["logs"].as_array().map_or(0, |a| a.len()) as u64)
            );
            omissions["details"] = json!(if self.collect_records {
                "none"
            } else {
                "process_payload_compaction"
            });
        }
        if let Some(errors) = report.get("errors") {
            omissions["records"] = errors["omitted"]["clusters"].clone();
            omissions["details"] = errors["omitted"].clone();
        }
        if let Some(omitted) = report.get("omitted") {
            omissions["details"] = omitted.clone();
        }
        if report.pointer("/search/entries").is_some() || report.get("trace").is_some() {
            omissions["records"] = json!(0);
            omissions["details"] = json!("payloads_absent_unless_requested");
        }
        json!({"contract_version":CONTRACT_VERSION,"snapshot_id": if self.inputs.is_empty(){Value::Null}else{json!(digest(&serde_json::to_vec(&self.inputs.iter().map(|input|&input["input_id"]).collect::<Vec<_>>()).unwrap()))}, "inputs":self.inputs,"profile_sha256":self.profile_digest,"query":self.query,"query_sha256":digest(&serde_json::to_vec(&self.query).unwrap()), "scope":{"status":status,"parsed_entries":parsed,"selected_entries":selected,"coverage_basis":"before_display_limits","capture_completeness":"unknown"},"redaction":{"applied":redacted,"masked_id_fields":masked,"legacy_sanitization": !redacted && (self.query.pointer("/command/Process/no_sanitize").is_some_and(|v|v==false) || self.query.pointer("/command/LlmDiff/no_sanitize").is_some_and(|v|v==false))},"omissions":omissions})
    }
}
