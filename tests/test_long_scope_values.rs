use log_analyzer::{
    comparator::LogFilter,
    config::{self, AnalyzerConfig},
    event_rules::{MAX_VALUE_BYTES, bounded_scope_value},
    parser::{self, LogEntry},
    perf_analyzer,
};

fn long_scope(tag: &str) -> String {
    let segment = format!("worker-{tag}/job-a/step-b");
    let mut scope = segment.clone();
    while scope.len() <= MAX_VALUE_BYTES {
        scope.push_str(" & ");
        scope.push_str(&segment);
    }
    scope
}

fn parse(cfg: &AnalyzerConfig, lines: &[(&str, &str)]) -> Vec<LogEntry> {
    lines
        .iter()
        .enumerate()
        .map(|(i, (scope, message))| {
            parser::parse_log_entry_with_config(
                &format!("worker ({scope}) | 2026-01-01T00:00:{i:02}.000Z [INFO] {message}"),
                i + 1,
                cfg,
            )
            .unwrap()
        })
        .collect()
}

fn analyze(lines: &[(&str, &str)]) -> perf_analyzer::PerfAnalysisResults {
    let cfg = config::load_builtin_template("service-api").unwrap();
    let logs = parse(&cfg, lines);
    perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &cfg)
}

#[test]
fn start_and_end_with_the_same_long_scope_pair() {
    let scope = long_scope("x");
    let result = analyze(&[
        (&scope, r#"Request "work" [r1] sent"#),
        (&scope, r#"Request "work" [r1] completed"#),
    ]);
    assert_eq!(result.operations.len(), 1, "{:?}", result.unmatched_events);
    assert_eq!(result.operations[0].duration_ms, 1000);
}

#[test]
fn different_long_scopes_do_not_pair() {
    let a = long_scope("a");
    let b = long_scope("b");
    assert_eq!(a.len(), b.len());
    let result = analyze(&[
        (&a, r#"Request "work" [r1] sent"#),
        (&b, r#"Request "work" [r1] completed"#),
    ]);
    assert!(result.operations.is_empty());
}

#[test]
fn bounded_value_keeps_short_values_and_limits_long_ones() {
    assert_eq!(bounded_scope_value("short"), "short");
    let long = "é".repeat(MAX_VALUE_BYTES);
    let bounded = bounded_scope_value(&long);
    assert!(bounded.len() <= MAX_VALUE_BYTES);
    assert!(bounded.contains(&format!("{} bytes", long.len())));
    assert_eq!(bounded, bounded_scope_value(&long));
    let mut changed = long.clone();
    changed.push('x');
    assert_ne!(bounded, bounded_scope_value(&changed));
}
