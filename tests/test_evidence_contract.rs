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
fn schema() -> Value {
    serde_json::from_str(include_str!("../schemas/report.schema.json")).unwrap()
}
fn validate(report: &Value) {
    let validator = jsonschema::validator_for(&schema()).unwrap();
    let errors: Vec<_> = validator
        .iter_errors(report)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "schema errors: {errors:?}\n{report}");
}
fn run(args: &[&str]) -> Value {
    let result = invoke(args);
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report = serde_json::from_slice(&result.stdout).unwrap();
    validate(&report);
    report
}
fn evidence(report: &Value) -> &Value {
    &report["report_metadata"]["evidence"]
}
fn fixture() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/synthetic.jsonl")
        .to_str()
        .unwrap()
        .into()
}

#[test]
fn published_schemas_cover_cli_variants_and_redaction() {
    let fixture = fixture();
    let variants = vec![
        vec!["info", fixture.as_str()],
        vec!["search", &fixture],
        vec!["search", &fixture, "--count-by", "matches"],
        vec!["search", &fixture, "--count-by", "component"],
        vec!["extract", &fixture, "--field", "request_id"],
        vec!["extract", &fixture, "--field", "request_id", "--rows"],
        vec![
            "extract",
            &fixture,
            "--field",
            "id",
            "--expand-array",
            "items",
        ],
        vec!["process", &fixture],
        vec!["process", &fixture, "--limit", "1"],
        vec!["errors", &fixture, "--bounded", "--sessions"],
        vec!["perf", &fixture],
        vec!["perf", &fixture, "--orphans-only"],
        vec!["trace", &fixture, "--id", "demo-123"],
        vec!["compare", &fixture, &fixture],
        vec!["diff", &fixture, &fixture],
        vec!["llm-diff", &fixture, &fixture],
        vec!["schema", &fixture],
    ];
    for variant in variants {
        for presentation in [
            vec!["-F", "json"],
            vec!["-j"],
            vec!["-j", "--redact", "--mask-id", "request_id"],
        ] {
            let mut args = vec!["--preset", "eyes"];
            args.extend(presentation);
            args.extend(&variant);
            run(&args);
        }
    }
    let caps: Value = serde_json::from_slice(&invoke(&["capabilities"]).stdout).unwrap();
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/capabilities.schema.json")).unwrap();
    assert!(jsonschema::is_valid(&schema, &caps));
}

#[test]
fn references_survive_selection_and_detect_changed_input_and_profile() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("input.jsonl");
    fs::copy(fixture(), &path).unwrap();
    let file = path.to_str().unwrap();
    let full = run(&["-j", "search", file]);
    let selected = run(&[
        "-j",
        "--redact",
        "--mask-id",
        "request_id",
        "-f",
        "l:ERROR",
        "search",
        file,
    ]);
    assert_eq!(
        full["search"]["entries"][1]["evidence_ref"],
        selected["search"]["entries"][0]["evidence_ref"]
    );
    assert_eq!(
        evidence(&full)["snapshot_id"],
        evidence(&selected)["snapshot_id"]
    );
    let process = run(&["process", file, "--sort-by", "level", "--limit", "1"]);
    assert_eq!(
        process["logs"][0]["evidence_ref"],
        full["search"]["entries"][1]["evidence_ref"]
    );
    assert_eq!(evidence(&process)["omissions"]["records"], 1);
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("worker failed", "worker timed out"),
    )
    .unwrap();
    let changed = run(&["-j", "search", file]);
    assert_ne!(
        evidence(&full)["snapshot_id"],
        evidence(&changed)["snapshot_id"]
    );
    assert_ne!(
        full["search"]["entries"][0]["evidence_ref"],
        changed["search"]["entries"][0]["evidence_ref"]
    );
    let profile = dir.path().join("profile.toml");
    fs::write(&profile, "extends = 'base'\nprofile_name = 'same-name'\n").unwrap();
    let a = run(&["--config", profile.to_str().unwrap(), "-j", "search", file]);
    fs::write(
        &profile,
        "extends = 'base'\nprofile_name = 'same-name'\n[parser]\nformat = 'json-lines'\n",
    )
    .unwrap();
    let b = run(&["--config", profile.to_str().unwrap(), "-j", "search", file]);
    assert_ne!(
        evidence(&a)["profile_sha256"],
        evidence(&b)["profile_sha256"]
    );
    assert_eq!(
        a["search"]["entries"][0]["evidence_ref"],
        b["search"]["entries"][0]["evidence_ref"]
    );
}

