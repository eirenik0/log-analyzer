use crate::LogEntry;
use crate::comparator::entities::LogFilter;
use colored::Colorize;
use similar::{ChangeTag, TextDiff};
use std::collections::HashMap;

/// Groups logs by their key
pub fn group_logs_by_key<'a>(
    logs: &'a [LogEntry],
    filter: &LogFilter,
) -> HashMap<String, Vec<&'a LogEntry>> {
    let mut grouped_logs: HashMap<_, Vec<&LogEntry>> = HashMap::new();

    for log in logs {
        if filter.matches(log) {
            let key = get_log_key(log);
            grouped_logs.entry(key).or_default().push(log);
        }
    }

    grouped_logs
}

/// Generates a unique key for a log entry
pub fn get_log_key(log: &LogEntry) -> String {
    format!("{}|{}|{}", log.component, log.level, log.log_key())
}

/// Computes a colored text diff between two strings
pub fn compute_text_diff(text1: &str, text2: &str) -> String {
    let diff = TextDiff::from_lines(text1, text2);
    let mut result = String::new();

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => result.push_str(&format!("{}", change.to_string().red())),
            ChangeTag::Insert => result.push_str(&format!("{}", change.to_string().green())),
            ChangeTag::Equal => continue,
        }
    }

    result
}
