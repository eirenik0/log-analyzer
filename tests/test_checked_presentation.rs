use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

const PROFILE: &str = "examples/investigations/checked/profile.toml";
const INPUT: &str = "examples/investigations/checked/events.jsonl";

fn run(temp: &Path, name: &str, profile: &str, inputs: &[&str], options: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["--profile", profile, "investigate"])
        .args(inputs)
        .arg("--artifact")
        .arg(temp.join(name))
        .args(options)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn shape(report: &Value, brief: bool) {
    let schema: Value = serde_json::from_str(if brief {
        include_str!("../schemas/investigation-brief.schema.json")
    } else {
        include_str!("../schemas/investigation.schema.json")
    })
    .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<_> = validator
        .iter_errors(report)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

fn chain<'a>(findings: &'a Value, identity: &str) -> &'a Value {
    findings
        .as_array()
        .unwrap()
        .iter()
        .find(|f| {
            f["checked"]["identity"]
                .as_array()
                .is_some_and(|fields| fields.iter().any(|field| field["value"] == identity))
        })
        .unwrap()
}

fn step<'a>(finding: &'a Value, id: &str) -> &'a Value {
    finding["details"]["event_chain"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["id"] == id)
        .unwrap()
}

// @lat: [[regression-tests#Evidence regressions#Checked finding presentation]]
#[test]
fn maintained_example_agrees_in_text_json_and_bounded_summary() {
    let temp = tempfile::tempdir().unwrap();
    let report: Value = serde_json::from_str(&run(
        temp.path(),
        "full",
        PROFILE,
        &[INPUT],
        &["--json", "--complete-output"],
    ))
    .unwrap();
    shape(&report, false);
    log_analyzer::investigation::validate_relations(
        &report,
        Some(&fs::read(temp.path().join("full")).unwrap()),
    )
    .unwrap();
    let alpha = chain(&report["findings"], "alpha");
    for (id, count) in [("polls", 2), ("responses", 1), ("cache", 2)] {
        assert_eq!(step(alpha, id)["observed_count"], count);
        assert_eq!(step(alpha, id)["count_completeness"], "complete");
    }
    assert_eq!(step(alpha, "accepted")["event_type"], "acceptance");
    assert_eq!(step(alpha, "responses")["outcomes"][0]["value"], "Failure");
    assert_eq!(
        step(alpha, "cache")["outcomes"].as_array().unwrap().len(),
        2
    );
    assert_eq!(step(alpha, "check-end")["outcomes"][0]["value"], false);
    assert_eq!(
        step(alpha, "check-end")["outcomes"][0]["occurrence"]["evidence_ref"]["line"],
        9
    );
    assert_eq!(step(alpha, "session-end")["outcomes"][0]["value"], "Passed");
    assert_eq!(
        step(alpha, "session-end")["outcomes"][0]["occurrence"]["evidence_ref"]["line"],
        11
    );
    let interval = &step(alpha, "capture-end")["join_samples"][0]["endpoint_interval"];
    assert_eq!(interval["value"], 9000.0);
    assert_eq!(interval["unit"], "ms");
    // Changing both the retained result and displayed result cannot hide bad arithmetic.
    let mut bad_report = report.clone();
    let mut bad_artifact: Value =
        serde_json::from_slice(&fs::read(temp.path().join("full")).unwrap()).unwrap();
    let bad_step = "/findings/0/details/event_chain/steps/4/join_samples/0/endpoint_interval/value";
    *bad_report.pointer_mut(bad_step).unwrap() = json!(1.0);
    *bad_artifact.pointer_mut(bad_step).unwrap() = json!(1.0);
    let bad_bytes = serde_json::to_vec(&bad_artifact).unwrap();
    bad_report["artifact"]["stored_sha256"] = json!(log_analyzer::evidence::digest(&bad_bytes));
    assert!(
        log_analyzer::investigation::validate_relations(&bad_report, Some(&bad_bytes))
            .unwrap_err()
            .reason
            .contains("arithmetic")
    );
    assert_eq!(interval["end_timestamp"], "2026-01-01T02:00:09+02:00");
    assert_eq!(alpha["checked"]["assurance"]["level"], "link_exists");
    assert_eq!(
        alpha["checked"]["assurance"]["subject_verification"],
        "not_requested"
    );
    assert!(
        alpha["checked"]["population_rule"]
            .as_str()
            .unwrap()
            .contains("distinct source occurrences")
    );
    let beta = chain(&report["findings"], "beta");
    assert_eq!(step(beta, "check-end")["outcomes"][0]["value"], true);
    assert_eq!(step(beta, "capture-end")["status"], "not_observed");
    assert_eq!(step(beta, "capture-end")["join_samples"], json!([]));

    let text = run(temp.path(), "text", PROFILE, &[INPUT], &[]);
    for expected in [
        "polls sent: 2 observed",
        "responses observed: 1 observed",
        "cached failures reused: 2 observed",
        "Outcome [check]: payload.asExpected = false; Input 1, line 9",
        "Outcome [session]: payload.status = \"Passed\"; Input 1, line 11",
        "Interval checkpoint -> capture_end: 9000.0 ms",
        "capture completion not observed within this input; endpoint interval unavailable",
        "Assurance: link_exists",
        "intended subject not_requested",
        "subject_verified requires independent expected subjects",
    ] {
        assert!(text.contains(expected), "Missing {expected}: {text}");
    }
    assert!(text.find("Material limits").unwrap() < text.find("Observed findings").unwrap());
    assert!(!text.contains("Recognition:"));
    let verbose = run(temp.path(), "verbose", PROFILE, &[INPUT], &["--verbose"]);
    assert!(verbose.contains("Recognition:"));
    assert!(verbose.contains("Join:"));

    let wire = run(
        temp.path(),
        "summary",
        PROFILE,
        &[INPUT],
        &["--summary", "--report-max-items", "2"],
    );
    let brief: Value = serde_json::from_str(&wire).unwrap();
    shape(&brief, true);
    assert_eq!(brief["findings"]["displayed"], 2);
    assert!(brief["findings"]["omitted"].as_u64().unwrap() > 0);
    assert_eq!(brief["coverage"][0]["coverage"]["parsed_entries"], 15);
    assert_eq!(brief["presentation"]["serialized_bytes"], wire.len());
    for identity in ["alpha", "beta"] {
        assert_eq!(
            chain(&brief["findings"]["items"], identity)["details"],
            chain(&report["findings"], identity)["details"]
        );
    }
    assert_eq!(
        brief["presentation"]["uncertainty"],
        report["presentation"]["uncertainty"]
    );
    let bounded = run(
        temp.path(),
        "bounded",
        PROFILE,
        &[INPUT],
        &[
            "--summary",
            "--report-max-items",
            "0",
            "--report-max-bytes",
            "16000",
        ],
    );
    assert!(bounded.len() <= 16000);
    let bounded: Value = serde_json::from_str(&bounded).unwrap();
    shape(&bounded, true);
    assert_eq!(bounded["findings"]["displayed"], 0);
    assert_eq!(bounded["findings"]["omitted"], bounded["findings"]["total"]);
    assert_eq!(bounded["coverage"], brief["coverage"]);
    assert_eq!(
        bounded["presentation"]["uncertainty"],
        brief["presentation"]["uncertainty"]
    );
}