#[test]
fn timing_boundaries_distinguish_files_and_missing_or_ambiguous_is_not_measured() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.log");
    let b = dir.path().join("b.log");
    fs::write(&a,"worker (demo) | 2026-01-01T00:00:00+02:00 [INFO] Request \"work\" [0--demo] will be sent\n").unwrap();
    fs::write(&b,"worker (demo) | 2026-01-01T00:00:01+02:00 [INFO] Request \"work\" [0--demo] finished successfully\n").unwrap();
    for flags in [
        vec!["-j"],
        vec!["-j", "--redact", "--mask-id", "component_id"],
    ] {
        let mut args = vec!["--preset", "eyes"];
        args.extend(flags);
        args.extend(["perf", a.to_str().unwrap(), b.to_str().unwrap()]);
        let report = run(&args);
        let op = &report["operations"][0];
        assert_eq!(op["duration_ms"], 1000);
        assert_eq!(op["start_source"]["line"], 1);
        assert_eq!(op["end_source"]["line"], 1);
        assert_ne!(
            op["start_source"]["evidence_ref"]["input_id"],
            op["end_source"]["evidence_ref"]["input_id"]
        );
        let mut invalid = report.clone();
        invalid["operations"][0]
            .as_object_mut()
            .unwrap()
            .remove("end_source");
        assert!(!jsonschema::is_valid(&schema(), &invalid));
    }
    let report = run(&["--preset", "eyes", "-j", "perf", a.to_str().unwrap()]);
    assert!(report["operations"].as_array().unwrap().is_empty());
    let duplicate = fs::read_to_string(&a).unwrap();
    fs::write(&a, format!("{duplicate}{duplicate}")).unwrap();
    let report = run(&[
        "--preset",
        "eyes",
        "-j",
        "perf",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
    ]);
    assert!(report["operations"].as_array().unwrap().is_empty());
    assert_eq!(report["ambiguous_groups"].as_array().unwrap().len(), 1);
}

#[test]
fn nested_and_extracted_rows_remain_addressable() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("nested.jsonl");
    let cfg = dir.path().join("profile.toml");
    fs::write(&input,json!({"rows":[{"ts":"2026-01-01T00:00:00+02:00","message":"one","payload":{"items":[{"id":"a"},{"id":"b"}]}},{"ts":"2026-01-01T00:00:01+02:00","message":"two","payload":{"items":[{"id":"c"}]}}]}).to_string()).unwrap();
    fs::write(&cfg,"extends = 'base'\n[normalization]\nroot_path = '/rows'\nexpand_rows = true\n[normalization.fields]\ntimestamp = '/ts'\nmessage = '/message'\npayload = '/payload'\n").unwrap();
    let a = run(&[
        "--config",
        cfg.to_str().unwrap(),
        "-j",
        "search",
        input.to_str().unwrap(),
    ]);
    assert_ne!(
        a["search"]["entries"][0]["evidence_ref"],
        a["search"]["entries"][1]["evidence_ref"]
    );
    let b = run(&[
        "--config",
        cfg.to_str().unwrap(),
        "-j",
        "extract",
        input.to_str().unwrap(),
        "--expand-array",
        "items",
        "--field",
        "id",
    ]);
    assert_ne!(
        b["extract"]["rows"][0]["source"]["evidence_ref"],
        b["extract"]["rows"][1]["source"]["evidence_ref"]
    );
    assert_eq!(
        b["extract"]["rows"][0]["source"]["evidence_ref"]["row_path"],
        "/rows/0"
    );
}

