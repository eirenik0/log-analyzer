use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::tempdir;

fn invoke(args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
    for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("LOG_ANALYZER_")) {
        command.env_remove(key);
    }
    command.args(args).output().unwrap()
}
fn validate(value: &Value) {
    static VALIDATOR: std::sync::LazyLock<jsonschema::Validator> = std::sync::LazyLock::new(|| {
        let schema: Value =
            serde_json::from_str(include_str!("../schemas/report.schema.json")).unwrap();
        jsonschema::validator_for(&schema).unwrap()
    });
    let validator = &*VALIDATOR;
    let errors: Vec<_> = validator
        .iter_errors(value)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}\n{value}");
}
fn run(args: &[&str]) -> Value {
    let result = invoke(args);
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value = serde_json::from_slice(&result.stdout).unwrap();
    validate(&value);
    value
}
fn inputs(dir: &Path) -> (String, String) {
    let mut a = String::new();
    let mut b = String::new();
    for index in 0..8 {
        for (output, change) in [(&mut a, 0), (&mut b, 1)] {
            output.push_str(&format!("core (scope/{index}) | 2026-01-01T00:00:00+02:00 [INFO] Request \"work\" [0--id{index}] will be sent\n"));
            output.push_str(&format!("core (scope/{index}) | 2026-01-01T00:00:01+02:00 [INFO] Request \"work\" [0--id{index}] finished successfully\n"));
            output.push_str(&format!("worker (scope/{index}) | 2026-01-01T00:00:02+02:00 [ERROR] failed {index} with payload {{\"value\":{},\"items\":[{{\"x\":1}},{{\"x\":2}}],\"password\":\"private-secret\"}}\n",index+change));
        }
    }
    let a_path = dir.join("a.log");
    let b_path = dir.join("b.log");
    fs::write(&a_path, a).unwrap();
    fs::write(&b_path, b).unwrap();
    (
        a_path.to_str().unwrap().into(),
        b_path.to_str().unwrap().into(),
    )
}
fn join_collection(target: &mut Value, page: &Value, path: &str) {
    let from = page.pointer(path).unwrap();
    let to = target.pointer_mut(path).unwrap();
    match (to, from) {
        (Value::Array(to), Value::Array(from)) => to.extend(from.clone()),
        (Value::Object(to), Value::Object(from)) => {
            for (key, value) in from {
                assert!(to.insert(key.clone(), value.clone()).is_none());
            }
        }
        (Value::String(to), Value::String(from)) => to.push_str(from),
        _ => panic!("collection shape changed: {path}"),
    }
}
fn reconstruct(base: &[&str], page_items: &str) -> (Value, Value, usize) {
    let mut args = base.to_vec();
    args.push("--complete-output");
    let complete = run(&args);
    let mut reconstructed = complete.clone();
    for collection in complete["retrieval"]["collections"].as_array().unwrap() {
        let target = reconstructed
            .pointer_mut(collection["path"].as_str().unwrap())
            .unwrap();
        match target {
            Value::Array(a) => a.clear(),
            Value::Object(m) => m.clear(),
            Value::String(s) => s.clear(),
            _ => unreachable!(),
        }
    }
    let mut cursor: Option<String> = None;
    let mut count = 0;
    let mut prior = 0;
    loop {
        let mut args = base.to_vec();
        args.extend(["--report-max-items", page_items]);
        if let Some(cursor) = &cursor {
            args.extend(["--report-cursor", cursor.as_str()]);
        }
        let page = run(&args);
        count += 1;
        assert!(count < 200, "pagination did not terminate");
        assert_eq!(page["retrieval"]["prior_items"], prior);
        assert_eq!(
            page["report_metadata"]["evidence"]["scope"],
            complete["report_metadata"]["evidence"]["scope"]
        );
        for key in ["totals", "operation_coverage", "summary"] {
            assert_eq!(page.get(key), complete.get(key));
        }
        if let Some(coverage) = page.get("coverage") {
            let mut page_counts = coverage.clone();
            let mut full_counts = complete["coverage"].clone();
            for coverage in [&mut page_counts, &mut full_counts] {
                for file in coverage["files"].as_array_mut().unwrap() {
                    file["normalization_diagnostics"] = json!([]);
                }
            }
            assert_eq!(page_counts, full_counts);
        }
        for collection in page["retrieval"]["collections"].as_array().unwrap() {
            assert_eq!(
                collection["total"].as_u64().unwrap(),
                collection["prior"].as_u64().unwrap()
                    + collection["displayed"].as_u64().unwrap()
                    + collection["remaining"].as_u64().unwrap()
            );
            join_collection(
                &mut reconstructed,
                &page,
                collection["path"].as_str().unwrap(),
            );
        }
        prior += page["retrieval"]["displayed_items"].as_u64().unwrap();
        cursor = page["retrieval"]["next_cursor"]
            .as_str()
            .map(str::to_string);
        if cursor.is_none() {
            assert_eq!(page["retrieval"]["status"], "complete");
            break;
        }
    }
    for collection in complete["retrieval"]["collections"].as_array().unwrap() {
        let path = collection["path"].as_str().unwrap();
        assert_eq!(
            reconstructed.pointer(path),
            complete.pointer(path),
            "{path}"
        );
    }
    (complete, reconstructed, count)
}

