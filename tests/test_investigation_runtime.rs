use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
}
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn run(args: &[&str]) -> Output {
    binary().current_dir(root()).args(args).output().unwrap()
}
fn success(args: &[&str]) -> Value {
    let mut json_args = vec!["--json"];
    json_args.extend_from_slice(args);
    let output = run(&json_args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn shape(value: &Value, schema: &str) {
    let schema: Value = serde_json::from_str(schema).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<_> = validator
        .iter_errors(value)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
fn check(report: &Value, artifact: &Path) {
    shape(report, include_str!("../schemas/investigation.schema.json"));
    let bytes = fs::read(artifact).ok();
    if let Some(bytes) = &bytes {
        shape(
            &serde_json::from_slice(bytes).unwrap(),
            include_str!("../schemas/evidence-artifact.schema.json"),
        );
    }
    log_analyzer::investigation::validate_relations(report, bytes.as_deref()).unwrap();
}
fn investigate(temp: &Path, extra: &[&str]) -> (Value, PathBuf) {
    let artifact = temp.join("evidence.json");
    let mut args = vec![
        "--config",
        "examples/investigations/profile.toml",
        "investigate",
        "examples/investigations/slow.jsonl",
        "--artifact",
        artifact.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    let report = success(&args);
    check(&report, &artifact);
    (report, artifact)
}
fn count(report: &Value, suffix: &str) -> u64 {
    report["populations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|population| population["id"].as_str().unwrap().ends_with(suffix))
        .unwrap()["count"]
        .as_u64()
        .unwrap()
}
#[test]
fn unified_counts_durations_and_legacy_perf_agree() {
    let temp = tempfile::tempdir().unwrap();
    let (report, artifact) = investigate(temp.path(), &["--complete-output"]);
    assert_eq!(count(&report, "physical-records"), 6);
    assert_eq!(count(&report, "paired-lifecycles"), 3);
    let durations: Vec<_> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "measurement")
        .map(|f| f["details"]["value"].as_i64().unwrap())
        .collect();
    let config = log_analyzer::config::load_config_from_path(
        &root().join("examples/investigations/profile.toml"),
    )
    .unwrap();
    let entries = log_analyzer::parser::parse_log_file_report(
        root().join("examples/investigations/slow.jsonl"),
        &config,
    )
    .unwrap()
    .entries;
    let legacy = log_analyzer::perf_analyzer::analyze_performance_with_config(
        &entries,
        &log_analyzer::comparator::LogFilter::new(),
        None,
        &config,
    );
    assert_eq!(
        durations,
        legacy
            .operations
            .iter()
            .map(|operation| operation.duration_ms)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        report.pointer("/report_metadata/evidence/query/execution/parse_passes"),
        Some(&json!(1))
    );
    assert_eq!(
        report.pointer("/report_metadata/evidence/query/execution/correlation_passes"),
        Some(&json!(1))
    );
    let retained: Value = serde_json::from_slice(&fs::read(artifact).unwrap()).unwrap();
    assert_eq!(retained["records"].as_array().unwrap().len(), 6);
    assert!(
        retained["records"]
            .as_array()
            .unwrap()
            .iter()
            .all(|record| record["timestamp_offset_source"] == "source")
    );
}
#[test]
fn limits_and_cancellation_return_valid_partial_processed_populations() {
    for extra in [
        vec!["--input-max-bytes", "700"],
        vec!["--processing-max-records", "1"],
        vec!["--record-max-bytes", "100"],
        vec!["--processing-max-work", "1"],
        vec!["--processing-max-ms", "0"],
        vec!["--processing-max-memory-bytes", "1"],
    ] {
        let temp = tempfile::tempdir().unwrap();
        let (report, _) = investigate(temp.path(), &extra);
        assert_eq!(report["processing"]["status"], "partial");
        assert_eq!(report["artifact"]["status"], "partial");
        assert!(
            report["assessments"]
                .as_array()
                .unwrap()
                .iter()
                .all(|assessment| assessment["status"] != "supported")
        );
    }
    let temp = tempfile::tempdir().unwrap();
    let cancel = temp.path().join("cancel");
    fs::write(&cancel, "").unwrap();
    let (report, _) = investigate(temp.path(), &["--cancel-file", cancel.to_str().unwrap()]);
    assert_eq!(report["processing"]["stop"]["reason"], "cancelled");
}

#[test]
fn default_memory_accounting_processes_eight_thousand_paired_records() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("synthetic.jsonl");
    let artifact = temp.path().join("evidence.json");
    let mut lines = String::new();
    for index in 0..8018 {
        let row = json!({
            "ts": format!("2026-01-01T00:00:0{}Z", index % 2),
            "level": "INFO", "component": "worker", "component_id": "synthetic",
            "session": "synthetic", "message": "synthetic boundary",
            "phase": if index % 2 == 0 { "start" } else { "end" },
            "operation": "run", "id": format!("op-{}", index / 2),
            "outcome": "success",
        });
        lines.push_str(&row.to_string());
        lines.push('\n');
    }
    fs::write(&source, lines).unwrap();
    let report = success(&[
        "--config",
        "examples/investigations/profile.toml",
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
    ]);
    assert_eq!(
        report["processing"]["limits"]["memory_bytes"]
            .as_u64()
            .unwrap(),
        fs::metadata(&source)
            .unwrap()
            .len()
            .saturating_mul(512)
            .clamp(512 * 1024 * 1024, 32 * 1024 * 1024 * 1024)
    );
    assert_eq!(report["processing"]["status"], "complete");
    assert!(report["processing"]["stop"].is_null());
    assert_eq!(report["processing"]["usage"]["records"], 8018);
    assert_eq!(count(&report, "physical-records"), 8018);
    assert_eq!(count(&report, "paired-lifecycles"), 4009);
    assert_eq!(report["artifact"]["status"], "complete");
    let retained: Value = serde_json::from_slice(&fs::read(artifact).unwrap()).unwrap();
    assert_eq!(retained["records"].as_array().unwrap().len(), 8018);
    assert_eq!(
        retained["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|finding| finding["kind"] == "measurement")
            .count(),
        4009
    );
}
#[test]
fn bounded_presentation_never_changes_retained_measurements() {
    for extra in [
        vec!["--report-max-items", "0"],
        vec!["--report-max-items", "2"],
        vec!["--report-max-bytes", "1"],
        vec!["--report-max-chars", "1"],
        vec!["--artifact-max-bytes", "1"],
    ] {
        let temp = tempfile::tempdir().unwrap();
        let (report, artifact) = investigate(temp.path(), &extra);
        if artifact.exists() {
            let retained: Value = serde_json::from_slice(&fs::read(artifact).unwrap()).unwrap();
            assert_eq!(
                retained["findings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|f| f["kind"] == "measurement")
                    .count(),
                3
            );
        } else {
            assert_eq!(report["artifact"]["status"], "unavailable");
        }
    }
}
#[test]
fn retrieval_is_snapshot_bound_and_does_not_need_sources() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.jsonl");
    fs::copy(root().join("examples/investigations/slow.jsonl"), &source).unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        "examples/investigations/profile.toml",
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
    ]);
    check(&report, &artifact);
    let hash = report["artifact"]["stored_sha256"].as_str().unwrap();
    let page = success(&[
        "investigation-evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        hash,
        "--report-max-items",
        "1",
    ]);
    let cursor = page["artifact_retrieval"]["next_cursor"].as_str().unwrap();
    fs::remove_file(&source).unwrap();
    let next = success(&[
        "investigation-evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        hash,
        "--report-cursor",
        cursor,
        "--report-max-items",
        "1",
        "--verify-sources",
    ]);
    assert_eq!(next["artifact_retrieval"]["parse_passes"], 0);
    assert_eq!(next["artifact_retrieval"]["correlation_passes"], 0);
    assert_eq!(
        next["artifact_retrieval"]["source_verification"]["inputs"][0]["status"],
        "missing"
    );
    assert_eq!(next["artifact_retrieval"]["prior"], 1);
    let changed = run(&[
        "investigation-evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        hash,
        "--collection",
        "/records",
        "--report-cursor",
        cursor,
    ]);
    assert!(!changed.status.success());
    fs::write(&artifact, "{}").unwrap();
    assert!(
        !run(&[
            "investigation-evidence",
            artifact.to_str().unwrap(),
            "--expected-sha256",
            hash
        ])
        .status
        .success()
    );
}
#[test]
fn altered_and_malformed_artifacts_fail_without_panicking() {
    let temp = tempfile::tempdir().unwrap();
    let artifact = temp.path().join("malformed.json");
    fs::write(
        &artifact,
        "{\"contract_version\":1,\"investigation_contract_version\":1}",
    )
    .unwrap();
    let hash = log_analyzer::evidence::digest(&fs::read(&artifact).unwrap());
    let output = run(&[
        "investigation-evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        &hash,
    ]);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
}
#[test]
fn redacted_artifact_omits_payload_rules_query_and_paths() {
    let temp = tempfile::tempdir().unwrap();
    let (report, artifact) = investigate(
        temp.path(),
        &["--redact", "--mask-id", "session", "--complete-output"],
    );
    let retained: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    assert_eq!(report["artifact"]["location"], "[REDACTED ARTIFACT PATH]");
    assert_eq!(retained["effective_profile"], Value::Null);
    assert!(retained["records"].as_array().unwrap().iter().all(
        |record| record["raw_text"].is_null()
            && record["message"].is_null()
            && record["fields"] == json!({})
    ));
    assert!(
        retained["captured_inputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|capture| capture["data"].is_null())
    );
    assert!(
        retained["findings"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|f| f["evidence"].as_array().unwrap())
            .all(|e| e["text"] == "[REDACTED SOURCE]")
    );
    let hash = report["artifact"]["stored_sha256"].as_str().unwrap();
    assert_eq!(
        success(&[
            "investigation-evidence",
            artifact.to_str().unwrap(),
            "--expected-sha256",
            hash,
            "--redact"
        ])["artifact_retrieval"]["parse_passes"],
        0
    );
}
#[test]
fn exact_compound_selection_preserves_boundaries_and_substring_filters() {
    let temp = tempfile::tempdir().unwrap();
    let (report, selected_artifact) = investigate(
        temp.path(),
        &[
            "--select",
            r#"{"input_ordinal":0,"kind":"request","name":"run","correlation_id":"parent","scope":["slow-run"]}"#,
            "--complete-output",
        ],
    );
    assert_eq!(count(&report, "paired-lifecycles"), 1);
    assert_eq!(
        report["report_metadata"]["evidence"]["inputs"][0]["selected_entries"],
        2
    );
    assert_eq!(
        report["report_metadata"]["evidence"]["scope"]["selected_entries"],
        2
    );
    assert_eq!(
        report["report_metadata"]["evidence"]["scope"]["parsed_entries"],
        6
    );
    assert_eq!(
        report["report_metadata"]["evidence"]["scope"]["status"],
        "parsed"
    );
    let retained: Value = serde_json::from_slice(&fs::read(selected_artifact).unwrap()).unwrap();
    assert_eq!(retained["records"].as_array().unwrap().len(), 2);
    let selected_identity = report["report_metadata"]["evidence"]["snapshot_id"].clone();
    let selected_input_id = report["report_metadata"]["evidence"]["inputs"][0]["input_id"].clone();
    assert_eq!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["kind"] == "measurement")
            .unwrap()["details"]["value"],
        8000
    );
    let temp = tempfile::tempdir().unwrap();
    let (report, _) = investigate(
        temp.path(),
        &["--filter", "operation:ru", "--complete-output"],
    );
    assert!(count(&report, "physical-records") > 0);
    let temp = tempfile::tempdir().unwrap();
    let (report, empty_artifact) =
        investigate(temp.path(), &["--select", r#"{"scope":["other-run"]}"#]);
    let retained: Value = serde_json::from_slice(&fs::read(empty_artifact).unwrap()).unwrap();
    assert!(retained["records"].as_array().unwrap().is_empty());
    assert_eq!(count(&report, "physical-records"), 0);
    assert_eq!(
        report["report_metadata"]["evidence"]["inputs"][0]["selected_entries"],
        0
    );
    assert_eq!(
        report["report_metadata"]["evidence"]["scope"]["selected_entries"],
        0
    );
    assert_eq!(
        report["report_metadata"]["evidence"]["scope"]["parsed_entries"],
        6
    );
    assert_eq!(
        report["report_metadata"]["evidence"]["scope"]["status"],
        "zero_filter_matches"
    );
    assert_eq!(
        report["report_metadata"]["evidence"]["snapshot_id"],
        selected_identity
    );
    assert_eq!(
        report["report_metadata"]["evidence"]["inputs"][0]["input_id"],
        selected_input_id
    );
    assert_eq!(
        report["scopes"][0]["semantic_coverage"]["relevant_records"],
        0
    );
    assert!(
        report["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| a["goal"] != "inspection")
            .all(|a| a["status"] == "insufficient_evidence"
                && a["reason"]
                    .as_str()
                    .unwrap()
                    .contains("zero processed records"))
    );
}
#[test]
fn inputs_are_independent_even_when_paths_and_ids_are_reused() {
    let temp = tempfile::tempdir().unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        "examples/investigations/profile.toml",
        "investigate",
        "examples/investigations/slow.jsonl",
        "examples/investigations/slow.jsonl",
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    assert_eq!(report["scopes"].as_array().unwrap().len(), 2);
    assert_eq!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["kind"] == "measurement")
            .count(),
        6
    );
}
#[test]
fn unparsed_and_generic_inputs_do_not_claim_supported_zero_lifecycles() {
    let temp = tempfile::tempdir().unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "investigate",
        "examples/investigations/unsupported.log",
        "--artifact",
        artifact.to_str().unwrap(),
    ]);
    check(&report, &artifact);
    assert_eq!(
        report["report_metadata"]["evidence"]["scope"]["status"],
        "unparsed_input"
    );
    assert!(
        report["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| a["goal"] != "inspection")
            .all(|a| a["status"] != "supported")
    );
    assert!(
        !report["populations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["entity"] == "operations")
    );
}

#[test]
fn explicit_population_identity_never_merges_reused_ids_across_scopes() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("reused.jsonl");
    let rows:Vec<_>=[("session-a",0,"start"),("session-a",1,"end"),("session-b",2,"start"),("session-b",3,"end")].into_iter().map(|(session,second,phase)|json!({"ts":format!("2026-01-01T00:00:0{second}+02:00"),"component":"worker","component_id":session,"message":phase,"session":session,"operation":"run","id":"reused","phase":phase,"outcome":"success"}).to_string()).collect();
    fs::write(&source, rows.join("\n") + "\n").unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        "examples/investigations/domain-policy.toml",
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    assert_eq!(count(&report, "policy-0"), 2);
    assert_eq!(count(&report, "policy-1"), 2);
    assert_eq!(count(&report, "policy-2"), 2);
    assert_eq!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(
                |finding| finding["id"].as_str().unwrap().contains("relationship-0-")
                    && finding["kind"] == "observation"
            )
            .count(),
        2
    );
}
#[test]
fn expanded_rows_stop_before_retaining_the_next_row_and_redact_private_paths() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("rows.jsonl");
    let profile = temp.path().join("profile.toml");
    fs::write(&profile,"extends = \"base\"\nprofile_name = \"normalization-example\"\n[normalization]\nroot_path = \"/private-sample-id\"\nexpand_rows = true\n[normalization.fields]\ntimestamp = \"/ts\"\nmessage = \"/message\"\n").unwrap();
    fs::write(&source,json!({"private-sample-id":[{"ts":"2026-01-01T00:00:00+02:00","message":"private-payload"},{"ts":"2026-01-01T00:00:01+02:00","message":"private-payload"}]}).to_string()+"\n").unwrap();
    let artifact = temp.path().join("bounded.json");
    let report = success(&[
        "--config",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--processing-max-expanded-records",
        "1",
    ]);
    check(&report, &artifact);
    assert_eq!(report["processing"]["stop"]["reason"], "expansion_limit");
    let retained: Value = serde_json::from_slice(&fs::read(artifact).unwrap()).unwrap();
    assert_eq!(retained["records"].as_array().unwrap().len(), 1);
    let artifact = temp.path().join("redacted.json");
    let report = success(&[
        "--config",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--redact",
    ]);
    check(&report, &artifact);
    let bytes = fs::read(artifact).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains("private-sample-id"));
    assert!(!text.contains("private-payload"));
}
#[test]
fn report_file_matches_serialized_budget_and_preserves_existing_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("report.json");
    let (report, artifact) = investigate(
        temp.path(),
        &[
            "--output",
            output.to_str().unwrap(),
            "--report-max-items",
            "2",
        ],
    );
    assert_eq!(
        fs::metadata(&output).unwrap().len(),
        report["presentation"]["serialized_bytes"].as_u64().unwrap()
    );
    let before = fs::read(&artifact).unwrap();
    let failed = run(&[
        "investigate",
        "examples/investigations/slow.jsonl",
        "--artifact",
        artifact.to_str().unwrap(),
    ]);
    assert!(!failed.status.success());
    assert_eq!(fs::read(&artifact).unwrap(), before);
}

#[test]
fn main_report_cursor_resumes_retained_findings_and_changed_sources_are_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.jsonl");
    fs::copy(root().join("examples/investigations/slow.jsonl"), &source).unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        "examples/investigations/profile.toml",
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--report-max-items",
        "1",
    ]);
    check(&report, &artifact);
    let hash = report["artifact"]["stored_sha256"].as_str().unwrap();
    let cursor = report["retrieval"]["next_cursor"].as_str().unwrap();
    fs::write(&source, "changed source\n").unwrap();
    let page = success(&[
        "investigation-evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        hash,
        "--report-cursor",
        cursor,
        "--verify-sources",
    ]);
    assert_eq!(page["artifact_retrieval"]["prior"], 1);
    assert_eq!(
        page["artifact_retrieval"]["source_verification"]["inputs"][0]["status"],
        "changed"
    );
    assert!(
        !run(&[
            "investigation-evidence",
            artifact.to_str().unwrap(),
            "--expected-sha256",
            hash,
            "--output",
            source.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert_eq!(fs::read_to_string(source).unwrap(), "changed source\n");
}
#[test]
fn byte_prefix_and_unread_inputs_are_retained_honestly() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("prefix.jsonl");
    let mut data = fs::read(root().join("examples/investigations/slow.jsonl")).unwrap();
    data.extend_from_slice(b"{\"message\":\"");
    data.extend_from_slice("🙂".as_bytes());
    fs::write(&source, &data).unwrap();
    let cap = (data.len() - 2).to_string();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        "examples/investigations/profile.toml",
        "investigate",
        source.to_str().unwrap(),
        "examples/investigations/slow.jsonl",
        "--artifact",
        artifact.to_str().unwrap(),
        "--input-max-bytes",
        &cap,
    ]);
    check(&report, &artifact);
    assert_eq!(report["processing"]["inputs"][0]["capture"], "prefix");
    assert_eq!(report["processing"]["inputs"][1]["capture"], "unread");
    assert_eq!(
        report["artifact"]["verification"]["source_and_rules"],
        "unavailable"
    );
}

#[test]
fn source_verification_uses_capture_directory_and_compares_consumed_prefix() {
    let temp = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let data = fs::read(root().join("examples/investigations/slow.jsonl")).unwrap();
    let source = temp.path().join("source.jsonl");
    fs::write(&source, &data).unwrap();
    let profile = root().join("examples/investigations/profile.toml");
    for prefix in [false, true] {
        fs::write(&source, &data).unwrap();
        let artifact = temp
            .path()
            .join(if prefix { "prefix.json" } else { "full.json" });
        let mut cmd = binary();
        cmd.current_dir(temp.path()).args([
            "--json",
            "--config",
            profile.to_str().unwrap(),
            "investigate",
            "source.jsonl",
            "--artifact",
            artifact.to_str().unwrap(),
        ]);
        if prefix {
            cmd.args(["--json", "--input-max-bytes", "128"]);
        }
        let output = cmd.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        check(&report, &artifact);
        let hash = report["artifact"]["stored_sha256"].as_str().unwrap();
        let verify = || {
            let output = binary()
                .current_dir(other.path())
                .args([
                    "--json",
                    "investigation-evidence",
                    artifact.to_str().unwrap(),
                    "--expected-sha256",
                    hash,
                    "--verify-sources",
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let page: Value = serde_json::from_slice(&output.stdout).unwrap();
            page["artifact_retrieval"]["source_verification"]["inputs"][0].clone()
        };
        assert_eq!(
            verify()["status"],
            if prefix {
                "prefix_unchanged"
            } else {
                "unchanged"
            }
        );
        if prefix {
            let mut changed = data.clone();
            changed[200] ^= 1;
            fs::write(&source, &changed).unwrap();
            assert_eq!(verify()["status"], "prefix_unchanged");
            changed[0] ^= 1;
            fs::write(&source, &changed).unwrap();
            assert_eq!(verify()["status"], "prefix_changed");
            fs::write(&source, &data[..64]).unwrap();
            let state = verify();
            assert_eq!(state["status"], "prefix_shortened");
            assert_eq!(state["extent"], "consumed_prefix");
            assert_eq!(state["verified_bytes"], 64);
        }
    }
}

#[test]
fn declared_domain_observations_keep_attempts_polls_cached_failures_and_work_separate() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("domain.toml");
    let mut config = format!(
        "extends = {:?}\nprofile_name = \"synthetic-domain\"\n[investigation]\nversion = 1\n",
        root()
            .join("examples/investigations/profile.toml")
            .to_str()
            .unwrap()
    );
    for role in [
        "screenshot-start",
        "poll-send",
        "poll-response",
        "cached-failure",
        "downstream",
    ] {
        config += &format!("[[investigation.roles]]\nid = {role:?}\nrule_ids = [{role:?}]\n");
    }
    for (id, entity, role, field, grouping) in [
        (
            "attempts",
            "attempts",
            "screenshot-start",
            "attempt",
            "identity",
        ),
        (
            "resources",
            "resources",
            "screenshot-start",
            "resource",
            "identity",
        ),
        (
            "starts",
            "events",
            "screenshot-start",
            "attempt",
            "occurrence",
        ),
        ("polls", "events", "poll-send", "poll", "occurrence"),
        ("responses", "events", "poll-response", "poll", "occurrence"),
        (
            "cached",
            "events",
            "cached-failure",
            "resource",
            "occurrence",
        ),
        ("work", "events", "downstream", "work", "occurrence"),
    ] {
        config += &format!(
            "[[investigation.populations]]\nid = {id:?}\nentity = {entity:?}\nroles = [{role:?}]\nidentity_fields = [{field:?}]\ngrouping = {grouping:?}\n"
        );
    }
    config += "[[investigation.relationships]]\nid = \"poll-observation\"\nsource_role = \"poll-send\"\ntarget_role = \"poll-response\"\njoin_fields = [\"poll\"]\nrequired_scope_fields = [\"session\"]\ncardinality = \"one_to_one\"\n";
    for role in [
        "screenshot-start",
        "poll-send",
        "poll-response",
        "cached-failure",
        "downstream",
    ] {
        config += &format!(
            "[[event_rules.rules]]\nid = {role:?}\n[event_rules.rules.adapter]\ntype = \"structured\"\nconditions = [{{field = \"observation\", equals = {role:?}}}]\n[event_rules.rules.mapping]\nkind = \"request\"\nname = {{from = \"literal\", value = {role:?}}}\nphase = {{from = \"literal\", value = \"start\"}}\ncorrelation_id = {{from = \"field\", field = \"resource\"}}\nscope = [{{from = \"field\", field = \"session\"}}]\n"
        );
    }
    fs::write(&profile, config).unwrap();
    let mut rows = Vec::new();
    for session in ["run-a", "run-b"] {
        for (second, observation) in [
            "screenshot-start",
            "screenshot-start",
            "poll-send",
            "poll-response",
            "cached-failure",
            "downstream",
        ]
        .into_iter()
        .enumerate()
        {
            rows.push(json!({"ts": format!("2026-01-01T00:00:0{second}+02:00"), "component":"worker", "component_id":session, "session": session, "message":observation, "observation":observation, "resource":"reused-resource", "attempt":"reused-attempt", "poll":"reused-poll", "work":"reused-work"}).to_string());
        }
    }
    let source = temp.path().join("domain.jsonl");
    fs::write(&source, rows.join("\n") + "\n").unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    for (index, expected) in [2, 2, 4, 2, 2, 2, 2].into_iter().enumerate() {
        assert_eq!(count(&report, &format!("policy-{index}")), expected);
    }
    let relations: Vec<_> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| {
            f["id"].as_str().unwrap().contains("relationship-0-") && f["kind"] == "observation"
        })
        .collect();
    assert_eq!(relations.len(), 2);
    assert!(
        relations
            .iter()
            .all(|f| f["evidence"].as_array().unwrap().len() == 2)
    );
    assert!(
        report["populations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|population| !population["id"]
                .as_str()
                .unwrap()
                .ends_with("paired-lifecycles"))
    );
    assert!(
        report["populations"]
            .as_array()
            .unwrap()
            .iter()
            .all(
                |population| !population["id"].as_str().unwrap().ends_with("-failures")
                    && !population["id"].as_str().unwrap().ends_with("-successes")
            )
    );
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["id"]
                .as_str()
                .unwrap()
                .ends_with("-failures-unavailable")
                && finding["kind"] == "unknown")
    );
    assert_eq!(
        report["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|assessment| assessment["goal"] == "failures")
            .unwrap()["status"],
        "unsupported"
    );
}

