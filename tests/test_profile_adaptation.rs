//! Independently authored source witnesses exercise adaptation, not model answer quality.
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::{TempDir, tempdir};
fn cli(args: &[&str]) -> (Value, bool) {
    let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .args(args)
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/report.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<_> = validator
        .iter_errors(&value)
        .map(|e| format!("{}: {e}", e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    (value, output.status.success())
}
fn row(event: &str, session: &str, capture: &str) -> Value {
    json!({"timestamp":"2026-01-01T00:00:00+02:00","level":"INFO","component":"worker-v2","component_id":"worker","message":"generic capture record","record_type":"capture","event_type":event,"session_id":session,"capture_id":capture,"request_id":"request-1","broker_id":"job-1","poll_id":"poll-1","payload":{"asExpected":false,"status":"Passed"}})
}
fn address(input: usize, line: usize) -> Value {
    json!({"input":input,"line":line,"row_path":null})
}
fn link(step: &str, source: usize, target: usize, linked: bool) -> Value {
    json!({"chain":"capture","step":step,"root":address(0,1),"source":address(0,source),"target":address(0,target),"linked":linked})
}
fn fixture(events: &[Value], suffix: &str) -> (TempDir, PathBuf, PathBuf) {
    let dir = tempdir().unwrap();
    let profile = dir.path().join("profile.toml");
    let input = dir.path().join("capture.jsonl");
    fs::write(
        &profile,
        format!(
            "{}\n{}",
            include_str!("../examples/investigations/chains/profile.toml"),
            suffix
        ),
    )
    .unwrap();
    fs::write(
        &input,
        events.iter().map(|e| format!("{e}\n")).collect::<String>(),
    )
    .unwrap();
    (dir, profile, input)
}
fn validate(dir: &Path, profile: &Path, input: &Path, links: &[Value]) -> (Value, bool) {
    let facts = dir.join("facts.json");
    fs::write(&facts, json!({"version":1,"links":links}).to_string()).unwrap();
    cli(&[
        "--profile",
        profile.to_str().unwrap(),
        "profile",
        "validate",
        input.to_str().unwrap(),
        "--kind",
        "event",
        "--purpose",
        "recognition",
        "--expected",
        facts.to_str().unwrap(),
        "--complete-output",
    ])
}
#[test]
fn validates_identity_bridges_late_ends_scope_exclusions_and_separate_outcomes() {
    let events = [
        row("checkpoint", "run-a", "capture-1"),
        row("submitted", "run-a", "capture-1"),
        row("accepted", "run-a", "capture-1"),
        row("poll_send", "run-a", "capture-1"),
        row("poll_response", "run-a", "capture-1"),
        row("capture_end", "run-other", "capture-1"),
        row("capture_end", "run-a", "capture-1"),
        row("check_end", "run-a", "capture-1"),
        row("session_end", "run-a", "capture-1"),
        row("contrary", "run-a", "capture-1"),
        row("later_activity", "run-a", "capture-other"),
    ];
    let (dir, profile, input) = fixture(&events, "");
    let assertions = [
        link("submitted", 1, 2, true),
        link("accepted", 2, 3, true),
        link("responses", 4, 5, true),
        link("capture-end", 1, 6, false),
        link("capture-end", 1, 7, true),
        link("check-end", 1, 8, true),
        link("session-end", 1, 9, true),
    ];
    let (value, success) = validate(dir.path(), &profile, &input, &assertions);
    assert!(success);
    let r = &value["profile_validation"];
    assert_eq!(r["relationships"]["status"], "passed_on_asserted_links");
    assert_eq!(r["relationships"]["negative_checks"], 1);
    assert_eq!(r["relationships"]["unasserted_supported_links"], 2);
    assert_eq!(
        r["relationships"]["roots"][0]["later_activity"][0]["source"]["line"],
        11
    );
    assert!(r["expected_results"][2]["target_source"]["evidence_ref"].is_object());
}
#[test]
fn negative_assertions_cannot_pass_unrecognized_roles_missing_keys_or_predecessors() {
    for variant in ["unrecognized", "missing-key", "predecessor"] {
        let mut events = vec![
            row("checkpoint", "run-a", "capture-1"),
            row("submitted", "run-a", "capture-1"),
            row("accepted", "run-a", "capture-1"),
        ];
        match variant {
            "unrecognized" => events[2]["event_type"] = json!("unrecognized"),
            "missing-key" => {
                events[2].as_object_mut().unwrap().remove("request_id");
            }
            _ => events[1]["capture_id"] = json!("different-capture"),
        }
        let (dir, profile, input) = fixture(&events, "");
        let (value, success) = validate(
            dir.path(),
            &profile,
            &input,
            &[link("accepted", 2, 3, false)],
        );
        assert!(!success, "{variant}");
        let check = &value["profile_validation"]["expected_results"][0];
        assert_eq!(check["observed_available"], false, "{variant}");
        assert!(check["observed_linked"].is_null());
    }
}
#[test]
fn ambiguous_joins_and_reused_roots_are_explicit_exclusions() {
    for duplicate in ["checkpoint", "submitted"] {
        let events = [
            row("checkpoint", "run-a", "capture-1"),
            row("submitted", "run-a", "capture-1"),
            row(duplicate, "run-a", "capture-1"),
        ];
        let (dir, profile, input) = fixture(&events, "");
        let (value, success) = validate(
            dir.path(),
            &profile,
            &input,
            &[link("submitted", 1, 2, false)],
        );
        assert!(success);
        let check = &value["profile_validation"]["expected_results"][0];
        assert_eq!(check["observed_linked"], false);
        assert_eq!(check["link_reason"], "ambiguous_cardinality_or_self_join");
    }
}
#[test]
fn exact_links_are_checked_before_bundle_sampling() {
    let mut events = vec![
        row("checkpoint", "run-a", "capture-1"),
        row("submitted", "run-a", "capture-1"),
        row("accepted", "run-a", "capture-1"),
    ];
    for i in 0..321 {
        let mut event = row("poll_send", "run-a", "capture-1");
        event["poll_id"] = json!(format!("poll-{i}"));
        events.push(event);
    }
    let mut response = row("poll_response", "run-a", "capture-1");
    response["poll_id"] = json!("poll-159");
    events.push(response);
    let (dir, profile, input) = fixture(&events, "");
    let (value, success) = validate(
        dir.path(),
        &profile,
        &input,
        &[link("responses", 163, 325, true)],
    );
    assert!(success);
    assert_eq!(
        value["profile_validation"]["expected_results"][0]["observed_linked"],
        true
    );
}
#[test]
fn adaptation_reports_starting_selector_failures_shapes_and_later_context() {
    let events = [
        row("checkpoint", "run-a", "capture-1"),
        row("submitted", "run-a", "capture-1"),
        row("later_activity", "run-a", "other"),
    ];
    let (dir, profile, input) = fixture(&events, "");
    let template = fs::read_to_string(&profile)
        .unwrap()
        .replace("equals = \"checkpoint\"", "equals = \"different-root\"");
    fs::write(&profile, template).unwrap();
    let destination = dir.path().join("candidate.toml");
    let (value, _) = cli(&[
        "profile",
        "prepare",
        input.to_str().unwrap(),
        "--template",
        profile.to_str().unwrap(),
        "--candidate-output",
        destination.to_str().unwrap(),
        "--kind",
        "event",
        "--purpose",
        "recognition",
        "--witness-limit",
        "32",
    ]);
    let packet = &value["profile_preparation"]["adaptation"];
    assert_eq!(packet["activation"], false);
    let role = packet["roles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["role"] == "checkpoint")
        .unwrap();
    assert_eq!(role["rule_matched_records"], 3);
    assert_eq!(role["matched_records"], 0);
    assert_eq!(role["failed_selectors"][0]["failed_records"], 3);
    assert_eq!(
        packet["inputs"][0]["records"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["source"]["line"],
        3
    );
    assert!(
        packet["inputs"][0]["field_shapes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["path"] == "structured:capture_id")
    );
    let saved = fs::read_to_string(destination).unwrap();
    assert!(saved.contains("different-root"));
}
#[test]
fn direct_relationships_use_runtime_scope_and_cardinality() {
    let suffix = r#"
[[investigation.relationships]]
id = "submission"
source_role = "checkpoint"
target_role = "submitted"
join_fields = ["capture_id"]
required_scope_fields = ["session_id"]
cardinality = "one_to_one"
"#;
    let events = [
        row("checkpoint", "run-a", "capture-1"),
        row("submitted", "run-a", "capture-1"),
        row("submitted", "other-run", "capture-1"),
    ];
    let (dir, profile, input) = fixture(&events, suffix);
    let facts = [
        json!({"relationship":"submission","source":address(0,1),"target":address(0,2),"linked":true}),
        json!({"relationship":"submission","source":address(0,1),"target":address(0,3),"linked":false}),
    ];
    let (value, success) = validate(dir.path(), &profile, &input, &facts);
    assert!(success);
    assert_eq!(
        value["profile_validation"]["relationships"]["failed_checks"],
        0
    );
}
#[test]
fn missing_source_fails_instead_of_becoming_negative_evidence() {
    let (dir, profile, input) = fixture(&[row("checkpoint", "run-a", "capture-1")], "");
    let (value, success) = validate(
        dir.path(),
        &profile,
        &input,
        &[link("submitted", 99, 1, false)],
    );
    assert!(!success);
    assert_eq!(
        value["profile_validation"]["expected_results"][0]["reason"],
        "missing_source"
    );
}

#[test]
fn adapts_text_wording_and_renamed_structured_fields_without_changing_roles() {
    let base = include_str!("../examples/investigations/chains/profile.toml");
    let adapter_begin = base.find("[event_rules.rules.adapter]").unwrap();
    let investigation = base.find("[investigation]").unwrap();
    let events = [
        "checkpoint",
        "submitted",
        "capture_end",
        "capture_end",
        "later_activity",
    ];
    let assertions = [
        link("submitted", 1, 2, true),
        link("capture-end", 1, 3, false),
        link("capture-end", 1, 4, true),
    ];
    for format in ["current", "json-lines"] {
        let dir = tempdir().unwrap();
        let profile = dir.path().join("adapted.toml");
        let input = dir.path().join("events.log");
        let candidate = dir.path().join("prepared.toml");
        let mut declarations = base[investigation..].to_owned();
        for (old, new) in [
            ("capture_id", "payload.capture"),
            ("session_id", "payload.run"),
            ("request_id", "payload.ticket"),
            ("broker_id", "payload.job"),
            ("poll_id", "payload.poll"),
        ] {
            declarations = declarations.replace(&format!("\"{old}\""), &format!("\"{new}\""));
        }
        let adapter = if format == "current" {
            "type = 'text'\npattern = 'Witness (?P<role>[a-z_]+) data (?s:.*)'\n"
        } else {
            "type = 'structured'\nconditions = [{field = 'payload.family', equals = 'synthetic'}]\n"
        };
        let name = if format == "current" {
            "{from = 'capture', capture = 'role'}"
        } else {
            "{from = 'field', field = 'payload.action'}"
        };
        let mapping = format!(
            "[event_rules.rules.mapping]\nkind = 'event'\nname = {name}\ncorrelation_id = {{from = 'field', field = 'payload.capture'}}\nscope = [{{from = 'field', field = 'payload.run'}}]\n"
        );
        let header = base[..adapter_begin].replace(
            "format = \"json-lines\"",
            &format!("format = \"{format}\"\nevent_payload_separator = 'data'"),
        );
        fs::write(
            &profile,
            format!("{header}[event_rules.rules.adapter]\n{adapter}{mapping}\n{declarations}"),
        )
        .unwrap();
        let data = events.iter().enumerate().map(|(i,event)| {
            let payload = json!({"family":"synthetic","action":event,"run":if i==2{"other-run"}else{"run-a"},"capture":"capture-1","ticket":"request-1","job":"job-1","poll":"poll-1"});
            if format == "current" {
                format!("different-worker (instance-2) | 2026-01-01T00:00:0{i}+02:00 [INFO] Witness {event} data {payload}\n")
            } else {
                format!("{}\n",json!({"timestamp":format!("2026-01-01T00:00:0{i}+02:00"),"level":"INFO","component":"other-component","component_id":"different-instance","message":"custom transport record","payload":payload}))
            }
        }).collect::<String>();
        fs::write(&input, data).unwrap();
        let (prepared, _) = cli(&[
            "profile",
            "prepare",
            input.to_str().unwrap(),
            "--template",
            profile.to_str().unwrap(),
            "--candidate-output",
            candidate.to_str().unwrap(),
            "--kind",
            "event",
            "--purpose",
            "recognition",
        ]);
        assert_eq!(
            prepared["profile_preparation"]["adaptation"]["roles"][0]["matched_records"],
            1,
            "{format}: {}",
            prepared["profile_preparation"]["adaptation"]["inputs"][0]["records"][0]["classification"]
        );
        let (validated, success) = validate(dir.path(), &candidate, &input, &assertions);
        assert!(success, "{format}: {validated}");
        assert_eq!(
            validated["profile_validation"]["relationships"]["status"],
            "passed_on_asserted_links"
        );
    }
}

#[test]
fn info_cached_failures_join_without_inventing_new_submissions() {
    let suffix = r#"
[[investigation.roles]]
id = "cached-failure"
rule_ids = ["typed-event"]
select = [{field = "event.name", equals = "cache_hit"}]
[[investigation.chains.steps]]
id = "cached-failure"
label = "previous failure returned from cache"
role = "cached-failure"
from = "$root"
join_fields = ["capture_id"]
cardinality = "one_to_many"
event_type = "cached_failure"
outcome_level = "cached_capture"
outcome_field = "payload.failure"
"#;
    let mut cached = row("cache_hit", "run-a", "capture-1");
    cached["payload"]["failure"] = json!("cached failure");
    let (dir, profile, input) = fixture(
        &[
            row("checkpoint", "run-a", "capture-1"),
            cached,
            row("capture_end", "run-a", "capture-1"),
        ],
        suffix,
    );
    let (value, success) = validate(
        dir.path(),
        &profile,
        &input,
        &[
            link("cached-failure", 1, 2, true),
            link("capture-end", 1, 3, true),
        ],
    );
    assert!(success);
    let steps = value["profile_validation"]["relationships"]["chains"][0]["steps"]
        .as_array()
        .unwrap();
    assert_eq!(
        steps.iter().find(|s| s["step"] == "submitted").unwrap()["linked_count"],
        0
    );
    assert_eq!(
        steps
            .iter()
            .find(|s| s["step"] == "cached-failure")
            .unwrap()["linked_count"],
        1
    );
}

#[test]
fn rejected_input_cannot_verify_negative_cardinality_or_positive_links() {
    let (dir, profile, input) = fixture(
        &[
            row("checkpoint", "run-a", "capture-1"),
            row("submitted", "run-a", "capture-1"),
        ],
        "",
    );
    let mut data = fs::read_to_string(&input).unwrap();
    data.push_str("this record has unavailable grammar\n");
    fs::write(&input, data).unwrap();
    let (value, success) = validate(
        dir.path(),
        &profile,
        &input,
        &[link("submitted", 1, 2, true)],
    );
    assert!(!success);
    let validation = &value["profile_validation"];
    assert_eq!(validation["relationships"]["status"], "unavailable");
    assert!(validation["relationships"]["supported_links"].is_null());
    assert_eq!(
        validation["expected_results"][0]["reason"],
        "input_structure_unavailable"
    );
    assert_eq!(
        validation["expected_results"][0]["observed_available"],
        false
    );
    assert_eq!(validation["expected_facts"]["status"], "failed");
}

#[test]
fn normalized_assertions_resolve_exact_rows_and_redact_private_paths() {
    let suffix = "\n[normalization]\nroot_path = '/private-rows'\nexpand_rows = true\n";
    let mut events = [
        row("checkpoint", "run-a", "capture-1"),
        row("submitted", "run-a", "capture-1"),
    ];
    for event in &mut events {
        event["component_id"] = json!("private-rows");
    }
    let (dir, profile, input) = fixture(&events, suffix);
    fs::write(&input, json!({"private-rows":events}).to_string()).unwrap();
    let mut assertion = link("submitted", 1, 1, true);
    for (field, row) in [("root", 0), ("source", 0), ("target", 1)] {
        assertion[field]["row_path"] = json!(format!("/private-rows/{row}"));
    }
    let (value, success) = validate(dir.path(), &profile, &input, &[assertion.clone()]);
    assert!(success);
    assert_eq!(
        value["profile_validation"]["expected_results"][0]["target_source"]["row_path"],
        "/private-rows/1"
    );
    let facts = dir.path().join("facts.json");
    let (redacted, success) = cli(&[
        "--profile",
        profile.to_str().unwrap(),
        "--redact",
        "--mask-id",
        "component_id",
        "profile",
        "validate",
        input.to_str().unwrap(),
        "--kind",
        "event",
        "--purpose",
        "recognition",
        "--expected",
        facts.to_str().unwrap(),
        "--complete-output",
    ]);
    assert!(success);
    assert!(!redacted.to_string().contains("private-rows"));
    assert_eq!(
        redacted["profile_validation"]["expected_results"][0]["assertion"]["target"]["location_redacted"],
        true
    );
    assertion["target"]["row_path"] = json!("/private-rows/99");
    let (_, success) = validate(dir.path(), &profile, &input, &[assertion]);
    assert!(!success);
}

#[test]
fn preparation_masks_numeric_selector_values_and_preserves_typed_metadata() {
    let events = [
        row("checkpoint", "run-a", "capture-1"),
        row("submitted", "run-a", "capture-1"),
    ];
    let (dir, profile, input) = fixture(&events, "");
    let text = fs::read_to_string(&profile).unwrap().replace(
        "select = [{field = \"event.name\", equals = \"checkpoint\"}]",
        "select = [{field = \"payload.private_selector\", equals = 987654321}]",
    );
    fs::write(&profile, text).unwrap();
    let candidate = dir.path().join("candidate.toml");
    let (value, _) = cli(&[
        "--redact",
        "--mask-id",
        "private_selector",
        "--mask-id",
        "roles",
        "--mask-id",
        "status",
        "profile",
        "prepare",
        input.to_str().unwrap(),
        "--template",
        profile.to_str().unwrap(),
        "--candidate-output",
        candidate.to_str().unwrap(),
        "--kind",
        "event",
        "--purpose",
        "recognition",
        "--witness-limit",
        "32",
    ]);
    assert!(!value.to_string().contains("987654321"));
    assert_eq!(
        value["profile_preparation"]["adaptation"]["basis"],
        "starting_profile_observations"
    );
    assert_eq!(
        value["profile_preparation"]["adaptation"]["roles"][0]["matched_records"],
        0
    );
}

#[test]
fn identical_ids_in_independent_files_cannot_link() {
    let events = [
        row("checkpoint", "run-a", "capture-1"),
        row("submitted", "run-a", "capture-1"),
    ];
    let (dir, profile, input) = fixture(&events, "");
    let other = dir.path().join("other.jsonl");
    fs::copy(&input, &other).unwrap();
    let facts = dir.path().join("facts.json");
    for expected in [false, true] {
        let mut assertion = link("submitted", 1, 2, expected);
        assertion["target"]["input"] = json!(1);
        fs::write(&facts, json!({"version":1,"links":[assertion]}).to_string()).unwrap();
        let (value, success) = cli(&[
            "--profile",
            profile.to_str().unwrap(),
            "profile",
            "validate",
            input.to_str().unwrap(),
            other.to_str().unwrap(),
            "--kind",
            "event",
            "--purpose",
            "recognition",
            "--expected",
            facts.to_str().unwrap(),
            "--complete-output",
        ]);
        assert_eq!(success, !expected);
        assert_eq!(
            value["profile_validation"]["expected_results"][0]["observed_linked"],
            false
        );
    }
}

#[test]
fn duplicate_witnesses_do_not_cover_other_links_and_wrong_negatives_fail() {
    let events = [
        row("checkpoint", "run-a", "capture-1"),
        row("submitted", "run-a", "capture-1"),
        row("capture_end", "run-a", "capture-1"),
    ];
    let (dir, profile, input) = fixture(&events, "");
    let fact = link("submitted", 1, 2, true);
    let (value, success) = validate(dir.path(), &profile, &input, &[fact.clone(), fact]);
    assert!(success);
    assert_eq!(
        value["profile_validation"]["relationships"]["supported_links"],
        2
    );
    assert_eq!(
        value["profile_validation"]["relationships"]["unasserted_supported_links"],
        1
    );
    let (value, success) = validate(
        dir.path(),
        &profile,
        &input,
        &[link("submitted", 1, 2, false)],
    );
    assert!(!success);
    assert_eq!(
        value["profile_validation"]["expected_results"][0]["reason"],
        "link_mismatch"
    );
}

#[test]
fn strict_link_fact_contract_rejects_missing_addresses_and_extra_fields() {
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/profile-expectations.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let (dir, profile, input) = fixture(&[row("checkpoint", "run-a", "capture-1")], "");
    for variant in ["row_path", "linked", "extra", "chain"] {
        let mut assertion = link("submitted", 1, 1, false);
        match variant {
            "row_path" => {
                assertion["target"]
                    .as_object_mut()
                    .unwrap()
                    .remove("row_path");
            }
            "linked" => {
                assertion.as_object_mut().unwrap().remove("linked");
            }
            "extra" => assertion["guess"] = json!(true),
            _ => assertion["chain"] = json!(""),
        }
        let facts = json!({"version":1,"links":[assertion]});
        assert!(!validator.is_valid(&facts), "{variant}");
        let path = dir.path().join("facts.json");
        fs::write(&path, facts.to_string()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
            .args([
                "--profile",
                profile.to_str().unwrap(),
                "profile",
                "validate",
                input.to_str().unwrap(),
                "--kind",
                "event",
                "--purpose",
                "recognition",
                "--expected",
                path.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{variant}");
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn relationship_processing_cutoff_cannot_verify_a_negative_link() {
    let mut events = vec![row("checkpoint", "run-a", "capture-1")];
    for _ in 0..1600 {
        let mut event = row("submitted", "run-a", "capture-1");
        event["message"] = json!("x".repeat(2048));
        events.push(event);
    }
    let (dir, profile, input) = fixture(&events, "");
    let (value, success) = validate(
        dir.path(),
        &profile,
        &input,
        &[link("submitted", 1, 2, false)],
    );
    assert!(!success);
    let validation = &value["profile_validation"];
    assert_eq!(validation["relationships"]["status"], "unavailable");
    assert_eq!(
        validation["relationships"]["processing"]["status"],
        "partial"
    );
    assert_eq!(
        validation["relationships"]["processing"]["stop"]["reason"],
        "memory_limit"
    );
    assert!(validation["relationships"]["supported_links"].is_null());
    assert!(validation["relationships"]["unasserted_supported_links"].is_null());
    assert_eq!(
        validation["expected_results"][0]["observed_available"],
        false
    );
    assert!(validation["expected_results"][0]["observed_linked"].is_null());
}

#[test]
fn field_bridges_link_renamed_payload_keys_but_reject_conflicting_identifiers() {
    let mut root = row("checkpoint", "run-a", "capture-a");
    root["payload"] = json!({"resource":"asset-a"});
    let mut submitted = row("submitted", "run-a", "capture-a");
    submitted["payload"] = json!({"resources":[{"resource":"asset-a"}]});
    let mut other_scope = submitted.clone();
    other_scope["session_id"] = json!("run-b");
    let (dir, profile, input) = fixture(&[root.clone(), submitted.clone(), other_scope], "");
    let adapted = fs::read_to_string(&profile).unwrap().replace(
        "join_fields = [\"capture_id\"]",
        "join_fields = [\"coalesce:payload.resource|payload.resources.0.resource\"]",
    );
    fs::write(&profile, adapted).unwrap();
    let (report, success) = validate(
        dir.path(),
        &profile,
        &input,
        &[
            link("submitted", 1, 2, true),
            link("submitted", 1, 3, false),
        ],
    );
    assert!(success, "{report}");
    assert_eq!(
        report["profile_validation"]["relationships"]["status"],
        "passed_on_asserted_links"
    );
    root["payload"]["resources"] = json!([{"resource":"different-asset"}]);
    fs::write(&input, format!("{root}\n{submitted}\n")).unwrap();
    let (report, success) = validate(
        dir.path(),
        &profile,
        &input,
        &[link("submitted", 1, 2, true)],
    );
    assert!(!success);
    assert_eq!(
        report["profile_validation"]["relationships"]["failed_checks"],
        1
    );
}
