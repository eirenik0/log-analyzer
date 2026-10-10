use super::*;
use crate::{config, parser};

fn parse(text: &str) -> parser::LogEntry {
    parser::parse_log_entry(text, 1).unwrap()
}

#[test]
fn text_settlement_releases_only_unused_source_amplification() {
    let text = format!("worker | 2026-01-01T00:00:00Z [INFO ] {}", "λ".repeat(1024));
    let entry = parse(&text);
    let mut budget = test_budget();
    assert!(budget.reserve("capture", 1234));
    assert!(budget.record(text.len(), 0, false));
    budget.settle_text_record(text.len(), &entry);
    assert_eq!(
        budget.memory_bytes,
        1234 + 16 * 1024 + text.len() as u64 * 32
    );
    assert!(budget.retain_classification(&entry.classification));
    assert!(budget.memory_bytes > 1234 + 16 * 1024 + text.len() as u64 * 32);
}

#[test]
fn dense_payload_keeps_worst_case_reservation() {
    let payload = serde_json::json!({"items": vec![0; 1024]});
    let text = format!("worker | 2026-01-01T00:00:00Z [INFO ] state {payload}");
    let entry = parse(&text);
    assert!(entry.payload().is_some());
    let mut budget = test_budget();
    assert!(budget.record(text.len(), 0, false));
    let reserved = budget.memory_bytes;
    budget.settle_text_record(text.len(), &entry);
    assert_eq!(budget.memory_bytes, reserved);
}

#[test]
fn insufficient_parse_scratch_still_stops_before_retaining_text() {
    let text = format!(
        "worker | 2026-01-01T00:00:00Z [INFO ] {}\n",
        "x".repeat(2048)
    );
    let mut budget = test_budget();
    budget.limits.memory_bytes = 128 * 1024;
    let result = parser::parse_capture(
        std::path::Path::new("synthetic.log"),
        text.as_bytes(),
        true,
        config::default_config(),
        &mut budget,
    )
    .unwrap();
    assert!(result.entries.is_empty());
    assert_eq!(budget.stop.as_ref().unwrap()["stage"], "parse");
    assert_eq!(budget.stop.as_ref().unwrap()["reason"], "memory_limit");
}

#[test]
fn json_envelopes_keep_the_original_source_charge() {
    let text = serde_json::json!({
        "ts": "2026-01-01T00:00:00Z", "message": "x".repeat(2048)
    })
    .to_string();
    let mut budget = test_budget();
    let result = parser::parse_capture(
        std::path::Path::new("synthetic.jsonl"),
        text.as_bytes(),
        true,
        config::default_config(),
        &mut budget,
    )
    .unwrap();
    assert_eq!(result.entries.len(), 1);
    let classification_bytes = serde_json::to_vec(&result.entries[0].classification)
        .unwrap()
        .len();
    assert_eq!(
        budget.memory_bytes,
        16 * 1024 + text.len() as u64 * 128 + classification_bytes as u64 * 32
    );
}
