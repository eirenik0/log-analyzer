use log_analyzer::{
    comparator::LogFilter,
    config::{self, AnalyzerConfig},
    parser::{self, LogEntry},
    perf_analyzer,
};

const ADDRESS: &str = "\"[POST]https://eyes.example.test/api/sessions\"";

fn parse(cfg: &AnalyzerConfig, component_id: &str, messages: &[String]) -> Vec<LogEntry> {
    messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            parser::parse_log_entry_with_config(
                &format!(
                    "core-requests ({component_id}) | 2026-01-01T00:00:{i:02}.000Z [INFO ] {m}"
                ),
                i + 1,
                cfg,
            )
            .unwrap()
        })
        .collect()
}

fn pair_count(messages: &[String]) -> usize {
    let cfg = config::load_builtin_template("eyes").unwrap();
    let logs = parse(&cfg, "manager-a/eyes-b/request-c", messages);
    perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &cfg)
        .operations
        .len()
}

fn request_pair(start_tail: &str, end_tail: &str) -> Vec<String> {
    vec![
        format!("Request \"openEyes\" [0--id1] will be sent to the address {ADDRESS}{start_tail}"),
        format!("Request \"openEyes\" [0--id1] that was sent to the address {ADDRESS}{end_tail}"),
    ]
}

#[test]
fn request_start_accepts_an_undefined_body() {
    assert_eq!(
        pair_count(&request_pair(
            " with body undefined",
            " respond with OK(200)"
        )),
        1
    );
}

#[test]
fn request_start_accepts_object_and_array_bodies() {
    assert_eq!(
        pair_count(&request_pair(
            " with body {\"a\":1}",
            " respond with OK(200)"
        )),
        1
    );
    assert_eq!(
        pair_count(&request_pair(" with body [\"a\"]", " respond with OK(200)")),
        1
    );
}

#[test]
fn request_end_accepts_every_response_suffix() {
    for suffix in [
        " respond with OK(200)",
        " respond with OK(200), dont retry returned false",
        " respond with OK(200), dont retry returned true, httpVersion: default",
        " with body undefined is going to retried due to a network error",
        r#" with body {"ok":false} is going to retried due to a network error"#,
        " with body [false] is going to retried due to a network error",
    ] {
        assert_eq!(
            pair_count(&request_pair(" with body undefined", suffix)),
            1,
            "{suffix}"
        );
    }
}

#[test]
fn command_start_accepts_the_default_driver_form() {
    let cfg = config::load_builtin_template("eyes").unwrap();
    let messages = vec![
        "Command \"openEyes\" is called with default driver and settings {".to_string(),
        "Command \"check\" is called with settings {".to_string(),
    ];
    let logs = parse(&cfg, "manager-a/eyes-b", &messages);
    let result =
        perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &cfg);
    let mut names: Vec<_> = result.orphans.iter().map(|o| o.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["check", "openEyes"]);
}

#[test]
fn pending_request_suffixes_do_not_supply_an_end_boundary() {
    let cfg = config::load_builtin_template("eyes").unwrap();
    for suffix in [
        " is still pending",
        " is still pending(200)",
        " respond with an unknown result",
        " with body undefined is still pending",
        r#" with body {"ok":false} is still pending"#,
        r#" with body [false] is still pending"#,
        r#" with body {"ok":false} is still pending {}"#,
        r#" with body [false] is still pending []"#,
        " with body {unfinished",
        " with body [unfinished",
        " respond with OK(200), still waiting for completion",
    ] {
        let messages = request_pair(" with body undefined", suffix);
        let logs = parse(&cfg, "manager-a/eyes-b/request-c", &messages);
        let result =
            perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &cfg);
        assert!(result.operations.is_empty(), "{suffix}");
        assert_eq!(result.orphans.len(), 1, "{suffix}");
        assert_eq!(result.operation_coverage.status, "insufficient_evidence");

        let missing_id = messages[1].replace(" [0--id1]", "");
        let logs = parse(&cfg, "manager-a/eyes-b/request-c", &[missing_id]);
        let result =
            perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &cfg);
        assert!(result.unmatched_events.is_empty(), "{suffix}");
        assert_eq!(result.operation_coverage.relevant_events, 0, "{suffix}");
    }
}

#[test]
fn default_driver_settings_are_decoded_like_regular_command_settings() {
    let cfg = config::load_builtin_template("eyes").unwrap();
    let messages = vec![
        r#"Command "openEyes" is called with default driver and settings {"appName":"demo","testName":"sample"}"#.to_string(),
        r#"Command "check" is called with settings {"appName":"demo","testName":"sample"}"#.to_string(),
    ];
    let logs = parse(&cfg, "manager-a/eyes-b", &messages);
    for log in &logs {
        let payload = log.payload().expect("command settings must be decoded");
        assert_eq!(payload["appName"], "demo");
        assert_eq!(payload["testName"], "sample");
        assert!(log.message.ends_with("settings [JSON removed]"));
    }
    assert_eq!(logs[0].payload(), logs[1].payload());
}

#[test]
fn direct_response_status_preserves_duration_and_endpoint() {
    let cfg = config::load_builtin_template("eyes").unwrap();
    let mut messages = request_pair(" with body undefined", " respond with OK(200)");
    messages[1] = r#"Request "openEyes" [0--id1] respond with Internal Server Error(500), dont retry returned true, httpVersion: 1.1"#.to_string();
    let logs = parse(&cfg, "manager-a/eyes-b/request-c", &messages);
    let result =
        perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &cfg);
    assert_eq!(result.operations.len(), 1);
    assert_eq!(result.operations[0].duration_ms, 1000);
    assert_eq!(
        result.operations[0].endpoint.as_deref(),
        Some("[POST]https://eyes.example.test/api/sessions")
    );
}

#[test]
fn complete_nested_response_bodies_keep_their_boundary() {
    for body in [
        r#" with body {"nested":[{"text":"} is still pending"}]}"#,
        r#" with body [{"nested":[false,{"text":"]"}]}]"#,
        " with body {nested: [undefined]} /* complete comment */",
    ] {
        assert_eq!(
            pair_count(&request_pair(" with body undefined", body)),
            1,
            "{body}"
        );
    }
}
