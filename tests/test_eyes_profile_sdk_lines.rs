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