#[test]
fn empty_filtered_unparsed_and_measured_zero_are_distinct() {
    let dir = tempdir().unwrap();
    let empty = dir.path().join("empty");
    fs::write(&empty, " \n").unwrap();
    let a = run(&["-j", "info", empty.to_str().unwrap()]);
    assert_eq!(evidence(&a)["scope"]["status"], "empty_input");
    let b = run(&["-j", "-f", "l:DEBUG", "info", &fixture()]);
    assert_eq!(evidence(&b)["scope"]["status"], "zero_filter_matches");
    let c = run(&["-j", "errors", &fixture()]);
    assert_eq!(evidence(&c)["scope"]["status"], "parsed");
    fs::write(&empty, "unsupported arbitrary input\n").unwrap();
    let result = invoke(&["-j", "info", empty.to_str().unwrap()]);
    assert_eq!(result.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    validate(&value);
    assert_eq!(evidence(&value)["scope"]["status"], "unparsed_input");
    let parsed = log_analyzer::parser::parse_log_file_report(
        fixture(),
        log_analyzer::config::default_config(),
    )
    .unwrap();
    assert_eq!(
        parsed.coverage.snapshot_sha256,
        log_analyzer::evidence::digest(&fs::read(fixture()).unwrap())
    );
}

#[test]
fn investigation_schema_rejects_hypotheses_as_measurements_and_invalid_citations() {
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/investigation.schema.json")).unwrap();
    let hash = "a".repeat(64);
    let reference = json!({"reference_id":hash,"location_redacted":false,"input_id":hash,"line":1,"row_path":null});
    let mut result = json!({"contract_version":1,"input_snapshot_id":hash,"profile_sha256":hash,"status":"supported","findings":[{"kind":"measurement","claim":"observed span","value":0,"unit":"ms","boundaries":{"start":reference,"end":reference},"profile_sha256":hash,"semantics":"elapsed_span","supporting_refs":[reference]}]});
    assert!(jsonschema::is_valid(&schema, &result));
    result["findings"][0]["kind"] = json!("hypothesis");
    assert!(!jsonschema::is_valid(&schema, &result));
    result["findings"][0]["kind"] = json!("measurement");
    result["findings"][0]["boundaries"]["end"]["line"] = json!(0);
    assert!(!jsonschema::is_valid(&schema, &result));
}

#[test]
fn structural_mask_fields_and_bounded_errors_preserve_identity_and_hide_paths() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("demo-sensitive-id.jsonl");
    fs::copy(fixture(), &path).unwrap();
    let file = path.to_str().unwrap();
    let original = run(&["-j", "search", file]);
    for field in [
        "input_id",
        "snapshot_sha256",
        "source_row_path",
        "source_line_number",
        "line",
    ] {
        let report = run(&[
            "-j",
            "--redact",
            "--mask-id",
            field,
            "trace",
            file,
            "--session",
            "demo",
        ]);
        assert_eq!(
            report["trace"]["span_boundaries"]["start"]["input_id"],
            original["search"]["entries"][0]["evidence_ref"]["input_id"]
        );
        assert_eq!(
            evidence(&report)["inputs"][0]["coverage"]["snapshot_sha256"],
            evidence(&original)["inputs"][0]["sha256"]
        );
    }
    let report = run(&[
        "-j",
        "--redact",
        "--mask-id",
        "component_id",
        "errors",
        file,
        "--bounded",
        "--sessions",
    ]);
    assert!(!report.to_string().contains("demo-sensitive-id.jsonl"));
    let process = run(&["--redact", "process", file]);
    assert_eq!(
        evidence(&process)["redaction"]["legacy_sanitization"],
        false
    );
    let grouped = run(&["-j", "search", file, "--count-by", "matches"]);
    assert!(evidence(&grouped)["omissions"]["records"].is_null());
}

