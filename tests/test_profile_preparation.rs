use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tempfile::tempdir;
fn run(args: &[&str]) -> (Value, std::process::Output) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
    command.args(args);
    for (key, _) in std::env::vars().filter(|(k, _)| k.starts_with("LOG_ANALYZER")) {
        command.env_remove(key);
    }
    let output = command.output().unwrap();
    if output.stdout.is_empty() {
        return (Value::Null, output);
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stderr)));
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

fn prepare(
    root: &Path,
    input: &str,
    template: &str,
    extra: &[&str],
) -> (Value, std::process::Output) {
    let destination = root.join("prepared.toml");
    let mut args = vec![
        "prepare-profile",
        input,
        "--candidate-output",
        destination.to_str().unwrap(),
        "--template",
        template,
        "--kind",
        "request",
    ];
    args.extend_from_slice(extra);
    run(&args)
}
#[test]
fn candidate_creation_is_separate_from_independent_sample_support() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let input = fixture(root);
    let template = profile(root, "source", "session");
    let expected = facts(root, "a");
    let original = fs::read(&template).unwrap();
    let (value, output) = prepare(root, &input, &template, &["--expected", &expected]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = &value["profile_preparation"];
    assert_eq!(report["creation"]["status"], "saved");
    assert_eq!(report["candidate"]["activation"], false);
    assert_eq!(
        report["sample_validation"]["suitability"]["status"],
        "supported"
    );
    assert_eq!(
        report["semantic_proof"]["status"],
        "sufficient_on_assertion_covered_sample"
    );
    assert_eq!(fs::read(&template).unwrap(), original);
    let candidate = fs::read_to_string(root.join("prepared.toml")).unwrap();
    assert!(!candidate.contains(input.as_str()));
    assert!(!candidate.contains("# Source"));
    let config = log_analyzer::config::load_config_from_path(&root.join("prepared.toml")).unwrap();
    assert_eq!(config.profile_name, "prepared-profile");
    let (_, again) = prepare(root, &input, &template, &[]);
    assert!(!again.status.success());
    assert_eq!(
        fs::read_to_string(root.join("prepared.toml")).unwrap(),
        candidate
    );
}
#[test]
fn absent_and_partial_assertions_do_not_verify_candidate_semantics() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let input = fixture(root);
    let template = profile(root, "source", "session");
    let (value, output) = prepare(root, &input, &template, &[]);
    assert!(output.status.success());
    assert_eq!(
        value["profile_preparation"]["sample_validation"]["expected_facts"]["status"],
        "not_supplied"
    );
    assert_eq!(
        value["profile_preparation"]["semantic_proof"]["status"],
        "insufficient"
    );
    fs::remove_file(root.join("prepared.toml")).unwrap();
    let expected = facts(root, "a");
    let mut facts: Value = serde_json::from_slice(&fs::read(&expected).unwrap()).unwrap();
    facts["records"] =
        json!([{"source":{"input":0,"line":1,"row_path":null},"checks":{"/status":"event"}}]);
    facts["pairs"] = json!([]);
    fs::write(&expected, facts.to_string()).unwrap();
    let (value, output) = prepare(root, &input, &template, &["--expected", &expected]);
    assert!(output.status.success());
    assert_eq!(
        value["profile_preparation"]["sample_validation"]["expected_facts"]["status"],
        "passed"
    );
    assert_eq!(
        value["profile_preparation"]["semantic_proof"]["status"],
        "insufficient"
    );
}
#[test]
fn unsupported_python_abstains_and_partial_structure_never_claims_repair() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let input = root.join("python.log");
    fs::write(&input,"2026-01-01 00:00:00 INFO worker started\nTraceback (most recent call last):\n  generic frame\n").unwrap();
    let (value, output) = prepare(root, input.to_str().unwrap(), "base", &[]);
    assert!(output.status.success());
    assert!(value["profile_preparation"]["candidate"].is_null());
    assert!(
        !value["profile_preparation"]["presentation"]["follow_up"]
            .as_str()
            .unwrap()
            .contains("saved candidate")
    );
    assert!(!root.join("prepared.toml").exists());
    assert_eq!(
        value["profile_preparation"]["missing_information"][0]["category"],
        "unsupported_structure"
    );
    let input = fixture(root);
    let template = profile(root, "source", "session");
    let mut text = fs::read_to_string(&input).unwrap();
    text.push_str("2026-01-01 00:00:02 INFO unknown grammar\ntraceback\n");
    fs::write(&input, text).unwrap();
    let (value, output) = prepare(root, &input, &template, &[]);
    assert!(output.status.success());
    assert!(root.join("prepared.toml").exists());
    assert_eq!(value["profile_preparation"]["creation"]["status"], "saved");
    assert_eq!(
        value["profile_preparation"]["structure"]["status"],
        "unsupported"
    );
    assert_ne!(
        value["profile_preparation"]["sample_validation"]["suitability"]["status"],
        "supported"
    );
}
#[test]
fn supported_multiline_sample_preserves_parser_and_unverified_heuristics() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let input = root.join("multiline.log");
    fs::write(&input,"worker/io (session-1/task-1) | 2026-01-01T12:00:00+02:00 [INFO ] generic message\n  continuation with Unicode λ\n").unwrap();
    let (value, output) = prepare(root, input.to_str().unwrap(), "base", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join("prepared.toml").exists());
    assert_eq!(
        value["profile_preparation"]["structure"]["files"][0]["structural_diagnostics"]["attached_nonempty_lines"],
        1
    );
    for heuristic in value["profile_preparation"]["provenance"]["heuristic_changes"]
        .as_array()
        .unwrap()
    {
        assert_eq!(heuristic["verified"], false);
    }
}
#[test]
fn failed_negative_assertions_and_missing_end_remain_unsupported() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let input = fixture(root);
    let template = profile(root, "source", "session");
    let expected = facts(root, "a");
    let mut facts: Value = serde_json::from_slice(&fs::read(&expected).unwrap()).unwrap();
    facts["records"][0]["checks"]["/status"] = json!("unclassified");
    fs::write(&expected, facts.to_string()).unwrap();
    let (value, output) = prepare(root, &input, &template, &["--expected", &expected]);
    assert!(output.status.success());
    assert_eq!(
        value["profile_preparation"]["sample_validation"]["expected_facts"]["status"],
        "failed"
    );
    assert!(
        value["profile_preparation"]["missing_information"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["category"] == "assertion_mismatch")
    );
    fs::remove_file(root.join("prepared.toml")).unwrap();
    let first = fs::read_to_string(&input)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_owned();
    fs::write(&input, first + "\n").unwrap();
    let (value, output) = prepare(root, &input, &template, &[]);
    assert!(output.status.success());
    assert!(
        value["profile_preparation"]["missing_information"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["category"] == "missing_boundary")
    );
    assert_ne!(
        value["profile_preparation"]["sample_validation"]["suitability"]["status"],
        "supported"
    );
}
#[test]
fn unsafe_destinations_and_cursor_replays_cannot_create_candidates() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let input = fixture(root);
    let template = profile(root, "source", "session");
    let original = fs::read(&input).unwrap();
    let (_, output) = run(&[
        "prepare-profile",
        &input,
        "--candidate-output",
        &input,
        "--kind",
        "request",
    ]);
    assert!(!output.status.success());
    assert_eq!(fs::read(&input).unwrap(), original);
    let (_, output) = prepare(root, &input, &template, &["--complete-output"]);
    assert!(!output.status.success());
    assert!(!root.join("prepared.toml").exists());
    let missing = root.join("absent/report.json");
    let (_, output) = prepare(
        root,
        &input,
        &template,
        &["--output", missing.to_str().unwrap()],
    );
    assert!(!output.status.success());
    assert!(!root.join("prepared.toml").exists());
    let output_path = root.join("prepared.toml");
    let (_, output) = prepare(
        root,
        &input,
        &template,
        &["--output", output_path.to_str().unwrap()],
    );
    assert!(!output.status.success());
    assert!(!output_path.exists());
}

