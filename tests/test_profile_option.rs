use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn setup() -> TempDir {
    let temp = TempDir::new().unwrap();
    fs::write(
        temp.path().join("input.jsonl"),
        include_str!("../examples/investigations/failure.jsonl"),
    )
    .unwrap();
    fs::write(
        temp.path().join("parent.toml"),
        include_str!("../examples/investigations/profile.toml"),
    )
    .unwrap();
    fs::write(
        temp.path().join("team profile.toml"),
        "extends = 'parent.toml'\n",
    )
    .unwrap();
    temp
}
fn command(root: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
    cmd.current_dir(root);
    for (name, _) in std::env::vars().filter(|(name, _)| name.starts_with("LOG_ANALYZER_")) {
        cmd.env_remove(name);
    }
    cmd
}
fn value(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn run(root: &Path, args: &[&str]) -> Value {
    value(command(root).args(args).output().unwrap())
}
fn selection(report: &Value) -> &Value {
    &report["report_metadata"]["evidence"]["query"]["execution"]["profile_selection"]
}

#[test]
fn builtin_and_file_selectors_match_legacy_results_across_analysis_commands() {
    let temp = setup();
    for (profile, alias) in [("base", "--preset"), ("team profile.toml", "--config")] {
        for args in [
            vec!["info", "input.jsonl", "--json"],
            vec!["perf", "input.jsonl", "--op-type", "request", "--json"],
            vec!["validate-profile", "input.jsonl", "--kind", "request"],
            vec!["resolve-profile", "input.jsonl", "--kind", "request"],
        ] {
            // Some profiles are unsuitable: compare the exit status and complete
            // diagnostic report as well as supported reports.
            let old = command(temp.path())
                .args([alias, profile])
                .args(&args)
                .output()
                .unwrap();
            let new = command(temp.path())
                .args(&args)
                .args(["--profile", profile])
                .output()
                .unwrap();
            assert_eq!(old.status.code(), new.status.code(), "{args:?}");
            let old: Value = serde_json::from_slice(&old.stdout).unwrap();
            let new: Value = serde_json::from_slice(&new.stdout).unwrap();
            assert_eq!(old, new, "{args:?}");
        }
    }
}

#[test]
fn explicit_investigation_bypasses_discovery_and_preserves_profile_provenance() {
    let temp = setup();
    for (index, profile, expected) in [
        (0, "base", "base"),
        (1, "team profile.toml", "investigation-example"),
    ] {
        let artifact = format!("evidence-{index}.json");
        let report = run(
            temp.path(),
            &[
                "--profile",
                profile,
                "investigate",
                "input.jsonl",
                "--artifact",
                &artifact,
                "--profiles-dir",
                "missing",
                "--complete-output",
            ],
        );
        let selected = selection(&report);
        assert_eq!(selected["status"], "explicit");
        assert_eq!(selected["profile"], expected);
        assert_eq!(
            selected["profile_sha256"],
            report["report_metadata"]["evidence"]["profile_sha256"]
        );
        assert_eq!(
            selected["origins"][0][if index == 0 { "preset" } else { "config" }],
            profile
        );
        assert_eq!(
            report["report_metadata"]["evidence"]["query"]["execution"]["detection_parse_passes"],
            0
        );
        log_analyzer::investigation::validate_relations(
            &report,
            Some(&fs::read(temp.path().join(artifact)).unwrap()),
        )
        .unwrap();
    }
}

#[test]
fn exact_builtin_names_win_and_explicit_paths_disambiguate_files() {
    let temp = setup();
    fs::write(
        temp.path().join("eyes"),
        "extends = 'base'\nprofile_name = 'local-eyes'\n",
    )
    .unwrap();
    let builtin = run(
        temp.path(),
        &["--profile", "eyes", "info", "input.jsonl", "--json"],
    );
    let file = run(
        temp.path(),
        &["--profile", "./eyes", "info", "input.jsonl", "--json"],
    );
    let alias = run(
        temp.path(),
        &["--config", "./eyes", "info", "input.jsonl", "--json"],
    );
    assert_eq!(file, alias);
    assert_ne!(
        builtin["report_metadata"]["evidence"]["profile_sha256"],
        file["report_metadata"]["evidence"]["profile_sha256"]
    );
}

#[test]
fn environment_selection_and_global_positions_are_equivalent() {
    let temp = setup();
    for profile in ["base", "team profile.toml"] {
        let before = run(
            temp.path(),
            &["--profile", profile, "info", "input.jsonl", "--json"],
        );
        let after = run(
            temp.path(),
            &["info", "input.jsonl", "--json", "--profile", profile],
        );
        let environment = value(
            command(temp.path())
                .env("LOG_ANALYZER_PROFILE", profile)
                .args(["info", "input.jsonl", "--json"])
                .output()
                .unwrap(),
        );
        assert_eq!(before, after);
        assert_eq!(before, environment);
    }
}

#[test]
fn selectors_and_preparation_templates_conflict_in_every_position() {
    let temp = setup();
    for (alias, choice) in [("--config", "team profile.toml"), ("--preset", "base")] {
        for args in [
            vec!["--profile", "base", "info", "input.jsonl", alias, choice],
            vec![alias, choice, "info", "input.jsonl", "--profile", "base"],
        ] {
            let output = command(temp.path()).args(args).output().unwrap();
            assert!(!output.status.success());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(
                error.contains("--profile") && error.contains(alias),
                "{error}"
            );
        }
    }
    for env in ["LOG_ANALYZER_CONFIG", "LOG_ANALYZER_PRESET"] {
        let output = command(temp.path())
            .env(env, "base")
            .args(["--profile", "base", "info", "input.jsonl"])
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
    for args in [
        vec![
            "--profile",
            "base",
            "prepare-profile",
            "input.jsonl",
            "--template",
            "base",
            "--kind",
            "request",
            "--candidate-output",
            "candidate.toml",
        ],
        vec![
            "prepare-profile",
            "input.jsonl",
            "--template",
            "base",
            "--kind",
            "request",
            "--candidate-output",
            "candidate.toml",
            "--profile",
            "base",
        ],
    ] {
        assert!(
            !command(temp.path())
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
        assert!(!temp.path().join("candidate.toml").exists());
    }
}

#[test]
fn invalid_explicit_profile_does_not_fall_back_or_write_artifacts() {
    let temp = setup();
    fs::write(temp.path().join("invalid.toml"), "invalid = [").unwrap();
    for profile in ["missing.toml", "invalid.toml"] {
        let output = command(temp.path())
            .args([
                "--profile",
                profile,
                "investigate",
                "input.jsonl",
                "--artifact",
                "evidence.json",
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!temp.path().join("evidence.json").exists());
    }
}

#[test]
fn unified_file_selection_protects_inherited_sources_and_redacts_paths() {
    let temp = setup();
    let original = fs::read(temp.path().join("parent.toml")).unwrap();
    let output = command(temp.path())
        .args([
            "--profile",
            "team profile.toml",
            "investigate",
            "input.jsonl",
            "--artifact",
            "evidence.json",
            "--output",
            "parent.toml",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(temp.path().join("parent.toml")).unwrap(), original);
    assert!(!temp.path().join("evidence.json").exists());
    let report = run(
        temp.path(),
        &[
            "--profile",
            "team profile.toml",
            "--redact",
            "investigate",
            "input.jsonl",
            "--artifact",
            "redacted.json",
            "--complete-output",
        ],
    );
    assert!(!report.to_string().contains("team profile.toml"));
    assert!(
        !fs::read_to_string(temp.path().join("redacted.json"))
            .unwrap()
            .contains("team profile.toml")
    );
}

#[test]
fn preparation_uses_the_same_profile_and_capabilities_advertise_compatibility() {
    let temp = setup();
    let report = run(
        temp.path(),
        &[
            "--profile",
            "team profile.toml",
            "prepare-profile",
            "input.jsonl",
            "--kind",
            "request",
            "--candidate-output",
            "candidate.toml",
        ],
    );
    let candidate = fs::read_to_string(temp.path().join("candidate.toml")).unwrap();
    assert_eq!(
        report["profile_preparation"]["provenance"]["inherited"]["profile"],
        "investigation-example"
    );
    let candidate: toml::Value = toml::from_str(&candidate).unwrap();
    let parent: toml::Value =
        toml::from_str(include_str!("../examples/investigations/profile.toml")).unwrap();
    assert_eq!(candidate["event_rules"], parent["event_rules"]);
    let caps = run(temp.path(), &["--profile", "missing.toml", "capabilities"]);
    assert_eq!(caps["profiles"]["option"], "profile");
    assert_eq!(caps["profiles"]["builtins"], caps["presets"]);
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/capabilities.schema.json")).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&caps)
        .unwrap();
}

#[test]
fn help_exposes_one_profile_selector() {
    let temp = setup();
    for args in [
        vec!["--help"],
        vec!["investigate", "--help"],
        vec!["info", "--help"],
    ] {
        let output = command(temp.path()).args(args).output().unwrap();
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("--profile <"), "{help}");
        assert!(!help.contains("--config"), "{help}");
        assert!(!help.contains("--preset"), "{help}");
    }
}