#[test]
fn selected_pair_support_ignores_unrelated_orphans_and_preserves_selected_ambiguity() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("selection.jsonl");
    let mut data = fs::read_to_string(root().join("examples/investigations/slow.jsonl")).unwrap();
    for (second, id) in [(9, "orphan"), (10, "overlap"), (11, "overlap")] {
        data += &json!({"ts":format!("2026-01-01T00:00:{second:02}+02:00"),"component":"worker","component_id":"other","message":"start","session":"other-run","operation":"unrelated","id":id,"phase":"start"}).to_string();
        data += "\n";
    }
    for (second, phase, component_id) in [(12, "start", "alias-a"), (13, "end", "alias-b")] {
        data += &json!({"ts":format!("2026-01-01T00:00:{second:02}+02:00"),"component":"worker","component_id":component_id,"message":phase,"session":"other-run","operation":"alias","id":"aliased","phase":phase,"outcome":"success"}).to_string();
        data += "\n";
    }
    fs::write(&source, data).unwrap();
    for (index, selector, expected) in [
        (
            0,
            r#"{"name":"run","correlation_id":"parent","scope":["slow-run"]}"#,
            "supported",
        ),
        (
            1,
            r#"{"correlation_id":"overlap","scope":["other-run"]}"#,
            "conflicting",
        ),
    ] {
        let artifact = temp.path().join(format!("evidence-{index}.json"));
        let report = success(&[
            "--config",
            "examples/investigations/profile.toml",
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--select",
            selector,
            "--complete-output",
        ]);
        check(&report, &artifact);
        assert_eq!(report["scopes"][0]["semantic_coverage"]["status"], expected);
        let slow = report["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["goal"] == "slow_operations")
            .unwrap();
        assert_eq!(slow["status"], expected);
        assert_eq!(
            report["scopes"][0]["semantic_coverage"]["relevant_records"],
            2
        );
        assert_eq!(
            report["scopes"][0]["semantic_coverage"]["unmatched_events"],
            if index == 0 { 0 } else { 2 }
        );
        assert!(
            !report["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["id"].as_str().unwrap().contains("scope-adequacy"))
        );
    }
}

#[test]
fn policy_payload_paths_use_decoded_payload_then_per_field_envelope_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("payload-policy.toml");
    let mut config =
        fs::read_to_string(root().join("examples/investigations/domain-policy.toml")).unwrap();
    config = config.replace(
        "extends = \"profile.toml\"",
        &format!(
            "extends = {:?}",
            root()
                .join("examples/investigations/profile.toml")
                .to_str()
                .unwrap()
        ),
    );
    config = config
        .replace(
            "identity_fields = [\"id\"]",
            "identity_fields = [\"payload.nested.resource\"]",
        )
        .replace(
            "join_fields = [\"id\"]",
            "join_fields = [\"payload.nested.resource\"]",
        );
    fs::write(&profile, config).unwrap();
    let rows: Vec<_> = [(0,"start"),(1,"end")].into_iter().map(|(second,phase)| json!({"ts":format!("2026-01-01T00:00:0{second}+02:00"),"component":"worker","component_id":"run","session":"run","operation":"capture","id":"attempt","phase":phase,"outcome":"success","message":r#"event payload {"nested":{"resource":"decoded-resource"}}"#,"payload":{"nested":{"resource":format!("envelope-{second}"),"fallback":"envelope-only"}}}).to_string()).collect();
    let source = temp.path().join("payload.jsonl");
    fs::write(&source, rows.join("\n") + "\n").unwrap();
    let parsed = log_analyzer::parser::parse_log_file_report(
        &source,
        &log_analyzer::config::load_config_from_path(&profile).unwrap(),
    )
    .unwrap();
    assert_eq!(
        parsed.entries[0].payload().unwrap()["nested"]["resource"],
        "decoded-resource"
    );
    for (index, field, expected) in [(0, "resource", 1), (1, "fallback", 1)] {
        if index == 1 {
            fs::write(
                &profile,
                fs::read_to_string(&profile)
                    .unwrap()
                    .replace("payload.nested.resource", "payload.nested.fallback"),
            )
            .unwrap();
        }
        let artifact = temp.path().join(format!("evidence-{field}.json"));
        let report = success(&[
            "--config",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ]);
        check(&report, &artifact);
        assert_eq!(count(&report, "policy-0"), expected);
        assert_eq!(count(&report, "policy-1"), expected);
        assert_eq!(
            report["findings"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| f["id"].as_str().unwrap().contains("relationship-0-")
                    && f["kind"] == "observation")
                .count(),
            1
        );
    }
}

#[test]
fn later_independent_capture_failure_does_not_downgrade_completed_scope() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing.jsonl");
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        "examples/investigations/profile.toml",
        "investigate",
        "examples/investigations/slow.jsonl",
        missing.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    assert_eq!(report["processing"]["status"], "partial");
    assert_eq!(
        report["processing"]["stop"]["scope_ids"],
        json!(["scope-1"])
    );
    assert_eq!(report["scopes"][0]["completeness"], "complete");
    assert_eq!(report["scopes"][0]["analysis_completion"], "complete");
    assert_eq!(report["scopes"][1]["completeness"], "unavailable");
    assert_eq!(report["scopes"][1]["analysis_completion"], "not_performed");
    assert!(
        report["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|assessment| assessment["scope_id"] == "scope-0")
            .all(|assessment| assessment["status"] == "supported")
    );
    assert!(
        report["populations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|population| population["completeness"] == "complete")
    );
    // Exhaustion during this earlier input's analysis still marks it affected.
    let temp = tempfile::tempdir().unwrap();
    let artifact = temp.path().join("work.json");
    let data = fs::read(root().join("examples/investigations/slow.jsonl")).unwrap();
    let limit = (data.len() + 80).to_string();
    let report = success(&[
        "--config",
        "examples/investigations/profile.toml",
        "investigate",
        "examples/investigations/slow.jsonl",
        missing.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--processing-max-work",
        &limit,
    ]);
    check(&report, &artifact);
    assert!(
        report["processing"]["stop"]["scope_ids"]
            .as_array()
            .unwrap()
            .contains(&json!("scope-0"))
    );
    assert_ne!(report["scopes"][0]["analysis_completion"], "complete");
}

#[test]
fn unverifiable_independent_capture_preserves_earlier_source_verification() {
    let temp = tempfile::tempdir().unwrap();
    let invalid = temp.path().join("invalid.jsonl");
    fs::write(&invalid, [0xff, 0xfe, b'\n']).unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        "examples/investigations/profile.toml",
        "investigate",
        "examples/investigations/slow.jsonl",
        invalid.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    assert_eq!(
        report["artifact"]["verification"]["source_and_rules"],
        "unavailable"
    );
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|finding| finding["scope_id"] == "scope-0")
            .all(|finding| finding["verification"]["source_and_rules"] == "available")
    );
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|finding| finding["scope_id"] == "scope-1")
            .all(|finding| finding["verification"]["source_and_rules"] == "unavailable")
    );
    let retained: Value = serde_json::from_slice(&fs::read(artifact).unwrap()).unwrap();
    assert!(
        retained["records"]
            .as_array()
            .unwrap()
            .iter()
            .all(|record| record["verification"]["source_and_rules"] == "available")
    );
    assert_eq!(report["scopes"][0]["completeness"], "complete");
}

#[test]
fn payload_identity_and_relationship_keys_accept_numeric_and_boolean_scalars() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("scalar.jsonl");
    let profile = temp.path().join("profile.toml");
    let config = fs::read_to_string(root().join("examples/investigations/domain-policy.toml"))
        .unwrap()
        .replace(
            "extends = \"profile.toml\"",
            &format!(
                "extends = {:?}",
                root()
                    .join("examples/investigations/profile.toml")
                    .to_str()
                    .unwrap()
            ),
        )
        .replace(
            "identity_fields = [\"id\"]",
            "identity_fields = [\"payload.identity\"]",
        )
        .replace(
            "join_fields = [\"id\"]",
            "join_fields = [\"payload.identity\"]",
        );
    fs::write(&profile, config).unwrap();
    let mut rows = Vec::new();
    for (index, identity) in [json!(17), json!(true)].into_iter().enumerate() {
        for (second, phase) in [(0, "start"), (1, "end")] {
            rows.push(json!({"ts":format!("2026-01-01T00:00:0{second}+02:00"),"component":"worker","component_id":"run","session":"run","operation":"run","id":format!("attempt-{index}"),"phase":phase,"outcome":"success","message":"observation","payload":{"identity":identity}}).to_string());
        }
    }
    fs::write(&source, rows.join("\n") + "\n").unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    assert_eq!(count(&report, "policy-0"), 2);
    assert_eq!(count(&report, "policy-1"), 2);
    assert_eq!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(
                |finding| finding["id"].as_str().unwrap().contains("relationship-0-")
                    && finding["kind"] == "observation"
            )
            .count(),
        2
    );
}

#[test]
fn ordinary_normalization_does_not_consume_array_expansion_capacity() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("normalization.toml");
    fs::write(&profile, "extends = \"base\"\n[parser]\nformat = \"json-lines\"\n[normalization]\nexpand_rows = false\n[normalization.fields]\ntimestamp = \"/ts\"\nmessage = \"/message\"\n").unwrap();
    let source = temp.path().join("ordinary.jsonl");
    let rows:Vec<_>=(0..3).map(|second|json!({"ts":format!("2026-01-01T00:00:0{second}+02:00"),"message":"ordinary normalized record"}).to_string()).collect();
    fs::write(&source, rows.join("\n") + "\n").unwrap();
    for (index, limited) in [false, true].into_iter().enumerate() {
        let artifact = temp.path().join(format!("evidence-{index}.json"));
        let mut args = vec![
            "--config",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--processing-max-expanded-records",
            "0",
        ];
        if limited {
            args.extend(["--processing-max-records", "1"]);
        }
        let report = success(&args);
        check(&report, &artifact);
        assert_eq!(report["processing"]["usage"]["expanded_records"], 0);
        assert_eq!(
            report["processing"]["usage"]["records"],
            if limited { 1 } else { 3 }
        );
        assert_eq!(
            count(&report, "normalized-records"),
            if limited { 1 } else { 3 }
        );
        assert_eq!(
            report["processing"]["status"],
            if limited { "partial" } else { "complete" }
        );
        if limited {
            assert_eq!(report["processing"]["stop"]["limit_name"], "records");
        }
    }
}

#[test]
fn early_normalization_failures_consume_general_record_capacity() {
    for (index, (settings, row, reason)) in [
        ("", "{invalid", "invalid_json"),
        ("root_path = \"/rows\"\n", "{}", "missing_root_path"),
        (
            "decode_paths = [\"/encoded\"]\n",
            "{}",
            "missing_decode_path",
        ),
        (
            "decode_paths = [\"/encoded\"]\n",
            "{\"encoded\":1}",
            "decode_requires_string",
        ),
        (
            "decode_paths = [\"/encoded\"]\n",
            "{\"encoded\":\"invalid\"}",
            "invalid_json_string",
        ),
        (
            "root_path = \"/rows\"\n",
            "{\"rows\":[]}",
            "empty_expansion",
        ),
        (
            "root_path = \"/rows\"\n",
            "{\"rows\":{}}",
            "expansion_requires_array",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("normalization.toml");
        fs::write(&profile, format!("extends = \"base\"\n[parser]\nformat = \"json-lines\"\n[normalization]\nexpand_rows = true\n{settings}")).unwrap();
        let source = temp.path().join("rejected.jsonl");
        fs::write(&source, format!("{row}\n{row}\n{row}\n")).unwrap();
        let artifact = temp.path().join(format!("evidence-{index}.json"));
        let report = success(&[
            "--config",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--processing-max-records",
            "1",
            "--processing-max-expanded-records",
            "0",
        ]);
        check(&report, &artifact);
        assert_eq!(report["processing"]["status"], "partial", "{reason}");
        assert_eq!(
            report["processing"]["stop"]["limit_name"], "records",
            "{reason}"
        );
        assert_eq!(report["processing"]["usage"]["records"], 0);
        assert_eq!(report["processing"]["usage"]["expanded_records"], 0);
        let retained: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
        let text = retained.to_string();
        assert!(text.contains(reason), "missing rejection {reason}");
    }
}

#[test]
fn source_verification_reports_every_declared_input_including_unread_sources() {
    for first_missing in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing.jsonl");
        let existing = root().join("examples/investigations/slow.jsonl");
        let first = if first_missing { &missing } else { &existing };
        let artifact = temp.path().join("evidence.json");
        let report = success(&[
            "--config",
            "examples/investigations/profile.toml",
            "investigate",
            first.to_str().unwrap(),
            missing.to_str().unwrap(),
            existing.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
        ]);
        check(&report, &artifact);
        // A subsequently created source still has no captured identity to compare.
        fs::write(&missing, fs::read(&existing).unwrap()).unwrap();
        let page = success(&[
            "investigation-evidence",
            artifact.to_str().unwrap(),
            "--expected-sha256",
            report["artifact"]["stored_sha256"].as_str().unwrap(),
            "--verify-sources",
        ]);
        let states = page["artifact_retrieval"]["source_verification"]["inputs"]
            .as_array()
            .unwrap();
        assert_eq!(states.len(), 3);
        for (ordinal, state) in states.iter().enumerate() {
            assert_eq!(state["input_ordinal"], ordinal);
            assert_eq!(
                state["status"],
                if ordinal == 0 && !first_missing {
                    "unchanged"
                } else {
                    "unavailable"
                }
            );
            if state["status"] == "unavailable" {
                assert!(state["current_sha256"].is_null());
                assert!(state["reason"].as_str().unwrap().contains("not captured"));
            }
        }
        assert_eq!(page["artifact_retrieval"]["parse_passes"], 0);
    }
}

