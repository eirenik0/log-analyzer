//! A consuming visitor: no parsed records are retained by this program.
use log_analyzer::{
    config::{AnalyzerConfig, LogFormat},
    parser::visit_log_file,
};
fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: stream-records INPUT.jsonl");
    let mut config = AnalyzerConfig::default();
    config.parser.format = LogFormat::JsonLines;
    let mut count = 0u64;
    let coverage = visit_log_file(path, &config, 256 * 1024, |_| {
        count += 1;
        Ok(())
    })
    .expect("stream records");
    println!(
        "{}",
        serde_json::json!({"records":count,"coverage":coverage})
    );
}