#[test]
fn pages_reconstruct_all_supported_reports_and_unique_source_records() {
    let dir = tempdir().unwrap();
    let (a, b) = inputs(dir.path());
    let variants = vec![
        vec!["info", &a],
        vec!["search", &a, "--context", "1", "--payloads"],
        vec!["search", &a, "--count-by", "component"],
        vec!["extract", &a, "--field", "value"],
        vec!["extract", &a, "--field", "x", "--expand-array", "items"],
        vec!["perf", &a],
        vec!["trace", &a, "--session", "scope"],
        vec!["process", &a],
        vec!["errors", &a, "--sessions"],
        vec!["compare", &a, &b],
        vec!["diff", &a, &b],
        vec!["llm-diff", &a, &b],
    ];
    for variant in variants {
        let mut args = vec!["--preset", "eyes"];
        args.extend(variant);
        let (complete, _, pages) = reconstruct(&args, "7");
        assert!(pages > 1);
        let sources = complete["evidence_records"].as_array().unwrap();
        let mut ids = std::collections::HashSet::new();
        for record in sources {
            assert!(ids.insert(record["evidence_ref"]["reference_id"].as_str().unwrap()));
        }
        assert!(sources.len() == 24 || sources.len() == 48);
    }
}

#[test]
fn tied_comparison_sort_orders_and_redaction_replay_deterministically() {
    let dir = tempdir().unwrap();
    let (a, b) = inputs(dir.path());
    for sort in ["time", "component", "level", "type", "diff-count"] {
        let args = [
            "--preset",
            "eyes",
            "--redact",
            "--mask-id",
            "component_id",
            "compare",
            &a,
            &b,
            "--sort-by",
            sort,
        ];
        let (complete, _, _) = reconstruct(&args, "17");
        assert!(!complete.to_string().contains("scope/"));
        assert!(!complete.to_string().contains("private-secret"));
    }
    let (complete, _, _) = reconstruct(
        &[
            "--preset",
            "eyes",
            "--redact",
            "--mask-id",
            "component_id",
            "process",
            &a,
        ],
        "9",
    );
    assert!(!complete.to_string().contains("private-secret"));
}