#[test]
fn nested_measurement_schemas_reject_missing_boundaries() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("timing.log");
    let cfg = dir.path().join("profile.toml");
    fs::write(&path,"worker (demo) | 2026-01-01T00:00:00+02:00 [INFO] Request \"work\" [0--demo] will be sent\nworker (demo) | 2026-01-01T00:00:01+02:00 [INFO] Request \"work\" [0--demo] finished successfully\nworker (demo) | 2026-01-01T00:00:02+02:00 [ERROR] failed\nworker (demo) | 2026-01-01T00:00:03+02:00 [INFO] alive\n").unwrap();
    fs::write(&cfg,"extends = 'eyes'\n[[timeline.events]]\nname = 'begin'\npattern = 'will be sent'\ncorrelation_fields = ['component_id']\n[[timeline.events]]\nname = 'end'\npattern = 'finished successfully'\ncorrelation_fields = ['component_id']\n[[timeline.pairs]]\nname = 'span'\nstart_event = 'begin'\nend_event = 'end'\ntiming = 'measured'\n").unwrap();
    let file = path.to_str().unwrap();
    let config = cfg.to_str().unwrap();
    for command in ["perf", "trace"] {
        let mut args = vec!["--config", config, "-j", command, file];
        if command == "trace" {
            args.extend(["--session", "demo"]);
        }
        let report = run(&args);
        let prefix = if command == "trace" {
            "/trace/event_timeline"
        } else {
            "/event_timeline"
        };
        let mut invalid = report.clone();
        invalid
            .pointer_mut(&format!("{prefix}/intervals/0/end/source"))
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("evidence_ref");
        assert!(!jsonschema::is_valid(&schema(), &invalid));
        if command == "perf" {
            let mut invalid = report;
            invalid["threshold_violations"][0]
                .as_object_mut()
                .unwrap()
                .remove("start_source");
            assert!(!jsonschema::is_valid(&schema(), &invalid));
        }
    }
    let report = run(&["--config", config, "-j", "errors", file, "--sessions"]);
    for pointer in [
        "/errors/summary/longest_blocking/end_source",
        "/errors/clusters/0/affected_sessions/0/start_source",
    ] {
        let mut invalid = report.clone();
        *invalid.pointer_mut(pointer).unwrap() = json!(null);
        assert!(!jsonschema::is_valid(&schema(), &invalid));
    }
}

#[test]
fn trace_inferred_years_never_become_measured_facts_and_labels_survive_masking() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("rollover.log");
    let file = path.to_str().unwrap();
    fs::write(
        &path,
        "Dec 31 23:59:59 host app[1]: INFO id=trace\nJan  1 00:00:00 host app[1]: INFO id=trace\n",
    )
    .unwrap();
    let report = run(&["-j", "trace", file, "--id", "trace"]);
    assert_eq!(report["trace"]["span_boundaries"]["kind"], "unavailable");
    assert!(report["trace"]["span_boundaries"]["measurement_ms"].is_null());
    let mut invalid = report;
    invalid["trace"]["span_boundaries"]["kind"] = json!("measurement");
    invalid["trace"]["span_boundaries"]["measurement_ms"] =
        json!(invalid["trace"]["total_duration_ms"]);
    invalid["trace"]["span_boundaries"]["reason"] = Value::Null;
    assert!(!jsonschema::is_valid(&schema(), &invalid));
    fs::write(
        &path,
        "worker (elapsed_span_of_matches) | 2026-01-01T00:00:00+02:00 [INFO] alive\n",
    )
    .unwrap();
    let report = run(&[
        "-j",
        "--redact",
        "--mask-id",
        "component_id",
        "trace",
        file,
        "--session",
        "elapsed_span_of_matches",
    ]);
    assert_eq!(
        report["trace"]["timing_semantics"],
        "elapsed_span_of_matches"
    );
    assert_eq!(report["trace"]["span_boundaries"]["measurement_ms"], 0);
}

