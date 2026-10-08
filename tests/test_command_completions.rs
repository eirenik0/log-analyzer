use log_analyzer::{
    comparator::LogFilter,
    config,
    parser::{self, LogEntryKind},
    perf_analyzer,
};
use serde_json::Value;
use std::{fs, process::Command};

fn parse(message: &str, index: usize, config: &config::AnalyzerConfig) -> parser::LogEntry {
    parser::parse_log_entry_with_config(
        &format!("worker (job-demo) | 2026-01-01T00:00:0{index}.000Z [INFO] {message}"),
        index + 1,
        config,
    )
    .unwrap()
}

#[test]
fn shipped_command_profiles_pair_starts_with_every_configured_completion() {
    for (preset, noun, start) in [
        ("eyes", "Command", "is called"),
        ("custom-start", "Command", "is called"),
        ("service-api", "Operation", "started"),
        ("event-pipeline", "Stage", "begin"),
    ] {
        let config = config::load_builtin_template(preset).unwrap();
        for completion in &config.perf.command_completion_markers {
            let start = parse(
                &format!("{noun} \"reconcile\" {start} with settings {{\"attempt\":1}}"),
                0,
                &config,
            );
            let end = parse(&format!("{noun} \"reconcile\" {completion}"), 1, &config);
            assert!(
                matches!(&start.kind, LogEntryKind::Command {command,settings:Some(settings)} if command=="reconcile" && settings["attempt"]==1),
                "{preset}: {:?}",
                start.kind
            );
            assert!(
                matches!(&end.kind, LogEntryKind::Command {command,..} if command=="reconcile"),
                "{preset}: {:?}",
                end.kind
            );
            let results = perf_analyzer::analyze_performance_with_config(
                &[start, end],
                &LogFilter::new(),
                None,
                &config,
            );
            assert_eq!(results.operations.len(), 1, "{preset} {completion}");
            assert_eq!(results.operations[0].duration_ms, 1000);
            assert_eq!(results.operation_coverage.status, "observed_pairs");
            assert!(results.orphans.is_empty());
        }
    }
}