#[test]
fn identity_only_semantics_do_not_authorize_lifecycle_support_or_zero_counts() {
    for (index, (mixed, selected_only)) in [(false, false), (true, false), (true, true)]
        .into_iter()
        .enumerate()
    {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("identity.toml");
        let mut config =
            fs::read_to_string(root().join("examples/investigations/profile.toml")).unwrap();
        config.push_str("\n[[event_rules.rules]]\nid = \"identity\"\n[event_rules.rules.adapter]\ntype = \"structured\"\nconditions = [{field = \"phase\", equals = \"identity\"}]\n[event_rules.rules.mapping]\nkind = \"request\"\nname = {from = \"field\", field = \"operation\"}\ncorrelation_id = {from = \"field\", field = \"id\"}\nscope = [{from = \"field\", field = \"session\"}]\n");
        fs::write(&profile, config).unwrap();
        let source = temp.path().join("identity.jsonl");
        let identity = json!({"timestamp":"2026-01-01T00:00:00+02:00","level":"INFO","message":"identity observation","operation":"identity","phase":"identity","id":"unpaired","session":"a","outcome":"failure"});
        let mut data = identity.to_string() + "\n";
        if mixed {
            data.push_str(
                &fs::read_to_string(root().join("examples/investigations/slow.jsonl")).unwrap(),
            );
        }
        fs::write(&source, data).unwrap();
        let artifact = temp.path().join(format!("evidence-{index}.json"));
        let mut args = vec![
            "--config",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ];
        if selected_only {
            args.extend(["--select", r#"{"name":"identity"}"#]);
        }
        let report = success(&args);
        check(&report, &artifact);
        for goal in ["slow_operations", "incomplete_lifecycles"] {
            let assessment = report["assessments"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["goal"] == goal)
                .unwrap();
            assert_eq!(
                assessment["status"],
                if mixed && !selected_only {
                    "insufficient_evidence"
                } else {
                    "unsupported"
                },
                "{goal}"
            );
        }
        let failure = report["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["goal"] == "failures")
            .unwrap();
        assert_eq!(
            failure["status"],
            if mixed && !selected_only {
                "insufficient_evidence"
            } else {
                "unsupported"
            }
        );
        if !mixed || selected_only {
            assert!(report["populations"].as_array().unwrap().iter().all(|p| {
                ![
                    "scope-0-starts",
                    "scope-0-ends",
                    "scope-0-paired-lifecycles",
                ]
                .contains(&p["id"].as_str().unwrap())
            }));
        } else {
            assert_eq!(count(&report, "paired-lifecycles"), 3);
        }
        if mixed && !selected_only {
            assert!(
                report["findings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|f| f["id"] == "scope-0-failures-unavailable" && f["kind"] == "unknown")
            );
        }
    }
}

#[test]
fn intentional_start_only_semantics_preserve_counts_without_missing_end_claims() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("start-only.toml");
    let config = fs::read_to_string(root().join("examples/investigations/profile.toml"))
        .unwrap()
        .replacen(
            "phase = {from = \"literal\", value = \"start\"}",
            "phase = {from = \"literal\", value = \"start\"}\nend_expected = false",
            1,
        );
    fs::write(&profile, config).unwrap();
    let source = temp.path().join("start.jsonl");
    fs::write(&source,"{\"timestamp\":\"2026-01-01T00:00:00+02:00\",\"level\":\"INFO\",\"message\":\"standalone start\",\"phase\":\"start\",\"operation\":\"notice\",\"id\":\"one\",\"session\":\"a\"}\n").unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    assert_eq!(count(&report, "starts"), 1);
    assert!(
        !report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["claim"].as_str().unwrap().contains("no observed end"))
    );
}

#[test]
fn structural_rejections_withhold_full_input_support_but_preserve_parsed_measurements() {
    for normalized in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("partial.jsonl");
        let profile = temp.path().join("profile.toml");
        let mut config =
            fs::read_to_string(root().join("examples/investigations/profile.toml")).unwrap();
        let data = fs::read_to_string(root().join("examples/investigations/slow.jsonl")).unwrap();
        if normalized {
            config.push_str("\n[normalization]\nexpand_rows = true\nroot_path = \"/rows\"\n");
            let mut rows: Vec<Value> = data
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            for row in &mut rows {
                row["timestamp"] = row["ts"].clone();
            }
            rows.push(json!({"timestamp":"invalid","message":"unparsed normalized row"}));
            fs::write(&source, json!({"rows":rows}).to_string() + "\n").unwrap();
        } else {
            fs::write(&source, data + "{not valid json}\n").unwrap();
        }
        fs::write(&profile, config).unwrap();
        let artifact = temp.path().join("evidence.json");
        let report = success(&[
            "--config",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ]);
        check(&report, &artifact);
        assert_eq!(report["processing"]["status"], "complete");
        assert_eq!(report["scopes"][0]["completeness"], "partial");
        assert_eq!(report["scopes"][0]["analysis_completion"], "complete");
        assert_eq!(count(&report, "paired-lifecycles"), 3);
        for goal in ["failures", "slow_operations", "incomplete_lifecycles"] {
            assert_eq!(
                report["assessments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["goal"] == goal)
                    .unwrap()["status"],
                "insufficient_evidence",
                "{goal}"
            );
        }
        assert_eq!(
            report["findings"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| f["kind"] == "measurement")
                .count(),
            3
        );
        assert!(
            report["populations"]
                .as_array()
                .unwrap()
                .iter()
                .all(|p| p["completeness"] == "partial" && p["basis"] == "processed_population")
        );
    }
}

#[test]
fn mixed_timestamp_provenance_retains_pairs_without_partial_population_distributions() {
    for all_reliable in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("timestamps.jsonl");
        let rows:Vec<_> = [
            ("offset","start","2026-01-01T10:00:00+02:00"),
            ("offset","end","2026-01-01T10:00:02+02:00"),
            ("other","start",if all_reliable {"2026-01-01T11:00:00+02:00"}else{"2026-01-01T11:00:00"}),
            ("other","end",if all_reliable {"2026-01-01T11:00:03+02:00"}else{"2026-01-01T11:00:03"}),
        ].into_iter().map(|(id,phase,ts)|json!({"ts":ts,"level":"INFO","message":"timestamp boundary","operation":id,"phase":phase,"id":id,"session":"a","outcome":"success"}).to_string()).collect();
        fs::write(&source, rows.join("\n") + "\n").unwrap();
        let artifact = temp.path().join("evidence.json");
        let report = success(&[
            "--config",
            "examples/investigations/profile.toml",
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ]);
        check(&report, &artifact);
        assert_eq!(count(&report, "paired-lifecycles"), 2);
        let findings = report["findings"].as_array().unwrap();
        assert_eq!(
            findings
                .iter()
                .filter(|f| f["kind"] == "measurement")
                .count(),
            if all_reliable { 2 } else { 1 }
        );
        assert_eq!(
            findings
                .iter()
                .filter(|f| f["details"]["calculation"] == "distribution")
                .count(),
            if all_reliable { 5 } else { 0 }
        );
        assert_eq!(
            report["assessments"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["goal"] == "slow_operations")
                .unwrap()["status"],
            if all_reliable {
                "supported"
            } else {
                "insufficient_evidence"
            }
        );
        if !all_reliable {
            assert!(findings.iter().any(|f| {
                f["id"]
                    .as_str()
                    .unwrap()
                    .contains("distribution-unavailable")
                    && f["kind"] == "unknown"
            }));
            assert!(findings.iter().any(|f| {
                f["id"].as_str().unwrap().contains("interval-unavailable") && f["kind"] == "unknown"
            }));
        }
    }
}

#[test]
fn outcome_capabilities_distinguish_literals_and_structured_field_constraints() {
    for (index, (mapping, restriction, outcome, failures, successes)) in [
        ("literal", "", "success", false, true),
        ("literal", "", "failure", true, false),
        ("field", "success", "success", false, true),
        ("field", "failure", "failure", true, false),
        ("field", "", "success", true, true),
    ]
    .into_iter()
    .enumerate()
    {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("outcomes.toml");
        let mut config =
            fs::read_to_string(root().join("examples/investigations/profile.toml")).unwrap();
        if mapping == "literal" {
            config = config.replace(
                "outcome = {from = \"field\", field = \"outcome\"}",
                &format!("outcome = {{from = \"literal\", value = {outcome:?}}}"),
            );
        }
        if !restriction.is_empty() {
            config=config.replace("conditions = [{field = \"phase\", equals = \"end\"}]",&format!("conditions = [{{field = \"phase\", equals = \"end\"}}, {{field = \"outcome\", equals = {restriction:?}}}]"));
        }
        fs::write(&profile, config).unwrap();
        let source = temp.path().join("outcomes.jsonl");
        let rows:Vec<_>=[("start",0),("end",1)].into_iter().map(|(phase,second)|json!({"ts":format!("2026-01-01T00:00:0{second}+02:00"),"message":"outcome boundary","operation":"run","phase":phase,"id":"one","session":"a","outcome":outcome}).to_string()).collect();
        fs::write(&source, rows.join("\n") + "\n").unwrap();
        let artifact = temp.path().join(format!("evidence-{index}.json"));
        let report = success(&[
            "--config",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ]);
        check(&report, &artifact);
        for (suffix, capable) in [("failures", failures), ("successes", successes)] {
            let population = report["populations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["id"] == format!("scope-0-{suffix}"));
            assert_eq!(population.is_some(), capable, "{index} {suffix}");
            if let Some(population) = population {
                assert_eq!(
                    population["count"],
                    u64::from(
                        (suffix == "failures" && outcome == "failure")
                            || (suffix == "successes" && outcome == "success")
                    )
                );
            } else {
                assert!(
                    report["findings"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|f| f["id"] == format!("scope-0-{suffix}-unavailable")
                            && f["kind"] == "unknown")
                );
            }
        }
        assert_eq!(
            report["assessments"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["goal"] == "failures")
                .unwrap()["status"],
            if failures { "supported" } else { "unsupported" }
        );
    }
}

#[test]
fn opposite_boundary_recognition_is_required_for_absence_claims() {
    let original = fs::read_to_string(root().join("examples/investigations/profile.toml")).unwrap();
    let begin = original.find("[[event_rules.rules]]").unwrap();
    let finish = original
        .find("[[event_rules.rules]]\nid = \"finish\"")
        .unwrap();
    for (index, config, selection) in [
        (0, original[..finish].to_string(), None),
        (1, original[..begin].to_string() + &original[finish..], None),
        (
            2,
            original.replacen(
                    "name = {from = \"field\", field = \"operation\"}",
                    "name = {from = \"literal\", value = \"unrelated\"}",
                    1,
                ),
            Some(r#"{"name":"run"}"#),
        ),
        (3, original.replace("conditions = [{field = \"phase\", equals = \"end\"}]", "conditions = [{field = \"phase\", equals = \"end\"}, {field = \"operation\", equals = \"other\"}]"), None),
        (4, original.replace("conditions = [{field = \"phase\", equals = \"end\"}]", "conditions = [{field = \"phase\", equals = \"end\"}, {field = \"session\", equals = \"other\"}]"), None),
        (5, original.replace("conditions = [{field = \"phase\", equals = \"end\"}]", "conditions = [{field = \"phase\", equals = \"end\"}, {field = \"id\", equals = \"other\"}]"), None),
        (6, original.replace("conditions = [{field = \"phase\", equals = \"end\"}]", "conditions = [{field = \"phase\", equals = \"end\"}, {field = \"mode\", equals = \"a\"}, {field = \"mode\", equals = \"b\"}]"), None),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("boundaries.toml");
        fs::write(&profile, config).unwrap();
        let source = temp.path().join("boundaries.jsonl");
        let rows:Vec<_>=[("start",0),("end",1)].into_iter().map(|(phase,second)|json!({"ts":format!("2026-01-01T00:00:0{second}+02:00"),"message":"boundary","operation":"run","phase":phase,"id":"one","session":"a","outcome":"success"}).to_string()).collect();
        fs::write(&source, rows.join("\n") + "\n").unwrap();
        let artifact = temp.path().join(format!("evidence-{index}.json"));
        let mut args = vec![
            "--config",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ];
        if let Some(selection) = selection {
            args.extend(["--select", selection]);
        }
        let report = success(&args);
        check(&report, &artifact);
        assert_eq!(
            report["assessments"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["goal"] == "incomplete_lifecycles")
                .unwrap()["status"],
            "unsupported"
        );
        assert!(
            !report["populations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"] == "scope-0-paired-lifecycles")
        );
        assert!(
            !report["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["kind"] == "observation"
                    && f["claim"].as_str().unwrap().contains("no observed"))
        );
        assert!(
            report["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["kind"] == "unknown"
                    && f["claim"].as_str().unwrap().contains("Opposite-boundary"))
        );
    }
}

#[test]
fn mixed_outcome_families_retain_positive_counts_without_complete_zero_claims() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("mixed.toml");
    let mut config = String::from(
        "extends = \"base\"\n[parser]\nformat = \"json-lines\"\n[event_rules]\nversion = 2\n",
    );
    for (name, outcome) in [("a", "success"), ("b", "failure")] {
        for phase in ["start", "end"] {
            config += &format!(
                "[[event_rules.rules]]\nid = \"{name}-{phase}\"\n[event_rules.rules.adapter]\ntype = \"structured\"\nconditions = [{{field = \"operation\", equals = {name:?}}}, {{field = \"phase\", equals = {phase:?}}}]\n[event_rules.rules.mapping]\nkind = \"request\"\nname = {{from = \"literal\", value = {name:?}}}\nphase = {{from = \"literal\", value = {phase:?}}}\ncorrelation_id = {{from = \"field\", field = \"id\"}}\nscope = [{{from = \"field\", field = \"session\"}}]\n"
            );
            if phase == "end" {
                config += &format!("outcome = {{from = \"literal\", value = {outcome:?}}}\n");
            }
        }
    }
    fs::write(&profile, config).unwrap();
    let source = temp.path().join("mixed.jsonl");
    let rows:Vec<_>=[("a","start",0),("a","end",1),("b","start",2),("b","end",3)].into_iter().map(|(name,phase,second)|json!({"ts":format!("2026-01-01T00:00:0{second}+02:00"),"message":"mixed boundary","operation":name,"phase":phase,"id":name,"session":"a"}).to_string()).collect();
    fs::write(&source, rows.join("\n") + "\n").unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    for suffix in ["failures", "successes"] {
        let population = report["populations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == format!("scope-0-{suffix}"))
            .unwrap();
        assert_eq!(population["count"], 1);
        assert_eq!(population["completeness"], "partial");
        assert!(population["exclusions"][0]["count"].is_null());
    }
    assert_eq!(
        report["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["goal"] == "failures")
            .unwrap()["status"],
        "insufficient_evidence"
    );
    assert_eq!(count(&report, "paired-lifecycles"), 2);
}

#[test]
fn dynamic_phase_mapping_respects_structured_equality_constraints() {
    for constrained in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("dynamic.toml");
        let condition = if constrained { "phase" } else { "component" };
        let value = if constrained { "start" } else { "worker" };
        fs::write(&profile,format!("extends = \"base\"\n[parser]\nformat = \"json-lines\"\n[event_rules]\nversion = 2\n[[event_rules.rules]]\nid = \"dynamic\"\n[event_rules.rules.adapter]\ntype = \"structured\"\nconditions = [{{field = {condition:?}, equals = {value:?}}}]\n[event_rules.rules.mapping]\nkind = \"request\"\nname = {{from = \"field\", field = \"operation\"}}\nphase = {{from = \"field\", field = \"phase\"}}\ncorrelation_id = {{from = \"field\", field = \"id\"}}\nscope = [{{from = \"field\", field = \"session\"}}]\n")).unwrap();
        let source = temp.path().join("dynamic.jsonl");
        let rows:Vec<_>=[("start",0),("end",1)].into_iter().map(|(phase,second)|json!({"ts":format!("2026-01-01T00:00:0{second}+02:00"),"component":"worker","message":"dynamic boundary","operation":"run","phase":phase,"id":"one","session":"a"}).to_string()).collect();
        fs::write(&source, rows.join("\n") + "\n").unwrap();
        let artifact = temp.path().join("evidence.json");
        let report = success(&[
            "--config",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ]);
        check(&report, &artifact);
        assert_eq!(
            report["assessments"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["goal"] == "incomplete_lifecycles")
                .unwrap()["status"],
            if constrained {
                "unsupported"
            } else {
                "supported"
            }
        );
        if !constrained {
            assert_eq!(count(&report, "paired-lifecycles"), 1);
        }
    }
}

#[test]
fn mixed_pair_capabilities_preserve_known_pairs_and_withhold_complete_zeros() {
    for paired in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("mixed-boundaries.toml");
        let mut config =
            fs::read_to_string(root().join("examples/investigations/profile.toml")).unwrap();
        config.push_str("\n[[event_rules.rules]]\nid = \"notice\"\n[event_rules.rules.adapter]\ntype = \"structured\"\nconditions = [{field = \"observation\", equals = \"notice\"}]\n[event_rules.rules.mapping]\nkind = \"request\"\nname = {from = \"literal\", value = \"notice\"}\nphase = {from = \"literal\", value = \"start\"}\ncorrelation_id = {from = \"field\", field = \"id\"}\nscope = [{from = \"field\", field = \"session\"}]\n");
        fs::write(&profile, config).unwrap();
        let source = temp.path().join("mixed.jsonl");
        let mut rows = vec![
            json!({"ts":"2026-01-01T00:00:00+02:00","message":"start","operation":"run","phase":"start","id":"run","session":"a"}),
            json!({"ts":"2026-01-01T00:00:01+02:00","message":"notice","observation":"notice","id":"notice","session":"a"}),
        ];
        if paired {
            rows.push(json!({"ts":"2026-01-01T00:00:02+02:00","message":"end","operation":"run","phase":"end","id":"run","session":"a","outcome":"success"}));
        }
        fs::write(
            &source,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();
        let artifact = temp.path().join("evidence.json");
        let report = success(&[
            "--config",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ]);
        check(&report, &artifact);
        let population = report["populations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "scope-0-paired-lifecycles");
        if paired {
            let population = population.unwrap();
            assert_eq!(population["count"], 1);
            assert_eq!(population["completeness"], "partial");
            assert!(
                report["findings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|f| f["kind"] == "measurement")
            );
        } else {
            assert!(population.is_none());
        }
        assert_eq!(
            report["assessments"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["goal"] == "incomplete_lifecycles")
                .unwrap()["status"],
            "insufficient_evidence"
        );
    }
}

#[test]
fn outcome_capability_requires_a_compatible_end_phase() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("contradictory.toml");
    let mut config = fs::read_to_string(root().join("examples/investigations/profile.toml"))
        .unwrap()
        .replace("outcome = {from = \"field\", field = \"outcome\"}\n", "");
    config.push_str("\n[[event_rules.rules]]\nid = \"not-an-end\"\n[event_rules.rules.adapter]\ntype = \"structured\"\nconditions = [{field = \"phase\", equals = \"start\"}, {field = \"component\", equals = \"other\"}]\n[event_rules.rules.mapping]\nkind = \"request\"\nname = {from = \"field\", field = \"operation\"}\nphase = {from = \"field\", field = \"phase\"}\noutcome = {from = \"literal\", value = \"failure\"}\ncorrelation_id = {from = \"field\", field = \"id\"}\nscope = [{from = \"field\", field = \"session\"}]\n");
    fs::write(&profile, config).unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        profile.to_str().unwrap(),
        "investigate",
        "examples/investigations/slow.jsonl",
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    assert_eq!(
        report["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["goal"] == "failures")
            .unwrap()["status"],
        "unsupported"
    );
    assert!(
        !report["populations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "scope-0-failures")
    );
}

#[test]
fn observed_capabilities_do_not_cross_constrained_correlation_ids() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("ids.toml");
    let mut config = String::from(
        "extends = \"base\"\n[parser]\nformat = \"json-lines\"\n[event_rules]\nversion = 2\n",
    );
    for (id, outcome) in [("a", "success"), ("b", "failure")] {
        for phase in ["start", "end"] {
            config += &format!(
                "[[event_rules.rules]]\nid = \"{id}-{phase}\"\n[event_rules.rules.adapter]\ntype = \"structured\"\nconditions = [{{field = \"id\", equals = {id:?}}}, {{field = \"phase\", equals = {phase:?}}}]\n[event_rules.rules.mapping]\nkind = \"request\"\nname = {{from = \"field\", field = \"operation\"}}\nphase = {{from = \"literal\", value = {phase:?}}}\ncorrelation_id = {{from = \"field\", field = \"id\"}}\nscope = [{{from = \"field\", field = \"session\"}}]\n"
            );
            if phase == "end" {
                config += &format!("outcome = {{from = \"literal\", value = {outcome:?}}}\n");
            }
        }
    }
    fs::write(&profile, config).unwrap();
    let source = temp.path().join("ids.jsonl");
    let rows:Vec<_>=[("a","start",0),("a","end",1),("b","start",2),("b","end",3)].into_iter().map(|(id,phase,second)|json!({"ts":format!("2026-01-01T00:00:0{second}+02:00"),"message":"id boundary","operation":"run","phase":phase,"id":id,"session":"same"}).to_string()).collect();
    fs::write(&source, rows.join("\n") + "\n").unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    for suffix in ["failures", "successes"] {
        let population = report["populations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == format!("scope-0-{suffix}"))
            .unwrap();
        assert_eq!(population["count"], 1);
        assert_eq!(population["completeness"], "partial");
    }
    assert_eq!(
        report["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["goal"] == "failures")
            .unwrap()["status"],
        "insufficient_evidence"
    );
}

#[test]
fn mutually_exclusive_shared_field_mappings_do_not_authorize_capability() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("shared-field.toml");
    let mut config = fs::read_to_string(root().join("examples/investigations/profile.toml"))
        .unwrap()
        .replace("outcome = {from = \"field\", field = \"outcome\"}\n", "");
    config.push_str("\n[[event_rules.rules]]\nid = \"impossible-end\"\n[event_rules.rules.adapter]\ntype = \"structured\"\nconditions = [{field = \"component\", equals = \"other\"}]\n[event_rules.rules.mapping]\nkind = \"request\"\nname = {from = \"field\", field = \"operation\"}\nphase = {from = \"field\", field = \"boundary\"}\noutcome = {from = \"field\", field = \"boundary\"}\ncorrelation_id = {from = \"field\", field = \"id\"}\nscope = [{from = \"field\", field = \"session\"}]\n");
    fs::write(&profile, config).unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        profile.to_str().unwrap(),
        "investigate",
        "examples/investigations/slow.jsonl",
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    assert_eq!(
        report["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["goal"] == "failures")
            .unwrap()["status"],
        "unsupported"
    );
    assert!(
        !report["populations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "scope-0-failures")
    );
}

#[test]
fn restrictive_end_outcomes_withhold_absence_claims_but_complementary_rules_cover_them() {
    let original = fs::read_to_string(root().join("examples/investigations/profile.toml")).unwrap();
    let finish = original
        .find("[[event_rules.rules]]\nid = \"finish\"")
        .unwrap();
    let success_rule=original[finish..].replace("conditions = [{field = \"phase\", equals = \"end\"}]","conditions = [{field = \"phase\", equals = \"end\"}, {field = \"outcome\", equals = \"success\"}]").replace("outcome = {from = \"field\", field = \"outcome\"}","outcome = {from = \"literal\", value = \"success\"}");
    for mode in 0..4 {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("end-outcomes.toml");
        let mut config = original[..finish].to_string();
        if mode == 2 {
            config += &original[finish..]
                .replace("outcome = {from = \"field\", field = \"outcome\"}\n", "");
        } else {
            config += &success_rule;
            if mode == 1 {
                config += &success_rule
                    .replace("id = \"finish\"", "id = \"failure-finish\"")
                    .replace("\"success\"", "\"failure\"");
            }
        }
        fs::write(&profile, config).unwrap();
        let source = temp.path().join("end-outcomes.jsonl");
        let mut rows = vec![
            json!({"ts":"2026-01-01T00:00:00+02:00","message":"start","operation":"run","phase":"start","id":"one","session":"a"}),
        ];
        if mode == 0 || mode == 3 {
            rows.push(json!({"ts":"2026-01-01T00:00:01+02:00","message":"end","operation":"run","phase":"end","id":"one","session":"a","outcome":if mode==0 {"failure"}else{"success"}}));
        }
        fs::write(
            &source,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();
        let artifact = temp.path().join("evidence.json");
        let report = success(&[
            "--config",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ]);
        check(&report, &artifact);
        assert_eq!(
            report["assessments"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["goal"] == "incomplete_lifecycles")
                .unwrap()["status"],
            if mode == 0 {
                "unsupported"
            } else {
                "supported"
            }
        );
        if mode == 0 {
            assert!(
                !report["populations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["id"] == "scope-0-ends")
            );
            assert!(
                !report["findings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|f| f["kind"] == "observation"
                        && f["claim"].as_str().unwrap().contains("no observed end"))
            );
        } else {
            assert_eq!(count(&report, "ends"), u64::from(mode == 3));
        }
    }
}

#[test]
fn record_size_cutoffs_preserve_nonempty_and_rejected_coverage() {
    let short = json!({"ts":"2026-01-01T00:00:00+02:00","message":"sample"}).to_string();
    let large = json!({"ts":"2026-01-01T00:00:00+02:00","message":"x".repeat(300)}).to_string();
    let classic = fs::read_to_string(root().join("evals/fixtures/classic.log"))
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_string();
    assert!(classic.len() < 160 && classic.len() + 121 > 160);
    for (index, (format, data, nonempty, blanks, rejection_line, reason)) in [
        (
            "json-lines",
            large.clone() + "\n",
            1,
            0,
            Some(1),
            "physical_record_limit",
        ),
        (
            "classic",
            classic.clone() + &"x".repeat(300) + "\n",
            1,
            0,
            Some(1),
            "physical_record_limit",
        ),
        (
            "auto",
            large.clone() + "\n",
            1,
            0,
            Some(1),
            "physical_record_limit",
        ),
        (
            "auto",
            format!("{short}\n{large}\n"),
            2,
            0,
            Some(2),
            "physical_record_limit",
        ),
        (
            "auto",
            format!("\n{short}\n{large}\n"),
            2,
            1,
            Some(3),
            "physical_record_limit",
        ),
        (
            "classic",
            format!("{classic}\n{}\n", "c".repeat(120)),
            2,
            0,
            Some(1),
            "multiline_record_limit",
        ),
        (
            "classic",
            format!("{classic}\n{}\n", "c".repeat(300)),
            2,
            0,
            Some(2),
            "physical_record_limit",
        ),
        ("json-lines", " ".repeat(300) + "\n", 0, 1, None, ""),
    ]
    .into_iter()
    .enumerate()
    {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("limit.toml");
        fs::write(
            &profile,
            format!("extends = \"base\"\n[parser]\nformat = {format:?}\n"),
        )
        .unwrap();
        let source = temp.path().join("limit.log");
        fs::write(&source, data).unwrap();
        let artifact = temp.path().join(format!("evidence-{index}.json"));
        let report = success(&[
            "--config",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--record-max-bytes",
            "160",
            "--complete-output",
        ]);
        check(&report, &artifact);
        assert_eq!(report["processing"]["status"], "partial");
        let coverage = &report["report_metadata"]["evidence"]["inputs"][0]["coverage"];
        assert_eq!(coverage["nonempty_lines"], nonempty, "case {index}");
        assert_eq!(
            coverage["structural_diagnostics"]["blank_lines"], blanks,
            "case {index}"
        );
        assert_eq!(coverage["parsed_entries"], 0);
        assert_eq!(
            coverage["rejected_candidates"],
            u64::from(rejection_line.is_some())
        );
        if let Some(line) = rejection_line {
            assert_eq!(
                report["report_metadata"]["evidence"]["scope"]["status"],
                "unparsed_input"
            );
            let diagnostics = coverage["structural_diagnostics"]["diagnostics"]
                .as_array()
                .unwrap();
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(diagnostics[0]["line"], line);
            assert_eq!(diagnostics[0]["reason"], reason);
        } else {
            assert_eq!(
                report["report_metadata"]["evidence"]["scope"]["status"],
                "empty_input"
            );
        }
        let retained: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
        assert!(retained["records"].as_array().unwrap().is_empty());
    }
}

#[test]
fn unresolved_classifications_withhold_semantic_zeros_and_policy_cardinality() {
    let original = fs::read_to_string(root().join("examples/investigations/profile.toml")).unwrap();
    let policy = r#"
[investigation]
version = 1
[[investigation.roles]]
id = "starts"
rule_ids = ["begin"]
[[investigation.roles]]
id = "ends"
rule_ids = ["finish"]
[[investigation.populations]]
id = "events"
entity = "events"
roles = ["starts", "ends"]
identity_fields = ["id"]
grouping = "occurrence"
[[investigation.relationships]]
id = "start-end"
source_role = "starts"
target_role = "ends"
join_fields = ["id"]
required_scope_fields = ["session"]
cardinality = "one_to_one"
"#;
    let conflicting = r#"
[[event_rules.rules]]
id = "conflicting-end"
[event_rules.rules.adapter]
type = "structured"
conditions = [{field = "message", equals = "uncertain"}, {field = "phase", equals = "end"}]
[event_rules.rules.mapping]
kind = "request"
name = {from = "literal", value = "other"}
phase = {from = "literal", value = "end"}
correlation_id = {from = "field", field = "id"}
scope = [{from = "field", field = "session"}]
outcome = {from = "literal", value = "failure"}
"#;
    let unrelated = r#"
[[event_rules.rules]]
id = "aux-one"
[event_rules.rules.adapter]
type = "structured"
conditions = [{field = "phase", equals = "aux"}]
[event_rules.rules.mapping]
kind = "request"
name = {from = "literal", value = "one"}
correlation_id = {from = "field", field = "id"}
[[event_rules.rules]]
id = "aux-two"
[event_rules.rules.adapter]
type = "structured"
conditions = [{field = "phase", equals = "aux"}]
[event_rules.rules.mapping]
kind = "request"
name = {from = "literal", value = "two"}
correlation_id = {from = "field", field = "id"}
"#;
    for variant in [
        "conflict",
        "invalid",
        "potential-conflict",
        "potential-invalid",
        "only-conflict",
        "only-invalid",
        "unrelated",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("uncertain.toml");
        fs::write(
            &profile,
            format!("{original}{conflicting}{unrelated}{policy}"),
        )
        .unwrap();
        let row = |phase: &str, message: &str, outcome: &str, second: u32| {
            json!({"ts":format!("2026-01-01T00:00:{second:02}+02:00"),"message":message,"operation":"run","phase":phase,"id":"one","session":"a","outcome":outcome}).to_string()
        };
        let only = variant.starts_with("only-");
        let potential = variant.starts_with("potential-");
        let mut rows = Vec::new();
        if !only {
            rows.push(row("start", "start", "success", 0));
            if !potential {
                rows.push(row("end", "end", "success", 1));
            }
        }
        rows.push(if variant == "unrelated" {
            row("aux", "unrelated", "success", 2)
        } else if variant.ends_with("invalid") {
            row("end", "uncertain-invalid", "not-an-outcome", 2)
        } else {
            row("end", "uncertain", "success", 2)
        });
        let source = temp.path().join("uncertain.jsonl");
        fs::write(&source, rows.join("\n") + "\n").unwrap();
        let artifact = temp.path().join("evidence.json");
        let report = success(&[
            "--config",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ]);
        check(&report, &artifact);
        let populations = report["populations"].as_array().unwrap();
        for suffix in [
            "starts",
            "ends",
            "successes",
            "failures",
            "paired-lifecycles",
        ] {
            let population = populations
                .iter()
                .find(|p| p["id"] == format!("scope-0-{suffix}"));
            if let Some(population) = population {
                assert_eq!(population["completeness"], "partial", "{variant} {suffix}");
                assert!(
                    population["count"].as_u64().unwrap() > 0,
                    "{variant} {suffix}"
                );
                assert!(
                    population["exclusions"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|e| e["count"].is_null())
                );
            }
        }
        let policy_population = populations.iter().find(|p| p["id"] == "scope-0-policy-0");
        if only {
            assert!(policy_population.is_none());
        } else {
            let population = policy_population.unwrap();
            assert_eq!(population["count"], if potential { 1 } else { 2 });
            assert_eq!(
                population["completeness"],
                if variant == "unrelated" {
                    "complete"
                } else {
                    "partial"
                }
            );
        }
        let findings = report["findings"].as_array().unwrap();
        let joins: Vec<_> = findings
            .iter()
            .filter(|f| {
                f["id"]
                    .as_str()
                    .unwrap()
                    .starts_with("scope-0-relationship-0-")
                    && f["kind"] == "observation"
            })
            .collect();
        assert_eq!(joins.len(), usize::from(variant == "unrelated"));
        if potential {
            assert!(!findings.iter().any(|f| f["kind"] == "observation"
                && f["claim"].as_str().unwrap().contains("no observed end")));
        }
        assert!(
            findings
                .iter()
                .any(|f| f["id"] == "scope-0-classification-unavailable" && f["kind"] == "unknown")
        );
        if !only && !potential {
            assert!(
                findings
                    .iter()
                    .any(|f| f["kind"] == "measurement" && f["details"]["value"] == 1000)
            );
        }
        let failure = report["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["goal"] == "failures")
            .unwrap();
        assert_eq!(
            failure["status"],
            if variant.ends_with("invalid") {
                "insufficient_evidence"
            } else {
                "conflicting"
            }
        );
        if variant.ends_with("invalid") {
            assert_eq!(
                report["scopes"][0]["semantic_coverage"]["status"],
                "insufficient_evidence"
            );
        }
    }
}

#[test]
fn readable_default_creates_fresh_artifacts_and_reports_observed_errors() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("ordinary.log");
    fs::write(&source, "2025-01-01T00:00:00Z ERROR app: renderer failed\n2025-01-01T00:00:01Z WARN app: retry scheduled\n").unwrap();
    let mut artifacts = Vec::new();
    for _ in 0..2 {
        let output = binary()
            .current_dir(temp.path())
            .args(["investigate", "ordinary.log"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.starts_with("Investigation: complete"), "{text}");
        assert!(text.contains("renderer failed"), "{text}");
        assert!(text.contains("failures: unsupported"), "{text}");
        assert!(text.contains("2 parsed records (complete)"), "{text}");
        assert!(!text.contains("--report-cursor"), "{text}");
        let path = text
            .lines()
            .find_map(|line| line.strip_prefix("Evidence: "))
            .unwrap();
        let artifact: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(artifact["processing"]["status"], "complete");
        artifacts.push(path.to_owned());
    }
    assert_ne!(artifacts[0], artifacts[1]);
}

#[test]
fn readable_next_command_continues_after_displayed_findings() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("ordinary.log");
    let artifact = temp
        .path()
        .join("evidence ' $(touch injected) `touch injected`.json");
    fs::write(
        &source,
        (0..60)
            .map(|index| format!("2025-01-01T00:00:00Z ERROR app: synthetic failure {index}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let output = run(&[
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let retained: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    let all = retained["findings"].as_array().unwrap();
    assert!(all.len() > 40);
    let individual = text
        .split("Observed findings (individual findings page):")
        .nth(1)
        .unwrap();
    assert_eq!(
        individual
            .lines()
            .filter(|line| line.starts_with("- ["))
            .count(),
        16
    );
    assert!(
        text.contains(
            "4 individual findings on this page are represented in the source error overview above."
        ),
        "{text}"
    );
    assert!(
        text.contains("60 physical records; 60 distinct source messages"),
        "{text}"
    );
    for finding in &all[..20] {
        for excerpt in finding["evidence"].as_array().unwrap() {
            assert!(
                text.contains(excerpt["text"].as_str().unwrap()),
                "first-page evidence missing: {text}"
            );
            assert!(
                text.contains(
                    excerpt["occurrence"]["evidence_ref"]["reference_id"]
                        .as_str()
                        .unwrap()
                ),
                "first-page citation missing: {text}"
            );
        }
    }
    let next = text
        .lines()
        .find_map(|line| line.strip_prefix("Next: "))
        .unwrap();
    assert!(next.contains(" --report-cursor v1:"), "{next}");
    let output = if cfg!(unix) {
        Command::new("/bin/sh")
            .current_dir(temp.path())
            .arg("-c")
            .arg(next)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    Path::new(env!("CARGO_BIN_EXE_log-analyzer"))
                        .parent()
                        .unwrap()
                        .display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .output()
            .unwrap()
    } else {
        // The printed command uses POSIX quoting; preserve pagination coverage elsewhere.
        let hash = text
            .lines()
            .find_map(|line| line.strip_prefix("SHA-256: "))
            .unwrap();
        let cursor = next.split(" --report-cursor ").nth(1).unwrap();
        run(&[
            "evidence",
            artifact.to_str().unwrap(),
            "--expected-sha256",
            hash,
            "--collection",
            "/findings",
            "--report-cursor",
            cursor,
        ])
    };
    assert!(!temp.path().join("injected").exists());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let page: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(page["artifact_retrieval"]["prior"], 20);
    assert_eq!(page["artifact_retrieval"]["displayed"], 20);
    assert_eq!(page["artifact_retrieval"]["items"], json!(&all[20..40]));
    let first_ids: Vec<_> = all[..20].iter().map(|finding| &finding["id"]).collect();
    assert!(
        page["artifact_retrieval"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| !first_ids.contains(&&finding["id"]))
    );
}

#[test]
fn discovery_samples_every_input_and_keeps_heterogeneous_profiles_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    let large = temp.path().join("first.log");
    fs::write(
        &large,
        "2025-01-01T00:00:00Z INFO app: ordinary record\n".repeat(2000),
    )
    .unwrap();
    let eyes = temp.path().join("second.log");
    fs::write(&eyes, "core-ufg (manager-ufg-one/eyes-ufg-two/check-ufg-three) | 2025-01-01T00:00:01Z [INFO ] Command \"check\" is called\n").unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "investigate",
        large.to_str().unwrap(),
        eyes.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
    ]);
    check(&report, &artifact);
    assert_eq!(report["report_metadata"]["active_profile"], "base");
    let selection =
        &report["report_metadata"]["evidence"]["query"]["execution"]["profile_selection"];
    assert_eq!(selection["samples"].as_array().unwrap().len(), 2);
    assert!(selection["samples"][1]["sample_bytes"].as_u64().unwrap() > 0);
    assert!(
        selection["limitations"]
            .as_str()
            .unwrap()
            .contains("semantic validation")
    );
    assert_eq!(selection["status"], "insufficient_evidence");
    assert_eq!(report["processing"]["inputs"][1]["capture"], "complete");
}

#[test]
fn planned_input_allowance_covers_multiple_files_above_old_sixteen_mib_cap() {
    let temp = tempfile::tempdir().unwrap();
    // Empty physical lines exercise capture planning without expensive synthetic parsing.
    let paths: Vec<_> = (0..4)
        .map(|index| {
            let path = temp.path().join(format!("input-{index}.log"));
            fs::write(&path, vec![b'\n'; 5 * 1024 * 1024]).unwrap();
            path
        })
        .collect();
    let artifact = temp.path().join("evidence.json");
    let mut args = vec!["--preset", "base", "investigate"];
    args.extend(paths.iter().map(|path| path.to_str().unwrap()));
    args.extend(["--artifact", artifact.to_str().unwrap()]);
    let report = success(&args);
    check(&report, &artifact);
    assert_eq!(report["processing"]["status"], "complete");
    assert!(
        report["processing"]["limits"]["input_bytes"]
            .as_u64()
            .unwrap()
            > 20 * 1024 * 1024
    );
    assert_eq!(
        report["report_metadata"]["evidence"]["query"]["execution"]["parse_passes"],
        4
    );
    assert!(
        report["processing"]["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|input| input["capture"] == "complete")
    );
}

const RESOURCE_PROFILE: &str = r#"
extends = "base"
profile_name = "custom-resources"
[[resource_observations]]
id = "assets"
scope_field = "context"
scope_separator = ":"
namespace_prefix = "run-"
owner_prefix = "operation-"
manifest_marker = "manifest listing"
url_contains = "asset:"
resource_markers = ["asset batch"]
resources_path = "bundle.resources"
entries_field = "items"
url_field = "address"
hash_field = "digest"
viewport_marker = "dimensions"
viewport_reset_marker = "new snapshot"
width_field = "w"
height_field = "h"
[[resource_observations.fingerprints]]
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
label = "synthetic reference image"
[investigation_view]
source_fields = ["component", "actor_kind"]
[[investigation_view.groups]]
id = "reference-observations"
title = "Resource observations"
select = [{path = "/details/resource_status", equals = "fingerprint_match"}]
by = [
  {label = "Viewport", path = "/details/viewport", template = "{/width}x{/height}"},
  {label = "Fingerprint", path = "/details/fingerprint", template = "{/label}"}
]

"#;

fn resource_line(scope: &str, message: &str, payload: Value) -> String {
    format!(
        "{}\n",
        json!({"timestamp":"2025-01-01T00:00:00Z","level":"INFO","message":message,"context":scope,"payload":payload})
    )
}

#[test]
fn resource_profiles_resolve_automatically_for_all_inputs_and_abstain_on_ambiguity() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config");
    fs::create_dir(&config).unwrap();
    fs::write(config.join("resources.toml"), RESOURCE_PROFILE).unwrap();
    let data = [
        resource_line("run-one:operation-one", "manifest listing", json!(["asset:reference"])),
        resource_line("run-one", "asset batch", json!([{"bundle":{"resources":{"items":[{"address":"asset:reference","digest":"a".repeat(64)}]}}}])),
    ].concat();
    let inputs: Vec<_> = (0..4)
        .map(|index| {
            let path = temp.path().join(format!("input-{index}.log"));
            fs::write(&path, &data).unwrap();
            path
        })
        .collect();
    let investigate = || {
        let mut command = binary();
        for (name, _) in std::env::vars().filter(|(name, _)| name.starts_with("LOG_ANALYZER_")) {
            command.env_remove(name);
        }
        let output = command
            .current_dir(temp.path())
            .arg("investigate")
            .args(&inputs)
            .arg("--complete-output")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let report = investigate();
    let selection =
        &report["report_metadata"]["evidence"]["query"]["execution"]["profile_selection"];
    assert_eq!(selection["status"], "selected", "{report}");
    assert_eq!(selection["profile"], "custom-resources");
    let candidate = selection["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| candidate["profile"] == "custom-resources")
        .unwrap();
    assert_eq!(candidate["lifecycle_records"], 0);
    assert_eq!(candidate["resource_records"], 8);
    assert_eq!(candidate["matched_inputs"], 4);
    assert_eq!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|finding| finding["claim"]
                .as_str()
                .unwrap()
                .contains("profile-declared fingerprint: synthetic reference image"))
            .count(),
        4
    );

    fs::write(
        &inputs[3],
        resource_line("unrelated", "manifest listing", json!(["asset:reference"])),
    )
    .unwrap();
    assert_eq!(
        investigate()["report_metadata"]["profile_selection"]["status"],
        "insufficient_evidence"
    );
    fs::write(&inputs[3], &data).unwrap();
    fs::write(
        config.join("other.toml"),
        RESOURCE_PROFILE
            .replace("custom-resources", "other-resources")
            .replace("synthetic reference image", "other reference"),
    )
    .unwrap();
    assert_eq!(
        investigate()["report_metadata"]["profile_selection"]["status"],
        "ambiguous"
    );
}

#[test]
fn readable_resource_overview_groups_all_captures_and_viewports_without_changing_findings() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile.toml");
    fs::write(&profile, RESOURCE_PROFILE).unwrap();
    let scope = "run-one:operation-one";
    let manifest = resource_line(scope, "manifest listing", json!(["asset:reference"]));
    let batch = resource_line(
        scope,
        "asset batch",
        json!([{"bundle":{"resources":{"items":[{"address":"asset:reference","digest":"a".repeat(64)}]}}}]),
    );
    let first = temp.path().join("first.log");
    let second = temp.path().join("second.log");
    fs::write(
        &first,
        format!(
            "{}{}{}{}{}{}{}",
            batch,
            resource_line(scope, "dimensions", json!({"w":1280,"h":720})),
            manifest.repeat(30),
            resource_line(scope, "dimensions", json!({"w":1920,"h":1080})),
            manifest.repeat(7),
            resource_line(scope, "new snapshot", Value::Null),
            manifest.repeat(2)
        ),
    )
    .unwrap();
    fs::write(
        &second,
        format!(
            "{}{}{}",
            batch,
            resource_line(scope, "dimensions", json!({"w":1280,"h":720})),
            manifest.repeat(5)
        ),
    )
    .unwrap();
    let artifact = temp.path().join("text-evidence.json");
    let output = run(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    for row in [
        "[scope-0] Viewport: 1280x720, Fingerprint: synthetic reference image — 30 findings",
        "[scope-0] Viewport: 1920x1080, Fingerprint: synthetic reference image — 7 findings",
        "[scope-0] Viewport: unknown, Fingerprint: synthetic reference image — 2 findings",
        "[scope-1] Viewport: 1280x720, Fingerprint: synthetic reference image — 5 findings",
    ] {
        assert!(text.contains(row), "missing {row}: {text}");
    }
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("- [") && line.contains(" — "))
            .count(),
        4
    );
    assert!(
        text.contains(
            "20 individual findings on this page are represented in the grouped overview"
        )
    );
    let retained: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    assert_eq!(
        retained["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|finding| finding["details"]["resource_status"] == "fingerprint_match")
            .count(),
        44
    );
    assert!(text.contains("Counts are findings, not unique resources, records or failures"));

    let json_artifact = temp.path().join("json-evidence.json");
    let report = success(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
        "--artifact",
        json_artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &json_artifact);
    assert_eq!(report["findings"], retained["findings"]);
    let next = text
        .lines()
        .find_map(|line| line.strip_prefix("Next: "))
        .unwrap();
    let args: Vec<_> = next
        .split_whitespace()
        .skip(1)
        .map(|arg| arg.trim_matches('\''))
        .collect();
    let page = success(&args);
    assert_eq!(page["artifact_retrieval"]["prior"], 20);
    assert_eq!(
        page["artifact_retrieval"]["items"],
        json!(&retained["findings"].as_array().unwrap()[20..40])
    );
}

#[test]
fn readable_resource_overview_bounds_groups_and_keeps_distinct_fingerprints_separate() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile.toml");
    fs::write(&profile, format!("{RESOURCE_PROFILE}\n[[resource_observations.fingerprints]]\nsha256 = \"{}\"\nlabel = \"another reference\"\n", "b".repeat(64))).unwrap();
    let scope = "run-one:operation-one";
    let mut data = resource_line(
        scope,
        "asset batch",
        json!([{"bundle":{"resources":{"items":[{"address":"asset:reference","digest":"a".repeat(64)},{"address":"asset:other","digest":"b".repeat(64)}]}}}]),
    );
    let manifest = resource_line(scope, "manifest listing", json!(["asset:reference"]));
    for index in 0..21 {
        data.push_str(&resource_line(
            scope,
            "dimensions",
            json!({"w":100+index,"h":100}),
        ));
        data.push_str(&manifest);
        if index == 0 {
            data.push_str(&resource_line(
                scope,
                "manifest listing",
                json!(["asset:other"]),
            ));
        }
    }
    data.push_str(&resource_line(
        scope,
        "dimensions",
        json!({"w":100,"h":100}),
    ));
    data.push_str(&manifest.repeat(3));
    let source = temp.path().join("resources.log");
    fs::write(&source, data).unwrap();
    let artifact = temp.path().join("evidence.json");
    let output = run(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains(
            "[scope-0] Viewport: 100x100, Fingerprint: synthetic reference image — 4 findings"
        ),
        "{text}"
    );
    assert!(
        text.contains("[scope-0] Viewport: 100x100, Fingerprint: another reference — 1 findings"),
        "{text}"
    );
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("- [") && line.contains(" — "))
            .count(),
        20
    );
    assert!(text.contains("2 additional matching findings are outside the displayed groups"));
    let retained: Value = serde_json::from_slice(&fs::read(artifact).unwrap()).unwrap();
    assert_eq!(
        retained["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|finding| finding["details"]["resource_status"] == "fingerprint_match")
            .count(),
        25
    );
}

#[test]
fn configured_views_group_measured_performance_by_components_and_arbitrary_source_fields() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile.toml");
    let views = r#"
[investigation_view]
source_fields = ["component", "actor_kind", "event.name"]
[[investigation_view.groups]]
id = "components"
title = "Performance by component"
select = [{path = "/kind", equals = "measurement"}]
by = [{label = "Component", path = "/evidence/0/fields/component"}]
measurements = [
 {label = "Mean elapsed", path = "/details/value", unit_path = "/details/unit", unit = "ms", aggregate = "mean"},
 {label = "Max elapsed", path = "/details/value", unit_path = "/details/unit", unit = "ms", aggregate = "max"},
 {label = "Unavailable unit", path = "/details/value", unit_path = "/details/unit", unit = "seconds", aggregate = "sum"}
]
[[investigation_view.groups]]
id = "actors"
title = "Intervals by actor"
select = [{path = "/kind", equals = "measurement"}]
by = [{label = "Actor", path = "/evidence/0/fields/actor_kind"}]
"#;
    fs::write(
        &profile,
        format!(
            "{}{views}",
            fs::read_to_string(root().join("examples/investigations/profile.toml")).unwrap()
        ),
    )
    .unwrap();
    let source = temp.path().join("input.jsonl");
    let data: String = fs::read_to_string(root().join("examples/investigations/slow.jsonl"))
        .unwrap()
        .lines()
        .map(|line| {
            let mut record: Value = serde_json::from_str(line).unwrap();
            record["actor_kind"] = if record["id"] == "parent" {
                json!("controller")
            } else {
                json!("worker")
            };
            format!("{record}\n")
        })
        .collect();
    fs::write(&source, data).unwrap();
    let artifact = temp.path().join("text.json");
    let output = run(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    for expected in [
        "Performance by component",
        "Component: worker — 3 findings",
        "Mean elapsed: 4000.00 ms (3 measured, 0 unavailable)",
        "Max elapsed: 8000.00 ms",
        "Unavailable unit: unknown (0 measured, 3 unavailable)",
        "Actor: controller — 1 findings",
        "Actor: worker — 2 findings",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    let report = success(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        temp.path().join("json.json").to_str().unwrap(),
        "--complete-output",
    ]);
    let retained: Value = serde_json::from_slice(&fs::read(artifact).unwrap()).unwrap();
    assert_eq!(report["findings"], retained["findings"]);
    let measurements: Vec<_> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["kind"] == "measurement")
        .collect();
    assert_eq!(measurements.len(), 3);
    assert!(
        measurements
            .iter()
            .all(|finding| finding["evidence"][0]["fields"]["event.name"].is_string())
    );
    let redacted_artifact = temp.path().join("redacted.json");
    let redacted = success(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        redacted_artifact.to_str().unwrap(),
        "--redact",
        "--complete-output",
    ]);
    check(&redacted, &redacted_artifact);
    assert!(
        redacted["findings"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|finding| finding["evidence"].as_array().unwrap())
            .all(|excerpt| excerpt
                .get("fields")
                .is_none_or(|fields| fields == &json!({})))
    );
    let private_view = fs::read_to_string(&profile)
        .unwrap()
        .replace("Performance by component", "private-config-title")
        .replace("label = \"Component\"", "label = \"private-config-label\"")
        .replace(
            "path = \"/evidence/0/fields/component\"}",
            "path = \"/evidence/0/fields/component\", template = \"private-config-template\"}",
        );
    fs::write(&profile, private_view).unwrap();
    let output = run(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        temp.path().join("redacted-text.json").to_str().unwrap(),
        "--redact",
    ]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Configured findings"), "{text}");
    assert!(!text.contains("private-config"), "{text}");
    assert!(text.contains("configured unit (3 measured"), "{text}");
}

#[test]
fn invalid_view_configuration_is_rejected_and_grouping_can_be_disabled() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("input.log");
    fs::write(&source, [
        resource_line("run-one:operation-one", "manifest listing", json!(["asset:reference"])),
        resource_line("run-one", "asset batch", json!([{"bundle":{"resources":{"items":[{"address":"asset:reference","digest":"a".repeat(64)}]}}}])),
    ].concat()).unwrap();
    for (index, data) in [
        RESOURCE_PROFILE.replace("/details/viewport", "details/viewport"),
        RESOURCE_PROFILE.replace("{/width}x{/height}", "{/width"),
        RESOURCE_PROFILE.replace(
            "source_fields = [\"component\", \"actor_kind\"]",
            "source_fields = [\"component\", \"component\"]",
        ),
        RESOURCE_PROFILE.replace(
            "id = \"reference-observations\"",
            "max_groups = 0\nid = \"reference-observations\"",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let profile = temp.path().join(format!("bad-{index}.toml"));
        fs::write(&profile, data).unwrap();
        let output = run(&[
            "--profile",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
        ]);
        assert!(!output.status.success(), "invalid view accepted: {index}");
    }
    let profile = temp.path().join("enabled.toml");
    fs::write(&profile, RESOURCE_PROFILE).unwrap();
    let output = run(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        temp.path().join("enabled.json").to_str().unwrap(),
    ]);
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("processed findings):")
    );
    let output = run(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        temp.path().join("unavailable.json").to_str().unwrap(),
        "--artifact-max-bytes",
        "100",
    ]);
    assert!(output.status.success());
    assert!(!temp.path().join("unavailable.json").exists());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("processed findings):"), "{text}");
    assert!(
        text.contains("Resource matches profile-declared fingerprint"),
        "{text}"
    );
    assert!(text.contains("Evidence artifact unavailable"), "{text}");
    let profile = temp.path().join("disabled.toml");
    fs::write(
        &profile,
        RESOURCE_PROFILE
            .split("[investigation_view]")
            .next()
            .unwrap(),
    )
    .unwrap();
    let output = run(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        temp.path().join("disabled.json").to_str().unwrap(),
    ]);
    assert!(output.status.success());
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("processed findings):")
    );
}

