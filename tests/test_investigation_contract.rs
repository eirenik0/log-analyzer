use log_analyzer::{
    evidence::digest,
    investigation::{OccurrenceId, validate_relations},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, process::Command};

fn fixture(name: &str) -> (Value, Value) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/investigation-contract");
    let report: Value =
        serde_json::from_slice(&fs::read(root.join(format!("{name}.report.json"))).unwrap())
            .unwrap();
    let artifact: Value = serde_json::from_slice(
        &fs::read(root.join(report["artifact"]["location"].as_str().unwrap())).unwrap(),
    )
    .unwrap();
    (report, artifact)
}
fn schema(name: &str) -> Value {
    serde_json::from_slice(
        &fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("schemas")
                .join(name),
        )
        .unwrap(),
    )
    .unwrap()
}
fn shape(value: &Value, name: &str) {
    let validator = jsonschema::validator_for(&schema(name)).unwrap();
    let errors: Vec<_> = validator
        .iter_errors(value)
        .map(|v| v.to_string())
        .collect();
    assert!(errors.is_empty(), "{name}: {errors:?}");
}
fn bytes(artifact: &Value) -> Vec<u8> {
    serde_json::to_vec(artifact).unwrap()
}
fn refresh(report: &mut Value, artifact: &Value) -> Vec<u8> {
    let bytes = bytes(artifact);
    report["artifact"]["stored_sha256"] = json!(digest(&bytes));
    bytes
}
fn refresh_membership(artifact: &mut Value, ordinal: usize) {
    let members = &artifact["memberships"][ordinal]["members"];
    artifact["populations"][ordinal]["membership"]["sha256"] =
        json!(digest(&serde_json::to_vec(members).unwrap()));
}
fn reject(mut report: Value, artifact: Value, expected: &str) {
    let bytes = refresh(&mut report, &artifact);
    shape(&report, "investigation.schema.json");
    shape(&artifact, "evidence-artifact.schema.json");
    let error = validate_relations(&report, Some(&bytes)).unwrap_err();
    assert!(
        error.to_string().contains(expected),
        "{error} did not contain {expected}"
    );
}
fn invoke(args: &[&str]) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
    command.current_dir(env!("CARGO_MANIFEST_DIR"));
    for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("LOG_ANALYZER_")) {
        command.env_remove(key);
    }
    let output = command.args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn published_reports_and_artifacts_validate_shape_and_semantics() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/investigation-contract");
    for name in [
        "supported",
        "insufficient",
        "conflicting",
        "unsupported",
        "processing-limited",
        "output-limited",
        "redacted",
        "measured-zero",
        "zero-filter-matches",
    ] {
        let (report, artifact) = fixture(name);
        shape(&report, "investigation.schema.json");
        shape(&artifact, "evidence-artifact.schema.json");
        let raw = fs::read(root.join(report["artifact"]["location"].as_str().unwrap())).unwrap();
        let summary =
            validate_relations(&report, Some(&raw)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(summary.artifact_checked);
        if name == "redacted" {
            assert_eq!(
                summary.deferred_relations,
                vec!["unredacted_query_and_input_identity"]
            );
        } else {
            assert!(summary.deferred_relations.is_empty());
        }
    }
}
#[test]
fn new_contract_is_discoverable_without_claiming_an_unimplemented_command() {
    let caps = invoke(&["capabilities"]);
    shape(&caps, "capabilities.schema.json");
    assert_eq!(caps["investigation_contracts"]["versions"], json!([1]));
    assert_eq!(caps["investigation_contracts"]["command_available"], false);
    assert_eq!(
        caps["investigation_contracts"]["artifact_retrieval_available"],
        false
    );
    assert_eq!(caps["bounded_reports"]["version"], 1);
    assert_eq!(caps["report_schemas"]["evidence_contract_version"], 1);
    for (key, path) in [
        ("investigation", "investigation.schema.json"),
        ("evidence_artifact", "evidence-artifact.schema.json"),
    ] {
        assert_eq!(caps["report_schemas"][key], schema(path));
    }
    let old = json!({"contract_version":1,"input_snapshot_id":"a".repeat(64),"profile_sha256":"b".repeat(64),"status":"insufficient_evidence","findings":[{"kind":"unknown","claim":"Completion unavailable","supporting_refs":[],"reason":"No end boundary"}]});
    let validator = jsonschema::validator_for(&caps["report_schemas"]["investigation"]).unwrap();
    assert!(validator.is_valid(&old));
    for name in ["investigation.schema.json", "evidence-artifact.schema.json"] {
        let new = schema(name);
        let shared = schema("report.schema.json");
        for definition in ["reference", "metadata", "evidence", "coverage", "digest"] {
            assert_eq!(new["$defs"][definition], shared["$defs"][definition]);
        }
    }
}
#[test]
fn measured_intervals_and_population_counts_agree_with_existing_engine() {
    let native = invoke(&[
        "--config",
        "examples/investigations/profile.toml",
        "--complete-output",
        "perf",
        "examples/investigations/slow.jsonl",
    ]);
    let (report, artifact) = fixture("supported");
    assert_eq!(
        native["report_metadata"]["evidence"],
        report["report_metadata"]["evidence"]
    );
    let measurements: Vec<_> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "measurement")
        .collect();
    assert_eq!(
        measurements.len(),
        native["operations"].as_array().unwrap().len()
    );
    for (finding, op) in measurements
        .iter()
        .zip(native["operations"].as_array().unwrap())
    {
        assert_eq!(finding["details"]["value"], op["duration_ms"]);
        for boundary in ["start", "end"] {
            assert_eq!(
                finding["details"]["boundaries"][boundary]["occurrence"]["evidence_ref"],
                op[format!("{boundary}_source")]["evidence_ref"]
            );
        }
    }
    let config = log_analyzer::config::load_config_from_path(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/investigations/profile.toml")
            .as_path(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(config).unwrap(),
        artifact["effective_profile"]
    );
    assert_eq!(
        report["populations"][1]["count"],
        native["report_metadata"]["evidence"]["scope"]["selected_entries"]
    );
}
#[test]
fn repeated_inputs_are_distinct_occurrences_and_not_distinct_sources() {
    let native = invoke(&[
        "--config",
        "examples/investigations/profile.toml",
        "--complete-output",
        "perf",
        "examples/investigations/slow.jsonl",
        "examples/investigations/slow.jsonl",
    ]);
    let snapshot = &native["report_metadata"]["evidence"]["snapshot_id"];
    let mut occurrences = BTreeSet::new();
    let mut references = BTreeSet::new();
    for record in native["evidence_records"].as_array().unwrap() {
        let occurrence = json!({"snapshot_id":snapshot,"input_ordinal":record["input_ordinal"],"evidence_ref":record["evidence_ref"]});
        assert!(occurrences.insert(OccurrenceId::from_value(&occurrence).unwrap()));
        references.insert(record["evidence_ref"]["reference_id"].as_str().unwrap());
    }
    assert_eq!(occurrences.len(), 12);
    assert_eq!(references.len(), 6);
    assert_eq!(
        native["report_metadata"]["evidence"]["scope"]["selected_entries"],
        12
    );
    assert!(native["operations"].as_array().unwrap().is_empty());
}
#[test]
fn unread_out_of_order_boundary_invalidates_an_apparent_pair() {
    let native = invoke(&[
        "--config",
        "examples/investigations/profile.toml",
        "--complete-output",
        "perf",
        "examples/investigation-contract/cutoff.jsonl",
    ]);
    assert!(native["operations"].as_array().unwrap().is_empty());
    assert_eq!(native["ambiguous_groups"].as_array().unwrap().len(), 1);
    let (partial, artifact) = fixture("processing-limited");
    assert_eq!(partial["processing"]["status"], "partial");
    assert_eq!(partial["assessments"][0]["status"], "insufficient_evidence");
    assert!(
        artifact["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["kind"] == "measurement" && f["details"]["value"] == 2000)
    );
    let mut wrong = partial.clone();
    wrong["assessments"][0]["status"] = json!("supported");
    let mut bad_artifact = artifact;
    bad_artifact["assessments"] = wrong["assessments"].clone();
    reject(wrong, bad_artifact, "partial scope");
}
#[test]
fn complete_capture_does_not_establish_completed_analysis() {
    let (mut report, mut artifact) = fixture("supported");
    for doc in [&mut report, &mut artifact] {
        doc["processing"]["status"] = json!("partial");
        doc["processing"]["stop"] = json!({"stage":"correlation","reason":"work_limit","limit_name":"work_units","scope_ids":["scope-1"]});
    }
    reject(report, artifact, "affected scope claims completed analysis");
}
#[test]
fn artifact_write_cutoff_preserves_completed_analysis() {
    let (mut report, mut artifact) = fixture("supported");
    for doc in [&mut report, &mut artifact] {
        doc["processing"]["status"] = json!("partial");
        doc["processing"]["stop"] = json!({"stage":"artifact_write","reason":"storage_limit","limit_name":"artifact_bytes","scope_ids":["scope-1"]});
    }
    report["artifact"]["status"] = json!("partial");
    let bytes = refresh(&mut report, &artifact);
    validate_relations(&report, Some(&bytes)).unwrap();
}
#[test]
fn negative_submillisecond_interval_is_not_measured_zero() {
    let (mut report, mut artifact) = fixture("measured-zero");
    for doc in [&mut report, &mut artifact] {
        doc["findings"][0]["details"]["boundaries"]["end"]["timestamp"] =
            json!("2025-12-31T23:59:59.999500+02:00");
    }
    reject(report, artifact, "duration differs");
}
#[test]
fn wrong_entity_missing_identity_and_duplicate_resources_are_rejected() {
    let (mut report, mut artifact) = fixture("supported");
    report["populations"][0]["entity"] = json!("physical_records");
    artifact["populations"] = report["populations"].clone();
    reject(report, artifact, "non-record member in record population");
    let (mut report, mut artifact) = fixture("supported");
    report["populations"][0]["identity_fields"]
        .as_array_mut()
        .unwrap()
        .push(json!("missing"));
    artifact["populations"] = report["populations"].clone();
    reject(report, artifact, "declared identity field missing");
    let (mut report, mut artifact) = fixture("supported");
    report["populations"][0]["entity"] = json!("resources");
    artifact["populations"] = report["populations"].clone();
    for member in artifact["memberships"][0]["members"]
        .as_array_mut()
        .unwrap()
    {
        member["kind"] = json!("resource");
    }
    artifact["memberships"][0]["members"][1]["identity"] =
        artifact["memberships"][0]["members"][0]["identity"].clone();
    refresh_membership(&mut artifact, 0);
    report["populations"] = artifact["populations"].clone();
    reject(report, artifact, "distinct resource identity");
}
#[test]
fn members_outside_population_scope_are_rejected() {
    let (mut report, mut artifact) = fixture("supported");
    report["scopes"][0]["correlation_scope"] = json!([{"field":"session","value":"another-run"}]);
    artifact["scopes"] = report["scopes"].clone();
    reject(report, artifact, "member correlation scope differs");
}
#[test]
fn distribution_uses_exact_population_members_and_declared_percentile_method() {
    let (mut report, mut artifact) = fixture("supported");
    let mut extra = artifact["findings"][0].clone();
    extra["id"] = json!("other-measurement");
    artifact["findings"]
        .as_array_mut()
        .unwrap()
        .push(extra.clone());
    report["findings"].as_array_mut().unwrap().push(extra);
    for doc in [&mut report, &mut artifact] {
        doc["findings"][4]["details"]["measurement_ids"][0] = json!("other-measurement");
    }
    report["presentation"]["displayed_findings"] = json!(7);
    report["presentation"]["total_findings"] = json!(7);
    reject(report, artifact, "samples differ from population");
    let (mut report, mut artifact) = fixture("supported");
    for doc in [&mut report, &mut artifact] {
        doc["findings"][4]["details"]["method"] = json!("maximum");
    }
    reject(report, artifact, "distribution value or method differs");
}
#[test]
fn changed_displayed_evidence_or_qualifications_are_not_hidden_by_saved_findings() {
    for field in ["claim", "limitations", "verification", "evidence"] {
        let (mut report, artifact) = fixture("supported");
        match field {
            "claim" => report["findings"][0]["claim"] = json!("unsupported claim"),
            "limitations" => report["findings"][0]["limitations"] = json!([]),
            "verification" => {
                report["findings"][0]["verification"]["arithmetic"] = json!("unavailable")
            }
            _ => report["findings"][0]["evidence"][0]["text"] = json!("changed excerpt"),
        }
        reject(
            report,
            artifact,
            if field == "evidence" {
                "invalid excerpt projection"
            } else {
                "material qualification differs"
            },
        );
    }
}
#[test]
fn essential_boundaries_are_retained_in_the_displayed_bundle() {
    let (mut report, artifact) = fixture("supported");
    report["findings"][0]["evidence"]
        .as_array_mut()
        .unwrap()
        .pop();
    reject(report, artifact, "essential supporting witness");
}
#[test]
fn forged_capture_bytes_and_lost_rules_cannot_claim_source_verification() {
    let (report, mut artifact) = fixture("supported");
    artifact["captured_inputs"][0]["data"] = json!("different bytes");
    artifact["captured_inputs"][0]["stored_sha256"] = json!(digest(b"different bytes"));
    reject(report, artifact, "original capture differs");
    let (report, mut artifact) = fixture("supported");
    artifact["effective_profile"] = Value::Null;
    artifact["effective_profile_omitted"] = json!(true);
    reject(report, artifact, "verification prerequisites lost");
}
#[test]
fn output_only_omission_can_defer_relations_without_reparsing() {
    let (report, artifact) = fixture("output-limited");
    let summary = validate_relations(&report, None).unwrap();
    assert!(!summary.artifact_checked);
    assert!(
        summary
            .deferred_relations
            .iter()
            .any(|v| v.starts_with("finding:"))
    );
    let mut report = report;
    let bytes = refresh(&mut report, &artifact);
    let complete = validate_relations(&report, Some(&bytes)).unwrap();
    assert!(complete.artifact_checked);
    assert!(complete.deferred_relations.is_empty());
}
#[test]
fn redacted_arithmetic_is_distinct_from_independent_source_checking() {
    let (mut report, artifact) = fixture("redacted");
    assert_eq!(
        report["artifact"]["verification"]["arithmetic"],
        "available"
    );
    assert_eq!(
        report["artifact"]["verification"]["source_and_rules"],
        "unavailable"
    );
    let bytes = refresh(&mut report, &artifact);
    validate_relations(&report, Some(&bytes)).unwrap();
}
#[test]
fn measured_zero_filter_zero_and_unsupported_input_remain_distinct() {
    let (zero, _) = fixture("measured-zero");
    assert_eq!(zero["findings"][0]["details"]["value"], 0);
    assert_ne!(
        zero["findings"][0]["details"]["boundaries"]["start"]["occurrence"],
        zero["findings"][0]["details"]["boundaries"]["end"]["occurrence"]
    );
    let (filtered, _) = fixture("zero-filter-matches");
    assert_eq!(
        filtered["report_metadata"]["evidence"]["scope"]["status"],
        "zero_filter_matches"
    );
    let (unsupported, _) = fixture("unsupported");
    assert_eq!(
        unsupported["report_metadata"]["evidence"]["scope"]["status"],
        "unparsed_input"
    );
    assert!(
        unsupported["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["kind"] == "unknown")
    );
}
#[test]
fn malformed_relation_input_returns_error_instead_of_panicking() {
    assert!(validate_relations(&json!({}), None).is_err());
    let (report, _) = fixture("supported");
    assert!(validate_relations(&report, Some(b"not JSON")).is_err());
}

#[test]
fn invented_saved_excerpt_and_overflowing_omissions_are_rejected() {
    let (mut report, mut artifact) = fixture("supported");
    for value in [&mut report, &mut artifact] {
        value["findings"][0]["evidence"][0]["text"] =
            json!("invented proof absent from the original logs");
    }
    reject(report, artifact, "excerpt differs from retained record");
    let (mut report, mut artifact) = fixture("supported");
    for value in [&mut report, &mut artifact] {
        value["findings"][0]["evidence"][0]["omitted_characters"] = json!(u64::MAX);
    }
    report["findings"][0]["evidence"][0]["text"] = json!("run star");
    reject(report, artifact, "omission count overflow");
}
#[test]
fn undeclared_identity_fields_do_not_create_distinct_resources() {
    let (mut report, mut artifact) = fixture("supported");
    artifact["populations"][0]["entity"] = json!("resources");
    let first = artifact["memberships"][0]["members"][0]["identity"].clone();
    for (i, member) in artifact["memberships"][0]["members"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        member["kind"] = json!("resource");
        member["identity"] = first.clone();
        member["identity"]
            .as_array_mut()
            .unwrap()
            .push(json!({"field":"undeclared","value":i.to_string()}));
    }
    refresh_membership(&mut artifact, 0);
    report["populations"] = artifact["populations"].clone();
    reject(report, artifact, "distinct resource identity");
}

#[test]
fn one_boundary_cannot_create_a_measured_zero() {
    let (mut report, mut artifact) = fixture("measured-zero");
    for value in [&mut report, &mut artifact] {
        value["findings"][0]["details"]["boundaries"]["end"] =
            value["findings"][0]["details"]["boundaries"]["start"].clone();
    }
    reject(report, artifact, "distinct boundary occurrences");
}
#[test]
fn manifest_identity_digests_are_recomputed() {
    for (pointer, value, expected) in [
        (
            "/query/filter",
            json!("message:changed"),
            "query identity digest mismatch",
        ),
        (
            "/inputs/0/input_id",
            json!("a".repeat(64)),
            "snapshot identity digest mismatch",
        ),
        (
            "/snapshot_id",
            json!("a".repeat(64)),
            "snapshot identity digest mismatch",
        ),
        (
            "/inputs/0/file",
            json!("changed.jsonl"),
            "input identity digest mismatch",
        ),
    ] {
        let (mut report, mut artifact) = fixture("supported");
        for document in [&mut report, &mut artifact] {
            *document["report_metadata"]["evidence"]
                .pointer_mut(pointer)
                .unwrap() = value.clone();
        }
        reject(report, artifact, expected);
    }
}
#[test]
fn forged_input_id_with_a_consistent_snapshot_is_rejected() {
    let (mut report, mut artifact) = fixture("supported");
    let fake = "a".repeat(64);
    let snapshot = digest(serde_json::to_vec(&json!([fake])).unwrap().as_slice());
    for document in [&mut report, &mut artifact] {
        document["report_metadata"]["evidence"]["inputs"][0]["input_id"] = json!(fake);
        document["report_metadata"]["evidence"]["snapshot_id"] = json!(snapshot);
    }
    reject(report, artifact, "input identity digest mismatch");
}
#[test]
fn supported_assessment_requires_a_finding() {
    let (mut report, mut artifact) = fixture("supported");
    for document in [&mut report, &mut artifact] {
        document["assessments"][0]["finding_ids"] = json!([]);
    }
    reject(report, artifact, "supported assessment requires a finding");
}
#[test]
fn byte_usage_matches_consumed_inputs_or_is_explicitly_unavailable() {
    let (mut report, mut artifact) = fixture("supported");
    for document in [&mut report, &mut artifact] {
        document["processing"]["usage"]["input_bytes"] = json!(0);
    }
    reject(report, artifact, "input byte usage differs");
    let (mut report, mut artifact) = fixture("supported");
    for document in [&mut report, &mut artifact] {
        document["processing"]["usage"]["input_bytes"] = Value::Null;
    }
    let raw = refresh(&mut report, &artifact);
    validate_relations(&report, Some(&raw)).unwrap();
}

#[test]
fn unparsed_coverage_cannot_carry_supported_measurements() {
    for clear_semantics in [false, true] {
        let (mut report, mut artifact) = fixture("supported");
        for document in [&mut report, &mut artifact] {
            document["report_metadata"]["evidence"]["inputs"][0]["coverage"]["parsed_entries"] =
                json!(0);
            document["report_metadata"]["evidence"]["inputs"][0]["selected_entries"] = json!(0);
            document["report_metadata"]["evidence"]["scope"]["parsed_entries"] = json!(0);
            document["report_metadata"]["evidence"]["scope"]["selected_entries"] = json!(0);
            document["report_metadata"]["evidence"]["scope"]["status"] = json!("unparsed_input");
            if clear_semantics {
                document["scopes"][0]["semantic_coverage"]["relevant_records"] = json!(0);
                document["scopes"][0]["semantic_coverage"]["classified_records"] = json!(0);
            }
        }
        reject(
            report,
            artifact,
            if clear_semantics {
                "source occurrence has no selected parse coverage"
            } else {
                "semantic coverage exceeds"
            },
        );
    }
}
#[test]
fn manifest_coverage_totals_and_status_match_each_input() {
    for (field, value) in [
        ("parsed_entries", json!(0)),
        ("selected_entries", json!(0)),
        ("status", json!("unparsed_input")),
    ] {
        let (mut report, mut artifact) = fixture("supported");
        for document in [&mut report, &mut artifact] {
            document["report_metadata"]["evidence"]["scope"][field] = value.clone();
        }
        reject(report, artifact, "parse coverage totals or status differ");
    }
}
#[test]
fn applied_redaction_rejects_original_persistence_and_verification() {
    let (mut report, mut artifact) = fixture("supported");
    for document in [&mut report, &mut artifact] {
        document["report_metadata"]["evidence"]["redaction"]["applied"] = json!(true);
    }
    reject(
        report,
        artifact,
        "applied redaction requires redacted persistence",
    );
    let (mut report, mut artifact) = fixture("redacted");
    for verification in [
        &mut report["artifact"]["verification"],
        &mut artifact["verification"],
    ] {
        verification["source_and_rules"] = json!("available");
        verification["losses"] = json!([]);
    }
    reject(
        report,
        artifact,
        "applied redaction requires redacted persistence",
    );
}
#[test]
fn redacted_artifact_cannot_retain_original_capture_or_profile() {
    let (report, mut artifact) = fixture("redacted");
    let (_, original) = fixture("supported");
    artifact["captured_inputs"] = original["captured_inputs"].clone();
    reject(report, artifact, "must omit captured data");
    let (report, mut artifact) = fixture("redacted");
    artifact["effective_profile"] = original["effective_profile"].clone();
    artifact["effective_profile_omitted"] = json!(false);
    reject(report, artifact, "cannot persist original effective rules");
}
#[test]
fn unknown_only_assessment_is_not_supported() {
    let (mut report, mut artifact) = fixture("supported");
    let (unsupported, _) = fixture("unsupported");
    for document in [&mut report, &mut artifact] {
        document["findings"]
            .as_array_mut()
            .unwrap()
            .push(unsupported["findings"][0].clone());
        document["assessments"][0]["finding_ids"] = json!(["unknown-1"]);
    }
    report["presentation"]["displayed_findings"] = json!(7);
    report["presentation"]["total_findings"] = json!(7);
    reject(report, artifact, "requires a positive finding");
}
#[test]
fn artifact_retention_matches_the_descriptor() {
    for policy in [
        json!({"policy":"until_deleted","expires_at":null}),
        json!({"policy":"expires_at","expires_at":"2026-12-01T00:00:00Z"}),
    ] {
        let (mut report, mut artifact) = fixture("supported");
        report["artifact"]["retention"] = policy.clone();
        reject(report.clone(), artifact.clone(), "retention differs");
        artifact["retention"] = policy;
        let raw = refresh(&mut report, &artifact);
        validate_relations(&report, Some(&raw)).unwrap();
    }
}

#[test]
fn redaction_cannot_be_proved_by_replacing_original_digests() {
    let (mut report, mut artifact) = fixture("redacted");
    let (_, original) = fixture("supported");
    artifact["captured_inputs"] = original["captured_inputs"].clone();
    for document in [&mut report, &mut artifact] {
        document["report_metadata"]["evidence"]["inputs"][0]["sha256"] = json!("a".repeat(64));
        document["report_metadata"]["evidence"]["inputs"][0]["coverage"]["snapshot_sha256"] =
            json!("a".repeat(64));
        document["processing"]["inputs"][0]["consumed_sha256"] = json!("a".repeat(64));
    }
    artifact["captured_inputs"][0]["original_consumed_sha256"] = json!("a".repeat(64));
    reject(report, artifact, "must omit captured data");
}
#[test]
fn capabilities_require_investigation_discovery_metadata() {
    let caps = invoke(&["capabilities"]);
    let validator = jsonschema::validator_for(&schema("capabilities.schema.json")).unwrap();
    for (parent, key) in [
        ("", "investigation_contracts"),
        ("/report_schemas", "evidence_artifact"),
        ("/report_schemas", "investigation_contract_versions"),
    ] {
        let mut missing = caps.clone();
        missing
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(!validator.is_valid(&missing), "missing {key} was accepted");
    }
}
#[test]
fn pagination_metadata_agrees_with_findings_and_omissions() {
    for (field, value, expected) in [
        ("total", 0, "collection counts do not reconcile"),
        ("displayed", 0, "collection counts do not reconcile"),
        ("prior", u64::MAX, "collection counts do not reconcile"),
    ] {
        let (mut report, artifact) = fixture("supported");
        report["presentation"]["collections"][0][field] = json!(value);
        reject(report, artifact, expected);
    }
    let (mut report, artifact) = fixture("supported");
    report["presentation"]["collections"][0] =
        json!({"path":"/findings","total":0,"prior":0,"displayed":0,"remaining":0});
    reject(report, artifact, "collection displayed count differs");
    let (mut report, artifact) = fixture("output-limited");
    report["presentation"]["status"] = json!("complete");
    reject(report, artifact, "complete presentation has omitted");
    let (mut report, artifact) = fixture("supported");
    report["presentation"]["status"] = json!("page");
    reject(report, artifact, "page presentation has no omissions");
}
