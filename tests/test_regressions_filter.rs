use chrono::{DateTime, Local};
use log_analyzer::filter::{FilterExpression, to_log_filter};
use log_analyzer::parser::{LogEntry, LogEntryKind, RequestDirection};

fn parse_local(ts: &str) -> DateTime<Local> {
    DateTime::parse_from_rfc3339(ts)
        .expect("valid RFC3339 timestamp")
        .with_timezone(&Local)
}

fn generic_log(component: &str) -> LogEntry {
    LogEntry {
        component: component.to_string(),
        component_id: String::new(),
        timestamp: parse_local("2026-01-01T00:00:00.000Z"),
        level: "INFO".to_string(),
        message: "hello".to_string(),
        raw_logline: "hello".to_string(),
        kind: LogEntryKind::Generic { payload: None },
        source_line_number: 1,
    }
}

fn request_log(direction: RequestDirection) -> LogEntry {
    LogEntry {
        component: "svc".to_string(),
        component_id: String::new(),
        timestamp: parse_local("2026-01-01T00:00:00.000Z"),
        level: "INFO".to_string(),
        message: "request".to_string(),
        raw_logline: "request".to_string(),
        kind: LogEntryKind::Request {
            request: "foo".to_string(),
            request_id: Some("0--id".to_string()),
            endpoint: None,
            direction,
            payload: Some(serde_json::json!({"x": 1})),
        },
        source_line_number: 1,
    }
}

#[test]
fn test_filter_expression_rejects_malformed_term() {
    let result = FilterExpression::parse("not-a-filter");
    assert!(
        result.is_err(),
        "malformed filter terms should return an error, not be silently ignored"
    );
}

#[test]
fn test_multiple_component_filters_are_combined_with_or_logic() {
    let expr = FilterExpression::parse("c:core c:socket").expect("valid expression");
    let filter = to_log_filter(&expr);

    assert!(
        filter.matches(&generic_log("core")),
        "same-type include filters should act as OR"
    );
    assert!(
        filter.matches(&generic_log("socket")),
        "same-type include filters should act as OR"
    );
    assert!(
        !filter.matches(&generic_log("other")),
        "at least one same-type include filter must match"
    );
}

#[test]
fn test_multiple_direction_filters_are_or_not_impossible_and() {
    let expr = FilterExpression::parse("d:incoming d:outgoing").expect("valid expression");
    let filter = to_log_filter(&expr);

    assert!(
        filter.matches(&request_log(RequestDirection::Send)),
        "outgoing log should match when outgoing is one of included directions"
    );
    assert!(
        filter.matches(&request_log(RequestDirection::Receive)),
        "incoming log should match when incoming is one of included directions"
    );
}