#[test]
fn readable_source_errors_cover_every_input_before_individual_pagination() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile.toml");
    fs::write(&profile, RESOURCE_PROFILE).unwrap();
    let manifest = resource_line(
        "run-one:operation-one",
        "manifest listing",
        json!(["asset:reference"]),
    );
    let batch = resource_line(
        "run-one",
        "asset batch",
        json!([{"bundle":{"resources":{"items":[{"address":"asset:reference","digest":"a".repeat(64)}]}}}]),
    );
    let inputs: Vec<_> = (0..4).map(|index| {
        let path = temp.path().join(format!("capture-{index}.jsonl"));
        let mut data = format!("{batch}{}", manifest.repeat(if index == 0 { 140 } else { 1 }));
        for pattern in 0..if index == 0 { 5 } else { 1 } {
            data.push_str(&format!("{}\n", json!({"timestamp":"2025-07-20T00:00:00+02:00", "level":if index == 3 {"FATAL"} else {"ERROR"}, "message":format!("render operation failed capture-{index} pattern-{pattern}")})));
        }
        fs::write(&path, data).unwrap();
        path
    }).collect();
    let artifact = temp.path().join("evidence.json");
    let mut args = vec!["--profile", profile.to_str().unwrap(), "investigate"];
    args.extend(inputs.iter().map(|path| path.to_str().unwrap()));
    args.extend(["--artifact", artifact.to_str().unwrap()]);
    let output = run(&args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("Observed ERROR/FATAL records (selected parsed population)"),
        "{text}"
    );
    for index in 0..4 {
        assert!(
            text.contains(&format!(
                "render operation failed capture-{index} pattern-0"
            )),
            "{text}"
        );
    }
    assert!(
        text.contains("[scope-0] 5 physical records; 5 distinct source messages"),
        "{text}"
    );
    assert!(
        text.contains("2 additional distinct source messages not displayed here"),
        "{text}"
    );
    assert!(!text.contains("capture-0 pattern-3"), "{text}");
    assert!(!text.contains("capture-0 pattern-4"), "{text}");
    assert!(
        text.find("Observed ERROR/FATAL").unwrap()
            < text
                .find("Resource observations (processed findings)")
                .unwrap()
    );
    let stored: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    let findings = stored["findings"].as_array().unwrap();
    let late = findings
        .iter()
        .position(|finding| {
            finding["evidence"][0]["text"] == "render operation failed capture-3 pattern-0"
        })
        .unwrap();
    assert!(late >= 140, "errors must still be retained beyond page 7");
    for index in 0..4 {
        let finding = findings
            .iter()
            .find(|finding| {
                finding["evidence"][0]["text"]
                    == format!("render operation failed capture-{index} pattern-0")
            })
            .unwrap();
        let reference = &finding["evidence"][0]["occurrence"]["evidence_ref"];
        assert!(
            text.contains(reference["reference_id"].as_str().unwrap()),
            "{text}"
        );
        assert!(
            text.contains(&format!("Input {}, line {}", index + 1, reference["line"])),
            "{text}"
        );
    }
    let next = text
        .lines()
        .find_map(|line| line.strip_prefix("Next: "))
        .unwrap();
    let next_args: Vec<_> = next
        .split_whitespace()
        .skip(1)
        .map(|arg| arg.trim_matches('\''))
        .collect();
    let page = success(&next_args);
    assert_eq!(page["artifact_retrieval"]["prior"], 20);
    assert_eq!(
        page["artifact_retrieval"]["items"],
        json!(&findings[20..40])
    );
    let mut json_args = vec!["--json"];
    json_args.extend(args.iter().copied());
    let json_artifact = temp.path().join("json.json");
    let last = json_args.len() - 1;
    json_args[last] = json_artifact.to_str().unwrap();
    let output = run(&json_args);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    check(&report, &json_artifact);
    assert_eq!(report["findings"], json!(&findings[..20]));
    let json_stored: Value = serde_json::from_slice(&fs::read(json_artifact).unwrap()).unwrap();
    assert_eq!(json_stored["findings"], stored["findings"]);
    let redacted_artifact = temp.path().join("redacted.json");
    let last = args.len() - 1;
    args[last] = redacted_artifact.to_str().unwrap();
    args.push("--redact");
    let output = run(&args);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Observed ERROR/FATAL records"), "{text}");
    assert!(text.contains("[REDACTED SOURCE]"), "{text}");
    assert!(!text.contains("render operation failed"), "{text}");
}

#[test]
fn fingerprint_matches_lead_the_first_page_despite_warnings_and_empty_manifests() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile.toml");
    fs::write(&profile, RESOURCE_PROFILE).unwrap();
    let source = temp.path().join("resources.jsonl");
    let scope = "run-one:operation-one";
    let mut data = String::new();
    for index in 0..260 {
        data.push_str(&format!("{}\n", json!({"timestamp":"2025-01-01T00:00:00Z","level":"WARN","message":format!("synthetic warning {index}"),"context":scope})));
        data.push_str(&resource_line(scope, "manifest listing", json!([])));
    }
    data.push_str(&resource_line(
        scope,
        "manifest listing",
        json!(["asset:reference"]),
    ));
    data.push_str(&resource_line(scope, "asset batch", json!([{"bundle":{"resources":{"items":[{"address":"asset:reference","digest":"a".repeat(64)}]}}}])));
    fs::write(&source, data).unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
    ]);
    check(&report, &artifact);
    assert_eq!(report["findings"].as_array().unwrap().len(), 20);
    assert!(report["presentation"]["omitted_findings"].as_u64().unwrap() > 500);
    assert_eq!(
        report["findings"][0]["details"]["resource_status"],
        "fingerprint_match"
    );
    assert!(
        report["findings"][0]["claim"]
            .as_str()
            .unwrap()
            .contains("synthetic reference image")
    );
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| finding["details"]["resource_status"] != "empty_manifest")
    );
    let retained: Value = serde_json::from_slice(&fs::read(artifact).unwrap()).unwrap();
    assert_eq!(
        retained["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|finding| finding["details"]["resource_status"] == "empty_manifest")
            .count(),
        260
    );
}

#[test]
fn profile_configured_resources_keep_fingerprints_different_and_missing_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile.toml");
    fs::write(&profile, RESOURCE_PROFILE).unwrap();
    let source = temp.path().join("resources.jsonl");
    let scope = "run-one:operation-one";
    let lines = [
        resource_line(scope, "dimensions", json!({"w":1920,"h":1080})),
        resource_line(
            scope,
            "manifest listing",
            json!(["asset:reference", "asset:different", "asset:missing"]),
        ),
        resource_line(
            scope,
            "asset batch",
            json!([{"bundle":{"resources":{"items":[{"address":"asset:reference","digest":"a".repeat(64)},{"address":"asset:different","digest":"b".repeat(64)}]}}}]),
        ),
    ];
    fs::write(&source, lines.concat()).unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    let claims: Vec<_> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| {
            finding["id"]
                .as_str()
                .unwrap()
                .contains("resource-observation")
        })
        .map(|finding| finding["claim"].as_str().unwrap())
        .collect();
    assert_eq!(claims.len(), 3, "{report}");
    assert!(
        claims
            .iter()
            .any(|claim| claim.contains("profile-declared fingerprint: synthetic reference image"))
    );
    assert!(claims.iter().any(|claim| claim.contains("different bytes")));
    assert!(
        claims
            .iter()
            .any(|claim| claim.contains("unavailable or ambiguous"))
    );
    assert!(
        claims
            .iter()
            .all(|claim| claim.contains("1920x1080") && claim.contains("failure cause"))
    );
    for (name, records, expected) in [
        (
            "keyed",
            vec![
                lines[0].clone(),
                resource_line(scope, "new snapshot", Value::Null),
                resource_line(scope, "manifest listing", json!(["asset:reference"])),
                resource_line(
                    scope,
                    "asset batch",
                    json!([{"bundle":{"resources":{"asset:reference":{"digest":"a".repeat(64)}}}}]),
                ),
            ],
            "profile-declared fingerprint: synthetic reference image",
        ),
        (
            "invalid",
            vec![
                lines.concat(),
                resource_line(
                    scope,
                    "asset batch",
                    json!([{"bundle":{"resources":{"asset:reference":{"digest":"invalid"}}}}]),
                ),
            ],
            "unavailable or ambiguous",
        ),
    ] {
        fs::write(&source, records.concat()).unwrap();
        let artifact = temp.path().join(format!("{name}.json"));
        let report = success(&[
            "--profile",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ]);
        check(&report, &artifact);
        let observation = report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|finding| {
                let id = finding["id"].as_str().unwrap();
                id.starts_with("scope-0-resource-observation-") && id.ends_with("-0")
            })
            .unwrap();
        assert!(observation["claim"].as_str().unwrap().contains(expected));
        if name == "keyed" {
            assert!(
                observation["claim"]
                    .as_str()
                    .unwrap()
                    .contains("Observed dimensions: unknown")
            );
        } else {
            assert!(
                !observation["claim"]
                    .as_str()
                    .unwrap()
                    .contains("synthetic reference image")
            );
        }
    }
    // Reuse within one namespace must not invent a unique originating owner.
    fs::write(
        &source,
        format!(
            "{}{}",
            lines.concat(),
            resource_line(
                "run-one:operation-two",
                "manifest listing",
                json!(["asset:reference"])
            )
        ),
    )
    .unwrap();
    let artifact = temp.path().join("ambiguous.json");
    let report = success(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    assert!(
        !report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["claim"]
                .as_str()
                .unwrap()
                .contains("synthetic reference image"))
    );
    // Removing the declaration disables resource processing without a CLI switch.
    fs::write(
        &profile,
        "extends = \"base\"\nprofile_name = \"custom-resources\"\n",
    )
    .unwrap();
    let artifact = temp.path().join("disabled.json");
    let report = success(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    assert!(
        !report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["id"]
                .as_str()
                .unwrap()
                .contains("resource-observation"))
    );
}

