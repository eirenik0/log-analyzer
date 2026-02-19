use chrono::Local;
use log_analyzer::comparator::LogFilter;
use log_analyzer::parser::{LogEntry, LogEntryKind};

fn create_test_log(component: &str, level: &str, message: &str) -> LogEntry {
    LogEntry {
        timestamp: Local::now(),
        component: component.to_string(),
        component_id: "test-id".to_string(),
        level: level.to_string(),
        message: message.to_string(),
        raw_logline: message.to_string(),
        kind: LogEntryKind::Generic { payload: None },
        source_line_number: 1,
    }
}

#[test]
fn test_component_filter_is_case_insensitive() {
    let log = create_test_log("core-ufg", "INFO", "test message");

    assert!(LogFilter::new().with_component(Some("core")).matches(&log));
    assert!(LogFilter::new().with_component(Some("CORE")).matches(&log));
    assert!(LogFilter::new().with_component(Some("UfG")).matches(&log));
}

#[test]
fn test_level_filter_is_case_insensitive() {
    let log = create_test_log("core", "ERROR", "test message");

    assert!(LogFilter::new().with_level(Some("error")).matches(&log));
    assert!(LogFilter::new().with_level(Some("ERROR")).matches(&log));
    assert!(LogFilter::new().with_level(Some("ErRoR")).matches(&log));
}

#[test]
fn test_message_filter_is_case_insensitive() {
    let log = create_test_log("core", "INFO", "Connection Timeout Error");

    assert!(
        LogFilter::new()
            .contains_text(Some("timeout"))
            .matches(&log)
    );
    assert!(
        LogFilter::new()
            .contains_text(Some("TIMEOUT"))
            .matches(&log)
    );
    assert!(
        LogFilter::new()
            .contains_text(Some("connection"))
            .matches(&log)
    );
}

#[test]
fn test_exclude_filters_are_case_insensitive() {
    let log = create_test_log("socket", "DEBUG", "request failed with timeout");

    assert!(
        !LogFilter::new()
            .exclude_component(Some("SOCKET"))
            .matches(&log)
    );
    assert!(
        !LogFilter::new()
            .exclude_level(Some("debug"))
            .matches(&log)
    );
    assert!(
        !LogFilter::new()
            .excludes_text(Some("TIMEOUT"))
            .matches(&log)
    );
}
