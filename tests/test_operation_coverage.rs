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
    let result = analyze(&[r#"Operation "work" started"#]);
    assert_eq!(result.operation_coverage.suppressed_events, 0);
    assert_eq!(result.operation_coverage.unmatched_events, 1);
    assert_eq!(result.unmatched_events[0].reason, "missing_end");
    assert_eq!(result.orphans.len(), 1);
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
    assert!(result.orphans.is_empty());
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
        assert!(result.orphans.is_empty());
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
        let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
            .args([
                "--preset",
                "eyes",
                "--redact",
                "--mask-id",
                "correlation_id",
                "perf",
            ])
            .arg(&file)
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            text.contains("Operation coverage: observed_pairs"),
            "{text}"
        );
        assert!(
            text.contains("Upstream export completeness: unknown"),
            "{text}"
        );
        assert!(text.contains("[MASKED_ID:"), "{text}");
    }
}

#[test]
fn normalized_rows_on_one_line_keep_their_source_order_at_equal_timestamps() {
    use log_analyzer::normalize::NormalizationRules;
    use std::collections::BTreeMap;
    let mut config = config::load_builtin_template("service-api").unwrap();
    config.normalization = Some(NormalizationRules {
        root_path: "/rows".into(),
        expand_rows: true,
        fields: BTreeMap::from([
            ("timestamp".into(), "/time".into()),
            ("message".into(), "/message".into()),
            ("component".into(), "/component".into()),
            ("component_id".into(), "/scope".into()),
        ]),
        ..Default::default()
    });
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("rows.jsonl");
    let rows = serde_json::json!({"rows":[
        {"time":"2026-01-01T00:00:00Z","component":"core","scope":"demo","message":"Request \"work\" [0--a] sent"},
        {"time":"2026-01-01T00:00:00Z","component":"core","scope":"demo","message":"Request \"work\" [0--a] completed"}
    ]});
    fs::write(&file, rows.to_string()).unwrap();
    let report = parser::parse_log_file_report(&file, &config).unwrap();
    assert_eq!(report.entries.len(), 2);
    let result = perf_analyzer::analyze_performance_with_config(
        &report.entries,
        &LogFilter::new(),
        None,
        &config,
    );
    assert_eq!(result.operations.len(), 1);
    assert_eq!(result.operations[0].duration_ms, 0);
    assert_eq!(
        result.operations[0].start_source.row_path.as_deref(),
        Some("/rows/0")
    );
    let mut duplicate = report.entries.clone();
    duplicate[1].source_row_path = duplicate[0].source_row_path.clone();
    let result = perf_analyzer::analyze_performance_with_config(
        &duplicate,
        &LogFilter::new(),
        None,
        &config,
    );
    assert!(result.operations.is_empty());
    assert_eq!(result.operation_coverage.ambiguous_events, 2);
}

#[test]
fn yearless_new_year_lifecycle_is_rejected_before_date_sorting() {
    use log_analyzer::parser::{LogEntryKind, RequestDirection};
    let config = config::load_builtin_template("service-api").unwrap();
    let mut logs = [
        "Dec 31 23:59:59 host worker[1]: begin",
        "Jan  1 00:00:01 host worker[1]: end",
    ]
    .iter()
    .enumerate()
    .map(|(i, line)| {
        let mut entry = parser::parse_log_entry_with_config(line, i + 1, &config).unwrap();
        entry.component_id = "demo".into();
        entry.kind = LogEntryKind::Request {
            request: "work".into(),
            request_id: Some("a".into()),
            direction: if i == 0 {
                RequestDirection::Send
            } else {
                RequestDirection::Receive
            },
            endpoint: None,
            payload: None,
        };
        parser::attach_legacy_event_evidence(&mut entry, &config::AnalyzerConfig::default());
        entry
    })
    .collect::<Vec<_>>();
    assert!(logs.iter().all(|entry| entry.timestamp_year_inferred));
    logs.sort_by_key(|entry| entry.timestamp);
    let result =
        perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &config);
    assert!(result.operations.is_empty());
    assert_eq!(result.operation_coverage.rejected_pairs, 1);
    assert!(result.orphans.is_empty());
    assert_eq!(result.operation_coverage.rejected_events, 2);
    assert!(
        result
            .unmatched_events
            .iter()
            .all(|event| event.reason == "incomplete_timestamp_year")
    );
}

