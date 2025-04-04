mod entities;
mod helpers;

pub use entities::*;
pub use helpers::*;

use crate::parser::{LogEntry, LogEntryKind};
use colored::Colorize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;

/// Compares two sets of logs with the provided filter and options
pub fn compare_logs(
    logs1: &[LogEntry],
    logs2: &[LogEntry],
    filter: &LogFilter,
    options: &ComparisonOptions,
) -> Result<ComparisonResults, ComparisonError> {
    // Group logs by component and event type
    let grouped_logs1 = group_logs_by_key(logs1, filter);
    let grouped_logs2 = group_logs_by_key(logs2, filter);

    // Find unique and shared keys
    let mut unique_to_log1 = Vec::new();
    let mut unique_to_log2 = Vec::new();
    let mut shared_comparisons = Vec::new();

    // Find keys unique to logs1
    for key in grouped_logs1.keys() {
        if !grouped_logs2.contains_key(key) {
            unique_to_log1.push(key.clone());
        }
    }

    // Find keys unique to logs2
    for key in grouped_logs2.keys() {
        if !grouped_logs1.contains_key(key) {
            unique_to_log2.push(key.clone());
        }
    }

    // Process the shared keys
    let mut keys: Vec<String> = grouped_logs1
        .keys()
        .filter(|k| grouped_logs2.contains_key(*k))
        .cloned()
        .collect();
    keys.sort();

    for key in keys {
        let entries1 = grouped_logs1.get(&key).unwrap();
        let entries2 = grouped_logs2.get(&key).unwrap();

        // Compare all entries of the same type
        for (idx1, log1) in entries1.iter().enumerate() {
            for (idx2, log2) in entries2.iter().enumerate() {
                if let (Some(payload1), Some(payload2)) = (&log1.payload(), &log2.payload()) {
                    let json_diffs = compare_json(payload1, payload2);

                    // Only process if there are differences or if we're not in diff_only mode
                    if !json_diffs.is_empty() || !options.diff_only {
                        let text_diff = if !json_diffs.is_empty() && log1.message != log2.message {
                            Some(compute_text_diff(&log1.message, &log2.message))
                        } else {
                            None
                        };

                        shared_comparisons.push(LogComparison {
                            key: key.clone(),
                            log1_index: idx1,
                            log2_index: idx2,
                            json_differences: json_diffs
                                .into_iter()
                                .map(|(path, val1, val2)| JsonDifference {
                                    path,
                                    value1: val1,
                                    value2: val2,
                                })
                                .collect(),
                            text_difference: text_diff,
                        });
                    }
                }
            }
        }
    }

    // Create and return results
    let results = ComparisonResults {
        unique_to_log1,
        unique_to_log2,
        shared_comparisons,
    };

    // Write to output file if specified
    if let Some(path) = &options.output_path {
        write_results_to_file(&results, options, Path::new(path))?;
    }

    Ok(results)
}

/// Formats and displays the comparison results to the console
pub fn display_comparison_results(results: &ComparisonResults, options: &ComparisonOptions) {
    println!(
        "Log file 1 has {} unique log types",
        results.unique_to_log1.len()
    );
    println!(
        "Log file 2 has {} unique log types",
        results.unique_to_log2.len()
    );
    println!("Shared log types: {}", results.shared_comparisons.len());

    // Display unique keys
    if !options.diff_only {
        for key in &results.unique_to_log1 {
            println!("\nLog type only in file 1: {}", key.cyan());
        }

        for key in &results.unique_to_log2 {
            println!("\nLog type only in file 2: {}", key.magenta());
        }
    }

    // Display shared key comparisons
    for comparison in &results.shared_comparisons {
        println!(
            "\n{} - Compare log {} #{} with log {} #{}",
            comparison.key.yellow(),
            "file1".cyan(),
            comparison.log1_index,
            "file2".magenta(),
            comparison.log2_index
        );

        if options.show_full_json {
            _ = display_full_json_comparison(&comparison);
        } else {
            display_json_differences(&comparison);
        }

        if let Some(text_diff) = &comparison.text_difference {
            println!("\nText differences:");
            println!("{}", text_diff);
        }
    }
}

