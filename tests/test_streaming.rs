use log_analyzer::{
    config::{AnalyzerConfig, LogFormat},
    parser::{self, ParseError},
};
use std::io::Write;

fn json_config() -> AnalyzerConfig {
    let mut config = AnalyzerConfig::default();
    config.parser.format = LogFormat::JsonLines;
    config
}
fn line(message: &str) -> String {
    serde_json::json!({"timestamp":"2026-01-01T01:00:00+01:00", "message":message}).to_string()
        + "\n"
}

#[test]
fn consumer_can_stop_before_later_invalid_input_is_read() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(line("first").as_bytes()).unwrap();
    file.write_all(&vec![b' '; 128 * 1024]).unwrap();
    file.write_all(&[0xff]).unwrap();
    let mut calls = 0;
    let error = parser::visit_log_file(file.path(), &json_config(), 1024, |entry| {
        calls += 1;
        assert_eq!(entry.message, "first");
        Err(ParseError::InvalidLogFormat("consumer stopped".into()))
    })
    .unwrap_err();
    assert_eq!(calls, 1);
    assert!(format!("{error:?}").contains("consumer stopped"));
}

#[test]
fn chunked_records_keep_offsets_hash_and_source_locations() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    let input = line(&"é🐾\\\"".repeat(9000)) + &line("second");
    file.write_all(input.as_bytes()).unwrap();
    let mut records = Vec::new();
    let coverage = parser::visit_log_file(file.path(), &json_config(), 256 * 1024, |entry| {
        records.push(entry);
        Ok(())
    })
    .unwrap();
    let old = parser::parse_log_file_report(file.path(), &json_config()).unwrap();
    assert_eq!(
        serde_json::to_value(&coverage).unwrap(),
        serde_json::to_value(&old.coverage).unwrap()
    );
    assert_eq!(records.len(), 2);
    for (index, entry) in records.iter().enumerate() {
        assert_eq!(entry.source_line_number, index + 1);
        assert_eq!(entry.raw_logline, old.entries[index].raw_logline);
        let source_timestamp = entry.source_timestamp.expect("explicit source timestamp");
        assert_eq!(source_timestamp.offset().local_minus_utc(), 3600);
        assert_eq!(source_timestamp.to_rfc3339(), "2026-01-01T01:00:00+01:00");
        assert_eq!(entry.timestamp.timestamp(), source_timestamp.timestamp());
        assert_eq!(entry.source_timestamp, old.entries[index].source_timestamp);
    }
}

#[test]
fn normalized_rows_yield_individually_with_their_physical_address() {
    use log_analyzer::normalize::NormalizationRules;
    use std::collections::BTreeMap;
    let config = AnalyzerConfig {
        normalization: Some(NormalizationRules {
            root_path: "/rows".into(),
            expand_rows: true,
            fields: BTreeMap::from([
                ("timestamp".into(), "/time".into()),
                ("message".into(), "/text".into()),
            ]),
            ..Default::default()
        }),
        ..json_config()
    };
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(
        file,
        "{}",
        serde_json::json!({"rows":[
            {"time":"2026-01-01T00:00:00Z","text":"a"},
            {"time":"2026-01-01T00:00:01Z","text":"b"}
        ]})
    )
    .unwrap();
    let mut rows = Vec::new();
    let coverage = parser::visit_log_file(file.path(), &config, 1024, |entry| {
        rows.push((entry.source_line_number, entry.source_row_path.clone()));
        Ok(())
    })
    .unwrap();
    assert_eq!(coverage.parsed_entries, 2);
    assert_eq!(
        rows,
        vec![(1, Some("/rows/0".into())), (1, Some("/rows/1".into()))]
    );
    let mut calls = 0;
    assert!(
        parser::visit_log_file(file.path(), &config, 1024, |_| {
            calls += 1;
            Err(ParseError::InvalidLogFormat("stop row expansion".into()))
        })
        .is_err()
    );
    assert_eq!(calls, 1);
}

#[test]
fn oversized_physical_and_multiline_records_stop_before_unbounded_growth() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(&vec![b'x'; 128 * 1024]).unwrap();
    assert!(
        format!(
            "{:?}",
            parser::visit_log_file(file.path(), &json_config(), 1024, |_| Ok(())).unwrap_err()
        )
        .contains("Physical record")
    );
    let mut native = tempfile::NamedTempFile::new().unwrap();
    writeln!(native, "worker | 2026-01-01T00:00:00Z [INFO ] first").unwrap();
    for _ in 0..128 {
        writeln!(native, "    continuation").unwrap();
    }
    assert!(
        format!(
            "{:?}",
            parser::visit_log_file(native.path(), &AnalyzerConfig::default(), 128, |_| Ok(()))
                .unwrap_err()
        )
        .contains("Multiline record")
    );
}

