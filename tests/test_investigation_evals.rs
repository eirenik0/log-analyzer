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

#[test]
fn layered_smoke_separates_tool_truth_interpretation_and_workflow() {
    let python = ["python3", "python"]
        .into_iter()
        .find(|name| {
            Command::new(name)
                .args(["-c", "import sys; sys.exit(sys.version_info < (3, 10))"])
                .output()
                .is_ok_and(|output| output.status.success())
        })
        .unwrap();
    let directory = tempdir().unwrap();
    let report_path = directory.path().join("layers.json");
    let output = Command::new(python)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "evals/layers.py",
            "--binary",
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
    assert_eq!(report["counts"]["PASS"], 98);
    assert_eq!(report["kind"], "scripted_layered_smoke");
    assert!(report["model"].is_null());
    assert_eq!(report["exclusions"].as_array().unwrap().len(), 3);
    let capabilities: Value = serde_json::from_slice(
        &Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
            .arg("capabilities")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let schemas = &capabilities["report_schemas"];
    let legacy = jsonschema::validator_for(&schemas["report"]).unwrap();
    let investigation = jsonschema::validator_for(&schemas["investigation"]).unwrap();
    let artifact = &schemas["evidence_artifact"];
    let members = jsonschema::validator_for(&serde_json::json!({"$defs":artifact["$defs"],"$ref":"#/$defs/population_members/properties/members/items"})).unwrap();
    let records = jsonschema::validator_for(
        &serde_json::json!({"$defs":artifact["$defs"],"$ref":"#/$defs/retained_record"}),
    )
    .unwrap();
    let findings = jsonschema::validator_for(
        &serde_json::json!({"$defs":artifact["$defs"],"$ref":"#/$defs/finding"}),
    )
    .unwrap();
    let populations = jsonschema::validator_for(
        &serde_json::json!({"$defs":artifact["$defs"],"$ref":"#/$defs/population"}),
    )
    .unwrap();
    let mut layers = std::collections::BTreeMap::new();
    for run in report["records"].as_array().unwrap() {
        assert_eq!(run["score"]["status"], "PASS", "{}", run["scenario"]);
        assert_eq!(run["execution_status"], "completed");
        assert_eq!(run["answer_available"], true);
        assert_eq!(run["validation_status"], "passed");
        assert!(run["usage"]["tokens"].is_null());
        assert!(run["usage"]["tokens_known_total"].is_null());
        *layers
            .entry(run["layer"].as_str().unwrap())
            .or_insert(0usize) += 1;
        for call in run["trace"].as_array().unwrap() {
            let Some(page) = call.get("output").and_then(|o| o.get("report")) else {
                continue;
            };
            if let Some(retrieval) = page.get("artifact_retrieval") {
                assert_eq!(retrieval["parse_passes"], 0);
                assert_eq!(retrieval["correlation_passes"], 0);
                assert_eq!(
                    retrieval["displayed"].as_u64().unwrap(),
                    retrieval["items"].as_array().unwrap().len() as u64
                );
                let validator = match retrieval["collection"].as_str().unwrap() {
                    "/records" => &records,
                    "/findings" => &findings,
                    "/populations" => &populations,
                    _ => &members,
                };
                for item in retrieval["items"].as_array().unwrap() {
                    assert!(
                        validator.is_valid(item),
                        "{} {}",
                        run["scenario"],
                        retrieval["collection"]
                    );
                }
            } else {
                let validator = if call["request"]["tool"] == "investigate" {
                    &investigation
                } else {
                    &legacy
                };
                assert!(
                    validator.is_valid(page),
                    "{} {}",
                    run["scenario"],
                    call["request"]
                );
            }
        }
        if run["arm"] == "verified-facts" {
            assert!(run["trace"].as_array().unwrap().is_empty());
        }
    }
    assert_eq!(layers["tool_correctness"], 14);
    assert_eq!(layers["verified_facts_interpretation"], 28);
    assert_eq!(layers["end_to_end"], 56);
}

#[test]
fn evaluation_schema_compositions_agree_with_rust_validation() {
    let schema = serde_json::json!({"allOf":[{"anyOf":[{"type":"integer","minimum":2,"maximum":4},{"type":"string","minLength":2}]}],"if":{"type":"string"},"then":{"pattern":"^ok"},"else":{"const":3}});
    let validator = jsonschema::validator_for(&schema).unwrap();
    for value in [serde_json::json!(3), serde_json::json!("okay")] {
        assert!(validator.is_valid(&value));
    }
    for value in [
        serde_json::json!(1),
        serde_json::json!(2),
        serde_json::json!(4),
        serde_json::json!(5),
        serde_json::json!("o"),
        serde_json::json!("bad"),
        serde_json::json!(true),
    ] {
        assert!(!validator.is_valid(&value));
    }
}