#[test]
fn resource_configuration_is_validated_and_no_product_flag_is_exposed() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("empty.log");
    fs::write(&source, "").unwrap();
    let profile = temp.path().join("profile.toml");
    for invalid in [
        RESOURCE_PROFILE.replace("manifest listing", ""),
        RESOURCE_PROFILE.replace(&"a".repeat(64), "invalid-sha256"),
    ] {
        fs::write(&profile, invalid).unwrap();
        let output = run(&[
            "--profile",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
        ]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Resource"));
    }
    let output = run(&["investigate", "--help"]);
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(!help.to_ascii_lowercase().contains("eyes"));
    assert!(!help.to_ascii_lowercase().contains("applitools"));
    let output = run(&["investigate", source.to_str().unwrap(), "--eyes-canvas"]);
    assert!(!output.status.success());
}

#[test]
fn evidence_large_unicode_pages_obey_exact_bytes_and_cursor_continuation() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("unicode.log");
    fs::write(
        &source,
        "2025-01-01T00:00:00Z INFO app: café 🙂 context\n".repeat(300),
    )
    .unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--preset",
        "base",
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
    ]);
    check(&report, &artifact);
    let mut cursor = None;
    let mut total = 0u64;
    loop {
        let mut args = vec![
            "evidence",
            artifact.to_str().unwrap(),
            "--expected-sha256",
            report["artifact"]["stored_sha256"].as_str().unwrap(),
            "--collection",
            "/records",
            "--report-max-items",
            "300",
            "--report-max-bytes",
            "8000",
            "--report-max-chars",
            "7900",
        ];
        if let Some(cursor) = cursor.as_deref() {
            args.extend(["--report-cursor", cursor]);
        }
        let output = run(&args);
        assert!(output.status.success());
        assert!(output.stdout.len() <= 8000);
        assert!(String::from_utf8_lossy(&output.stdout).chars().count() <= 7900);
        let page: Value = serde_json::from_slice(&output.stdout).unwrap();
        let displayed = page["artifact_retrieval"]["displayed"].as_u64().unwrap();
        assert!(displayed > 0);
        total += displayed;
        cursor = page["artifact_retrieval"]["next_cursor"]
            .as_str()
            .map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(total, 300);
}

#[test]
fn retained_normalization_coverage_accumulates_within_the_memory_budget() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile.toml");
    fs::write(&profile, format!("extends = \"base\"\n[parser]\nformat = \"json-lines\"\n[normalization]\nroot_path = \"/rows\"\nexpand_rows = true\n[normalization.fields]\n\"{}\" = \"/missing\"\n", "synthetic_field_".repeat(128))).unwrap();
    let sources: Vec<_> = (0..8)
        .map(|i| {
            let source = temp.path().join(format!("input-{i}.jsonl"));
            fs::write(
                &source,
                json!({"rows":vec![json!({});20]}).to_string() + "\n",
            )
            .unwrap();
            source
        })
        .collect();
    for (name, limit) in [("bounded", "4000000"), ("sufficient", "10000000")] {
        let artifact = temp.path().join(format!("{name}.json"));
        let mut args = vec!["--config", profile.to_str().unwrap(), "investigate"];
        args.extend(sources.iter().map(|path| path.to_str().unwrap()));
        args.extend([
            "--artifact",
            artifact.to_str().unwrap(),
            "--processing-max-memory-bytes",
            limit,
        ]);
        let report = success(&args);
        check(&report, &artifact);
        let inputs = report["report_metadata"]["evidence"]["inputs"]
            .as_array()
            .unwrap();
        assert!(!inputs.is_empty());
        assert!(inputs.iter().all(|input| {
            !input["coverage"]["normalization_diagnostics"]
                .as_array()
                .unwrap()
                .is_empty()
        }));
        let stored: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
        assert_eq!(
            stored["report_metadata"]["evidence"]["inputs"],
            report["report_metadata"]["evidence"]["inputs"]
        );
        if name == "bounded" {
            assert_eq!(
                report["processing"]["status"], "partial",
                "{}",
                report["processing"]
            );
            assert_eq!(report["processing"]["stop"]["reason"], "memory_limit");
            assert!(inputs.len() < sources.len(), "{report}");
            assert!(
                report["processing"]["inputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|input| input["capture"] == "unread")
            );
        } else {
            assert_eq!(report["processing"]["status"], "complete");
            assert_eq!(inputs.len(), sources.len());
            assert!(inputs.iter().all(|input| {
                input["coverage"]["normalization_diagnostics"]
                    .as_array()
                    .unwrap()
                    .len()
                    == 20
            }));
            assert!(
                report["processing"]["usage"]["memory_bytes"]
                    .as_u64()
                    .unwrap()
                    >= (serde_json::to_vec(inputs).unwrap().len() as u64) * 16
            );
        }
    }
}

#[test]
fn sequential_processing_releases_independent_parse_working_sets() {
    let temp = tempfile::tempdir().unwrap();
    let row = format!("2025-01-01T00:00:00Z INFO app: {}\n", "context".repeat(150));
    let sources: Vec<_> = (0..2)
        .map(|index| {
            let path = temp.path().join(format!("input-{index}.log"));
            fs::write(&path, row.repeat(1000)).unwrap();
            path
        })
        .collect();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--preset",
        "base",
        "investigate",
        sources[0].to_str().unwrap(),
        sources[1].to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--processing-max-memory-bytes",
        "300000000",
    ]);
    check(&report, &artifact);
    assert_eq!(
        report["processing"]["status"], "complete",
        "{}",
        report["processing"]
    );
    assert_eq!(report["processing"]["usage"]["records"], 2000);
    assert_eq!(
        report["report_metadata"]["evidence"]["query"]["execution"]["working_sets"],
        "sequential_independent_inputs"
    );
    let stored: Value = serde_json::from_slice(&fs::read(artifact).unwrap()).unwrap();
    assert_eq!(stored["records"].as_array().unwrap().len(), 2000);
    assert!(
        stored["records"]
            .as_array()
            .unwrap()
            .iter()
            .all(|record| record["occurrence"]["snapshot_id"]
                == report["report_metadata"]["evidence"]["snapshot_id"])
    );
}

#[test]
fn summary_and_readable_redacted_file_outputs_follow_requested_format() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("private-source.log");
    fs::write(&source, "2025-01-01T00:00:00Z ERROR app: private-message\n2025-01-01T00:00:01Z ERROR app: second error\n2025-01-01T00:00:02Z WARN app: first warning\n2025-01-01T00:00:03Z WARN app: second warning\n").unwrap();
    let artifact = temp.path().join("summary.json");
    let output = run(&[
        "--preset",
        "base",
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--summary",
    ]);
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    shape(
        &report,
        include_str!("../schemas/investigation-brief.schema.json"),
    );
    assert_eq!(report["findings"]["items"].as_array().unwrap().len(), 5);
    let artifact = temp.path().join("redacted.json");
    let destination = temp.path().join("explanation.txt");
    let output = run(&[
        "--preset",
        "base",
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--redact",
        "--output",
        destination.to_str().unwrap(),
    ]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("Investigation: complete"));
    assert!(!text.contains("private-message"));
    assert!(!text.contains("private-source.log"));
    assert_eq!(fs::read_to_string(destination).unwrap(), text);
    let stored = fs::read_to_string(artifact).unwrap();
    assert!(!stored.contains("private-message"));
}

#[test]
fn binding_final_snapshot_preserves_source_objects_that_imitate_occurrences() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.jsonl");
    let spoof = json!({"snapshot_id":"pending-snapshot","input_ordinal":0,"evidence_ref":{"reference_id":"source-data"}});
    let row = json!({"timestamp":"2025-01-01T00:00:00Z","level":"ERROR","message":"opaque source payload","payload":{"spoof":spoof}});
    fs::write(&source, format!("{row}\n")).unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--preset",
        "base",
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
    ]);
    check(&report, &artifact);
    let stored: Value = serde_json::from_slice(&fs::read(artifact).unwrap()).unwrap();
    assert_eq!(
        stored["records"][0]["fields"]["envelope_payload"]["spoof"],
        spoof
    );
    assert_eq!(
        stored["records"][0]["occurrence"]["snapshot_id"],
        report["report_metadata"]["evidence"]["snapshot_id"]
    );
}

