use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn profile() -> &'static str {
    include_str!("../examples/investigations/profile.toml")
}
fn rows() -> Vec<Value> {
    vec![
        json!({"timestamp":"2026-01-01T00:00:00+02:00","message":"begin","phase":"start","operation":"lookup","id":"r1","session":"one"}),
        json!({"timestamp":"2026-01-01T00:00:02+02:00","message":"finish","phase":"end","operation":"lookup","id":"r1","session":"one","outcome":"success"}),
    ]
}
fn input() -> String {
    rows().iter().map(|row| format!("{row}\n")).collect()
}
fn setup() -> TempDir {
    let temp = TempDir::new().unwrap();
    fs::create_dir(temp.path().join("config")).unwrap();
    temp
}
fn invoke(root: &Path, data: &str, artifact: &str, flags: &[&str]) -> Output {
    fs::write(root.join("input.jsonl"), data).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
    for (name, _) in std::env::vars().filter(|(name, _)| name.starts_with("LOG_ANALYZER_")) {
        cmd.env_remove(name);
    }
    cmd.current_dir(root)
        .args([
            "investigate",
            "input.jsonl",
            "--artifact",
            artifact,
            "--complete-output",
        ])
        .args(flags)
        .output()
        .unwrap()
}
fn run(root: &Path, data: &str, artifact: &str, flags: &[&str]) -> (Value, Value) {
    let output = invoke(root, data, artifact, flags);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let bytes = fs::read(root.join(artifact)).unwrap();
    log_analyzer::investigation::validate_relations(&report, Some(&bytes)).unwrap();
    let retained: Value = serde_json::from_slice(&bytes).unwrap();
    (report, retained)
}
fn selection(report: &Value) -> &Value {
    &report["report_metadata"]["evidence"]["query"]["execution"]["profile_selection"]
}
fn local_candidate(report: &Value) -> &Value {
    selection(report)["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["profile"] == "investigation-example")
        .unwrap()
}

#[test]
fn default_config_folder_is_recursive_and_tracks_inherited_sources() {
    let temp = setup();
    let root = temp.path();
    fs::create_dir(root.join("config/profiles")).unwrap();
    fs::write(root.join("shared.toml"), profile()).unwrap();
    fs::write(
        root.join("config/profiles/local.toml"),
        "extends = '../../shared.toml'\n",
    )
    .unwrap();
    let (report, artifact) = run(root, &input(), "artifact.json", &[]);
    assert_eq!(selection(&report)["status"], "selected");
    assert_eq!(selection(&report)["profile"], "investigation-example");
    assert_eq!(report["processing"]["usage"]["records"], 2);
    assert_eq!(
        artifact["effective_profile"]["profile_name"],
        "investigation-example"
    );
    assert_eq!(
        selection(&report)["origins"],
        local_candidate(&report)["origins"]
    );
    assert_eq!(
        selection(&report)["profile_sha256"],
        local_candidate(&report)["profile_sha256"]
    );
    let origins = local_candidate(&report)["origins"].as_array().unwrap();
    assert_eq!(origins.len(), 1);
    let dependencies = origins[0]["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 2);
    for dependency in dependencies {
        let bytes = fs::read(dependency["path"].as_str().unwrap()).unwrap();
        assert_eq!(dependency["sha256"], log_analyzer::evidence::digest(&bytes));
    }
    assert_eq!(
        local_candidate(&report)["profile_sha256"],
        report["report_metadata"]["evidence"]["profile_sha256"]
    );
}

#[test]
fn normalized_config_can_win_when_builtins_cannot_parse_the_input() {
    let temp = setup();
    let root = temp.path();
    fs::write(
        root.join("config/nested.toml"),
        format!(
            "{}\n[normalization]\nroot_path = '/rows'\nexpand_rows = true\n",
            profile()
        ),
    )
    .unwrap();
    let (report, artifact) = run(
        root,
        &format!("{}\n", json!({"rows":rows()})),
        "artifact.json",
        &[],
    );
    assert_eq!(selection(&report)["status"], "selected");
    assert_eq!(selection(&report)["profile"], "investigation-example");
    assert_eq!(report["processing"]["usage"]["records"], 2);
    assert_eq!(artifact["records"].as_array().unwrap().len(), 2);
    let (redacted, _) = run(
        root,
        &format!("{}\n", json!({"rows":rows()})),
        "redacted.json",
        &["--redact"],
    );
    let summary = &redacted["report_metadata"]["profile_selection"];
    assert_eq!(summary["status"], "selected");
    assert!(
        summary["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|candidate| candidate["structural_loss"] == true)
    );
    assert_eq!(summary, &report["report_metadata"]["profile_selection"]);
    assert!(
        artifact["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["kind"] == "measurement" && f["details"]["value"] == 2000)
    );
}

#[test]
fn labels_merge_origins_but_distinct_analysis_rules_remain_ambiguous() {
    let temp = setup();
    let root = temp.path();
    fs::write(root.join("config/a.toml"), profile()).unwrap();
    fs::write(root.join("config/b.toml"), profile()).unwrap();
    let (report, _) = run(root, &input(), "first.json", &[]);
    assert_eq!(selection(&report)["status"], "selected");
    assert_eq!(
        local_candidate(&report)["origins"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        report["report_metadata"]["evidence"]["query"]["execution"]["detection_parse_passes"],
        5
    );
    fs::write(
        root.join("config/b.toml"),
        profile().replace("investigation-example", "different-label"),
    )
    .unwrap();
    let (report, _) = run(root, &input(), "second.json", &[]);
    assert_eq!(selection(&report)["status"], "selected");
    assert_eq!(
        local_candidate(&report)["origins"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    fs::write(
        root.join("config/b.toml"),
        profile().replace(
            "outcome = {from = \"field\", field = \"outcome\"}",
            "outcome = {from = \"literal\", value = \"failure\"}",
        ),
    )
    .unwrap();
    let (report, _) = run(root, &input(), "third.json", &[]);
    assert_eq!(selection(&report)["status"], "ambiguous");
    assert_eq!(selection(&report)["profile"], "base");
}

#[test]
fn builtin_copies_are_deduplicated_and_config_changes_are_rediscovered() {
    let temp = setup();
    let root = temp.path();
    fs::write(
        root.join("config/service.toml"),
        include_str!("../config/templates/service-api.toml"),
    )
    .unwrap();
    let data = format!(
        "{}\n",
        json!({"timestamp":"2026-01-01T00:00:00Z","message":"Operation \"sync\" started"})
    );
    let (report, _) = run(root, &data, "first.json", &[]);
    assert_eq!(selection(&report)["status"], "selected");
    let candidate = selection(&report)["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["profile"] == "service-api")
        .unwrap();
    assert_eq!(candidate["origins"].as_array().unwrap().len(), 2);
    assert_eq!(
        report["report_metadata"]["evidence"]["query"]["execution"]["detection_parse_passes"],
        4
    );
    fs::write(
        root.join("config/service.toml"),
        "extends = 'service-api'\nprofile_name = 'modified'\n",
    )
    .unwrap();
    let (changed, _) = run(root, &data, "second.json", &[]);
    assert_eq!(selection(&changed)["status"], "selected");
    assert_eq!(selection(&changed)["origins"].as_array().unwrap().len(), 2);
}

#[test]
fn alternate_folder_replaces_default_discovery_and_explicit_overrides_bypass_it() {
    let temp = setup();
    let root = temp.path();
    fs::write(root.join("config/broken.toml"), "invalid = [").unwrap();
    fs::create_dir(root.join("profiles")).unwrap();
    fs::write(root.join("profiles/local.toml"), profile()).unwrap();
    let (report, _) = run(
        root,
        &input(),
        "folder.json",
        &["--profiles-dir", "profiles"],
    );
    assert_eq!(selection(&report)["status"], "selected");
    for (artifact, flags) in [
        ("preset.json", vec!["--preset", "base"]),
        ("config.json", vec!["--config", "profiles/local.toml"]),
    ] {
        let (report, _) = run(root, &input(), artifact, &flags);
        assert_eq!(selection(&report)["status"], "explicit");
        assert!(selection(&report).get("discovery").is_none());
    }
}

#[test]
fn bad_configs_and_discovery_limits_are_visible_and_prevent_silent_selection() {
    for case in [
        "invalid",
        "oversize",
        "too_many",
        "depth",
        "missing_parent",
        "cycle",
        "entries",
        "inherited_oversize",
        "total_bytes",
    ] {
        let temp = setup();
        let root = temp.path();
        fs::write(root.join("config/local.toml"), profile()).unwrap();
        match case {
            "invalid" => fs::write(root.join("config/bad.toml"), "not = [").unwrap(),
            "oversize" => fs::write(root.join("config/big.toml"), "#".repeat(65537)).unwrap(),
            "too_many" => {
                for i in 0..16 {
                    fs::write(
                        root.join(format!("config/extra-{i}.toml")),
                        "extends='base'\n",
                    )
                    .unwrap();
                }
            }
            "depth" => fs::create_dir_all(root.join("config/a/b/c/d/e")).unwrap(),
            "missing_parent" => {
                fs::write(root.join("config/bad.toml"), "extends='../missing.toml'\n").unwrap()
            }
            "cycle" => fs::write(root.join("config/bad.toml"), "extends='./bad.toml'\n").unwrap(),
            "entries" => {
                for i in 0..257 {
                    fs::write(root.join(format!("config/entry-{i}.txt")), "").unwrap();
                }
            }
            "inherited_oversize" => {
                fs::write(root.join("shared.toml"), "#".repeat(65537)).unwrap();
                fs::write(root.join("config/bad.toml"), "extends='../shared.toml'\n").unwrap();
            }
            "total_bytes" => {
                fs::write(
                    root.join("shared.toml"),
                    format!("#{}\n{}", "x".repeat(60_000), profile()),
                )
                .unwrap();
                for i in 0..14 {
                    fs::write(
                        root.join(format!("config/padded-{i}.toml")),
                        format!("#{}\nextends='../shared.toml'\n", "x".repeat(60_000)),
                    )
                    .unwrap();
                }
            }
            _ => unreachable!(),
        }
        let (report, _) = run(root, &input(), "artifact.json", &[]);
        assert_eq!(
            selection(&report)["status"],
            "insufficient_evidence",
            "{case}"
        );
        assert_eq!(selection(&report)["discovery"]["complete"], false);
        assert!(
            selection(&report)["discovery"]["read_bytes"]
                .as_u64()
                .unwrap()
                <= 1024 * 1024 + 1
        );
        let reason = match case {
            "oversize" | "inherited_oversize" => "configuration_file_limit",
            "total_bytes" => "configuration_total_byte_limit",
            "entries" => "directory_entry_limit",
            "too_many" => "profile_file_limit",
            "depth" => "directory_depth_limit",
            _ => "invalid_or_unreadable_configuration",
        };
        assert!(
            selection(&report)["discovery"]["diagnostics"]
                .to_string()
                .contains(reason),
            "{case}"
        );
        assert!(
            !selection(&report)["discovery"]["diagnostics"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn absent_default_folder_is_optional_but_an_explicit_missing_folder_is_reported() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (report, _) = run(root, &input(), "default.json", &[]);
    assert_eq!(selection(&report)["discovery"]["complete"], true);
    let (report, _) = run(
        root,
        &input(),
        "requested.json",
        &["--profiles-dir", "missing"],
    );
    assert_eq!(selection(&report)["status"], "insufficient_evidence");
    assert_eq!(selection(&report)["discovery"]["complete"], false);
}

#[test]
fn inherited_sources_cannot_be_overwritten_by_report_delivery() {
    let temp = setup();
    let root = temp.path();
    fs::write(
        root.join("config/local.toml"),
        "extends='../missing.toml'\n",
    )
    .unwrap();
    let output = invoke(
        root,
        &input(),
        "artifact.json",
        &["--output", "missing.toml"],
    );
    assert!(!output.status.success());
    assert!(!root.join("missing.toml").exists());
    assert!(!root.join("artifact.json").exists());
}

#[test]
fn config_paths_and_inherited_sources_are_removed_by_redaction() {
    let temp = setup();
    let root = temp.path();
    fs::write(root.join("config/private-profile.toml"), profile()).unwrap();
    let (report, artifact) = run(root, &input(), "artifact.json", &["--redact"]);
    for value in [&report, &artifact] {
        let text = value.to_string();
        assert!(!text.contains("private-profile.toml"));
        assert!(!text.contains("investigation-example"));
        assert_eq!(
            value["report_metadata"]["profile_selection"]["status"],
            "selected"
        );
        assert_eq!(
            value["report_metadata"]["profile_selection"]["discovery"]["complete"],
            true
        );
        assert!(!text.contains(root.to_str().unwrap()));
    }
}

#[cfg(unix)]
#[test]
fn symlink_directory_cycles_are_skipped_and_reported() {
    let temp = setup();
    let root = temp.path();
    fs::write(root.join("config/local.toml"), profile()).unwrap();
    std::os::unix::fs::symlink(root.join("config"), root.join("config/loop")).unwrap();
    let (report, _) = run(root, &input(), "artifact.json", &[]);
    assert_eq!(selection(&report)["status"], "selected");
    assert!(
        selection(&report)["discovery"]["diagnostics"]
            .to_string()
            .contains("symlink_directory_not_followed")
    );
}

#[test]
fn unread_profiles_remain_protected_at_discovery_limits() {
    for case in ["files", "entries", "depth", "work", "memory", "cancel"] {
        let temp = setup();
        let root = temp.path();
        let victim = if case == "depth" {
            fs::create_dir_all(root.join("config/a/b/c/d/e")).unwrap();
            "config/a/b/c/d/e/victim.toml"
        } else {
            "config/z-victim.TOML"
        };
        fs::write(root.join(victim), profile()).unwrap();
        let mut flags = vec!["--output", victim];
        match case {
            "files" => {
                for index in 0..20 {
                    fs::write(
                        root.join(format!("config/a-{index}.toml")),
                        "extends='base'\n",
                    )
                    .unwrap();
                }
            }
            "entries" => {
                for index in 0..300 {
                    fs::write(root.join(format!("config/a-{index}.txt")), "").unwrap();
                }
            }
            "work" => flags.extend(["--processing-max-work", "1"]),
            "memory" => flags.extend(["--processing-max-memory-bytes", "1"]),
            "cancel" => {
                fs::write(root.join("cancel"), "").unwrap();
                flags.extend(["--cancel-file", "cancel"]);
            }
            "depth" => (),
            _ => unreachable!(),
        }
        let output = invoke(root, &input(), "artifact.json", &flags);
        assert!(!output.status.success(), "{case}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("conflicts"),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(root.join(victim)).unwrap(),
            profile(),
            "{case}"
        );
        assert!(!root.join("artifact.json").exists(), "{case}");
    }
}

#[test]
fn redaction_retains_discovery_failure_reasons_without_paths() {
    let temp = setup();
    let root = temp.path();
    fs::write(
        root.join("config/private-broken.toml"),
        "extends='../private-missing.toml'\n",
    )
    .unwrap();
    let (report, artifact) = run(root, &input(), "artifact.json", &["--redact"]);
    for value in [&report, &artifact] {
        let summary = &value["report_metadata"]["profile_selection"];
        assert_eq!(summary["status"], "insufficient_evidence");
        assert_eq!(summary["discovery"]["complete"], false);
        assert_eq!(
            summary["discovery"]["diagnostics"],
            json!([{"reason":"invalid_or_unreadable_configuration"}])
        );
        assert!(!value.to_string().contains("private-"));
    }
}

#[cfg(unix)]
#[test]
fn encountered_and_unvisited_profile_symlinks_remain_protected() {
    let temp = setup();
    let root = temp.path();
    fs::write(root.join("private-profile.toml"), profile()).unwrap();
    std::os::unix::fs::symlink(
        root.join("private-profile.toml"),
        root.join("config/local.toml"),
    )
    .unwrap();
    let output = invoke(
        root,
        &input(),
        "first.json",
        &[
            "--processing-max-work",
            "500",
            "--output",
            "private-profile.toml",
        ],
    );
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(root.join("private-profile.toml")).unwrap(),
        profile()
    );
    assert!(!root.join("first.json").exists());
    fs::write(root.join("config/hidden.toml"), profile()).unwrap();
    std::os::unix::fs::symlink(root.join("config/hidden.toml"), root.join("alias.json")).unwrap();
    let output = invoke(
        root,
        &input(),
        "second.json",
        &["--processing-max-work", "1", "--output", "alias.json"],
    );
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(root.join("config/hidden.toml")).unwrap(),
        profile()
    );
    assert!(!root.join("second.json").exists());
}

#[test]
fn profile_destination_guard_allows_normalized_paths_outside_discovery() {
    let temp = setup();
    let root = temp.path();
    fs::write(root.join("config/local.toml"), profile()).unwrap();
    let output = invoke(
        root,
        &input(),
        "artifact.json",
        &["--output", "config/../report.toml"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join("report.toml").exists());
    assert_eq!(
        fs::read_to_string(root.join("config/local.toml")).unwrap(),
        profile()
    );
}