#[test]
fn checked_lifecycles_keep_failed_ends_missing_ends_and_ambiguous_starts_separate() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("events.jsonl");
    let rows: Vec<_> = [("a", "start", 0), ("a", "end", 3), ("b", "start", 4), ("c", "start", 5), ("c", "start", 6), ("c", "end", 7)].into_iter().map(|(id, phase, second)|
        json!({"ts":format!("2026-01-01T00:00:{second:02}+00:00"),"level":"INFO","message":format!("{id} {phase}"),"session":"run","operation":"render","id":id,"phase":phase,"outcome":"failure"}).to_string()).collect();
    fs::write(&input, rows.join("\n") + "\n").unwrap();
    let report: Value = serde_json::from_str(&run(
        temp.path(),
        "full",
        "examples/investigations/profile.toml",
        &[input.to_str().unwrap()],
        &["--json", "--complete-output"],
    ))
    .unwrap();
    shape(&report, false);
    let measurements: Vec<_> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "measurement")
        .collect();
    assert_eq!(measurements.len(), 1);
    assert_eq!(measurements[0]["details"]["value"], 3000);
    assert_eq!(
        measurements[0]["checked"]["subjects"][1]["event"]["outcome"],
        "failure"
    );
    assert_eq!(
        measurements[0]["checked"]["subjects"][1]["event"]["correlation_id"],
        "a"
    );
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["claim"].as_str().unwrap().contains("no observed end"))
    );
    let bytes = fs::read(temp.path().join("full")).unwrap();
    let index = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .position(|f| f["kind"] == "measurement")
        .unwrap();
    for path in [
        "/details/value",
        "/details/boundaries/end/occurrence/evidence_ref/line",
        "/checked/subjects/1/event/outcome",
    ] {
        let mut mutated = report.clone();
        *mutated["findings"][index].pointer_mut(path).unwrap() = json!(999);
        assert!(log_analyzer::investigation::validate_relations(&mutated, Some(&bytes)).is_err());
    }
}

