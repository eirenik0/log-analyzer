use log_analyzer::{
    comparator::LogFilter,
    config::{self, AnalyzerConfig},
    event_rules::{ClassifiedRecord, CompiledEventRules, EventRuleConfig, Phase},
    parser::{self, LogEntry, LogEntryKind},
    perf_analyzer,
};
use serde_json::{Value, json};
use std::{fs, process::Command};

fn parse(config: &AnalyzerConfig, messages: &[&str]) -> Vec<LogEntry> {
    messages
        .iter()
        .enumerate()
        .map(|(i, message)| {
            parser::parse_log_entry_with_config(
                &format!("worker (job-demo) | 2026-01-01T00:00:{i:02}.000Z [INFO] {message}"),
                i + 1,
                config,
            )
            .unwrap()
        })
        .collect()
}
fn analyze(config: &AnalyzerConfig, logs: &[LogEntry]) -> perf_analyzer::PerfAnalysisResults {
    perf_analyzer::analyze_performance_with_config(logs, &LogFilter::new(), None, config)
}
fn rules(value: Value) -> CompiledEventRules {
    CompiledEventRules::compile(
        serde_json::from_value::<EventRuleConfig>(json!({"version":1,"rules":value})).unwrap(),
    )
    .unwrap()
}
fn mapping(name: Value, phase: &str) -> Value {
    json!({"kind":"command","name":name,"phase":{"from":"literal","value":phase},"correlation_id":{"from":"field","field":"id"},"scope":[{"from":"field","field":"session"}]})
}

#[test]
fn issue_19_reproduction_is_one_1500_ms_operation_in_text_and_json() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("service-operation.log");
    fs::write(&file,"worker (job-demo) | 2026-01-01T00:00:00.000Z [INFO] Operation \"reconcile\" started with settings {\"attempt\":1}\nworker (job-demo) | 2026-01-01T00:00:01.500Z [INFO] Operation \"reconcile\" completed\n").unwrap();
    for json in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
        command.args(["--preset", "service-api"]);
        if json {
            command.arg("-j");
        }
        command.arg("perf").arg(&file);
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        if json {
            let report: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(report["operations"].as_array().unwrap().len(), 1);
            assert_eq!(report["operations"][0]["duration_ms"], 1500);
            assert_eq!(report["operation_coverage"]["status"], "observed_pairs");
        } else {
            assert!(text.contains("1500ms"), "{text}");
            assert!(text.contains("1 completed operations"), "{text}");
        }
    }
}

