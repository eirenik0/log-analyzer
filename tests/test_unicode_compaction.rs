use log_analyzer::{llm_processor::compact_json_value, perf_analyzer::truncate_string};
use serde_json::{Value, json};
use std::{fs, process::Command};
use tempfile::tempdir;

#[test]
fn values_keep_whole_emoji_combining_sequences_and_flags() {
    for (text, expected) in [
        ("🙂".repeat(26), format!("{}...", "🙂".repeat(24))),
        (
            format!("{}e\u{301}tail", "a".repeat(96)),
            format!("{}...", "a".repeat(96)),
        ),
        (
            format!("{}👩‍💻tail", "a".repeat(92)),
            format!("{}...", "a".repeat(92)),
        ),
        (
            format!("{}🇸🇰tail", "a".repeat(93)),
            format!("{}...", "a".repeat(93)),
        ),
    ] {
        let value = compact_json_value(&json!(text), 3, 0);
        assert_eq!(value, expected);
        assert!(value.as_str().unwrap().len() <= 100);
    }
    assert_eq!(truncate_string("🙂tail", 1), "");
    assert_eq!(truncate_string("e\u{301}tail", 2), "");
    assert_eq!(truncate_string("e\u{301}tail", 3), "...");
}

#[test]
fn shortened_keys_preserve_original_short_names_and_every_retained_value() {
    let prefix = "x".repeat(27);
    let short = format!("{prefix}...");
    let value = json!({short.clone():"original",format!("{prefix}first-long-name"):"first",format!("{prefix}second-long-name"):"second"});
    let compact = compact_json_value(&value, 3, 0);
    let map = compact.as_object().unwrap();
    assert_eq!(map.len(), 3);
    assert_eq!(map[&short], "original");
    let values: std::collections::HashSet<_> = map.values().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(
        values,
        std::collections::HashSet::from(["original", "first", "second"])
    );
    assert!(map.keys().all(|key| key.len() <= 30));
    let prefix = "a".repeat(26);
    let compact = compact_json_value(
        &json!({format!("{prefix}e\u{301}first"):1,format!("{prefix}e\u{301}second"):2}),
        3,
        0,
    );
    let keys: Vec<_> = compact.as_object().unwrap().keys().collect();
    assert_eq!(keys.len(), 2);
    assert!(
        keys.iter()
            .all(|key| key.len() <= 30 && !key.contains("e..."))
    );
    assert_eq!(
        compact
            .as_object()
            .unwrap()
            .values()
            .cloned()
            .collect::<std::collections::HashSet<_>>(),
        std::collections::HashSet::from([json!(1), json!(2)])
    );
}

#[test]
fn omission_metadata_does_not_overwrite_real_fields() {
    let mut map = serde_json::Map::new();
    map.insert("_truncated_fields".into(), json!(99));
    map.insert("_truncated_fields~2".into(), json!(98));
    for i in 0..25 {
        map.insert(format!("field{i:02}"), json!(i));
    }
    let compact = compact_json_value(&Value::Object(map), 3, 0);
    assert_eq!(compact["_truncated_fields"], 99);
    assert_eq!(compact["_truncated_fields~2"], 98);
    assert_eq!(compact["_truncated_fields~3"], 7);
    assert_eq!(compact.as_object().unwrap().len(), 21);
}

#[test]
fn process_cli_handles_the_unicode_reproduction_and_combining_message_boundary() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("unicode.jsonl");
    let message = format!("{}e\u{301}tail", "a".repeat(196));
    let row = json!({"timestamp":"2026-01-01T00:00:00Z","message":message,"payload":{"description":"🙂".repeat(26)}});
    fs::write(&file, format!("{row}\n")).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_log-analyzer"))
        .args(["--preset", "eyes", "process", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        report["logs"][0]["data"]["description"],
        format!("{}...", "🙂".repeat(24))
    );
    assert_eq!(report["logs"][0]["msg"], format!("{}...", "a".repeat(196)));
}
