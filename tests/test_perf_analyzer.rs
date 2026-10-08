use log_analyzer::perf_analyzer;

#[test]
fn test_extract_request_id() {
    // Valid request ID patterns
    assert_eq!(
        perf_analyzer::extract_request_id(r#"Request "check" [0--abc123-def] will be sent"#),
        Some("0--abc123-def".to_string())
    );
    assert_eq!(
        perf_analyzer::extract_request_id(r#"Request "openEyes" [1--uuid-here#2] that was sent"#),
        Some("1--uuid-here#2".to_string())
    );

    // Invalid patterns - no request ID after name
    assert_eq!(perf_analyzer::extract_request_id("No brackets here"), None);
    assert_eq!(
        perf_analyzer::extract_request_id(r#"Request "check" called for target {"#),
        None
    );
    // Brackets in wrong place (JSON content)
    assert_eq!(
        perf_analyzer::extract_request_id(r#"Request "check" called for renders [1,2,3]"#),
        None
    );
}

#[test]
fn test_extract_event_key() {
    let json = serde_json::json!({
        "key": "test-event-key",
        "data": "some data"
    });
    assert_eq!(
        perf_analyzer::extract_event_key(&json),
        Some("test-event-key".to_string())
    );

    let json_no_key = serde_json::json!({
        "data": "some data"
    });
    assert_eq!(perf_analyzer::extract_event_key(&json_no_key), None);
}

#[test]
fn test_truncate_string() {
    assert_eq!(perf_analyzer::truncate_string("short", 10), "short");
    assert_eq!(
        perf_analyzer::truncate_string("this is a very long string", 10),
        "this is..."
    );
    assert_eq!(
        perf_analyzer::truncate_string("exactly10c", 10),
        "exactly10c"
    );
}

#[test]
fn sequential_reuse_and_different_names_do_not_create_cross_pairs() {
    let config = log_analyzer::config::load_builtin_template("service-api").unwrap();
    let lines = [
        "core (demo) | 2026-01-01T00:00:00.000Z [INFO] Request \"a\" [0--same] sent",
        "core (demo) | 2026-01-01T00:00:01.000Z [INFO] Request \"b\" [0--same] sent",
        "core (demo) | 2026-01-01T00:00:02.000Z [INFO] Request \"a\" [0--same] completed",
        "core (demo) | 2026-01-01T00:00:03.000Z [INFO] Request \"a\" [0--same] sent",
        "core (demo) | 2026-01-01T00:00:04.000Z [INFO] Request \"a\" [0--same] completed",
        "core (demo) | 2026-01-01T00:00:05.000Z [INFO] Request \"b\" [0--same] completed",
        "core (demo) | 2026-01-01T00:00:06.000Z [INFO] Request \"missing\" completed",
    ];
    let mut logs = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            log_analyzer::parser::parse_log_entry_with_config(line, i + 1, &config).unwrap()
        })
        .collect::<Vec<_>>();
    logs.reverse();
    let results = perf_analyzer::analyze_performance_with_config(
        &logs,
        &log_analyzer::comparator::LogFilter::new(),
        None,
        &config,
    );
    assert_eq!(results.operations.len(), 3);
    assert!(results.ambiguous_groups.is_empty());
    let mut durations = results
        .operations
        .iter()
        .map(|op| op.duration_ms)
        .collect::<Vec<_>>();
    durations.sort();
    assert_eq!(durations, [1000, 2000, 4000]);
    assert_eq!(results.unmatched_events.len(), 1);
    assert_eq!(
        results.unmatched_events[0].reason,
        "missing_correlation_key"
    );
    assert!(results.operations.iter().all(|op| op.duration_ms >= 0));
}

#[test]
fn events_and_commands_preserve_overlapping_starts_instead_of_overwriting() {
    use log_analyzer::parser::{EventDirection, LogEntryKind};
    let mut config = log_analyzer::config::load_builtin_template("service-api").unwrap();
    config.perf.event_correlation_keys = vec!["id".to_string()];
    for command in [false, true] {
        let mut logs = Vec::new();
        for (i, start) in [true, true, false, false].iter().enumerate() {
            let line = format!(
                "core (demo) | 2026-01-01T00:00:0{i}.000Z [INFO] {}",
                if command {
                    if *start {
                        r#"Operation "work" started"#
                    } else {
                        r#"Operation "work" completed"#
                    }
                } else {
                    if *start { "started" } else { "completed" }
                }
            );
            let mut entry =
                log_analyzer::parser::parse_log_entry_with_config(&line, i + 1, &config).unwrap();
            entry.kind = if command {
                LogEntryKind::Command {
                    command: "work".to_string(),
                    settings: None,
                }
            } else {
                LogEntryKind::Event {
                    event_type: "work".to_string(),
                    direction: if *start {
                        EventDirection::Receive
                    } else {
                        EventDirection::Emit
                    },
                    payload: Some(serde_json::json!({"id": "same"})),
                }
            };
            if !command {
                let mut legacy = log_analyzer::config::AnalyzerConfig::default();
                legacy.perf.event_correlation_keys = vec!["id".into()];
                log_analyzer::parser::attach_legacy_event_evidence(&mut entry, &legacy);
            }
            logs.push(entry);
        }
        let results = perf_analyzer::analyze_performance_with_config(
            &logs,
            &log_analyzer::comparator::LogFilter::new(),
            None,
            &config,
        );
        assert!(results.operations.is_empty());
        assert_eq!(results.ambiguous_groups.len(), 1);
        assert_eq!(results.unmatched_events.len(), 4);
        assert_eq!(results.orphans.len(), 2);
    }
}
