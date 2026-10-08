use log_analyzer::config::{AnalyzerConfig, load_config_from_path};
use log_analyzer::event_rules::*;
use log_analyzer::parser::{LogEntry, parse_log_entry};
use serde_json::{Value, json};

fn record() -> LogEntry {
    parse_log_entry("worker | 2026-10-08T10:00:00+02:00 [INFO ] inspected", 7).unwrap()
}

fn compile(rules: Value) -> CompiledEventRules {
    let schema = serde_json::from_value(json!({"version": 1, "rules": rules})).unwrap();
    CompiledEventRules::compile(schema).unwrap()
}

fn literal(value: &str) -> Value {
    json!({"from": "literal", "value": value})
}
fn field(key: &str) -> Value {
    json!({"from": "field", "field": key})
}
fn capture(key: &str) -> Value {
    json!({"from": "capture", "capture": key})
}
fn mapping(name: Value) -> Value {
    json!({"kind": "command", "name": name, "phase": literal("end"),
        "outcome": literal("success"), "correlation_id": field("id"), "scope": [field("session")]})
}
fn text_rule() -> Value {
    json!({"id": "text-end", "adapter": {"type": "text", "pattern": r#"Operation (?P<name>"(?:[^"\\]|\\.)*") completed(?: payload=.*)?"#},
        "mapping": mapping(json!({"from": "capture", "capture": "name", "decode": "json_string"}))})
}
fn structured_rule() -> Value {
    json!({"id": "structured-end", "adapter": {"type": "structured", "conditions": [
        {"field": "phase", "equals": "end"}, {"field": "ok", "equals": true}, {"field": "code", "equals": 200}
    ]}, "mapping": mapping(field("operation"))})
}
fn classify<'a>(
    rules: &'a CompiledEventRules,
    record: &'a LogEntry,
    message: &'a str,
    fields: &'a Value,
) -> Classification<'a> {
    rules.classify(
        "synthetic",
        RecordInput {
            record,
            original_message: message,
            fields: StructuredFields::Json(fields.as_object().unwrap()),
        },
    )
}
fn recognized(result: Classification<'_>) -> EventSemantics {
    match result {
        Classification::Recognized(event) => event.semantics,
        other => panic!("{other:?}"),
    }
}
fn basic_fields() -> Value {
    json!({"id": "trace-1", "session": "session-1"})
}

#[test]
fn adapters_produce_equivalent_semantics_and_preserve_provenance() {
    let rules = compile(json!([text_rule(), structured_rule()]));
    let mut entry = record();
    entry.source_file = Some("synthetic.log".into());
    entry.source_row_path = Some("/rows/2".into());
    entry.normalized_record = Some("normalized synthetic record".into());
    let fields = basic_fields();
    let text = recognized(classify(
        &rules,
        &entry,
        r#"Operation "reconcile" completed"#,
        &fields,
    ));
    let structured = json!({"phase": "end", "ok": true, "code": 200, "operation": "reconcile", "id": "trace-1", "session": "session-1"});
    let Classification::Recognized(event) =
        classify(&rules, &entry, "structured result", &structured)
    else {
        panic!()
    };
    assert_eq!(event.semantics, text);
    assert!(std::ptr::eq(event.record, &entry));
    assert_eq!(event.profile, "synthetic");
    assert_eq!(event.rule_ids, ["structured-end"]);
    assert_eq!(
        event
            .record
            .source_timestamp
            .unwrap()
            .offset()
            .local_minus_utc(),
        7200
    );
    assert_eq!(event.record.source_row_path.as_deref(), Some("/rows/2"));
    assert_eq!(event.record.source_line_number, 7);
}

#[test]
fn full_message_matching_rejects_unrelated_prose_and_preserves_unknown_record() {
    let rules = compile(json!([text_rule()]));
    let entry = record();
    for message in [
        "cleanup completed",
        r#"Maybe Operation "work" completed"#,
        r#"Operation "work" completed while cleanup started"#,
        r#"Operation "work" inspected and assembly completed"#,
        r#"Operation "work" inspected etc. Cleanup completed"#,
        r#"Operation "work" inspected so cleanup completed"#,
        r#"Operation "work" not completed"#,
        r#"Operation "work" completed
"#,
    ] {
        assert!(
            matches!(
                classify(&rules, &entry, message, &basic_fields()),
                Classification::Unclassified
            ),
            "{message}"
        );
    }
    assert_eq!(entry.message, "inspected");
}

