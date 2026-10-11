use serde_json::Value;
use std::{fs, process::Command};

#[test]
fn bounded_budget_processes_large_multiline_text_without_losing_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("synthetic.log");
    let artifact = temp.path().join("evidence.json");
    let block = format!(
        "worker | 2026-01-01T00:00:00Z [INFO ] observed state\n  details: {}\n",
        "λ".repeat(450)
    );
    fs::write(&source, block.repeat(6000)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .args([
            "--json",
            "--preset",
            "eyes",
            "investigate",
            "--processing-max-memory-bytes",
            "536870912",
        ])
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
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["processing"]["limits"]["memory_bytes"], 536870912u64);
    assert_eq!(report["processing"]["status"], "complete");
    assert!(report["processing"]["stop"].is_null());
    assert_eq!(report["processing"]["usage"]["records"], 6000);
    assert_eq!(report["artifact"]["status"], "complete");
    let bytes = fs::read(&artifact).unwrap();
    log_analyzer::investigation::validate_relations(&report, Some(&bytes)).unwrap();
    let retained: Value = serde_json::from_slice(&bytes).unwrap();
    let records = retained["records"].as_array().unwrap();
    assert_eq!(records.len(), 6000);
    assert_eq!(
        records.last().unwrap()["raw_text"],
        block.trim_end_matches('\n')
    );
}
