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
