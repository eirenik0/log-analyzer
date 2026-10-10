use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn line(message: &str) -> String {
    format!(
        "{}\n",
        json!({"timestamp":"2026-01-01T00:00:00+02:00","level":"INFO","component":"worker","message":message})
    )
}

fn run(directory: &Path, inputs: &[String], flags: &[&str]) -> (Value, Value) {
    let paths: Vec<_> = inputs
        .iter()
        .enumerate()
        .map(|(i, data)| {
            let path = directory.join(format!("input-{i}.jsonl"));
            fs::write(&path, data).unwrap();
            path
        })
        .collect();
    let artifact = directory.join("artifact.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
    for (name, _) in std::env::vars().filter(|(name, _)| name.starts_with("LOG_ANALYZER_")) {
        command.env_remove(name);
    }
    let output = command
        .arg("investigate")
        .args(&paths)
        .arg("--artifact")
        .arg(&artifact)
        .arg("--complete-output")
        .args(flags)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let bytes = fs::read(artifact).unwrap();
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
        jsonschema::validator_for(&schema)
            .unwrap()
            .validate(value)
            .unwrap();
    }
    log_analyzer::investigation::validate_relations(&report, Some(&bytes)).unwrap();
    (report, retained)
}

fn selection(report: &Value) -> &Value {
    &report["report_metadata"]["evidence"]["query"]["execution"]["profile_selection"]
}

