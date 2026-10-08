use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::tempdir;
fn invoke(args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
    cmd.args(args);
    for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("LOG_ANALYZER")) {
        cmd.env_remove(key);
    }
    cmd.output().unwrap()
}
fn report(args: &[&str]) -> (Value, Output) {
    let output = invoke(args);
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    static VALIDATOR: std::sync::LazyLock<jsonschema::Validator> = std::sync::LazyLock::new(|| {
        jsonschema::validator_for(
            &serde_json::from_str::<Value>(include_str!("../schemas/report.schema.json")).unwrap(),
        )
        .unwrap()
    });
    let errors: Vec<_> = VALIDATOR
        .iter_errors(&value)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}\n{value}");
    (value, output)
}
fn field(name: &str) -> Value {
    json!({"from":"field","field":name})
}
fn literal(value: &str) -> Value {
    json!({"from":"literal","value":value})
}
fn rules() -> Value {
    json!([{"id":"start","adapter":{"type":"structured","conditions":[{"field":"phase","equals":"start"}]},"mapping":{"kind":"request","name":field("operation"),"phase":literal("start"),"correlation_id":field("id"),"scope":[field("session")]}},{"id":"end","adapter":{"type":"structured","conditions":[{"field":"phase","equals":"end"}]},"mapping":{"kind":"request","name":field("operation"),"phase":literal("end"),"correlation_id":field("id"),"scope":[field("session")]}}])
}
fn profile(dir: &Path, name: &str, rules: Value) -> String {
    let path = dir.join(format!("{name}.toml"));
    fs::write(&path,toml::to_string(&json!({"extends":"base","profile_name":name,"parser":{"format":"json-lines"},"event_rules":{"version":2,"rules":rules}})).unwrap()).unwrap();
    path.to_str().unwrap().into()
}
fn rows() -> Vec<Value> {
    vec![
        json!({"ts":"2026-01-01T00:00:00+02:00","level":"INFO","component":"worker","component_id":"a","message":"Begin work reused a","phase":"start","operation":"work","id":"reused","session":"a"}),
        json!({"ts":"2026-01-01T00:00:01+02:00","level":"INFO","component":"worker","component_id":"a","message":"Done work reused a","phase":"end","operation":"work","id":"reused","session":"a"}),
        json!({"ts":"2026-01-01T00:00:02+02:00","level":"INFO","component":"worker","component_id":"a","message":"context only"}),
    ]
}
fn log(dir: &Path, rows: &[Value]) -> String {
    let path = dir.join("sample.jsonl");
    fs::write(
        &path,
        rows.iter()
            .map(|v| v.to_string() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    path.to_str().unwrap().into()
}
fn run(profile: &str, file: &str, extra: &[&str]) -> (Value, Output) {
    let mut args = vec![
        "--config",
        profile,
        "validate-profile",
        file,
        "--kind",
        "request",
    ];
    args.extend(extra);
    report(&args)
}
#[test]
fn supported_sample_has_rule_witnesses_and_separate_parsing_classification_and_pairing() {
    let dir = tempdir().unwrap();
    let p = profile(dir.path(), "candidate", rules());
    let file = log(dir.path(), &rows());
    let (v, o) = run(&p, &file, &[]);
    assert!(o.status.success());
    let r = &v["profile_validation"];
    assert_eq!(r["suitability"]["status"], "supported");
    assert_eq!(v["coverage"]["parsed_entries"], 3);
    assert_eq!(r["global_classification"]["unclassified_records"], 1);
    assert_eq!(
        r["requested_coverage"]["classification"]["selected_records"],
        2
    );
    assert_eq!(r["operations"][0]["duration_ms"], 1000);
    assert_eq!(r["records"][0]["classification"]["rule_ids"][0], "start");
    assert!(r["records"][0]["evidence_ref"]["reference_id"].is_string());
    assert!(r["operations"][0]["start_source"]["evidence_ref"].is_object());
}
#[test]
fn parsed_wrong_vocabulary_and_other_kind_are_unsupported_without_match_ranking() {
    let dir = tempdir().unwrap();
    let mut wrong = rules();
    for r in wrong.as_array_mut().unwrap() {
        r["adapter"]["conditions"][0]["field"] = json!("unknown_phase");
    }
    let p = profile(dir.path(), "wrong", wrong);
    let file = log(dir.path(), &rows());
    let (v, o) = run(&p, &file, &[]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(v["coverage"]["parsed_entries"], 3);
    assert_eq!(
        v["profile_validation"]["suitability"]["status"],
        "unsupported"
    );
    assert!(v["profile_validation"]["suggestions"][0]["source"]["evidence_ref"].is_object());
    let mut other = rules();
    for r in other.as_array_mut().unwrap() {
        r["mapping"]["kind"] = json!("event");
    }
    let p = profile(dir.path(), "other-kind", other);
    let (v, o) = run(&p, &file, &[]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        v["profile_validation"]["global_classification"]["classified_records"],
        2
    );
    assert_eq!(
        v["profile_validation"]["requested_coverage"]["paired_events"],
        0
    );
    assert_eq!(
        v["profile_validation"]["suitability"]["status"],
        "unsupported"
    );
}
#[test]
fn conflicting_invalid_identity_only_incomplete_and_start_only_have_distinct_diagnostics() {
    let dir = tempdir().unwrap();
    let file = log(dir.path(), &rows());
    let mut conflict = rules();
    let mut duplicate = conflict[0].clone();
    duplicate["id"] = json!("different");
    duplicate["mapping"]["name"] = literal("other");
    conflict.as_array_mut().unwrap().push(duplicate);
    let p = profile(dir.path(), "conflict", conflict);
    let (v, _) = run(&p, &file, &[]);
    assert_eq!(
        v["profile_validation"]["suitability"]["status"],
        "conflicting"
    );
    assert_eq!(
        v["profile_validation"]["records"][0]["classification"]["status"],
        "conflict"
    );
    let mut invalid = rows();
    invalid[0]["operation"] = json!({"unsupported":"type"});
    let f = log(dir.path(), &invalid);
    let p = profile(dir.path(), "valid", rules());
    let (v, _) = run(&p, &f, &[]);
    assert_eq!(
        v["profile_validation"]["records"][0]["classification"]["status"],
        "invalid"
    );
    assert_eq!(
        v["profile_validation"]["suitability"]["status"],
        "insufficient_evidence"
    );
    let mut identity = rules();
    for r in identity.as_array_mut().unwrap() {
        r["mapping"].as_object_mut().unwrap().remove("phase");
    }
    let p = profile(dir.path(), "identity", identity);
    let f = log(dir.path(), &rows());
    let (v, _) = run(&p, &f, &[]);
    assert_eq!(
        v["profile_validation"]["suitability"]["status"],
        "insufficient_evidence"
    );
    let (v, o) = run(&p, &f, &["--purpose", "recognition"]);
    assert!(o.status.success());
    assert_eq!(
        v["profile_validation"]["requested_coverage"]["classification"]["identity_only_records"],
        2
    );
    let p = profile(dir.path(), "normal", rules());
    let f = log(dir.path(), &rows()[..1]);
    let (v, _) = run(&p, &f, &[]);
    assert_eq!(
        v["profile_validation"]["suitability"]["status"],
        "insufficient_evidence"
    );
    assert!(
        v["profile_validation"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["reason"] == "missing_end")
    );
    let mut start = rules();
    start.as_array_mut().unwrap().truncate(1);
    start[0]["mapping"]["end_expected"] = json!(false);
    let p = profile(dir.path(), "start-only", start);
    let (v, _) = run(&p, &f, &[]);
    assert_eq!(
        v["profile_validation"]["suitability"]["status"],
        "unsupported"
    );
    assert_eq!(
        v["profile_validation"]["requested_coverage"]["start_only_events"],
        1
    );
    assert_eq!(v["profile_validation"]["totals"]["operations"], 0);
    assert_eq!(
        v["profile_validation"]["diagnostics"][0]["reason"],
        "intentional_start_only"
    );
    let (_, o) = run(&p, &f, &["--purpose", "recognition"]);
    assert!(o.status.success());
}
#[test]
fn reused_ids_expose_missing_or_aliasing_scope_with_witnesses() {
    let dir = tempdir().unwrap();
    let mut data = rows();
    data.truncate(2);
    let mut more = data.clone();
    for row in &mut more {
        row["component_id"] = json!("b");
        row["session"] = json!("b");
    }
    data.extend(more);
    let file = log(dir.path(), &data);
    let p = profile(dir.path(), "scoped", rules());
    let (v, o) = run(&p, &file, &[]);
    assert!(o.status.success());
    assert_eq!(v["profile_validation"]["totals"]["operations"], 2);
    let mut missing = rules();
    for r in missing.as_array_mut().unwrap() {
        r["mapping"]["scope"] = json!([]);
    }
    let p = profile(dir.path(), "missing", missing);
    let mut config = fs::read_to_string(&p).unwrap();
    config.push_str("\n[perf]\ncorrelation_scope_fields = []\n");
    fs::write(&p, config).unwrap();
    let (v, _) = run(&p, &file, &[]);
    assert_eq!(
        v["profile_validation"]["suitability"]["status"],
        "insufficient_evidence"
    );
    assert!(
        v["profile_validation"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["reason"] == "missing_scope_field")
    );
    let mut constant = rules();
    for r in constant.as_array_mut().unwrap() {
        r["mapping"]["scope"] = json!([literal("constant")]);
    }
    let p = profile(dir.path(), "constant", constant);
    let (v, o) = run(&p, &file, &[]);
    assert!(!o.status.success());
    assert!(
        v["profile_validation"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["reason"] == "scope_adequacy_unknown"
                && d["witnesses"].as_array().unwrap().len() == 4)
    );
}
#[test]
fn strict_expected_positive_negative_and_pair_facts_validate_inherited_candidate_without_writes() {
    let dir = tempdir().unwrap();
    let parent = profile(dir.path(), "parent", rules());
    let child = dir.path().join("child.toml");
    fs::write(
        &child,
        "extends = 'parent.toml'\nprofile_name = 'editable-candidate'\n",
    )
    .unwrap();
    let before = fs::read(&child).unwrap();
    let file = log(dir.path(), &rows());
    let address = |line| json!({"input":0,"line":line,"row_path":null});
    let facts = dir.path().join("expected.json");
    fs::write(&facts,json!({"version":1,"records":[{"source":address(1),"checks":{"/status":"event","/semantics/phase":"start","/semantics/scope":["a"],"/rule_ids":["start"],"/semantics/end_expected":true}},{"source":address(3),"checks":{"/status":"unclassified"}}],"pairs":[{"start":address(1),"end":address(2),"duration_ms":1000}]}).to_string()).unwrap();
    let (v, o) = run(
        child.to_str().unwrap(),
        &file,
        &["--expected", facts.to_str().unwrap()],
    );
    assert!(o.status.success());
    assert_eq!(
        v["profile_validation"]["expected_facts"]["status"],
        "passed"
    );
    assert_eq!(v["profile_validation"]["totals"]["expected_checks"], 7);
    assert_eq!(fs::read(&child).unwrap(), before);
    assert!(Path::new(&parent).exists());
    let mut bad: Value = serde_json::from_slice(&fs::read(&facts).unwrap()).unwrap();
    bad["records"][0]["checks"]["/semantics/phase"] = json!("end");
    fs::write(&facts, bad.to_string()).unwrap();
    let (v, o) = run(
        child.to_str().unwrap(),
        &file,
        &["--expected", facts.to_str().unwrap()],
    );
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        v["profile_validation"]["suitability"]["status"],
        "unsupported"
    );
    bad["records"][0]["source"]["line"] = json!(99);
    fs::write(&facts, bad.to_string()).unwrap();
    let (v, _) = run(
        child.to_str().unwrap(),
        &file,
        &["--expected", facts.to_str().unwrap()],
    );
    assert_eq!(
        v["profile_validation"]["expected_results"][0]["reason"],
        "missing_source"
    );
    let mut unknown = bad.clone();
    unknown["typo"] = json!(true);
    fs::write(&facts, unknown.to_string()).unwrap();
    let o = invoke(&[
        "--config",
        child.to_str().unwrap(),
        "validate-profile",
        &file,
        "--kind",
        "request",
        "--expected",
        facts.to_str().unwrap(),
    ]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("unknown field"));
}
#[test]
fn text_and_structured_vocabularies_are_equivalent_and_malformed_rules_fail() {
    let dir = tempdir().unwrap();
    let file = log(dir.path(), &rows());
    let p = profile(dir.path(), "structured", rules());
    let (a, _) = run(&p, &file, &[]);
    let mut text = rules();
    for (idx, r) in text.as_array_mut().unwrap().iter_mut().enumerate() {
        r["adapter"] = json!({"type":"text","pattern":format!("{} (?P<name>\\w+) (?P<id>\\w+) (?P<scope>\\w+)",if idx==0{"Begin"}else{"Done"})});
        r["mapping"]["name"] = json!({"from":"capture","capture":"name"});
        r["mapping"]["correlation_id"] = json!({"from":"capture","capture":"id"});
        r["mapping"]["scope"] = json!([{"from":"capture","capture":"scope"}]);
    }
    let p = profile(dir.path(), "text", text.clone());
    let (b, o) = run(&p, &file, &[]);
    assert!(o.status.success());
    assert_eq!(
        a["profile_validation"]["records"][0]["classification"]["semantics"],
        b["profile_validation"]["records"][0]["classification"]["semantics"]
    );
    text[0]["adapter"]["pattern"] = json!("[");
    let p = profile(dir.path(), "malformed", text);
    let o = invoke(&[
        "--config",
        &p,
        "validate-profile",
        &file,
        "--kind",
        "request",
    ]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("event rules"));
}
#[test]
fn unparsed_empty_filtered_and_assumed_chronology_never_report_supported_timing() {
    let dir = tempdir().unwrap();
    let p = profile(dir.path(), "sample", rules());
    let file = dir.path().join("unknown.log");
    fs::write(&file, "not a record\n").unwrap();
    let (v, o) = run(&p, file.to_str().unwrap(), &[]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(v["coverage"]["status"], "unparsed_input");
    assert_eq!(
        v["profile_validation"]["suitability"]["status"],
        "insufficient_evidence"
    );
    fs::write(&file, "").unwrap();
    let (v, _) = run(&p, file.to_str().unwrap(), &[]);
    assert_eq!(v["coverage"]["status"], "empty_input");
    let file = log(dir.path(), &rows());
    let (v, o) = report(&[
        "--config",
        &p,
        "--filter",
        "level:ERROR",
        "validate-profile",
        &file,
        "--kind",
        "request",
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(v["coverage"]["status"], "zero_filter_matches");
    let mut nooffset = rows();
    for row in &mut nooffset {
        row["ts"] = json!("2026-01-01T00:00:00");
    }
    let file = log(dir.path(), &nooffset);
    let (v, o) = run(&p, &file, &[]);
    assert!(!o.status.success());
    assert_eq!(v["coverage"]["status"], "parsed");
    assert_eq!(
        v["profile_validation"]["suitability"]["status"],
        "insufficient_evidence"
    );
}

#[test]
fn generated_candidate_and_failed_negative_facts_use_the_same_validation_path() {
    let dir = tempdir().unwrap();
    let p = profile(dir.path(), "template", rules());
    let file = log(dir.path(), &rows());
    let generated = invoke(&[
        "--config",
        &p,
        "generate-config",
        &file,
        "--profile-name",
        "editable-candidate",
    ]);
    assert!(generated.status.success());
    let candidate = dir.path().join("generated.toml");
    fs::write(&candidate, generated.stdout).unwrap();
    let (v, o) = run(candidate.to_str().unwrap(), &file, &[]);
    assert!(o.status.success());
    assert_eq!(v["report_metadata"]["active_profile"], "editable-candidate");
    let facts = dir.path().join("facts.json");
    fs::write(&facts,json!({"version":1,"records":[{"source":{"input":0,"line":3,"row_path":null},"checks":{"/semantics/phase":null}}]}).to_string()).unwrap();
    let (v, o) = run(
        candidate.to_str().unwrap(),
        &file,
        &["--expected", facts.to_str().unwrap()],
    );
    assert!(!o.status.success());
    assert_eq!(
        v["profile_validation"]["expected_results"][0]["reason"],
        "missing_classification_field"
    );
    assert_eq!(
        v["profile_validation"]["expected_results"][0]["observed_available"],
        false
    );
}

#[test]
fn bounded_redacted_validation_reconstructs_and_changed_expectations_reject_cursor() {
    let dir = tempdir().unwrap();
    let mut private = rows();
    for row in &mut private {
        row["session"] = json!("private-scope");
        row["component_id"] = json!("private-scope");
    }
    let file = log(dir.path(), &private);
    let p = profile(dir.path(), "private-scope", rules());
    let facts = dir.path().join("facts.json");
    fs::write(&facts,json!({"version":1,"records":[{"source":{"input":0,"line":1,"row_path":null},"checks":{"/semantics/scope":["private-scope"]}}]}).to_string()).unwrap();
    let base = vec![
        "--config",
        &p,
        "--redact",
        "--mask-id",
        "component_id",
        "validate-profile",
        &file,
        "--kind",
        "request",
        "--expected",
        facts.to_str().unwrap(),
    ];
    let mut complete_args = base.clone();
    complete_args.push("--complete-output");
    let (full, o) = report(&complete_args);
    assert!(o.status.success());
    assert!(!String::from_utf8_lossy(&o.stdout).contains("private-scope"));
    let mut rebuilt = full.clone();
    let mut cursor: Option<String> = None;
    let mut first_cursor = None;
    let mut paths = Vec::new();
    for n in 0..100 {
        let mut args = base.clone();
        args.extend(["--report-max-items", "2", "--report-max-bytes", "30000"]);
        if let Some(c) = &cursor {
            args.extend(["--report-cursor", c]);
        }
        let (page, o) = report(&args);
        assert!(o.status.success());
        assert!(o.stdout.len() <= 30000);
        assert!(!String::from_utf8_lossy(&o.stdout).contains("private-scope"));
        assert_eq!(
            page["profile_validation"]["totals"],
            full["profile_validation"]["totals"]
        );
        if n == 0 {
            paths = page["retrieval"]["collections"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["path"].as_str().unwrap().to_string())
                .collect();
            for path in &paths {
                *rebuilt.pointer_mut(path).unwrap() = json!([]);
            }
            first_cursor = page["retrieval"]["next_cursor"]
                .as_str()
                .map(str::to_string);
        }
        for path in &paths {
            rebuilt
                .pointer_mut(path)
                .unwrap()
                .as_array_mut()
                .unwrap()
                .extend(page.pointer(path).unwrap().as_array().unwrap().clone());
        }
        cursor = page["retrieval"]["next_cursor"]
            .as_str()
            .map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    assert!(cursor.is_none());
    for path in paths {
        assert_eq!(rebuilt.pointer(&path), full.pointer(&path), "{path}");
    }
    fs::write(&facts,json!({"version":1,"records":[{"source":{"input":0,"line":2,"row_path":null},"checks":{"/status":"event"}}]}).to_string()).unwrap();
    let mut args = base.clone();
    args.extend(["--report-cursor", first_cursor.as_deref().unwrap()]);
    let (v, o) = report(&args);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(v["retrieval"]["status"], "invalid_cursor");
    for field in [
        "profile_validation",
        "classification",
        "timestamp",
        "totals",
        "requested_coverage",
        "input_ordinal",
        "scope",
        "rule_ids",
    ] {
        let mut args = base.clone();
        args.splice(0..0, ["--complete-output", "--mask-id", field]);
        let (v, o) = report(&args);
        assert!(o.status.success());
        assert!(!String::from_utf8_lossy(&o.stdout).contains("private-scope"));
        assert_eq!(
            v["profile_validation"]["suitability"]["status"],
            "supported"
        );
    }
}

#[test]
fn normalized_row_facts_are_exact_and_redacted_location_loss_remains_explicit() {
    let dir = tempdir().unwrap();
    let mut data = rows();
    for row in &mut data {
        row["timestamp"] = row["ts"].clone();
        row["session"] = json!("private-row");
        row["component_id"] = json!("private-row");
    }
    let file = dir.path().join("nested.jsonl");
    fs::write(&file, json!({"private-row":data}).to_string()).unwrap();
    let p = profile(dir.path(), "nested", rules());
    let mut cfg = fs::read_to_string(&p).unwrap();
    cfg.push_str("\n[normalization]\nroot_path = '/private-row'\nexpand_rows = true\n");
    fs::write(&p, cfg).unwrap();
    let facts = dir.path().join("facts.json");
    let mut expected = json!({"version":1,"records":[{"source":{"input":0,"line":1,"row_path":"/private-row/0"},"checks":{"/status":"event"}}],"pairs":[{"start":{"input":0,"line":1,"row_path":"/private-row/0"},"end":{"input":0,"line":1,"row_path":"/private-row/1"},"duration_ms":1000}]});
    fs::write(&facts, expected.to_string()).unwrap();
    let (v, o) = report(&[
        "--config",
        &p,
        "--redact",
        "--mask-id",
        "component_id",
        "--complete-output",
        "validate-profile",
        file.to_str().unwrap(),
        "--kind",
        "request",
        "--expected",
        facts.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(!String::from_utf8_lossy(&o.stdout).contains("private-row"));
    assert_eq!(
        v["profile_validation"]["records"][0]["evidence_ref"]["location_redacted"],
        true
    );
    assert!(v["profile_validation"]["records"][0]["evidence_ref"]["row_path"].is_null());
    let (masked, o) = report(&[
        "--config",
        &p,
        "--redact",
        "--mask-id",
        "row_path",
        "--complete-output",
        "validate-profile",
        file.to_str().unwrap(),
        "--kind",
        "request",
        "--expected",
        facts.to_str().unwrap(),
    ]);
    assert!(o.status.success());
    for result in masked["profile_validation"]["expected_results"]
        .as_array()
        .unwrap()
    {
        for key in ["address", "start_address", "end_address"] {
            if let Some(address) = result.get(key) {
                assert!(address["row_path"].is_null());
                assert_eq!(address["location_redacted"], true);
            }
        }
    }

    expected["records"][0]["source"]["row_path"] = json!("/private-row/99");
    fs::write(&facts, expected.to_string()).unwrap();
    let (v, o) = run(
        &p,
        file.to_str().unwrap(),
        &["--expected", facts.to_str().unwrap()],
    );
    assert!(!o.status.success());
    assert_eq!(
        v["profile_validation"]["expected_results"][0]["reason"],
        "missing_source"
    );
    expected["records"][0]["source"]["row_path"] = json!("/private-row/0");
    fs::write(&facts, expected.to_string()).unwrap();
    let (v, o) = report(&[
        "--config",
        &p,
        "--filter",
        "message:Done",
        "validate-profile",
        file.to_str().unwrap(),
        "--kind",
        "request",
        "--expected",
        facts.to_str().unwrap(),
    ]);
    assert!(!o.status.success());
    assert_eq!(
        v["profile_validation"]["expected_results"][0]["reason"],
        "filtered_out_source"
    );
}

#[test]
fn unmatched_expected_condition_and_literal_values_keep_their_redaction_context() {
    let dir = tempdir().unwrap();
    let mut r = rules();
    r[0]["mapping"]["correlation_id"] = literal("literal-private");
    let mut unmatched = r[0].clone();
    unmatched["id"] = json!("unmatched");
    unmatched["adapter"]["conditions"] =
        json!([{"field":"trace_id","equals":"configured-private"}]);
    r.as_array_mut().unwrap().push(unmatched);
    let p = profile(dir.path(), "context", r);
    let mut data = rows();
    for row in &mut data {
        row["trace_id"] = json!("actual-id");
    }
    let file = log(dir.path(), &data);
    let facts = dir.path().join("facts.json");
    fs::write(&facts,json!({"version":1,"records":[{"source":{"input":0,"line":1,"row_path":null},"checks":{"/semantics/correlation_id":"expected-private"}}]}).to_string()).unwrap();
    let (v, o) = report(&[
        "--config",
        &p,
        "--redact",
        "--mask-id",
        "correlation_id",
        "--mask-id",
        "trace_id",
        "--complete-output",
        "validate-profile",
        &file,
        "--kind",
        "request",
        "--expected",
        facts.to_str().unwrap(),
    ]);
    assert!(!o.status.success());
    let text = String::from_utf8_lossy(&o.stdout);
    for private in [
        "literal-private",
        "expected-private",
        "configured-private",
        "actual-id",
    ] {
        assert!(!text.contains(private), "leaked {private}: {text}");
    }
    assert!(
        v["profile_validation"]["expected_results"][0]["expected"]
            .as_str()
            .unwrap()
            .starts_with("[MASKED_ID:")
    );
    let mut numeric = rules();
    numeric[0]["adapter"]["conditions"] = json!([{"field":"trace_id","equals":701}]);
    let p = profile(dir.path(), "numeric", numeric);
    let (v, o) = report(&[
        "--config",
        &p,
        "--redact",
        "--mask-id",
        "trace_id",
        "--complete-output",
        "validate-profile",
        &file,
        "--kind",
        "request",
    ]);
    assert!(!o.stdout.is_empty());
    assert!(v["profile_validation"]["effective_rules"]["event_rules"]["rules"][0]["adapter"]["conditions"][0]["equals"].as_str().unwrap().starts_with("[MASKED_ID:"));
    let mut invalid: Value = serde_json::from_slice(&fs::read(&facts).unwrap()).unwrap();
    invalid["records"][0]["checks"]["/semantics/correlation_id"] = json!(701);
    fs::write(&facts, invalid.to_string()).unwrap();
    let o = invoke(&[
        "--config",
        &p,
        "validate-profile",
        &file,
        "--kind",
        "request",
        "--expected",
        facts.to_str().unwrap(),
    ]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("invalid value types"));
}
#[test]
fn declared_scope_fields_expose_aliasing_and_recognition_ignores_timing_ambiguity() {
    let dir = tempdir().unwrap();
    let mut constant = rules();
    for r in constant.as_array_mut().unwrap() {
        r["mapping"]["scope"] = json!([literal("constant")]);
    }
    let p = profile(dir.path(), "constant", constant);
    let mut config = fs::read_to_string(&p).unwrap();
    config.push_str("\n[perf]\ncorrelation_scope_fields = ['session', 'missing']\n");
    fs::write(&p, config).unwrap();
    let mut data = rows();
    data.truncate(2);
    let mut second = data.clone();
    for row in &mut second {
        row["session"] = json!("b");
    }
    data.extend(second);
    let file = log(dir.path(), &data);
    let (v, o) = run(&p, &file, &[]);
    assert!(!o.status.success());
    assert_eq!(v["profile_validation"]["totals"]["scope_alias_groups"], 1);
    assert!(
        v["profile_validation"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["reason"] == "scope_adequacy_unknown")
    );
    let (v, o) = run(&p, &file, &["--purpose", "recognition"]);
    assert!(o.status.success());
    assert_eq!(
        v["profile_validation"]["suitability"]["status"],
        "supported"
    );
    assert_eq!(
        v["profile_validation"]["global_classification"]["classified_records"],
        4
    );
}
#[test]
fn every_validation_reference_path_rejects_invalid_lines_and_digest_mutations() {
    let dir = tempdir().unwrap();
    let file = log(dir.path(), &rows()[..1]);
    let p = profile(dir.path(), "refs", rules());
    let facts = dir.path().join("facts.json");
    fs::write(&facts,json!({"version":1,"records":[{"source":{"input":0,"line":1,"row_path":null},"checks":{"/status":"event"}}],"pairs":[{"start":{"input":0,"line":1,"row_path":null},"end":{"input":0,"line":1,"row_path":null}}]}).to_string()).unwrap();
    let (v, _) = run(
        &p,
        &file,
        &["--expected", facts.to_str().unwrap(), "--complete-output"],
    );
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/report.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    fn paths(v: &Value, path: &str, out: &mut Vec<String>) {
        match v {
            Value::Object(map) => {
                for (k, v) in map {
                    let p = format!("{path}/{}", k.replace('~', "~0").replace('/', "~1"));
                    if k == "evidence_ref" {
                        out.push(p);
                    } else {
                        paths(v, &p, out);
                    }
                }
            }
            Value::Array(a) => {
                for (i, v) in a.iter().enumerate() {
                    paths(v, &format!("{path}/{i}"), out);
                }
            }
            _ => (),
        }
    }
    let mut documents = vec![v];
    let mut constant = rules();
    for rule in constant.as_array_mut().unwrap() {
        rule["mapping"]["scope"] = json!([literal("constant")]);
    }
    let profile = profile(dir.path(), "witnesses", constant);
    let mut aliases = rows();
    aliases.truncate(2);
    let mut second = aliases.clone();
    for row in &mut second {
        row["component_id"] = json!("b");
        row["session"] = json!("b");
    }
    aliases.extend(second);
    let file = log(dir.path(), &aliases);
    let (value, _) = run(&profile, &file, &["--complete-output"]);
    documents.push(value);
    let mut witness_paths = 0;
    for v in documents {
        let mut refs = Vec::new();
        paths(&v["profile_validation"], "/profile_validation", &mut refs);
        assert!(refs.iter().any(|p| p.contains("suggestions")));
        assert!(refs.iter().any(|p| p.contains("diagnostics")));
        witness_paths += refs.iter().filter(|p| p.contains("witnesses")).count();
        for path in refs {
            for (key, bad) in [("line", json!(0)), ("reference_id", json!("bad-digest"))] {
                let mut mutant = v.clone();
                mutant.pointer_mut(&path).unwrap()[key] = bad;
                assert!(!validator.is_valid(&mutant), "accepted {path}/{key}");
            }
        }
    }
    assert!(witness_paths > 0);
}
#[test]
fn array_masks_without_expected_facts_hide_leaf_ids_and_expected_paths_are_path_redacted() {
    let dir = tempdir().unwrap();
    let mut data = rows();
    for row in &mut data {
        row["session"] = json!("private-id");
        row["component_id"] = json!("private-id");
    }
    let file = log(dir.path(), &data);
    let p = profile(dir.path(), "arrays", rules());
    for command in ["validate-profile", "perf"] {
        let mut args = vec![
            "--config",
            &p,
            "--redact",
            "--mask-id",
            "scope",
            "--complete-output",
            command,
            &file,
        ];
        if command == "validate-profile" {
            args.extend(["--kind", "request"]);
        }
        let (_, o) = report(&args);
        assert!(o.status.success());
        assert!(!String::from_utf8_lossy(&o.stdout).contains("private-id"));
    }
    let facts = dir.path().join("prefixprivate-id.json");
    fs::write(&facts,json!({"version":1,"records":[{"source":{"input":0,"line":1,"row_path":null},"checks":{"/status":"event"}}]}).to_string()).unwrap();
    for common in [false, true] {
        let mut args = vec![
            "--config",
            &p,
            "--redact",
            "--mask-id",
            "component_id",
            "validate-profile",
            &file,
            "--kind",
            "request",
            "--expected",
            facts.to_str().unwrap(),
        ];
        if common {
            args.extend(["--report-max-items", "1"]);
        }
        let (_, o) = report(&args);
        assert!(o.status.success());
        assert!(!String::from_utf8_lossy(&o.stdout).contains("private-id"));
    }
}

#[test]
fn validation_boundaries_preserve_mixed_source_offsets_across_host_timezones() {
    let dir = tempdir().unwrap();
    let p = profile(dir.path(), "offset-candidate", rules());
    let mut input = rows();
    input[1]["ts"] = json!("2026-01-01T01:00:01+03:00");
    let file = log(dir.path(), &input);
    for timezone in ["UTC", "Pacific/Honolulu"] {
        for extra in [
            vec![],
            vec!["--report-max-items", "10"],
            vec!["--complete-output", "--redact"],
        ] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
            for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("LOG_ANALYZER_")) {
                command.env_remove(key);
            }
            let output = command
                .env("TZ", timezone)
                .args([
                    "--config",
                    &p,
                    "validate-profile",
                    &file,
                    "--kind",
                    "request",
                ])
                .args(extra)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            let validation = &value["profile_validation"];
            let operation = &validation["operations"][0];
            assert_eq!(operation["start_time"], "2026-01-01T00:00:00+02:00");
            assert_eq!(operation["end_time"], "2026-01-01T01:00:01+03:00");
            assert_eq!(
                operation["start_time"],
                validation["records"][0]["timestamp"]
            );
            assert_eq!(operation["end_time"], validation["records"][1]["timestamp"]);
            assert_eq!(operation["duration_ms"], 1000);
            assert_eq!(
                operation["start_source"]["evidence_ref"],
                validation["records"][0]["evidence_ref"]
            );
            assert_eq!(
                operation["end_source"]["evidence_ref"],
                validation["records"][1]["evidence_ref"]
            );
            let schema: Value =
                serde_json::from_str(include_str!("../schemas/report.schema.json")).unwrap();
            assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&value));
        }
    }
}

#[test]
fn ordinal_scoped_facts_and_normalized_duplicate_witnesses_remain_distinct() {
    let dir = tempdir().unwrap();
    let p = profile(dir.path(), "ordinal-candidate", rules());
    let start = dir.path().join("start.jsonl");
    let end = dir.path().join("end.jsonl");
    let data = rows();
    fs::write(&start, data[0].to_string()).unwrap();
    fs::write(&end, data[1].to_string()).unwrap();
    let facts = dir.path().join("facts.json");
    let mut expected = json!({"version":1,"pairs":[{"start":{"input":0,"line":1,"row_path":null},"end":{"input":1,"line":1,"row_path":null},"duration_ms":1000}]});
    fs::write(&facts, expected.to_string()).unwrap();
    let args = [
        "--config",
        &p,
        "validate-profile",
        start.to_str().unwrap(),
        end.to_str().unwrap(),
        "--kind",
        "request",
        "--expected",
        facts.to_str().unwrap(),
    ];
    let (value, output) = report(&args);
    assert!(output.status.success());
    let result = &value["profile_validation"]["expected_results"][0];
    assert_eq!(result["status"], "passed");
    assert_eq!(result["start_source"]["input_ordinal"], 0);
    assert_eq!(result["end_source"]["input_ordinal"], 1);
    expected["pairs"][0]["start"]["input"] = json!(1);
    expected["pairs"][0]["end"]["input"] = json!(0);
    fs::write(&facts, expected.to_string()).unwrap();
    let (value, output) = report(&args);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        value["profile_validation"]["expected_results"][0]["status"],
        "failed"
    );

    let mut cfg = fs::read_to_string(&p).unwrap();
    cfg.push_str("\n[normalization]\nroot_path = '/rows'\nexpand_rows = true\n");
    fs::write(&p, cfg).unwrap();
    let nested = dir.path().join("nested.jsonl");
    let mut normalized_data = data.clone();
    for row in &mut normalized_data {
        row["timestamp"] = row["ts"].clone();
    }
    fs::write(&nested, json!({"rows":normalized_data}).to_string()).unwrap();
    let address = |input, row| json!({"input":input,"line":1,"row_path":format!("/rows/{row}")});
    fs::write(&facts, json!({"version":1,"records":[{"source":address(1,0),"checks":{"/semantics/phase":"start"}},{"source":address(0,1),"checks":{"/semantics/phase":"end"}}],"pairs":[{"start":address(1,0),"end":address(1,1)},{"start":address(0,0),"end":address(1,1)}]}).to_string()).unwrap();
    for options in [
        vec![],
        vec![
            "--redact",
            "--mask-id",
            "scope",
            "--report-max-items",
            "100",
        ],
    ] {
        let mut args = vec!["--config", &p];
        args.extend(options);
        args.extend([
            "validate-profile",
            nested.to_str().unwrap(),
            nested.to_str().unwrap(),
            "--kind",
            "request",
            "--expected",
            facts.to_str().unwrap(),
        ]);
        let (value, output) = report(&args);
        assert_eq!(output.status.code(), Some(1));
        let validation = &value["profile_validation"];
        assert_eq!(validation["suitability"]["status"], "conflicting");
        assert_eq!(validation["totals"]["operations"], 0);
        assert_eq!(validation["expected_results"][0]["status"], "passed");
        assert_eq!(
            validation["expected_results"][0]["source"]["input_ordinal"],
            1
        );
        assert_eq!(
            validation["expected_results"][1]["source"]["input_ordinal"],
            0
        );
        for result in &validation["expected_results"].as_array().unwrap()[2..] {
            assert_eq!(result["status"], "failed");
        }
        let witnesses = validation["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|diagnostic| diagnostic["reason"] == "ambiguous_pairing")
            .unwrap()["witnesses"]
            .as_array()
            .unwrap();
        let ordinals: std::collections::BTreeSet<_> = witnesses
            .iter()
            .map(|witness| witness["input_ordinal"].as_u64().unwrap())
            .collect();
        assert_eq!(ordinals, [0, 1].into_iter().collect());
        for witness in witnesses {
            assert!(witness["row_path"].as_str().unwrap().starts_with("/rows/"));
            assert_eq!(
                witness["evidence_ref"]["input_id"],
                value["report_metadata"]["evidence"]["inputs"]
                    [witness["input_ordinal"].as_u64().unwrap() as usize]["input_id"]
            );
        }
    }
}

#[test]
fn legacy_scope_aliasing_uses_the_effective_correlation_key() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("legacy.toml");
    fs::write(
        &p,
        r#"profile_name = "legacy-candidate"
[parser]
format = "json-lines"
request_prefix = 'Request "'
request_send_markers = ['sent']
request_receive_markers = ['done']
[perf]
correlation_scope_fields = ['session']
"#,
    )
    .unwrap();
    let mut data = Vec::new();
    for (second, component, phase) in [
        (0, "a", "sent"),
        (1, "a", "done"),
        (2, "b", "sent"),
        (3, "b", "done"),
    ] {
        data.push(json!({"ts":format!("2026-01-01T00:00:0{second}+02:00"),"component":"worker","component_id":component,"level":"INFO","session":"private-scope","message":format!("Request \"work\" [0--reused] {phase}")}));
    }
    let file = log(dir.path(), &data);
    for extra in [
        vec![],
        vec![
            "--redact",
            "--mask-id",
            "correlation_id",
            "--report-max-items",
            "100",
        ],
    ] {
        let (value, output) = run(p.to_str().unwrap(), &file, &extra);
        assert_eq!(output.status.code(), Some(1));
        let validation = &value["profile_validation"];
        assert_eq!(validation["suitability"]["status"], "insufficient_evidence");
        assert_eq!(validation["totals"]["operations"], 2);
        assert_eq!(validation["totals"]["scope_alias_groups"], 1);
        assert_eq!(validation["records"][0]["classification"]["legacy"], true);
        let diagnostic = validation["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["reason"] == "scope_adequacy_unknown")
            .unwrap();
        assert_eq!(diagnostic["scope"], json!(["private-scope"]));
        assert_eq!(diagnostic["witnesses"].as_array().unwrap().len(), 4);
        assert!(
            diagnostic["witnesses"]
                .as_array()
                .unwrap()
                .iter()
                .all(|w| w["evidence_ref"].is_object())
        );
    }
    for length in [1, data.len()] {
        let scope_file = log(dir.path(), &data[..length]);
        for mask in [
            "scope",
            "effective_scope",
            "profile_validation.records.effective_scope",
        ] {
            for options in [
                vec![],
                vec!["--report-max-items", "100"],
                vec!["--complete-output"],
            ] {
                let mut args = vec!["--redact", "--mask-id", mask];
                args.extend(options);
                let (value, output) = run(p.to_str().unwrap(), &scope_file, &args);
                assert_eq!(output.status.code(), Some(1));
                assert!(!String::from_utf8_lossy(&output.stdout).contains("private-scope"));
                assert!(value["profile_validation"]["records"][0]["effective_scope"].is_array());
            }
        }
    }
    let file = log(dir.path(), &data);
    let (value, output) = run(p.to_str().unwrap(), &file, &["--purpose", "recognition"]);
    assert!(output.status.success());
    assert_eq!(
        value["profile_validation"]["totals"]["scope_alias_groups"],
        1
    );
    for row in &mut data {
        row["session"] = row["component_id"].clone();
    }
    let file = log(dir.path(), &data);
    let (value, output) = run(p.to_str().unwrap(), &file, &[]);
    assert!(output.status.success());
    assert_eq!(
        value["profile_validation"]["totals"]["scope_alias_groups"],
        0
    );
    assert_eq!(value["profile_validation"]["totals"]["operations"], 2);
}