#[test]
fn masked_normalization_pointers_report_loss_without_changing_reference_identity() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nested.jsonl");
    let cfg = dir.path().join("profile.toml");
    fs::write(&path,json!({"private-id":[{"ts":"2026-01-01T00:00:00+02:00","message":"alive","component_id":"private-id","payload":{"items":[{"id":"a"},{"id":"b"}]}}]}).to_string()).unwrap();
    fs::write(&cfg,"extends = 'base'\n[normalization]\nroot_path = '/private-id'\nexpand_rows = true\n[normalization.fields]\ntimestamp = '/ts'\nmessage = '/message'\ncomponent_id = '/component_id'\npayload = '/payload'\n").unwrap();
    for command in ["search", "extract"] {
        let mut args = vec![
            "--config",
            cfg.to_str().unwrap(),
            "-j",
            command,
            path.to_str().unwrap(),
        ];
        if command == "extract" {
            args.extend(["--field", "id", "--expand-array", "items"]);
        }
        let complete = run(&args);
        args.extend(["--redact", "--mask-id", "component_id"]);
        let redacted = run(&args);
        assert!(
            !redacted.to_string().contains("private-id"),
            "{command}: {redacted}"
        );
        let pointer = if command == "extract" {
            "/extract/rows/0/source/evidence_ref"
        } else {
            "/search/entries/0/evidence_ref"
        };
        let before = complete.pointer(pointer).unwrap();
        let after = redacted.pointer(pointer).unwrap();
        assert_eq!(before["reference_id"], after["reference_id"]);
        assert_eq!(after["location_redacted"], true);
        assert!(after["row_path"].is_null());
    }
}

#[test]
fn bounded_nested_error_locations_are_redacted_before_prepared_output() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nested.jsonl");
    let cfg = dir.path().join("profile.toml");
    fs::write(&path,json!({"private-id":[{"ts":"2026-01-01T00:00:00+02:00","message":"failed","level":"ERROR","component_id":"private-id"},{"ts":"2026-01-01T00:00:01+02:00","message":"alive","level":"INFO","component_id":"private-id"}]}).to_string()).unwrap();
    fs::write(&cfg,"extends = 'base'\n[normalization]\nroot_path = '/private-id'\nexpand_rows = true\n[normalization.fields]\ntimestamp = '/ts'\nmessage = '/message'\nlevel = '/level'\ncomponent_id = '/component_id'\n").unwrap();
    let args = [
        "--config",
        cfg.to_str().unwrap(),
        "-j",
        "--redact",
        "--mask-id",
        "component_id",
        "errors",
        path.to_str().unwrap(),
        "--bounded",
        "--sessions",
    ];
    let report = run(&args);
    assert!(!report.to_string().contains("private-id"), "{report}");
    for pointer in [
        "/errors/clusters/0/sample_source",
        "/errors/clusters/0/affected_sessions/0/start_source",
        "/errors/clusters/0/affected_sessions/0/end_source",
        "/errors/summary/longest_blocking/start_source",
        "/errors/summary/longest_blocking/end_source",
    ] {
        let source = report.pointer(pointer).unwrap();
        assert_eq!(source["evidence_ref"]["location_redacted"], true);
        assert!(source["evidence_ref"]["row_path"].is_null());
        assert!(
            source["row_path"]
                .as_str()
                .unwrap()
                .starts_with("/[MASKED_ID:")
        );
    }
}

#[test]
fn embedded_ids_in_normalized_and_extraction_paths_preserve_opaque_identity() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nested.jsonl");
    let cfg = dir.path().join("profile.toml");
    fs::write(&path,json!({"prefixprivate-id":[{"ts":"2026-01-01T00:00:00+02:00","message":"alive","component_id":"private-id","payload":{"prefixprivate-id":[{"id":"a"}]}}]}).to_string()).unwrap();
    fs::write(&cfg,"extends = 'base'\n[normalization]\nroot_path = '/prefixprivate-id'\nexpand_rows = true\n[normalization.fields]\ntimestamp = '/ts'\nmessage = '/message'\ncomponent_id = '/component_id'\npayload = '/payload'\n").unwrap();
    for command in ["search", "trace", "process", "extract"] {
        let mut args = vec![
            "--config",
            cfg.to_str().unwrap(),
            "-j",
            command,
            path.to_str().unwrap(),
        ];
        if command == "trace" {
            args.extend(["--session", "private-id"]);
        }
        if command == "extract" {
            args.extend(["--expand-array", "prefixprivate-id", "--field", "id"]);
        }
        let full = run(&args);
        args.extend(["--redact", "--mask-id", "component_id"]);
        let redacted = run(&args);
        assert!(
            !redacted.to_string().contains("private-id"),
            "{command}: {redacted}"
        );
        let pointer = match command {
            "search" => "/search/entries/0/evidence_ref",
            "trace" => "/trace/entries/0/evidence_ref",
            "process" => "/logs/0/evidence_ref",
            _ => "/extract/rows/0/source/evidence_ref",
        };
        let before = full.pointer(pointer).unwrap();
        let after = redacted.pointer(pointer).unwrap();
        assert_eq!(before["reference_id"], after["reference_id"]);
        assert_eq!(after["location_redacted"], true);
    }
}

