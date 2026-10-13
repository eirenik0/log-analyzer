use log_analyzer::{
    comparator::LogFilter,
    config::{self, AnalyzerConfig},
    correlation_stream::{CorrelationStream, Decision, Limits},
    parser::{self, LogEntry},
    perf_analyzer,
};
use serde_json::{Value, json};
use std::io::Write;

fn config() -> AnalyzerConfig {
    config::load_config_from_path(std::path::Path::new("examples/investigations/profile.toml"))
        .unwrap()
}
fn records(rows: &[Value]) -> Vec<LogEntry> {
    let mut source = tempfile::NamedTempFile::new().unwrap();
    for row in rows {
        writeln!(source, "{row}").unwrap();
    }
    let mut entries = parser::parse_log_file_report(source.path(), &config())
        .unwrap()
        .entries;
    for entry in &mut entries {
        entry.source_input_ordinal = Some(0);
    }
    entries
}
fn event(id: &str, phase: &str, time: &str) -> Value {
    json!({"ts":time,"phase":phase,"operation":"fetch","id":id,"session":"run-a","message":"synthetic boundary","outcome":"success"})
}
fn stream(entries: &[LogEntry], memory: usize) -> CorrelationStream {
    let mut stream = CorrelationStream::new(
        config(),
        Limits {
            index_bytes: memory,
            disk_bytes: 16 * 1024 * 1024,
            record_bytes: 65536,
        },
    )
    .unwrap();
    for entry in entries {
        stream.push(entry.clone()).unwrap();
    }
    stream
}
fn decisions(stream: CorrelationStream) -> (Vec<Value>, Vec<Value>) {
    let mut pairs = Vec::new();
    let mut unmatched = Vec::new();
    stream
        .finish(
            || true,
            |decision| {
                assert!(decision.records.len() <= 2);
                if let Some(pair) = decision.operation {
                    pairs.push(serde_json::to_value(pair)?);
                }
                if let Some(event) = decision.unmatched {
                    unmatched.push(serde_json::to_value(event)?);
                }
                Ok(())
            },
        )
        .unwrap();
    pairs.sort_by_key(Value::to_string);
    unmatched.sort_by_key(Value::to_string);
    (pairs, unmatched)
}
fn assert_parity(entries: &[LogEntry]) {
    let result =
        perf_analyzer::analyze_performance_with_config(entries, &LogFilter::new(), None, &config());
    let mut pairs: Vec<_> = result
        .operations
        .into_iter()
        .map(|v| serde_json::to_value(v).unwrap())
        .collect();
    let mut unmatched: Vec<_> = result
        .unmatched_events
        .into_iter()
        .map(|v| serde_json::to_value(v).unwrap())
        .collect();
    pairs.sort_by_key(Value::to_string);
    unmatched.sort_by_key(Value::to_string);
    for memory in [0, 1024 * 1024] {
        assert_eq!(
            decisions(stream(entries, memory)),
            (pairs.clone(), unmatched.clone())
        );
    }
}
#[test]
fn late_completion_reused_ids_offsets_and_whole_key_ambiguity_match() {
    let entries = records(&[
        event("reused", "start", "2026-01-01T02:00:00+02:00"),
        event("reused", "end", "2026-01-01T00:00:01Z"),
        event("open", "start", "2026-01-01T00:00:02Z"),
        event("reused", "start", "2026-01-01T00:00:03Z"),
        event("reused", "start", "2026-01-01T00:00:04Z"),
        event("reused", "end", "2026-01-01T00:00:05Z"),
        event("unmatched", "end", "2026-01-01T00:00:06Z"),
        event("open", "end", "2026-01-02T00:00:02Z"),
    ]);
    assert_parity(&entries);
    let (pairs, unmatched) = decisions(stream(&entries, 0));
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0]["duration_ms"], 86_400_000);
    assert_eq!(
        unmatched
            .iter()
            .filter(|e| e["reason"] == "overlapping_starts")
            .count(),
        5
    );
}
#[test]
fn normalized_rows_keep_emission_order_and_inferred_year_rejects_whole_key() {
    let mut entries = records(&[
        event("a", "start", "2026-01-01T00:00:00Z"),
        event("a", "end", "2026-01-01T00:00:00Z"),
        event("b", "start", "2026-01-01T00:00:01Z"),
        event("b", "end", "2026-01-01T00:00:02Z"),
    ]);
    for (i, e) in entries[..2].iter_mut().enumerate() {
        e.source_line_number = 1;
        e.source_row_path = Some(format!("/rows/{i}"));
    }
    entries[2].timestamp_year_inferred = true;
    assert_parity(&entries);
}
#[test]
fn independent_inputs_and_scopes_never_pair() {
    let mut entries = records(&[
        event("a", "start", "2026-01-01T00:00:00Z"),
        event("a", "end", "2026-01-01T00:00:01Z"),
    ]);
    entries[1].source_input_ordinal = Some(1);
    let (pairs, unmatched) = decisions(stream(&entries, 0));
    assert!(pairs.is_empty());
    assert_eq!(unmatched.len(), 2);
    let mut rows = vec![
        event("a", "start", "2026-01-01T00:00:00Z"),
        event("a", "end", "2026-01-01T00:00:01Z"),
    ];
    rows[1]["session"] = json!("run-b");
    assert_parity(&records(&rows));
}
#[test]
fn source_replay_is_bounded_and_cancelled_decisions_are_not_reported_as_success() {
    let entries = records(&[
        event("a", "start", "2026-01-01T00:00:00Z"),
        event("a", "end", "2026-01-01T00:00:01Z"),
    ]);
    let stream = stream(&entries, 0);
    assert!(stream.spilled());
    assert_eq!(stream.len(), 2);
    let mut lines = Vec::new();
    stream
        .visit_records(|e| {
            lines.push(e.source_line_number);
            Ok(())
        })
        .unwrap();
    assert_eq!(lines, [1, 2]);
    let mut checkpoints = 0;
    assert!(
        stream
            .finish(
                || {
                    checkpoints += 1;
                    false
                },
                |_| panic!("cancelled pass emitted a decision")
            )
            .is_err()
    );
    assert_eq!(checkpoints, 1);
    assert!(
        self::stream(&entries, 0)
            .finish(|| true, |_: Decision| Err("consumer failed".into()))
            .is_err()
    );
}
#[test]
fn duplicate_addresses_and_io_limits_poison_the_stream() {
    let entries = records(&[event("a", "start", "2026-01-01T00:00:00Z")]);
    let mut s = stream(&entries, 0);
    assert!(s.push(entries[0].clone()).is_err());
    assert!(s.finish(|| true, |_| Ok(())).is_err());
    let mut s = CorrelationStream::new(
        config(),
        Limits {
            index_bytes: 1024,
            disk_bytes: 128,
            record_bytes: 64,
        },
    )
    .unwrap();
    assert!(s.push(entries[0].clone()).is_err());
    assert!(s.finish(|| true, |_| Ok(())).is_err());
}
