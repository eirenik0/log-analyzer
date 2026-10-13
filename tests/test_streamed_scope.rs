use serde_json::{Value, json};
use std::{fs, process::Command};

#[test]
fn scope_alias_context_chunks_keep_every_witness_and_uncertainty() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("aliases.jsonl");
    let artifact = temp.path().join("evidence.json");
    let data: String = (0..300)
        .map(|index| {
            format!(
                "{}\n",
                json!({
                    "ts":"2026-01-01T00:00:00+02:00", "component":"worker",
                    "component_id":format!("worker-{}", index % 2), "message":"start",
                    "session":"run", "operation":"fetch", "id":"shared", "phase":"start"
                })
            )
        })
        .collect();
    fs::write(&source, data).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "--json",
            "--config",
            "examples/investigations/profile.toml",
            "investigate",
        ])
        .arg(&source)
        .arg("--artifact")
        .arg(&artifact)
        .args(["--complete-output", "--correlation-max-index-bytes", "1"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let bytes = fs::read(&artifact).unwrap();
    log_analyzer::investigation::validate_relations(&report, Some(&bytes)).unwrap();
    let retained: Value = serde_json::from_slice(&bytes).unwrap();
    for (value, schema) in [
        (
            &report,
            include_str!("../schemas/investigation.schema.json"),
        ),
        (
            &retained,
            include_str!("../schemas/evidence-artifact.schema.json"),
        ),
    ] {
        let schema: Value = serde_json::from_str(schema).unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();
        let errors: Vec<_> = validator
            .iter_errors(value)
            .map(|error| error.to_string())
            .collect();
        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }
    let aliases: Vec<_> = retained["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["id"].as_str().unwrap().contains("scope-adequacy"))
        .collect();
    assert_eq!(aliases.len(), 3);
    let mut lines = Vec::new();
    for alias in aliases {
        assert_eq!(alias["kind"], "unknown");
        assert!(
            alias["details"]["reason"]
                .as_str()
                .unwrap()
                .contains("of 300 witnesses")
        );
        let witnesses = alias["details"]["supporting_occurrences"]
            .as_array()
            .unwrap();
        assert!(witnesses.len() <= 128);
        assert_eq!(alias["evidence"].as_array().unwrap().len(), witnesses.len());
        lines.extend(
            witnesses
                .iter()
                .map(|item| item["evidence_ref"]["line"].as_u64().unwrap()),
        );
    }
    assert_eq!(lines, (1..=300).collect::<Vec<_>>());
    assert!(
        report["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|assessment| assessment["goal"] == "slow_operations")
            .all(|assessment| assessment["status"] != "supported")
    );
}

#[test]
fn exhausted_construction_storage_publishes_no_false_report_or_artifact() {
    let temp = tempfile::tempdir().unwrap();
    let artifact = temp.path().join("evidence.json");
    let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "--json",
            "--config",
            "examples/investigations/profile.toml",
            "investigate",
            "examples/investigations/slow.jsonl",
        ])
        .arg("--artifact")
        .arg(&artifact)
        .args(["--construction-max-disk-bytes", "1"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!artifact.exists());
}
