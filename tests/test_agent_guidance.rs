use serde_json::Value;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn command(root: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
    cmd.current_dir(root);
    for (key, _) in std::env::vars().filter(|(k, _)| k.starts_with("LOG_ANALYZER_")) {
        cmd.env_remove(key);
    }
    cmd
}
fn fixture() -> TempDir {
    let temp = TempDir::new().unwrap();
    fs::create_dir(temp.path().join("config")).unwrap();
    fs::create_dir(temp.path().join("captures")).unwrap();
    fs::write(
        temp.path().join("config/team.toml"),
        include_str!("../examples/investigations/profile.toml"),
    )
    .unwrap();
    fs::write(
        temp.path().join("captures/input.jsonl"),
        include_str!("../examples/investigations/failure.jsonl"),
    )
    .unwrap();
    temp
}
fn run(root: &Path, args: &[&str]) -> Value {
    let result = command(root).args(args).output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
fn has_action(value: &Value, kind: &str) -> bool {
    value["guidance"]["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["kind"] == kind)
}

#[test]
fn capability_summary_has_identity_and_contracts_without_schema_catalog() {
    let temp = fixture();
    let summary = run(temp.path(), &["capabilities", "--summary"]);
    let full = run(temp.path(), &["capabilities"]);
    assert!(summary.get("report_schemas").is_none());
    assert_eq!(summary["build"], full["build"]);
    assert_eq!(summary["profiles"], full["profiles"]);
    assert_eq!(summary["investigation_contracts"]["brief_version"], 1);
    assert!(summary.to_string().len() < 6000);
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/capabilities.schema.json")).unwrap();
    for value in [&summary, &full] {
        jsonschema::validator_for(&schema)
            .unwrap()
            .validate(value)
            .unwrap();
    }
}

#[test]
fn explicit_project_root_is_stable_from_nested_working_directory() {
    let temp = fixture();
    let root = temp.path();
    let nested = root.join("captures");
    let report = run(
        &nested,
        &[
            "investigate",
            "input.jsonl",
            "--project-root",
            "..",
            "--artifact",
            "brief.json",
            "--brief",
        ],
    );
    assert_eq!(report["profile"]["status"], "selected");
    assert_eq!(report["profile"]["profile"], "investigation-example");
    assert_eq!(
        report["profile"]["project_root"],
        fs::canonicalize(root).unwrap().to_str().unwrap()
    );
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/investigation-brief.schema.json")).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&report)
        .unwrap();
    assert_eq!(report["findings"]["displayed"], 0);
    assert_eq!(report["upstream_completeness"], "unknown");
    // The brief delivers no findings, so its exact first request must start at zero.
    let action = report["guidance"]["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["kind"] == "retrieve_findings")
        .unwrap();
    let argv: Vec<_> = action["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert!(!argv.contains(&"--report-cursor"));
    let page = run(root, &argv[1..]);
    assert_eq!(page["artifact_retrieval"]["prior"], 0);
    assert_eq!(
        page["artifact_retrieval"]["artifact_sha256"],
        report["artifact"]["stored_sha256"]
    );
    assert_eq!(page["artifact_retrieval"]["parse_passes"], 0);
}

#[test]
fn full_report_actions_resume_the_exact_omitted_findings_page() {
    let temp = fixture();
    let report = run(
        temp.path(),
        &[
            "investigate",
            "captures/input.jsonl",
            "--artifact",
            "full.json",
            "--report-max-items",
            "1",
        ],
    );
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/investigation.schema.json")).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&report)
        .unwrap();
    let action = report["guidance"]["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["kind"] == "retrieve_findings")
        .unwrap();
    let argv: Vec<_> = action["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    let page = run(temp.path(), &argv[1..]);
    assert_eq!(page["artifact_retrieval"]["prior"], 1);
    assert_eq!(
        page["artifact_retrieval"]["items"][0]["id"],
        "scope-0-interval-0"
    );
}

#[test]
fn discovery_failure_and_semantic_gaps_have_different_recovery_actions() {
    let temp = fixture();
    let report = run(
        temp.path(),
        &[
            "investigate",
            "captures/input.jsonl",
            "--profiles-dir",
            "missing",
            "--artifact",
            "missing.json",
            "--brief",
        ],
    );
    assert!(has_action(&report, "repair_discovery"));
    assert!(has_action(&report, "resolve_profile"));
    assert!(has_action(&report, "inspect_mappings"));
    assert_eq!(
        report["profile"]["discovery"]["diagnostics"][0]["reason"],
        "directory_unavailable"
    );
    let report = run(
        temp.path(),
        &[
            "investigate",
            "captures/input.jsonl",
            "--profile",
            "base",
            "--artifact",
            "base.json",
            "--brief",
        ],
    );
    assert!(has_action(&report, "inspect_goal_support"));
    assert!(!has_action(&report, "resolve_profile"));
    assert!(!has_action(&report, "repair_discovery"));
    assert!(
        report["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["goal"] == "failures" && a["status"] == "unsupported")
    );
}

#[test]
fn redaction_preserves_guidance_without_paths_or_profile_names() {
    let temp = fixture();
    let report = run(
        temp.path(),
        &[
            "investigate",
            "captures/input.jsonl",
            "--artifact",
            "private.json",
            "--brief",
            "--redact",
        ],
    );
    assert!(has_action(&report, "verify_terminal_evidence"));
    assert!(has_action(&report, "retrieve_findings"));
    assert!(
        report["guidance"]["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["argv"].is_null())
    );
    let text = report.to_string();
    for private in [
        "investigation-example",
        "team.toml",
        "captures/input.jsonl",
        temp.path().to_str().unwrap(),
    ] {
        assert!(!text.contains(private));
    }
    assert_eq!(report["profile"]["identity_omitted"], true);
}

#[test]
fn processing_and_artifact_limits_do_not_suggest_pagination_as_recovery() {
    let temp = fixture();
    let partial = run(
        temp.path(),
        &[
            "investigate",
            "captures/input.jsonl",
            "--artifact",
            "partial.json",
            "--processing-max-records",
            "1",
            "--brief",
        ],
    );
    assert!(has_action(&partial, "processing_stopped"));
    assert_eq!(partial["processing"]["status"], "partial");
    let missing = run(
        temp.path(),
        &[
            "investigate",
            "captures/input.jsonl",
            "--artifact",
            "missing.json",
            "--artifact-max-bytes",
            "1",
            "--brief",
        ],
    );
    assert!(has_action(&missing, "artifact_unavailable"));
    assert!(!has_action(&missing, "retrieve_findings"));
    assert!(
        missing["findings"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["kind"] == "measurement")
    );
    assert_eq!(
        missing["findings"]["displayed"],
        missing["findings"]["items"].as_array().unwrap().len()
    );
    assert!(!temp.path().join("missing.json").exists());
}

#[test]
fn brief_size_is_measured_and_an_impossible_budget_is_explicit() {
    let temp = fixture();
    let output = command(temp.path())
        .args([
            "investigate",
            "captures/input.jsonl",
            "--artifact",
            "brief.json",
            "--brief",
            "--report-max-bytes",
            "1",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let brief: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        brief["presentation"]["serialized_bytes"],
        output.stdout.len()
    );
    assert_eq!(
        brief["presentation"]["status"],
        "mandatory_metadata_over_budget"
    );
    assert!(brief["findings"]["total"].as_u64().unwrap() > 0);
    assert_eq!(brief["findings"]["displayed"], 0);
}

#[test]
fn brief_preserves_unread_inputs_as_unknown_coverage() {
    let temp = fixture();
    let report = run(
        temp.path(),
        &[
            "investigate",
            "captures/input.jsonl",
            "captures/input.jsonl",
            "--artifact",
            "stopped.json",
            "--processing-max-work",
            "0",
            "--brief",
        ],
    );
    assert_eq!(report["coverage"].as_array().unwrap().len(), 2);
    for (ordinal, input) in report["coverage"].as_array().unwrap().iter().enumerate() {
        assert_eq!(input["input_ordinal"], ordinal);
        if ordinal == 0 {
            assert_eq!(input["capture"], "prefix");
            assert_eq!(input["consumed_bytes"], 0);
            assert!(input["coverage"].is_object());
        } else {
            assert_eq!(input["capture"], "unread");
            assert!(input["coverage"].is_null());
            assert!(input["selected_entries"].is_null());
        }
    }
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/investigation-brief.schema.json")).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&report)
        .unwrap();
}
