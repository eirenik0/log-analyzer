use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tempfile::tempdir;

fn invoke(args: &[&str]) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
    for (name, _) in std::env::vars().filter(|(name, _)| name.starts_with("LOG_ANALYZER_")) {
        command.env_remove(name);
    }
    let output = command.args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn row(second: u8, phase: &str, outcome: Option<&str>, scope: &str) -> Value {
    let mut row = json!({"ts":format!("2026-07-01T00:00:{second:02}+02:00"),
        "level":"INFO", "message":"synthetic event", "component":"worker",
        "component_id":scope, "session":scope, "operation":"lookup", "id":"request-7"});
    if !phase.is_empty() {
        row["phase"] = json!(phase);
    }
    if let Some(outcome) = outcome {
        row["outcome"] = json!(outcome);
    }
    row
}

fn inspect(
    rows: &[Value],
    record_limit: &str,
    base: bool,
    rejected_tail: bool,
) -> (Value, Vec<Value>, usize) {
    let directory = tempdir().unwrap();
    let input = directory.path().join("capture.jsonl");
    let artifact = directory.path().join("artifact.json");
    let mut text = rows
        .iter()
        .map(|row| format!("{row}\n"))
        .collect::<String>();
    if rejected_tail {
        text.push_str("{invalid terminal record\n");
    }
    fs::write(&input, text).unwrap();
    let profile =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/investigations/profile.toml");
    let report = invoke(&[
        if base { "--preset" } else { "--config" },
        if base {
            "base"
        } else {
            profile.to_str().unwrap()
        },
        "investigate",
        input.to_str().unwrap(),
        "--artifact",
        artifact.to_str().unwrap(),
        "--report-max-items",
        "1",
        "--processing-max-records",
        record_limit,
    ]);
    assert_eq!(
        report["report_metadata"]["evidence"]["scope"]["capture_completeness"],
        "unknown"
    );
    let checksum = report["artifact"]["stored_sha256"].as_str().unwrap();
    let mut records = Vec::new();
    let mut cursor: Option<String> = None;
    for pages in 1..=16 {
        let mut args = vec![
            "investigation-evidence",
            artifact.to_str().unwrap(),
            "--expected-sha256",
            checksum,
            "--collection",
            "/records",
            "--report-max-items",
            "2",
        ];
        if let Some(cursor) = &cursor {
            args.extend(["--report-cursor", cursor]);
        }
        let value = invoke(&args);
        let page = &value["artifact_retrieval"];
        assert_eq!(page["artifact_sha256"], checksum);
        assert_eq!(page["prior"], records.len());
        assert_eq!(page["parse_passes"], 0);
        assert_eq!(page["correlation_passes"], 0);
        let items = page["items"].as_array().unwrap();
        records.extend(items.iter().cloned());
        if page["next_cursor"].is_null() {
            assert_eq!(page["remaining"], 0);
            assert_eq!(page["total"], records.len());
            return (report, records, pages);
        }
        assert!(!items.is_empty());
        cursor = Some(page["next_cursor"].as_str().unwrap().to_owned());
    }
    panic!("evidence pagination exceeded its budget");
}

fn count(report: &Value, population: &str) -> Option<u64> {
    report["populations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == format!("scope-0-{population}"))
        .and_then(|p| p["count"].as_u64())
}

fn assessment<'a>(report: &'a Value, goal: &str) -> &'a Value {
    &report["assessments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["goal"] == goal)
        .unwrap()["status"]
}

fn late_end() -> Vec<Value> {
    let mut rows = vec![row(0, "start", None, "run-a")];
    rows.extend((1..7).map(|i| row(i, "", None, "run-a")));
    rows[1]["level"] = json!("ERROR");
    rows.push(row(7, "end", Some("success"), "run-a"));
    rows
}

#[test]
fn terminal_success_survives_presentation_omissions_and_intermediate_errors() {
    let (report, records, pages) = inspect(&late_end(), "100", false, false);
    assert_eq!(report["processing"]["status"], "complete");
    assert!(report["presentation"]["omitted_findings"].as_u64().unwrap() > 0);
    assert_eq!((records.len(), pages), (8, 4));
    assert_eq!(count(&report, "paired-lifecycles"), Some(1));
    assert_eq!(count(&report, "successes"), Some(1));
    assert_eq!(records[7]["occurrence"]["evidence_ref"]["line"], 8);
    assert_eq!(
        records[7]["fields"]["classification"]["semantics"]["outcome"],
        "success"
    );
}

