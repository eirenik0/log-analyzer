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
            if name == "redacted" {
                "unavailable"
            } else {
                "not_requested"
            }
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
fn aggregate_subject_verification_preserves_unavailable_prerequisites_when_paged() {
    let temp = tempfile::tempdir().unwrap();
    let non_utf8 = temp.path().join("non-utf8.jsonl");
    let mut bytes = fs::read(INPUT).unwrap();
    bytes.extend_from_slice(b"\xff\n");
    fs::write(&non_utf8, bytes).unwrap();
    for (name, inputs, options, expected) in [
        ("available", vec![INPUT], vec![], "not_requested"),
        ("redacted", vec![INPUT], vec!["--redact"], "unavailable"),
        (
            "artifact-limit",
            vec![INPUT],
            vec!["--artifact-max-bytes", "1"],
            "unavailable",
        ),
        (
            "mixed",
            vec![INPUT, non_utf8.to_str().unwrap()],
            vec![],
            "unavailable",
        ),
    ] {
        let mut full_options = options.clone();
        full_options.extend(["--json", "--complete-output"]);
        let full: Value = serde_json::from_str(&run(
            temp.path(),
            &format!("{name}-full"),
            PROFILE,
            &inputs,
            &full_options,
        ))
        .unwrap();
        shape(&full, false);
        let uncertainty = &full["presentation"]["uncertainty"];
        assert_eq!(uncertainty["subject_verification"], expected, "{name}");
        let unavailable = full["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["checked"]["assurance"]["subject_verification"] == "unavailable")
            .count();
        assert_eq!(
            uncertainty["source_binding_unavailable_findings"],
            unavailable
        );
        assert_eq!(unavailable > 0, expected == "unavailable", "{name}");
        if name == "mixed" {
            assert!(unavailable < full["findings"].as_array().unwrap().len());
        }
        if name != "artifact-limit" {
            let mut page_options = options.clone();
            page_options.extend(["--json", "--report-max-items", "0"]);
            let page_name = format!("{name}-page");
            let page: Value = serde_json::from_str(&run(
                temp.path(),
                &page_name,
                PROFILE,
                &inputs,
                &page_options,
            ))
            .unwrap();
            assert!(page["findings"].as_array().unwrap().is_empty());
            let artifact = fs::read(temp.path().join(page_name)).unwrap();
            log_analyzer::investigation::validate_relations(&page, Some(&artifact)).unwrap();
            for field in [
                "unknown_findings",
                "incomplete_chains",
                "source_binding_unavailable_findings",
                "subject_verification",
            ] {
                let mut altered = page.clone();
                let value = &mut altered["presentation"]["uncertainty"][field];
                *value = if field == "subject_verification" {
                    json!(if expected == "unavailable" {
                        "not_requested"
                    } else {
                        "unavailable"
                    })
                } else {
                    json!(if value == 0 { 1 } else { 0 })
                };
                shape(&altered, false);
                let error =
                    log_analyzer::investigation::validate_relations(&altered, Some(&artifact))
                        .unwrap_err();
                assert!(
                    error.reason.contains("retained findings"),
                    "{name}/{field}: {error}"
                );
            }
            let mut legacy = page;
            legacy["presentation"]
                .as_object_mut()
                .unwrap()
                .remove("uncertainty");
            log_analyzer::investigation::validate_relations(&legacy, Some(&artifact)).unwrap();
        }
        let mut summary_options = options.clone();
        summary_options.extend(["--summary", "--report-max-items", "0"]);
        let summary: Value = serde_json::from_str(&run(
            temp.path(),
            &format!("{name}-summary"),
            PROFILE,
            &inputs,
            &summary_options,
        ))
        .unwrap();
        shape(&summary, true);
        assert_eq!(summary["findings"]["displayed"], 0);
        assert_eq!(summary["presentation"]["uncertainty"], *uncertainty);
        let text = run(
            temp.path(),
            &format!("{name}-text"),
            PROFILE,
            &inputs,
            &options,
        );
        assert!(
            text.contains(&format!("Intended subject verification: {expected}.")),
            "{name}: {text}"
        );
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

#[test]
fn checked_assurance_rejects_levels_and_bindings_that_disagree_with_evidence() {
    let temp = tempfile::tempdir().unwrap();
    for (name, options) in [
        ("original", vec!["--json", "--complete-output"]),
        ("redacted", vec!["--json", "--complete-output", "--redact"]),
        (
            "unavailable",
            vec!["--json", "--complete-output", "--artifact-max-bytes", "1"],
        ),
    ] {
        let report: Value =
            serde_json::from_str(&run(temp.path(), name, PROFILE, &[INPUT], &options)).unwrap();
        let bytes = fs::read(temp.path().join(name)).ok();
        log_analyzer::investigation::validate_relations(&report, bytes.as_deref()).unwrap();
        let mut levels = std::collections::BTreeSet::new();
        for (index, finding) in report["findings"].as_array().unwrap().iter().enumerate() {
            let level = finding["checked"]["assurance"]["level"].as_str().unwrap();
            if !levels.insert(level) {
                continue;
            }
            for altered_level in ["source_bound", "link_exists", "relations_only"] {
                if altered_level == level {
                    continue;
                }
                let mut altered = report.clone();
                altered["findings"][index]["checked"]["assurance"]["level"] = json!(altered_level);
                shape(&altered, false);
                for artifact in [None, bytes.as_deref()] {
                    let error = log_analyzer::investigation::validate_relations(&altered, artifact)
                        .unwrap_err();
                    assert!(
                        error.reason.contains("assurance level"),
                        "{name}/{level}/{altered_level}: {error}"
                    );
                }
            }
            if bytes.is_some() {
                let mut altered = report.clone();
                let assurance = &mut altered["findings"][index]["checked"]["assurance"];
                let validated = assurance["source_binding"] == "validated";
                assurance["source_binding"] = json!(if validated {
                    "unavailable"
                } else {
                    "validated"
                });
                assurance["subject_verification"] = json!(if validated {
                    "unavailable"
                } else {
                    "not_requested"
                });
                assurance["level"] = json!(if validated {
                    "relations_only"
                } else {
                    "link_exists"
                });
                shape(&altered, false);
                let error =
                    log_analyzer::investigation::validate_relations(&altered, bytes.as_deref())
                        .unwrap_err();
                assert!(
                    error.reason.contains("source binding"),
                    "{name}/{level}: {error}"
                );
            }
        }
        if name == "original" {
            assert_eq!(
                levels,
                ["link_exists", "source_bound"].into_iter().collect()
            );
        } else {
            assert_eq!(levels, ["relations_only"].into_iter().collect());
        }
    }
}

#[test]
fn unavailable_intervals_reject_changed_endpoint_metadata_in_report_and_artifact() {
    let temp = tempfile::tempdir().unwrap();
    for (name, expected_reason) in [
        ("ambiguous", "ambiguous_endpoint_population"),
        ("negative", "negative_or_out_of_range_interval"),
        ("missing-time", "explicit_source_timestamps_unavailable"),
    ] {
        let mut rows: Vec<Value> = fs::read_to_string(INPUT)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        if name == "ambiguous" {
            rows.push(rows[0].clone());
        } else if name == "negative" {
            rows[10]["ts"] = json!("2025-12-31T23:59:59+00:00");
        } else {
            rows[0]["ts"] = json!("2026-01-01T00:00:00");
            rows[10]["ts"] = json!("2026-01-01T00:00:10");
        }
        let input = temp.path().join(format!("{name}.jsonl"));
        fs::write(
            &input,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();
        let report: Value = serde_json::from_str(&run(
            temp.path(),
            name,
            PROFILE,
            &[input.to_str().unwrap()],
            &["--json", "--complete-output"],
        ))
        .unwrap();
        let artifact_bytes = fs::read(temp.path().join(name)).unwrap();
        let artifact: Value = serde_json::from_slice(&artifact_bytes).unwrap();
        log_analyzer::investigation::validate_relations(&report, Some(&artifact_bytes)).unwrap();
        let finding = chain(&report["findings"], "alpha");
        let finding_index = report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .position(|f| f["id"] == finding["id"])
            .unwrap();
        let artifact_index = artifact["findings"]
            .as_array()
            .unwrap()
            .iter()
            .position(|f| f["id"] == finding["id"])
            .unwrap();
        let step_index = finding["details"]["event_chain"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .position(|s| s["id"] == "session-end")
            .unwrap();
        let interval_path =
            format!("/details/event_chain/steps/{step_index}/join_samples/0/endpoint_interval");
        let interval = finding.pointer(&interval_path).unwrap();
        assert_eq!(interval["status"], "unavailable");
        assert_eq!(interval["reason"], expected_reason);
        assert!(interval["value"].is_null());
        for reason in [Value::Null, json!("arbitrary_reason")] {
            let mut altered = report.clone();
            let mut saved = artifact.clone();
            altered["findings"][finding_index]
                .pointer_mut(&interval_path)
                .unwrap()["reason"] = reason.clone();
            saved["findings"][artifact_index]
                .pointer_mut(&interval_path)
                .unwrap()["reason"] = reason;
            let bytes = serde_json::to_vec(&saved).unwrap();
            altered["artifact"]["stored_sha256"] = json!(log_analyzer::evidence::digest(&bytes));
            let error = log_analyzer::investigation::validate_relations(&altered, Some(&bytes))
                .unwrap_err();
            assert!(
                error.reason.contains("status or reason differs"),
                "{name}: {error}"
            );
        }
        for field in [
            "start_timestamp",
            "end_timestamp",
            "start_operation",
            "end_operation",
        ] {
            let mut altered = report.clone();
            let mut altered_artifact = artifact.clone();
            let path = format!("{interval_path}/{field}");
            let replacement = if field.ends_with("timestamp") {
                json!("2026-02-01T00:00:00+00:00")
            } else {
                json!("unrelated_operation")
            };
            *altered["findings"][finding_index]
                .pointer_mut(&path)
                .unwrap() = replacement.clone();
            *altered_artifact["findings"][artifact_index]
                .pointer_mut(&path)
                .unwrap() = replacement;
            let bytes = serde_json::to_vec(&altered_artifact).unwrap();
            altered["artifact"]["stored_sha256"] = json!(log_analyzer::evidence::digest(&bytes));
            shape(&altered, false);
            let error = log_analyzer::investigation::validate_relations(&altered, Some(&bytes))
                .unwrap_err();
            assert!(
                error.reason.contains("differs from source"),
                "{name}/{field}: {error}"
            );
        }
    }
}

#[test]
fn checked_subjects_require_exact_root_outcome_and_evidence_sets() {
    let temp = tempfile::tempdir().unwrap();
    for (name, options) in [
        ("original", vec!["--json", "--complete-output"]),
        ("redacted", vec!["--json", "--complete-output", "--redact"]),
    ] {
        let report: Value =
            serde_json::from_str(&run(temp.path(), name, PROFILE, &[INPUT], &options)).unwrap();
        let bytes = fs::read(temp.path().join(name)).unwrap();
        let artifact: Value = serde_json::from_slice(&bytes).unwrap();
        for is_chain in [true, false] {
            let (index, finding) = report["findings"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .find(|(_, f)| {
                    f["details"]["event_chain"].is_object() == is_chain
                        && f["checked"]["subjects"]
                            .as_array()
                            .is_some_and(|s| !s.is_empty())
                })
                .unwrap();
            let subjects = finding["checked"]["subjects"].as_array().unwrap();
            if is_chain {
                assert!(subjects.len() > 1);
                assert!(
                    subjects
                        .iter()
                        .any(|s| s["occurrence"] == finding["details"]["event_chain"]["root"])
                );
            }
            let mut mutations = vec![("empty", Vec::new())];
            // Removing each subject individually covers the root and every sampled outcome.
            for removed in 0..subjects.len() {
                let mut omitted = subjects.clone();
                omitted.remove(removed);
                mutations.push(("missing", omitted));
            }
            let mut duplicate = subjects.clone();
            duplicate.push(subjects[0].clone());
            mutations.push(("duplicate", duplicate));
            if is_chain {
                let witness = finding["evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|e| !subjects.iter().any(|s| s["occurrence"] == e["occurrence"]))
                    .unwrap();
                let record = artifact["records"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["occurrence"] == witness["occurrence"])
                    .unwrap();
                let replacement = json!({"occurrence": witness["occurrence"], "event":record["fields"]["classification"]["semantics"]});
                let mut substituted = subjects.clone();
                substituted[0] = replacement.clone();
                mutations.push(("substituted", substituted));
                let mut extra = subjects.clone();
                extra.push(replacement);
                mutations.push(("extra", extra));
            }
            for (mutation, subjects) in mutations {
                let mut altered = report.clone();
                altered["findings"][index]["checked"]["subjects"] = json!(subjects);
                shape(&altered, false);
                for artifact in [None, Some(bytes.as_slice())] {
                    let error = log_analyzer::investigation::validate_relations(&altered, artifact)
                        .unwrap_err();
                    assert!(
                        error.reason.contains("checked subject set"),
                        "{name}/{is_chain}/{mutation}: {error}"
                    );
                }
            }
            // The projection is a set: display order does not change its subjects.
            let mut reordered = report.clone();
            reordered["findings"][index]["checked"]["subjects"]
                .as_array_mut()
                .unwrap()
                .reverse();
            log_analyzer::investigation::validate_relations(&reordered, Some(&bytes)).unwrap();
        }
        let mut legacy = report;
        for finding in legacy["findings"].as_array_mut().unwrap() {
            finding.as_object_mut().unwrap().remove("checked");
        }
        log_analyzer::investigation::validate_relations(&legacy, Some(&bytes)).unwrap();
    }
}

#[test]
fn measured_intervals_cannot_be_hidden_by_unavailable_status_or_missing_timestamps() {
    let temp = tempfile::tempdir().unwrap();
    let report: Value = serde_json::from_str(&run(
        temp.path(),
        "measured",
        PROFILE,
        &[INPUT],
        &["--json", "--complete-output"],
    ))
    .unwrap();
    let artifact: Value =
        serde_json::from_slice(&fs::read(temp.path().join("measured")).unwrap()).unwrap();
    let finding = chain(&report["findings"], "alpha");
    let index = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .position(|f| f["id"] == finding["id"])
        .unwrap();
    let saved_index = artifact["findings"]
        .as_array()
        .unwrap()
        .iter()
        .position(|f| f["id"] == finding["id"])
        .unwrap();
    let step_index = finding["details"]["event_chain"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .position(|s| s["id"] == "capture-end")
        .unwrap();
    let path = format!("/details/event_chain/steps/{step_index}/join_samples/0/endpoint_interval");
    assert_eq!(finding.pointer(&path).unwrap()["status"], "measured");
    for (reason, hide_timestamps) in [
        ("arbitrary_reason", false),
        ("ambiguous_endpoint_population", false),
        ("negative_or_out_of_range_interval", false),
        ("explicit_source_timestamps_unavailable", false),
        ("explicit_source_timestamps_unavailable", true),
    ] {
        let mut altered = report.clone();
        let mut saved = artifact.clone();
        for interval in [
            altered["findings"][index].pointer_mut(&path).unwrap(),
            saved["findings"][saved_index].pointer_mut(&path).unwrap(),
        ] {
            interval["status"] = json!("unavailable");
            interval["value"] = Value::Null;
            interval["reason"] = json!(reason);
            if hide_timestamps {
                interval["start_timestamp"] = Value::Null;
                interval["end_timestamp"] = Value::Null;
            }
        }
        let bytes = serde_json::to_vec(&saved).unwrap();
        altered["artifact"]["stored_sha256"] = json!(log_analyzer::evidence::digest(&bytes));
        shape(&altered, false);
        let error =
            log_analyzer::investigation::validate_relations(&altered, Some(&bytes)).unwrap_err();
        assert!(
            error.reason.contains(if hide_timestamps {
                "timestamp differs from source"
            } else {
                "status or reason differs"
            }),
            "{reason}: {error}"
        );
    }
}

#[test]
fn retained_endpoint_names_are_checked_when_source_verification_is_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    fn withhold_source_verification(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                if fields.contains_key("source_and_rules") {
                    fields.insert(
                        "losses".into(),
                        json!(["Synthetic stored source verification unavailable"]),
                    );
                }
                for (key, value) in fields {
                    if key == "source_and_rules" {
                        *value = json!("unavailable");
                    } else {
                        withhold_source_verification(value);
                    }
                }
            }
            Value::Array(values) => values.iter_mut().for_each(withhold_source_verification),
            _ => (),
        }
    }
    for redacted in [false, true] {
        let name = if redacted { "redacted" } else { "retained" };
        let mut options = vec!["--json", "--complete-output"];
        if redacted {
            options.push("--redact");
        }
        let mut report: Value =
            serde_json::from_str(&run(temp.path(), name, PROFILE, &[INPUT], &options)).unwrap();
        let mut artifact: Value =
            serde_json::from_slice(&fs::read(temp.path().join(name)).unwrap()).unwrap();
        // A stored artifact can withhold source verification while retaining classifications.
        withhold_source_verification(&mut artifact);
        withhold_source_verification(&mut report);
        for finding in report["findings"].as_array_mut().unwrap() {
            finding["checked"]["assurance"]["source_binding"] = json!("unavailable");
            finding["checked"]["assurance"]["subject_verification"] = json!("unavailable");
            finding["checked"]["assurance"]["level"] = json!("relations_only");
        }
        report["presentation"]["uncertainty"]["source_binding_unavailable_findings"] =
            json!(report["findings"].as_array().unwrap().len());
        report["presentation"]["uncertainty"]["subject_verification"] = json!("unavailable");
        report["presentation"]["serialized_bytes"] = Value::Null;
        report["presentation"]["serialized_characters"] = Value::Null;
        let bytes = serde_json::to_vec(&artifact).unwrap();
        report["artifact"]["stored_sha256"] = json!(log_analyzer::evidence::digest(&bytes));
        shape(&report, false);
        log_analyzer::investigation::validate_relations(&report, Some(&bytes)).unwrap();
        let mut statuses = std::collections::BTreeSet::new();
        for (index, finding) in report["findings"].as_array().unwrap().iter().enumerate() {
            let Some(steps) = finding["details"]["event_chain"]["steps"].as_array() else {
                continue;
            };
            assert_eq!(finding["verification"]["source_and_rules"], "unavailable");
            let saved_index = artifact["findings"]
                .as_array()
                .unwrap()
                .iter()
                .position(|f| f["id"] == finding["id"])
                .unwrap();
            for (step_index, step) in steps.iter().enumerate() {
                for (join_index, join) in
                    step["join_samples"].as_array().unwrap().iter().enumerate()
                {
                    let status = join["endpoint_interval"]["status"].as_str().unwrap();
                    if !statuses.insert(status) {
                        continue;
                    }
                    let path = format!(
                        "/details/event_chain/steps/{step_index}/join_samples/{join_index}/endpoint_interval"
                    );
                    for (field, side) in
                        [("start_operation", "source"), ("end_operation", "target")]
                    {
                        let record = artifact["records"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|r| r["occurrence"] == join[side])
                            .unwrap();
                        assert_eq!(
                            record["fields"]["classification"]["semantics"].is_object(),
                            !redacted
                        );
                        if redacted {
                            continue;
                        }
                        for replacement in [json!("wrong_operation"), Value::Null] {
                            let mut altered = report.clone();
                            let mut saved = artifact.clone();
                            altered["findings"][index].pointer_mut(&path).unwrap()[field] =
                                replacement.clone();
                            saved["findings"][saved_index].pointer_mut(&path).unwrap()[field] =
                                replacement;
                            let bytes = serde_json::to_vec(&saved).unwrap();
                            altered["artifact"]["stored_sha256"] =
                                json!(log_analyzer::evidence::digest(&bytes));
                            shape(&altered, false);
                            let error = log_analyzer::investigation::validate_relations(
                                &altered,
                                Some(&bytes),
                            )
                            .unwrap_err();
                            assert!(
                                error.reason.contains("operation differs from source"),
                                "{status}/{field}: {error}"
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(statuses, ["measured", "unavailable"].into_iter().collect());
    }
}

#[test]
fn interval_cardinalities_are_checked_against_retained_chain_populations() {
    let temp = tempfile::tempdir().unwrap();
    for (name, step_id) in [
        ("root", "session-end"),
        ("source", "responses"),
        ("target", "polls"),
    ] {
        let input = temp.path().join(format!("{name}.jsonl"));
        let mut contents = fs::read_to_string(INPUT).unwrap();
        if name == "root" {
            contents.push_str(fs::read_to_string(INPUT).unwrap().lines().next().unwrap());
            contents.push('\n');
        }
        fs::write(&input, contents).unwrap();
        let profile = temp.path().join(format!("{name}.toml"));
        let mut policy = fs::read_to_string(PROFILE).unwrap();
        if name == "source" {
            policy = policy.replace(
                "join_fields = [\"capture_id\", \"poll_id\"]\ncardinality = \"one_to_one\"",
                "join_fields = [\"capture_id\"]\ncardinality = \"many_to_one\"",
            );
        }
        fs::write(&profile, policy).unwrap();
        let report: Value = serde_json::from_str(&run(
            temp.path(),
            name,
            profile.to_str().unwrap(),
            &[input.to_str().unwrap()],
            &["--json", "--complete-output"],
        ))
        .unwrap();
        let artifact_bytes = fs::read(temp.path().join(name)).unwrap();
        let artifact: Value = serde_json::from_slice(&artifact_bytes).unwrap();
        log_analyzer::investigation::validate_relations(&report, Some(&artifact_bytes)).unwrap();
        let finding = chain(&report["findings"], "alpha");
        let index = report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .position(|f| f["id"] == finding["id"])
            .unwrap();
        let saved_index = artifact["findings"]
            .as_array()
            .unwrap()
            .iter()
            .position(|f| f["id"] == finding["id"])
            .unwrap();
        let step_index = finding["details"]["event_chain"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .position(|s| s["id"] == step_id)
            .unwrap();
        let join_path = format!("/details/event_chain/steps/{step_index}/join_samples/0");
        let join = finding.pointer(&join_path).unwrap();
        assert_eq!(join["endpoint_interval"]["status"], "unavailable");
        assert_eq!(
            join["endpoint_interval"]["reason"],
            "ambiguous_endpoint_population"
        );
        let mut altered = report.clone();
        let mut saved = artifact.clone();
        for finding in [
            &mut altered["findings"][index],
            &mut saved["findings"][saved_index],
        ] {
            finding["details"]["event_chain"]["root_occurrences"] = json!(1);
            let join = finding.pointer_mut(&join_path).unwrap();
            join["source_count"] = json!(1);
            join["target_count"] = json!(1);
            let interval = &mut join["endpoint_interval"];
            let start =
                chrono::DateTime::parse_from_rfc3339(interval["start_timestamp"].as_str().unwrap())
                    .unwrap();
            let end =
                chrono::DateTime::parse_from_rfc3339(interval["end_timestamp"].as_str().unwrap())
                    .unwrap();
            interval["status"] = json!("measured");
            interval["value"] = json!(
                end.signed_duration_since(start).num_nanoseconds().unwrap() as f64 / 1_000_000.0
            );
            interval["reason"] = Value::Null;
        }
        let bytes = serde_json::to_vec(&saved).unwrap();
        altered["artifact"]["stored_sha256"] = json!(log_analyzer::evidence::digest(&bytes));
        shape(&altered, false);
        let error =
            log_analyzer::investigation::validate_relations(&altered, Some(&bytes)).unwrap_err();
        assert!(
            error
                .reason
                .contains("cardinality differs from retained population"),
            "{name}: {error}"
        );
    }
    let redacted: Value = serde_json::from_str(&run(
        temp.path(),
        "redacted",
        PROFILE,
        &[INPUT],
        &["--json", "--complete-output", "--redact"],
    ))
    .unwrap();
    let summary = log_analyzer::investigation::validate_relations(
        &redacted,
        Some(&fs::read(temp.path().join("redacted")).unwrap()),
    )
    .unwrap();
    assert!(
        summary
            .deferred_relations
            .iter()
            .any(|r| r == "chain_cardinalities_require_retained_profile_and_capture")
    );
}

#[test]
fn chain_validation_honors_raised_processing_limits_before_selection() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("expanded.toml");
    fs::write(
        &profile,
        format!(
            "{}\n[normalization]\nroot_path = \"/rows\"\nexpand_rows = true\n",
            fs::read_to_string(PROFILE).unwrap()
        ),
    )
    .unwrap();
    let rows: Vec<Value> = fs::read_to_string(INPUT)
        .unwrap()
        .lines()
        .map(|line| {
            let mut row: Value = serde_json::from_str(line).unwrap();
            row["timestamp"] = row["ts"].take();
            row
        })
        .collect();
    let count = 100_001 + rows.len();
    let input = temp.path().join("expanded.jsonl");
    let mut contents =
        "{\"rows\":[{\"timestamp\":\"2026-01-01T00:00:00Z\",\"message\":\"unselected\"}]}\n"
            .repeat(100_001);
    contents.push_str(&(json!({"rows":rows}).to_string() + "\n"));
    fs::write(&input, contents).unwrap();
    let report: Value = serde_json::from_str(&run(
        temp.path(),
        "retained",
        profile.to_str().unwrap(),
        &[input.to_str().unwrap()],
        &[
            "--json",
            "--complete-output",
            "--select",
            r#"{"correlation_id":"alpha"}"#,
            "--processing-max-expanded-records",
            "200000",
            "--processing-max-work",
            "1000000000",
            "--processing-max-ms",
            "240000",
            "--processing-max-memory-bytes",
            "8589934592",
        ],
    ))
    .unwrap();
    assert_eq!(
        report["processing"]["status"], "complete",
        "{}",
        report["processing"]
    );
    assert_eq!(report["processing"]["usage"]["expanded_records"], count);
    let bytes = fs::read(temp.path().join("retained")).unwrap();
    let artifact: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(artifact["records"].as_array().unwrap().len(), 10);
    let summary = log_analyzer::investigation::validate_relations(&report, Some(&bytes)).unwrap();
    assert!(
        !summary
            .deferred_relations
            .iter()
            .any(|r| r == "chain_cardinalities_require_retained_profile_and_capture")
    );
    let finding = chain(&report["findings"], "alpha");
    assert_eq!(
        step(finding, "capture-end")["join_samples"][0]["endpoint_interval"]["status"],
        "measured"
    );
}

#[test]
fn replay_rejects_invented_completion_and_incomplete_step_results() {
    let temp = tempfile::tempdir().unwrap();
    let report: Value = serde_json::from_str(&run(
        temp.path(),
        "steps",
        PROFILE,
        &[INPUT],
        &["--json", "--complete-output"],
    ))
    .unwrap();
    let bytes = fs::read(temp.path().join("steps")).unwrap();
    let artifact: Value = serde_json::from_slice(&bytes).unwrap();
    log_analyzer::investigation::validate_relations(&report, Some(&bytes)).unwrap();
    for (case, identity, step_id, reason) in [
        ("completion", "beta", "capture-end", "chain step status"),
        ("join", "alpha", "submitted", "chain join samples"),
        ("join-identity", "alpha", "submitted", "chain join identity"),
        ("step", "beta", "cache", "chain steps"),
        ("count", "alpha", "polls", "chain step observed_count"),
        ("ambiguity", "beta", "capture-end", "chain step ambiguity"),
        (
            "completeness",
            "alpha",
            "submitted",
            "chain step completeness",
        ),
    ] {
        let id = chain(&report["findings"], identity)["id"].clone();
        let mut altered = report.clone();
        let mut saved = artifact.clone();
        for document in [&mut altered, &mut saved] {
            let finding = document["findings"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|finding| finding["id"] == id)
                .unwrap();
            let excerpt = finding["evidence"][0].clone();
            let steps = finding["details"]["event_chain"]["steps"]
                .as_array_mut()
                .unwrap();
            if case == "step" {
                steps.retain(|step| step["id"] != step_id);
                continue;
            }
            let step = steps.iter_mut().find(|step| step["id"] == step_id).unwrap();
            match case {
                "completion" => {
                    assert_eq!(step["status"], "not_observed");
                    step["observed_count"] = json!(1);
                    step["linked_count"] = json!(1);
                    step["status"] = json!("observed");
                    step["evidence"] = json!([excerpt]);
                    assert_eq!(step["join_samples"], json!([]));
                }
                "join" => step["join_samples"] = json!([]),
                "join-identity" => {
                    step["join_samples"][0]["identity"][0]["value"] = json!("neighbor");
                }
                "count" => {
                    step["observed_count"] = json!(1);
                    step["linked_count"] = json!(1);
                    step["evidence"].as_array_mut().unwrap().truncate(1);
                    step["join_samples"].as_array_mut().unwrap().truncate(1);
                }
                "ambiguity" => step["ambiguity"]["source_occurrences_missing_identity"] = json!(1),
                "completeness" => {
                    assert_eq!(step["count_completeness"], "complete");
                    step["count_completeness"] = json!("partial");
                }
                _ => unreachable!(),
            }
        }
        let bytes = serde_json::to_vec(&saved).unwrap();
        altered["artifact"]["stored_sha256"] = json!(log_analyzer::evidence::digest(&bytes));
        // These mutually consistent edits pass report-only relations, but not source replay.
        altered["presentation"]["serialized_bytes"] = Value::Null;
        altered["presentation"]["serialized_characters"] = Value::Null;
        shape(&altered, false);
        log_analyzer::investigation::validate_relations(&altered, None).unwrap();
        let error =
            log_analyzer::investigation::validate_relations(&altered, Some(&bytes)).unwrap_err();
        assert!(error.reason.contains(reason), "{case}: {error}");
    }
    let partial: Value = serde_json::from_str(&run(
        temp.path(),
        "partial-steps",
        PROFILE,
        &[INPUT],
        &[
            "--json",
            "--complete-output",
            "--processing-max-records",
            "4",
        ],
    ))
    .unwrap();
    let summary = log_analyzer::investigation::validate_relations(
        &partial,
        Some(&fs::read(temp.path().join("partial-steps")).unwrap()),
    )
    .unwrap();
    assert!(
        summary
            .deferred_relations
            .iter()
            .any(|r| r == "chain_completeness_requires_processing_stage_history")
    );
}

#[test]
fn replay_binds_chain_identity_and_context_to_retained_subjects() {
    let temp = tempfile::tempdir().unwrap();
    let report: Value = serde_json::from_str(&run(
        temp.path(),
        "identity",
        PROFILE,
        &[INPUT],
        &["--json", "--complete-output"],
    ))
    .unwrap();
    let bytes = fs::read(temp.path().join("identity")).unwrap();
    let artifact: Value = serde_json::from_slice(&bytes).unwrap();
    log_analyzer::investigation::validate_relations(&report, Some(&bytes)).unwrap();
    let id = chain(&report["findings"], "alpha")["id"].clone();
    let mut earlier_samples =
        chain(&report["findings"], "alpha")["details"]["event_chain"]["later_activity"].clone();
    earlier_samples[0] = chain(&report["findings"], "alpha")["evidence"][0].clone();
    for (field, replacement) in [
        (
            "identity",
            json!([{"field":"capture_id","value":"neighbor"},{"field":"session_id","value":"demo"}]),
        ),
        (
            "context_identity",
            json!([{"field":"session_id","value":"neighbor"}]),
        ),
        ("scope_fields", json!(["capture_id"])),
        ("root_event_name", json!("neighbor")),
        ("ambiguity", json!("reused_root_identity")),
        ("later_activity_observed_count", json!(0)),
        ("later_activity", earlier_samples),
    ] {
        let mut altered = report.clone();
        let mut saved = artifact.clone();
        for document in [&mut altered, &mut saved] {
            let finding = document["findings"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|finding| finding["id"] == id)
                .unwrap();
            finding["details"]["event_chain"][field] = replacement.clone();
            if field == "identity" && finding["checked"].is_object() {
                finding["checked"]["identity"] = replacement.clone();
            }
            let event_chain = &mut finding["details"]["event_chain"];
            if field == "later_activity_observed_count" {
                event_chain["later_activity"] = json!([]);
                event_chain["omitted_later_activity"] = json!(0);
            } else if field == "later_activity" {
                event_chain["omitted_later_activity"] = json!(
                    event_chain["later_activity_observed_count"]
                        .as_u64()
                        .unwrap()
                        - replacement.as_array().unwrap().len() as u64
                );
            }
        }
        let bytes = serde_json::to_vec(&saved).unwrap();
        altered["artifact"]["stored_sha256"] = json!(log_analyzer::evidence::digest(&bytes));
        altered["presentation"]["serialized_bytes"] = Value::Null;
        altered["presentation"]["serialized_characters"] = Value::Null;
        shape(&altered, false);
        log_analyzer::investigation::validate_relations(&altered, None).unwrap();
        let error =
            log_analyzer::investigation::validate_relations(&altered, Some(&bytes)).unwrap_err();
        assert!(
            error
                .reason
                .contains(&format!("chain {field} differs from retained population")),
            "{error}"
        );
    }
}

#[test]
fn replay_requires_all_chain_findings_for_completed_inputs() {
    let temp = tempfile::tempdir().unwrap();
    let report: Value = serde_json::from_str(&run(
        temp.path(),
        "all-chains",
        PROFILE,
        &[INPUT],
        &["--json", "--complete-output"],
    ))
    .unwrap();
    let artifact: Value =
        serde_json::from_slice(&fs::read(temp.path().join("all-chains")).unwrap()).unwrap();
    let beta_id = chain(&report["findings"], "beta")["id"].clone();
    let beta_root = chain(&report["findings"], "beta")["details"]["event_chain"]["root"].clone();
    for (all, omit_record) in [(false, false), (true, false), (false, true)] {
        let mut altered = report.clone();
        let mut saved = artifact.clone();
        for document in [&mut altered, &mut saved] {
            document["findings"]
                .as_array_mut()
                .unwrap()
                .retain(|finding| {
                    if all {
                        !finding["details"]["event_chain"].is_object()
                    } else {
                        finding["id"] != beta_id
                    }
                });
            let ids: Vec<_> = document["findings"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["id"].clone())
                .collect();
            for assessment in document["assessments"].as_array_mut().unwrap() {
                assessment["finding_ids"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|id| ids.contains(id));
                if assessment["finding_ids"].as_array().unwrap().is_empty()
                    && assessment["status"] == "supported"
                {
                    assessment["status"] = json!("insufficient_evidence");
                }
            }
        }
        let findings = altered["findings"].as_array().unwrap();
        let ids: Vec<_> = findings.iter().map(|f| f["id"].clone()).collect();
        let count = findings.len();
        let incomplete = findings
            .iter()
            .filter(|f| {
                f["details"]["event_chain"].is_object()
                    && (f["details"]["event_chain"]["completeness"] != "complete"
                        || f["details"]["event_chain"]["steps"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|s| s["status"] != "observed"))
            })
            .count();
        altered["presentation"]["uncertainty"]["incomplete_chains"] = json!(incomplete);
        altered["presentation"]["total_findings"] = json!(count);
        altered["presentation"]["displayed_findings"] = json!(count);
        altered["presentation"]["collections"][0]["total"] = json!(count);
        altered["presentation"]["collections"][0]["displayed"] = json!(count);
        let targets = altered["retrieval"]["targets"].as_array_mut().unwrap();
        targets.retain(|target| target["kind"] != "finding" || ids.contains(&target["id"]));
        for target in targets.iter_mut().filter(|t| t["kind"] == "finding") {
            target["collection"] = json!(format!(
                "/findings/{}",
                ids.iter().position(|id| *id == target["id"]).unwrap()
            ));
        }
        if omit_record {
            saved["records"]
                .as_array_mut()
                .unwrap()
                .retain(|record| record["occurrence"] != beta_root);
            for membership in saved["memberships"].as_array_mut().unwrap() {
                membership["members"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|member| member["occurrence"] != beta_root);
            }
            for sequence in saved["sequences"].as_array_mut().unwrap() {
                sequence["events"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|event| event["occurrence"] != beta_root);
            }
            let memberships = saved["memberships"].clone();
            for document in [&mut altered, &mut saved] {
                for population in document["populations"].as_array_mut().unwrap() {
                    let membership = memberships
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|m| m["population_id"] == population["id"])
                        .unwrap();
                    let members = &membership["members"];
                    population["count"] = json!(members.as_array().unwrap().len());
                    population["membership"]["count"] = population["count"].clone();
                    population["membership"]["sha256"] = json!(log_analyzer::evidence::digest(
                        &serde_json::to_vec(members).unwrap()
                    ));
                }
                let populations = document["populations"].clone();
                for finding in document["findings"].as_array_mut().unwrap() {
                    if finding["details"]["calculation"] == "count" {
                        let population = populations
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|p| p["id"] == finding["details"]["population_id"])
                            .unwrap();
                        finding["details"]["value"] = population["count"].clone();
                        if finding["checked"].is_object() {
                            finding["checked"]["population"] = population.clone();
                        }
                    }
                }
            }
        }
        let bytes = serde_json::to_vec(&saved).unwrap();
        altered["artifact"]["stored_sha256"] = json!(log_analyzer::evidence::digest(&bytes));
        altered["presentation"]["serialized_bytes"] = Value::Null;
        altered["presentation"]["serialized_characters"] = Value::Null;
        shape(&altered, false);
        log_analyzer::investigation::validate_relations(&altered, None).unwrap();
        let error =
            log_analyzer::investigation::validate_relations(&altered, Some(&bytes)).unwrap_err();
        assert!(
            error.reason.contains(if omit_record {
                "retained records differ from selected captured population"
            } else {
                "retained chain findings differ from replayed population"
            }),
            "{error}"
        );
    }
}

#[test]
fn independent_cutoffs_do_not_defer_unaffected_chain_completeness() {
    let temp = tempfile::tempdir().unwrap();
    let ambiguous = temp.path().join("ambiguous.jsonl");
    let input = fs::read_to_string(INPUT).unwrap();
    fs::write(
        &ambiguous,
        format!("{input}{}\n", input.lines().next().unwrap()),
    )
    .unwrap();
    let invalid = temp.path().join("invalid.jsonl");
    fs::write(&invalid, [0xff, b'\n']).unwrap();
    for (name, inputs, extra) in [
        (
            "cutoff-after",
            vec![ambiguous.to_str().unwrap(), invalid.to_str().unwrap()],
            vec![],
        ),
        (
            "record-cutoff-after",
            vec![ambiguous.to_str().unwrap(), INPUT],
            vec!["--processing-max-records", "20"],
        ),
    ] {
        let mut options = vec!["--json", "--complete-output"];
        options.extend(extra);
        let report: Value =
            serde_json::from_str(&run(temp.path(), name, PROFILE, &inputs, &options)).unwrap();
        let bytes = fs::read(temp.path().join(name)).unwrap();
        let artifact: Value = serde_json::from_slice(&bytes).unwrap();
        log_analyzer::investigation::validate_relations(&report, Some(&bytes)).unwrap();
        assert!(
            !report["processing"]["stop"]["scope_ids"]
                .as_array()
                .unwrap()
                .contains(&json!("scope-0")),
            "{name}: {}",
            report["processing"]
        );
        let id = chain(&report["findings"], "alpha")["id"].clone();
        for step_only in [false, true] {
            let mut altered = report.clone();
            let mut saved = artifact.clone();
            for document in [&mut altered, &mut saved] {
                let finding = document["findings"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|f| f["id"] == id)
                    .unwrap();
                let chain = &mut finding["details"]["event_chain"];
                if step_only {
                    assert_eq!(chain["steps"][0]["count_completeness"], "partial");
                    chain["steps"][0]["count_completeness"] = json!("complete");
                } else {
                    assert_eq!(chain["completeness"], "partial");
                    chain["completeness"] = json!("complete");
                }
            }
            let bytes = serde_json::to_vec(&saved).unwrap();
            altered["artifact"]["stored_sha256"] = json!(log_analyzer::evidence::digest(&bytes));
            altered["presentation"]["serialized_bytes"] = Value::Null;
            altered["presentation"]["serialized_characters"] = Value::Null;
            shape(&altered, false);
            log_analyzer::investigation::validate_relations(&altered, None).unwrap();
            let error = log_analyzer::investigation::validate_relations(&altered, Some(&bytes))
                .unwrap_err();
            assert!(
                error
                    .reason
                    .contains("completeness differs from retained population"),
                "{error}"
            );
        }
    }
}

#[test]
fn replay_checks_classifications_against_captured_source_and_rules() {
    let temp = tempfile::tempdir().unwrap();
    let report: Value = serde_json::from_str(&run(
        temp.path(),
        "classification",
        PROFILE,
        &[INPUT],
        &["--json", "--complete-output"],
    ))
    .unwrap();
    let bytes = fs::read(temp.path().join("classification")).unwrap();
    let artifact: Value = serde_json::from_slice(&bytes).unwrap();
    let root = chain(&report["findings"], "alpha")["details"]["event_chain"]["root"].clone();
    for rule_ids in [false, true] {
        let mut altered = report.clone();
        let mut saved = artifact.clone();
        let record = saved["records"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["occurrence"] == root)
            .unwrap();
        if rule_ids {
            record["fields"]["classification"]["rule_ids"]
                .as_array_mut()
                .unwrap()
                .push(json!("synthetic-extra-rule"));
        } else {
            record["fields"]["classification"]["semantics"]["correlation_id"] = json!("neighbor");
            for finding in altered["findings"].as_array_mut().unwrap() {
                for subject in finding["checked"]["subjects"].as_array_mut().unwrap() {
                    if subject["occurrence"] == root {
                        subject["event"]["correlation_id"] = json!("neighbor");
                    }
                }
            }
        }
        let bytes = serde_json::to_vec(&saved).unwrap();
        altered["artifact"]["stored_sha256"] = json!(log_analyzer::evidence::digest(&bytes));
        altered["presentation"]["serialized_bytes"] = Value::Null;
        altered["presentation"]["serialized_characters"] = Value::Null;
        shape(&altered, false);
        log_analyzer::investigation::validate_relations(&altered, None).unwrap();
        let error =
            log_analyzer::investigation::validate_relations(&altered, Some(&bytes)).unwrap_err();
        assert!(
            error
                .reason
                .contains("retained classification differs from captured source and profile"),
            "{error}"
        );
    }
}
