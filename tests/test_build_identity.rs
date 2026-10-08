#[path = "../build_support.rs"]
mod build_support;
use serde_json::Value;
use std::{fs, path::Path, process::Command};
use tempfile::tempdir;

fn git(root: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .current_dir(root)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn git_identity_distinguishes_clean_dirty_and_source_archives() {
    let dir = tempdir().unwrap();
    let unknown = build_support::source_identity(dir.path());
    assert!(unknown.revision.is_none());
    assert_eq!(unknown.state, "unknown");
    assert!(unknown.git_paths.is_empty());
    git(dir.path(), &["init", "-q"]);
    fs::write(dir.path().join("source.rs"), "fn main() {}\n").unwrap();
    git(dir.path(), &["add", "source.rs"]);
    git(
        dir.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "test: fixture",
        ],
    );
    let clean = build_support::source_identity(dir.path());
    assert_eq!(clean.state, "clean");
    assert_eq!(clean.revision.as_ref().unwrap().len(), 40);
    assert!(!clean.git_paths.is_empty());
    fs::write(dir.path().join("untracked.log"), "synthetic log\n").unwrap();
    assert_eq!(build_support::source_identity(dir.path()).state, "clean");
    fs::write(
        dir.path().join("source.rs"),
        "fn main() { println!(\"demo\"); }\n",
    )
    .unwrap();
    let dirty = build_support::source_identity(dir.path());
    assert_eq!(dirty.state, "dirty");
    assert_eq!(dirty.revision, clean.revision);
    let archive = dir.path().join("archive");
    fs::create_dir(&archive).unwrap();
    let nested = build_support::source_identity(&archive);
    assert_eq!(nested.state, "unknown");
    assert!(nested.revision.is_none());
}

#[test]
fn capabilities_work_without_loading_an_invalid_profile() {
    let result = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .env("LOG_ANALYZER_PRESET", "missing-profile")
        .args(["capabilities"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert!(
        value["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "process")
    );
    assert!(
        value["presets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "eyes")
    );
    assert_eq!(value["output_formats"], serde_json::json!(["text", "json"]));
    let result = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .arg("--version")
        .output()
        .unwrap();
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.len() < 100);
    assert!(text.contains("log-analyzer"));
    assert!(text.contains("(") && text.contains(")"));
}

#[test]
fn maintained_examples_match_the_built_binary() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/synthetic.jsonl");
    let examples: Value = serde_json::from_str(include_str!("../examples/commands.json")).unwrap();
    for example in examples.as_array().unwrap() {
        let args: Vec<_> = example["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| {
                arg.as_str()
                    .unwrap()
                    .replace("{fixture}", fixture.to_str().unwrap())
            })
            .collect();
        let result = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
            .env_remove("LOG_ANALYZER_PRESET")
            .args(&args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        if example["type"] == "toml" {
            let text = String::from_utf8(result.stdout).unwrap();
            let profile: toml::Value = toml::from_str(&text).unwrap();
            assert_eq!(profile["profile_name"].as_str(), Some("example-profile"));
            assert!(text.contains("# Build: log-analyzer"));
            continue;
        }
        if example["type"] == "version" {
            let text = String::from_utf8(result.stdout).unwrap();
            assert!(text.starts_with("log-analyzer ") && text.len() < 100);
            continue;
        }
        let report: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert!(
            report
                .pointer(example["pointer"].as_str().unwrap())
                .is_some(),
            "{args:?}: {report}"
        );
        if args[0] != "capabilities" {
            assert_eq!(report["report_metadata"]["schema_version"], 1);
            assert!(report["report_metadata"]["active_profile"].is_string());
            if args.iter().any(|arg| arg == "--preset") {
                assert_eq!(report["report_metadata"]["active_profile"], "eyes");
            }
            assert_eq!(
                report["report_metadata"]["build"],
                log_analyzer::build_info::identity()
            );
        }
    }
}

#[test]
fn compact_capabilities_and_bounded_metadata_preserve_output_contracts() {
    let result = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .args(["-j", "capabilities"])
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(
        result.stdout.iter().filter(|&&byte| byte == b'\n').count(),
        1
    );
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/synthetic.jsonl");
    let dir = tempdir().unwrap();
    let saved = dir.path().join("errors.txt");
    let result = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .args([
            "--preset",
            "eyes",
            "-o",
            saved.to_str().unwrap(),
            "errors",
            fixture.to_str().unwrap(),
            "--bounded",
            "--max-output-chars",
            "0",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let text = String::from_utf8(result.stdout).unwrap();
    assert_eq!(text, fs::read_to_string(saved).unwrap());
    assert_eq!(text.matches("Build: log-analyzer").count(), 1);
    assert!(text.contains("Mandatory metadata exceeds budget"));
    assert!(text.contains("profile=eyes schema=1"));
}