#[test]
fn every_shipped_profile_pairs_all_documented_command_forms() {
    for (profile, prefix, starts, ends, payload) in [
        (
            "eyes",
            "Command",
            vec!["is called"],
            vec!["finished", "finished successfully", "returned", "completed"],
            "with settings",
        ),
        (
            "custom-start",
            "Command",
            vec!["is called"],
            vec!["finished", "finished successfully", "returned", "completed"],
            "with settings",
        ),
        (
            "service-api",
            "Operation",
            vec!["started", "begin"],
            vec!["completed", "finished", "failed"],
            "settings",
        ),
        (
            "event-pipeline",
            "Stage",
            vec!["begin", "started"],
            vec!["done", "completed", "failed"],
            "config",
        ),
    ] {
        let cfg = config::load_builtin_template(profile).unwrap();
        for start in &starts {
            for end in &ends {
                for with_payload in [false, true] {
                    let suffix = if with_payload {
                        format!(
                            " {payload} {{\"status\":\"started completed failed\",\"attempt\":1}}"
                        )
                    } else {
                        String::new()
                    };
                    let first = format!("{prefix} \"work\" {start}{suffix}");
                    let last = format!("{prefix} \"work\" {end}{suffix}");
                    let logs = parse(&cfg, &[&first, &last]);
                    assert!(
                        logs.iter()
                            .all(|log| matches!(log.kind, LogEntryKind::Command { .. })),
                        "{profile}: {first} / {last}"
                    );
                    let result = analyze(&cfg, &logs);
                    assert_eq!(result.operations.len(), 1, "{profile}: {first} / {last}");
                    assert_eq!(result.operations[0].duration_ms, 1000);
                    assert_eq!(result.operations[0].scope, ["job-demo"]);
                    assert_eq!(
                        result.operations[0].status.as_deref(),
                        Some(if *end == "failed" {
                            "failure"
                        } else {
                            "success"
                        })
                    );
                    if with_payload {
                        assert_eq!(logs[0].payload().unwrap()["attempt"], 1);
                        assert!(logs[0].message.contains("[JSON removed]"));
                    }
                }
            }
        }
    }
    let cfg = config::default_config();
    let logs = parse(
        cfg,
        &[r#"Command "work" is called"#, r#"Command "work" completed"#],
    );
    assert!(logs.iter().all(
        |log| matches!(log.kind, LogEntryKind::Generic { .. }) && log.classification.is_none()
    ));
    assert!(analyze(cfg, &logs).operations.is_empty());
}

#[test]
fn cached_phases_cannot_be_changed_by_display_cleanup_or_analysis_config() {
    let cfg = config::load_builtin_template("service-api").unwrap();
    let mut logs = parse(
        &cfg,
        &[
            r#"Operation "work" started with settings {"completed":true}"#,
            r#"Operation "work" completed"#,
        ],
    );
    logs[0].message = "completed".into();
    logs[1].message = "started".into();
    let result = analyze(&AnalyzerConfig::default(), &logs);
    assert_eq!(result.operations.len(), 1);
    assert!(
        matches!(&logs[0].classification,Some(ClassifiedRecord::Event{semantics,..}) if semantics.phase==Some(Phase::Start))
    );
}

#[test]
fn arbitrary_prose_payloads_and_marker_names_do_not_invent_boundaries() {
    let cfg = config::load_builtin_template("service-api").unwrap();
    for message in [
        r#"No evidence that Operation "work" completed"#,
        r#"Operation "work" inspected and assembly completed"#,
        r#"Operation "work" inspected etc. Cleanup completed"#,
        r#"Operation "work" inspected so cleanup completed"#,
        r#"metadata="Operation \"work\" completed""#,
        r#"Operation "work" probably completed"#,
        r#"Operation "work" not completed"#,
    ] {
        let logs = parse(&cfg, &[r#"Operation "work" started"#, message]);
        let result = analyze(&cfg, &logs);
        assert!(result.operations.is_empty(), "{message}");
        assert_eq!(result.orphans.len(), 1);
        assert_eq!(result.operation_coverage.unclassified_command_records, 1);
    }
    let logs = parse(
        &cfg,
        &[
            r#"Operation "completed with settings 世界 \"quoted\"" started with settings {"phase":"completed"}"#,
            r#"Operation "completed with settings 世界 \"quoted\"" completed"#,
        ],
    );
    let result = analyze(&cfg, &logs);
    assert_eq!(result.operations.len(), 1);
    assert_eq!(
        result.operations[0].name,
        "completed with settings 世界 \"quoted\""
    );
    assert_eq!(logs[0].payload().unwrap()["phase"], "completed");
    let logs = parse(
        &cfg,
        &[r#"Operation "work" started with settings {"message":"Operation \"work\" completed"}"#],
    );
    assert_eq!(analyze(&cfg, &logs).orphans.len(), 1);
}

#[test]
fn incomplete_identity_only_and_overlapping_commands_remain_diagnostic() {
    let cfg = config::load_builtin_template("service-api").unwrap();
    for (message, reason, orphans) in [
        (r#"Operation "work" started"#, "missing_end", 1),
        (r#"Operation "work" completed"#, "missing_start", 0),
        (r#"Operation "work""#, "identity_only", 0),
    ] {
        let result = analyze(&cfg, &parse(&cfg, &[message]));
        assert!(result.operations.is_empty());
        assert_eq!(result.orphans.len(), orphans);
        assert_eq!(result.unmatched_events[0].reason, reason);
        assert_eq!(result.operation_coverage.status, "insufficient_evidence");
        assert!(result.unmatched_events[0].classification.is_some());
    }
    let logs = parse(
        &cfg,
        &[
            r#"Operation "work" started"#,
            r#"Operation "work" started"#,
            r#"Operation "work" completed"#,
            r#"Operation "work" completed"#,
        ],
    );
    let result = analyze(&cfg, &logs);
    assert!(result.operations.is_empty());
    assert_eq!(result.ambiguous_groups.len(), 1);
    assert_eq!(result.operation_coverage.ambiguous_pairs, None);
}

#[test]
fn missing_scope_offsets_and_cross_file_ties_remain_safe() {
    let mut cfg = config::load_builtin_template("service-api").unwrap();
    let mut logs = parse(
        &cfg,
        &[
            r#"Operation "work" started"#,
            r#"Operation "work" completed"#,
        ],
    );
    logs[0].component_id.clear();
    assert!(analyze(&cfg, &logs).operations.is_empty());
    logs[0].component_id = logs[1].component_id.clone();
    cfg.perf.correlation_scope_fields.clear();
    let result = analyze(&cfg, &logs);
    assert!(result.operations.is_empty());
    assert!(
        result
            .unmatched_events
            .iter()
            .all(|e| e.reason == "missing_scope_field")
    );
    cfg.perf.correlation_scope_fields = vec!["component_id".into()];
    logs[0].component_id = "other".into();
    assert!(analyze(&cfg, &logs).operations.is_empty());
    logs[0].component_id = logs[1].component_id.clone();
    logs[0].timestamp_year_inferred = true;
    assert!(analyze(&cfg, &logs).operations.is_empty());
    logs[0].timestamp_year_inferred = false;
    logs[1].timestamp = logs[0].timestamp;
    logs[0].source_file = Some("first.log".into());
    logs[1].source_file = Some("second.log".into());
    assert!(analyze(&cfg, &logs).operations.is_empty());
    let first = parser::parse_log_entry_with_config(
        r#"worker (job-demo) | 2026-01-01T01:00:00+01:00 [INFO] Operation "work" started"#,
        1,
        &cfg,
    )
    .unwrap();
    let last = parser::parse_log_entry_with_config(
        r#"worker (job-demo) | 2026-01-01T00:00:01+00:00 [INFO] Operation "work" completed"#,
        2,
        &cfg,
    )
    .unwrap();
    assert_eq!(
        first.source_timestamp.unwrap().offset().local_minus_utc(),
        3600
    );
    assert_eq!(
        analyze(&cfg, &[first, last]).operations[0].duration_ms,
        1000
    );
}

#[test]
fn structured_json_command_rules_preserve_types_and_explicit_mappings() {
    let definitions = json!([{"id":"structured","adapter":{"type":"structured","conditions":[{"field":"event","equals":"command"},{"field":"ok","equals":true}]},"mapping":{
        "kind":"command","name":{"from":"field","field":"operation"},"phase":{"from":"field","field":"phase"},"outcome":{"from":"field","field":"outcome"},"correlation_id":{"from":"field","field":"id"},"scope":[{"from":"field","field":"session"}]}}]);
    let cfg = AnalyzerConfig {
        command_rules: Some(rules(definitions)),
        ..AnalyzerConfig::default()
    };
    let mut logs = Vec::new();
    // An outcome is only legal for end, so use two end records to verify independent recognition and IDs.
    for (i, ok) in [json!(true), json!("true")].into_iter().enumerate() {
        let line=json!({"timestamp":format!("2026-01-01T00:00:0{i}Z"),"component":"worker","message":"arbitrary display text","event":"command","ok":ok,"operation":"work","phase":"end","outcome":"failure","id":"trace","session":"s"}).to_string();
        logs.push(parser::parse_log_entry_with_config(&line, i + 1, &cfg).unwrap());
    }
    assert!(matches!(logs[0].kind, LogEntryKind::Command { .. }));
    assert!(matches!(logs[1].kind, LogEntryKind::Generic { .. }));
    let result = analyze(&cfg, &logs);
    assert_eq!(result.unmatched_events[0].reason, "missing_start");
    assert_eq!(result.unmatched_events[0].scope, ["s"]);
    assert_eq!(result.operation_coverage.unclassified_command_records, 1);
}

#[test]
fn conflicting_and_invalid_classifications_are_reported_in_text_and_json() {
    let mut a = json!({"id":"a","adapter":{"type":"text","pattern":"done"},"mapping":mapping(json!({"from":"literal","value":"work"}),"end")});
    a["mapping"]["correlation_id"] = json!({"from":"literal","value":"trace"});
    a["mapping"]["scope"] = json!([{"from":"literal","value":"session"}]);
    let mut b = a.clone();
    b["id"] = json!("b");
    b["mapping"]["name"]["value"] = json!("other");
    for (definitions, reason) in [
        (json!([a, b]), "conflicting_event_rules"),
        (
            json!([{"id":"bad","adapter":{"type":"text","pattern":"done"},"mapping":mapping(json!({"from":"field","field":"missing"}),"end")}]),
            "invalid_event_data",
        ),
    ] {
        let cfg = AnalyzerConfig {
            command_rules: Some(rules(definitions)),
            ..AnalyzerConfig::default()
        };
        let logs = parse(&cfg, &["done"]);
        let result = analyze(&cfg, &logs);
        assert!(result.operations.is_empty());
        assert_eq!(result.unmatched_events[0].reason, reason);
        assert_eq!(result.operation_coverage.relevant_events, 1);
        let text = perf_analyzer::format_perf_results_text(
            &result,
            0,
            0,
            false,
            log_analyzer::cli::PerfSortOrder::Duration,
        );
        let report: Value =
            serde_json::from_str(&perf_analyzer::format_perf_results_json(&result)).unwrap();
        assert!(text.contains(reason));
        assert_eq!(report["unmatched_events"][0]["reason"], reason);
        assert!(report["unmatched_events"][0]["classification"].is_object());
    }
}

#[test]
fn command_scoped_migration_is_explicit_and_templates_stay_synchronized() {
    for (source, target) in [
        (
            "config/profiles/eyes.toml",
            ".claude/skills/analyze-logs/templates/eyes.toml",
        ),
        (
            "config/templates/custom-start.toml",
            ".claude/skills/analyze-logs/templates/custom-start.toml",
        ),
        (
            "config/templates/service-api.toml",
            ".claude/skills/analyze-logs/templates/service-api.toml",
        ),
        (
            "config/templates/event-pipeline.toml",
            ".claude/skills/analyze-logs/templates/event-pipeline.toml",
        ),
    ] {
        assert_eq!(
            fs::read_to_string(source).unwrap(),
            fs::read_to_string(target).unwrap()
        );
    }
    let cfg = config::load_builtin_template("service-api").unwrap();
    assert!(cfg.command_rules.is_some());
    assert!(!cfg.parser.request_prefix.is_empty());
    assert!(!cfg.parser.event_emit_markers.is_empty());
    let generated = log_analyzer::config_generator::generate_config(
        &parse(&cfg, &[r#"Operation "work" completed"#]),
        &cfg,
        &log_analyzer::config_generator::GenerateConfigOptions {
            profile_name: "generated".into(),
        },
    );
    assert!(std::ptr::eq(
        cfg.command_rules.as_ref().unwrap().schema(),
        generated.command_rules.as_ref().unwrap().schema()
    ));
    assert_eq!(generated.profile.known_commands, ["work"]);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("mixed.toml");
    let mut mixed = cfg;
    mixed.perf.command_completion_markers = vec!["completed".into()];
    fs::write(&file, toml::to_string_pretty(&mixed).unwrap()).unwrap();
    let message = config::load_config_from_path(&file)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("remove parser.command_prefix"),
        "{message}"
    );
    mixed.perf.command_completion_markers.clear();
    mixed.event_rules = mixed.command_rules.clone();
    assert!(mixed.validate_event_rules().is_err());
}

#[test]
fn legacy_custom_profiles_are_not_reinterpreted_as_strict_rules() {
    let mut cfg = AnalyzerConfig::default();
    cfg.parser.command_prefix = "Task ".into();
    cfg.parser.command_start_marker = " BEGIN".into();
    cfg.perf.command_start_markers = vec!["BEGIN".into()];
    cfg.perf.command_completion_markers = vec!["DONE".into()];
    let result = analyze(&cfg, &parse(&cfg, &["Task work BEGIN"]));
    assert_eq!(result.orphans.len(), 1);
    assert!(cfg.command_rules.is_none());
    assert_eq!(result.unmatched_events[0].reason, "missing_end");
    let logs = parse(&cfg, &["Task work BEGIN DONE"]);
    let result = analyze(&cfg, &logs);
    assert!(result.operations.is_empty());
    assert_eq!(result.operation_coverage.ambiguous_events, 1);
    let logs = parse(&cfg, &["Task work DONE"]);
    assert!(matches!(logs[0].kind, LogEntryKind::Generic { .. }));
}

#[test]
fn explicit_session_hints_require_lifecycle_evidence() {
    let cfg = config::load_builtin_template("eyes").unwrap();
    for (message, completed) in [
        (r#"Command "close""#, false),
        (r#"Command "close" is called"#, false),
        (r#"Command "close" completed"#, true),
    ] {
        let line = format!("worker (eyes-demo) | 2026-01-01T00:00:00Z [INFO] {message}");
        let log = parser::parse_log_entry_with_config(&line, 1, &cfg).unwrap();
        let insight = config::analyze_profile(&[log], &cfg);
        let session = &insight.sessions.levels[1].sessions["eyes-demo"];
        assert_eq!(session.completed_via.is_some(), completed);
    }
}

#[test]
fn cached_classification_redaction_preserves_typed_enum_labels() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("enum-collisions.log");
    for (id, message) in [
        ("event", r#"Operation "work" started"#),
        ("command", r#"Operation "work" started"#),
        ("start", r#"Operation "work" started"#),
        ("end", r#"Operation "work" completed"#),
        ("success", r#"Operation "work" completed"#),
        ("failure", r#"Operation "work" failed"#),
        ("invalid", r#"Operation "bad\q" completed"#),
    ] {
        fs::write(
            &file,
            format!("worker ({id}) | 2026-01-01T00:00:00Z [INFO] {message}\n"),
        )
        .unwrap();
        for json in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
            command.args([
                "--preset",
                "service-api",
                "--redact",
                "--mask-id",
                "component_id",
            ]);
            if json {
                command.arg("-j");
            }
            let output = command.arg("perf").arg(&file).output().unwrap();
            assert!(
                output.status.success(),
                "{id}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(!text.contains(&format!("worker ({id})")), "{text}");
            if json {
                let report: Value = serde_json::from_str(&text).unwrap();
                assert!(report["unmatched_events"][0]["classification"]["status"].is_string());
                if id != "invalid" {
                    assert_eq!(
                        report["unmatched_events"][0]["classification"]["semantics"]["kind"],
                        "command"
                    );
                }
            }
        }
    }
}

#[test]
fn explicit_command_payload_decoding_is_bounded_and_keeps_malformed_payloads_opaque() {
    let cfg = config::load_builtin_template("service-api").unwrap();
    for body in [
        "{\"nested\":[}",
        "{\"nested\":",
        "{a: 1} not-json",
        "{a: 1} {b: 2}",
        "[1] /* unfinished",
        "[1] /* valid */ trailing",
        &format!("{{a: 1}} {}0{}", "[".repeat(50000), "]".repeat(50000)),
        &format!("{}0{}", "[".repeat(50000), "]".repeat(50000)),
    ] {
        let message = format!("Operation \"work\" started with settings {body}");
        let logs = parse(&cfg, &[&message]);
        assert!(logs[0].payload().is_none());
        assert_eq!(analyze(&cfg, &logs).orphans.len(), 1);
        assert_eq!(logs[0].message, message);
    }
    let message = "Operation \"work\" started with settings {nested: {a: 1}, note: '[]{}', /* } ] */ key: 'completed'}";
    let logs = parse(&cfg, &[message]);
    assert_eq!(logs[0].payload().unwrap()["nested"]["a"], 1);
    assert_eq!(logs[0].payload().unwrap()["key"], "completed");
    assert_eq!(analyze(&cfg, &logs).orphans.len(), 1);
}

#[test]
fn programmatic_profiles_cannot_silently_mix_modes_or_other_operation_kinds() {
    let mut cfg = config::load_builtin_template("service-api").unwrap();
    cfg.parser.command_prefix = "Operation".into();
    assert!(
        parser::parse_log_entry_with_config("worker | 2026-01-01T00:00:00Z [INFO] alive", 1, &cfg)
            .is_err()
    );
    cfg.parser.command_prefix.clear();
    cfg.parser.command_payload_markers = vec!["with settings".into(); 17];
    assert!(
        cfg.validate_event_rules()
            .unwrap_err()
            .contains("at most 16")
    );
    let definition = json!({"id":"request","adapter":{"type":"text","pattern":"done"},"mapping":{"kind":"request","name":{"from":"literal","value":"work"},"phase":{"from":"literal","value":"end"}}});
    let cfg = AnalyzerConfig {
        command_rules: Some(rules(json!([definition]))),
        ..AnalyzerConfig::default()
    };
    assert!(
        cfg.validate_event_rules()
            .unwrap_err()
            .contains("only kind = command")
    );
}

#[test]
fn cli_command_selection_preserves_full_coverage_and_diagnostic_omissions() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("selection.log");
    let messages = [
        r#"Operation "work" started"#,
        r#"Operation "work" completed"#,
        r#"Operation "other" started"#,
        r#"Operation "identity""#,
    ];
    let lines = messages
        .iter()
        .enumerate()
        .map(|(i, message)| {
            format!("worker (job-demo) | 2026-01-01T00:00:0{i}Z [INFO] {message}\n")
        })
        .collect::<String>();
    fs::write(&file, lines).unwrap();
    for json in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
        command.args(["--preset", "service-api"]);
        if json {
            command.arg("-j");
        }
        let output = command
            .arg("perf")
            .arg(&file)
            .args(["--top-n", "1", "--orphans-only"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        if json {
            let report: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(report["operations"], json!([]));
            assert_eq!(report["operation_coverage"]["paired_events"], 2);
            assert_eq!(report["operation_coverage"]["unmatched_events"], 2);
            assert_eq!(report["unmatched_events"].as_array().unwrap().len(), 1);
            assert_eq!(
                report["unmatched_events"][0]["classification"]["semantics"]["phase"],
                "start"
            );
            assert_eq!(report["totals"]["operations"], 1);
            assert_eq!(report["omitted"]["unmatched_events"], 1);
        } else {
            assert!(text.contains("paired: 2; unmatched: 2"), "{text}");
            assert!(
                text.contains("Full totals: 1 completed operations"),
                "{text}"
            );
        }
    }
}

#[test]
fn payload_marker_limits_apply_to_both_explicit_command_entry_points() {
    let schema = config::load_builtin_template("service-api")
        .unwrap()
        .command_rules
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("limits.toml");
    for global in [false, true] {
        let mut cfg = AnalyzerConfig::default();
        if global {
            cfg.event_rules = Some(schema.clone());
        } else {
            cfg.command_rules = Some(schema.clone());
        }
        for markers in [vec!["with settings".into(); 17], vec!["x".repeat(4097)]] {
            cfg.parser.command_payload_markers = markers;
            fs::write(&file, toml::to_string_pretty(&cfg).unwrap()).unwrap();
            assert!(
                config::load_config_from_path(&file)
                    .unwrap_err()
                    .to_string()
                    .contains("command_payload_markers")
            );
            assert!(
                parser::parse_log_entry_with_config(
                    "worker | 2026-01-01T00:00:00Z [INFO] alive",
                    1,
                    &cfg
                )
                .is_err()
            );
        }
        cfg.parser.command_payload_markers = vec!["x".repeat(4096); 16];
        assert!(cfg.validate_event_rules().is_ok());
    }
}

#[test]
fn identity_only_diagnostics_preserve_inherited_and_mapped_scope() {
    let cfg = config::load_builtin_template("service-api").unwrap();
    let logs = parse(&cfg, &[r#"Operation "work""#]);
    let result = analyze(&cfg, &logs);
    assert_eq!(result.unmatched_events[0].scope, ["job-demo"]);
    let text = perf_analyzer::format_perf_results_text(
        &result,
        0,
        0,
        false,
        log_analyzer::cli::PerfSortOrder::Duration,
    );
    assert!(text.contains("Scope: job-demo"));
    assert!(result.operations.is_empty());
    assert_eq!(result.unmatched_events[0].reason, "identity_only");
    let report: Value =
        serde_json::from_str(&perf_analyzer::format_perf_results_json(&result)).unwrap();
    assert_eq!(report["unmatched_events"][0]["scope"], json!(["job-demo"]));
    let definition = json!({"id":"identity","adapter":{"type":"text","pattern":"work"},"mapping":{"kind":"command","name":{"from":"literal","value":"work"},"scope":[{"from":"literal","value":"mapped-session"}]}});
    let cfg = AnalyzerConfig {
        command_rules: Some(rules(json!([definition]))),
        ..AnalyzerConfig::default()
    };
    let result = analyze(&cfg, &parse(&cfg, &["work"]));
    assert_eq!(result.unmatched_events[0].scope, ["mapped-session"]);
    assert!(result.operations.is_empty());
}

#[test]
fn explicit_payload_accepts_only_trailing_trivia_and_searches_overlapping_markers() {
    let mut cfg = config::load_builtin_template("service-api").unwrap();
    for tail in [
        " \t",
        " /* trailing []{} */ ",
        " // trailing\n /* another */ ",
    ] {
        let message = format!("Operation \"work\" started with settings {{a: 1}}{tail}");
        let logs = parse(&cfg, &[&message]);
        assert_eq!(logs[0].payload().unwrap()["a"], 1);
        assert!(logs[0].message.ends_with("[JSON removed]"));
    }
    cfg.parser.command_payload_markers = vec!["aaa".into(), "aaaa".into()];
    let message = "Operation \"aaa {name}\" started with settings aaaa {a: 2}";
    let logs = parse(&cfg, &[message]);
    assert_eq!(logs[0].payload().unwrap()["a"], 2);
    assert_eq!(
        logs[0].message,
        "Operation \"aaa {name}\" started with settings aaaa [JSON removed]"
    );
}

#[test]
fn maximum_marker_configuration_handles_repeated_prefixes_in_a_large_message() {
    let mut cfg = config::load_builtin_template("service-api").unwrap();
    cfg.command_rules = Some(rules(json!([{
        "id":"large-command", "adapter":{"type":"text","pattern":".*"},
        "mapping":{"kind":"command","name":{"from":"literal","value":"work"},
        "phase":{"from":"literal","value":"start"},
        "correlation_id":{"from":"literal","value":"work"}}
    }])));
    cfg.parser.command_payload_markers = (0..16)
        .map(|i| format!("{}{}", "a".repeat(4095), char::from(b'b' + i)))
        .collect();
    let message = format!(
        "Operation \"work\" started with settings {}",
        "a".repeat(1_000_000)
    );
    let logs = parse(&cfg, &[&message]);
    assert!(matches!(logs[0].kind, LogEntryKind::Command { .. }));
    assert!(logs[0].payload().is_none());
    assert_eq!(logs[0].message, message);
    assert_eq!(analyze(&cfg, &logs).orphans.len(), 1);
}

#[test]
fn whitespace_markers_do_not_repeatedly_scan_a_large_suffix() {
    let mut cfg = config::load_builtin_template("service-api").unwrap();
    cfg.command_rules = Some(rules(json!([{
        "id":"large-command", "adapter":{"type":"text","pattern":".*"},
        "mapping":{"kind":"command","name":{"from":"literal","value":"work"},
        "phase":{"from":"literal","value":"start"},
        "correlation_id":{"from":"literal","value":"work"}}
    }])));
    cfg.parser.command_payload_markers = vec![" ".into(); 16];
    let message = format!("work{}{{a: 1}}", " ".repeat(100_000));
    let logs = parse(&cfg, &[&message]);
    assert_eq!(logs[0].payload().unwrap()["a"], 1);
    assert_eq!(analyze(&cfg, &logs).orphans.len(), 1);
}
