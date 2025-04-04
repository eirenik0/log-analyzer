pub mod cli;
pub mod comparator;
pub mod parser;

pub use cli::{Commands, cli_parse};
pub use comparator::{compare_json, compare_logs, display_log_info};
pub use parser::{LogEntry, LogEntryKind, parse_log_entry, parse_log_file};
