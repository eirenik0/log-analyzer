use log_analyzer::{
    comparator::LogFilter,
    config::{self, AnalyzerConfig},
    event_rules::{ClassifiedRecord, CompiledEventRules, EventRuleConfig, OperationKind},
    parser::{self, LogEntry, LogEntryKind},
    perf_analyzer,
};
use serde_json::{Value, json};
use std::{fs, process::Command};

fn parse(cfg: &AnalyzerConfig, messages: &[&str]) -> Vec<LogEntry> {
    messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            parser::parse_log_entry_with_config(
                &format!("worker (demo) | 2026-01-01T00:00:{i:02}.000+02:00 [INFO] {m}"),
                i + 1,
                cfg,
            )
            .unwrap()
        })
        .collect()
}
fn analyze(cfg: &AnalyzerConfig, logs: &[LogEntry]) -> perf_analyzer::PerfAnalysisResults {
    perf_analyzer::analyze_performance_with_config(logs, &LogFilter::new(), None, cfg)
}
fn compile(rules: Value) -> CompiledEventRules {
    CompiledEventRules::compile(
        serde_json::from_value::<EventRuleConfig>(json!({"version":2,"rules":rules})).unwrap(),
    )
    .unwrap()
}
fn explicit(rules: Value) -> AnalyzerConfig {
    AnalyzerConfig {
        event_rules: Some(compile(rules)),
        ..AnalyzerConfig::default()
    }
}
fn rule(id: &str, kind: &str, phase: &str) -> Value {
    json!({"id":id,"adapter":{"type":"text","pattern":"boundary"},"mapping":{"kind":kind,"name":{"from":"literal","value":"work"},"phase":{"from":"literal","value":phase},"correlation_id":{"from":"literal","value":"a"}}})
}

#[test]
fn all_shipped_request_directions_have_explicit_phases_and_missing_ids_stay_incomplete() {
    for (profile, prefix, starts, ends) in [
        (
            "eyes",
            "Request",
            vec!["will be sent"],
            vec![
                "finished successfully",
                "respond with",
                "that was sent",
                "is going to retried",
            ],
        ),
        (
            "custom-start",
            "Request",
            vec!["will be sent"],
            vec!["finished successfully", "respond with"],
        ),
        (
            "service-api",
            "Request",
            vec!["sent", "queued", "requested"],
            vec!["completed", "responded", "failed"],
        ),
        (
            "event-pipeline",
            "Call",
            vec!["started", "dispatched"],
            vec!["done", "completed", "failed"],
        ),
    ] {
        let cfg = config::load_builtin_template(profile).unwrap();
        for start in &starts {
            for end in &ends {
                let a = format!(
                    "{prefix} \"世界 \\\"quoted\\\"\" [0--demo] {start} with body {{note:'completed'}}"
                );
                let b = format!("{prefix} \"世界 \\\"quoted\\\"\" [0--demo] {end} with body [1,2]");
                let logs = parse(&cfg, &[&a, &b]);
                let result = analyze(&cfg, &logs);
                assert_eq!(result.operations.len(), 1, "{profile}: {a}, {b}");
                assert_eq!(result.operations[0].duration_ms, 1000);
                assert_eq!(logs[0].payload().unwrap()["note"], "completed");
                assert!(
                    matches!(&logs[0].kind, LogEntryKind::Request { direction, .. } if direction.to_string()=="Send")
                );
                assert!(
                    matches!(&logs[1].kind, LogEntryKind::Request { direction, .. } if direction.to_string()=="Receive")
                );
                if *end == "failed" {
                    assert_eq!(result.operations[0].status.as_deref(), Some("failure"));
                }
                let missing = format!("{prefix} \"work\" {end}");
                let result = analyze(&cfg, &parse(&cfg, &[&missing]));
                assert!(result.operations.is_empty());
                assert_eq!(result.unmatched_events[0].reason, "missing_correlation_key");
            }
        }
    }
}

