use serde_json::{Value, json};
use std::{fs, process::Command};
use tempfile::tempdir;

fn invoke(args: &[&str]) -> std::process::Output {
    invoke_exit(args, 0)
}

fn invoke_exit(args: &[&str], expected_exit: i32) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
    for (name, _) in std::env::vars().filter(|(name, _)| name.starts_with("LOG_ANALYZER_")) {
        command.env_remove(name);
    }
    let output = command.args(args).output().unwrap();
    assert!(
        output.status.code() == Some(expected_exit),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn info_guidance_agrees_in_text_json_and_bounded_pages_without_asserting_semantics() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("capture.jsonl");
    fs::write(&input, "{\"timestamp\":\"2026-01-01T00:00:00Z\",\"level\":\"INFO\",\"message\":\"generic record\"}\n").unwrap();
    let path = input.to_str().unwrap();
    let text =
        String::from_utf8(invoke(&["--preset", "base", "--color", "never", "info", path]).stdout)
            .unwrap();
    let value: Value =
        serde_json::from_slice(&invoke(&["--preset", "base", "--json", "info", path]).stdout)
            .unwrap();
    let bounded: Value = serde_json::from_slice(
        &invoke(&["--preset", "base", "--report-max-items", "1", "info", path]).stdout,
    )
    .unwrap();
    assert_eq!(value["coverage"]["status"], "parsed");
    assert_eq!(value["info"]["total_entries"], 1);
    let steps = value["info"]["next_steps"].as_array().unwrap();
    assert!(!steps.is_empty());
    assert_eq!(bounded["info"]["next_steps"], value["info"]["next_steps"]);
    for step in steps {
        assert!(text.contains(step.as_str().unwrap()), "{text}");
    }
    let guide = steps
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(guide.contains("resolve-profile"));
    assert!(guide.contains("validate-profile"));
    assert!(guide.contains("investigate"));
    assert!(guide.contains("specific remaining gap before custom parsing"));
    assert!(!text.contains("capture/semantics unknown"));
    assert!(text.contains("not assessed by structure"));
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/report.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    validator.validate(&value).unwrap();
    validator.validate(&bounded).unwrap();
    let mut malformed = value.clone();
    malformed["info"]["next_steps"] = json!([7]);
    assert!(!validator.is_valid(&malformed));
}

#[test]
fn generic_inspection_can_resolve_profiles_then_investigate_without_inventing_semantics() {
    let directory = tempdir().unwrap();
    let input = directory.path().join("capture.jsonl");
    fs::write(&input, "{\"timestamp\":\"2026-01-01T00:00:00Z\",\"level\":\"INFO\",\"message\":\"application-specific status\"}\n").unwrap();
    let path = input.to_str().unwrap();
    let resolution: Value = serde_json::from_slice(
        &invoke_exit(
            &[
                "resolve-profile",
                path,
                "--kind",
                "request",
                "--no-mappings",
            ],
            1,
        )
        .stdout,
    )
    .unwrap();
    assert_ne!(resolution["profile_resolution"]["status"], "selected");
    let artifact = directory.path().join("initial.json");
    let report: Value = serde_json::from_slice(
        &invoke(&[
            "--preset",
            "base",
            "investigate",
            path,
            "--artifact",
            artifact.to_str().unwrap(),
            "--report-max-items",
            "1",
        ])
        .stdout,
    )
    .unwrap();
    assert_eq!(report["processing"]["status"], "complete");
    assert_eq!(report["processing"]["usage"]["records"], 1);
    assert_eq!(report["report_metadata"]["active_profile"], "base");
    assert_eq!(report["artifact"]["status"], "complete");
    let assessments = report["assessments"].as_array().unwrap();
    assert!(
        assessments
            .iter()
            .any(|a| a["goal"] == "inspection" && a["status"] == "supported")
    );
    assert!(assessments.iter().any(|a| a["goal"] == "incomplete_lifecycles" && a["status"] == "insufficient_evidence"));
}

#[test]
fn capabilities_advertise_default_detection_and_explicit_overrides() {
    let caps: Value = serde_json::from_slice(&invoke(&["capabilities"]).stdout).unwrap();
    assert_eq!(
        caps["investigation_contracts"]["profile_detection"],
        json!({
            "default":true, "method":"unique_profile_lifecycle_grammar", "overrides":["profile","config","preset"],"sources":["builtin","config_directory"],"default_directory":"config","directory_option":"profiles-dir"
        })
    );
    let validator = jsonschema::validator_for(&caps["report_schemas"]["capabilities"]).unwrap();
    validator.validate(&caps).unwrap();
    let mut malformed = caps.clone();
    malformed["investigation_contracts"]["profile_detection"] = json!({"default":"yes"});
    assert!(!validator.is_valid(&malformed));
    let mut missing = caps.clone();
    missing["investigation_contracts"]
        .as_object_mut()
        .unwrap()
        .remove("profile_detection");
    assert!(!validator.is_valid(&missing));
    let mut missing = caps.clone();
    missing.as_object_mut().unwrap().remove("profiles");
    assert!(!validator.is_valid(&missing));
}