#[test]
fn final_serialized_budgets_handle_unicode_large_payloads_and_blocked_items() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("large.jsonl");
    let rows:Vec<_>=(0..5).map(|index|json!({"timestamp":"2026-01-01T00:00:00+02:00","level":"INFO","message":format!("row{index} {}", "漢👩🏽‍💻\\\"".repeat(2000)),"payload":{"nested":{"a":["界".repeat(5000)]},"password":"private-secret"}})).collect();
    fs::write(
        &file,
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let file = file.to_str().unwrap();
    let full = run(&["--complete-output", "--redact", "process", file]);
    assert!(full["logs"][0]["msg"].as_str().unwrap().len() > 200);
    assert!(full["logs"][0]["data"]["nested"]["a"].is_array());
    for (flag, limit) in [("--report-max-chars", 4000), ("--report-max-bytes", 6000)] {
        let limit_text = limit.to_string();
        let output = invoke(&["--redact", flag, &limit_text, "process", file]);
        assert!(output.status.success());
        let page: Value = serde_json::from_slice(&output.stdout).unwrap();
        validate(&page);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("private-secret"));
        let measured = if flag.ends_with("bytes") {
            output.stdout.len()
        } else {
            String::from_utf8_lossy(&output.stdout).chars().count()
        };
        assert!(
            measured <= limit || page["retrieval"]["metadata_over_budget"] == true,
            "{measured} > {limit}"
        );
        assert_eq!(page["retrieval"]["displayed_items"], 0);
        let cursor = page["retrieval"]["next_cursor"].as_str().unwrap();
        let next = run(&[
            "--redact",
            "--report-max-bytes",
            "250000",
            "--report-max-items",
            "1",
            "--report-cursor",
            cursor,
            "process",
            file,
        ]);
        assert_eq!(next["retrieval"]["prior_items"], 0);
        assert_eq!(next["retrieval"]["displayed_items"], 1);
    }
    let tiny = run(&["--report-max-bytes", "1", "search", file]);
    assert_eq!(
        tiny["retrieval"]["status"],
        "mandatory_metadata_over_budget"
    );
    assert_eq!(tiny["retrieval"]["metadata_over_budget"], true);
}

#[test]
fn cursor_changes_are_rejected_with_structured_saved_output_and_exit_one() {
    let dir = tempdir().unwrap();
    let (a, _) = inputs(dir.path());
    let page = run(&["--report-max-items", "1", "search", &a]);
    let cursor = page["retrieval"]["next_cursor"].as_str().unwrap();
    let saved = dir.path().join("error.json");
    for extra in [
        vec!["-f", "l:ERROR"],
        vec!["--redact"],
        vec!["--preset", "eyes"],
    ] {
        let mut args = vec![
            "--output",
            saved.to_str().unwrap(),
            "--report-cursor",
            cursor,
            "search",
            &a,
        ];
        args.extend(extra);
        let result = invoke(&args);
        assert_eq!(result.status.code(), Some(1));
        let report: Value = serde_json::from_slice(&result.stdout).unwrap();
        validate(&report);
        assert_eq!(report["retrieval"]["status"], "invalid_cursor");
        assert_eq!(
            report,
            serde_json::from_slice::<Value>(&fs::read(&saved).unwrap()).unwrap()
        );
    }
    fs::write(
        &a,
        fs::read_to_string(&a).unwrap() + "\ncore | 2026-01-01T00:00:03Z [INFO] changed\n",
    )
    .unwrap();
    let result = invoke(&["--report-cursor", cursor, "search", &a]);
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["retrieval"]["status"],
        "invalid_cursor"
    );
    for malformed in [
        "bad",
        "é:1",
        "abc:-1",
        &format!("{}:999999999999999999999999", "0".repeat(64)),
    ] {
        assert_eq!(
            invoke(&["--report-cursor", malformed, "search", &a])
                .status
                .code(),
            Some(1)
        );
    }
}

#[test]
fn common_flags_preserve_legacy_errors_and_reject_unretrievable_clipping() {
    let dir = tempdir().unwrap();
    let (a, _) = inputs(dir.path());
    let legacy = run(&["-j", "errors", &a, "--bounded"]);
    assert!(legacy.get("retrieval").is_none());
    assert!(
        !invoke(&["--report-max-items", "2", "errors", &a, "--bounded"])
            .status
            .success()
    );
    assert!(
        !invoke(&[
            "--complete-output",
            "--report-max-bytes",
            "10",
            "search",
            &a
        ])
        .status
        .success()
    );
    assert!(
        !invoke(&["--report-max-items", "2", "capabilities"])
            .status
            .success()
    );
    let zero = run(&["--report-max-items", "0", "search", &a]);
    assert_eq!(zero["retrieval"]["status"], "item_limit_zero");
    assert_eq!(zero["retrieval"]["prior_items"], 0);
    let saved = dir.path().join("page.json");
    let output = invoke(&[
        "--report-max-items",
        "2",
        "--output",
        saved.to_str().unwrap(),
        "search",
        &a,
    ]);
    assert!(output.status.success());
    assert_eq!(output.stdout, fs::read(saved).unwrap());
}