#[test]
fn all_event_forms_declare_receive_start_emit_end_and_payload_key_priority() {
    for (profile, receives, emits, separator, keys) in [
        (
            "eyes",
            vec!["Received event of type"],
            vec!["Emit event of type"],
            "with payload",
            vec!["key"],
        ),
        (
            "custom-start",
            vec!["Received event of type"],
            vec!["Emit event of type"],
            "with payload",
            vec!["key"],
        ),
        (
            "service-api",
            vec!["Consumed event", "Received event"],
            vec!["Published event"],
            "payload",
            vec!["key", "traceId", "requestId"],
        ),
        (
            "event-pipeline",
            vec!["Consumed", "Received"],
            vec!["Published", "Emitted"],
            "payload",
            vec!["key", "eventId", "traceId", "jobId"],
        ),
    ] {
        let cfg = config::load_builtin_template(profile).unwrap();
        for receive in receives {
            for emit in &emits {
                for key in &keys {
                    let a = format!("{receive} \"work\" {separator} {{\"{key}\":\"same\"}}");
                    let b = format!("{emit} \"work\" {separator} {{\"{key}\":\"same\"}}");
                    let result = analyze(&cfg, &parse(&cfg, &[&a, &b]));
                    assert_eq!(result.operations.len(), 1, "{profile}, {key}");
                    assert_eq!(result.operations[0].duration_ms, 1000);
                    assert_eq!(result.operations[0].op_type, "Event");
                }
            }
        }
        let missing = format!("{} \"work\" {separator} {{}}", emits[0]);
        let result = analyze(&cfg, &parse(&cfg, &[&missing]));
        assert_eq!(result.unmatched_events[0].reason, "missing_correlation_key");
        if keys.len() > 1 {
            let wrong = format!(
                "{} \"work\" {separator} {{\"{}\":true,\"{}\":\"same\"}}",
                emits[0], keys[0], keys[1]
            );
            assert!(matches!(
                parse(&cfg, &[&wrong])[0].classification,
                Some(ClassifiedRecord::Invalid { .. })
            ));
        }
    }
}

#[test]
fn structured_and_text_events_measure_equally_and_perf_never_reinterprets_direction_or_payload() {
    for kind in ["request", "event", "command"] {
        let mut cfg = config::load_builtin_template("service-api").unwrap();
        let text = match kind {
            "request" => vec![
                r#"Request "work" [same] sent"#,
                r#"Request "work" [same] completed"#,
            ],
            "event" => vec![
                r#"Consumed event "work" payload {"key":"same"}"#,
                r#"Published event "work" payload {"key":"same"}"#,
            ],
            _ => vec![
                r#"Operation "work" started"#,
                r#"Operation "work" completed"#,
            ],
        };
        let expected = analyze(&cfg, &parse(&cfg, &text));
        assert_eq!(expected.operations.len(), 1);
        let id = if kind == "command" { "work" } else { "same" };
        let mut logs = Vec::new();
        for (i, phase) in ["start", "end"].iter().enumerate() {
            let direction = if kind == "request" {
                if i == 0 { "send" } else { "receive" }
            } else {
                if i == 0 { "receive" } else { "emit" }
            };
            let line = json!({"timestamp":format!("2026-01-01T00:00:{i:02}+02:00"),"component":"worker","component_id":"demo","message":"unrelated wording","operation_kind":kind,"operation_name":"work","operation_phase":phase,"operation_direction":direction,"correlation_id":id}).to_string();
            let mut entry = parser::parse_log_entry_with_config(&line, i + 1, &cfg).unwrap();
            entry.message = "different message".into();
            entry.kind = LogEntryKind::Generic {
                payload: Some(json!({"key":"other"})),
            };
            logs.push(entry);
        }
        cfg.perf.event_correlation_keys = vec!["other".into()];
        cfg.perf.correlation_scope_fields = vec!["missing".into()];
        let result = analyze(&cfg, &logs);
        assert_eq!(result.operations.len(), 1, "{kind}");
        assert_eq!(
            result.operations[0].duration_ms,
            expected.operations[0].duration_ms
        );
        assert_eq!(
            result.operations[0].correlation_id,
            expected.operations[0].correlation_id
        );
        assert_eq!(result.operations[0].op_type, expected.operations[0].op_type);
        assert!(result.operations[0].start_classification.is_some());
    }
}

#[test]
fn phase_is_independent_of_transport_direction_and_unknown_direction_remains_unknown() {
    let mut start = rule("start", "request", "start");
    start["adapter"]["pattern"] = json!("a");
    start["mapping"]["direction"] = json!({"from":"literal","value":"receive"});
    let mut end = rule("end", "request", "end");
    end["adapter"]["pattern"] = json!("b");
    end["mapping"]["direction"] = json!({"from":"literal","value":"send"});
    let cfg = explicit(json!([start, end]));
    let logs = parse(&cfg, &["a", "b"]);
    assert_eq!(analyze(&cfg, &logs).operations[0].duration_ms, 1000);
    let cfg = explicit(json!([rule("a", "event", "start")]));
    assert!(
        matches!(&parse(&cfg,&["boundary"])[0].kind,LogEntryKind::Event {direction,..} if direction.to_string()=="Unknown")
    );
}

