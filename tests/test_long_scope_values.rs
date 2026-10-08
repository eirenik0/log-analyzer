use log_analyzer::{
    comparator::LogFilter,
    config::{self, AnalyzerConfig},
    event_rules::{CompiledEventRules, MAX_VALUE_BYTES, ValueMapping, bounded_scope_value},
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

fn with_explicit_scope(rule_id: &str, source: ValueMapping) -> AnalyzerConfig {
    let mut cfg = config::load_builtin_template("service-api").unwrap();
    let mut schema = cfg.event_rules.as_ref().unwrap().schema().clone();
    let rule = schema
        .rules
        .iter_mut()
        .find(|rule| rule.id == rule_id)
        .unwrap();
    rule.mapping.scope = vec![source];
    cfg.event_rules = Some(CompiledEventRules::compile(schema).unwrap());
    cfg
}

#[test]
fn explicit_and_inherited_marker_scopes_pair_in_both_directions() {
    let scope = "tenant bytes, fnv1a128:raw";
    for rule_id in ["request-start-id", "request-end-id"] {
        let cfg = with_explicit_scope(
            rule_id,
            ValueMapping::Literal {
                value: scope.into(),
            },
        );
        let logs = parse(
            &cfg,
            &[
                (scope, r#"Request "work" [r1] sent"#),
                (scope, r#"Request "work" [r1] completed"#),
            ],
        );
        let result =
            perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &cfg);
        assert_eq!(
            result.operations.len(),
            1,
            "{rule_id}: {:?}",
            result.unmatched_events
        );
        assert_eq!(result.operations[0].duration_ms, 1000);
    }
}

#[test]
fn an_explicit_summary_cannot_impersonate_an_inherited_long_scope() {
    let long = long_scope("x");
    let summary = bounded_scope_value(&long);
    for rule_id in ["request-start-id", "request-end-id"] {
        let cfg = with_explicit_scope(
            rule_id,
            ValueMapping::Literal {
                value: summary.clone(),
            },
        );
        let logs = parse(
            &cfg,
            &[
                (&long, r#"Request "work" [r1] sent"#),
                (&long, r#"Request "work" [r1] completed"#),
            ],
        );
        let result =
            perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &cfg);
        assert!(
            result.operations.is_empty(),
            "{rule_id} impersonated a long scope"
        );
    }
}

#[test]
fn explicit_scope_values_retain_the_input_limit() {
    let cfg = with_explicit_scope(
        "request-start-id",
        ValueMapping::Field {
            field: "payload.scope".into(),
        },
    );
    for length in [MAX_VALUE_BYTES, MAX_VALUE_BYTES + 1] {
        let scope = "x".repeat(length);
        let message = format!(
            "Request \"work\" [r1] sent with body {}",
            serde_json::json!({"scope": scope})
        );
        let logs = parse(
            &cfg,
            &[
                (&scope, &message),
                (&scope, r#"Request "work" [r1] completed"#),
            ],
        );
        let result =
            perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, &cfg);
        assert_eq!(
            result.operations.len(),
            usize::from(length == MAX_VALUE_BYTES),
            "length={length}"
        );
        if length > MAX_VALUE_BYTES {
            assert!(matches!(
                &logs[0].classification,
                Some(log_analyzer::event_rules::ClassifiedRecord::Invalid { .. })
            ));
        }
    }
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
fn a_short_scope_cannot_impersonate_a_long_scope_summary() {
    let long = long_scope("x");
    let summary = bounded_scope_value(&long);
    let result = analyze(&[
        (&long, r#"Request "work" [r1] sent"#),
        (&summary, r#"Request "work" [r1] completed"#),
    ]);
    assert!(
        result.operations.is_empty(),
        "a raw summary impersonated a long scope"
    );
}

#[test]
fn long_scopes_with_identical_prefix_and_length_remain_separate() {
    let a = format!("{}a", "x".repeat(MAX_VALUE_BYTES));
    let b = format!("{}b", "x".repeat(MAX_VALUE_BYTES));
    let result = analyze(&[
        (&a, r#"Request "work" [r1] sent"#),
        (&b, r#"Request "work" [r1] completed"#),
    ]);
    assert!(result.operations.is_empty());
}

#[test]
fn reserved_marker_values_are_encoded_even_when_short() {
    let raw = " bytes, fnv1a128:";
    let key = bounded_scope_value(raw);
    assert_ne!(raw, key);
    assert_ne!(key, bounded_scope_value(&key));
    assert!(key.len() <= MAX_VALUE_BYTES);
}

#[test]
fn prefix_truncation_preserves_utf8_boundaries() {
    let value = format!("a{}", "😀".repeat(MAX_VALUE_BYTES));
    let key = bounded_scope_value(&value);
    assert!(key.starts_with(&format!("a{}…[", "😀".repeat(15))));
    assert!(key.len() <= MAX_VALUE_BYTES);
    assert_eq!(
        bounded_scope_value(&"x".repeat(MAX_VALUE_BYTES)).len(),
        MAX_VALUE_BYTES
    );
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