#[test]
fn complete_capture_bytes_and_processing_do_not_imply_complete_evidence() {
    let (report, records, _) = inspect(&late_end(), "3", false, false);
    assert_eq!(report["processing"]["inputs"][0]["capture"], "complete");
    assert_eq!(report["processing"]["status"], "partial");
    assert_eq!(report["processing"]["stop"]["reason"], "record_limit");
    assert_eq!(records.len(), 3);
    assert_eq!(
        assessment(&report, "incomplete_lifecycles"),
        "insufficient_evidence"
    );
    let (report, _, _) = inspect(&[row(0, "start", None, "run-a")], "100", false, true);
    assert_eq!(report["processing"]["status"], "complete");
    assert_eq!(
        report["report_metadata"]["evidence"]["inputs"][0]["coverage"]["rejected_candidates"],
        1
    );
    assert_eq!(
        assessment(&report, "incomplete_lifecycles"),
        "insufficient_evidence"
    );
}

#[test]
fn failure_is_a_terminal_outcome_and_later_success_preserves_both() {
    let mut rows = vec![
        row(0, "start", None, "run-a"),
        row(1, "end", Some("failure"), "run-a"),
    ];
    let (report, _, _) = inspect(&rows, "100", false, false);
    assert_eq!(count(&report, "paired-lifecycles"), Some(1));
    assert_eq!(count(&report, "failures"), Some(1));
    rows.extend([
        row(2, "start", None, "run-a"),
        row(3, "end", Some("success"), "run-a"),
    ]);
    let (report, records, _) = inspect(&rows, "100", false, false);
    assert_eq!(count(&report, "paired-lifecycles"), Some(2));
    assert_eq!(count(&report, "failures"), Some(1));
    assert_eq!(count(&report, "successes"), Some(1));
    assert_eq!(
        records[1]["fields"]["classification"]["semantics"]["outcome"],
        "failure"
    );
    assert_eq!(
        records[3]["fields"]["classification"]["semantics"]["outcome"],
        "success"
    );
}

#[test]
fn unmapped_outcome_is_available_even_when_no_end_is_recognized() {
    let rows = [
        row(0, "start", None, "run-a"),
        row(1, "result", Some("success"), "run-a"),
    ];
    let (report, records, _) = inspect(&rows, "100", false, false);
    assert_eq!(count(&report, "ends"), Some(0));
    assert_eq!(
        records[1]["fields"]["classification"]["status"],
        "unclassified"
    );
    assert_eq!(
        records[1]["fields"]["structured_fields"]["outcome"],
        "success"
    );
    assert_eq!(records[1]["occurrence"]["evidence_ref"]["line"], 2);
    let (report, _, _) = inspect(&late_end(), "100", true, false);
    assert_eq!(count(&report, "ends"), None);
    assert_eq!(
        assessment(&report, "incomplete_lifecycles"),
        "insufficient_evidence"
    );
}

#[test]
fn boundary_evidence_remains_scoped_and_does_not_invent_a_full_lifecycle() {
    for rows in [
        vec![row(0, "start", None, "run-a")],
        vec![row(1, "end", Some("success"), "run-a")],
        vec![
            row(0, "start", None, "run-a"),
            row(1, "end", Some("success"), "run-b"),
        ],
    ] {
        let (report, records, _) = inspect(&rows, "100", false, false);
        assert_eq!(count(&report, "paired-lifecycles"), Some(0));
        assert_eq!(
            assessment(&report, "slow_operations"),
            "insufficient_evidence"
        );
        assert_eq!(records.len(), rows.len());
        let end = rows.iter().any(|r| r["phase"] == "end");
        assert_eq!(count(&report, "successes"), Some(u64::from(end)));
        for (record, source) in records.iter().zip(&rows) {
            assert_eq!(
                record["fields"]["classification"]["semantics"]["scope"],
                json!([source["session"]])
            );
        }
    }
}