#[test]
fn classification_coverage_partitions_records_and_conflicts_retain_kind_provenance() {
    let mut invalid = rule("bad", "event", "end");
    invalid["adapter"]["pattern"] = json!("bad");
    invalid["mapping"]["name"] = json!({"from":"field","field":"missing"});
    let cfg = explicit(json!([
        rule("a", "request", "start"),
        rule("b", "event", "end"),
        invalid
    ]));
    let logs = parse(&cfg, &["boundary", "bad", "unknown"]);
    let result = analyze(&cfg, &logs);
    let c = &result.operation_coverage.classification;
    assert_eq!(
        (
            c.selected_records,
            c.conflicting_records,
            c.invalid_records,
            c.unclassified_records
        ),
        (3, 1, 1, 1)
    );
    assert_eq!(result.unmatched_events.len(), 2);
    assert!(
        matches!(&result.unmatched_events[0].classification,Some(ClassifiedRecord::Conflict {kinds,..}) if kinds.contains(&OperationKind::Request) && kinds.contains(&OperationKind::Event))
    );
    let selected = perf_analyzer::analyze_performance_with_config(
        &logs,
        &LogFilter::new(),
        Some("Request"),
        &cfg,
    );
    assert_eq!(
        selected.operation_coverage.classification.selected_records,
        3
    );
    assert_eq!(selected.unmatched_events.len(), 1);
    assert_eq!(selected.operation_coverage.suppressed_events, 1);
    let filtered = perf_analyzer::analyze_performance_with_config(
        &logs,
        &LogFilter::new().contains_text(Some("unknown")),
        None,
        &cfg,
    );
    assert_eq!(
        filtered.operation_coverage.classification.selected_records,
        1
    );
}

#[test]
fn unknown_request_prose_missing_scope_and_uncached_library_records_never_measure() {
    let cfg = config::load_builtin_template("service-api").unwrap();
    let mut logs = parse(
        &cfg,
        &[
            r#"Request "work" [same] sent"#,
            r#"No evidence Request "work" [same] completed"#,
        ],
    );
    assert_eq!(
        analyze(&cfg, &logs)
            .operation_coverage
            .classification
            .unclassified_records,
        1
    );
    logs[0] = parser::parse_log_entry_with_config(
        r#"worker | 2026-01-01T00:00:00Z [INFO] Request "work" [same] sent"#,
        1,
        &cfg,
    )
    .unwrap();
    assert_eq!(
        analyze(&cfg, &logs).unmatched_events[0].reason,
        "missing_scope_field"
    );
    logs[0].classification = None;
    let result = analyze(&cfg, &logs);
    assert_eq!(
        result.unmatched_events[0].reason,
        "unclassified_operation_record"
    );
    assert_eq!(
        result.operation_coverage.classification.unavailable_records,
        1
    );
}