#[test]
fn escaped_unicode_names_and_original_payload_message() {
    let rules = compile(json!([text_rule()]));
    let mut entry = record();
    entry.message = "clean display message".into();
    let fields = basic_fields();
    let event = recognized(classify(
        &rules,
        &entry,
        r#"Operation "héllo \"世界\"" completed payload={"phase":"start"}"#,
        &fields,
    ));
    assert_eq!(event.name, "héllo \"世界\"");
    assert_eq!(event.phase, Some(Phase::End));
    assert!(matches!(
        classify(&rules, &entry, &entry.message, &fields),
        Classification::Unclassified
    ));
    // JSON-looking text does not decode itself or provide structured phase fields.
    let structured = compile(json!([structured_rule()]));
    assert!(matches!(
        classify(
            &structured,
            &entry,
            r#"{"phase":"end","ok":true,"code":200}"#,
            &fields
        ),
        Classification::Unclassified
    ));
}

#[test]
fn malformed_escapes_and_missing_optional_capture_are_invalid() {
    let mut rule = text_rule();
    let rules = compile(json!([rule]));
    assert!(matches!(
        classify(
            &rules,
            &record(),
            r#"Operation "bad\q" completed"#,
            &basic_fields()
        ),
        Classification::Invalid { .. }
    ));
    rule = text_rule();
    rule["adapter"]["pattern"] = json!(r#"Operation(?: (?P<name>"[^"]*"))? completed"#);
    let rules = compile(json!([rule]));
    let entry = record();
    let fields = basic_fields();
    let Classification::Invalid { diagnostics } =
        classify(&rules, &entry, "Operation completed", &fields)
    else {
        panic!()
    };
    assert_eq!(diagnostics[0].reason, "missing_capture");
    assert_eq!(diagnostics[0].target, "name");
}

#[test]
fn structured_conditions_do_not_coerce_types() {
    let rules = compile(json!([structured_rule()]));
    let entry = record();
    let mut fields =
        json!({"phase":"end", "ok":true,"code":200,"operation":"work","id":"1","session":"s"});
    for (key, value) in [
        ("ok", json!("true")),
        ("ok", Value::Null),
        ("code", json!("200")),
        ("phase", json!(["end"])),
    ] {
        let previous = fields[key].clone();
        fields[key] = value;
        assert!(matches!(
            classify(&rules, &entry, "anything", &fields),
            Classification::Unclassified
        ));
        fields[key] = previous;
    }
    for value in [Value::Null, json!(22), json!([]), json!({}), json!(" ")] {
        fields["operation"] = value;
        assert!(matches!(
            classify(&rules, &entry, "anything", &fields),
            Classification::Invalid { .. }
        ));
    }
    let mut flat = entry.structured_fields.clone();
    for (key, value) in [("phase", "end"), ("ok", "true"), ("code", "200")] {
        flat.insert(key.into(), value.into());
    }
    assert!(matches!(
        rules.classify(
            "synthetic",
            RecordInput {
                record: &entry,
                original_message: "",
                fields: StructuredFields::Flat(&flat)
            }
        ),
        Classification::Unclassified
    ));
}

#[test]
fn equivalent_rules_keep_all_ids_conflicts_never_pick_first_and_invalid_wins() {
    let mut second = text_rule();
    second["id"] = json!("equivalent");
    let rules = compile(json!([text_rule(), second]));
    let entry = record();
    let fields = basic_fields();
    let message = r#"Operation "work" completed"#;
    let Classification::Recognized(event) = classify(&rules, &entry, message, &fields) else {
        panic!()
    };
    assert_eq!(event.rule_ids, ["text-end", "equivalent"]);
    for key in [
        "name",
        "phase",
        "correlation_id",
        "scope",
        "kind",
        "outcome",
    ] {
        let mut other = text_rule();
        other["id"] = json!("other");
        other["mapping"][key] = match key {
            "kind" => json!("request"),
            "scope" => json!([literal("other")]),
            "outcome" => literal("failure"),
            "phase" => {
                other["mapping"]["outcome"] = Value::Null;
                literal("start")
            }
            _ => literal("other"),
        };
        for definitions in [json!([text_rule(), other]), json!([other, text_rule()])] {
            let rules = compile(definitions);
            assert!(
                matches!(classify(&rules,&entry,message,&fields),Classification::Conflict {rule_ids} if rule_ids.len()==2),
                "{key}"
            );
        }
    }
    let mut bad = text_rule();
    bad["id"] = json!("invalid");
    bad["mapping"]["name"] = field("missing");
    let rules = compile(json!([text_rule(), bad]));
    assert!(matches!(
        classify(&rules, &entry, message, &fields),
        Classification::Invalid { .. }
    ));
}

#[test]
fn identity_only_and_missing_evidence_never_imply_completion() {
    let mut rule = text_rule();
    rule["mapping"]["phase"] = Value::Null;
    rule["mapping"]["outcome"] = Value::Null;
    rule["mapping"]["correlation_id"] = Value::Null;
    rule["mapping"]["scope"] = json!([]);
    let rules = compile(json!([rule]));
    assert!(
        matches!(classify(&rules,&record(),r#"Operation "work" completed"#,&json!({})),Classification::IdentityOnly(event) if event.semantics.phase.is_none() && event.semantics.correlation_id.is_none())
    );
    let rules = compile(json!([text_rule()]));
    assert!(matches!(
        classify(
            &rules,
            &record(),
            r#"Operation "work" completed"#,
            &json!({"id":"i"})
        ),
        Classification::Invalid { .. }
    ));
}

#[test]
fn dynamic_phase_and_outcome_are_strict() {
    let mut rule = text_rule();
    rule["mapping"]["phase"] = field("phase");
    rule["mapping"]["outcome"] = field("outcome");
    let rules = compile(json!([rule]));
    for (phase, outcome) in [
        ("END", "success"),
        ("end", "ok"),
        ("start", "success"),
        ("", "failure"),
    ] {
        let mut fields = basic_fields();
        fields["phase"] = json!(phase);
        fields["outcome"] = json!(outcome);
        assert!(matches!(
            classify(&rules, &record(), r#"Operation "work" completed"#, &fields),
            Classification::Invalid { .. }
        ));
    }
}

#[test]
fn configuration_validation_and_limits() {
    fn invalid(value: Value) {
        let result = serde_json::from_value::<EventRuleConfig>(value)
            .map_err(|e| e.to_string())
            .and_then(|s| CompiledEventRules::compile(s).map_err(|e| e.to_string()));
        assert!(result.is_err());
    }
    invalid(json!({"version":2,"rules":[]}));
    invalid(json!({"version":1,"rules":[text_rule(),text_rule()]}));
    for (path, value) in [
        ("/adapter/pattern", json!("(")),
        ("/adapter/pattern", json!("a".repeat(MAX_PATTERN_BYTES + 1))),
        ("/id", json!("x".repeat(129))),
        ("/mapping/name/capture", json!("absent")),
        ("/mapping/phase", literal("complete")),
        ("/mapping/outcome", literal("ok")),
        ("/mapping/name", literal("")),
        ("/mapping/phase", literal("start")),
        (
            "/mapping/scope",
            json!(vec![literal("scope"); MAX_SCOPE_FIELDS + 1]),
        ),
    ] {
        let mut rule = text_rule();
        *rule.pointer_mut(path).unwrap() = value;
        invalid(json!({"version":1,"rules":[rule]}));
    }
    let mut rule = text_rule();
    rule["mapping"]["name"]["unknown"] = json!("typo");
    invalid(json!({"version":1,"rules":[rule]}));
    let mut rule = structured_rule();
    rule["mapping"]["name"] = capture("absent");
    invalid(json!({"version":1,"rules":[rule]}));
    for conditions in [
        json!([]),
        json!(vec![
            json!({"field":"ok","equals":true});
            MAX_CONDITIONS + 1
        ]),
        json!([{"field":"ok","equals":{}}]),
        json!([{"field":"ok","equals":null}]),
    ] {
        let mut rule = structured_rule();
        rule["adapter"]["conditions"] = conditions;
        invalid(json!({"version":1,"rules":[rule]}));
    }
    let many: Vec<_> = (0..=MAX_RULES)
        .map(|i| {
            let mut r = text_rule();
            r["id"] = json!(format!("rule-{i}"));
            r
        })
        .collect();
    invalid(json!({"version":1,"rules":many}));
    // A short pattern can still exceed the compiled automaton limit.
    let mut rule = text_rule();
    rule["adapter"]["pattern"] = json!(r"(?P<name>\w{1000000})");
    invalid(json!({"version":1,"rules":[rule]}));
}

#[test]
fn record_resource_limits_are_explicit_and_unicode_safe() {
    let rules = compile(json!([text_rule()]));
    let entry = record();
    let fields = basic_fields();
    let huge = "界".repeat(MAX_MESSAGE_BYTES / 3 + 1);
    assert!(
        matches!(classify(&rules,&entry,&huge,&fields),Classification::Invalid {diagnostics} if diagnostics[0].reason=="message_limit_exceeded")
    );
    let message = format!(
        "Operation \"{}\" completed",
        "界".repeat(MAX_VALUE_BYTES / 3 + 1)
    );
    assert!(matches!(
        classify(&rules, &entry, &message, &fields),
        Classification::Invalid { .. }
    ));
    let mut fields = fields;
    fields["id"] = json!("x".repeat(MAX_VALUE_BYTES + 1));
    assert!(matches!(
        classify(&rules, &entry, r#"Operation "work" completed"#, &fields),
        Classification::Invalid { .. }
    ));
}

#[test]
fn loaded_profiles_compile_once_round_trip_and_reject_mixed_markers() {
    let rules = compile(json!([text_rule()]));
    let config = AnalyzerConfig {
        event_rules: Some(rules),
        ..AnalyzerConfig::default()
    };
    let raw = toml::to_string_pretty(&config).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profile.toml");
    std::fs::write(&path, &raw).unwrap();
    let loaded = load_config_from_path(&path).unwrap();
    let clone = loaded.clone();
    assert!(std::ptr::eq(
        loaded.event_rules.as_ref().unwrap().schema(),
        clone.event_rules.as_ref().unwrap().schema()
    ));
    let mut mixed = config.clone();
    mixed.parser.command_prefix = "Operation".into();
    std::fs::write(&path, toml::to_string_pretty(&mixed).unwrap()).unwrap();
    assert!(
        load_config_from_path(&path)
            .unwrap_err()
            .to_string()
            .contains("cannot coexist")
    );
    std::fs::write(&path, raw.replace("version = 1", "version = 2")).unwrap();
    assert!(
        load_config_from_path(&path)
            .unwrap_err()
            .to_string()
            .contains("unsupported version")
    );
    assert!(AnalyzerConfig::default().event_rules.is_none());
}

#[test]
fn adding_a_format_needs_only_rules_and_flat_field_fixtures() {
    let rule = json!({"id":"new-format", "adapter":{"type":"text","pattern":r"DONE\|(?P<name>[^|]+)\|(?P<id>[^|]+)"},
        "mapping":{"kind":"request","name":capture("name"),"phase":literal("end"),"correlation_id":capture("id"),"scope":[field("service")]}});
    let rules = compile(json!([rule]));
    let mut entry = record();
    entry
        .structured_fields
        .insert("service".into(), "worker".into());
    let result = rules.classify(
        "new-format",
        RecordInput {
            record: &entry,
            original_message: "DONE|héllo|trace-2",
            fields: StructuredFields::Flat(&entry.structured_fields),
        },
    );
    let event = recognized(result);
    assert_eq!(event.kind, OperationKind::Request);
    assert_eq!(event.name, "héllo");
    assert_eq!(event.scope, ["worker"]);
}

#[test]
fn documented_toml_example_loads_and_compiles() {
    let document = include_str!("../docs/design/event-classification.md");
    let raw = document
        .split("```toml\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("documented.toml");
    std::fs::write(&path, raw).unwrap();
    let config = load_config_from_path(&path).unwrap();
    let fields = json!({"trace_id":"trace-1","session":"session-1"});
    assert!(matches!(
        classify(
            config.event_rules.as_ref().unwrap(),
            &record(),
            r#"Operation "work" completed"#,
            &fields
        ),
        Classification::Recognized(_)
    ));
}

#[test]
fn absolute_anchors_hold_under_inline_multiline_flags() {
    let mut rule = text_rule();
    rule["adapter"]["pattern"] = json!(r#"(?m)^Operation (?P<name>"[^"]*") completed$"#);
    let rules = compile(json!([rule]));
    let entry = record();
    let fields = basic_fields();
    for message in [
        "prefix\nOperation \"work\" completed",
        "Operation \"work\" completed\n",
        "Operation \"work\" completed\nsuffix",
    ] {
        assert!(matches!(
            classify(&rules, &entry, message, &fields),
            Classification::Unclassified
        ));
    }
}
