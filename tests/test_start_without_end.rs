use log_analyzer::{
    comparator::LogFilter,
    config::{self, AnalyzerConfig},
    parser,
    perf_analyzer::{self, PerfAnalysisResults},
};
use std::fs;
use tempfile::tempdir;

fn profile(end_expected: &str) -> String {
    format!(
        r#"
extends = "base"

[[sessions.levels]]
name = "job"
segment_prefix = "job-"
create_command = "open"
complete_commands = ["close"]

[event_rules]
version = 2

[[event_rules.rules]]
id = "task-start"
adapter = {{ type = "text", pattern = 'Task (?P<name>"\w+") begins' }}
[event_rules.rules.mapping]
kind = "command"
name = {{ from = "capture", capture = "name", decode = "json_string" }}
phase = {{ from = "literal", value = "start" }}
correlation_id = {{ from = "capture", capture = "name", decode = "json_string" }}
{end_expected}

[[event_rules.rules]]
id = "call-start"
adapter = {{ type = "text", pattern = 'Call (?P<name>"\w+") \[(?P<id>\w+)\] sent' }}
[event_rules.rules.mapping]
kind = "request"
name = {{ from = "capture", capture = "name", decode = "json_string" }}
phase = {{ from = "literal", value = "start" }}
correlation_id = {{ from = "capture", capture = "id" }}

[[event_rules.rules]]
id = "call-end"
adapter = {{ type = "text", pattern = 'Call (?P<name>"\w+") \[(?P<id>\w+)\] done' }}
[event_rules.rules.mapping]
kind = "request"
name = {{ from = "capture", capture = "name", decode = "json_string" }}
phase = {{ from = "literal", value = "end" }}
correlation_id = {{ from = "capture", capture = "id" }}
"#
    )
}

fn load(text: &str) -> Result<AnalyzerConfig, config::ConfigError> {
    let dir = tempdir().unwrap();
    let path = dir.path().join("p.toml");
    fs::write(&path, text).unwrap();
    config::load_config_from_path(&path)
}

const LINES: &[&str] = &[
    r#"Task "open" begins"#,
    r#"Call "fetch" [c1] sent"#,
    r#"Call "fetch" [c1] done"#,
    r#"Task "close" begins"#,
];

fn analyze(cfg: &AnalyzerConfig) -> PerfAnalysisResults {
    let logs: Vec<_> = LINES
        .iter()
        .enumerate()
        .map(|(i, m)| {
            parser::parse_log_entry_with_config(
                &format!("worker (job-1) | 2026-01-01T00:00:{i:02}.000Z [INFO] {m}"),
                i + 1,
                cfg,
            )
            .unwrap()
        })
        .collect();
    perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, cfg)
}

#[test]
fn starts_without_an_expected_end_are_not_orphans() {
    let cfg = load(&profile("end_expected = false")).unwrap();
    let result = analyze(&cfg);
    let coverage = &result.operation_coverage;
    assert_eq!(result.operations.len(), 1);
    assert!(result.orphans.is_empty(), "{:?}", result.orphans);
    assert!(result.unmatched_events.is_empty());
    assert_eq!(coverage.start_only_events, 2);
    assert_eq!(coverage.relevant_events, 4);
    assert_eq!(coverage.status, "observed_pairs");
}

#[test]
fn by_default_a_start_without_an_end_stays_an_orphan() {
    let cfg = load(&profile("")).unwrap();
    let result = analyze(&cfg);
    assert_eq!(result.orphans.len(), 2);
    assert_eq!(result.operation_coverage.start_only_events, 0);
    assert_eq!(result.operation_coverage.status, "partial_evidence");
}

#[test]
fn an_operation_type_filter_also_skips_start_only_events() {
    let cfg = load(&profile("end_expected = false")).unwrap();
    let logs: Vec<_> = LINES
        .iter()
        .enumerate()
        .map(|(i, m)| {
            parser::parse_log_entry_with_config(
                &format!("worker (job-1) | 2026-01-01T00:00:{i:02}.000Z [INFO] {m}"),
                i + 1,
                &cfg,
            )
            .unwrap()
        })
        .collect();
    let result = perf_analyzer::analyze_performance_with_config(
        &logs,
        &LogFilter::new(),
        Some("Request"),
        &cfg,
    );
    assert_eq!(result.operation_coverage.start_only_events, 0);
    assert_eq!(result.operations.len(), 1);
}

#[test]
fn session_lifecycle_creates_and_completes_on_start_only_commands() {
    let cfg = load(&profile("end_expected = false")).unwrap();
    let logs: Vec<_> = LINES
        .iter()
        .enumerate()
        .map(|(i, m)| {
            parser::parse_log_entry_with_config(
                &format!("worker (job-1) | 2026-01-01T00:00:{i:02}.000Z [INFO] {m}"),
                i + 1,
                &cfg,
            )
            .unwrap()
        })
        .collect();
    let insights = config::analyze_profile(&logs, &cfg);
    assert_eq!(insights.sessions.levels[0].completed_count(), 1);
}

#[test]
fn end_expected_false_needs_a_start_phase_and_version_two() {
    let err = load(&profile("end_expected = false").replacen(
        r#"phase = { from = "literal", value = "start" }
correlation_id = { from = "capture", capture = "name", decode = "json_string" }"#,
        r#"phase = { from = "literal", value = "end" }
correlation_id = { from = "capture", capture = "name", decode = "json_string" }"#,
        1,
    ))
    .unwrap_err();
    assert!(err.to_string().contains("requires a start phase"), "{err}");

    let err =
        load(&profile("end_expected = false").replace("version = 2", "version = 1")).unwrap_err();
    assert!(err.to_string().contains("version 2"), "{err}");
}

#[test]
fn by_default_a_start_does_not_complete_a_session() {
    let cfg = load(&profile("")).unwrap();
    let logs: Vec<_> = LINES
        .iter()
        .enumerate()
        .map(|(i, m)| {
            parser::parse_log_entry_with_config(
                &format!("worker (job-1) | 2026-01-01T00:00:{i:02}.000Z [INFO] {m}"),
                i + 1,
                &cfg,
            )
            .unwrap()
        })
        .collect();
    let insights = config::analyze_profile(&logs, &cfg);
    assert_eq!(insights.sessions.levels[0].sessions.len(), 1);
    assert_eq!(insights.sessions.levels[0].completed_count(), 0);
}
