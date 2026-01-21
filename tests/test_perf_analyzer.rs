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