#[test]
fn redacted_performance_masks_ids_in_late_appended_parse_coverage_filenames() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("capture-sensitive-id.log");
    let payload = serde_json::json!({"key":"sensitive-id"});
    fs::write(&file, format!("core (demo) | 2026-01-01T00:00:00Z [INFO] Received event of type {{\"name\":\"work\"}} with payload {payload}\ncore (demo) | 2026-01-01T00:00:01Z [INFO] Emit event of type \"work\" with payload {payload}\n")).unwrap();
    for format in ["json", "text"] {
        let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
            .args([
                "--preset",
                "eyes",
                "--redact",
                "--mask-id",
                "correlation_id",
                "-F",
                format,
                "perf",
            ])
            .arg(&file)
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains("sensitive-id"), "{text}");
        assert!(text.contains("[MASKED_ID:"), "{text}");
    }
}

#[test]
fn explicitly_selected_input_status_is_masked_without_changing_coverage_status() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("status.log");
    fs::write(&file, "core (demo) | 2026-01-01T00:00:00Z [INFO] Received event of type {\"name\":\"work\"} with payload {\"key\":\"a\"}\ncore (demo) | 2026-01-01T00:00:01Z [INFO] Emit event of type \"work\" with payload {\"key\":\"a\",\"statusCode\":701}\n").unwrap();
    for format in ["text", "json"] {
        let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
            .args([
                "--preset",
                "eyes",
                "--redact",
                "--mask-id",
                "status",
                "-F",
                format,
                "perf",
            ])
            .arg(&file)
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains("701"), "{text}");
        if format == "json" {
            let value: Value = serde_json::from_str(&text).unwrap();
            assert!(
                value["operations"][0]["status"]
                    .as_str()
                    .unwrap()
                    .starts_with("[MASKED_ID:")
            );
            assert_eq!(value["operation_coverage"]["status"], "observed_pairs");
        } else {
            assert!(text.contains("Operation coverage: observed_pairs"));
        }
    }
}