/// Displays all statistics about the logs
pub fn display_log_summary(logs: &[LogEntry]) {
    // Collect unique components, event types, commands, etc.
    let components: HashSet<_> = logs.iter().map(|log| &log.component).collect();
    let levels: HashSet<_> = logs.iter().map(|log| &log.level).collect();

    let mut event_types = HashSet::new();
    let mut commands = HashSet::new();
    let mut requests = HashSet::new();

    for log in logs {
        match &log.kind {
            LogEntryKind::Event { event_type, .. } => {
                event_types.insert(event_type);
            }
            LogEntryKind::Command { command, .. } => {
                commands.insert(command);
            }
            LogEntryKind::Request { request, .. } => {
                requests.insert(request);
            }
            LogEntryKind::Generic { .. } => {}
        }
    }

    // Display statistics
    println!("Log Components:");
    for component in components {
        println!("  - {}", component);
    }

    println!("\nLog Levels:");
    for level in levels {
        println!("  - {}", level);
    }

    println!("\nEvent Types:");
    for event_type in event_types {
        println!("  - {}", event_type);
    }

    println!("\nCommands:");
    for command in commands {
        println!("  - {}", command);
    }

    println!("\nRequests:");
    for req in requests {
        println!("  - {}", req);
    }

    println!("\nTotal log entries: {}", logs.len());
}

/// Compares two JSON values and returns a vector of differences.
///
/// Each difference is represented as a tuple with the JSON path and the differing values.
/// This function compares values semantically, ignoring the order of object properties.
pub fn compare_json(json1: &Value, json2: &Value) -> Vec<(String, Value, Value)> {
    let mut differences = Vec::new();
    compare_json_recursive(json1, json2, "".to_string(), &mut differences);
    differences
}

fn compare_json_recursive(
    json1: &Value,
    json2: &Value,
    path: String,
    differences: &mut Vec<(String, Value, Value)>,
) {
    match (json1, json2) {
        (Value::Object(obj1), Value::Object(obj2)) => {
            compare_objects(obj1, obj2, path, differences);
        }
        (Value::Array(arr1), Value::Array(arr2)) => {
            compare_arrays(arr1, arr2, path, differences);
        }
        (val1, val2) => {
            if val1 != val2 {
                differences.push((path, val1.clone(), val2.clone()));
            }
        }
    }
}

/// Compares two JSON objects
fn compare_objects(
    obj1: &serde_json::Map<String, Value>,
    obj2: &serde_json::Map<String, Value>,
    path: String,
    differences: &mut Vec<(String, Value, Value)>,
) {
    // Check keys that exist in both objects.
    for (key, val1) in obj1 {
        let current_path = if path.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", path, key)
        };

        match obj2.get(key) {
            Some(val2) => compare_json_recursive(val1, val2, current_path, differences),
            None => differences.push((current_path, val1.clone(), json!(null))),
        }
    }

    // Check keys that only exist in obj2.
    for (key, val2) in obj2 {
        if !obj1.contains_key(key) {
            let current_path = if path.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", path, key)
            };
            differences.push((current_path, json!(null), val2.clone()));
        }
    }
}

/// Compares two JSON arrays
fn compare_arrays(
    arr1: &[Value],
    arr2: &[Value],
    path: String,
    differences: &mut Vec<(String, Value, Value)>,
) {
    // Special handling for arrays containing objects
    if arr1.len() == arr2.len()
        && arr1.iter().all(|v| v.is_object())
        && arr2.iter().all(|v| v.is_object())
    {
        compare_object_arrays(arr1, arr2, path, differences);
        return;
    }

    // Standard array comparison for non-object arrays or different length arrays
    let max_len = arr1.len().max(arr2.len());
    for i in 0..max_len {
        let current_path = format!("{}[{}]", path, i);
        if i < arr1.len() && i < arr2.len() {
            compare_json_recursive(&arr1[i], &arr2[i], current_path, differences);
        } else if i < arr1.len() {
            differences.push((current_path.clone(), arr1[i].clone(), json!(null)));
        } else {
            differences.push((current_path.clone(), json!(null), arr2[i].clone()));
        }
    }
}

/// Compares arrays of objects using best-match strategy
fn compare_object_arrays(
    arr1: &[Value],
    arr2: &[Value],
    path: String,
    differences: &mut Vec<(String, Value, Value)>,
) {
    let mut matched_indices = vec![false; arr2.len()];

    for (i, obj1) in arr1.iter().enumerate() {
        let mut best_match_idx = None;
        let mut fewest_differences = usize::MAX;

        // Find the best matching object in arr2
        for (j, obj2) in arr2.iter().enumerate() {
            if !matched_indices[j] {
                let mut temp_differences = Vec::new();
                compare_json_recursive(obj1, obj2, "temp".to_string(), &mut temp_differences);

                if temp_differences.is_empty() {
                    // Perfect match
                    best_match_idx = Some(j);
                    break;
                } else if temp_differences.len() < fewest_differences {
                    fewest_differences = temp_differences.len();
                    best_match_idx = Some(j);
                }
            }
        }

        // Compare with best match
        if let Some(j) = best_match_idx {
            matched_indices[j] = true;
            let current_path = format!("{}[{}]", path, i);
            compare_json_recursive(&arr1[i], &arr2[j], current_path, differences);
        }
    }
}
