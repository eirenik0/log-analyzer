use log_analyzer::{
    config::AnalyzerConfig,
    normalize::{NormalizationRules, TimestampUnit},
    parser::parse_log_file_report,
};
use std::collections::BTreeMap;

fn rules() -> NormalizationRules {
    NormalizationRules {
        fields: BTreeMap::from([
            ("timestamp".into(), "/0".into()),
            ("message".into(), "/1/info/event".into()),
            ("payload".into(), "/1/info".into()),
        ]),
        ..Default::default()
    }
}

#[test]
fn tuple_rows_keep_source_and_diagnose_rejections() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tuple.jsonl");
    std::fs::write(&file, concat!(
        "[\"2026-01-01T00:00:01Z\",{\"info\":{\"event\":\"heartbeat\",\"processId\":\"demo\"}}]\n",
        "[null,{\"info\":{\"event\":\"heartbeat\"}}]\n",
        "[123,{\"info\":{\"event\":\"heartbeat\"}}]\n",
        "[]\n"
    )).unwrap();
    let cfg = AnalyzerConfig {
        normalization: Some(rules()),
        ..Default::default()
    };
    let report = parse_log_file_report(&file, &cfg).unwrap();
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.coverage.rejected_candidates, 3);
    assert_eq!(report.entries[0].source_line_number, 1);
    assert_eq!(report.entries[0].source_row_path.as_deref(), Some(""));
    assert_eq!(report.entries[0].payload().unwrap()["processId"], "demo");
    let reasons = report
        .coverage
        .normalization_diagnostics
        .iter()
        .map(|d| d.reason.as_str())
        .collect::<Vec<_>>();
    assert!(reasons.contains(&"null_field"));
    assert!(reasons.contains(&"wrong_field_type"));
    assert!(reasons.contains(&"missing_field"));
}

#[test]
fn nested_expansion_decodes_only_explicit_strings_and_handles_epoch_units() {
    let mut configured = NormalizationRules {
        root_path: "/rows".into(),
        expand_rows: true,
        row_decode_paths: vec!["/detail".into()],
        fields: BTreeMap::from([
            ("timestamp".into(), "/time".into()),
            ("payload".into(), "/detail".into()),
        ]),
        timestamp_unit: Some(TimestampUnit::Milliseconds),
        ..Default::default()
    };
    let raw=serde_json::json!({"rows":[{"time":-1,"detail":"{\"event\":\"pulse\"}"},{"time":1000,"detail":"not JSON"}]}).to_string();
    let rows = log_analyzer::normalize::normalize(&raw, 7, &configured);
    assert_eq!(rows[0].0, "/rows/0");
    assert_eq!(
        rows[0].1.as_ref().unwrap()["timestamp"],
        "1969-12-31T23:59:59.999000000Z"
    );
    assert_eq!(rows[0].1.as_ref().unwrap()["payload"]["event"], "pulse");
    assert_eq!(
        rows[1].1.as_ref().unwrap_err().reason,
        "invalid_json_string"
    );
    configured.row_decode_paths.clear();
    let rows = log_analyzer::normalize::normalize(&raw, 7, &configured);
    assert!(rows[0].1.as_ref().unwrap()["payload"].is_string());
    for (unit, n) in [
        (TimestampUnit::Seconds, 1),
        (TimestampUnit::Milliseconds, 1000),
        (TimestampUnit::Microseconds, 1_000_000),
        (TimestampUnit::Nanoseconds, 1_000_000_000),
    ] {
        configured.timestamp_unit = Some(unit);
        let rows = log_analyzer::normalize::normalize(
            &serde_json::json!({"rows":[{"time":n,"detail":{}}]}).to_string(),
            1,
            &configured,
        );
        assert_eq!(
            rows[0].1.as_ref().unwrap()["timestamp"],
            "1970-01-01T00:00:01.000000000Z"
        );
    }
}
