use log_analyzer::{comparator::LogFilter, config, parser, perf_analyzer};
use serde_json::Value;
use std::{fs, process::Command};

fn analyze(messages: &[&str]) -> perf_analyzer::PerfAnalysisResults {
    let config = config::load_builtin_template("service-api").unwrap();
    let logs = messages
        .iter()
        .enumerate()
        .map(|(i, message)| {
            parser::parse_log_entry_with_config(
                &format!("core (demo) | 2026-01-01T00:00:0{i}.000Z [INFO] {message}"),
                i + 1,
                &config,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &config)
}

#[test]
fn evidence_counts_partition_candidates_without_claiming_completeness() {
    let results = analyze(&["worker alive", "worker alive"]);
    let coverage = results.operation_coverage;
    assert_eq!(results.total_entries, 2);
    assert_eq!(coverage.status, "no_applicable_events");
    assert_eq!(coverage.relevant_events, 0);
    assert_eq!(coverage.capture_window.elapsed_ms, Some(1000));
    assert_eq!(coverage.upstream_export_completeness, "unknown");
    for (messages, status, paired, unmatched) in [
        (
            vec![
                r#"Request "work" [0--a] sent"#,
                r#"Request "work" [0--a] completed"#,
            ],
            "observed_pairs",
            2,
            0,
        ),
        (
            vec![r#"Request "work" [0--a] sent"#],
            "insufficient_evidence",
            0,
            1,
        ),
        (
            vec![
                r#"Request "work" [0--a] sent"#,
                r#"Request "work" [0--a] completed"#,
                r#"Request "other" completed"#,
            ],
            "partial_evidence",
            2,
            1,
        ),
    ] {
        let result = analyze(&messages);
        let c = result.operation_coverage;
        assert_eq!(c.status, status);
        assert_eq!(c.paired_events, paired);
        assert_eq!(c.unmatched_events, unmatched);
        assert_eq!(
            c.relevant_events,
            c.paired_events + c.unmatched_events + c.suppressed_events
        );
    }
    let c = analyze(&[
        r#"Request "work" [0--a] sent"#,
        r#"Request "work" [0--a] sent"#,
        r#"Request "work" [0--a] completed"#,
    ])
    .operation_coverage;
    assert_eq!(c.ambiguous_groups, 1);
    assert_eq!(c.ambiguous_events, 3);
    assert_eq!(c.ambiguous_pairs, None);
    assert_eq!(c.paired_events, 0);
    let c = analyze(&[r#"Operation "work" started"#]).operation_coverage;
    assert_eq!(c.suppressed_events, 1);
    assert_eq!(
        c.suppressed_operation_types[0].reason,
        "no_recognized_command_completion"
    );
}

#[test]
fn inferred_years_and_equal_timestamp_files_cannot_invent_pairs() {
    let config = config::load_builtin_template("service-api").unwrap();
    let mut logs = ["sent", "completed"].iter().enumerate().map(|(i, marker)| parser::parse_log_entry_with_config(&format!("core (demo) | 2026-01-01T00:00:0{i}.000Z [INFO] Request \"work\" [0--a] {marker}"), i+1, &config).unwrap()).collect::<Vec<_>>();
    logs[0].timestamp_year_inferred = true;
    let result =
        perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &config);
    assert!(result.operations.is_empty());
    assert_eq!(result.operation_coverage.rejected_pairs, 1);
    assert_eq!(result.operation_coverage.rejected_events, 2);
    assert_eq!(result.operation_coverage.capture_window.elapsed_ms, None);
    logs[0].timestamp_year_inferred = false;
    logs[1].timestamp = logs[0].timestamp;
    logs[0].source_file = Some("first.log".into());
    logs[1].source_file = Some("second.log".into());
    for _ in 0..2 {
        let result =
            perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &config);
        assert!(result.operations.is_empty());
        assert_eq!(result.operation_coverage.ambiguous_events, 2);
        assert_eq!(result.operation_coverage.ambiguous_pairs, None);
        logs.reverse();
    }
}

#[test]
fn cli_coverage_survives_selection_and_redaction() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("generic.log");
    fs::write(&file, "core (demo) | 2026-01-01T00:00:00.000Z [INFO] worker alive\ncore (demo) | 2026-01-01T00:00:04.000Z [INFO] worker alive\n").unwrap();
    for json in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
        command.args(["--preset", "service-api"]);
        if json {
            command.arg("-j");
        }
        let output = command.arg("perf").arg(&file).output().unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        if json {
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(
                value["operation_coverage"]["capture_window"]["elapsed_ms"],
                4000
            );
            assert_eq!(
                value["operation_coverage"]["status"],
                "no_applicable_events"
            );
            assert_eq!(value["coverage"]["parsed_entries"], 2);
        } else {
            assert!(text.contains("Operation coverage: no_applicable_events"));
            assert!(text.contains("4000ms"));
            assert!(text.contains("Upstream export completeness: unknown"));
        }
    }
    for id in ["unknown", "Request", "2026-01-01T00:00:00+00:00"] {
        fs::write(&file, format!("core (demo) | 2026-01-01T00:00:00.000Z [INFO] Request \"work\" [0--{id}] sent\ncore (demo) | 2026-01-01T00:00:01.000Z [INFO] Request \"work\" [0--{id}] completed\n")).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
            .args([
                "--preset",
                "service-api",
                "--redact",
                "--mask-id",
                "correlation_id",
                "-j",
                "perf",
            ])
            .arg(&file)
            .args(["--orphans-only", "--top-n", "1"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["operation_coverage"]["paired_events"], 2);
        assert_eq!(
            value["operation_coverage"]["upstream_export_completeness"],
            "unknown"
        );
        assert_eq!(
            value["operation_coverage"]["capture_window"]["elapsed_ms"],
            1000
        );
    }
}

#[test]
fn opaque_event_ids_cannot_replace_analytic_labels_or_typed_timestamps() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("events.log");
    for id in [
        "unknown",
        "Event",
        "observed_pairs",
        "2026-01-01T00:00:00+00:00",
    ] {
        let payload = serde_json::json!({"key": id});
        fs::write(&file, format!("core (demo) | 2026-01-01T00:00:00Z [INFO] Received event of type {{\"name\":\"work\"}} with payload {payload}\ncore (demo) | 2026-01-01T00:00:01Z [INFO] Emit event of type \"work\" with payload {payload}\n")).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
            .args([
                "--preset",
                "eyes",
                "--redact",
                "--mask-id",
                "correlation_id",
                "-j",
                "perf",
            ])
            .arg(&file)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["operation_coverage"]["status"], "observed_pairs");
        assert_eq!(
            value["operation_coverage"]["upstream_export_completeness"],
            "unknown"
        );
        assert_eq!(value["operations"][0]["op_type"], "Event");
        assert!(
            value["operations"][0]["correlation_id"]
                .as_str()
                .unwrap()
                .starts_with("[MASKED_ID:")
        );
        chrono::DateTime::parse_from_rfc3339(
            value["operations"][0]["start_time"].as_str().unwrap(),
        )
        .unwrap();
    }
}
