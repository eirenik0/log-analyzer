use serde_json::Value;
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
    command
        .current_dir(root)
        .args(args)
        .env("HOME", root)
        .env("USERPROFILE", root);
    for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("LOG_ANALYZER_")) {
        command.env_remove(key);
    }
    command.output().unwrap()
}

fn json(root: &Path, args: &[&str]) -> Value {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("profile.toml"),
        include_str!("../examples/investigations/profile.toml"),
    )
    .unwrap();
    fs::write(
        dir.path().join("input.jsonl"),
        include_str!("../examples/investigations/failure.jsonl"),
    )
    .unwrap();
    dir
}

fn help_commands(root: &Path, args: &[&str]) -> Vec<String> {
    let output = run(root, args);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    text.split("Commands:\n")
        .nth(1)
        .unwrap()
        .split("\n\n")
        .next()
        .unwrap()
        .lines()
        .map(|line| line.split_whitespace().next().unwrap().to_owned())
        .collect()
}

#[test]
fn primary_and_advanced_help_expose_distinct_navigation() {
    let dir = fixture();
    let root = dir.path();
    assert_eq!(
        help_commands(root, &["--help"]),
        [
            "investigate",
            "evidence",
            "profile",
            "capabilities",
            "compare",
            "search",
            "help"
        ]
    );
    assert_eq!(
        help_commands(root, &["profile", "--help"]),
        ["prepare", "validate", "resolve", "mappings", "help"]
    );
    let advanced = help_commands(root, &["--help-advanced"]);
    for name in [
        "info", "errors", "perf", "trace", "schema", "extract", "process",
    ] {
        assert!(advanced.iter().any(|item| item == name));
    }
    for name in [
        "diff",
        "llm-diff",
        "generate-config",
        "prepare-profile",
        "resolve-profile",
        "validate-profile",
        "profile-mappings",
    ] {
        assert!(!advanced.iter().any(|item| item == name));
        assert!(run(root, &[name, "--help"]).status.success());
    }
    // A positional filename never switches help modes.
    let missing = run(root, &["search", "--", "--help-advanced"]);
    assert!(!missing.status.success());
    assert!(!String::from_utf8_lossy(&missing.stdout).contains("Commands:"));
}

#[test]
fn grouped_profile_commands_preserve_reports_and_global_options() {
    let dir = fixture();
    let root = dir.path();
    for (old, new) in [
        ("validate-profile", "validate"),
        ("resolve-profile", "resolve"),
    ] {
        let legacy = json(
            root,
            &[
                "--profile",
                "profile.toml",
                old,
                "input.jsonl",
                "--kind",
                "request",
                "--report-max-items",
                "1",
            ],
        );
        for args in [
            vec![
                "--profile",
                "profile.toml",
                "profile",
                new,
                "input.jsonl",
                "--kind",
                "request",
                "--report-max-items",
                "1",
            ],
            vec![
                "profile",
                "--profile",
                "profile.toml",
                new,
                "input.jsonl",
                "--kind",
                "request",
                "--report-max-items",
                "1",
            ],
            vec![
                "profile",
                new,
                "input.jsonl",
                "--kind",
                "request",
                "--profile",
                "profile.toml",
                "--report-max-items",
                "1",
            ],
        ] {
            assert_eq!(json(root, &args), legacy);
        }
    }
    assert_eq!(
        json(
            root,
            &["profile", "mappings", "--project-root", ".", "inspect"]
        ),
        json(
            root,
            &["profile-mappings", "--project-root", ".", "inspect"]
        )
    );
    let conflict = run(
        root,
        &[
            "--profile",
            "profile.toml",
            "profile",
            "prepare",
            "input.jsonl",
            "--kind",
            "request",
            "--template",
            "base",
            "--candidate-output",
            "forbidden.toml",
        ],
    );
    assert!(!conflict.status.success());
    assert!(!root.join("forbidden.toml").exists());
}

