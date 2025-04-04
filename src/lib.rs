pub mod cli;
pub mod comparator;
pub mod parser;

pub use cli::{Commands, cli_parse};
pub use comparator::{
    compare_json, compare_logs, display_log_info, extract_all_json_objects,
    is_only_json_formatting_difference,
};
pub use parser::{LogEntry, parse_log_entry, parse_log_file};