#[test]
fn generated_source_objects_survive_mask_field_collisions() {
    let dir = tempdir().unwrap();
    let a = dir.path().join("a.jsonl");
    let b = dir.path().join("b.jsonl");
    for (path, n) in [(&a, 1), (&b, 2)] {
        fs::write(path,json!({"ts":"2026-01-01T00:00:00Z","component":"worker","component_id":"demo","level":"ERROR","message":"same","payload":{"value":n}}).to_string()).unwrap();
    }
    for field in [
        "sample_source",
        "start_source",
        "end_source",
        "evidence_ref",
    ] {
        run(&[
            "-j",
            "--redact",
            "--mask-id",
            field,
            "errors",
            a.to_str().unwrap(),
            "--sessions",
        ]);
    }
    for field in ["log1_source", "log2_source", "evidence_ref"] {
        let report = run(&[
            "-j",
            "--redact",
            "--mask-id",
            field,
            "compare",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
        ]);
        assert!(
            report["comparisons"][0]["instances"][0]["log1_source"]["evidence_ref"].is_object()
        );
        assert!(
            report["comparisons"][0]["instances"][0]["log2_source"]["evidence_ref"].is_object()
        );
    }
}

#[test]
fn normalized_timeline_and_operation_exports_omit_hidden_source_keys() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("nested.jsonl");
    let cfg = dir.path().join("profile.toml");
    fs::write(&path,json!({"prefixprivate-id":[{"ts":"2026-01-01T00:00:00+02:00","message":"begin","component_id":"private-id"},{"ts":"2026-01-01T00:00:01+02:00","message":"end","component_id":"private-id"},{"ts":"2026-01-01T00:00:02+02:00","message":"begin","component_id":"private-id"}]}).to_string()).unwrap();
    fs::write(&cfg,"extends = 'base'\n[normalization]\nroot_path = '/prefixprivate-id'\nexpand_rows = true\n[normalization.fields]\ntimestamp = '/ts'\nmessage = '/message'\ncomponent_id = '/component_id'\n[[timeline.events]]\nname = 'begin'\npattern = 'begin'\ncorrelation_fields = ['component_id']\n[[timeline.events]]\nname = 'end'\npattern = 'end'\ncorrelation_fields = ['component_id']\n[[timeline.pairs]]\nname = 'span'\nstart_event = 'begin'\nend_event = 'end'\ntiming = 'measured'\n").unwrap();
    for command in ["perf", "trace"] {
        let mut args = vec![
            "--config",
            cfg.to_str().unwrap(),
            "-j",
            "--redact",
            "--mask-id",
            "component_id",
            command,
            path.to_str().unwrap(),
        ];
        if command == "trace" {
            args.extend(["--session", "private-id"]);
        }
        let report = run(&args);
        assert!(
            !report.to_string().contains("private-id"),
            "{command}: {report}"
        );
        let pointer = if command == "trace" {
            "/trace/event_timeline"
        } else {
            "/event_timeline"
        };
        let timeline = report.pointer(pointer).unwrap();
        assert_eq!(timeline["events"][0]["raw"], "[REDACTED SOURCE LOCATION]");
        assert_eq!(
            timeline["intervals"][0]["end"]["raw"],
            "[REDACTED SOURCE LOCATION]"
        );
        assert_eq!(
            timeline["incomplete"][0]["event"]["raw"],
            "[REDACTED SOURCE LOCATION]"
        );
    }
}
