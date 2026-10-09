use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::tempdir;
fn run(args: &[&str]) -> (Value, Output) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
    command.args(args);
    for (key, _) in std::env::vars().filter(|(k, _)| k.starts_with("LOG_ANALYZER")) {
        command.env_remove(key);
    }
    let output = command.output().unwrap();
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stderr)));
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/report.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<_> = validator
        .iter_errors(&value)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}\n{value}");
    (value, output)
}
fn profile(dir: &Path, name: &str, scope: &str) -> String {
    let path = dir.join(format!("{name}.toml"));
    let field = |key: &str| json!({"from":"field","field":key});
    let rules:Vec<_>=["start","end"].iter().map(|phase|json!({"id":phase,"adapter":{"type":"structured","conditions":[{"field":"phase","equals":phase}]},"mapping":{"kind":"request","name":field("operation"),"phase":{"from":"literal","value":phase},"correlation_id":field("id"),"scope":[field(scope)]}})).collect();
    fs::write(&path,toml::to_string(&json!({"extends":"base","profile_name":name,"parser":{"format":"json-lines"},"event_rules":{"version":2,"rules":rules}})).unwrap()).unwrap();
    path.to_str().unwrap().into()
}
fn fixture(dir: &Path) -> String {
    let path = dir.join("sample with spaces.jsonl");
    let rows:Vec<_>=["start","end"].iter().enumerate().map(|(i,phase)|json!({"ts":format!("2026-01-01T00:00:0{i}+02:00"),"level":"INFO","component":"worker","component_id":"a","message":"generic","phase":phase,"operation":"work","id":"shared","session":"a","tenant":"b"})).collect();
    fs::write(
        &path,
        rows.iter()
            .map(|v| v.to_string() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    path.to_str().unwrap().into()
}
fn facts(dir: &Path, scope: &str) -> String {
    let path = dir.join("expected.json");
    let records:Vec<_>=["start","end"].iter().enumerate().map(|(i,phase)|json!({"source":{"input":0,"line":i+1,"row_path":null},"checks":{"/status":"event","/semantics/kind":"request","/semantics/name":"work","/semantics/phase":phase,"/semantics/correlation_id":"shared","/semantics/scope":[scope],"/semantics/end_expected":true}})).collect();
    fs::write(&path,json!({"version":1,"records":records,"pairs":[{"start":{"input":0,"line":1,"row_path":null},"end":{"input":0,"line":2,"row_path":null},"duration_ms":1000}]}).to_string()).unwrap();
    path.to_str().unwrap().into()
}
fn candidate<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["profile_resolution"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["identity"]["name"] == name)
        .unwrap()
}
#[test]
fn no_facts_abstains_despite_supported_counts_and_explicit_choice_wins() {
    let dir = tempdir().unwrap();
    let file = fixture(dir.path());
    let p = profile(dir.path(), "candidate", "session");
    let args = [
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--candidate-config",
        &p,
    ];
    let (v, o) = run(&args);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(v["profile_resolution"]["status"], "insufficient_evidence");
    assert_eq!(
        candidate(&v, "candidate")["profile_validation"]["suitability"]["status"],
        "supported"
    );
    assert_eq!(
        candidate(&v, "candidate")["semantic_evidence"]["reason"],
        "semantic_assertions_not_supplied"
    );
    let (again, _) = run(&args);
    assert_eq!(v, again);
    let (v, o) = run(&[
        "--config",
        &p,
        "resolve-profile",
        &file,
        "--kind",
        "request",
    ]);
    assert!(o.status.success());
    assert_eq!(v["profile_resolution"]["selection_provenance"], "explicit");
    assert_eq!(v["profile_resolution"]["selected"]["name"], "candidate");
    assert_eq!(v["profile_resolution"]["candidate_activation"], false);
    let (v, o) = run(&[
        "--preset",
        "missing",
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--candidate-config",
        &p,
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(v["profile_resolution"]["status"], "invalid_explicit_choice");
    assert!(v["profile_resolution"]["selected"].is_null());
}
#[test]
fn equal_parse_counts_with_different_semantic_support_select_only_asserted_candidate() {
    let dir = tempdir().unwrap();
    let file = fixture(dir.path());
    let a = profile(dir.path(), "right", "session");
    let b = profile(dir.path(), "wrong", "tenant");
    let f = facts(dir.path(), "a");
    let (v, o) = run(&[
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--candidate-config",
        &b,
        "--candidate-config",
        &a,
        "--expected",
        &f,
    ]);
    assert!(o.status.success(), "{v}");
    assert_eq!(v["profile_resolution"]["selected"]["name"], "right");
    assert_eq!(
        candidate(&v, "right")["parsing"]["coverage"][0]["parsed_entries"],
        candidate(&v, "wrong")["parsing"]["coverage"][0]["parsed_entries"]
    );
    assert_eq!(
        candidate(&v, "wrong")["semantic_evidence"]["reason"],
        "sample_validation_not_supported"
    );
    let (reordered, _) = run(&[
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--candidate-config",
        &a,
        "--candidate-config",
        &b,
        "--expected",
        &f,
    ]);
    assert_eq!(v["profile_resolution"], reordered["profile_resolution"]); // query identities can reflect literal option order
    let second = profile(dir.path(), "also-right", "session");
    let (v, o) = run(&[
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--candidate-config",
        &a,
        "--candidate-config",
        &second,
        "--expected",
        &f,
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(v["profile_resolution"]["status"], "ambiguous");
    assert_eq!(v["profile_resolution"]["eligible_distinct_profiles"], 2);
}
#[test]
fn incomplete_unrelated_and_wrong_duration_assertions_cannot_authorize_selection() {
    let dir = tempdir().unwrap();
    let file = fixture(dir.path());
    let p = profile(dir.path(), "candidate", "session");
    let f = facts(dir.path(), "a");
    let full: Value = serde_json::from_slice(&fs::read(&f).unwrap()).unwrap();
    for (mutated, reason) in [
        (
            json!({"version":1,"records":[{"source":{"input":0,"line":1,"row_path":null},"checks":{"/profile":"candidate"}}]}),
            "requested_record_assertions_incomplete",
        ),
        (
            {
                let mut v = full.clone();
                v["records"].as_array_mut().unwrap().pop();
                v
            },
            "requested_record_assertions_incomplete",
        ),
        (
            {
                let mut v = full.clone();
                v["pairs"][0]["duration_ms"] = json!(999);
                v
            },
            "sample_validation_not_supported",
        ),
        (
            {
                let mut v = full.clone();
                v["pairs"] = json!([]);
                v
            },
            "operation_pair_or_duration_assertion_missing",
        ),
    ] {
        fs::write(&f, mutated.to_string()).unwrap();
        let (v, o) = run(&[
            "resolve-profile",
            &file,
            "--kind",
            "request",
            "--candidate-config",
            &p,
            "--expected",
            &f,
        ]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(
            candidate(&v, "candidate")["semantic_evidence"]["reason"],
            reason
        );
    }
}
#[test]
fn associations_revalidate_and_stale_digest_scope_or_contract_fall_back() {
    let dir = tempdir().unwrap();
    let file = fixture(dir.path());
    let p = profile(dir.path(), "candidate", "session");
    let f = facts(dir.path(), "a");
    let (v, _) = run(&[
        "--config",
        &p,
        "resolve-profile",
        &file,
        "--kind",
        "request",
    ]);
    let digest = v["profile_resolution"]["selected"]["sha256"].clone();
    let saved = json!({"version":1,"profile":{"config":"candidate.toml","sha256":digest},"sources":[{"file":file,"selected_parser":"json-lines"}],"event_contract":2,"structural_contract":1});
    let path = dir.path().join("association.json");
    let path = path.to_str().unwrap();
    fs::write(path, saved.to_string()).unwrap();
    let before = fs::read(path).unwrap();
    let (v, o) = run(&[
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--association",
        path,
        "--expected",
        &f,
    ]);
    assert!(o.status.success(), "{v}");
    assert_eq!(
        v["profile_resolution"]["selection_provenance"],
        "association"
    );
    assert_eq!(
        v["profile_resolution"]["association"]["status"],
        "revalidated"
    );
    assert_eq!(fs::read(path).unwrap(), before);
    for (mutated, reason) in [
        (
            {
                let mut v = saved.clone();
                v["profile"]["sha256"] = json!("0".repeat(64));
                v
            },
            "association_profile_digest_changed",
        ),
        (
            {
                let mut v = saved.clone();
                v["sources"][0]["file"] = json!("other");
                v
            },
            "association_source_scope_mismatch",
        ),
        (
            {
                let mut v = saved.clone();
                v["event_contract"] = json!(1);
                v
            },
            "association_contract_changed",
        ),
    ] {
        fs::write(path, mutated.to_string()).unwrap();
        let (v, o) = run(&[
            "resolve-profile",
            &file,
            "--kind",
            "request",
            "--association",
            path,
        ]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(v["profile_resolution"]["association"]["reason"], reason);
    }
    fs::write(path, "invalid").unwrap();
    let (v, o) = run(&[
        "--config",
        &p,
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--association",
        path,
    ]);
    assert!(o.status.success());
    assert_eq!(
        v["profile_resolution"]["association"]["status"],
        "bypassed_by_explicit_choice"
    );
}
#[test]
fn unsupported_structure_is_separate_from_unrecognized_lifecycle() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("unsupported.log");
    fs::write(&file, "2026-01-01 00:00:00 INFO Python work\ntraceback\n").unwrap();
    let (v, o) = run(&[
        "resolve-profile",
        file.to_str().unwrap(),
        "--kind",
        "request",
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(v["coverage"]["status"], "unparsed_input");
    assert!(
        v["profile_resolution"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["parsing"]["status"] == "unavailable")
    );
    let file = fixture(dir.path());
    let (v, _) = run(&["resolve-profile", &file, "--kind", "request"]);
    assert_eq!(v["coverage"]["status"], "parsed");
    assert_eq!(
        candidate(&v, "base")["profile_validation"]["suitability"]["reason"],
        "requested_lifecycle_not_recognized"
    );
}
#[test]
fn nested_candidate_redaction_covers_unmatched_assertions_rules_and_metadata() {
    let dir = tempdir().unwrap();
    let file = fixture(dir.path());
    let p = profile(dir.path(), "candidate", "session");
    let f = facts(dir.path(), "unmatched-private-scope");
    let mut cfg: toml::Value = toml::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
    let mut rule = serde_json::to_value(&cfg["event_rules"]["rules"][0]).unwrap();
    rule["id"] = json!("unused");
    rule["adapter"]["conditions"][0]["equals"] = json!("never");
    rule["mapping"]["scope"] = json!([{"from":"literal","value":"unmatched-rule-scope"}]);
    cfg["event_rules"]["rules"]
        .as_array_mut()
        .unwrap()
        .push(toml::Value::try_from(rule).unwrap());
    fs::write(&p, toml::to_string(&cfg).unwrap()).unwrap();
    let (v, o) = run(&[
        "--redact",
        "--mask-id",
        "scope",
        "--complete-output",
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--candidate-config",
        &p,
        "--expected",
        &f,
    ]);
    assert_eq!(o.status.code(), Some(1));
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(!text.contains("unmatched-private-scope"));
    assert!(!text.contains("unmatched-rule-scope"));
    let c = candidate(&v, "candidate");
    assert_eq!(c["evidence"]["redaction"]["applied"], true);
    assert_eq!(
        c["evidence"]["redaction"]["masked_id_fields"],
        json!(["scope"])
    );
    assert_eq!(c["support"]["recognition"]["status"], "supported");
    assert_eq!(
        c["profile_validation"]["suitability"]["status"],
        "unsupported"
    );
    assert!(
        c["profile_validation"]["records"][0]["timestamp"]
            .as_str()
            .unwrap()
            .ends_with("+02:00")
    );
}
#[test]
fn normalized_candidate_has_own_source_identity_and_retrievable_pages() {
    let dir = tempdir().unwrap();
    let flat = fixture(dir.path());
    let data: Vec<Value> = fs::read_to_string(&flat)
        .unwrap()
        .lines()
        .map(|l| {
            let mut v: Value = serde_json::from_str(l).unwrap();
            v["timestamp"] = v["ts"].clone();
            v
        })
        .collect();
    let file = dir.path().join("nested.jsonl");
    fs::write(&file, json!({"rows":data}).to_string()).unwrap();
    let file = file.to_str().unwrap();
    let p = profile(dir.path(), "nested", "session");
    let mut cfg = fs::read_to_string(&p).unwrap();
    cfg.push_str("\n[normalization]\nroot_path = '/rows'\nexpand_rows = true\n");
    fs::write(&p, cfg).unwrap();
    let f = facts(dir.path(), "a");
    let mut expected: Value = serde_json::from_slice(&fs::read(&f).unwrap()).unwrap();
    for (i, r) in expected["records"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        r["source"]["line"] = json!(1);
        r["source"]["row_path"] = json!(format!("/rows/{i}"));
    }
    for (i, key) in ["start", "end"].iter().enumerate() {
        expected["pairs"][0][key]["line"] = json!(1);
        expected["pairs"][0][key]["row_path"] = json!(format!("/rows/{i}"));
    }
    fs::write(&f, expected.to_string()).unwrap();
    let base = [
        "resolve-profile",
        file,
        "--kind",
        "request",
        "--candidate-config",
        &p,
        "--expected",
        &f,
    ];
    let mut args = vec!["--complete-output"];
    args.extend(base);
    let (full, o) = run(&args);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(full["coverage"]["status"], "unparsed_input");
    assert_eq!(full["profile_resolution"]["selected"]["name"], "nested");
    let c = candidate(&full, "nested");
    assert_eq!(c["identity"]["sha256"], c["evidence"]["profile_sha256"]);
    assert_eq!(c["evidence_records"].as_array().unwrap().len(), 2);
    assert_eq!(
        c["profile_validation"]["records"][0]["evidence_ref"]["row_path"],
        "/rows/0"
    );
    assert_eq!(
        c["profile_validation"]["records"][0]["evidence_ref"],
        c["evidence_records"][0]["evidence_ref"]
    );
    let mut cursor: Option<String> = None;
    let mut observed = Vec::new();
    let mut records = Vec::new();
    let mut pages = 0;
    loop {
        let mut args = vec!["--report-max-items", "20"];
        args.extend(base);
        if let Some(ref value) = cursor {
            args.extend(["--report-cursor", value]);
        }
        let (v, o) = run(&args);
        assert!(o.status.success());
        assert_eq!(
            v["profile_resolution"]["selected"],
            full["profile_resolution"]["selected"]
        );
        assert_eq!(
            v["profile_resolution"]["candidates"]
                .as_array()
                .unwrap()
                .len(),
            full["profile_resolution"]["candidates"]
                .as_array()
                .unwrap()
                .len()
        );
        let c = candidate(&v, "nested");
        observed.extend(
            c["profile_validation"]["expected_results"]
                .as_array()
                .unwrap()
                .clone(),
        );
        records.extend(c["evidence_records"].as_array().unwrap().clone());
        pages += 1;
        assert!(pages < 60);
        cursor = v["retrieval"]["next_cursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    assert!(pages > 1);
    assert_eq!(json!(observed), c["profile_validation"]["expected_results"]);
    assert_eq!(json!(records), c["evidence_records"]);
}
#[test]
fn explicit_semantic_mismatch_is_preserved_and_start_only_has_no_timing_support() {
    let dir = tempdir().unwrap();
    let file = fixture(dir.path());
    let wrong = profile(dir.path(), "wrong", "tenant");
    let right = profile(dir.path(), "right", "session");
    let f = facts(dir.path(), "a");
    let (v, o) = run(&[
        "--config",
        &wrong,
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--candidate-config",
        &right,
        "--expected",
        &f,
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        v["profile_resolution"]["status"],
        "unsupported_explicit_choice"
    );
    assert_eq!(v["profile_resolution"]["selected"]["name"], "wrong");
    assert_eq!(candidate(&v, "right")["eligible"], true);
    let mut cfg: toml::Value = toml::from_str(&fs::read_to_string(&right).unwrap()).unwrap();
    cfg["event_rules"]["rules"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    cfg["event_rules"]["rules"][0]["mapping"]
        .as_table_mut()
        .unwrap()
        .insert("end_expected".into(), toml::Value::Boolean(false));
    fs::write(&right, toml::to_string(&cfg).unwrap()).unwrap();
    let (v, o) = run(&[
        "resolve-profile",
        &file,
        "--kind",
        "request",
        "--candidate-config",
        &right,
    ]);
    assert_eq!(o.status.code(), Some(1));
    let c = candidate(&v, "right");
    assert_eq!(c["support"]["recognition"]["status"], "supported");
    assert_eq!(c["support"]["timing"]["status"], "unsupported");
    assert_eq!(
        c["support"]["timing"]["reason"],
        "intentional_start_only_has_no_timing_boundary"
    );
}
#[test]
fn candidate_payload_redaction_is_not_reversed_by_generated_metadata_restoration() {
    let dir = tempdir().unwrap();
    let file = fixture(dir.path());
    let p = profile(dir.path(), "candidate", "session");
    let data=fs::read_to_string(&file).unwrap().lines().map(|l|{let mut v:Value=serde_json::from_str(l).unwrap();v["payload"]=json!({"status":"private@example.invalid","account_id":918273,"reason":"private@example.invalid"});v.to_string()+"\n"}).collect::<String>();
    fs::write(&file, data).unwrap();
    let (v, o) = run(&[
        "--redact",
        "--mask-id",
        "account_id",
        "--mask-id",
        "status",
        "--complete-output",
        "--config",
        &p,
        "resolve-profile",
        &file,
        "--kind",
        "request",
    ]);
    assert!(o.status.success());
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(!text.contains("private@example.invalid"));
    assert!(!text.contains("918273"));
    assert_eq!(
        v["profile_resolution"]["association"]["status"],
        "not_supplied"
    );
    for c in v["profile_resolution"]["candidates"].as_array().unwrap() {
        assert_eq!(c["parsing"]["status"], "no_reported_structural_rejections");
        assert_eq!(c["identity"]["sha256"], c["evidence"]["profile_sha256"]);
    }
    assert_eq!(
        candidate(&v, "candidate")["profile_validation"]["suitability"]["status"],
        "supported"
    );
}
