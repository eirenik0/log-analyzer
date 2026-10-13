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
