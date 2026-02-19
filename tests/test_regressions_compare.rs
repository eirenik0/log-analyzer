use chrono::{DateTime, Local};
use log_analyzer::SortOrder;
use log_analyzer::comparator::{ComparisonOptions, LogFilter, compare_logs};
use log_analyzer::parser::{LogEntry, LogEntryKind, RequestDirection};
use serde_json::json;

fn parse_local(ts: &str) -> DateTime<Local> {
    DateTime::parse_from_rfc3339(ts)
        .expect("valid RFC3339 timestamp")
        .with_timezone(&Local)
}

fn request_log(
    component: &str,
    timestamp: &str,
    line: usize,
    request_id: &str,
    payload: serde_json::Value,
) -> LogEntry {
    LogEntry {
        component: component.to_string(),
        component_id: String::new(),
        timestamp: parse_local(timestamp),
        level: "INFO".to_string(),
        message: format!("Request \"foo\" [{request_id}] will be sent with body [JSON removed]"),
        raw_logline: String::new(),
        kind: LogEntryKind::Request {
            request: "foo".to_string(),
            request_id: Some(request_id.to_string()),
            endpoint: None,
            direction: RequestDirection::Send,
            payload: Some(payload),
        },
        source_line_number: line,
    }
}

#[test]
fn test_compare_pairs_repeated_entries_one_to_one_not_cross_product() {
    let logs1 = vec![
        request_log(
            "svc",
            "2026-01-01T00:00:00.000Z",
            1,
            "0--id-a1",
            json!({"x": 1}),
        ),
        request_log(
            "svc",
            "2026-01-01T00:00:01.000Z",
            2,
            "0--id-a2",
            json!({"x": 2}),
        ),
    ];
    let logs2 = vec![
        request_log(
            "svc",
            "2026-01-01T00:00:02.000Z",
            1,
            "0--id-b1",
            json!({"x": 3}),
        ),
        request_log(
            "svc",
            "2026-01-01T00:00:03.000Z",
            2,
            "0--id-b2",
            json!({"x": 4}),
        ),
    ];

    let results = compare_logs(
        &logs1,
        &logs2,
        &LogFilter::new(),
        &ComparisonOptions::new()
            .diff_only(true)
            .sort_by(SortOrder::Time),
    )
    .expect("comparison should succeed");

    assert_eq!(
        results.shared_comparisons.len(),
        2,
        "same-key repeated entries should be paired one-to-one"
    );
}

#[test]
fn test_compare_sort_by_time_uses_timestamps_not_key_text() {
    // Earliest log is component "z", later log is component "a".
    // Sorting by time should keep z first even though a < z alphabetically.
    let logs1 = vec![
        request_log("z", "2026-01-01T00:00:00.000Z", 1, "0--z", json!({"x": 1})),
        request_log("a", "2026-01-01T00:00:10.000Z", 2, "0--a", json!({"x": 1})),
    ];
    let logs2 = vec![
        request_log("z", "2026-01-01T00:00:00.100Z", 1, "0--z", json!({"x": 2})),
        request_log("a", "2026-01-01T00:00:10.100Z", 2, "0--a", json!({"x": 2})),
    ];

    let results = compare_logs(
        &logs1,
        &logs2,
        &LogFilter::new(),
        &ComparisonOptions::new()
            .diff_only(true)
            .sort_by(SortOrder::Time),
    )
    .expect("comparison should succeed");

    let first_key = &results.shared_comparisons[0].key;
    assert!(
        first_key.starts_with("z|"),
        "expected earliest timestamp key to come first when sorting by time, got: {first_key}"
    );
}

#[test]
fn test_compare_reports_unpaired_entries_as_unique() {
    let logs1 = vec![
        request_log(
            "svc",
            "2026-01-01T00:00:00.000Z",
            1,
            "0--id-a1",
            json!({"x": 1}),
        ),
        request_log(
            "svc",
            "2026-01-01T00:00:01.000Z",
            2,
            "0--id-a2",
            json!({"x": 2}),
        ),
        request_log(
            "svc",
            "2026-01-01T00:00:02.000Z",
            3,
            "0--id-a3",
            json!({"x": 3}),
        ),
    ];
    let logs2 = vec![
        request_log(
            "svc",
            "2026-01-01T00:00:03.000Z",
            1,
            "0--id-b1",
            json!({"x": 10}),
        ),
        request_log(
            "svc",
            "2026-01-01T00:00:04.000Z",
            2,
            "0--id-b2",
            json!({"x": 20}),
        ),
    ];

    let results = compare_logs(
        &logs1,
        &logs2,
        &LogFilter::new(),
        &ComparisonOptions::new()
            .diff_only(true)
            .sort_by(SortOrder::Time),
    )
    .expect("comparison should succeed");

    assert_eq!(
        results.shared_comparisons.len(),
        2,
        "only the paired entries should be compared"
    );
    assert_eq!(
        results.unique_to_log1.len(),
        1,
        "unpaired entry from log1 should be surfaced as unique"
    );
    assert_eq!(results.unique_to_log2.len(), 0);
}
