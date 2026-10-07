use log_analyzer::{
    config::AnalyzerConfig,
    parser::parse_log_entry_with_config,
    timeline::{self, EventRule, PairRule, TimelineRules, Timing},
};

fn rules() -> TimelineRules {
    TimelineRules {
        events: ["begin", "response", "retry"]
            .iter()
            .map(|name| EventRule {
                name: name.to_string(),
                pattern: format!(r"{name} id=(?P<id>\w+)"),
                correlation_fields: vec!["component_id".into(), "id".into()],
            })
            .collect(),
        pairs: vec![
            PairRule {
                name: "fetch".into(),
                start_event: "begin".into(),
                end_event: "response".into(),
                timing: Timing::Measured,
            },
            PairRule {
                name: "backoff".into(),
                start_event: "response".into(),
                end_event: "retry".into(),
                timing: Timing::InferredSleep,
            },
        ],
    }
}

fn entries(lines: &[&str]) -> Vec<log_analyzer::LogEntry> {
    lines
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            let line = format!("core (demo) | 2026-01-01T00:00:0{i}.000Z [INFO] {msg}");
            parse_log_entry_with_config(&line, i + 1, &AnalyzerConfig::default()).unwrap()
        })
        .collect()
}

#[test]
fn retry_boundaries_separate_response_and_inferred_sleep_with_parallel_work() {
    let logs = entries(&[
        "begin id=a",
        "begin id=b",
        "response id=a",
        "response id=b",
        "retry id=a",
        "retry id=b",
    ]);
    let report = timeline::analyze(&logs.iter().collect::<Vec<_>>(), &rules())
        .unwrap()
        .unwrap();
    assert_eq!(report.intervals.len(), 4);
    assert_eq!(report.measured_work_sum_ms, Some(4000));
    assert_eq!(report.elapsed_capture_ms, Some(5000));
    assert_eq!(report.sample_counts["begin"], 2);
    assert_eq!(report.sample_counts["response"], 2);
    assert!(report.incomplete.is_empty());
    assert_eq!(report.upstream_capture_completeness, "unknown");
    for interval in &report.intervals {
        assert_eq!(interval.observed_gap_ms, 2000);
        assert_eq!(
            interval.measured_duration_ms,
            (interval.timing == Timing::Measured).then_some(2000)
        );
    }
    assert_eq!(report.events[1].gap_since_previous_match_ms, Some(1000));
    assert_eq!(report.intervals[0].start.source.line, 1);
    assert!(timeline::format_text(&report).contains("2026-01-01T00:00:00"));
}

#[test]
fn missing_ends_overlap_and_unknown_time_never_invent_duration() {
    let logs = entries(&[
        "begin id=a",
        "begin id=a",
        "response id=a",
        "begin id=b",
        "response id=c",
    ]);
    let report = timeline::analyze(&logs.iter().collect::<Vec<_>>(), &rules())
        .unwrap()
        .unwrap();
    assert_eq!(report.status, "insufficient_evidence");
    assert!(report.intervals.is_empty());
    assert_eq!(report.ambiguous_groups.len(), 1);
    assert!(
        report
            .incomplete
            .iter()
            .any(|i| i.reason.contains("missing_end"))
    );
    assert!(
        report
            .incomplete
            .iter()
            .any(|i| i.reason.contains("missing_start"))
    );
    assert_eq!(report.measured_work_sum_ms, None);
    let mut unknown = rules();
    unknown.pairs.truncate(1);
    unknown.pairs[0].timing = Timing::Unknown;
    let logs = entries(&["begin id=a", "response id=a"]);
    let report = timeline::analyze(&logs.iter().collect::<Vec<_>>(), &unknown)
        .unwrap()
        .unwrap();
    assert_eq!(report.intervals[0].measured_duration_ms, None);
    assert_eq!(report.measured_work_sum_ms, None);
}

#[test]
fn invalid_rules_and_missing_keys_are_explicit() {
    let logs = entries(&["worker alive"]);
    assert_eq!(
        timeline::analyze(&logs.iter().collect::<Vec<_>>(), &rules())
            .unwrap()
            .unwrap()
            .status,
        "no_applicable_events"
    );
    let mut invalid = rules();
    invalid.events[0].pattern = "[".into();
    assert!(timeline::analyze(&[], &invalid).is_err());
    let mut missing = rules();
    missing
        .events
        .iter_mut()
        .for_each(|r| r.correlation_fields = vec!["tenant".into()]);
    let logs = entries(&["begin id=a", "response id=a"]);
    let report = timeline::analyze(&logs.iter().collect::<Vec<_>>(), &missing)
        .unwrap()
        .unwrap();
    assert!(report.intervals.is_empty());
    assert!(
        report
            .incomplete
            .iter()
            .all(|i| i.reason == "missing_correlation_field")
    );
}

#[test]
fn tied_cross_file_boundaries_are_ambiguous_in_both_input_orders() {
    let mut logs = entries(&["begin id=a", "response id=a"]);
    logs[0].source_file = Some("start.log".into());
    logs[1].source_file = Some("end.log".into());
    logs[1].timestamp = logs[0].timestamp;
    logs[1].source_timestamp = logs[0].source_timestamp;
    let mut configured = rules();
    configured.pairs.truncate(1);
    for _ in 0..2 {
        let report = timeline::analyze(&logs.iter().collect::<Vec<_>>(), &configured)
            .unwrap()
            .unwrap();
        assert!(report.intervals.is_empty());
        assert_eq!(report.ambiguous_groups.len(), 1);
        assert_eq!(report.incomplete.len(), 2);
        assert_eq!(report.measured_work_sum_ms, None);
        assert!(
            report
                .incomplete
                .iter()
                .all(|item| item.reason == "ambiguous_boundary")
        );
        logs.reverse();
    }
}

#[test]
fn timeline_evidence_preserves_source_offsets_in_text_and_json() {
    let mut configured = rules();
    configured.pairs.truncate(1);
    for json in [false, true] {
        let inputs = [
            ("2026-01-01T05:30:00+05:30", "begin id=a"),
            ("2025-12-31T19:00:01-05:00", "response id=a"),
        ];
        let logs = inputs
            .iter()
            .enumerate()
            .map(|(i, (timestamp, message))| {
                let raw = if json {
                    serde_json::json!({"timestamp":timestamp,"session_id":"demo","message":message})
                        .to_string()
                } else {
                    format!("core (demo) | {timestamp} [INFO] {message}")
                };
                parse_log_entry_with_config(&raw, i + 1, &AnalyzerConfig::default()).unwrap()
            })
            .collect::<Vec<_>>();
        let report = timeline::analyze(&logs.iter().collect::<Vec<_>>(), &configured)
            .unwrap()
            .unwrap();
        assert_eq!(report.intervals[0].measured_duration_ms, Some(1000));
        assert_eq!(report.events[0].timestamp.offset().local_minus_utc(), 19800);
        assert_eq!(
            report.events[1].timestamp.offset().local_minus_utc(),
            -18000
        );
        assert_eq!(report.events[0].timestamp_offset_source, "source");
        let text = timeline::format_text(&report);
        let json = serde_json::to_string(&report).unwrap();
        for offset in ["+05:30", "-05:00"] {
            assert!(text.contains(offset));
            assert!(json.contains(offset));
        }
    }
}
