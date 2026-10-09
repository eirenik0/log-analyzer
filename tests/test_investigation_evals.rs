use serde_json::Value;
use std::{fs, process::Command};
use tempfile::tempdir;

#[test]
fn credential_free_corpus_and_investigation_scores_match_advertised_contracts() {
    let python = ["python3", "python"]
        .into_iter()
        .find(|name| {
            Command::new(name)
                .args(["-c", "import sys; sys.exit(sys.version_info < (3, 10))"])
                .output()
                .is_ok_and(|output| output.status.success())
        })
        .expect("evaluation checks require Python 3.10+");
    let directory = tempdir().unwrap();
    for (script, file, expected) in [
        ("evals/run.py", "cli.json", 28),
        ("evals/agents.py", "agents.json", 26),
    ] {
        let report_path = directory.path().join(file);
        let output = Command::new(python)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args([
                script,
                "--binary",
                env!("CARGO_BIN_EXE_log-analyzer"),
                "--report",
            ])
            .arg(&report_path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{script}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&fs::read(report_path).unwrap()).unwrap();
        assert_eq!(report["counts"]["PASS"], expected);
        if script == "evals/agents.py" {
            assert_eq!(report["kind"], "deterministic_harness_baseline");
            assert!(report["model"].is_null());
            let findings =
                jsonschema::validator_for(&report["report_schemas"]["investigation"]).unwrap();
            let pages = jsonschema::validator_for(&report["report_schemas"]["report"]).unwrap();
            for run in report["records"].as_array().unwrap() {
                assert_eq!(run["score"]["status"], "PASS");
                assert!(run["usage"]["tokens"].is_null());
                for contract in run["investigations"].as_array().unwrap() {
                    let errors: Vec<_> = findings
                        .iter_errors(contract)
                        .map(|e| e.to_string())
                        .collect();
                    assert!(errors.is_empty(), "{}: {errors:?}", run["scenario"]);
                }
                for call in run["trace"].as_array().unwrap() {
                    if let Some(report) = call["output"].get("report") {
                        let errors: Vec<_> =
                            pages.iter_errors(report).map(|e| e.to_string()).collect();
                        assert!(errors.is_empty(), "{}: {errors:?}", run["scenario"]);
                    }
                }
            }
        }
    }
}