#[test]
fn reused_identifiers_missing_fields_and_conflicting_rules_provide_specific_diagnostics() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let input = fixture(root);
    let template = profile(root, "source", "session");
    let rows: Vec<Value> = fs::read_to_string(&input)
        .unwrap()
        .lines()
        .enumerate()
        .map(|(i, line)| {
            let mut row: Value = serde_json::from_str(line).unwrap();
            row["component_id"] = json!(format!("worker-{i}"));
            row
        })
        .collect();
    fs::write(
        &input,
        rows.iter()
            .map(|v| v.to_string() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    let (value, output) = prepare(root, &input, &template, &[]);
    assert!(output.status.success());
    assert!(
        value["profile_preparation"]["missing_information"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["category"] == "scope_ambiguity")
    );
    assert_ne!(
        value["profile_preparation"]["sample_validation"]["suitability"]["status"],
        "supported"
    );
    fs::remove_file(root.join("prepared.toml")).unwrap();
    let mut row = rows[0].clone();
    row.as_object_mut().unwrap().remove("id");
    fs::write(&input, row.to_string() + "\n").unwrap();
    let (value, output) = prepare(root, &input, &template, &[]);
    assert!(output.status.success());
    assert!(
        value["profile_preparation"]["missing_information"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["category"] == "missing_identity_field"
                && v["fields"].as_array().unwrap().contains(&json!("id")))
    );
    fs::remove_file(root.join("prepared.toml")).unwrap();
    fixture(root);
    let mut config: Value = serde_json::to_value(
        fs::read_to_string(&template)
            .unwrap()
            .parse::<toml::Value>()
            .unwrap(),
    )
    .unwrap();
    let mut rule = config["event_rules"]["rules"][0].clone();
    rule["id"] = json!("alternate");
    rule["mapping"]["name"] = json!({"from":"literal","value":"contradictory"});
    config["event_rules"]["rules"]
        .as_array_mut()
        .unwrap()
        .push(rule);
    fs::write(&template, toml::to_string(&config).unwrap()).unwrap();
    let (value, output) = prepare(root, &input, &template, &[]);
    assert!(output.status.success());
    assert_eq!(
        value["profile_preparation"]["sample_validation"]["suitability"]["status"],
        "conflicting"
    );
    assert!(
        value["profile_preparation"]["missing_information"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["category"] == "conflicting_rules")
    );
}
#[test]
fn redacted_saved_reports_keep_metadata_and_bound_unicode_witnesses() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let input = fixture(root);
    let template = profile(root, "source", "session");
    let expected = facts(root, "a");
    let mut rows: Vec<Value> = fs::read_to_string(&input)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for row in &mut rows {
        row["component_id"] = json!("private-id");
        row["operation"] = json!("🧪λ".repeat(600));
    }
    fs::write(
        &input,
        rows.iter()
            .map(|v| v.to_string() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    let mut facts: Value = serde_json::from_slice(&fs::read(&expected).unwrap()).unwrap();
    facts["records"][0]["checks"]["/semantics/name"] = json!("private-unmatched");
    fs::write(&expected, facts.to_string()).unwrap();
    let output = root.join("saved.json");
    let (value, result) = prepare(
        root,
        &input,
        &template,
        &[
            "--expected",
            &expected,
            "--witness-limit",
            "1",
            "--redact",
            "--mask-id",
            "component_id",
            "--mask-id",
            "semantics.name",
            "--mask-id",
            "status",
            "--mask-id",
            "records",
            "--output",
            output.to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = String::from_utf8_lossy(&result.stdout);
    assert!(!text.contains("private-id"));
    assert!(!text.contains("private-unmatched"));
    let saved = fs::read_to_string(output).unwrap();
    assert!(!saved.contains("private-id"));
    assert!(!saved.contains("private-unmatched"));
    assert_eq!(value["profile_preparation"]["creation"]["status"], "saved");
    assert_eq!(
        value["profile_preparation"]["report_save"]["status"],
        "succeeded"
    );
    assert!(
        value["profile_preparation"]["sample_validation"]["records"]
            .as_array()
            .unwrap()
            .len()
            <= 1
    );
    assert!(
        !value["profile_preparation"]["presentation"]["omissions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn blank_input_is_not_an_unsupported_parser_claim() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let input = root.join("empty.log");
    fs::write(&input, "\n  \n").unwrap();
    let (value, output) = prepare(root, input.to_str().unwrap(), "base", &[]);
    assert!(output.status.success());
    assert_eq!(
        value["profile_preparation"]["creation"]["status"],
        "not_created_empty_input"
    );
    assert_eq!(
        value["profile_preparation"]["structure"]["status"],
        "unverified_empty"
    );
    let follow_up = value["profile_preparation"]["presentation"]["follow_up"]
        .as_str()
        .unwrap();
    assert!(follow_up.contains("nonempty sample"));
    assert!(!follow_up.contains("saved candidate"));
    assert!(!root.join("prepared.toml").exists());
}

#[test]
fn copied_failed_assertion_is_redacted_after_main_assertion_page_is_bounded() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let input = fixture(root);
    let template = profile(root, "source", "session");
    let expected = root.join("expected.json");
    fs::write(&expected,json!({"version":1,"records":[{"source":{"input":0,"line":1,"row_path":null},"checks":{"/status":"event","/semantics/name":"private-unmatched"}}]}).to_string()).unwrap();
    let (value, output) = prepare(
        root,
        &input,
        &template,
        &[
            "--expected",
            expected.to_str().unwrap(),
            "--witness-limit",
            "1",
            "--redact",
            "--mask-id",
            "semantics.name",
        ],
    );
    assert!(output.status.success());
    let report = &value["profile_preparation"];
    assert_eq!(
        report["sample_validation"]["expected_results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let copied = &report["missing_information"][0]["witnesses"][0];
    assert_eq!(copied["status"], "failed");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("private-unmatched"));
    assert_ne!(copied["expected"], "private-unmatched");
}

#[test]
fn report_output_protects_every_filesystem_template_ancestor() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let input = fixture(root);
    let parent = profile(root, "parent", "session");
    let middle = root.join("middle.toml");
    let child = root.join("child.toml");
    fs::write(
        &middle,
        "extends = 'parent.toml'\nprofile_name = 'middle'\n",
    )
    .unwrap();
    fs::write(&child, "extends = 'middle.toml'\nprofile_name = 'child'\n").unwrap();
    let original = fs::read(&parent).unwrap();
    let (_, output) = prepare(
        root,
        &input,
        child.to_str().unwrap(),
        &["--output", &parent],
    );
    assert!(!output.status.success());
    assert!(!root.join("prepared.toml").exists());
    assert_eq!(fs::read(&parent).unwrap(), original);
    let alias = root.join("parent-alias.toml");
    fs::hard_link(&parent, &alias).unwrap();
    let (_, output) = run(&[
        "--config",
        child.to_str().unwrap(),
        "--output",
        alias.to_str().unwrap(),
        "prepare-profile",
        &input,
        "--candidate-output",
        root.join("prepared.toml").to_str().unwrap(),
        "--kind",
        "request",
    ]);
    assert!(!output.status.success());
    assert!(!root.join("prepared.toml").exists());
    assert_eq!(fs::read(&parent).unwrap(), original);
}

#[test]
fn global_source_conflicts_are_rejected_before_loading_or_candidate_creation() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let input = fixture(root);
    let destination = root.join("prepared.toml");
    let report = root.join("report.json");
    for global in ["--config", "--preset"] {
        for before in [true, false] {
            let mut args = vec!["--output", report.to_str().unwrap()];
            if before {
                args.extend([global, "missing-source"]);
            }
            args.extend([
                "prepare-profile",
                &input,
                "--template",
                "base",
                "--candidate-output",
                destination.to_str().unwrap(),
                "--kind",
                "request",
            ]);
            if !before {
                args.extend([global, "missing-source"]);
            }
            let (_, output) = run(&args);
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("--template"));
            assert!(!destination.exists());
            assert!(!report.exists());
        }
    }
}
