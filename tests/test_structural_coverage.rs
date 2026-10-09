use log_analyzer::{
    config::{AnalyzerConfig, LogFormat},
    normalize::NormalizationRules,
    parser::{ParsedLogFile, parse_log_file_report},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, process::Command};
use tempfile::tempdir;

fn parse(content: &str, config: &AnalyzerConfig) -> ParsedLogFile {
    let dir = tempdir().unwrap();
    let file = dir.path().join("synthetic.log");
    fs::write(&file, content).unwrap();
    let result = parse_log_file_report(&file, config).unwrap();
    assert_eq!(result.coverage.input_bytes, content.len() as u64);
    assert_eq!(
        result.coverage.snapshot_sha256,
        log_analyzer::evidence::digest(content.as_bytes())
    );
    let structure = &result.coverage.structural_diagnostics;
    assert_eq!(
        structure.physical_candidate_blocks + structure.attached_nonempty_lines,
        result.coverage.nonempty_lines
    );
    assert_eq!(
        structure.diagnostics.len() + structure.omitted_diagnostics,
        structure.diagnostic_count
    );
    result
}

#[test]
fn unsupported_python_headers_after_sample_are_isolated_from_real_records() {
    let mut content = String::new();
    for _ in 0..10 {
        content.push_str("服务/io | 2026-01-01T00:00:00.000+02:00 [INFO ] started\n");
    }
    content.push_str("2026-01-01 10:00:00,123 ERROR service - failure\nTraceback (most recent call last):\n  File \"synthetic.py\", line 1\nValueError: synthetic failure\n");
    content.push_str("worker | 2026-01-01T00:00:01.000Z [INFO ] finished\n");
    let report = parse(&content, &AnalyzerConfig::default());
    assert_eq!(report.entries.len(), 11);
    assert_eq!(report.coverage.rejected_candidates, 1);
    let structure = &report.coverage.structural_diagnostics;
    assert_eq!(structure.sampled_nonempty_lines, 10);
    assert_eq!(structure.sample_format_matches.classic, 10);
    assert_eq!(structure.observed_format_matches.classic, 11);
    assert_eq!(structure.unsupported_python_headers, 1);
    assert_eq!(structure.attached_nonempty_lines, 3);
    assert_eq!(structure.diagnostics[0].line, 11);
    assert_eq!(structure.diagnostics[0].reason, "unsupported_python_header");
    assert!(!report.entries[9].raw_logline.contains("Traceback"));
    assert_eq!(report.entries[10].source_line_number, 15);
    assert_eq!(
        report.entries[0]
            .source_timestamp
            .unwrap()
            .offset()
            .local_minus_utc(),
        7200
    );
}

#[test]
fn no_match_ties_mixed_formats_and_explicit_choices_disclose_limits() {
    let unsupported = parse(
        "2026-01-01 10:00:00 [ERROR] service failure\n  at frame\n",
        &AnalyzerConfig::default(),
    );
    assert!(unsupported.coverage.is_unparsed());
    assert_eq!(
        unsupported.coverage.structural_diagnostics.sample_status,
        "no_match"
    );
    assert_eq!(
        unsupported
            .coverage
            .structural_diagnostics
            .unsupported_python_headers,
        1
    );
    let content = concat!(
        "worker | 2026-01-01T00:00:00.000Z [INFO ] classic\n",
        "{\"timestamp\":\"2026-01-01T00:00:01Z\",\"message\":\"json\"}\n",
    );
    let report = parse(content, &AnalyzerConfig::default());
    assert_eq!(report.coverage.selected_parser, LogFormat::Classic);
    assert_eq!(report.coverage.structural_diagnostics.sample_status, "tied");
    assert_eq!(
        report.coverage.structural_diagnostics.observed_status,
        "tied"
    );
    assert_eq!(report.coverage.rejected_candidates, 1);
    assert_eq!(
        report.coverage.structural_diagnostics.diagnostics[0].reason,
        "selected_parser_mismatch"
    );
    let mut config = AnalyzerConfig::default();
    config.parser.format = LogFormat::Classic;
    let report = parse(content, &config);
    assert_eq!(report.coverage.structural_diagnostics.selection, "explicit");
    assert_eq!(
        report.coverage.structural_diagnostics.sample_status,
        "not_sampled"
    );
    assert_eq!(
        report
            .coverage
            .structural_diagnostics
            .sampled_nonempty_lines,
        0
    );
    assert_eq!(report.coverage.rejected_candidates, 1);
    let content = format!(
        "{}{}",
        "worker | 2026-01-01T00:00:00.000Z [INFO ] classic\n".repeat(10),
        "2026-01-01T00:00:01Z INFO module::worker: traced\n"
    );
    let report = parse(&content, &AnalyzerConfig::default());
    assert_eq!(
        report.coverage.structural_diagnostics.sample_status,
        "single_format"
    );
    assert_eq!(
        report.coverage.structural_diagnostics.observed_status,
        "mixed"
    );
    assert_eq!(
        report.coverage.structural_diagnostics.diagnostics[0].line,
        11
    );
}