#[test]
fn cli_limits_preserve_classification_counts_and_redact_pair_rule_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("logs.log");
    fs::write(&file,concat!(
        "worker (demo) | 2026-01-01T00:00:00Z [INFO] Request \"work\" [user@example.test] sent\n",
        "worker (demo) | 2026-01-01T00:00:01Z [INFO] Request \"work\" [user@example.test] completed\n",
        "worker (demo) | 2026-01-01T00:00:02Z [INFO] unknown\n")).unwrap();
    for format in ["text", "json"] {
        for top in ["0", "1"] {
            let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
                .args([
                    "--preset",
                    "service-api",
                    "--redact",
                    "--mask-id",
                    "correlation_id",
                    "-F",
                    format,
                    "perf",
                    file.to_str().unwrap(),
                    "--top-n",
                    top,
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(!stdout.contains("user@example.test"));
            if format == "json" {
                let v: Value = serde_json::from_str(&stdout).unwrap();
                assert_eq!(
                    v["operation_coverage"]["classification"]["selected_records"],
                    3
                );
                assert_eq!(
                    v["operation_coverage"]["classification"]["classified_records"],
                    2
                );
                assert_eq!(
                    v["operation_coverage"]["classification"]["unclassified_records"],
                    1
                );
                assert_eq!(v["totals"]["operations"], 1);
                if top == "1" {
                    assert_eq!(
                        v["operations"][0]["start_classification"]["rule_ids"][0],
                        "request-start-id"
                    );
                }
            } else {
                assert!(stdout.contains("selected=3 classified=2"));
                assert!(stdout.contains("unclassified=1"));
            }
        }
    }
}

#[test]
fn explicit_event_payload_bounds_and_first_field_limits_are_validated() {
    let mut cfg = config::load_builtin_template("service-api").unwrap();
    for body in [
        "{key:'a'} trailing",
        "{key:'a'} /* unfinished",
        "{broken:[}",
    ] {
        let m = format!("Consumed event \"work\" payload {body}");
        let logs = parse(&cfg, &[&m]);
        assert!(logs[0].payload().is_none());
        assert_eq!(logs[0].message, m);
        assert_eq!(
            analyze(&cfg, &logs).unmatched_events[0].reason,
            "missing_correlation_key"
        );
    }
    cfg.parser.request_payload_markers = vec!["x".repeat(4097)];
    assert!(cfg.validate_event_rules().is_err());
    cfg.parser.request_payload_markers.clear();
    cfg.perf.correlation_scope_fields = vec!["id".into(); 17];
    assert!(cfg.validate_event_rules().is_err());
    let mut bad = rule("a", "event", "end");
    bad["mapping"]["correlation_id"] = json!({"from":"first_field","fields":vec!["a";17]});
    let schema: EventRuleConfig =
        serde_json::from_value(json!({"version":2,"rules":[bad]})).unwrap();
    assert!(CompiledEventRules::compile(schema).is_err());
}

#[test]
fn templates_generation_and_capabilities_expose_the_final_contract() {
    for name in config::builtin_template_names() {
        let cfg = config::load_builtin_template(name).unwrap();
        assert!(cfg.event_rules.is_some());
        assert!(cfg.command_rules.is_none());
        let generated = log_analyzer::config_generator::generate_config(
            &[],
            &cfg,
            &log_analyzer::config_generator::GenerateConfigOptions {
                profile_name: "generated".into(),
            },
        );
        assert!(std::ptr::eq(
            cfg.event_rules.as_ref().unwrap().schema(),
            generated.event_rules.as_ref().unwrap().schema()
        ));
        let path = if matches!(*name, "base" | "eyes") {
            format!("config/profiles/{name}.toml")
        } else {
            format!("config/templates/{name}.toml")
        };
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            fs::read_to_string(format!(".claude/skills/analyze-logs/templates/{name}.toml"))
                .unwrap()
        );
    }
    let v = log_analyzer::build_info::capabilities();
    assert_eq!(
        v["event_classification"]["kinds"],
        json!(["command", "request", "event"])
    );
    assert_eq!(v["schema_version"], 1);
}

#[test]
fn version_one_rejects_new_grammar_and_version_two_preserves_literal_payload_keys() {
    let mut definition = rule("a", "request", "start");
    definition["mapping"]["direction"] = json!({"from":"literal","value":"send"});
    let schema: EventRuleConfig =
        serde_json::from_value(json!({"version":1,"rules":[definition]})).unwrap();
    assert!(
        CompiledEventRules::compile(schema)
            .unwrap_err()
            .to_string()
            .contains("version 2")
    );
    let mut definition = rule("a", "command", "start");
    definition["mapping"]["correlation_id"] = json!({"from":"first_field","fields":["id"]});
    let schema: EventRuleConfig =
        serde_json::from_value(json!({"version":1,"rules":[definition]})).unwrap();
    assert!(CompiledEventRules::compile(schema).is_err());
    let mut definition = rule("a", "command", "start");
    definition["mapping"]["correlation_id"] = json!({"from":"field","field":"payload.key"});
    let schema: EventRuleConfig =
        serde_json::from_value(json!({"version":1,"rules":[definition]})).unwrap();
    let cfg = AnalyzerConfig {
        event_rules: Some(CompiledEventRules::compile(schema).unwrap()),
        ..AnalyzerConfig::default()
    };
    let line=json!({"timestamp":"2026-01-01T00:00:00Z","component":"worker","component_id":"demo","message":"boundary","payload.key":"literal-key"}).to_string();
    assert!(
        matches!(parser::parse_log_entry_with_config(&line,1,&cfg).unwrap().classification,Some(ClassifiedRecord::Event {semantics,..}) if semantics.correlation_id.as_deref()==Some("literal-key"))
    );
}