#[test]
fn auto_detection_preserves_blank_continuations_and_bounded_blank_gaps() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(b"worker | 2026-01-01T00:00:00Z [INFO ] first\n\n\nworker | 2026-01-01T00:00:01Z [INFO ] second\n")
        .unwrap();
    let mut records = Vec::new();
    parser::visit_log_file(file.path(), &AnalyzerConfig::default(), 1024, |entry| {
        records.push(entry);
        Ok(())
    })
    .unwrap();
    assert_eq!(records.len(), 2);
    assert!(records[0].raw_logline.ends_with("\n\n"));
    assert_eq!(records[1].source_line_number, 4);
    let mut blank = tempfile::NamedTempFile::new().unwrap();
    blank.write_all(line("first").as_bytes()).unwrap();
    blank.write_all(&vec![b'\n'; 100_000]).unwrap();
    blank.write_all(line("last").as_bytes()).unwrap();
    let coverage =
        parser::visit_log_file(blank.path(), &AnalyzerConfig::default(), 1024, |_| Ok(())).unwrap();
    assert_eq!(coverage.parsed_entries, 2);
    assert_eq!(coverage.structural_diagnostics.blank_lines, 100_000);
}

#[test]
fn streaming_normalization_diagnostics_have_an_explicit_bound() {
    use log_analyzer::normalize::NormalizationRules;
    use std::collections::BTreeMap;
    let config = AnalyzerConfig {
        normalization: Some(NormalizationRules {
            fields: BTreeMap::from([("timestamp".into(), "/missing".into())]),
            ..Default::default()
        }),
        ..json_config()
    };
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for _ in 0..21 {
        writeln!(file, "{{}}").unwrap();
    }
    let error = parser::visit_log_file(file.path(), &config, 1024, |_| {
        panic!("invalid row emitted")
    })
    .unwrap_err();
    assert!(format!("{error:?}").contains("diagnostic limit"));
}

#[test]
fn investigation_streams_capture_strings_and_bytes_into_compatible_artifacts() {
    use std::{fs, process::Command};
    let directory = tempfile::tempdir().unwrap();
    let unicode = line(&"é🐾\\\"\t".repeat(10000)).into_bytes();
    let mut invalid = unicode.clone();
    invalid.push(0xff);
    for (index, input) in [unicode, invalid].iter().enumerate() {
        let source = directory.path().join(format!("input-{index}.jsonl"));
        let artifact = directory.path().join(format!("artifact-{index}.json"));
        fs::write(&source, input).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_log-analyzer"));
        for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("LOG_ANALYZER_")) {
            command.env_remove(key);
        }
        let output = command
            .args(["--json", "--profile", "base", "investigate"])
            .arg(&source)
            .arg("--artifact")
            .arg(&artifact)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = serde_json::from_slice(&output.stdout).unwrap();
        let stored = fs::read(&artifact).unwrap();
        log_analyzer::investigation::validate_relations(&report, Some(&stored)).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&stored).unwrap();
        let capture = &value["captured_inputs"][0];
        if index == 0 {
            assert_eq!(capture["data"].as_str().unwrap().as_bytes(), input);
        } else {
            let bytes: Vec<u8> = serde_json::from_value(capture["data"].clone()).unwrap();
            assert_eq!(&bytes, input);
        }
    }
}

#[test]
fn filtered_investigation_finishes_without_retaining_discarded_record_state() {
    use std::{fs, process::Command};
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.jsonl");
    let artifact = directory.path().join("artifact.json");
    fs::write(&source, line("excluded").repeat(1000)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .args([
            "--json",
            "--profile",
            "base",
            "--filter",
            "t:absent",
            "investigate",
        ])
        .arg(&source)
        .arg("--artifact")
        .arg(&artifact)
        .args(["--processing-max-memory-bytes", "1048576"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["processing"]["status"], "complete", "{report}");
    let input = &report["report_metadata"]["evidence"]["inputs"][0];
    assert_eq!(input["coverage"]["parsed_entries"], 1000);
    assert_eq!(input["selected_entries"], 0);
    log_analyzer::investigation::validate_relations(&report, Some(&fs::read(artifact).unwrap()))
        .unwrap();
}
