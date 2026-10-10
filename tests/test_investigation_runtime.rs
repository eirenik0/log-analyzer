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
    let artifact = temp.path().join("evidence.json");
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
    assert_eq!(
        text.lines().filter(|line| line.starts_with("- [")).count(),
        20
    );
    let next = text
        .lines()
        .find_map(|line| line.strip_prefix("Next: "))
        .unwrap();
    assert!(next.contains(" --report-cursor v1:"), "{next}");
    // The synthetic fixture's paths contain no whitespace, so execute the printed arguments directly.
    let args: Vec<_> = next
        .split_whitespace()
        .skip(1)
        .map(|arg| arg.trim_matches('\''))
        .collect();
    let output = run(&args);
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
            .find(|finding| finding["id"] == "scope-0-resource-observation-assets-0")
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