#[test]
fn blank_lines_indented_headers_and_multiline_data_preserve_source_bytes() {
    let content = concat!(
        "\r\n  \r\nworker/io | 2026-01-01T00:00:00.000Z [INFO ] payload {\r\n",
        "  \"value\": \"🦀\"\r\n}\r\n\r\n",
        "    2026-01-01 10:00:00 ERROR indented evidence\r\n",
        "worker/io | 2026-99-01T00:00:01.000Z [ERROR] malformed\r\n",
        "    at rejected frame\r\n",
        "worker | 2026-01-01T00:00:02.000Z [INFO ] finished\r\n",
    );
    let report = parse(content, &AnalyzerConfig::default());
    assert_eq!(report.entries.len(), 2);
    assert_eq!(report.coverage.rejected_candidates, 1);
    let structure = &report.coverage.structural_diagnostics;
    assert_eq!(structure.blank_lines, 3);
    assert_eq!(structure.physical_candidate_blocks, 3);
    assert_eq!(structure.attached_nonempty_lines, 4);
    assert_eq!(structure.unsupported_python_headers, 0);
    assert_eq!(structure.diagnostics[0].line, 8);
    assert_eq!(structure.diagnostics[0].reason, "invalid_selected_record");
    assert_eq!(report.entries[0].source_line_number, 3);
    assert!(report.entries[0].raw_logline.contains("}\n\n    2026"));
    assert_eq!(report.entries[0].payload(), Some(&json!({"value":"🦀"})));
}

