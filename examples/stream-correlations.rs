//! Select every source record and retain arbitrarily late lifecycle boundaries.
//! Results are counted, not collected, so this measures correlation-state RSS.
use log_analyzer::{
    config,
    correlation_stream::{CorrelationStream, Limits},
    parser::{ParseError, visit_log_file},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: stream-correlations INPUT.jsonl")?;
    let profile = config::load_config_from_path(std::path::Path::new(
        "examples/investigations/profile.toml",
    ))?;
    let mut stream = CorrelationStream::new(
        profile.clone(),
        Limits {
            index_bytes: 1024 * 1024,
            disk_bytes: 4 * 1024 * 1024 * 1024,
            record_bytes: 256 * 1024,
        },
    )?;
    let coverage = visit_log_file(path, &profile, 256 * 1024, |mut entry| {
        entry.source_input_ordinal = Some(0);
        stream
            .push(entry)
            .map_err(|error| ParseError::InvalidLogFormat(error.to_string()))
    })
    .map_err(|e| format!("{e:?}"))?;
    let selected = stream.len();
    let spilled = stream.spilled();
    let mut records = 0;
    let mut pairs = 0;
    let mut unmatched = 0;
    let mut elapsed_sum_ms = 0i64;
    stream.finish(
        || true,
        |decision| {
            records += decision.records.len();
            if let Some(pair) = decision.operation {
                assert_eq!(pair.duration_ms, 86_400_000);
                assert_eq!(pair.scope, ["run-a"]);
                assert!(pair.start_source.line < pair.end_source.line);
                pairs += 1;
                elapsed_sum_ms += pair.duration_ms;
            }
            unmatched += usize::from(decision.unmatched.is_some());
            Ok(())
        },
    )?;
    println!(
        "{}",
        serde_json::json!({"selected_records":selected,"emitted_records":records,"pairs":pairs,"unmatched":unmatched,"elapsed_sum_ms":elapsed_sum_ms,"spilled":spilled,"coverage":coverage})
    );
    Ok(())
}