#[test]
fn nested_timeline_pages_reconstruct_diagnostics_and_preserve_redaction_loss() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("nested.jsonl");
    let profile = dir.path().join("profile.toml");
    let mut rows = Vec::new();
    for index in 0..3 {
        rows.push(json!({"ts":format!("2026-01-01T00:00:{:02}+02:00", index*2),"message":"begin","sid":"private-session"}));
        rows.push(json!({"ts":format!("2026-01-01T00:00:{:02}+02:00", index*2+1),"message":"end","sid":"private-session"}));
    }
    rows.push(json!({"ts":"invalid","message":"begin","sid":"private-session"}));
    fs::write(&file, json!({"prefixprivate-session":rows}).to_string()).unwrap();
    fs::write(&profile, "extends = 'base'\n[normalization]\nroot_path = '/prefixprivate-session'\nexpand_rows = true\n[normalization.fields]\ntimestamp = '/ts'\nmessage = '/message'\ncomponent_id = '/sid'\n[[timeline.events]]\nname = 'begin'\npattern = 'begin'\ncorrelation_fields = ['component_id']\n[[timeline.events]]\nname = 'end'\npattern = 'end'\ncorrelation_fields = ['component_id']\n[[timeline.pairs]]\nname = 'span'\nstart_event = 'begin'\nend_event = 'end'\ntiming = 'measured'\n").unwrap();
    for command in ["perf", "trace"] {
        let mut args = vec![
            "--config",
            profile.to_str().unwrap(),
            "--redact",
            "--mask-id",
            "component_id",
            command,
            file.to_str().unwrap(),
        ];
        if command == "trace" {
            args.extend(["--session", "private-session"]);
        }
        let (complete, _, _) = reconstruct(&args, "4");
        assert!(!complete.to_string().contains("private-session"));
        assert_eq!(complete["evidence_records"].as_array().unwrap().len(), 6);
        assert_eq!(
            complete["evidence_records"][0]["evidence_ref"]["location_redacted"],
            true
        );
        assert!(complete["evidence_records"][0]["evidence_ref"]["row_path"].is_null());
        let timeline = if command == "perf" {
            &complete["event_timeline"]
        } else {
            &complete["trace"]["event_timeline"]
        };
        assert_eq!(timeline["intervals"].as_array().unwrap().len(), 3);
        assert_eq!(timeline["intervals"][0]["measured_duration_ms"], 1000);
        if command == "perf" {
            assert_eq!(complete["coverage"]["files"][0]["rejected_candidates"], 1);
        }
    }
}

#[test]
fn changed_profile_contents_reject_cursor_and_structural_masks_keep_canonical_types() {
    let dir = tempdir().unwrap();
    let (a, _) = inputs(dir.path());
    let profile = dir.path().join("profile.toml");
    fs::write(&profile, "extends = 'base'\n[parser]\nformat = 'auto'\n").unwrap();
    let page = run(&[
        "--config",
        profile.to_str().unwrap(),
        "--report-max-items",
        "1",
        "search",
        &a,
    ]);
    let cursor = page["retrieval"]["next_cursor"].as_str().unwrap();
    fs::write(&profile, "extends = 'base'\n[parser]\nformat = 'classic'\n").unwrap();
    let output = invoke(&[
        "--config",
        profile.to_str().unwrap(),
        "--report-cursor",
        cursor,
        "search",
        &a,
    ]);
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    validate(&value);
    assert_eq!(value["retrieval"]["status"], "invalid_cursor");
    for field in [
        "evidence_records",
        "structured_fields",
        "input_ordinal",
        "timestamp_year_inferred",
        "timestamp",
        "classification",
        "semantics",
    ] {
        let value = run(&[
            "--complete-output",
            "--redact",
            "--mask-id",
            field,
            "perf",
            &a,
        ]);
        assert_eq!(value["evidence_records"].as_array().unwrap().len(), 24);
        assert!(value["evidence_records"][0]["input_ordinal"].is_number());
        assert!(value["evidence_records"][0]["timestamp_year_inferred"].is_boolean());
        let record = &value["evidence_records"][0];
        chrono::DateTime::parse_from_rfc3339(record["timestamp"].as_str().unwrap()).unwrap();
        assert!(record["classification"]["status"].is_string());
        assert!(!value.to_string().contains("private-secret"));
    }
    let numeric = dir.path().join("numeric.jsonl");
    fs::write(&numeric,json!({"timestamp":"2026-01-01T00:00:00Z","level":"INFO","message":"alive","payload":{"trace_id":701}}).to_string()).unwrap();
    let value = run(&[
        "--complete-output",
        "--redact",
        "--mask-id",
        "trace_id",
        "perf",
        numeric.to_str().unwrap(),
    ]);
    assert!(
        value["evidence_records"][0]["payload"]["trace_id"]
            .as_str()
            .unwrap()
            .starts_with("[MASKED_ID:")
    );
}