#[test]
fn provenance_redaction_preserves_new_typed_direction_and_kind_labels() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("labels.log");
    for id in [
        "send", "receive", "request", "event", "start", "end", "failure",
    ] {
        fs::write(&file,format!("worker (demo) | 2026-01-01T00:00:00Z [INFO] Request \"work\" [{id}] sent\nworker (demo) | 2026-01-01T00:00:01Z [INFO] Request \"work\" [{id}] failed\n")).unwrap();
        for format in ["text", "json"] {
            let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
                .args([
                    "--preset",
                    "service-api",
                    "--redact",
                    "--mask-id",
                    "correlation_id",
                    "-F",
                    format,
                    "perf",
                    file.to_str().unwrap(),
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{id}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            if format == "json" {
                let v: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(
                    v["operations"][0]["start_classification"]["semantics"]["direction"],
                    "send"
                );
                assert_eq!(
                    v["operations"][0]["end_classification"]["semantics"]["outcome"],
                    "failure"
                );
                assert_eq!(
                    v["operations"][0]["end_classification"]["semantics"]["kind"],
                    "request"
                );
                assert_ne!(v["operations"][0]["correlation_id"], id);
            }
        }
    }
}

#[test]
fn invalid_sibling_evidence_is_visible_for_every_matched_kind() {
    let request = rule("request", "request", "start");
    let mut invalid = rule("invalid", "event", "end");
    invalid["mapping"]["name"] = json!({"from":"field","field":"missing"});
    let cfg = explicit(json!([request, invalid]));
    let logs = parse(&cfg, &["boundary"]);
    let result = perf_analyzer::analyze_performance_with_config(
        &logs,
        &LogFilter::new(),
        Some("Request"),
        &cfg,
    );
    assert_eq!(result.unmatched_events.len(), 1);
    assert_eq!(result.unmatched_events[0].reason, "invalid_event_data");
    assert_eq!(result.operation_coverage.suppressed_events, 0);
    assert_eq!(result.operation_coverage.classification.invalid_records, 1);
}

#[test]
fn undefined_compatibility_never_rewrites_event_identity_or_json5_keys() {
    let cfg = config::load_builtin_template("service-api").unwrap();
    let logs = parse(
        &cfg,
        &[
            r#"Consumed event "work" payload {key:'undefined-task', undefined:1, missing:/*comment*/undefined, values:[undefined, /*comment*/undefined, 'undefined']}"#,
            r#"Published event "work" payload {key:'null-task'}"#,
        ],
    );
    let payload = logs[0].payload().unwrap();
    assert_eq!(payload["key"], "undefined-task");
    assert_eq!(payload["undefined"], 1);
    assert_eq!(payload["missing"], Value::Null);
    assert_eq!(payload["values"], json!([null, null, "undefined"]));
    let result = analyze(&cfg, &logs);
    assert!(result.operations.is_empty());
    assert_eq!(result.unmatched_events.len(), 2);
    assert_eq!(
        result.operation_coverage.classification.classified_records,
        2
    );
}

#[test]
fn unknown_direction_can_be_included_and_excluded_through_public_cli_filters() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("direction.log");
    fs::write(
        &file,
        concat!(
            "worker (demo) | 2026-01-01T00:00:00Z [INFO] Request \"identity\"\n",
            "worker (demo) | 2026-01-01T00:00:01Z [INFO] Request \"lifecycle\" [a] sent\n"
        ),
    )
    .unwrap();
    for expression in ["d:unknown", "!d:unknown"] {
        for format in ["text", "json"] {
            let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
                .args([
                    "--preset",
                    "service-api",
                    "-F",
                    format,
                    "search",
                    file.to_str().unwrap(),
                    "-f",
                    expression,
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8(output.stdout).unwrap();
            let (present, absent) = if expression == "d:unknown" {
                ("identity", "lifecycle")
            } else {
                ("lifecycle", "identity")
            };
            assert!(stdout.contains(present), "{stdout}");
            assert!(!stdout.contains(absent), "{stdout}");
        }
    }
    let error = log_analyzer::filter::FilterExpression::parse("d:nonsense")
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown"));
}
