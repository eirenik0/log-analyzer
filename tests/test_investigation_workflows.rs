use serde_json::Value;
use std::{fs, process::Command};
use tempfile::tempdir;

#[test]
fn maintained_investigations_execute_and_validate_every_page_and_finding() {
    let python = ["python3", "python"]
        .into_iter()
        .find(|name| {
            Command::new(name)
                .args(["-c", "import sys; sys.exit(sys.version_info < (3, 10))"])
                .output()
                .is_ok_and(|output| output.status.success())
        })
        .expect("maintained workflow checks require Python 3.10+");
    let directory = tempdir().unwrap();
    let report_path = directory.path().join("workflow-report.json");
    let output = Command::new(python)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "scripts/check-examples.py",
            env!("CARGO_BIN_EXE_log-analyzer"),
            "--report",
        ])
        .arg(&report_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&fs::read(report_path).unwrap()).unwrap();
    assert_eq!(report["kind"], "deterministic_workflow_checks");
    assert_eq!(report["workflows"].as_array().unwrap().len(), 8);
    let schemas = &report["capabilities"]["report_schemas"];
    let page_validator = jsonschema::validator_for(&schemas["report"]).unwrap();
    let findings_validator = jsonschema::validator_for(&schemas["investigation"]).unwrap();
    let capability_validator = jsonschema::validator_for(&schemas["capabilities"]).unwrap();
    assert!(capability_validator.is_valid(&report["capabilities"]));
    for workflow in report["workflows"].as_array().unwrap() {
        assert_eq!(workflow["status"], "pass");
        for call in workflow["tool_calls"].as_array().unwrap() {
            let errors: Vec<_> = page_validator
                .iter_errors(&call["report"])
                .map(|error| error.to_string())
                .collect();
            assert!(errors.is_empty(), "{}: {errors:?}", workflow["id"]);
            assert!(call["elapsed_ms"].as_f64().unwrap() >= 0.0);
            assert!(
                call["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|arg| !arg.as_str().unwrap().contains("injection-sentinel")),
                "log text became a command"
            );
        }
        for findings in workflow["investigations"].as_array().unwrap() {
            let errors: Vec<_> = findings_validator
                .iter_errors(findings)
                .map(|error| error.to_string())
                .collect();
            assert!(errors.is_empty(), "{}: {errors:?}", workflow["id"]);
        }
    }
    let comparison = report["workflows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|workflow| workflow["id"] == "slow-info-comparison")
        .unwrap();
    assert_ne!(
        comparison["investigations"][0]["input_snapshot_id"],
        comparison["investigations"][1]["input_snapshot_id"]
    );
    assert_eq!(comparison["comparison"]["elapsed_difference_ms"], 7000);
}