#[test]
fn checked_findings_preserve_independent_inputs_and_renamed_ids() {
    let temp = tempfile::tempdir().unwrap();
    let renamed = temp.path().join("renamed.jsonl");
    fs::write(
        &renamed,
        fs::read_to_string(INPUT).unwrap().replace("alpha", "gamma"),
    )
    .unwrap();
    let report: Value = serde_json::from_str(&run(
        temp.path(),
        "both",
        PROFILE,
        &[INPUT, renamed.to_str().unwrap()],
        &["--json", "--complete-output"],
    ))
    .unwrap();
    for (identity, input) in [("alpha", 0), ("gamma", 1)] {
        let finding = chain(&report["findings"], identity);
        assert_eq!(
            finding["checked"]["scope"]["input_ordinals"],
            json!([input])
        );
        assert_eq!(
            finding["checked"]["subjects"][0]["occurrence"]["input_ordinal"],
            input
        );
        assert_eq!(
            step(finding, "capture-end")["join_samples"][0]["endpoint_interval"]["value"],
            9000.0
        );
        assert_eq!(step(finding, "polls")["observed_count"], 2);
    }
    // Identical beta IDs in independent inputs stay as separate findings.
    assert_eq!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["checked"]["identity"]
                .as_array()
                .is_some_and(|fields| fields.iter().any(|field| field["value"] == "beta")))
            .count(),
        2
    );
}

#[test]
fn bounded_and_redacted_output_keeps_limits_without_claiming_subject_verification() {
    let temp = tempfile::tempdir().unwrap();
    for (name, options) in [
        ("tiny", vec!["--summary", "--report-max-bytes", "1"]),
        ("redacted", vec!["--summary", "--redact"]),
    ] {
        let report: Value =
            serde_json::from_str(&run(temp.path(), name, PROFILE, &[INPUT], &options)).unwrap();
        shape(&report, true);
        assert_eq!(
            report["presentation"]["uncertainty"]["subject_verification"],
            "not_requested"
        );
        assert!(report["coverage"].is_array());
        if name == "tiny" {
            assert_eq!(
                report["presentation"]["status"],
                "mandatory_metadata_over_budget"
            );
            assert_eq!(report["findings"]["displayed"], 0);
            assert!(report["findings"]["omitted"].as_u64().unwrap() > 0);
        } else {
            for finding in report["findings"]["items"].as_array().unwrap() {
                assert_eq!(
                    finding["checked"]["assurance"]["source_binding"],
                    "unavailable"
                );
                assert_eq!(finding["checked"]["assurance"]["level"], "relations_only");
            }
            assert!(!report.to_string().contains("alpha"));
        }
    }
}

#[test]
fn checked_coverage_distinguishes_unparsed_partial_and_empty_selection() {
    let temp = tempfile::tempdir().unwrap();
    let bad = temp.path().join("unparsed.log");
    fs::write(&bad, "not a parsed event\n").unwrap();
    let unparsed: Value = serde_json::from_str(&run(
        temp.path(),
        "unparsed",
        PROFILE,
        &[bad.to_str().unwrap()],
        &["--summary"],
    ))
    .unwrap();
    assert_eq!(unparsed["coverage"][0]["coverage"]["parsed_entries"], 0);
    assert_eq!(unparsed["coverage"][0]["coverage"]["nonempty_lines"], 1);
    assert!(
        unparsed["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["status"] != "supported")
    );
    let partial: Value = serde_json::from_str(&run(
        temp.path(),
        "partial",
        PROFILE,
        &[INPUT],
        &["--json", "--processing-max-records", "4"],
    ))
    .unwrap();
    shape(&partial, false);
    assert_eq!(partial["processing"]["status"], "partial");
    assert!(
        partial["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["checked"]["scope"]["completeness"] != "complete")
    );
    let empty: Value = serde_json::from_str(&run(
        temp.path(),
        "empty",
        PROFILE,
        &[INPUT],
        &["--json", "--select", r#"{"name":"absent"}"#],
    ))
    .unwrap();
    shape(&empty, false);
    assert_eq!(
        empty["report_metadata"]["evidence"]["inputs"][0]["coverage"]["parsed_entries"],
        15
    );
    assert_eq!(
        empty["report_metadata"]["evidence"]["inputs"][0]["selected_entries"],
        0
    );
    let population = empty["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["details"]["population_id"] == "scope-0-physical-records")
        .unwrap();
    assert_eq!(population["details"]["value"], 0);
    assert_eq!(
        population["checked"]["population"]["completeness"],
        "complete"
    );
}
