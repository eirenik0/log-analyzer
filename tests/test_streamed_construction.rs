use serde_json::{Value, json};
use std::{fs, io::Write, process::Command};

#[test]
fn selected_cli_construction_matches_forced_spill_with_many_pending_boundaries() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("selected.jsonl");
    let mut file = fs::File::create(&source).unwrap();
    let count = 256;
    let start = chrono::DateTime::parse_from_rfc3339("2026-01-01T02:00:00+02:00").unwrap();
    for phase in ["start", "end"] {
        for ordinal in 0..count {
            let time = if phase == "start" {
                start
            } else {
                start + chrono::Duration::seconds(ordinal + 1)
            };
            writeln!(file, "{}", json!({"ts":time.to_rfc3339(),"phase":phase,"operation":if ordinal % 2 == 0 {"fetch"} else {"other"},"id":format!("job-{ordinal:04}"),"session":"selected-run","message":"synthetic boundary é","level":"ERROR","outcome":"success"})).unwrap();
        }
    }
    drop(file);
    let run = |index_bytes: &str| {
        let artifact = temp.path().join(format!("artifact-{index_bytes}.json"));
        let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args([
                "--json",
                "--config",
                "examples/investigations/profile.toml",
                "investigate",
            ])
            .arg(&source)
            .args(["--artifact"])
            .arg(&artifact)
            .args([
                "--complete-output",
                "--select",
                r#"{"name":"fetch"}"#,
                "--correlation-max-index-bytes",
                index_bytes,
                "--processing-max-memory-bytes",
                "67108864",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["processing"]["status"], "complete");
        let bytes = fs::read(artifact).unwrap();
        let retained: Value = serde_json::from_slice(&bytes).unwrap();
        for field in [
            "report_metadata",
            "processing",
            "scopes",
            "populations",
            "assessments",
        ] {
            assert_eq!(report[field], retained[field], "report/artifact {field}");
        }
        log_analyzer::investigation::validate_relations(&report, Some(&bytes)).unwrap();
        assert_eq!(
            retained["records"].as_array().unwrap().len(),
            count as usize
        );
        assert_eq!(
            retained["report_metadata"]["evidence"]["inputs"][0]["selected_entries"],
            count
        );
        let findings = retained["findings"].as_array().unwrap();
        assert_eq!(
            findings
                .iter()
                .filter(|finding| finding["kind"] == "measurement")
                .count(),
            count as usize / 2
        );
        let expected: Vec<i64> = (0..count)
            .filter(|ordinal| ordinal % 2 == 0)
            .map(|ordinal| (ordinal + 1) * 1000)
            .collect();
        for (statistic, value) in [
            ("minimum", expected[0]),
            ("maximum", *expected.last().unwrap()),
            ("p50", expected[expected.len() * 50 / 100]),
            ("p95", expected[expected.len() * 95 / 100]),
            ("p99", expected[expected.len() * 99 / 100]),
        ] {
            let finding = findings
                .iter()
                .find(|finding| finding["details"]["statistic"] == statistic)
                .unwrap();
            assert_eq!(finding["details"]["value"], value);
            assert_eq!(finding["details"]["sample_count"], expected.len());
            assert_eq!(
                finding["details"]["measurement_ids"]
                    .as_array()
                    .unwrap()
                    .len(),
                expected.len()
            );
        }
        retained
    };
    let resident = run("1048576");
    let spilled = run("0");
    for collection in [
        "records",
        "findings",
        "populations",
        "memberships",
        "sequences",
        "scopes",
        "assessments",
    ] {
        assert_eq!(resident[collection], spilled[collection], "{collection}");
    }
}