#[test]
fn controlled_timeline_status_paths_survive_opaque_id_collisions() {
    use log_analyzer::timeline::{EventRule, PairRule, TimelineRules, Timing};
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("timeline.log");
    let config_file = dir.path().join("config.toml");
    let mut config = config::load_builtin_template("eyes").unwrap();
    config.timeline = TimelineRules {
        events: [("begin", "Received"), ("end", "Emit"), ("absent", "absent")]
            .iter()
            .map(|(name, pattern)| EventRule {
                name: (*name).into(),
                pattern: (*pattern).into(),
                correlation_fields: vec!["component_id".into()],
            })
            .collect(),
        pairs: vec![
            PairRule {
                name: "fetch".into(),
                start_event: "begin".into(),
                end_event: "end".into(),
                timing: Timing::Measured,
            },
            PairRule {
                name: "missing".into(),
                start_event: "absent".into(),
                end_event: "end".into(),
                timing: Timing::Measured,
            },
        ],
    };
    fs::write(&config_file, toml::to_string(&config).unwrap()).unwrap();
    for id in ["boundaries_available", "insufficient_evidence"] {
        let payload = serde_json::json!({"key":id});
        fs::write(&file, format!("core (demo) | 2026-01-01T00:00:00Z [INFO] Received event of type {{\"name\":\"work\"}} with payload {payload}\ncore (demo) | 2026-01-01T00:00:01Z [INFO] Emit event of type \"work\" with payload {payload}\n")).unwrap();
        for format in ["text", "json"] {
            let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
                .arg("--config")
                .arg(&config_file)
                .args([
                    "--redact",
                    "--mask-id",
                    "correlation_id",
                    "-F",
                    format,
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
            let text = String::from_utf8(output.stdout).unwrap();
            if format == "json" {
                let value: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(value["event_timeline"]["status"], "insufficient_evidence");
                assert_eq!(
                    value["event_timeline"]["pair_coverage"]["fetch"]["status"],
                    "boundaries_available"
                );
                assert_eq!(
                    value["event_timeline"]["pair_coverage"]["missing"]["status"],
                    "insufficient_evidence"
                );
            } else {
                assert!(text.contains("boundaries_available"), "{text}");
                assert!(text.contains("insufficient_evidence"), "{text}");
            }
        }
    }
}

#[test]
fn a_later_tied_lifecycle_does_not_discard_established_sequential_pairs() {
    let config = config::load_builtin_template("service-api").unwrap();
    let mut logs = [
        (0, "sent", "first.log"),
        (1, "completed", "first.log"),
        (2, "sent", "first.log"),
        (2, "completed", "second.log"),
        (3, "sent", "first.log"),
        (4, "completed", "first.log"),
    ]
    .iter()
    .enumerate()
    .map(|(i, (time, marker, file))| {
        let mut entry = parser::parse_log_entry_with_config(
            &format!(
                "core (demo) | 2026-01-01T00:00:0{time}Z [INFO] Request \"work\" [0--a] {marker}"
            ),
            i + 1,
            &config,
        )
        .unwrap();
        entry.source_file = Some((*file).into());
        parser::attach_legacy_event_evidence(&mut entry, &config::AnalyzerConfig::default());
        entry
    })
    .collect::<Vec<_>>();
    for _ in 0..2 {
        let result =
            perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &config);
        assert_eq!(result.operations.len(), 2);
        assert!(
            result
                .operations
                .iter()
                .all(|operation| operation.duration_ms == 1000)
        );
        assert_eq!(result.operation_coverage.paired_events, 4);
        assert_eq!(result.operation_coverage.ambiguous_events, 2);
        assert_eq!(result.operation_coverage.ambiguous_groups, 1);
        assert_eq!(result.operation_coverage.status, "partial_evidence");
        assert!(result.orphans.is_empty());
        logs.reverse();
    }
}

#[test]
fn late_profile_and_normalization_diagnostics_remain_masked_in_performance_reports() {
    use log_analyzer::normalize::NormalizationRules;
    use std::collections::BTreeMap;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("input.jsonl");
    let config_file = dir.path().join("config.toml");
    let mut config = config::load_builtin_template("eyes").unwrap();
    config.profile_name = "late-id".into();
    config.normalization = Some(NormalizationRules {
        root_path: "/late-id".into(),
        expand_rows: true,
        fields: BTreeMap::from([
            ("timestamp".into(), "/time".into()),
            ("message".into(), "/message".into()),
            ("component".into(), "/component".into()),
            ("component_id".into(), "/scope".into()),
        ]),
        ..Default::default()
    });
    fs::write(&config_file, toml::to_string(&config).unwrap()).unwrap();
    let payload = serde_json::json!({"key":"late-id"});
    let rows = serde_json::json!({"late-id":[
        {"time":"2026-01-01T00:00:00Z","scope":"demo","component":"core","message":format!("Received event of type {{\"name\":\"work\"}} with payload {payload}")},
        {"time":"2026-01-01T00:00:01Z","scope":"demo","component":"core","message":format!("Emit event of type \"work\" with payload {payload}")},
        {"time":null,"message":"invalid timestamp"}
    ]});
    fs::write(&file, rows.to_string()).unwrap();
    for format in ["text", "json"] {
        let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
            .arg("--config")
            .arg(&config_file)
            .args([
                "--redact",
                "--mask-id",
                "correlation_id",
                "-F",
                format,
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
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains("late-id"), "{text}");
        assert!(text.contains("[MASKED_ID:"), "{text}");
        assert!(text.contains("observed_pairs"));
    }
}

#[test]
fn ordered_end_and_next_start_at_same_timestamp_close_before_later_ties() {
    let config = config::load_builtin_template("service-api").unwrap();
    let mut logs = [
        (0, "sent", "first.log"),
        (1, "completed", "first.log"),
        (1, "sent", "first.log"),
        (2, "completed", "first.log"),
        (2, "sent", "second.log"),
        (3, "completed", "second.log"),
    ]
    .iter()
    .enumerate()
    .map(|(i, (time, marker, file))| {
        let mut entry = parser::parse_log_entry_with_config(
            &format!(
                "core (demo) | 2026-01-01T00:00:0{time}Z [INFO] Request \"work\" [0--a] {marker}"
            ),
            i + 1,
            &config,
        )
        .unwrap();
        entry.source_file = Some((*file).into());
        parser::attach_legacy_event_evidence(&mut entry, &config::AnalyzerConfig::default());
        entry
    })
    .collect::<Vec<_>>();
    for _ in 0..2 {
        let result =
            perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &config);
        assert_eq!(result.operations.len(), 1);
        assert_eq!(result.operations[0].duration_ms, 1000);
        assert_eq!(result.operation_coverage.paired_events, 2);
        assert_eq!(result.operation_coverage.ambiguous_events, 4);
        assert!(result.orphans.is_empty());
        logs.reverse();
    }
}