#[test]
fn explicit_json_format_overrides_readable_investigation_default() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("source.log"),
        "2025-01-01T00:00:00Z ERROR app: synthetic failure\n",
    )
    .unwrap();
    for (index, flags) in [vec!["--json"], vec!["--format", "json"]]
        .iter()
        .enumerate()
    {
        let artifact = temp.path().join(format!("evidence-{index}.json"));
        let output = binary()
            .current_dir(temp.path())
            .args(flags)
            .args([
                "--profile",
                "base",
                "investigate",
                "source.log",
                "--artifact",
                artifact.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        check(&report, &artifact);
        assert_eq!(count(&report, "observed-errors"), 1);
    }
}

#[test]
fn severity_zero_requires_complete_parse_coverage() {
    let temp = tempfile::tempdir().unwrap();
    for (name, data, extra, expect_zero) in [
        ("unparsed", "not a log record\n", Vec::<&str>::new(), false),
        (
            "rejected",
            "2025-99-99T00:00:00Z ERROR app: invalid timestamp\n",
            vec![],
            false,
        ),
        (
            "oversize",
            "2025-01-01T00:00:00Z ERROR app: oversized\n",
            vec!["--record-max-bytes", "10"],
            false,
        ),
        ("empty", "", vec![], true),
        (
            "filtered",
            "2025-01-01T00:00:00Z ERROR app: excluded\n",
            vec!["--filter", "t:absent"],
            true,
        ),
    ] {
        let source = temp.path().join(format!("{name}.log"));
        let artifact = temp.path().join(format!("{name}.json"));
        fs::write(&source, data).unwrap();
        let mut args = vec![
            "--preset",
            "base",
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ];
        args.extend(extra);
        let report = success(&args);
        check(&report, &artifact);
        let populations: Vec<_> = report["populations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|population| population["id"].as_str().unwrap().contains("-observed-"))
            .collect();
        if expect_zero {
            assert_eq!(populations.len(), 2, "{name}: {report}");
            assert!(
                populations.iter().all(|population| population["count"] == 0
                    && population["completeness"] == "complete")
            );
        } else {
            assert!(populations.is_empty(), "{name}: {report}");
        }
    }
}

#[test]
fn redacted_resource_labels_ids_and_automatic_artifacts_remain_private_and_retrievable() {
    let temp = tempfile::tempdir().unwrap();
    let private = "private-customer-label-Ω";
    let private_rule = "private-customer-rule";
    let profile = temp.path().join("profile.toml");
    fs::write(
        &profile,
        RESOURCE_PROFILE
            .replace("synthetic reference image", private)
            .replace("id = \"assets\"", &format!("id = \"{private_rule}\"")),
    )
    .unwrap();
    let source = temp.path().join("source.log");
    fs::write(&source, [
        resource_line("run-one:operation-one", "manifest listing", json!(["asset:reference"])),
        resource_line("run-one", "asset batch", json!([{"bundle":{"resources":{"items":[{"address":"asset:reference","digest":"a".repeat(64)}]}}}])),
    ].concat()).unwrap();
    for json_output in [false, true] {
        let output_file = temp.path().join(if json_output {
            "report.json"
        } else {
            "report.txt"
        });
        let mut command = binary();
        command.current_dir(temp.path()).args([
            "--profile",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--redact",
            "--output",
            output_file.to_str().unwrap(),
        ]);
        if json_output {
            command.arg("--json");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = fs::read_to_string(&output_file).unwrap();
        let handle = if json_output {
            serde_json::from_str::<Value>(&text).unwrap()["artifact"]["location"]
                .as_str()
                .unwrap()
                .to_owned()
        } else {
            text.lines()
                .find_map(|line| line.strip_prefix("Evidence: "))
                .unwrap()
                .to_owned()
        };
        assert!(handle.starts_with("log-analyzer-evidence-"));
        assert!(!Path::new(&handle).is_absolute());
        let artifact = temp.path().join(&handle);
        let bytes = fs::read_to_string(&artifact).unwrap();
        for result in [
            text.as_str(),
            bytes.as_str(),
            String::from_utf8_lossy(&output.stdout).as_ref(),
        ] {
            assert!(!result.contains(private));
            assert!(!result.contains(private_rule));
            assert!(!result.contains(&"a".repeat(64)));
            assert!(!result.contains(temp.path().to_str().unwrap()));
        }
        let retained: Value = serde_json::from_str(&bytes).unwrap();
        assert!(
            retained["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|finding| finding["details"]["resource_status"] == "fingerprint_match")
        );
        assert!(
            retained["findings"]
                .as_array()
                .unwrap()
                .iter()
                .all(|finding| finding["details"]["fingerprint"].is_null())
        );
        let hash = if json_output {
            serde_json::from_str::<Value>(&text).unwrap()["artifact"]["stored_sha256"]
                .as_str()
                .unwrap()
                .to_owned()
        } else {
            text.lines()
                .find_map(|line| line.strip_prefix("SHA-256: "))
                .unwrap()
                .to_owned()
        };
        let retrieval = binary()
            .current_dir(temp.path())
            .args([
                "evidence",
                &handle,
                "--expected-sha256",
                &hash,
                "--collection",
                "/findings",
            ])
            .output()
            .unwrap();
        assert!(
            retrieval.status.success(),
            "{}",
            String::from_utf8_lossy(&retrieval.stderr)
        );
        let retrieved: Value = serde_json::from_slice(&retrieval.stdout).unwrap();
        assert!(!String::from_utf8_lossy(&retrieval.stdout).contains(&"a".repeat(64)));
        assert!(
            !retrieved["artifact_retrieval"]["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        if json_output {
            check(&serde_json::from_str(&text).unwrap(), &artifact);
        }
    }
}

#[test]
fn redacted_resources_omit_viewport_dimensions_for_every_status() {
    let temp = tempfile::tempdir().unwrap();
    for (status, digest) in [
        ("fingerprint_match", "a".repeat(64)),
        ("different_fingerprint", "b".repeat(64)),
        ("unconfigured_fingerprint", "b".repeat(64)),
        ("unavailable", "invalid-digest".into()),
    ] {
        let profile = temp.path().join(format!("{status}.toml"));
        let configuration = if status == "unconfigured_fingerprint" {
            RESOURCE_PROFILE.replace(
                &format!("[[resource_observations.fingerprints]]\nsha256 = \"{}\"\nlabel = \"synthetic reference image\"\n", "a".repeat(64)),
                "",
            )
        } else {
            RESOURCE_PROFILE.to_owned()
        };
        fs::write(&profile, configuration).unwrap();
        let source = temp.path().join(format!("{status}.log"));
        fs::write(&source, [
            resource_line("run-one:operation-one", "dimensions", json!({"w":987654321,"h":123456789})),
            resource_line("run-one:operation-one", "manifest listing", json!(["asset:reference"])),
            resource_line("run-one", "asset batch", json!([{"bundle":{"resources":{"items":[{"address":"asset:reference","digest":digest}]}}}])),
        ].concat()).unwrap();
        let original = success(&[
            "--profile",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            temp.path()
                .join(format!("{status}-original.json"))
                .to_str()
                .unwrap(),
        ]);
        let original_resource = original["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|finding| finding["details"].get("resource_status").is_some())
            .unwrap();
        assert_eq!(original_resource["details"]["resource_status"], status);
        assert_eq!(
            original_resource["details"]["viewport"],
            json!({"width":987654321,"height":123456789})
        );
        for (json_output, saved) in [(false, false), (false, true), (true, false), (true, true)] {
            let artifact = temp
                .path()
                .join(format!("{status}-{json_output}-{saved}.json"));
            let destination = temp.path().join("report.txt");
            let mut args = vec![
                "--profile",
                profile.to_str().unwrap(),
                "investigate",
                source.to_str().unwrap(),
                "--artifact",
                artifact.to_str().unwrap(),
                "--redact",
            ];
            if json_output {
                args.push("--json");
            }
            if saved {
                args.extend(["--output", destination.to_str().unwrap()]);
            }
            let output = run(&args);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8(output.stdout).unwrap();
            let text = if saved {
                fs::read_to_string(&destination).unwrap()
            } else {
                stdout.clone()
            };
            let retained_text = fs::read_to_string(&artifact).unwrap();
            for result in [&stdout, &text, &retained_text] {
                assert!(!result.contains("987654321"), "{status}: {result}");
                assert!(!result.contains("123456789"), "{status}: {result}");
            }
            let retained: Value = serde_json::from_str(&retained_text).unwrap();
            let resource = retained["findings"]
                .as_array()
                .unwrap()
                .iter()
                .find(|finding| finding["details"].get("resource_status").is_some())
                .unwrap();
            assert_eq!(resource["details"]["resource_status"], status);
            assert!(resource["details"]["viewport"].is_null());
            if json_output {
                check(&serde_json::from_str(&text).unwrap(), &artifact);
            }
        }
    }
}

#[test]
fn resource_observations_require_namespace_before_manifests_or_viewport_updates() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile.toml");
    fs::write(&profile, RESOURCE_PROFILE).unwrap();
    let source = temp.path().join("source.log");
    let artifact = temp.path().join("evidence.json");
    fs::write(&source, [
        resource_line("operation-one", "manifest listing", json!([])),
        resource_line("operation-one", "manifest listing", json!(["asset:owner-only"])),
        resource_line("run-one:operation-one", "dimensions", json!({"w":640,"h":480})),
        resource_line("operation-one", "dimensions", json!({"w":999,"h":999})),
        resource_line("operation-one", "new snapshot", Value::Null),
        resource_line("run-one:operation-one", "manifest listing", json!(["asset:reference"])),
        resource_line("run-one", "asset batch", json!([{"bundle":{"resources":{"items":[{"address":"asset:reference","digest":"a".repeat(64)}]}}}])),
    ].concat()).unwrap();
    let report = success(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    let observations: Vec<_> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["details"].get("resource_status").is_some())
        .collect();
    assert_eq!(observations.len(), 1, "{report}");
    assert_eq!(
        observations[0]["details"]["resource_status"],
        "fingerprint_match"
    );
    assert_eq!(
        observations[0]["details"]["viewport"],
        json!({"width":640,"height":480})
    );
}

#[test]
fn evidence_rejects_large_artifacts_before_reading_with_a_safe_default_limit() {
    let temp = tempfile::tempdir().unwrap();
    let artifact = temp.path().join("oversized.json");
    fs::File::create(&artifact)
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    let output = run(&[
        "evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        &"0".repeat(64),
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Artifact exceeds read limit"));
    assert!(output.stdout.is_empty());
    let help = run(&["evidence", "--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("67108864"));
}

#[test]
fn resource_viewports_do_not_cross_namespaces_with_a_shared_owner() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile.toml");
    fs::write(&profile, RESOURCE_PROFILE).unwrap();
    let input = temp.path().join("source.log");
    fs::write(
        &input,
        [
            resource_line(
                "operation-shared:run-a",
                "dimensions",
                json!({"w":640,"h":480}),
            ),
            resource_line(
                "operation-shared:run-b",
                "manifest listing",
                json!(["asset:reference"]),
            ),
            resource_line(
                "operation-shared:run-b",
                "dimensions",
                json!({"w":800,"h":600}),
            ),
            resource_line(
                "operation-shared:run-a",
                "manifest listing",
                json!(["asset:reference"]),
            ),
            resource_line("operation-shared:run-b", "new snapshot", Value::Null),
            resource_line(
                "operation-shared:run-a",
                "manifest listing",
                json!(["asset:reference"]),
            ),
            resource_line(
                "operation-shared:run-b",
                "manifest listing",
                json!(["asset:reference"]),
            ),
            resource_line(
                "operation-shared:run-b",
                "dimensions",
                json!({"w":1024,"h":768}),
            ),
            resource_line(
                "operation-shared:run-b",
                "manifest listing",
                json!(["asset:reference"]),
            ),
        ]
        .concat(),
    )
    .unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        input.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    let observations: Vec<_> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["details"].get("resource_status").is_some())
        .collect();
    assert_eq!(observations.len(), 5);
    for (line, viewport, expected_lines) in [
        (2, Value::Null, vec![2]),
        (4, json!({"width":640,"height":480}), vec![1, 4]),
        (6, json!({"width":640,"height":480}), vec![1, 6]),
        (7, Value::Null, vec![7]),
        (9, json!({"width":1024,"height":768}), vec![8, 9]),
    ] {
        let finding = observations
            .iter()
            .find(|finding| finding["evidence"][0]["occurrence"]["evidence_ref"]["line"] == line)
            .unwrap();
        assert_eq!(finding["details"]["viewport"], viewport);
        let mut lines: Vec<_> = finding["details"]["supporting_occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .map(|occurrence| occurrence["evidence_ref"]["line"].as_u64().unwrap())
            .collect();
        lines.sort_unstable();
        assert_eq!(lines, expected_lines);
    }
}

#[test]
fn resource_hashes_and_citations_never_cross_manifest_owners() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile.toml");
    fs::write(&profile, RESOURCE_PROFILE).unwrap();
    for keyed in [false, true] {
        for (name, batches, expected) in [
            (
                "foreign-only",
                vec![("run-one:operation-two", "a")],
                "unavailable",
            ),
            (
                "matching-owner",
                vec![("run-one:operation-one", "a")],
                "fingerprint_match",
            ),
            (
                "foreign-conflict",
                vec![
                    ("run-one:operation-one", "a"),
                    ("run-one:operation-two", "b"),
                ],
                "fingerprint_match",
            ),
            (
                "foreign-fingerprint",
                vec![
                    ("run-one:operation-one", "b"),
                    ("run-one:operation-two", "a"),
                ],
                "different_fingerprint",
            ),
            (
                "namespace-only",
                vec![("run-one", "a")],
                "fingerprint_match",
            ),
            (
                "namespace-and-foreign",
                vec![("run-one", "a"), ("run-one:operation-two", "b")],
                "fingerprint_match",
            ),
            (
                "different-namespace",
                vec![("run-two:operation-one", "a")],
                "unavailable",
            ),
        ] {
            let mut rows = vec![
                resource_line(
                    "run-one:operation-one",
                    "dimensions",
                    json!({"w":640,"h":480}),
                ),
                resource_line(
                    "run-one:operation-one",
                    "manifest listing",
                    json!(["asset:reference"]),
                ),
            ];
            let mut eligible_lines = vec![1, 2];
            for (scope, hash) in batches {
                let resources = if keyed {
                    json!({"asset:reference":{"digest":hash.repeat(64)}})
                } else {
                    json!({"items":[{"address":"asset:reference","digest":hash.repeat(64)}]})
                };
                rows.push(resource_line(
                    scope,
                    "asset batch",
                    json!([{"bundle":{"resources":resources}}]),
                ));
                if scope == "run-one" || scope == "run-one:operation-one" {
                    eligible_lines.push(rows.len() as u64);
                }
            }
            let source = temp.path().join(format!("{name}-{keyed}.log"));
            let artifact = temp.path().join(format!("{name}-{keyed}.json"));
            fs::write(&source, rows.concat()).unwrap();
            let report = success(&[
                "--profile",
                profile.to_str().unwrap(),
                "investigate",
                source.to_str().unwrap(),
                "--artifact",
                artifact.to_str().unwrap(),
                "--complete-output",
            ]);
            check(&report, &artifact);
            let finding = report["findings"]
                .as_array()
                .unwrap()
                .iter()
                .find(|finding| finding["details"].get("resource_status").is_some())
                .unwrap();
            assert_eq!(
                finding["details"]["resource_status"], expected,
                "{name}-{keyed}: {finding}"
            );
            assert_eq!(
                finding["details"]["viewport"],
                json!({"width":640,"height":480})
            );
            let evidence_lines: Vec<_> = finding["evidence"]
                .as_array()
                .unwrap()
                .iter()
                .map(|excerpt| {
                    excerpt["occurrence"]["evidence_ref"]["line"]
                        .as_u64()
                        .unwrap()
                })
                .collect();
            assert_eq!(evidence_lines.len(), eligible_lines.len());
            assert!(
                evidence_lines
                    .iter()
                    .all(|line| eligible_lines.contains(line))
            );
            let supporting_lines: Vec<_> = finding["details"]["supporting_occurrences"]
                .as_array()
                .unwrap()
                .iter()
                .map(|occurrence| occurrence["evidence_ref"]["line"].as_u64().unwrap())
                .collect();
            assert_eq!(supporting_lines, evidence_lines);
            let retained: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
            assert!(retained["findings"].as_array().unwrap().contains(finding));
        }
    }
}

#[test]
#[cfg(unix)]
fn readable_paths_escape_controls_and_next_command_preserves_the_artifact_path() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source-\u{1b}[2J\nΩ.log");
    let artifact = temp.path().join("evidence-\u{1b}[2J\n'\\Ω.json");
    fs::write(
        &source,
        "2025-01-01T00:00:00Z ERROR app: synthetic failure\n",
    )
    .unwrap();
    let output = run(&[
        "--preset",
        "base",
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.chars().any(|c| c.is_control() && c != '\n'));
    assert!(text.contains("source-\\u{1b}[2J\\nΩ.log"), "{text}");
    assert!(text.contains("evidence-\\u{1b}[2J\\n"), "{text}");
    let next = text
        .lines()
        .find_map(|line| line.strip_prefix("Next: "))
        .unwrap();
    assert!(next.contains("\\033[2J\\012"), "{next}");
    let bin_dir = Path::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .parent()
        .unwrap();
    let mut paths = vec![bin_dir.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let retrieval = Command::new("/bin/bash")
        .args(["-c", next])
        .env("PATH", std::env::join_paths(paths).unwrap())
        .output()
        .unwrap();
    assert!(
        retrieval.status.success(),
        "{}",
        String::from_utf8_lossy(&retrieval.stderr)
    );
    let retrieved: Value = serde_json::from_slice(&retrieval.stdout).unwrap();
    assert!(
        !retrieved["artifact_retrieval"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let retained: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    assert_eq!(
        retained["report_metadata"]["evidence"]["inputs"][0]["file"],
        source.to_str().unwrap()
    );
}

#[test]
fn readable_evidence_escapes_unicode_controls_without_changing_retained_text() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("controls.jsonl");
    let message: String = (0..=0x9f)
        .filter_map(char::from_u32)
        .filter(|character| character.is_control())
        .collect();
    let message = format!("synthetic Ω🙂 {message}[2J");
    fs::write(
        &source,
        ["ERROR", "WARN"]
            .map(|level| {
                json!({"timestamp":"2025-01-01T00:00:00Z", "level":level, "message":message})
                    .to_string()
            })
            .join("\n")
            + "\n",
    )
    .unwrap();
    let escaped = message
        .chars()
        .fold(String::new(), |mut result, character| {
            if character.is_control() {
                result.extend(character.escape_default());
            } else {
                result.push(character);
            }
            result
        });
    for saved in [false, true] {
        let artifact = temp.path().join(format!("evidence-{saved}.json"));
        let report_path = temp.path().join("report.txt");
        let mut args = vec![
            "--preset",
            "base",
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
        ];
        if saved {
            args.extend(["--output", report_path.to_str().unwrap()]);
        }
        let output = run(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = if saved {
            fs::read_to_string(&report_path).unwrap()
        } else {
            String::from_utf8(output.stdout).unwrap()
        };
        assert!(text.contains(&escaped), "{text}");
        assert!(text.contains("Observed ERROR/FATAL records"));
        assert!(
            !text
                .chars()
                .any(|character| character.is_control() && character != '\n')
        );
        let retained: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
        let excerpts: Vec<_> = retained["findings"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|finding| finding["evidence"].as_array().unwrap())
            .collect();
        assert_eq!(excerpts.len(), 2);
        assert!(excerpts.iter().all(|excerpt| excerpt["text"] == message));
        let report = success(&[
            "--preset",
            "base",
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            temp.path()
                .join(format!("json-{saved}.json"))
                .to_str()
                .unwrap(),
            "--complete-output",
        ]);
        assert!(
            report["findings"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|finding| finding["evidence"].as_array().unwrap())
                .all(|excerpt| excerpt["text"] == message)
        );
    }
}

#[test]
fn resource_evidence_retains_all_normalized_row_paths_and_deduplicates_exact_rows() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile.toml");
    fs::write(
        &profile,
        format!("{RESOURCE_PROFILE}\n[normalization]\nroot_path = \"/rows\"\nexpand_rows = true\n"),
    )
    .unwrap();
    for (name, sibling_hash, status) in [
        ("conflict", "b".repeat(64), "unavailable"),
        ("invalid", "invalid-hash".into(), "unavailable"),
        ("identical", "a".repeat(64), "fingerprint_match"),
    ] {
        let resource = |hash: &str| json!({"address":"asset:reference", "digest":hash});
        let rows: Vec<Value> = [
            resource_line("run-one:operation-one", "dimensions", json!({"w":640,"h":480})),
            resource_line("run-one:operation-one", "manifest listing", json!(["asset:reference"])),
            resource_line("run-one:operation-one", "asset batch", json!([{"bundle":{"resources":{"items":[resource(&"a".repeat(64)),resource(&"a".repeat(64))]}}}])),
            resource_line("run-one:operation-one", "asset batch", json!([{"bundle":{"resources":{"items":[resource(&sibling_hash)]}}}])),
        ].iter().map(|row| serde_json::from_str(row).unwrap()).collect();
        let source = temp.path().join(format!("{name}.log"));
        let artifact = temp.path().join(format!("{name}.json"));
        fs::write(&source, json!({"rows":rows}).to_string() + "\n").unwrap();
        let report = success(&[
            "--profile",
            profile.to_str().unwrap(),
            "investigate",
            source.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ]);
        check(&report, &artifact);
        let finding = report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|finding| finding["details"].get("resource_status").is_some())
            .unwrap();
        assert_eq!(finding["details"]["resource_status"], status);
        let support = finding["details"]["supporting_occurrences"]
            .as_array()
            .unwrap();
        assert_eq!(support.len(), 4, "{finding}");
        assert!(
            support
                .iter()
                .all(|occurrence| occurrence["evidence_ref"]["line"] == 1)
        );
        let paths: std::collections::BTreeSet<_> = support
            .iter()
            .map(|occurrence| occurrence["evidence_ref"]["row_path"].as_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            std::collections::BTreeSet::from(["/rows/0", "/rows/1", "/rows/2", "/rows/3"])
        );
        let references: std::collections::BTreeSet<_> = support
            .iter()
            .map(|occurrence| occurrence["evidence_ref"]["reference_id"].as_str().unwrap())
            .collect();
        assert_eq!(references.len(), 4);
        let evidence = finding["evidence"].as_array().unwrap();
        assert_eq!(evidence.len(), 4);
        assert!(
            evidence
                .iter()
                .all(|excerpt| support.contains(&excerpt["occurrence"]))
        );
        let retained: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
        assert!(retained["findings"].as_array().unwrap().contains(finding));
    }
}

fn chain_fixture(events: &[Value], profile: Option<&str>) -> (tempfile::TempDir, Value, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("profile.toml");
    fs::write(
        &config,
        profile.unwrap_or(include_str!(
            "../examples/investigations/chains/profile.toml"
        )),
    )
    .unwrap();
    let input = temp.path().join("events.jsonl");
    fs::write(
        &input,
        events.iter().map(|e| format!("{e}\n")).collect::<String>(),
    )
    .unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--config",
        config.to_str().unwrap(),
        "investigate",
        input.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    (temp, report, artifact)
}
fn chain_event(event: &str, session: &str, capture: &str, poll: usize) -> Value {
    json!({"timestamp":"2026-01-01T00:00:00Z","level":"INFO","message":event,"record_type":"capture","event_type":event,"session_id":session,"capture_id":capture,"request_id":"request-1","broker_id":"broker-1","poll_id":poll.to_string(),"payload":{"asExpected":false,"status":"Passed"}})
}
fn event_chains(report: &Value) -> Vec<&Value> {
    report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["details"].get("event_chain"))
        .collect()
}
fn chain_step<'a>(chain: &'a Value, id: &str) -> &'a Value {
    chain["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap()
}
#[test]
fn connected_chains_separate_poll_sends_acceptance_completion_and_outcome_levels() {
    let mut events = vec![
        chain_event("checkpoint", "run-1", "capture-8", 0),
        chain_event("submitted", "run-1", "capture-8", 0),
        chain_event("accepted", "run-1", "capture-8", 0),
    ];
    events.extend((0..321).map(|i| chain_event("poll_send", "run-1", "capture-8", i)));
    events.push(chain_event("check_end", "run-1", "capture-8", 0));
    events.push(chain_event("session_end", "run-1", "different-capture", 0));
    events.push(chain_event("contrary", "run-1", "capture-8", 0));
    events.push(chain_event("later_activity", "run-1", "capture-9", 0));
    let (temp, report, artifact) = chain_fixture(&events, None);
    let chains = event_chains(&report);
    assert_eq!(chains.len(), 1);
    let chain = chains[0];
    assert_eq!(chain_step(chain, "polls")["observed_count"], 321);
    assert_eq!(chain_step(chain, "polls")["omitted_occurrences"], 319);
    assert_eq!(chain_step(chain, "responses")["observed_count"], 0);
    assert_eq!(chain_step(chain, "accepted")["observed_count"], 1);
    assert_eq!(chain_step(chain, "capture-end")["status"], "not_observed");
    assert_eq!(
        chain_step(chain, "check-end")["outcomes"][0]["value"],
        false
    );
    assert_eq!(
        chain_step(chain, "check-end")["outcomes"][0]["level"],
        "check"
    );
    assert_eq!(
        chain_step(chain, "session-end")["outcomes"][0]["value"],
        "Passed"
    );
    assert_eq!(
        chain_step(chain, "session-end")["outcomes"][0]["level"],
        "session"
    );
    assert_eq!(chain_step(chain, "contrary")["observed_count"], 1);
    assert_eq!(
        chain["later_activity"][0]["occurrence"]["evidence_ref"]["line"],
        328
    );
    assert_eq!(
        chain_step(chain, "accepted")["join_samples"][0]["identity"][1]["value"],
        "request-1"
    );
    assert!(report["findings"][0]["details"]["event_chain"].is_object());
    let readable = run(&[
        "--config",
        temp.path().join("profile.toml").to_str().unwrap(),
        "investigate",
        temp.path().join("events.jsonl").to_str().unwrap(),
        "--artifact",
        temp.path().join("text.json").to_str().unwrap(),
    ]);
    assert!(
        readable.status.success(),
        "{}",
        String::from_utf8_lossy(&readable.stderr)
    );
    let text = String::from_utf8(readable.stdout).unwrap();
    assert!(text.contains("polls sent: 321 observed"), "{text}");
    assert!(text.contains("responses observed: 0 observed"));
    assert!(text.contains("capture completion: 0 observed, 0 linked; not observed"));
    let retrieved = success(&[
        "evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        report["artifact"]["stored_sha256"].as_str().unwrap(),
        "--collection",
        "/findings",
        "--id",
        report["findings"][0]["id"].as_str().unwrap(),
    ]);
    assert!(retrieved.to_string().contains("event_chain"));
}
#[test]
fn connected_chains_abstain_on_reused_roots_and_keep_input_and_session_scope() {
    let events = vec![
        chain_event("checkpoint", "run-1", "capture-1", 0),
        chain_event("checkpoint", "run-1", "capture-1", 0),
        chain_event("submitted", "run-1", "capture-1", 0),
        chain_event("submitted", "run-2", "capture-1", 0),
    ];
    let (_temp, report, _artifact) = chain_fixture(&events, None);
    let chain = event_chains(&report)[0];
    assert_eq!(chain["ambiguity"], "reused_root_identity");
    assert_eq!(chain_step(chain, "submitted")["status"], "ambiguous");
    assert_eq!(chain_step(chain, "submitted")["observed_count"], 1);
    assert_eq!(chain_step(chain, "submitted")["linked_count"], 0);
    assert_eq!(
        chain_step(chain, "submitted")["ambiguity"]["excluded_target_occurrences"],
        1
    );
    let events = vec![
        chain_event("checkpoint", "run-1", "capture-1", 0),
        chain_event("submitted", "run-2", "capture-1", 0),
    ];
    let (temp, report, _artifact) = chain_fixture(&events, None);
    assert_eq!(
        chain_step(event_chains(&report)[0], "submitted")["observed_count"],
        0
    );
    let output = success(&[
        "--config",
        temp.path().join("profile.toml").to_str().unwrap(),
        "investigate",
        temp.path().join("events.jsonl").to_str().unwrap(),
        temp.path().join("events.jsonl").to_str().unwrap(),
        "--artifact",
        temp.path().join("two.json").to_str().unwrap(),
        "--complete-output",
    ]);
    assert_eq!(event_chains(&output).len(), 2);
    assert_eq!(
        chain_step(event_chains(&output)[1], "submitted")["observed_count"],
        0
    );
}
#[test]
fn connected_chains_redact_nested_bundles_labels_join_values_and_outcomes() {
    let events: Vec<_> = [
        "checkpoint",
        "submitted",
        "accepted",
        "check_end",
        "session_end",
        "later_activity",
    ]
    .iter()
    .map(|e| chain_event(e, "secret-session", "secret-capture", 0))
    .collect();
    let (temp, _report, _artifact) = chain_fixture(&events, None);
    let artifact = temp.path().join("redacted.json");
    let report = success(&[
        "--redact",
        "--config",
        temp.path().join("profile.toml").to_str().unwrap(),
        "investigate",
        temp.path().join("events.jsonl").to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    let retained = fs::read_to_string(&artifact).unwrap();
    for secret in [
        "secret-session",
        "secret-capture",
        "Checkpoint",
        "broker accepted",
        "Passed",
    ] {
        assert!(!retained.contains(secret), "leaked {secret}");
    }
    let chain = event_chains(&report)[0];
    assert_eq!(
        chain["steps"][0]["evidence"][0]["text"],
        "[REDACTED SOURCE]"
    );
}

#[test]
fn connected_chains_keep_intermediate_contrary_outcomes_in_small_bundles() {
    let mut events = vec![chain_event("checkpoint", "run-1", "capture-1", 0)];
    for expected in [true, false, true] {
        let mut event = chain_event("check_end", "run-1", "capture-1", 0);
        event["payload"]["asExpected"] = json!(expected);
        events.push(event);
    }
    let (_temp, report, _artifact) = chain_fixture(&events, None);
    let step = chain_step(event_chains(&report)[0], "check-end");
    assert_eq!(step["evidence"].as_array().unwrap().len(), 3);
    assert!(
        step["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["value"] == false)
    );
    assert_eq!(step["join_samples"].as_array().unwrap().len(), 3);
    assert_eq!(step["omitted_outcomes"], 0);
    let mut corrupt = report.clone();
    corrupt["findings"][0]["details"]["event_chain"]["steps"][0]["observed_count"] = json!(99);
    assert!(log_analyzer::investigation::validate_relations(&corrupt, None).is_err());
    let mut overflowing = report.clone();
    let step = &mut overflowing["findings"][0]["details"]["event_chain"]["steps"][0];
    step["observed_count"] = json!(u64::MAX);
    step["linked_count"] = json!(u64::MAX);
    step["ambiguity"]["excluded_target_occurrences"] = json!(1);
    assert!(log_analyzer::investigation::validate_relations(&overflowing, None).is_err());
    let mut corrupt = report.clone();
    corrupt["findings"][0]["details"]["event_chain"]["root"]["input_ordinal"] = json!(99);
    assert!(log_analyzer::investigation::validate_relations(&corrupt, None).is_err());
}
#[test]
fn connected_chains_canonicalize_scalar_join_values_without_guessing_other_types() {
    let profile = include_str!("../examples/investigations/chains/profile.toml")
        .split("[[investigation.chains.steps]]")
        .take(2)
        .collect::<Vec<_>>()
        .join("[[investigation.chains.steps]]")
        .replace(
            "join_fields = [\"capture_id\"]",
            "join_fields = [\"payload.join_id\"]",
        );
    for (source, target, linked) in [
        (json!(1), json!("1"), 1),
        (json!(true), json!("true"), 1),
        (json!(1.5), json!("1.5"), 1),
        (json!(1), json!("01"), 0),
        (Value::Null, Value::Null, 0),
        (json!([1]), json!([1]), 0),
        (json!({"id":1}), json!({"id":1}), 0),
        (json!(""), json!(""), 0),
    ] {
        let mut events = vec![
            chain_event("checkpoint", "run-1", "capture-1", 0),
            chain_event("submitted", "run-1", "capture-1", 0),
        ];
        events[0]["payload"]["join_id"] = source.clone();
        events[1]["payload"]["join_id"] = target.clone();
        let (_temp, report, _artifact) = chain_fixture(&events, Some(&profile));
        let step = chain_step(event_chains(&report)[0], "submitted");
        assert_eq!(step["linked_count"], linked, "{source} -> {target}");
        if linked == 1 {
            assert_eq!(step["join_samples"][0]["identity"][0]["value"], source);
            assert_eq!(step["status"], "observed");
        }
    }
}

#[test]
fn connected_chains_preserve_expansion_order_for_roots_and_role_endpoints() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile.toml");
    fs::write(
        &profile,
        format!(
            "{}\n[normalization]\nroot_path = \"/rows\"\nexpand_rows = true\n",
            include_str!("../examples/investigations/chains/profile.toml")
        ),
    )
    .unwrap();
    let mut endpoints = vec![
        chain_event("checkpoint", "run-1", "capture-1", 0),
        chain_event("submitted", "run-1", "capture-1", 0),
        chain_event("accepted", "run-1", "capture-1", 0),
    ];
    endpoints.extend((0..12).map(|i| chain_event("poll_send", "run-1", "capture-1", i)));
    let roots: Vec<_> = (0..12)
        .map(|i| chain_event("checkpoint", "run-1", &format!("capture-{i}"), 0))
        .collect();
    for (name, rows) in [("endpoints", endpoints), ("roots", roots)] {
        let input = temp.path().join(format!("{name}.jsonl"));
        let artifact = temp.path().join(format!("{name}.json"));
        fs::write(&input, json!({"rows":rows}).to_string() + "\n").unwrap();
        let report = success(&[
            "--profile",
            profile.to_str().unwrap(),
            "investigate",
            input.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ]);
        check(&report, &artifact);
        let chains = event_chains(&report);
        if name == "endpoints" {
            let step = chain_step(chains[0], "polls");
            assert_eq!(step["linked_count"], 12);
            assert_eq!(step["omitted_occurrences"], 10);
            assert_eq!(
                step["evidence"][0]["occurrence"]["evidence_ref"]["row_path"],
                "/rows/3"
            );
            assert_eq!(
                step["evidence"][1]["occurrence"]["evidence_ref"]["row_path"],
                "/rows/14"
            );
        } else {
            assert_eq!(chains.len(), 12);
            for (i, chain) in chains.iter().enumerate() {
                assert_eq!(chain["root"]["evidence_ref"]["line"], 1);
                assert_eq!(
                    chain["root"]["evidence_ref"]["row_path"],
                    format!("/rows/{i}")
                );
            }
        }
    }
}

#[test]
fn connected_chains_keep_later_expanded_rows_in_the_same_context() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile.toml");
    fs::write(
        &profile,
        format!(
            "{}\n[normalization]\nroot_path = \"/rows\"\nexpand_rows = true\n",
            include_str!("../examples/investigations/chains/profile.toml")
        ),
    )
    .unwrap();
    let mut rows = vec![
        chain_event("checkpoint", "run-1", "capture-1", 0),
        chain_event("submitted", "run-1", "capture-1", 0),
        chain_event("accepted", "run-1", "capture-1", 0),
    ];
    rows.extend((0..12).map(|i| chain_event("poll_send", "run-1", "capture-1", i)));
    rows.push(chain_event("later_activity", "run-1", "capture-2", 0));
    rows.push(chain_event("later_activity", "run-2", "capture-1", 0));
    rows.extend((0..12).map(|i| chain_event("later_activity", "run-1", "capture-3", i)));
    let input = temp.path().join("events.jsonl");
    fs::write(&input, json!({"rows":rows}).to_string() + "\n").unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--profile",
        profile.to_str().unwrap(),
        "investigate",
        input.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    let chain = event_chains(&report)[0];
    assert_eq!(chain["later_activity_observed_count"], 13);
    assert_eq!(chain["omitted_later_activity"], 10);
    let later = chain["later_activity"].as_array().unwrap();
    assert_eq!(later.len(), 3);
    for (sample, row) in later.iter().zip(26..29) {
        assert_eq!(sample["occurrence"]["evidence_ref"]["line"], 1);
        assert_eq!(
            sample["occurrence"]["evidence_ref"]["row_path"],
            format!("/rows/{row}")
        );
        assert!(
            report["findings"][0]["evidence"]
                .as_array()
                .unwrap()
                .contains(sample)
        );
    }
}

#[test]
fn connected_chains_missing_root_context_keep_links_but_report_partial_analysis() {
    let profile = include_str!("../examples/investigations/chains/profile.toml");
    let profile = profile.replace(
        "context_fields = [\"session_id\"]",
        "context_fields = [\"session_id\", \"worker_id\"]",
    );
    let mut events = vec![
        chain_event("checkpoint", "run-1", "capture-1", 0),
        chain_event("submitted", "run-1", "capture-1", 0),
        chain_event("accepted", "run-1", "capture-1", 0),
        chain_event("poll_send", "run-1", "capture-1", 1),
        chain_event("poll_response", "run-1", "capture-1", 1),
        chain_event("capture_end", "run-1", "capture-1", 0),
        chain_event("check_end", "run-1", "capture-1", 0),
        chain_event("session_end", "run-1", "capture-1", 0),
        chain_event("contrary", "run-1", "capture-1", 0),
        chain_event("later_activity", "run-1", "capture-1", 0),
    ];
    for event in &mut events {
        event["worker_id"] = json!("worker-1");
    }
    let (_temp, complete, _artifact) = chain_fixture(&events, Some(&profile));
    let complete = event_chains(&complete)[0];
    assert_eq!(complete["completeness"], "complete");
    assert_eq!(complete["later_activity_observed_count"], 1);
    events[0].as_object_mut().unwrap().remove("worker_id");
    let (_temp, partial, _artifact) = chain_fixture(&events, Some(&profile));
    let partial = event_chains(&partial)[0];
    assert_eq!(partial["completeness"], "partial");
    assert!(partial["later_activity_observed_count"].is_null());
    assert!(partial["omitted_later_activity"].is_null());
    assert!(partial["later_activity"].as_array().unwrap().is_empty());
    for step in partial["steps"].as_array().unwrap() {
        assert_eq!(step["linked_count"], 1);
        assert_eq!(step["status"], "observed");
        assert_eq!(step["count_completeness"], "partial");
    }
}

#[test]
fn connected_chains_missing_join_identity_is_partial_and_never_guessed() {
    let mut submission = chain_event("submitted", "run-1", "capture-1", 0);
    submission.as_object_mut().unwrap().remove("request_id");
    let events = vec![
        chain_event("checkpoint", "run-1", "capture-1", 0),
        submission,
        chain_event("accepted", "run-1", "capture-1", 0),
    ];
    let (_temp, report, _artifact) = chain_fixture(&events, None);
    let step = chain_step(event_chains(&report)[0], "accepted");
    assert_eq!(step["observed_count"], 0);
    assert_eq!(step["count_completeness"], "partial");
    assert_eq!(step["ambiguity"]["source_occurrences_missing_identity"], 1);
    let mut events = vec![
        chain_event("checkpoint", "run-1", "capture-1", 0),
        chain_event("submitted", "run-1", "capture-1", 0),
        chain_event("accepted", "run-1", "capture-1", 0),
        chain_event("poll_send", "run-1", "capture-1", 1),
        chain_event("poll_response", "run-1", "capture-1", 1),
    ];
    let mut missing = chain_event("poll_send", "run-1", "capture-1", 2);
    missing.as_object_mut().unwrap().remove("broker_id");
    events.push(missing);
    let (_temp, report, _artifact) = chain_fixture(&events, None);
    let chain = event_chains(&report)[0];
    assert_eq!(chain_step(chain, "polls")["count_completeness"], "partial");
    assert_eq!(chain_step(chain, "responses")["observed_count"], 1);
    assert_eq!(
        chain_step(chain, "responses")["count_completeness"],
        "partial"
    );
    assert_eq!(chain["completeness"], "partial");
}
#[test]
fn shipped_profile_chains_count_http_sends_and_responses_without_wrappers_or_retries() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("generic.log");
    fs::write(&input,concat!(
        "core-ufg (manager-a/eyes-a/check-a) | 2026-01-01T00:00:00Z [INFO ] Command \"check\" is called\n",
        "core-ufg (manager-a/eyes-a/check-a) | 2026-01-01T00:00:01Z [INFO ] Command \"takeSnapshots\" is called\n",
        "ufg-requests (manager-a/eyes-a/check-a/environment-a & manager-a/eyes-a/check-a/environment-b) | 2026-01-01T00:00:02Z [INFO ] Request \"startRenders\" finished successfully with body [{\"status\":\"rendering\"}]\n",
        "ufg-requests (manager-a/eyes-a/check-a/environment-a & manager-a/eyes-a/check-a/environment-b) | 2026-01-01T00:00:03Z [INFO ] Request \"getRenderResults\" [poll-1] will be sent\n",
        "ufg-requests (manager-a/eyes-a/check-a/environment-a & manager-a/eyes-a/check-a/environment-b) | 2026-01-01T00:00:04Z [INFO ] Request \"getRenderResults\" [poll-1] that was sent to the address \"[POST]https://example.invalid/status\" respond with OK(200) \t\r\n",
        "ufg-requests (manager-a/eyes-a/check-a/environment-a) | 2026-01-01T00:00:05Z [INFO ] Request \"getRenderResults\" finished successfully with body []\n",
        "ufg-requests (manager-a/eyes-a/check-a/environment-a) | 2026-01-01T00:00:06Z [INFO ] Request \"getRenderResults\" [poll-1] is going to retried due to timeout\n",
        "core-requests (manager-a/eyes-a/check-a/environment-a) | 2026-01-01T00:00:07Z [INFO ] Request \"check\" finished successfully with body {\"asExpected\":false}\n",
        "core-requests (manager-a/eyes-a/check-a/environment-a) | 2026-01-01T00:00:08Z [INFO ] Request \"close\" finished successfully with body {\"status\":\"Passed\"}\n",
        "ufg-requests (manager-a/eyes-a/check-a/environment-a & manager-a/eyes-a/check-b/environment-b) | 2026-01-01T00:00:09Z [INFO ] Request \"getRenderResults\" [poll-2] will be sent\n",
    )).unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--profile",
        "eyes",
        "investigate",
        input.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    let chain = event_chains(&report)[0];
    assert_eq!(chain_step(chain, "render-accepted")["observed_count"], 1);
    assert_eq!(chain_step(chain, "polls")["observed_count"], 1);
    assert_eq!(chain_step(chain, "responses")["observed_count"], 1);
    assert_eq!(
        chain_step(chain, "polls")["ambiguity"]["role_occurrences_missing_identity"],
        1
    );
    assert_eq!(chain_step(chain, "capture-end")["status"], "not_observed");
    assert_eq!(chain_step(chain, "check-end")["status"], "not_observed");
    assert_eq!(
        chain_step(chain, "check-result")["outcomes"][0]["value"],
        false
    );
    assert_eq!(
        chain_step(chain, "session-result")["outcomes"][0]["value"],
        "Passed"
    );
}
#[test]
fn connected_chain_configuration_rejects_forward_links_and_unscoped_context() {
    let temp = tempfile::tempdir().unwrap();
    for config in [
        include_str!("../examples/investigations/chains/profile.toml")
            .replace("from = \"submitted\"", "from = \"responses\""),
        include_str!("../examples/investigations/chains/profile.toml").replace(
            "context_fields = [\"session_id\"]",
            "context_fields = [\"capture_id\"]",
        ),
    ] {
        let path = temp.path().join("bad.toml");
        fs::write(&path, config).unwrap();
        let output = run(&[
            "--config",
            path.to_str().unwrap(),
            "investigate",
            "examples/investigations/chains/events.jsonl",
            "--artifact",
            temp.path().join("evidence.json").to_str().unwrap(),
        ]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("investigation policy"));
    }
}

#[test]
fn connected_chains_preserve_unfinished_eighth_check_and_paginate_whole_chains() {
    let mut events = Vec::new();
    for index in 1..=8 {
        events.push(chain_event(
            "checkpoint",
            "run-1",
            &format!("capture-{index}"),
            0,
        ));
        if index < 8 {
            events.push(chain_event(
                "check_end",
                "run-1",
                &format!("capture-{index}"),
                0,
            ));
        }
    }
    events.push(chain_event("session_end", "run-1", "different-capture", 0));
    events.extend((0..12).map(|i| chain_event("later_activity", "run-1", "other-capture", i)));
    let (temp, report, _artifact) = chain_fixture(&events, None);
    let chains = event_chains(&report);
    assert_eq!(chains.len(), 8);
    for chain in &chains[..7] {
        assert_eq!(chain_step(chain, "check-end")["status"], "observed");
    }
    assert_eq!(chain_step(chains[7], "check-end")["status"], "not_observed");
    assert_eq!(
        chain_step(chains[7], "session-end")["outcomes"][0]["value"],
        "Passed"
    );
    assert_eq!(chains[7]["later_activity_observed_count"], 12);
    assert_eq!(chains[7]["omitted_later_activity"], 9);
    let artifact = temp.path().join("paged.json");
    let brief = success(&[
        "--config",
        temp.path().join("profile.toml").to_str().unwrap(),
        "investigate",
        temp.path().join("events.jsonl").to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--summary",
        "--report-max-items",
        "1",
    ]);
    assert_eq!(
        brief["findings"]["items"][0]["details"]["event_chain"]["identity"][0]["value"],
        "capture-1"
    );
    let action = brief["guidance"]["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["kind"] == "retrieve_findings")
        .unwrap();
    let argv = action["argv"]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
        .map(|v| v.as_str().unwrap())
        .collect::<Vec<_>>();
    let next = success(&argv);
    assert_ne!(next["items"][0]["id"], brief["findings"]["items"][0]["id"]);
}

#[test]
fn checkpoint_component_labels_do_not_change_scoped_recognition() {
    for component in ["core-ufg", "core-002", "core-classic"] {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("alias.log");
        fs::write(&input, format!(concat!(
            "{} (manager-a/eyes-a/check-a) | 2026-01-01T00:00:00Z [INFO ] Command \"check\" is called\n",
            "nml-client (manager-a/eyes-a/check-a) | 2026-01-01T00:00:01Z [INFO ] Command \"takeScreenshots\" is called\n",
            "nml-client (manager-b/eyes-a/check-a) | 2026-01-01T00:00:02Z [INFO ] Command \"takeScreenshots\" finished\n",
            "nml-client (manager-a/eyes-a/check-a) | 2026-01-01T00:00:09Z [INFO ] Command \"takeScreenshots\" finished\n",
            "{} (manager-a/eyes-a/check-a) | 2026-01-01T00:00:10Z [INFO ] Command \"check\" finished\n",
            "core-requests (manager-a/eyes-a/check-a) | 2026-01-01T00:00:11Z [INFO ] Request \"check\" finished successfully with body {{\"asExpected\":false}}\n",
            "core-requests (manager-a/eyes-a) | 2026-01-01T00:00:12Z [INFO ] Request \"close\" finished successfully with body {{\"status\":\"Passed\"}}\n",
        ), component, component)).unwrap();
        let artifact = temp.path().join("evidence.json");
        let report = success(&[
            "--profile",
            "eyes",
            "investigate",
            input.to_str().unwrap(),
            "--artifact",
            artifact.to_str().unwrap(),
            "--complete-output",
        ]);
        check(&report, &artifact);
        let chains = event_chains(&report);
        assert_eq!(chains.len(), 1);
        assert_eq!(chain_step(chains[0], "screenshot-end")["linked_count"], 1);
        assert_eq!(
            chain_step(chains[0], "screenshot-end")["evidence"][0]["occurrence"]["evidence_ref"]["line"],
            4
        );
        assert_eq!(
            chain_step(chains[0], "check-result")["outcomes"][0]["value"],
            false
        );
        assert_eq!(
            chain_step(chains[0], "session-result")["outcomes"][0]["value"],
            "Passed"
        );
    }
}

#[test]
fn chain_recognition_gap_names_failed_root_selectors() {
    let profile = include_str!("../examples/investigations/chains/profile.toml")
        .replace("equals = \"checkpoint\"", "equals = \"missing-root\"");
    let (_temp, report, _artifact) = chain_fixture(
        &[chain_event("checkpoint", "run-1", "capture-1", 0)],
        Some(&profile),
    );
    assert!(event_chains(&report).is_empty());
    let gap = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"].as_str().unwrap().ends_with("unrecognized"))
        .unwrap();
    assert_eq!(gap["kind"], "unknown");
    assert_eq!(report["findings"][0]["id"], gap["id"]);
    let reason = gap["details"]["reason"].as_str().unwrap();
    assert!(reason.contains("event.name equals \"missing-root\": 1"));
    assert!(reason.contains("1 matched rule IDs"));
}

#[test]
fn compact_chain_retrieval_preserves_cached_failures_and_late_completion() {
    let events = [
        chain_event("checkpoint", "run-1", "capture-1", 0),
        chain_event("submitted", "run-1", "capture-1", 0),
        chain_event("accepted", "run-1", "capture-1", 0),
        chain_event("cached_failure", "run-1", "capture-1", 0),
        chain_event("contrary", "run-1", "capture-1", 0),
        chain_event("capture_end", "run-1", "capture-1", 0),
        chain_event("later_activity", "run-1", "other-capture", 0),
    ];
    let profile = include_str!("../examples/investigations/chains/profile.toml").replace(
        "[[investigation.chains]]",
        r#"[[investigation.roles]]
id = "cached-failure"
rule_ids = ["typed-event"]
select = [{field = "event.name", equals = "cached_failure"}]
[[investigation.chains]]"#,
    ) + r#"
[[investigation.chains.steps]]
id = "cached-failure"
label = "cached failure observed (not a new attempt)"
role = "cached-failure"
from = "$root"
join_fields = ["capture_id"]
cardinality = "one_to_many"
event_type = "cached_failure"
"#;
    let (_temp, report, artifact) = chain_fixture(&events, Some(&profile));
    let finding = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["details"].get("event_chain").is_some())
        .unwrap();
    let action = report["guidance"]["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["kind"] == "verify_chain_evidence")
        .unwrap();
    let argv = action["argv"].as_array().unwrap();
    assert!(argv.contains(&json!("/chain-bundles")));
    assert!(argv.contains(&report["artifact"]["stored_sha256"]));
    let page = success(&[
        "evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        report["artifact"]["stored_sha256"].as_str().unwrap(),
        "--collection",
        "/chain-bundles",
        "--id",
        finding["id"].as_str().unwrap(),
    ]);
    assert_eq!(page["artifact_retrieval"]["parse_passes"], 0);
    assert_eq!(page["artifact_retrieval"]["total"], 1);
    let bundle = &page["artifact_retrieval"]["items"][0];
    assert_eq!(bundle["chain"], finding["details"]["event_chain"]);
    assert_eq!(bundle["evidence"], finding["evidence"]);
    assert_eq!(
        chain_step(&bundle["chain"], "cached-failure")["linked_count"],
        1
    );
    assert_eq!(
        chain_step(&bundle["chain"], "capture-end")["evidence"][0]["occurrence"]["evidence_ref"]["line"],
        6
    );
    assert_eq!(bundle["chain"]["later_activity_observed_count"], 1);
}

#[test]
fn checkpoint_recognition_uses_normalized_command_semantics_across_formats() {
    let events: Vec<_> = [
        ("check", "start"),
        ("takeScreenshots", "start"),
        ("takeScreenshots", "end"),
        ("check", "end"),
    ]
    .iter()
    .enumerate()
    .map(|(i, (name, phase))| {
        json!({
            "timestamp":format!("2026-01-01T00:00:0{i}+02:00"),"level":"INFO",
            "component":"core-002", "component_id":"manager-a/eyes-a/check-a",
            "message":"normalized SDK operation", "operation_kind":"command",
            "operation_name":name, "operation_phase":phase
        })
    })
    .collect();
    let (_temp, report, _artifact) =
        chain_fixture(&events, Some(include_str!("../config/profiles/eyes.toml")));
    let chains = event_chains(&report);
    assert_eq!(chains.len(), 1);
    assert_eq!(chain_step(chains[0], "screenshot-start")["linked_count"], 1);
    assert_eq!(chain_step(chains[0], "screenshot-end")["linked_count"], 1);
    assert_eq!(chain_step(chains[0], "check-end")["linked_count"], 1);
}

#[test]
fn nml_requests_link_broker_attempts_late_capture_and_independent_close() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("nml.log");
    let root_id = "manager-a/eyes-a/check-a";
    let request_id = format!("{root_id}/nml-request-a");
    let messages = [
        (root_id, "Command \"check\" is called"),
        (
            "manager-a/eyes-a/check-a/environment-a",
            "Command \"check\" is called",
        ),
        (
            "manager-a/eyes-a/check-a/environment-a/eyes-base-a/check-base-a",
            "Command \"check\" is called",
        ),
        (
            &request_id,
            "Request \"takeScreenshots\" called with settings {}",
        ),
        (
            &request_id,
            "Broker request \"TAKE_SCREENSHOT\" will be sent to the address \"[POST]https://example.invalid/capture\" with body {}",
        ),
        (
            &request_id,
            "Broker request \"TAKE_SCREENSHOT\" that was sent to the address \"[POST]https://example.invalid/capture\" respond with OK(200)",
        ),
        (
            &request_id,
            "Broker polling request \"TAKE_SCREENSHOT\" attempt #1/100 will be sent to the address \"[GET]https://example.invalid/result\" with body undefined",
        ),
        (
            &request_id,
            "Broker polling request \"TAKE_SCREENSHOT\" attempt #2/100 will be sent to the address \"[GET]https://example.invalid/result\" with body undefined",
        ),
        (
            &request_id,
            "Broker polling request \"TAKE_SCREENSHOT\" attempt #2/100 that was sent to the address \"[GET]https://example.invalid/result\" respond with OK(200) with body {}",
        ),
        (
            "manager-b/eyes-a/check-a/nml-request-a",
            "Request \"takeScreenshots\" finished successfully with body []",
        ),
        (
            &request_id,
            "Request \"takeScreenshots\" finished successfully with body []",
        ),
        (
            "manager-a/eyes-a/check-a/environment-a",
            "Request \"check\" finished successfully with body {\"asExpected\":false}",
        ),
        (
            "manager-a/eyes-a/close-a/core-request-a",
            "Request \"close\" finished successfully with body {\"status\":\"Passed\"}",
        ),
        (
            "manager-a/eyes-a/results-a/core-request-a",
            "Request \"getResults\" finished successfully with body [{\"status\":\"Passed\"}]",
        ),
    ];
    fs::write(
        &source,
        messages
            .iter()
            .enumerate()
            .map(|(i, (id, message))| {
                format!("core-002 ({id}) | 2026-01-01T00:00:{i:02}+02:00 [INFO ] {message}\n")
            })
            .collect::<String>(),
    )
    .unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--profile",
        "eyes",
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    let chains = event_chains(&report);
    assert_eq!(chains.len(), 1);
    let chain = chains[0];
    assert_eq!(chain["root_occurrences"], 1);
    for (step, count) in [
        ("screenshot-start", 1),
        ("screenshot-end", 1),
        ("broker-submission", 1),
        ("broker-acceptance", 1),
        ("broker-polls", 2),
        ("broker-responses", 1),
        ("check-result", 1),
        ("close-end", 1),
        ("session-result", 1),
    ] {
        assert_eq!(chain_step(chain, step)["linked_count"], count, "{step}");
    }
    assert_eq!(
        chain_step(chain, "broker-responses")["join_samples"][0]["source"]["evidence_ref"]["line"],
        8
    );
    assert_eq!(
        chain_step(chain, "screenshot-end")["evidence"][0]["occurrence"]["evidence_ref"]["line"],
        11
    );
    assert_eq!(
        chain_step(chain, "check-result")["outcomes"][0]["value"],
        false
    );
    assert_eq!(
        chain_step(chain, "session-result")["outcomes"][0]["value"],
        "Passed"
    );
    assert_eq!(chain_step(chain, "session-summary")["linked_count"], 1);
    assert_eq!(
        chain_step(chain, "session-summary")["outcomes"][0]["value"],
        "Passed"
    );
    let interval = &chain_step(chain, "screenshot-end")["join_samples"][0]["endpoint_interval"];
    assert_eq!(interval["value"], 7000.0);
    assert_eq!(interval["start_operation"], "takeScreenshots");
    assert_eq!(interval["end_timestamp"], "2026-01-01T00:00:10+02:00");
    let recognition = &chain_step(chain, "capture-start")["recognition"];
    assert_eq!(recognition["role_matched_records"], 0);
    assert!(
        recognition["failed_selectors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["failed_records"].as_u64().unwrap() > 0)
    );
    assert!(
        report["guidance"]["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["kind"] == "inspect_relationship_coverage")
    );
}

#[test]
fn failed_resource_cache_bundle_excludes_other_sessions_without_inventing_requests() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("cache.log");
    fs::write(&source,concat!(
        "ufg-client (manager-a/eyes-a/check-a/environment-a/fetch-resource-a) | 2026-01-01T00:00:00Z [INFO ] Resource with url https://example.invalid/asset respond with Forbidden(Forbidden)\n",
        "ufg-client (manager-b/eyes-a/check-b/environment-b) | 2026-01-01T00:00:01Z [INFO ] resource retrieved from cache, with dependencies (0): https://example.invalid/asset with dependencies --> \n",
        "ufg-client (manager-a/eyes-a/check-b/environment-b) | 2026-01-01T00:00:02Z [INFO ] resource retrieved from cache, with dependencies (0): https://example.invalid/asset with dependencies --> \n",
        "ufg-client (manager-a/eyes-a/check-b/environment-b) | 2026-01-01T00:00:03Z [INFO ] resource retrieved from cache, with dependencies (0): https://example.invalid/other with dependencies --> \n",
    )).unwrap();
    let artifact = temp.path().join("evidence.json");
    let report = success(&[
        "--profile",
        "eyes",
        "investigate",
        source.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--complete-output",
    ]);
    check(&report, &artifact);
    let chain = event_chains(&report)[0];
    assert_eq!(chain["id"], "resource-cache-evidence");
    let step = chain_step(chain, "cache-observations");
    assert_eq!(step["linked_count"], 1);
    assert_eq!(step["event_type"], "cache_observation");
    assert_eq!(step["evidence"][0]["occurrence"]["evidence_ref"]["line"], 3);
    assert!(step["outcomes"].as_array().unwrap().is_empty());
    assert!(
        !chain["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["event_type"] == "poll_send")
    );
}

#[test]
fn chain_intervals_preserve_offsets_fractional_precision_and_unknown_boundaries() {
    for (start, end, expected) in [
        (
            "2026-01-01T02:00:00.000+02:00",
            "2026-01-01T00:00:22.928Z",
            Some(22928.0),
        ),
        (
            "2026-01-01T00:00:00.000Z",
            "2026-01-01T00:00:00.000125Z",
            Some(0.125),
        ),
        ("2026-01-01T00:00:01Z", "2026-01-01T00:00:00Z", None),
    ] {
        let mut root = chain_event("checkpoint", "run-a", "capture-a", 0);
        let mut submitted = chain_event("submitted", "run-a", "capture-a", 0);
        root["timestamp"] = json!(start);
        submitted["timestamp"] = json!(end);
        let (_temp, report, _artifact) = chain_fixture(&[root, submitted], None);
        let interval = &chain_step(event_chains(&report)[0], "submitted")["join_samples"][0]["endpoint_interval"];
        assert_eq!(interval["value"], json!(expected));
        assert_eq!(
            interval["status"],
            if expected.is_some() {
                "measured"
            } else {
                "unavailable"
            }
        );
    }
    let events = [
        chain_event("checkpoint", "run-a", "capture-a", 0),
        chain_event("submitted", "run-a", "capture-a", 0),
        chain_event("submitted", "run-a", "capture-a", 0),
    ];
    let profile = include_str!("../examples/investigations/chains/profile.toml").replacen(
        "cardinality = \"one_to_one\"",
        "cardinality = \"one_to_many\"",
        1,
    );
    let (_temp, report, _artifact) = chain_fixture(&events, Some(&profile));
    let step = chain_step(event_chains(&report)[0], "submitted");
    assert_eq!(step["linked_count"], 2);
    for join in step["join_samples"].as_array().unwrap() {
        assert_eq!(
            join["endpoint_interval"]["reason"],
            "ambiguous_endpoint_population"
        );
        assert!(join["endpoint_interval"]["value"].is_null());
    }
}

#[test]
fn targeted_compact_evidence_checks_required_claims_and_rejects_wrong_values() {
    let mut first = chain_event("checkpoint", "run-1", "capture-1", 0);
    first["payload"]["large"] = json!("x".repeat(8000));
    let events = [
        first,
        chain_event("submitted", "run-1", "capture-1", 0),
        chain_event("accepted", "run-1", "capture-1", 0),
        chain_event("capture_end", "run-1", "capture-1", 0),
        chain_event("later_activity", "run-1", "other", 0),
    ];
    let (temp, report, artifact) = chain_fixture(
        &events,
        Some(
            &(include_str!("../examples/investigations/chains/profile.toml").to_string()
                + "\n[investigation_view]\nsource_fields = [\"event.name\"]\n"),
        ),
    );
    let finding = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["details"].get("event_chain").is_some())
        .unwrap();
    let claims = temp.path().join("claims.json");
    fs::write(&claims, json!({"version":1,"required_claim_ids":["end","literal"],"claims":[{"id":"end","kind":"link","finding_id":finding["id"],"step_id":"capture-end"},{"id":"literal","kind":"fact","pointer":"/records/0/fields/payload/asExpected","equals":false}]}).to_string()).unwrap();
    let args = [
        "evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        report["artifact"]["stored_sha256"].as_str().unwrap(),
        "--collection",
        "/chain-bundles",
        "--id",
        finding["id"].as_str().unwrap(),
        "--compact",
        "--claims",
        claims.to_str().unwrap(),
    ];
    let output = success(&args);
    assert_eq!(
        output["artifact_retrieval"]["claim_verification"]["status"],
        "verified"
    );
    let compact = &output["artifact_retrieval"]["items"][0];
    assert_eq!(compact["chain"]["later_activity_observed_count"], 1);
    assert!(compact.to_string().len() < finding.to_string().len());
    assert!(compact["evidence"][0].get("fields").is_none());
    assert!(
        compact["evidence"][0]["omitted_fields_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(compact["evidence"][0]["record_retrieval"]["id"].is_string());
    let action = report["guidance"]["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["kind"] == "verify_chain_evidence")
        .unwrap();
    assert!(
        action["argv"]
            .as_array()
            .unwrap()
            .contains(&json!("--compact"))
    );
    assert_eq!(action["finding_id"], finding["id"]);
    fs::write(&claims,json!({"version":1,"required_claim_ids":["literal","missing-link"],"claims":[{"id":"literal","kind":"fact","pointer":"/records/0/fields/payload/asExpected","equals":true}]}).to_string()).unwrap();
    let failed = run(&args);
    assert!(!failed.status.success());
    let result: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(
        result["artifact_retrieval"]["claim_verification"]["checks"][0]["status"],
        "contradicted"
    );
    assert_eq!(
        result["artifact_retrieval"]["claim_verification"]["missing_required_claim_ids"],
        json!(["missing-link"])
    );
    fs::write(&claims,json!({"version":1,"required_claim_ids":["unfinished"],"claims":[{"id":"unfinished","kind":"absence","finding_id":finding["id"],"step_id":"capture-end"}]}).to_string()).unwrap();
    let absent = run(&args);
    assert!(!absent.status.success());
    let checked: Value = serde_json::from_slice(&absent.stdout).unwrap();
    assert_eq!(
        checked["artifact_retrieval"]["claim_verification"]["checks"][0]["status"],
        "contradicted"
    );
    fs::write(&claims,json!({"version":1,"required_claim_ids":["x"],"claims":[{"id":"x","kind":"fact","pointer":"/records/0","equals":null,"unexpected":true}]}).to_string()).unwrap();
    assert!(!run(&args).status.success());
    assert!(
        !run(&[
            "evidence",
            artifact.to_str().unwrap(),
            "--expected-sha256",
            report["artifact"]["stored_sha256"].as_str().unwrap(),
            "--compact"
        ])
        .status
        .success()
    );
}

#[test]
fn relationship_recognition_gap_requires_inspection_before_profile_recovery() {
    let profile = include_str!("../examples/investigations/chains/profile.toml")
        .replace("equals = \"checkpoint\"", "equals = \"missing-root\"");
    let events = [chain_event("checkpoint", "run-1", "capture-1", 0)];
    let (_temp, report, _artifact) = chain_fixture(&events, Some(&profile));
    let kinds: Vec<_> = report["guidance"]["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"inspect_relationship_coverage"));
    // A rule mismatch is a request for source inspection, not semantic truth.
    for kind in [
        "prepare_profile_candidate",
        "validate_profile_candidate",
        "rerun_validated_profile",
    ] {
        assert!(!kinds.contains(&kind));
    }
    assert!(event_chains(&report).is_empty());
    let root_gap = report["guidance"]["workflow"]["gaps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["step_id"] == "$root")
        .unwrap();
    assert_eq!(root_gap["classification"], "recognition_unresolved");
    let (_temp, positive, _artifact) = chain_fixture(&events, None);
    assert!(
        !positive["guidance"]["workflow"]["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["step_id"] == "$root")
    );
    assert_eq!(event_chains(&positive).len(), 1);
    assert_eq!(event_chains(&positive)[0]["root_event_name"], "checkpoint");
}

fn subject_link(finding: &Value, step_id: &str) -> Value {
    let chain = &finding["details"]["event_chain"];
    let join = &chain_step(chain, step_id)["join_samples"][0];
    json!({"id":"requested-link","label":"Human checkpoint label is not identity","kind":"link","finding_id":finding["id"],"step_id":step_id,"subject":{"snapshot_id":chain["root"]["snapshot_id"],"input_ordinal":chain["root"]["input_ordinal"],"scope_id":finding["scope_id"],"root":chain["root"],"identity":chain["identity"],"event_name":chain["root_event_name"],"source":join["source"],"target":join["target"]}})
}

#[test]
fn claim_drafts_verify_exact_endpoints_but_require_final_review() {
    let events = [
        chain_event("checkpoint", "run-a", "capture-a", 0),
        chain_event("capture_end", "run-a", "capture-a", 20),
        chain_event("checkpoint", "run-a", "capture-b", 30),
        chain_event("capture_end", "run-a", "capture-b", 40),
    ];
    let (temp, report, artifact) = chain_fixture(&events, None);
    let path = temp.path().join("draft.json");
    let argv = [
        "evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        report["artifact"]["stored_sha256"].as_str().unwrap(),
        "--draft-claims",
        "--output",
        path.to_str().unwrap(),
    ];
    let output = run(&argv);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["status"], "draft_requires_review");
    let mut draft: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/claim-assertions.schema.json")).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&draft)
        .unwrap();
    assert!(
        draft["claims"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["kind"] == "interval")
    );
    assert!(
        !draft["claims"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["kind"] == "absence")
    );
    let verify = |draft: &Value| {
        fs::write(&path, draft.to_string()).unwrap();
        let o = run(&[
            "evidence",
            artifact.to_str().unwrap(),
            "--expected-sha256",
            report["artifact"]["stored_sha256"].as_str().unwrap(),
            "--claims",
            path.to_str().unwrap(),
        ]);
        let v: Value = serde_json::from_slice(&o.stdout).unwrap();
        (
            o.status.success(),
            v["artifact_retrieval"]["claim_verification"].clone(),
        )
    };
    let (ok, v) = verify(&draft);
    assert!(!ok);
    assert_eq!(v["final_review"]["status"], "needs_revision");
    assert!(
        v["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["status"] == "verified"),
        "{v}"
    );
    draft["final_review"] = json!({"summary_claim_ids":draft["required_claim_ids"],
        "finding_claim_ids":draft["required_claim_ids"],
        "checks":["event_subjects","duration_endpoints_and_units","operation_types_and_count_semantics","terminal_later_cached_contrary","summary_consistency","causal_support"]});
    let (ok, unmapped) = verify(&draft);
    assert!(!ok);
    assert_eq!(unmapped["final_review"]["status"], "needs_revision");
    assert!(
        unmapped["requirement_results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["reason"] == "typed_question_requirement_missing")
    );
    // A caller can check a supported subset before acknowledging prose review.
    draft.as_object_mut().unwrap().remove("final_review");
    assert!(verify(&draft).0);
    let positive = draft.clone();
    draft["claims"][0]["subject"]["event_name"] = json!("adjacent wrong checkpoint");
    assert_eq!(verify(&draft).1["checks"][0]["status"], "contradicted");
    assert!(verify(&positive).0);
    let original = fs::read(&artifact).unwrap();
    assert!(!run(&argv).status.success());
    assert_eq!(fs::read(&artifact).unwrap(), original);
    let selected_id = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["details"].get("event_chain").is_some())
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let page = run(&[
        "evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        report["artifact"]["stored_sha256"].as_str().unwrap(),
        "--collection",
        "/chain-bundles",
        "--id",
        selected_id,
        "--compact",
        "--report-max-items",
        "50",
    ]);
    let page: Value = serde_json::from_slice(&page.stdout).unwrap();
    let next = &page["artifact_retrieval"]["workflow"]["next_action"];
    assert_eq!(next["kind"], "review_then_draft_claims");
    let command: Vec<_> = next["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        Command::new(command[0])
            .args(&command[1..])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(temp.path().join("claims-draft.json").is_file());
    assert!(command.windows(2).any(|pair| pair == ["--id", selected_id]));
    assert!(
        !command
            .iter()
            .any(|arg| matches!(*arg, "--collection" | "--report-max-items" | "--compact"))
    );
    let selected_draft: Value =
        serde_json::from_slice(&fs::read(temp.path().join("claims-draft.json")).unwrap()).unwrap();
    assert!(
        selected_draft["claims"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["id"].as_str().unwrap().starts_with(selected_id))
    );
}
fn verify_subject_claim(
    temp: &tempfile::TempDir,
    report: &Value,
    artifact: &Path,
    claim: Value,
) -> Value {
    let claims = temp.path().join("subject-claims.json");
    let assertions = json!({"version":1,"questions":["Did the requested checkpoint complete?"],"required_claim_ids":["requested-link"],"claims":[claim]});
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/claim-assertions.schema.json")).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&assertions)
        .unwrap();
    fs::write(&claims, assertions.to_string()).unwrap();
    let output = run(&[
        "evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        report["artifact"]["stored_sha256"].as_str().unwrap(),
        "--collection",
        "/chain-bundles",
        "--claims",
        claims.to_str().unwrap(),
        "--report-max-items",
        "1",
    ]);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let verification = &value["artifact_retrieval"]["claim_verification"];
    assert_eq!(
        output.status.success(),
        verification["status"] == "verified",
        "{value}"
    );
    verification.clone()
}

#[test]
fn subject_bound_links_reject_adjacent_names_scopes_ids_and_endpoints() {
    let events = [
        chain_event("checkpoint", "run-a", "capture-a", 0),
        chain_event("capture_end", "run-a", "capture-a", 0),
        chain_event("checkpoint", "run-a", "capture-b", 0),
        chain_event("capture_end", "run-a", "capture-b", 0),
        chain_event("checkpoint", "run-b", "capture-a", 0),
        chain_event("capture_end", "run-b", "capture-a", 0),
    ];
    let (temp, report, artifact) = chain_fixture(&events, None);
    let findings: Vec<_> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["details"].get("event_chain").is_some())
        .collect();
    let positive = subject_link(findings[0], "capture-end");
    assert_eq!(positive["subject"]["root"]["evidence_ref"]["line"], 1);
    assert_eq!(positive["subject"]["target"]["evidence_ref"]["line"], 2);
    let checked = verify_subject_claim(&temp, &report, &artifact, positive.clone());
    assert_eq!(checked["checks"][0]["assurance"], "subject_verified");
    assert_eq!(
        checked["checks"][0]["source_citations"]["target"],
        positive["subject"]["target"]
    );
    for field in [
        "snapshot_id",
        "input_ordinal",
        "scope_id",
        "root",
        "identity",
        "event_name",
        "source",
        "target",
    ] {
        let mut wrong = positive.clone();
        wrong["subject"][field] = match field {
            "snapshot_id" | "scope_id" | "event_name" => json!("a-different-subject"),
            "input_ordinal" => json!(1),
            "identity" => findings[2]["details"]["event_chain"]["identity"].clone(),
            "root" => findings[1]["details"]["event_chain"]["root"].clone(),
            "source" | "target" => {
                subject_link(findings[1], "capture-end")["subject"][field].clone()
            }
            _ => unreachable!(),
        };
        let result = verify_subject_claim(&temp, &report, &artifact, wrong);
        assert_eq!(
            result["checks"][0]["status"], "contradicted",
            "{field}: {result}"
        );
        assert_eq!(result["checks"][0]["assurance"], "subject_unverified");
        assert!(result["checks"][0]["identity"]["expected"].is_object());
        // The minimally changed positive control must still pass.
        assert_eq!(
            verify_subject_claim(&temp, &report, &artifact, positive.clone())["checks"][0]["assurance"],
            "subject_verified"
        );
    }
    let mut missing = positive.clone();
    missing["subject"]
        .as_object_mut()
        .unwrap()
        .remove("identity");
    assert_eq!(
        verify_subject_claim(&temp, &report, &artifact, missing)["checks"][0]["status"],
        "unavailable"
    );
    for field in ["root", "source", "target", "identity"] {
        let mut incomplete = positive.clone();
        incomplete["subject"][field] = if field == "identity" {
            json!([])
        } else {
            json!({})
        };
        assert_eq!(
            verify_subject_claim(&temp, &report, &artifact, incomplete)["checks"][0]["status"],
            "unavailable",
            "{field}"
        );
        assert_eq!(
            verify_subject_claim(&temp, &report, &artifact, positive.clone())["checks"][0]["assurance"],
            "subject_verified"
        );
    }
    let mut legacy = positive;
    legacy.as_object_mut().unwrap().remove("subject");
    let result = verify_subject_claim(&temp, &report, &artifact, legacy);
    assert_eq!(result["checks"][0]["status"], "verified");
    assert_eq!(result["checks"][0]["assurance"], "link_exists");
}

#[test]
fn subject_bound_intervals_reject_wrong_named_operations_and_ambiguous_endpoints() {
    for ambiguous in [false, true] {
        let mut events = vec![
            chain_event("checkpoint", "run-a", "capture-a", 0),
            chain_event("capture_end", "run-a", "capture-a", 0),
        ];
        events[1]["timestamp"] = json!("2026-01-01T00:00:00.025Z");
        if ambiguous {
            events.push(events[1].clone());
        }
        let profile = include_str!("../examples/investigations/chains/profile.toml").replace(
            "cardinality = \"one_to_one\"",
            "cardinality = \"one_to_many\"",
        );
        let (temp, report, artifact) = chain_fixture(&events, Some(&profile));
        let finding_index = report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .position(|f| f["details"].get("event_chain").is_some())
            .unwrap();
        let finding = &report["findings"][finding_index];
        let step_index = finding["details"]["event_chain"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .position(|s| s["id"] == "capture-end")
            .unwrap();
        let mut subject = subject_link(finding, "capture-end")["subject"].clone();
        for key in ["root", "scope_id", "identity", "event_name"] {
            subject.as_object_mut().unwrap().remove(key);
        }
        subject["start_operation"] = json!("checkpoint");
        subject["end_operation"] = json!("capture_end");
        let claim = json!({"id":"requested-link","kind":"interval","pointer":format!("/findings/{finding_index}/details/event_chain/steps/{step_index}/join_samples/0/endpoint_interval"),"expected_ms":25,"subject":subject});
        let positive = verify_subject_claim(&temp, &report, &artifact, claim.clone());
        assert_eq!(
            positive["checks"][0]["status"],
            if ambiguous { "unavailable" } else { "verified" }
        );
        let mut wrong = claim.clone();
        wrong["subject"]["start_operation"] = json!("close");
        assert_eq!(
            verify_subject_claim(&temp, &report, &artifact, wrong)["checks"][0]["status"],
            "contradicted"
        );
        let mut wrong = claim;
        wrong["expected_ms"] = json!(24);
        assert_eq!(
            verify_subject_claim(&temp, &report, &artifact, wrong)["checks"][0]["status"],
            if ambiguous {
                "unavailable"
            } else {
                "contradicted"
            }
        );
    }
}

#[test]
fn omitted_subject_endpoint_witness_is_unavailable_until_supported_sample_is_selected() {
    let events = [
        chain_event("checkpoint", "run-a", "capture-a", 0),
        chain_event("check_end", "run-a", "capture-a", 0),
        chain_event("check_end", "run-a", "capture-a", 0),
        chain_event("check_end", "run-a", "capture-a", 0),
    ];
    let (temp, report, artifact) = chain_fixture(&events, None);
    let finding = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["details"].get("event_chain").is_some())
        .unwrap();
    let positive = subject_link(finding, "check-end");
    let mut omitted = positive.clone();
    let retained: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    omitted["subject"]["target"] = retained["records"][2]["occurrence"].clone();
    assert_eq!(
        chain_step(&finding["details"]["event_chain"], "check-end")["omitted_occurrences"],
        1
    );
    assert_eq!(
        verify_subject_claim(&temp, &report, &artifact, omitted)["checks"][0]["status"],
        "unavailable"
    );
    assert_eq!(
        verify_subject_claim(&temp, &report, &artifact, positive)["checks"][0]["assurance"],
        "subject_verified"
    );
}

#[test]
fn workflow_checklist_survives_retrieval_and_cannot_be_dropped_by_claims() {
    let events = [
        chain_event("checkpoint", "run-a", "capture-a", 0),
        chain_event("capture_end", "run-a", "capture-a", 0),
    ];
    let (temp, _, _) = chain_fixture(&events, None);
    let artifact = temp.path().join("checklist.json");
    let report = success(&[
        "--config",
        temp.path().join("profile.toml").to_str().unwrap(),
        "investigate",
        temp.path().join("events.jsonl").to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--question",
        "Did this checkpoint finish?",
        "--required-relationship",
        "requested-link",
        "--summary",
    ]);
    let workflow = &report["guidance"]["workflow"];
    assert_eq!(workflow["input_processing"], "complete");
    assert_eq!(workflow["relevant_evidence_review"], "unconfirmed");
    assert_eq!(workflow["declared_claims"], "not_checked");
    let argv: Vec<_> = workflow["next_action"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(argv.contains(&"--compact"));
    let page = success(&argv[1..]);
    let retrieved = &page["artifact_retrieval"]["workflow"];
    assert_eq!(
        retrieved["questions"],
        json!(["Did this checkpoint finish?"])
    );
    assert_eq!(
        retrieved["required_relationships"],
        json!(["requested-link"])
    );
    assert_eq!(retrieved["relevant_evidence_review"], "unconfirmed");
    assert_eq!(retrieved["declared_claims"], "not_checked");
    let claims = temp.path().join("missing-required.json");
    fs::write(&claims,json!({"version":1,"required_claim_ids":["literal"],"claims":[{"id":"literal","kind":"fact","pointer":"/contract_version","equals":1}]}).to_string()).unwrap();
    let checked = run(&[
        "evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        report["artifact"]["stored_sha256"].as_str().unwrap(),
        "--claims",
        claims.to_str().unwrap(),
    ]);
    assert!(!checked.status.success());
    let value: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(
        value["artifact_retrieval"]["claim_verification"]["missing_required_claim_ids"],
        json!(["requested-link"])
    );
    let full: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    let finding = full["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["details"].get("event_chain").is_some())
        .unwrap();
    assert_eq!(
        verify_subject_claim(
            &temp,
            &report,
            &artifact,
            subject_link(finding, "capture-end")
        )["status"],
        "verified"
    );
}

#[test]
fn redacted_checklists_preserve_opaque_required_ids() {
    let temp = tempfile::tempdir().unwrap();
    let (report, artifact) = investigate(
        temp.path(),
        &[
            "--redact",
            "--question",
            "Private checkpoint question",
            "--required-relationship",
            "private-relationship",
        ],
    );
    let required = report["guidance"]["workflow"]["required_relationships"].clone();
    assert!(required[0].as_str().unwrap().starts_with("opaque-"));
    let bytes = fs::read_to_string(&artifact).unwrap();
    assert!(!bytes.contains("Private checkpoint question"));
    assert!(!bytes.contains("private-relationship"));
    let retained: Value = serde_json::from_str(&bytes).unwrap();
    assert_eq!(
        retained["question_checklist"]["required_relationship"],
        required
    );
    let hash = report["artifact"]["stored_sha256"].as_str().unwrap();
    let page = success(&[
        "evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        hash,
    ]);
    assert_eq!(
        page["artifact_retrieval"]["workflow"]["required_relationships"],
        required
    );
    assert_eq!(
        page["artifact_retrieval"]["workflow"]["questions"],
        json!(["[REDACTED QUESTION]"])
    );
    let claims = temp.path().join("claims.json");
    let mut assertion = json!({"version":1,"required_claim_ids":["unrelated"],"claims":[{"id":"unrelated","kind":"fact","pointer":"/contract_version","equals":1}]});
    let verify = |a: &Value| {
        fs::write(&claims, a.to_string()).unwrap();
        let result = run(&[
            "evidence",
            artifact.to_str().unwrap(),
            "--expected-sha256",
            hash,
            "--claims",
            claims.to_str().unwrap(),
        ]);
        let output: Value = serde_json::from_slice(&result.stdout).unwrap();
        (
            result.status.success(),
            output["artifact_retrieval"]["claim_verification"].clone(),
        )
    };
    let (ok, wrong) = verify(&assertion);
    assert!(!ok);
    assert_eq!(wrong["missing_required_claim_ids"], required);
    assertion["required_claim_ids"] = required.clone();
    assertion["claims"][0]["id"] = required[0].clone();
    assert!(verify(&assertion).0);
}

#[test]
fn invalid_checklists_fail_before_artifact_creation() {
    let temp = tempfile::tempdir().unwrap();
    let artifact = temp.path().join("invalid.json");
    for (flag, values) in [
        ("--question", vec!["".to_string()]),
        ("--required-relationship", vec!["".to_string()]),
        ("--required-relationship", vec![" ".to_string()]),
        ("--required-relationship", vec!["duplicate".to_string(); 2]),
        (
            "--question",
            (0..101).map(|i| format!("question-{i}")).collect(),
        ),
        (
            "--required-relationship",
            (0..101).map(|i| format!("id-{i}")).collect(),
        ),
    ] {
        let mut command = binary();
        command.current_dir(root()).args([
            "investigate",
            "examples/investigations/slow.jsonl",
            "--artifact",
            artifact.to_str().unwrap(),
        ]);
        for value in values {
            command.args([flag, &value]);
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Checklist accepts"));
        assert!(!artifact.exists());
    }
    let mut options = Vec::new();
    for i in 0..100 {
        options.extend([
            "--question".to_string(),
            format!("question-{i}"),
            "--required-relationship".to_string(),
            format!("id-{i}"),
        ]);
    }
    let (report, _) = investigate(
        temp.path(),
        &options.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    assert_eq!(
        report["guidance"]["workflow"]["required_relationships"]
            .as_array()
            .unwrap()
            .len(),
        100
    );
}

#[test]
fn identical_ids_in_independent_snapshots_do_not_verify_as_the_same_subject() {
    let events = [
        chain_event("checkpoint", "run-a", "capture-a", 0),
        chain_event("capture_end", "run-a", "capture-a", 0),
    ];
    let (temp, report, artifact) = chain_fixture(&events, None);
    let (_other, other_report, _) = chain_fixture(&events, None);
    let finding = |report: &Value| {
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["details"].get("event_chain").is_some())
            .unwrap()
            .clone()
    };
    let positive = subject_link(&finding(&report), "capture-end");
    let mut wrong = positive.clone();
    wrong["subject"] = subject_link(&finding(&other_report), "capture-end")["subject"].clone();
    assert_eq!(
        verify_subject_claim(&temp, &report, &artifact, wrong)["checks"][0]["status"],
        "contradicted"
    );
    assert_eq!(
        verify_subject_claim(&temp, &report, &artifact, positive)["checks"][0]["assurance"],
        "subject_verified"
    );
}

#[test]
fn typed_checklists_keep_unknown_questions_separate_from_verified_claims() {
    let events = [
        chain_event("checkpoint", "east", "finished", 0),
        chain_event("checkpoint", "east", "unfinished", 0),
        chain_event("capture_end", "east", "finished", 0),
        chain_event("contrary", "east", "unfinished", 0),
    ];
    let (temp, report, artifact) = chain_fixture(&events, None);
    let draft_path = temp.path().join("draft.json");
    success(&[
        "evidence",
        artifact.to_str().unwrap(),
        "--expected-sha256",
        report["artifact"]["stored_sha256"].as_str().unwrap(),
        "--draft-claims",
        "--output",
        draft_path.to_str().unwrap(),
    ]);
    let draft: Value = serde_json::from_slice(&fs::read(&draft_path).unwrap()).unwrap();
    let findings = report["findings"].as_array().unwrap();
    let finding = |line| {
        findings
            .iter()
            .find(|f| f["details"]["event_chain"]["root"]["evidence_ref"]["line"] == line)
            .unwrap()
    };
    let known = finding(1);
    let unknown = finding(2);
    let mut interval = draft["claims"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| {
            c["kind"] == "interval"
                && c["subject"]["source"]["evidence_ref"]["line"] == 1
                && c["subject"]["end_operation"] == "capture_end"
        })
        .unwrap()
        .clone();
    interval["id"] = json!("finished-duration");
    let mut contrary = subject_link(unknown, "contrary");
    contrary["id"] = json!("contrary-link");
    let mut assertions = json!({"version":1,"required_claim_ids":["finished-duration","unfinished-duration"],
        "requirements":[
            {"id":"finished-duration","kind":"interval","finding_id":known["id"],"start_operation":"checkpoint","end_operation":"capture_end"},
            {"id":"unfinished-duration","kind":"interval","finding_id":unknown["id"],"start_operation":"checkpoint","end_operation":"capture_end"}],
        "claims":[interval,contrary],"unresolved":[{"id":"unfinished-duration","reason":"No capture ending retained for this scoped root; completion and duration remain unknown."}],
        "final_review":{"summary_claim_ids":["finished-duration"],"finding_claim_ids":["finished-duration","contrary-link"],"unresolved_question_ids":["unfinished-duration"],"checks":["event_subjects","duration_endpoints_and_units","operation_types_and_count_semantics","terminal_later_cached_contrary","summary_consistency","causal_support"]}});
    let path = temp.path().join("review.json");
    let verify = |a: &Value| {
        let schema: Value =
            serde_json::from_str(include_str!("../schemas/claim-assertions.schema.json")).unwrap();
        jsonschema::validator_for(&schema)
            .unwrap()
            .validate(a)
            .unwrap();
        fs::write(&path, a.to_string()).unwrap();
        let output = run(&[
            "evidence",
            artifact.to_str().unwrap(),
            "--expected-sha256",
            report["artifact"]["stored_sha256"].as_str().unwrap(),
            "--claims",
            path.to_str().unwrap(),
        ]);
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        (output.status.success(), value)
    };
    let (ok, value) = verify(&assertions);
    assert!(ok, "{value}");
    let cv = &value["artifact_retrieval"]["claim_verification"];
    assert_eq!(cv["status"], "verified_with_unknowns");
    assert_eq!(
        cv["unresolved_required_claim_ids"],
        json!(["unfinished-duration"])
    );
    assert_eq!(cv["missing_required_claim_ids"], json!([]));
    assert_eq!(cv["requirement_results"][1]["status"], "declared_unknown");
    assert_eq!(
        cv["final_review"]["status"],
        "declared_checks_passed_with_unknowns"
    );
    assert_eq!(
        value["artifact_retrieval"]["workflow"]["declared_claims"],
        "verified_with_unknowns"
    );
    // The skill compiler produces schema-valid mappings and does not manufacture
    // proof for an unknown. The retained verifier remains the authority.
    let proposal_id = draft["claims"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| {
            c["kind"] == "interval"
                && c["subject"]["source"]["evidence_ref"]["line"] == 1
                && c["subject"]["end_operation"] == "capture_end"
        })
        .unwrap()["id"]
        .clone();
    let mut plan = json!({"questions":[
        {"id":"finished-duration","claim_id":proposal_id,
         "binding":{"finding_id":known["id"],"start_operation":"checkpoint","end_operation":"capture_end"}},
        {"id":"unfinished-duration","kind":"interval","finding_id":unknown["id"],
         "end_operation":"capture_end","reason":"No supported ending for this scoped root"}],
        "reviewed_checks":assertions["final_review"]["checks"]});
    let plan_path = temp.path().join("plan.json");
    let compile = |plan: &Value, name: &str| {
        fs::write(&plan_path, plan.to_string()).unwrap();
        let output_path = temp.path().join(name);
        let output = Command::new("python3")
            .args(["-I"])
            .arg(root().join(".agents/skills/analyze-logs/scripts/map_questions.py"))
            .arg("--draft")
            .arg(if name == "compiled-unknown.json" {
                temp.path().join("no-draft.json")
            } else {
                draft_path.clone()
            })
            .arg("--artifact")
            .arg(&artifact)
            .arg("--plan")
            .arg(&plan_path)
            .arg("--output")
            .arg(&output_path)
            .arg("--analyzer")
            .arg(env!("CARGO_BIN_EXE_log-analyzer"))
            .arg("--verification-output")
            .arg(output_path.with_extension("verification.json"))
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            name != "compiled-wrong.json",
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            summary["status"],
            if name == "compiled-wrong.json" {
                "needs_revision"
            } else {
                "verified_with_unknowns"
            }
        );
        let report: Value = serde_json::from_slice(
            &fs::read(output_path.with_extension("verification.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            summary["requirement_results"],
            report["artifact_retrieval"]["claim_verification"]["requirement_results"]
        );
        serde_json::from_slice::<Value>(&fs::read(output_path).unwrap()).unwrap()
    };
    let compiled = compile(&plan, "compiled.json");
    assert!(verify(&compiled).0);
    let all_unknown = json!({"questions":[
        {"id":"unfinished-duration","kind":"interval","finding_id":unknown["id"],
         "end_operation":"capture_end","reason":"No supported ending for this scoped root"},
        {"id":"cause","kind":"fact","finding_id":unknown["id"],"reason":"No causal mechanism established"},
        {"id":"finish-link","kind":"link","finding_id":unknown["id"],"step_id":"capture-end","reason":"No supported scoped ending"}],
        "reviewed_checks":assertions["final_review"]["checks"]});
    let all_unknown = compile(&all_unknown, "compiled-unknown.json");
    assert_eq!(all_unknown["claims"], json!([]));
    let (ok, value) = verify(&all_unknown);
    assert!(ok, "{value}");
    assert_eq!(
        value["artifact_retrieval"]["claim_verification"]["checks"],
        json!([])
    );
    assert_eq!(
        value["artifact_retrieval"]["claim_verification"]["final_review"]["status"],
        "declared_checks_passed_with_unknowns"
    );
    let mut incomplete_unknown = all_unknown.clone();
    incomplete_unknown["unresolved"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(!verify(&incomplete_unknown).0);
    let mut unanchored_unknown = all_unknown.clone();
    unanchored_unknown["requirements"][0]["finding_id"] = json!("missing-root");
    assert!(!verify(&unanchored_unknown).0);
    let mut available_unknown = all_unknown.clone();
    available_unknown["requirements"][0]["finding_id"] = known["id"].clone();
    assert!(!verify(&available_unknown).0);
    assert!(verify(&all_unknown).0);
    fs::write(&plan_path, plan.to_string()).unwrap();
    let prior_report = temp.path().join("compiled.verification.json");
    let prior_bytes = fs::read(&prior_report).unwrap();
    let fresh_claims = temp.path().join("stale-proof-claims.json");
    let rejected = Command::new("python3")
        .arg("-I")
        .arg(root().join(".agents/skills/analyze-logs/scripts/map_questions.py"))
        .arg("--draft")
        .arg(&draft_path)
        .arg("--artifact")
        .arg(&artifact)
        .arg("--plan")
        .arg(&plan_path)
        .arg("--output")
        .arg(&fresh_claims)
        .arg("--analyzer")
        .arg(env!("CARGO_BIN_EXE_log-analyzer"))
        .arg("--verification-output")
        .arg(&prior_report)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    assert!(!fresh_claims.exists());
    assert_eq!(fs::read(&prior_report).unwrap(), prior_bytes);
    assert!(verify(&compiled).0);
    plan["questions"][1] = json!({"id":"unfinished-duration","kind":"interval",
        "finding_id":known["id"],"end_operation":"capture_end","reason":"Trying to hide a supported measurement"});
    assert!(!verify(&compile(&plan, "compiled-wrong.json")).0);
    assert!(verify(&compiled).0);
    // An unrelated supported link cannot fulfill a requested interval ID.
    let positive = assertions.clone();
    assertions["unresolved"] = json!([]);
    assertions["claims"][1]["id"] = json!("unfinished-duration");
    assertions["final_review"]["finding_claim_ids"][1] = json!("unfinished-duration");
    assertions["final_review"]["unresolved_question_ids"] = json!([]);
    let (ok, value) = verify(&assertions);
    assert!(!ok);
    assert_eq!(
        value["artifact_retrieval"]["claim_verification"]["checks"][1]["status"],
        "verified"
    );
    assert_eq!(
        value["artifact_retrieval"]["claim_verification"]["requirement_results"][1]["status"],
        "needs_revision"
    );
    assert!(verify(&positive).0);
    // A source-bound cached/contrary interval also has the wrong ending name.
    let mut wrong_interval = draft["claims"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "interval" && c["subject"]["source"]["evidence_ref"]["line"] == 2)
        .unwrap()
        .clone();
    wrong_interval["id"] = json!("unfinished-duration");
    assertions["claims"][1] = wrong_interval;
    assert!(!verify(&assertions).0);
    assert!(verify(&positive).0);
    // A known supported interval cannot be hidden in the unresolved list.
    assertions = positive.clone();
    assertions["claims"] = json!([assertions["claims"][1]]);
    assertions["unresolved"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"finished-duration","reason":"Unknown"}));
    assertions["final_review"]["summary_claim_ids"] = json!([]);
    assertions["final_review"]["finding_claim_ids"] = json!(["contrary-link"]);
    assertions["final_review"]["unresolved_question_ids"] =
        json!(["finished-duration", "unfinished-duration"]);
    let (ok, value) = verify(&assertions);
    assert!(!ok);
    assert_eq!(
        value["artifact_retrieval"]["claim_verification"]["requirement_results"][0]["reason"],
        "requested_interval_available_cannot_declare_unknown"
    );
    assert!(verify(&positive).0);
    let mut missing_summary_unknown = positive.clone();
    missing_summary_unknown["final_review"]["unresolved_question_ids"] = json!([]);
    assert!(!verify(&missing_summary_unknown).0);
    assert!(verify(&positive).0);
}