#[test]
fn byte_and_character_limits_apply_together_on_every_returned_page() {
    let dir = tempdir().unwrap();
    let (a, _) = inputs(dir.path());
    let full = run(&["--complete-output", "search", &a, "--payloads"]);
    let mut cursor: Option<String> = None;
    let mut ids = Vec::new();
    for _ in 0..200 {
        let mut args = vec![
            "--report-max-bytes",
            "18000",
            "--report-max-chars",
            "14000",
            "search",
            &a,
            "--payloads",
        ];
        if let Some(cursor) = &cursor {
            args.extend(["--report-cursor", cursor]);
        }
        let result = invoke(&args);
        assert!(result.status.success());
        let page: Value = serde_json::from_slice(&result.stdout).unwrap();
        validate(&page);
        assert!(result.stdout.len() <= 18000);
        assert!(String::from_utf8_lossy(&result.stdout).chars().count() <= 14000);
        assert!(matches!(
            page["retrieval"]["status"].as_str().unwrap(),
            "page" | "complete"
        ));
        ids.extend(
            page["evidence_records"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["evidence_ref"]["reference_id"].clone()),
        );
        cursor = page["retrieval"]["next_cursor"]
            .as_str()
            .map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    assert!(cursor.is_none());
    assert_eq!(
        ids,
        full["evidence_records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["evidence_ref"]["reference_id"].clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn canonical_configured_provenance_preserves_types_without_id_collisions() {
    let dir = tempdir().unwrap();
    let log = dir.path().join("events.log");
    fs::write(&log, "core (private-id) | 2026-01-01T00:00:00+02:00 [INFO] Request \"work\" [0--id] will be sent\ncore (private-id) | 2026-01-01T00:00:01+02:00 [INFO] Request \"work\" [0--id] finished successfully\n").unwrap();
    let profile = dir.path().join("candidate.toml");
    fs::write(
        &profile,
        toml::to_string(&json!({
            "extends":"eyes", "profile_name":"private-id",
            "event_rules":{"version":1,"rules":[{
                "id":"private-id", "adapter":{"type":"text","pattern":"Request .* will be sent"},
                "mapping":{"kind":"request","name":{"from":"literal","value":"work"},
                    "phase":{"from":"literal","value":"start"},
                    "correlation_id":{"from":"literal","value":"request1"},
                    "scope":[{"from":"literal","value":"generic"}]}
            }]}
        }))
        .unwrap(),
    )
    .unwrap();
    for field in [
        "component_id",
        "classification",
        "profile",
        "rule_ids",
        "diagnostics",
    ] {
        let result = invoke(&[
            "--config",
            profile.to_str().unwrap(),
            "--complete-output",
            "--redact",
            "--mask-id",
            field,
            "--mask-id",
            "component_id",
            "process",
            log.to_str().unwrap(),
        ]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(!text.contains("private-id"), "leaked under {field}: {text}");
        let value: Value = serde_json::from_str(&text).unwrap();
        validate(&value);
        let cls = &value["evidence_records"][0]["classification"];
        assert_eq!(cls["status"], "event");
        assert!(cls["profile"].is_string());
        assert!(cls["rule_ids"].is_array());
        assert_eq!(cls["rule_ids"].as_array().unwrap().len(), 1);
        assert!(
            cls["rule_ids"][0]
                .as_str()
                .unwrap()
                .starts_with("[MASKED_ID:")
        );
        if cls["profile"].is_string() {
            assert!(cls["profile"].as_str().unwrap().starts_with("[MASKED_ID:"));
        }
    }
}
