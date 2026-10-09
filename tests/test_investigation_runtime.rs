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
    let output = run(args);
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
            "--config",
            profile.to_str().unwrap(),
            "investigate",
            "source.jsonl",
            "--artifact",
            artifact.to_str().unwrap(),
        ]);
        if prefix {
            cmd.args(["--input-max-bytes", "128"]);
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
    assert_eq!(count(&report, "paired-lifecycles"), 0);
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
                "supported"
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
                report["populations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["id"] == "scope-0-failures")
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