#[test]
fn normalized_rows_are_distinct_from_physical_block_populations() {
    let config = AnalyzerConfig {
        normalization: Some(NormalizationRules {
            root_path: "/rows".into(),
            expand_rows: true,
            fields: BTreeMap::from([
                ("timestamp".into(), "/time".into()),
                ("message".into(), "/message".into()),
            ]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let report = parse(
        "{\"rows\":[{\"time\":\"2026-01-01T00:00:00Z\",\"message\":\"a\"},{\"time\":\"2026-01-01T00:00:01Z\",\"message\":\"b\"},{\"time\":null,\"message\":\"c\"}]}\n",
        &config,
    );
    assert_eq!(report.entries.len(), 2);
    assert_eq!(report.coverage.rejected_candidates, 1);
    assert_eq!(report.coverage.nonempty_lines, 1);
    assert_eq!(
        report
            .coverage
            .structural_diagnostics
            .physical_candidate_blocks,
        1
    );
    assert_eq!(
        report.coverage.structural_diagnostics.selection,
        "normalization"
    );
    assert_eq!(report.coverage.normalization_diagnostics.len(), 1);
}

#[test]
fn rejection_locations_are_bounded_without_raw_customer_content() {
    let report = parse(
        &"worker | 2026-99-01T00:00:00.000Z [ERROR] synthetic marker\n".repeat(25),
        &AnalyzerConfig::default(),
    );
    let structure = &report.coverage.structural_diagnostics;
    assert_eq!(structure.diagnostic_count, 25);
    assert_eq!(structure.diagnostics.len(), 20);
    assert_eq!(structure.omitted_diagnostics, 5);
    assert_eq!(structure.diagnostics[19].line, 20);
    assert!(
        !serde_json::to_string(structure)
            .unwrap()
            .contains("synthetic marker")
    );
}

#[test]
fn shared_cli_reports_disclose_identical_structural_coverage_and_schema() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("synthetic.log");
    fs::write(&file, "worker | 2026-01-01T00:00:00.000Z [INFO ] valid\n2026-01-01 10:00:00,123 ERROR service - failure\n  at frame\n").unwrap();
    let binary = env!("CARGO_BIN_EXE_log-analyzer");
    let mut expected = None;
    for operation in ["info", "errors", "perf"] {
        let output = Command::new(binary)
            .args([
                "--preset",
                "base",
                "-F",
                "json",
                operation,
                file.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        let structure = &value["coverage"]["files"][0]["structural_diagnostics"];
        assert_eq!(
            structure["diagnostics"][0]["reason"],
            "unsupported_python_header"
        );
        if let Some(previous) = &expected {
            assert_eq!(previous, structure);
        }
        expected = Some(structure.clone());
        for schema in ["report", "investigation", "evidence-artifact"] {
            let schema: Value = serde_json::from_str(
                &fs::read_to_string(format!("schemas/{schema}.schema.json")).unwrap(),
            )
            .unwrap();
            jsonschema::validator_for(&json!({
                "$defs": schema["$defs"],
                "$ref": "#/$defs/coverage/properties/structural_diagnostics"
            }))
            .unwrap()
            .validate(structure)
            .unwrap();
        }
        let output = Command::new(binary)
            .args([
                "--preset",
                "base",
                "--color",
                "never",
                operation,
                file.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            text.contains("Structural rejection at line 2: unsupported_python_header"),
            "{text}"
        );
        assert!(text.contains("attached=1 (unverified)"));
        assert!(text.contains("capture/semantics unknown"));
    }
    fs::write(
        &file,
        "2026-01-01 10:00:00,123 ERROR service - failure\n  at frame\n",
    )
    .unwrap();
    let output = Command::new(binary)
        .args([
            "--preset",
            "base",
            "-F",
            "json",
            "info",
            file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["coverage"]["status"], "unparsed_input");
    assert_eq!(
        value["coverage"]["files"][0]["structural_diagnostics"]["sample_status"],
        "no_match"
    );
}

#[test]
fn empty_and_blank_inputs_have_explicit_no_match_observations() {
    for content in ["", "\n\r\n  \n\t\n"] {
        let report = parse(content, &AnalyzerConfig::default());
        assert!(!report.coverage.is_unparsed());
        assert_eq!(report.coverage.nonempty_lines, 0);
        let structure = &report.coverage.structural_diagnostics;
        assert_eq!(structure.sample_status, "no_match");
        assert_eq!(structure.physical_candidate_blocks, 0);
        assert_eq!(structure.blank_lines, content.lines().count());
        assert_eq!(structure.diagnostic_count, 0);
    }
}

#[test]
fn redacted_pages_retrieve_retained_diagnostics_and_preserve_global_omissions() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("synthetic.log");
    let content = format!(
        "worker | 2026-01-01T00:00:00Z [INFO ] started\n{}",
        "worker | 2026-99-01T00:00:00Z [ERROR] password=synthetic-secret\n".repeat(25)
    );
    fs::write(&file, content).unwrap();
    let mut cursor: Option<String> = None;
    let mut lines = Vec::new();
    let mut snapshot = None;
    let path = "/coverage/files/0/structural_diagnostics/diagnostics";
    for page_number in 0..70 {
        let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
        command.args([
            "--preset",
            "base",
            "--redact",
            "--mask-id",
            "component",
            "--report-max-items",
            "1",
            "info",
            file.to_str().unwrap(),
        ]);
        if let Some(cursor) = &cursor {
            command.args(["--report-cursor", cursor]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains("synthetic-secret"));
        let page: Value = serde_json::from_str(&text).unwrap();
        let structure = &page["coverage"]["files"][0]["structural_diagnostics"];
        assert_eq!(structure["diagnostic_count"], 25);
        assert_eq!(structure["omitted_diagnostics"], 5);
        let identity = &page["report_metadata"]["evidence"]["snapshot_id"];
        if let Some(previous) = &snapshot {
            assert_eq!(previous, identity);
        }
        snapshot = Some(identity.clone());
        let collection = page["retrieval"]["collections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["path"] == path)
            .unwrap();
        assert_eq!(collection["total"], 20);
        assert_eq!(
            collection["prior"].as_u64().unwrap()
                + collection["displayed"].as_u64().unwrap()
                + collection["remaining"].as_u64().unwrap(),
            20
        );
        for diagnostic in structure["diagnostics"].as_array().unwrap() {
            lines.push(diagnostic["line"].as_u64().unwrap());
            assert_eq!(diagnostic["reason"], "invalid_selected_record");
        }
        cursor = page["retrieval"]["next_cursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
        assert!(page_number < 69, "pagination did not terminate");
    }
    assert_eq!(lines, (2..=21).collect::<Vec<_>>());
}

#[test]
fn clean_and_blank_text_reports_expose_full_structural_observations() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("synthetic.log");
    for (content, expected) in [
        (
            "worker | 2026-01-01T00:00:00Z [INFO ] valid\n\n",
            [
                "sample=single_format/1",
                "observed=single_format",
                "=1/1,0/0,0/0,0/0",
                "blocks=1",
                "blank=1",
            ],
        ),
        (
            "\n\n",
            [
                "sample=no_match/0",
                "observed=no_match",
                "=0/0,0/0,0/0,0/0",
                "blocks=0",
                "blank=2",
            ],
        ),
    ] {
        fs::write(&file, content).unwrap();
        for operation in ["info", "errors", "perf"] {
            let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
                .args([
                    "--preset",
                    "base",
                    "--color",
                    "never",
                    operation,
                    file.to_str().unwrap(),
                ])
                .output()
                .unwrap();
            assert!(output.status.success());
            let text = String::from_utf8(output.stdout).unwrap();
            for expected in expected {
                assert!(text.contains(expected), "missing {expected}: {text}");
            }
            for expected in [
                "automatic_sample",
                "headers(classic/rust/syslog/json; sample/consumed)",
                "attached=0 (unverified)",
                "Python=0",
                "capture/semantics unknown",
            ] {
                assert!(text.contains(expected), "missing {expected}: {text}");
            }
        }
    }
}

#[test]
fn bounded_clean_errors_label_the_structural_summary() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("synthetic.log");
    fs::write(
        &file,
        "worker | 2026-01-01T00:00:00Z [ERROR] failure\n  at frame\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .args([
            "--preset",
            "base",
            "--color",
            "never",
            "errors",
            file.to_str().unwrap(),
            "--bounded",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Structure summary:"));
    assert!(text.contains("attached=1 unverified"));
    assert!(text.contains("capture/semantics unknown"));
}