#[test]
fn unique_grammars_are_detected_and_bound_to_captured_evidence() {
    for (profile, messages) in [
        (
            "service-api",
            ["Operation \"sync\" started", "Operation \"sync\" completed"],
        ),
        (
            "event-pipeline",
            ["Stage \"sync\" begin", "Stage \"sync\" done"],
        ),
    ] {
        let dir = TempDir::new().unwrap();
        let input = messages.map(line).concat();
        let (report, artifact) = run(dir.path(), std::slice::from_ref(&input), &[]);
        assert_eq!(selection(&report)["status"], "selected");
        assert_eq!(selection(&report)["profile"], profile);
        assert_eq!(report["report_metadata"]["active_profile"], profile);
        assert_eq!(artifact["effective_profile"]["profile_name"], profile);
        assert_eq!(artifact["captured_inputs"][0]["data"], input);
        assert_eq!(report["processing"]["status"], "complete");
        assert_eq!(report["processing"]["usage"]["records"], 2);
        assert_eq!(report["processing"]["usage"]["input_bytes"], input.len());
        assert_eq!(selection(&report)["probe_records"], 8);
        assert_eq!(selection(&report)["samples"][0]["entire_input"], true);
        let execution = &report["report_metadata"]["evidence"]["query"]["execution"];
        assert_eq!(execution["analysis_parse_passes"], 1);
        assert_eq!(execution["detection_parse_passes"], 4);
        assert_eq!(execution["parse_passes"], 5);
        assert!(selection(&report)["work_units"].as_u64().unwrap() > 0);
        assert!(
            report["processing"]["usage"]["work_units"]
                .as_u64()
                .unwrap()
                >= selection(&report)["work_units"].as_u64().unwrap()
        );
        assert!(
            artifact["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r.to_string().contains("success"))
        );
    }
}

#[test]
fn ambiguity_unknown_and_mixed_inputs_remain_generic() {
    let cases = [
        (
            vec![line("Command \"sync\" is called") + &line("Command \"sync\" completed")],
            "ambiguous",
        ),
        (vec![line("ordinary application record")], "no_match"),
        (
            vec![
                line("Operation \"sync\" started"),
                line("Stage \"sync\" begin"),
            ],
            "ambiguous",
        ),
        (
            vec![line("Operation \"sync\" started"), line("ordinary record")],
            "insufficient_evidence",
        ),
        (vec![line("Operation \"sync\"")], "no_match"),
        (vec![String::new()], "no_match"),
    ];
    for (inputs, status) in cases {
        let dir = TempDir::new().unwrap();
        let (report, _) = run(dir.path(), &inputs, &[]);
        assert_eq!(selection(&report)["status"], status);
        assert_eq!(report["report_metadata"]["active_profile"], "base");
        assert_eq!(report["processing"]["status"], "complete");
    }
}

#[test]
fn a_larger_match_count_does_not_resolve_overlapping_grammars() {
    let dir = TempDir::new().unwrap();
    let input = line("Stage \"sync\" begin").repeat(20) + &line("Operation \"sync\" started");
    let (report, _) = run(dir.path(), &[input], &[]);
    assert_eq!(selection(&report)["status"], "ambiguous");
    assert_eq!(selection(&report)["profile"], "base");
}

#[test]
fn every_input_is_sampled_before_selection() {
    let dir = TempDir::new().unwrap();
    let (report, _) = run(
        dir.path(),
        &[
            line("Operation \"one\" started"),
            line("Operation \"two\" completed"),
        ],
        &[],
    );
    assert_eq!(selection(&report)["status"], "selected");
    assert_eq!(selection(&report)["samples"].as_array().unwrap().len(), 2);
    assert_eq!(report["processing"]["usage"]["records"], 2);
    let matched = selection(&report)["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["profile"] == "service-api")
        .unwrap();
    assert_eq!(matched["matched_inputs"], 2);
}

#[test]
fn explicit_presets_and_configs_bypass_detection_even_when_input_disagrees() {
    for flags in [
        vec!["--preset", "base"],
        vec!["--preset", "event-pipeline"],
        vec!["--config", "examples/investigations/profile.toml"],
    ] {
        let dir = TempDir::new().unwrap();
        let (report, _) = run(dir.path(), &[line("Operation \"sync\" started")], &flags);
        assert_eq!(selection(&report)["status"], "explicit");
        let execution = &report["report_metadata"]["evidence"]["query"]["execution"];
        assert_eq!(execution["detection_parse_passes"], 0);
        assert_eq!(execution["parse_passes"], 1);
        assert_ne!(report["report_metadata"]["active_profile"], "service-api");
    }
}

#[test]
fn invalid_explicit_config_is_an_error_without_automatic_fallback() {
    let dir = TempDir::new().unwrap();
    let config = dir.path().join("bad.toml");
    fs::write(&config, "invalid = [").unwrap();
    let input = dir.path().join("log.jsonl");
    fs::write(&input, line("Operation \"sync\" started")).unwrap();
    let artifact = dir.path().join("artifact.json");
    let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .arg("--config")
        .arg(config)
        .arg("investigate")
        .arg(input)
        .arg("--artifact")
        .arg(&artifact)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!artifact.exists());
}

#[test]
fn samples_are_bounded_and_do_not_hide_unsampled_analysis_records() {
    let dir = TempDir::new().unwrap();
    let input = line("Operation \"sync\" started").repeat(128) + &line("Stage \"late\" done");
    let (report, artifact) = run(dir.path(), &[input], &[]);
    assert_eq!(selection(&report)["status"], "selected");
    assert_eq!(selection(&report)["samples"][0]["entire_input"], false);
    assert_eq!(selection(&report)["probe_records"], 128 * 4);
    assert_eq!(report["processing"]["usage"]["records"], 129);
    assert_eq!(artifact["records"].as_array().unwrap().len(), 129);
    assert!(
        selection(&report)["limitations"]
            .as_str()
            .unwrap()
            .contains("Unsampled records may differ")
    );
}

#[test]
fn oversized_utf8_line_is_not_split_or_used_as_a_signature() {
    let dir = TempDir::new().unwrap();
    let input = line(&"é".repeat(40_000)) + &line("Operation \"sync\" started");
    let (report, _) = run(dir.path(), &[input], &[]);
    assert_eq!(selection(&report)["status"], "insufficient_evidence");
    assert_eq!(selection(&report)["samples"][0]["sample_bytes"], 0);
    assert_eq!(report["processing"]["status"], "complete");
    assert_eq!(report["processing"]["usage"]["records"], 2);
}

#[test]
fn malformed_sample_cannot_authorize_a_profile() {
    let dir = TempDir::new().unwrap();
    let input =
        line("Operation \"sync\" started") + "{\"timestamp\":\"invalid\",\"message\":\"broken\"}\n";
    let (report, _) = run(dir.path(), &[input], &[]);
    assert_eq!(selection(&report)["status"], "insufficient_evidence");
    assert_eq!(selection(&report)["profile"], "base");
}

#[test]
fn detection_obeys_work_limits_and_capture_cutoffs() {
    for flags in [
        vec!["--processing-max-work", "3"],
        vec!["--input-max-bytes", "64"],
    ] {
        let dir = TempDir::new().unwrap();
        let (report, _) = run(dir.path(), &[line("Operation \"sync\" started")], &flags);
        assert_eq!(selection(&report)["status"], "budget_stopped");
        assert_eq!(selection(&report)["profile"], "base");
        assert_eq!(report["processing"]["status"], "partial");
        assert!(report["processing"]["stop"].is_object());
        assert!(
            report["processing"]["usage"]["work_units"]
                .as_u64()
                .unwrap()
                <= report["processing"]["limits"]["work_units"]
                    .as_u64()
                    .unwrap()
        );
    }
}

#[test]
fn cancelled_capture_does_not_probe_or_claim_a_selection() {
    let dir = TempDir::new().unwrap();
    let cancel = dir.path().join("cancel");
    fs::write(&cancel, "").unwrap();
    let (report, _) = run(
        dir.path(),
        &[line("Operation \"sync\" started")],
        &["--cancel-file", cancel.to_str().unwrap()],
    );
    assert_eq!(selection(&report)["status"], "budget_stopped");
    assert_eq!(selection(&report)["probe_records"], 0);
    assert_eq!(report["processing"]["stop"]["reason"], "cancelled");
}

#[test]
fn invalid_classification_prevents_selection() {
    let dir = TempDir::new().unwrap();
    let invalid = format!(
        "{}\n",
        json!({"timestamp":"2026-01-01T00:00:00Z", "level":"INFO",
        "message":"structured boundary", "operation_kind":"command", "operation_name":"sync", "operation_phase":"not-a-phase"})
    );
    let (report, _) = run(
        dir.path(),
        &[line("Operation \"sync\" started") + &invalid],
        &[],
    );
    assert_eq!(selection(&report)["status"], "insufficient_evidence");
    assert!(
        selection(&report)["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["invalid_or_conflicting_records"].as_u64().unwrap_or(0) > 0)
    );
}

#[test]
fn retained_retrieval_preserves_inference_after_source_changes() {
    let dir = TempDir::new().unwrap();
    let input = line("Operation \"sync\" started");
    let (report, _) = run(dir.path(), std::slice::from_ref(&input), &[]);
    fs::write(
        dir.path().join("input-0.jsonl"),
        line("Stage \"other\" begin"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .arg("investigation-evidence")
        .arg(dir.path().join("artifact.json"))
        .args([
            "--expected-sha256",
            report["artifact"]["stored_sha256"].as_str().unwrap(),
            "--collection",
            "/records",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let page: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(page["artifact_retrieval"]["parse_passes"], 0);
    assert!(page.to_string().contains("service-api"));
    assert!(!page.to_string().contains("other"));
}

#[test]
fn blank_inputs_do_not_prevent_detection_but_unsampled_records_still_do() {
    for blank in ["", " \t\r\n\n", "\u{2003}\u{00a0}\n"] {
        let dir = TempDir::new().unwrap();
        let (report, _) = run(
            dir.path(),
            &[
                line("Operation \"sync\" started") + &line("Operation \"sync\" completed"),
                blank.to_owned(),
            ],
            &[],
        );
        assert_eq!(selection(&report)["status"], "selected");
        assert_eq!(selection(&report)["profile"], "service-api");
        assert_eq!(report["processing"]["usage"]["records"], 2);
        assert_eq!(
            report["report_metadata"]["evidence"]["inputs"][1]["coverage"]["nonempty_lines"],
            0
        );
    }
    let dir = TempDir::new().unwrap();
    let (report, _) = run(
        dir.path(),
        &[
            line("Operation \"sync\" started"),
            "\n".repeat(128) + &line("ordinary record beyond the sample"),
        ],
        &[],
    );
    assert_eq!(selection(&report)["status"], "insufficient_evidence");
    assert_eq!(selection(&report)["samples"][1]["entire_input"], false);
}

#[test]
fn redaction_preserves_selection_status_and_coverage_without_profile_identity() {
    for (input, flags, expected) in [
        (line("Operation \"sync\" started"), vec![], "selected"),
        (line("Command \"sync\" is called"), vec![], "ambiguous"),
        (line("ordinary record"), vec![], "no_match"),
        (
            line("Operation \"sync\" started")
                + "{\"timestamp\":\"invalid\",\"message\":\"broken\"}\n",
            vec![],
            "insufficient_evidence",
        ),
        (
            line("Operation \"sync\" started"),
            vec!["--processing-max-work", "3"],
            "budget_stopped",
        ),
        (
            line("Operation \"sync\" started"),
            vec!["--profile", "service-api"],
            "explicit",
        ),
    ] {
        let original_dir = TempDir::new().unwrap();
        let (original, _) = run(original_dir.path(), std::slice::from_ref(&input), &flags);
        let dir = TempDir::new().unwrap();
        let mut flags = flags;
        flags.push("--redact");
        let (report, artifact) = run(dir.path(), &[input], &flags);
        for value in [&report, &artifact] {
            let summary = &value["report_metadata"]["profile_selection"];
            assert_eq!(summary["status"], expected);
            assert_eq!(summary, &original["report_metadata"]["profile_selection"]);
            assert!(summary.get("profile").is_none());
            assert!(summary.get("origins").is_none());
            assert_eq!(
                value["report_metadata"]["evidence"]["query"],
                json!({"command":"[REDACTED QUERY]","filter":"[REDACTED FILTER]"})
            );
            if expected != "explicit" {
                assert_eq!(summary["samples"], selection(&original)["samples"]);
                for (summary, candidate) in summary["candidates"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(selection(&original)["candidates"].as_array().unwrap())
                {
                    for key in [
                        "status",
                        "parse_passes",
                        "parse_failures",
                        "structural_loss",
                        "lifecycle_records",
                        "matched_inputs",
                        "invalid_or_conflicting_records",
                    ] {
                        assert_eq!(summary[key], candidate[key]);
                    }
                    assert!(summary.get("profile").is_none());
                    assert!(summary.get("origins").is_none());
                }
            }
        }
    }
}

#[test]
fn redaction_retains_probe_parse_failures_for_non_utf8_capture() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("input.log");
    fs::write(&input, b"\xff\n").unwrap();
    let artifact = dir.path().join("artifact.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
    for (name, _) in std::env::vars().filter(|(name, _)| name.starts_with("LOG_ANALYZER_")) {
        command.env_remove(name);
    }
    let output = command
        .current_dir(dir.path())
        .arg("investigate")
        .arg(input)
        .arg("--artifact")
        .arg(&artifact)
        .args(["--redact", "--complete-output"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let bytes = fs::read(artifact).unwrap();
    log_analyzer::investigation::validate_relations(&report, Some(&bytes)).unwrap();
    let retained: Value = serde_json::from_slice(&bytes).unwrap();
    for value in [&report, &retained] {
        let summary = &value["report_metadata"]["profile_selection"];
        let candidates = summary["candidates"].as_array().unwrap();
        assert_eq!(candidates.len(), 4);
        for candidate in candidates {
            assert_eq!(candidate["status"], "partial");
            assert_eq!(candidate["parse_failures"], 1);
            assert_eq!(candidate["structural_loss"], true);
        }
        assert!(!value.to_string().contains("input.log"));
    }
}
