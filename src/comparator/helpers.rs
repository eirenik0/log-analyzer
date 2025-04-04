use crate::LogEntry;
use crate::comparator::entities::{
    ComparisonError, ComparisonOptions, ComparisonResults, LogFilter,
};
use colored::Colorize;
use similar::{ChangeTag, TextDiff};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

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
    format!("{}|{}|{} ", log.component, log.level, log.log_key())
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

/// Writes comparison results to a file
pub fn write_results_to_file(
    results: &ComparisonResults,
    options: &ComparisonOptions,
    path: &Path,
) -> Result<(), ComparisonError> {
    let mut file = File::create(path)?;

    writeln!(file, "{}", results.summary())?;

    if !options.diff_only {
        for key in &results.unique_to_log1 {
            writeln!(file, "\nLog type only in file 1: {}", key)?;
        }

        for key in &results.unique_to_log2 {
            writeln!(file, "\nLog type only in file 2: {}", key)?;
        }
    }

    for comparison in &results.shared_comparisons {
        let json_key = serde_json::to_string_pretty(&comparison.json_differences[0].path)?;
        writeln!(
            file,
            "\n{} - Compare log line in file1 N`{}` with file2 N`{}` \n\twith json key: `{}`",
            comparison.key, comparison.log1_index, comparison.log2_index, json_key
        )?;

        if options.show_full_json {
            if !comparison.json_differences.is_empty() {
                let val1 = serde_json::to_string_pretty(&comparison.json_differences[0].value1)?;
                let val2 = serde_json::to_string_pretty(&comparison.json_differences[0].value2)?;
                writeln!(file, "\t- file1: {val1}")?;
                writeln!(file, "\t- file2: {val2}")?;
            }
        } else {
            for diff in &comparison.json_differences {
                writeln!(
                    file,
                    "{}: {:?} => {:?}",
                    diff.path, diff.value1, diff.value2
                )?;
            }
        }

        if let Some(text_diff) = &comparison.text_difference {
            writeln!(file, "\nText differences:")?;
            writeln!(file, "{}", text_diff)?;
        }
    }

    Ok(())
}
