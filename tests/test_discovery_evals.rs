use serde_json::Value;
use std::{fs, process::Command};

#[test]
fn discovery_evaluation_runs_without_injecting_a_profile() {
    let python = ["python3", "python"]
        .into_iter()
        .find(|name| {
            Command::new(name)
                .args(["-c", "import sys; sys.exit(sys.version_info < (3, 10))"])
                .output()
                .is_ok_and(|o| o.status.success())
        })
        .expect("evaluation requires Python 3.10+");
    let directory = tempfile::tempdir().unwrap();
    let report = directory.path().join("discovery.json");
    let output = Command::new(python)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "evals/discovery.py",
            "--binary",
            env!("CARGO_BIN_EXE_log-analyzer"),
            "--report",
        ])
        .arg(&report)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(report["counts"]["PASS"], 9);
    assert_eq!(report["kind"], "scripted_profile_discovery");
    for record in report["records"].as_array().unwrap() {
        assert_eq!(record["profile_injected"], false);
        assert!(record["usage"]["tokens"].is_null());
        let first = record["trace"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["request"]["tool"] == "investigate")
            .unwrap();
        assert!(first["request"].get("profile").is_none());
        let schema: Value =
            serde_json::from_str(include_str!("../schemas/investigation-brief.schema.json"))
                .unwrap();
        for call in record["trace"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["request"]["tool"] == "investigate")
        {
            jsonschema::validator_for(&schema)
                .unwrap()
                .validate(&call["output"]["report"])
                .unwrap();
        }
    }
}