#[test]
fn grouped_preparation_saves_the_same_candidate_without_activation() {
    let dir = fixture();
    let root = dir.path();
    for (prefix, output) in [
        (vec!["prepare-profile"], "old.toml"),
        (vec!["profile", "prepare"], "new.toml"),
    ] {
        let mut args = prefix;
        args.extend([
            "input.jsonl",
            "--kind",
            "request",
            "--template",
            "profile.toml",
            "--candidate-output",
            output,
        ]);
        let report = json(root, &args);
        assert_eq!(report["profile_preparation"]["creation"]["status"], "saved");
        assert_eq!(
            report["profile_preparation"]["candidate"]["activation"],
            false
        );
    }
    assert_eq!(
        fs::read(root.join("old.toml")).unwrap(),
        fs::read(root.join("new.toml")).unwrap()
    );
}

#[test]
fn evidence_alias_and_navigation_contract_match_executable_commands() {
    let dir = fixture();
    let root = dir.path();
    let report = json(
        root,
        &[
            "investigate",
            "input.jsonl",
            "--profile",
            "profile.toml",
            "--artifact",
            "artifact.json",
            "--brief",
        ],
    );
    let checksum = report["artifact"]["stored_sha256"].as_str().unwrap();
    assert_eq!(
        json(
            root,
            &["evidence", "artifact.json", "--expected-sha256", checksum]
        ),
        json(
            root,
            &[
                "investigation-evidence",
                "artifact.json",
                "--expected-sha256",
                checksum
            ]
        )
    );
    let capabilities = json(root, &["capabilities", "--summary"]);
    let primary: Vec<_> = capabilities["navigation"]["primary_commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        primary,
        help_commands(root, &["--help"])
            .into_iter()
            .filter(|name| name != "help")
            .collect::<Vec<_>>()
    );
    assert!(
        capabilities["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "investigation-evidence")
    );
    for argv in capabilities["navigation"]["profile_commands"]
        .as_array()
        .unwrap()
    {
        let mut args: Vec<_> = argv
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        args.push("--help");
        assert!(run(root, &args).status.success());
    }
    for action in report["guidance"]["next_actions"].as_array().unwrap() {
        if action["kind"] == "retrieve_findings" {
            assert_eq!(action["argv"][1], "evidence");
        }
    }
}

#[test]
fn default_results_are_json_including_specialized_and_compatibility_commands() {
    let dir = fixture();
    let root = dir.path();
    for args in [
        vec!["info", "input.jsonl"],
        vec!["search", "input.jsonl"],
        vec!["errors", "input.jsonl"],
        vec!["perf", "input.jsonl"],
        vec!["trace", "input.jsonl", "--id", "r1"],
        vec!["extract", "input.jsonl", "--field", "outcome"],
        vec!["process", "input.jsonl"],
        vec!["compare", "input.jsonl", "input.jsonl"],
        vec!["diff", "input.jsonl", "input.jsonl"],
        vec!["llm-diff", "input.jsonl", "input.jsonl"],
        vec!["schema", "input.jsonl"],
        vec!["generate-config", "input.jsonl"],
    ] {
        assert!(json(root, &args).is_object(), "{args:?}");
    }
    let text = run(root, &["--format", "text", "info", "input.jsonl"]);
    assert!(text.status.success());
    assert!(serde_json::from_slice::<Value>(&text.stdout).is_err());
    let raw = run(
        root,
        &["--format", "text", "generate-config", "input.jsonl"],
    );
    assert!(raw.status.success());
    toml::from_str::<toml::Value>(&String::from_utf8(raw.stdout).unwrap()).unwrap();
}

#[test]
fn summary_keeps_inline_findings_and_exact_continuation() {
    let dir = fixture();
    let root = dir.path();
    let report = json(
        root,
        &[
            "--summary",
            "investigate",
            "input.jsonl",
            "--profile",
            "profile.toml",
            "--artifact",
            "evidence.json",
            "--report-max-items",
            "1",
            "--output",
            "summary.json",
        ],
    );
    assert_eq!(report["brief_version"], 1);
    assert_eq!(report["findings"]["displayed"], 1);
    assert_eq!(report["upstream_completeness"], "unknown");
    assert_eq!(
        report,
        serde_json::from_slice::<Value>(&fs::read(root.join("summary.json")).unwrap()).unwrap()
    );
    let action = report["guidance"]["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["kind"] == "retrieve_findings")
        .unwrap();
    let argv: Vec<_> = action["argv"]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
        .map(|arg| arg.as_str().unwrap())
        .collect();
    let page = json(root, &argv);
    assert_eq!(page["artifact_retrieval"]["prior"], 1);
    assert_eq!(
        page["artifact_retrieval"]["artifact_sha256"],
        report["artifact"]["stored_sha256"]
    );
    assert!(
        json(root, &["--summary", "capabilities"])
            .get("report_schemas")
            .is_none()
    );
    for args in [
        vec!["--summary", "--complete-output", "info", "input.jsonl"],
        vec!["--summary", "--format", "text", "info", "input.jsonl"],
        vec!["--human", "info", "input.jsonl"],
    ] {
        assert!(!run(root, &args).status.success(), "{args:?}");
    }
}

#[test]
fn summary_is_json_with_explicit_omissions_and_unparsed_input_stays_unavailable() {
    let dir = fixture();
    let root = dir.path();
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/command-summary.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    for args in [
        vec!["profile", "mappings", "inspect", "--summary"],
        vec!["schema", "input.jsonl", "--summary"],
        vec!["generate-config", "input.jsonl", "--summary"],
    ] {
        let report = json(root, &args);
        validator.validate(&report).unwrap();
        assert!(report["report"].is_object());
    }
    for name in ["info", "search", "errors", "perf", "process"] {
        let report = json(root, &[name, "input.jsonl", "--summary"]);
        assert!(report["report_metadata"].is_object());
        assert!(report["retrieval"].is_object(), "{name}: {report}");
    }
    fs::write(root.join("unparsed.log"), "unsupported physical record\n").unwrap();
    let output = run(root, &["info", "unparsed.log", "--summary"]);
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["coverage"]["status"], "unparsed_input");
}

#[test]
fn summary_pages_recover_all_records_with_stable_coverage_and_saved_json() {
    let dir = fixture();
    let root = dir.path();
    let lines: String = (0..12).map(|index| serde_json::json!({"timestamp":format!("2026-01-01T00:00:{index:02}Z"),"message":format!("sample {index}")}).to_string() + "\n").collect();
    fs::write(root.join("many.jsonl"), lines).unwrap();
    let mut cursor: Option<String> = None;
    let mut references = std::collections::HashSet::new();
    let mut prior = 0;
    for page_number in 0..20 {
        let mut args = vec!["info", "many.jsonl", "--summary"];
        if let Some(cursor) = &cursor {
            args.extend(["--report-cursor", cursor]);
        }
        if page_number == 0 {
            args.extend(["--output", "page.json"]);
        }
        let report = json(root, &args);
        assert_eq!(report["coverage"]["parsed_entries"], 12);
        assert_eq!(report["retrieval"]["prior_items"], prior);
        let displayed = report["retrieval"]["displayed_items"].as_u64().unwrap();
        assert!((1..=5).contains(&displayed));
        prior += displayed;
        if page_number == 0 {
            let saved: Value =
                serde_json::from_slice(&fs::read(root.join("page.json")).unwrap()).unwrap();
            assert_eq!(saved, report);
        }
        for record in report["evidence_records"].as_array().unwrap() {
            assert!(
                references.insert(
                    record["evidence_ref"]["reference_id"]
                        .as_str()
                        .unwrap()
                        .to_owned()
                )
            );
        }
        cursor = report["retrieval"]["next_cursor"]
            .as_str()
            .map(str::to_owned);
        if cursor.is_none() {
            assert_eq!(report["retrieval"]["total_items"], prior);
            assert_eq!(references.len(), 12);
            return;
        }
    }
    panic!("Summary pagination did not terminate");
}