#[test]
fn missing_boundaries_stay_diagnostic_without_invented_operations() {
    let config = config::load_builtin_template("service-api").unwrap();
    let start = parse(r#"Operation "work" started"#, 0, &config);
    let end = parse(r#"Operation "work" completed"#, 1, &config);
    let start_only =
        perf_analyzer::analyze_performance_with_config(&[start], &LogFilter::new(), None, &config);
    assert!(start_only.operations.is_empty());
    assert_eq!(start_only.operation_coverage.suppressed_events, 1);
    let end_only = perf_analyzer::analyze_performance_with_config(
        &[end.clone()],
        &LogFilter::new(),
        None,
        &config,
    );
    assert!(end_only.operations.is_empty());
    assert_eq!(end_only.unmatched_events[0].reason, "missing_start");
    let unknown = parse(r#"Operation "other" inspected"#, 0, &config);
    assert!(matches!(unknown.kind, LogEntryKind::Command { .. }));
    let result = perf_analyzer::analyze_performance_with_config(
        &[unknown, end],
        &LogFilter::new(),
        None,
        &config,
    );
    assert_eq!(
        result.operation_coverage.suppressed_operation_types[0].reason,
        "no_recognized_boundary"
    );
}

#[test]
fn quoted_names_handle_escapes_and_utf8_and_reject_malformed_names() {
    let config = config::load_builtin_template("service-api").unwrap();
    let start = parse(r#"Operation "récon\"cile👩‍💻" started"#, 0, &config);
    let end = parse(r#"Operation "récon\"cile👩‍💻" completed"#, 1, &config);
    let result = perf_analyzer::analyze_performance_with_config(
        &[start, end],
        &LogFilter::new(),
        None,
        &config,
    );
    assert_eq!(result.operations.len(), 1);
    assert_eq!(result.operations[0].name, "récon\"cile👩‍💻");
    for message in [
        r#"Operation "" completed"#,
        r#"Operation "   " completed"#,
        r#"Operation "unfinished completed"#,
    ] {
        assert!(
            matches!(
                parse(message, 0, &config).kind,
                LogEntryKind::Generic { .. }
            ),
            "{message}"
        );
    }
    let base = config::load_builtin_template("base").unwrap();
    assert!(matches!(
        parse(r#"Operation "work" completed"#, 0, &base).kind,
        LogEntryKind::Generic { .. }
    ));
}

#[test]
fn payload_prefixes_cannot_steal_request_classification_and_custom_names_remain_supported() {
    let mut config = config::load_builtin_template("service-api").unwrap();
    let request = parse(
        r#"Request "fetch" [0--a] sent with body {"Operation ":"work"}"#,
        0,
        &config,
    );
    assert!(
        matches!(request.kind, LogEntryKind::Request { .. }),
        "{:?}",
        request.kind
    );
    let generic = parse(r#"payload {"Operation ":"work"}"#, 0, &config);
    assert!(matches!(generic.kind, LogEntryKind::Generic { .. }));
    config.parser.command_prefix = "Task: ".into();
    config.parser.command_start_marker = " started".into();
    assert!(
        matches!(parse("Task: work started",0,&config).kind,LogEntryKind::Command {command,..} if command=="work")
    );
    config.parser.command_start_marker.clear();
    assert!(
        matches!(parse("Task: 'work' completed",0,&config).kind,LogEntryKind::Command {command,..} if command=="work")
    );
    assert!(matches!(
        parse("Task: work completed", 0, &config).kind,
        LogEntryKind::Generic { .. }
    ));
}

#[test]
fn service_api_cli_reproduction_measures_1500ms_in_text_and_json() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("service-operation.log");
    fs::write(&file,"worker (job-demo) | 2026-01-01T00:00:00.000Z [INFO] Operation \"reconcile\" started with settings {\"attempt\":1}\nworker (job-demo) | 2026-01-01T00:00:01.500Z [INFO] Operation \"reconcile\" completed\n").unwrap();
    for format in ["text", "json"] {
        let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
            .args(["--preset", "service-api", "-F", format, "perf"])
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
            assert_eq!(value["operations"].as_array().unwrap().len(), 1);
            assert_eq!(value["operations"][0]["duration_ms"], 1500);
            assert_eq!(value["operations"][0]["name"], "reconcile");
            assert_eq!(value["operation_coverage"]["paired_events"], 2);
        } else {
            assert!(text.contains("reconcile"), "{text}");
            assert!(text.contains("1500"), "{text}");
            assert!(text.contains("Operation coverage: observed_pairs"));
        }
    }
}

#[test]
fn lifecycle_words_and_payload_markers_inside_names_or_json_are_not_boundaries() {
    let config = config::load_builtin_template("service-api").unwrap();
    for name in ["completed", "started", "with settings"] {
        let start = parse(
            &format!("Operation \"{name}\" started with settings {{\"attempt\":1}}"),
            0,
            &config,
        );
        let end = parse(&format!("Operation \"{name}\" completed"), 1, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1, "{name}");
        assert_eq!(result.operations[0].name, name);
    }
    for message in [
        r#"Operation "completed" started"#,
        r#"Operation "work" inspected details {"note":"completed"}"#,
    ] {
        let result = perf_analyzer::analyze_performance_with_config(
            &[parse(message, 0, &config)],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty());
        assert!(result.unmatched_events.is_empty());
        assert_eq!(result.operation_coverage.suppressed_events, 1);
    }
}

#[test]
fn leading_json_context_does_not_hide_commands_and_crossed_brackets_never_panic() {
    let config = config::load_builtin_template("service-api").unwrap();
    for prefix in [
        "context {} ",
        "context [] ",
        "context {\"Operation \":\"ignored\"} ",
    ] {
        let start = parse(
            &format!("{prefix}Operation \"reconcile\" started"),
            0,
            &config,
        );
        let end = parse(
            &format!("{prefix}Operation \"reconcile\" completed"),
            1,
            &config,
        );
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1, "{prefix}");
        assert_eq!(result.operations[0].duration_ms, 1000);
    }
    for brackets in ["[}", "{]", "[{]}", "{[}]", "[{", "é[}💻"] {
        let start = parse(
            &format!("Operation \"work\" started {brackets}"),
            0,
            &config,
        );
        let end = parse(
            &format!("Operation \"work\" completed {brackets}"),
            1,
            &config,
        );
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1, "{brackets}");
        // The shared JSON probe also handles arbitrary generic log content.
        parse(&format!("heartbeat {brackets}"), 0, &config);
    }
}

#[test]
fn json5_comments_keep_apostrophes_quotes_and_brackets_opaque() {
    let config = config::load_builtin_template("service-api").unwrap();
    for payload in [
        "{foo: 1 // user's choice\n}",
        "{foo: 1 /* user's \"choice [} */}",
        "{foo: 1 // user's choice [}\r\n}",
        "{foo: 1, text: 'it\\'s }[', /* \"unterminated quote */}",
    ] {
        let command = parse(
            &format!("Operation \"work\" started with settings {payload}"),
            0,
            &config,
        );
        assert!(
            matches!(&command.kind,LogEntryKind::Command {settings:Some(settings),..} if settings["foo"]==1),
            "{payload}: {:?}",
            command.kind
        );
        let generic = parse(&format!("heartbeat {payload}"), 0, &config);
        assert_eq!(generic.payload().unwrap()["foo"], 1, "{payload}");
        let end = parse("Operation \"work\" completed", 1, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[command, end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1);
    }
}

#[test]
fn invalid_early_candidates_do_not_hide_a_valid_later_command() {
    let config = config::load_builtin_template("service-api").unwrap();
    for context in [
        r#"previous=Operation "" "#,
        r#"previous=Operation "  " "#,
        r#"previous=Operation "unfinished "#,
    ] {
        let start = parse(&format!("{context}Operation \"work\" started"), 0, &config);
        let end = parse(
            &format!("{context}Operation \"work\" completed"),
            1,
            &config,
        );
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1, "{context}");
        assert_eq!(result.operations[0].name, "work");
    }
}

#[test]
fn many_unmatched_openers_do_not_prevent_command_classification_or_pairing() {
    let config = config::load_builtin_template("service-api").unwrap();
    let context = "{".repeat(50_000);
    let start = parse(&format!("{context} Operation \"work\" started"), 0, &config);
    let end = parse(
        &format!("{context} Operation \"work\" completed"),
        1,
        &config,
    );
    let result = perf_analyzer::analyze_performance_with_config(
        &[start, end],
        &LogFilter::new(),
        None,
        &config,
    );
    assert_eq!(result.operations.len(), 1);
    assert_eq!(result.operations[0].duration_ms, 1000);
}

#[test]
fn custom_quoted_names_can_keep_adjacent_lifecycle_wording() {
    let mut config = config::load_builtin_template("service-api").unwrap();
    config.parser.command_start_marker = "\"started".into();
    let start = parse("Operation \"work\"started", 0, &config);
    let end = parse("Operation \"work\"completed", 1, &config);
    let result = perf_analyzer::analyze_performance_with_config(
        &[start, end],
        &LogFilter::new(),
        None,
        &config,
    );
    assert_eq!(result.operations.len(), 1);
}

#[test]
fn undecodable_payload_words_cannot_complete_an_operation() {
    let config = config::load_builtin_template("service-api").unwrap();
    for payload in [
        r#"{"note":"completed",,}"#,
        r#"{"note":"completed""#,
        r#"[{"note":"completed"}] trailing"#,
        r#"[{"note":"completed"}]"#,
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let inspect = parse(
            &format!("Operation \"work\" inspected {payload}"),
            1,
            &config,
        );
        let other = parse(r#"Operation "other" completed"#, 2, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, inspect, other],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty(), "{payload}");
        assert_eq!(result.orphans.len(), 1);
        assert_eq!(result.orphans[0].name, "work");
        assert!(
            result
                .operation_coverage
                .suppressed_operation_types
                .iter()
                .any(|s| s.reason == "no_recognized_boundary")
        );
    }
}

#[test]
fn invalid_payload_command_keys_cannot_steal_requests_or_pair_with_commands() {
    let config = config::load_builtin_template("service-api").unwrap();
    let start = parse(r#"Operation "work" started"#, 0, &config);
    let request = parse(
        r#"Request "fetch" [0--request] sent with body {"Operation ":"work","note":"completed",,}"#,
        1,
        &config,
    );
    assert!(
        matches!(request.kind, LogEntryKind::Request { .. }),
        "{:?}",
        request.kind
    );
    let other = parse(r#"Operation "other" completed"#, 2, &config);
    let result = perf_analyzer::analyze_performance_with_config(
        &[start, request, other],
        &LogFilter::new(),
        None,
        &config,
    );
    assert!(result.operations.is_empty());
    let generic = parse(
        r#"payload {"Operation ":"work","note":"completed",,}"#,
        0,
        &config,
    );
    assert!(matches!(generic.kind, LogEntryKind::Generic { .. }));
}

#[test]
fn multiple_valid_subjects_cannot_attribute_a_completion_to_the_first_command() {
    let config = config::load_builtin_template("service-api").unwrap();
    let start = parse(r#"Operation "old" started"#, 0, &config);
    let ambiguous = parse(r#"Operation "old" Operation "work" completed"#, 1, &config);
    assert!(matches!(ambiguous.kind, LogEntryKind::Generic { .. }));
    let other = parse(r#"Operation "other" completed"#, 2, &config);
    let result = perf_analyzer::analyze_performance_with_config(
        &[start, ambiguous, other],
        &LogFilter::new(),
        None,
        &config,
    );
    assert!(result.operations.is_empty());
    assert_eq!(result.orphans.len(), 1);
    assert_eq!(result.orphans[0].name, "old");
}

#[test]
fn configured_unfinished_payloads_keep_command_looking_values_opaque() {
    let config = config::load_builtin_template("service-api").unwrap();
    for payload in [
        r#"{note:'Operation "work" completed'"#,
        r#"[{note:'Operation "work" completed'}"#,
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let request = parse(
            &format!("Request \"fetch\" [0--request] sent with body {payload}"),
            1,
            &config,
        );
        assert!(
            matches!(request.kind, LogEntryKind::Request { .. }),
            "{:?}",
            request.kind
        );
        let other = parse(r#"Operation "other" completed"#, 2, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, request, other],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty(), "{payload}");
        assert!(result.orphans.iter().any(|orphan| orphan.name == "work"));
        let generic = parse(&format!("payload {payload}"), 0, &config);
        assert!(matches!(generic.kind, LogEntryKind::Generic { .. }));
    }
    let start = parse(
        r#"context with body {} Operation "work" started"#,
        0,
        &config,
    );
    let end = parse(
        r#"context with body {} Operation "work" completed"#,
        1,
        &config,
    );
    let result = perf_analyzer::analyze_performance_with_config(
        &[start, end],
        &LogFilter::new(),
        None,
        &config,
    );
    assert_eq!(result.operations.len(), 1);
}

#[test]
fn unfinished_json_like_payloads_are_opaque_without_configured_marker_words() {
    let config = config::load_builtin_template("service-api").unwrap();
    for payload in [
        r#"{note:'Operation "work" completed'"#,
        r#"{"note":'Operation "work" completed'"#,
        r#"['Operation "work" completed'"#,
        r#"[[{note:'Operation "work" completed'}"#,
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let request = parse(
            &format!("Request \"fetch\" [0--request] sent {payload}"),
            1,
            &config,
        );
        assert!(
            matches!(request.kind, LogEntryKind::Request { .. }),
            "{payload}: {:?}",
            request.kind
        );
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, request],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty());
    }
}

#[test]
fn braces_inside_command_names_cannot_hide_another_subject() {
    let config = config::load_builtin_template("service-api").unwrap();
    let start = parse(r#"Operation "{" started"#, 0, &config);
    assert!(matches!(&start.kind,LogEntryKind::Command {command,..} if command=="{"));
    let ambiguous = parse(r#"Operation "{" Operation "work" completed"#, 1, &config);
    assert!(matches!(ambiguous.kind, LogEntryKind::Generic { .. }));
    let other = parse(r#"Operation "other" completed"#, 2, &config);
    let result = perf_analyzer::analyze_performance_with_config(
        &[start, ambiguous, other],
        &LogFilter::new(),
        None,
        &config,
    );
    assert!(result.operations.is_empty());
}

#[test]
fn unfinished_arrays_cover_all_json5_value_and_comment_beginnings() {
    let config = config::load_builtin_template("service-api").unwrap();
    for beginning in [
        ".5",
        "-.5",
        "+.5",
        "Infinity",
        "NaN",
        "-Infinity",
        "/* choice */ .5",
        "// user's choice\n.5",
        "{}",
        "[]",
        "[[]]",
        "true",
        "null",
        "undefined",
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let generic = parse(
            &format!("[{beginning}, 'Operation \"work\" completed'"),
            1,
            &config,
        );
        assert!(
            matches!(generic.kind, LogEntryKind::Generic { .. }),
            "{beginning}: {:?}",
            generic.kind
        );
        let other = parse(r#"Operation "other" completed"#, 2, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, generic, other],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty(), "{beginning}");
    }
}

#[test]
fn quoted_context_and_crossed_json_payloads_cannot_complete_commands() {
    let config = config::load_builtin_template("service-api").unwrap();
    for message in [
        r#"note='Operation "work" completed'"#,
        r#"note='Operation "work" completed"#,
        r#"note="Operation \"work\" completed""#,
        r#"{foo:[} Operation "work" completed"#,
        r#"[{} } Operation "work" completed"#,
        r#"Request "fetch" [0--request] sent {foo:[} Operation "work" completed"#,
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let contextual = parse(message, 1, &config);
        assert!(
            !matches!(contextual.kind, LogEntryKind::Command { .. }),
            "{message}"
        );
        let other = parse(r#"Operation "other" completed"#, 2, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, contextual, other],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty(), "{message}");
    }
    for context in [
        r#"note='Operation "old" completed'"#,
        "worker's note",
        "{foo:[}",
    ] {
        let command = parse(
            &format!("Operation \"work\" completed {context}"),
            1,
            &config,
        );
        assert!(
            matches!(command.kind, LogEntryKind::Command { .. }),
            "{context}"
        );
    }
}

#[test]
fn quoted_tail_markers_cannot_invent_lifecycle_boundaries() {
    let config = config::load_builtin_template("service-api").unwrap();
    for tail in [
        "note='completed'",
        "note='completed",
        r#"note="completed""#,
        "note='started completed'",
        "note='{} completed'",
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let inspected = parse(&format!("Operation \"work\" inspected {tail}"), 1, &config);
        let other = parse(r#"Operation "other" completed"#, 2, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, inspected, other],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty(), "{tail}");
    }
    for body in [
        "note='started' completed",
        "note='{} started' completed",
        "completed note='started'",
        "worker's completed",
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let end = parse(&format!("Operation \"work\" {body}"), 1, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1, "{body}");
        assert_eq!(result.operations[0].duration_ms, 1000, "{body}");
    }
}

#[test]
fn nested_payloads_under_stray_openers_remain_opaque() {
    let config = config::load_builtin_template("service-api").unwrap();
    for payload in [
        r#"{{text:'Operation "work" completed'}"#,
        r#"{{text:'Operation "work" completed'"#,
        r#"{{text:[} Operation "work" completed"#,
        r#"{['Operation "work" completed']"#,
        r#"{note='Operation "work" completed'"#,
        r#"{note='Operation "work" completed"#,
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let contextual = parse(payload, 1, &config);
        assert!(
            !matches!(contextual.kind, LogEntryKind::Command { .. }),
            "{payload}"
        );
        let other = parse(r#"Operation "other" completed"#, 2, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, contextual, other],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty(), "{payload}");
    }
}

#[test]
fn quoted_payload_markers_and_openers_cannot_hide_later_commands() {
    let config = config::load_builtin_template("service-api").unwrap();
    for context in [
        "note='payload ['",
        "note='with settings {'",
        "note='with body ['",
        "note='payload' bracket='['",
        "{ worker's note",
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let end = parse(
            &format!("{context} Operation \"work\" completed"),
            1,
            &config,
        );
        assert!(
            matches!(end.kind, LogEntryKind::Command { .. }),
            "{context}"
        );
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1, "{context}");
        assert_eq!(result.operations[0].duration_ms, 1000, "{context}");
    }
}

#[test]
fn deeply_nested_unfinished_arrays_keep_their_contents_opaque() {
    let config = config::load_builtin_template("service-api").unwrap();
    let entry = parse(
        &format!("{}Operation \"work\" completed", "[".repeat(50_000)),
        1,
        &config,
    );
    assert!(matches!(entry.kind, LogEntryKind::Generic { .. }));
}

#[test]
fn deeply_nested_closed_payloads_are_skipped_without_decoding_inner_fragments() {
    let config = config::load_builtin_template("service-api").unwrap();
    let payload = format!("{}0{}", "[".repeat(50_000), "]".repeat(50_000));
    let entry = parse(&payload, 1, &config);
    assert!(matches!(
        entry.kind,
        LogEntryKind::Generic { payload: None }
    ));
    let entry = parse(&format!("{payload} {{following:1}}"), 1, &config);
    assert!(
        matches!(entry.kind, LogEntryKind::Generic { payload: Some(ref value) } if value["following"] == 1)
    );
}

#[test]
fn assignment_metadata_cannot_supply_lifecycle_boundaries() {
    let config = config::load_builtin_template("service-api").unwrap();
    for body in [
        "inspected status=completed",
        "inspected status = completed",
        "inspected status:completed",
        "inspected completed=false",
        "inspected phase=started status=completed",
        "inspected status=(completed)",
        "inspected status=(version 1.2 completed)",
        "inspected status=(version 1.2 (completed))",
        "inspected status=(version 1.2 completed",
        "inspected status=(version 1.2. completed)",
        "inspected status=(note=')' version 1.2 completed)",
        "inspected status=not v1.2 completed",
        "inspected status = not completed",
        "inspected status=was completed",
        "inspected status=is completed",
        "inspected status=has already completed",
        "inspected status=had been completed",
        "inspected status=got completed",
        "inspected status=never completed",
        "inspected status=not-yet completed",
        "inspected status=never-successfully completed",
        "inspected status=will be completed",
        "inspected status=to be completed",
        "inspected status=unlikely to have completed",
        "inspected status=only after validation completed",
        "inspected status=(not completed)",
        "inspected status=not only completed",
        "inspected uncompleted",
        "inspected completedReason",
        "inspected unfinished",
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let metadata = parse(&format!("Operation \"work\" {body}"), 1, &config);
        let other = parse(r#"Operation "other" completed"#, 2, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, metadata, other],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty(), "{body}");
    }
    for body in [
        "status=started completed",
        "status = started completed",
        "status:'started' completed",
        "note='with settings' completed",
        "note='with settings {' completed",
        "note='with settings' completed with settings {attempt:1}",
        "status=settings completed",
        "completed: note='started?'",
        "completed: 'did it start?'",
        "note='not completed' completed",
        "status='was completed' completed",
        "note='probably completed' completed",
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let end = parse(&format!("Operation \"work\" {body}"), 1, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1, "{body}");
        assert_eq!(result.operations[0].duration_ms, 1000, "{body}");
    }
}

#[test]
fn embedded_start_words_cannot_replace_a_real_command_start() {
    let config = config::load_builtin_template("service-api").unwrap();
    let start = parse(r#"Operation "work" started"#, 0, &config);
    let metadata = parse(r#"Operation "work" restarted"#, 1, &config);
    let end = parse(r#"Operation "work" completed"#, 2, &config);
    let result = perf_analyzer::analyze_performance_with_config(
        &[start, metadata, end],
        &LogFilter::new(),
        None,
        &config,
    );
    assert_eq!(result.operations.len(), 1);
    assert_eq!(result.operations[0].duration_ms, 2000);
}

#[test]
fn negated_markers_cannot_create_command_boundaries() {
    let config = config::load_builtin_template("service-api").unwrap();
    for wording in [
        "not completed",
        "not-completed",
        "not-yet-completed",
        "not-successfully-completed",
        "not—yet—completed",
        "not/yet/completed",
        "not yet completed",
        "hasn't completed",
        "hasn’t completed",
        "never completed",
        "not successfully completed",
        "cannot be completed",
        "no longer completed",
        "will be completed",
        "can be completed",
        "shall be completed",
        "is yet to be completed",
        "awaiting completed",
        "is scheduled to be completed",
        "is being completed",
        "needs to be completed",
        "remains to be completed",
        "is almost completed",
        "is partially completed",
        "is unlikely to have completed",
        "is considered completed if validation passes",
        "is considered completed only when validation passes",
        "completed once validation passes",
        "completed only after validation passes",
        "completed as soon as validation passes",
        "completed subject to validation",
        "completed on condition that validation passes",
        "completed unless validation fails",
        "completed, if validation passes",
        "completed; if validation passes",
        "completed, probably",
        "completed probably",
        "completed or not",
        "completed or pending",
        "completed versus pending",
        "completed allegedly",
        "is likely to have completed",
        "probably completed",
        "was not, however, completed",
        "was not, in fact, completed",
        "was not,however,completed",
        "is anything but completed",
        "is far from completed",
        "is nowhere near completed",
        "status=far from completed",
        "is all but completed",
        "status=anything but completed",
        "probably v1.2 completed",
        "not v1.2 completed",
        "unlikely host.example completed",
        "perhaps completed",
        "possibly completed",
        "appears completed",
        "was assumed completed",
        "I think it completed",
        "is estimated to have completed",
        "I doubt it completed",
        "was unable to have completed",
        "is impossible to have completed",
        "would have completed",
        "if completed",
        "completed?",
        "completed v1.2?",
        "completed approx. yesterday?",
        "completed e.g. yesterday?",
        "completed approx. yesterday if validation passes",
        "probably approx. yesterday completed",
        "completed 1.5 seconds ago?",
        "completed host.example?",
        "completed v1.2 if validation passes",
        "completed: cache flushed?",
        "completed:false?",
        "has completed successfully?",
        "has completed successfully or failed?",
        "has completed, successfully?",
        "has completed; successfully?",
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let negated = parse(&format!("Operation \"work\" {wording}"), 1, &config);
        let other = parse(r#"Operation "other" completed"#, 2, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, negated, other],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty(), "{wording}");
    }
    for wording in [
        "not started",
        "not-started",
        "not-yet-started",
        "hasn't started",
        "never started",
        "did not begin",
        "status = not started",
        "status=was started",
        "is yet to be started",
        "is scheduled to be started",
        "is unlikely to have started",
        "probably started",
        "was not, however, started",
        "is anything but started",
        "is far from started",
        "is nowhere near started",
        "is all but started",
        "status=anything but started",
        "probably v1.2 started",
        "not v1.2 started",
        "can be started",
        "shall be started",
        "started if validation passes",
        "started or not",
        "started v1.2?",
        "started approx. yesterday?",
    ] {
        let negated = parse(&format!("Operation \"work\" {wording}"), 0, &config);
        let end = parse(r#"Operation "work" completed"#, 1, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[negated, end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty(), "{wording}");
    }
    for wording in [
        "not failed, completed",
        "probably failed v1.2. completed",
        "not failed but completed",
        "is nothing but completed",
        "anything but failed however completed",
        "not only completed",
        "with no errors completed",
        "completed. Is everything okay?",
        "completed v1.2. Is everything okay?",
        "status=not completed, completed",
        "status=(version 1.2 (pending)) completed",
        "status=(note=')' version 1.2 pending) completed",
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let end = parse(&format!("Operation \"work\" {wording}"), 1, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1, "{wording}");
        assert_eq!(result.operations[0].duration_ms, 1000, "{wording}");
    }
}

#[test]
fn explanatory_colons_preserve_boundary_phrases_without_matching_the_explanation() {
    for profile in ["service-api", "event-pipeline", "eyes"] {
        let config = config::load_builtin_template(profile).unwrap();
        let prefix = &config.parser.command_prefix;
        let start = parse(
            &format!(
                "{prefix}work\" {}: details",
                config.perf.command_start_markers[0]
            ),
            0,
            &config,
        );
        for marker in &config.perf.command_completion_markers {
            let end = parse(
                &format!("{prefix}work\" {marker}: child started with no errors"),
                1,
                &config,
            );
            let result = perf_analyzer::analyze_performance_with_config(
                &[start.clone(), end],
                &LogFilter::new(),
                None,
                &config,
            );
            assert_eq!(result.operations.len(), 1, "{profile}/{marker}");
            assert_eq!(result.operations[0].duration_ms, 1000);
        }
    }
    let config = config::load_builtin_template("service-api").unwrap();
    for metadata in [
        "completed:false",
        "completed: true",
        "completed: null",
        "completed: 0",
        "completed: TRUE",
        "completed:false?",
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let flag = parse(
            &format!("Operation \"work\" inspected {metadata}"),
            1,
            &config,
        );
        let other = parse(r#"Operation "other" completed"#, 2, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, flag, other],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty(), "{metadata}");
    }
}

#[test]
fn assignment_values_cannot_supply_command_subjects() {
    let config = config::load_builtin_template("service-api").unwrap();
    for context in ["note=", "note = ", "note:", "note: ", "note=inner="] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let note = parse(
            &format!("{context}Operation \"work\" completed"),
            1,
            &config,
        );
        assert!(
            !matches!(note.kind, LogEntryKind::Command { .. }),
            "{context}"
        );
        let other = parse(r#"Operation "other" completed"#, 2, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, note, other],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty(), "{context}");
    }
    let start = parse(r#"note=old Operation "work" started"#, 0, &config);
    let end = parse(r#"note=old Operation "work" completed"#, 1, &config);
    let result = perf_analyzer::analyze_performance_with_config(
        &[start, end],
        &LogFilter::new(),
        None,
        &config,
    );
    assert_eq!(result.operations.len(), 1);
    assert_eq!(result.operations[0].duration_ms, 1000);
}

#[test]
fn markers_inside_known_payload_spans_cannot_hide_real_commands() {
    let config = config::load_builtin_template("service-api").unwrap();
    for context in [
        "context {payload:1} [tag",
        "context {settings:1} [tag",
        "context {payload:1} {tag",
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let end = parse(
            &format!("{context} Operation \"work\" completed"),
            1,
            &config,
        );
        assert!(
            matches!(end.kind, LogEntryKind::Command { .. }),
            "{context}"
        );
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1, "{context}");
        assert_eq!(result.operations[0].duration_ms, 1000);
    }
}

#[test]
fn assignment_subjects_do_not_hide_a_genuine_subject_or_complete_the_assigned_name() {
    let config = config::load_builtin_template("service-api").unwrap();
    for assigned_name in ["old", "{", "completed"] {
        let old_start = parse(
            &format!("Operation \"{assigned_name}\" started"),
            0,
            &config,
        );
        let start = parse(r#"Operation "work" started"#, 1, &config);
        let end = parse(
            &format!("note=Operation \"{assigned_name}\" Operation \"work\" completed"),
            2,
            &config,
        );
        assert!(matches!(&end.kind, LogEntryKind::Command { command, .. } if command == "work"));
        let result = perf_analyzer::analyze_performance_with_config(
            &[old_start, start, end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1);
        assert_eq!(result.operations[0].name, "work");
        assert_eq!(result.operations[0].duration_ms, 1000);
        assert!(
            result
                .orphans
                .iter()
                .any(|orphan| orphan.name == assigned_name)
        );
    }
}

#[test]
fn command_prefixes_require_identifier_boundaries() {
    let config = config::load_builtin_template("service-api").unwrap();
    for fake in [
        "FakeOperation",
        "_Operation",
        "0Operation",
        "фOperation",
        "Fake\u{0301}Operation",
        "Fake\u{200d}Operation",
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let contextual = parse(&format!("{fake} \"work\" completed"), 1, &config);
        assert!(
            !matches!(contextual.kind, LogEntryKind::Command { .. }),
            "{fake}"
        );
        let other = parse(r#"Operation "other" completed"#, 2, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, contextual, other],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty(), "{fake}");
    }
    let contextual = parse(
        r#"FakeOperation "note Operation "work" completed"#,
        1,
        &config,
    );
    assert!(!matches!(contextual.kind, LogEntryKind::Command { .. }));
    for context in ["context; ", "context -> ", "context|"] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let end = parse(
            &format!("{context}Operation \"work\" completed"),
            1,
            &config,
        );
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1, "{context}");
        assert_eq!(result.operations[0].duration_ms, 1000);
    }
}

#[test]
fn adjacent_name_text_requires_a_configured_boundary_marker() {
    let config = config::load_builtin_template("service-api").unwrap();
    for suffix in [
        "x completed",
        "_junk completed",
        "é completed",
        "\u{0301} completed",
        "completedX",
        "completed\"",
    ] {
        let start = parse(r#"Operation "work" started"#, 0, &config);
        let malformed = parse(&format!("Operation \"work\"{suffix}"), 1, &config);
        assert!(
            !matches!(malformed.kind, LogEntryKind::Command { .. }),
            "{suffix}"
        );
        let other = parse(r#"Operation "other" completed"#, 2, &config);
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, malformed, other],
            &LogFilter::new(),
            None,
            &config,
        );
        assert!(result.operations.is_empty(), "{suffix}");
    }
    for profile in ["service-api", "event-pipeline", "eyes", "custom-start"] {
        let config = config::load_builtin_template(profile).unwrap();
        let prefix = &config.parser.command_prefix;
        let start = parse(
            &format!("{prefix}work\"{}", config.perf.command_start_markers[0]),
            0,
            &config,
        );
        for marker in &config.perf.command_completion_markers {
            let end = parse(&format!("{prefix}work\"{marker}"), 1, &config);
            let result = perf_analyzer::analyze_performance_with_config(
                &[start.clone(), end],
                &LogFilter::new(),
                None,
                &config,
            );
            assert_eq!(result.operations.len(), 1, "{profile}/{marker}");
            assert_eq!(result.operations[0].duration_ms, 1000);
        }
    }
}

#[test]
fn request_subjects_keep_command_shaped_context_from_completing_operations() {
    let config = config::load_builtin_template("service-api").unwrap();
    for tail in [
        "completed after Operation \"work\" completed",
        "completed after Operation \"work\" started",
    ] {
        let operation = parse(r#"Operation "work" started"#, 0, &config);
        let request_start = parse(r#"Request "fetch" [0--id] sent"#, 1, &config);
        let request_end = parse(&format!("Request \"fetch\" [0--id] {tail}"), 2, &config);
        assert!(
            matches!(&request_end.kind, LogEntryKind::Request {request, ..} if request == "fetch")
        );
        let result = perf_analyzer::analyze_performance_with_config(
            &[operation, request_start, request_end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1, "{tail}");
        assert_eq!(result.operations[0].name, "fetch");
        assert_eq!(result.operations[0].duration_ms, 1000);
    }
    for context in [
        r#"note='Request "fetch" completed' "#,
        r#"context={note:'Request "fetch" completed'} "#,
        r#"previous=Request "fetch" "#,
        r#"FakeRequest "fetch" "#,
    ] {
        let start = parse(&format!("{context}Operation \"work\" started"), 0, &config);
        let end = parse(
            &format!("{context}Operation \"work\" completed"),
            1,
            &config,
        );
        let result = perf_analyzer::analyze_performance_with_config(
            &[start, end],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1, "{context}");
        assert_eq!(result.operations[0].name, "work");
    }
}

#[test]
fn pre_subject_qualifiers_cannot_create_command_boundaries() {
    let config = config::load_builtin_template("service-api").unwrap();
    for prefix in [
        "Did not observe ",
        "Probably ",
        "Could not confirm ",
        "Never observed ",
        "Has ",
        "Have ",
        "Had ",
        "Is ",
        "Are ",
        "Was ",
        "Were ",
        "Did ",
        "Does ",
        "Do ",
        "Am ",
        "Checking: Has ",
    ] {
        for (first, second) in [
            (
                r#"Operation "work" started"#.to_string(),
                format!("{prefix}Operation \"work\" completed"),
            ),
            (
                format!("{prefix}Operation \"work\" started"),
                r#"Operation "work" completed"#.to_string(),
            ),
        ] {
            let result = perf_analyzer::analyze_performance_with_config(
                &[parse(&first, 0, &config), parse(&second, 1, &config)],
                &LogFilter::new(),
                None,
                &config,
            );
            assert!(result.operations.is_empty(), "{first}; {second}");
        }
    }
    for prefix in [
        r#"note='not observed' "#,
        r#"context={note:'not observed'} "#,
        "status=not observed, ",
    ] {
        let result = perf_analyzer::analyze_performance_with_config(
            &[
                parse(&format!("{prefix}Operation \"work\" started"), 0, &config),
                parse(&format!("{prefix}Operation \"work\" completed"), 1, &config),
            ],
            &LogFilter::new(),
            None,
            &config,
        );
        assert_eq!(result.operations.len(), 1, "{prefix}");
    }
}
