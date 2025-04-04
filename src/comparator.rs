use crate::parser::LogEntry;
use colored::Colorize;
use serde_json::{Value, json};
use similar::{ChangeTag, TextDiff};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub fn compare_logs(
    logs1: &[LogEntry],
    logs2: &[LogEntry],
    component_filter: Option<&str>,
    level_filter: Option<&str>,
    contains_filter: Option<&str>,
    diff_only: bool,
    output_path: Option<&Path>,
    show_full: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut output_file = if let Some(path) = output_path {
        Some(File::create(path)?)
    } else {
        None
    };

    // Group logs by component and event type
    let mut grouped_logs1: HashMap<String, Vec<&LogEntry>> = HashMap::new();
    let mut grouped_logs2: HashMap<String, Vec<&LogEntry>> = HashMap::new();

    for log in logs1 {
        if should_include_log(log, component_filter, level_filter, contains_filter) {
            let key = get_log_key(log);
            grouped_logs1.entry(key).or_default().push(log);
        }
    }

    for log in logs2 {
        if should_include_log(log, component_filter, level_filter, contains_filter) {
            let key = get_log_key(log);
            grouped_logs2.entry(key).or_default().push(log);
        }
    }

    // Print unique keys in each log
    let keys1: Vec<&String> = grouped_logs1.keys().collect();
    let keys2: Vec<&String> = grouped_logs2.keys().collect();

    println!("Log file 1 has {} unique log types", keys1.len());
    println!("Log file 2 has {} unique log types", keys2.len());

    let mut unique_to_log1 = 0;
    let mut unique_to_log2 = 0;
    let mut shared_keys = 0;

    for key in &keys1 {
        if !grouped_logs2.contains_key(*key) {
            unique_to_log1 += 1;
            if !diff_only {
                println!("\nLog type only in file 1: {}", key.cyan());
                if let Some(ref mut file) = output_file {
                    writeln!(file, "\nLog type only in file 1: {}", key)?;
                }
            }
        } else {
            shared_keys += 1;
        }
    }

    for key in &keys2 {
        if !grouped_logs1.contains_key(*key) {
            unique_to_log2 += 1;
            if !diff_only {
                println!("\nLog type only in file 2: {}", key.magenta());
                if let Some(ref mut file) = output_file {
                    writeln!(file, "\nLog type only in file 2: {}", key)?;
                }
            }
        }
    }

    println!("Unique to log file 1: {}", unique_to_log1);
    println!("Unique to log file 2: {}", unique_to_log2);
    println!("Shared log types: {}", shared_keys);

    // Compare shared keys
    let mut keys: Vec<String> = grouped_logs1.keys().cloned().collect();
    keys.sort();

    for key in keys {
        if grouped_logs2.contains_key(&key) {
            let entries1 = grouped_logs1.get(&key).unwrap();
            let entries2 = grouped_logs2.get(&key).unwrap();

            // Compare all entries of the same type
            for (idx1, log1) in entries1.iter().enumerate() {
                for (idx2, log2) in entries2.iter().enumerate() {
                    if let (Some(payload1), Some(payload2)) = (&log1.payload, &log2.payload) {
                        let diff = compare_json(payload1, payload2);
                        if !diff.is_empty() || !diff_only {
                            println!(
                                "\n{} - Compare log {} #{} with log {} #{}",
                                key.yellow(),
                                "file1".cyan(),
                                idx1,
                                "file2".magenta(),
                                idx2
                            );

                            if let Some(ref mut file) = output_file {
                                writeln!(
                                    file,
                                    "\n{} - Compare log file1 #{} with log file2 #{}",
                                    key, idx1, idx2
                                )?;
                            }

                            if show_full {
                                // Show full JSON objects
                                println!("Log file 1:");
                                println!("{}", serde_json::to_string_pretty(payload1)?);
                                println!("\nLog file 2:");
                                println!("{}", serde_json::to_string_pretty(payload2)?);

                                if let Some(ref mut file) = output_file {
                                    writeln!(file, "Log file 1:")?;
                                    writeln!(file, "{}", serde_json::to_string_pretty(payload1)?)?;
                                    writeln!(file, "\nLog file 2:")?;
                                    writeln!(file, "{}", serde_json::to_string_pretty(payload2)?)?;
                                }
                            } else {
                                // Show only differences
                                for diff_item in &diff {
                                    let (path, val1, val2) = diff_item;
                                    println!(
                                        "{}: {} => {}",
                                        path.yellow(),
                                        format!("{:?}", val1).cyan(),
                                        format!("{:?}", val2).magenta()
                                    );

                                    if let Some(ref mut file) = output_file {
                                        writeln!(file, "{}: {:?} => {:?}", path, val1, val2)?;
                                    }
                                }
                            }

                            // Show text diff for non-JSON parts,
                            // but only if we have real differences in the JSON content
                            if !diff.is_empty() {
                                let text1 = log1.message.clone();
                                let text2 = log2.message.clone();

                                // Only show text differences if the messages are not identical
                                if text1 != text2 {
                                    // Check if the differences might be just JSON formatting
                                    let is_formatting_difference =
                                        is_only_json_formatting_difference(&text1, &text2);

                                    if !is_formatting_difference {
                                        let diff = TextDiff::from_lines(&text1, &text2);

                                        println!("\nText differences:");
                                        for change in diff.iter_all_changes() {
                                            let formatted = match change.tag() {
                                                ChangeTag::Delete => {
                                                    format!("{}", change.to_string().red())
                                                }
                                                ChangeTag::Insert => {
                                                    format!("{}", change.to_string().green())
                                                }
                                                ChangeTag::Equal => continue,
                                            };
                                            print!("{}", formatted);

                                            if let Some(ref mut file) = output_file {
                                                write!(file, "{}", change)?;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn get_log_key(log: &LogEntry) -> String {
    format!(
        "{}_{}{}",
        log.component,
        log.level,
        if let Some(event_type) = &log.event_type {
            format!("_{}", event_type)
        } else {
            if let Some(command) = &log.command {
                format!("_{}", command)
            } else {
                String::new()
            }
        }
    )
}

fn should_include_log(
    log: &LogEntry,
    component_filter: Option<&str>,
    level_filter: Option<&str>,
    contains_filter: Option<&str>,
) -> bool {
    let component_match = component_filter
        .map(|filter| log.component.contains(filter))
        .unwrap_or(true);

    let level_match = level_filter
        .map(|filter| log.level.contains(filter))
        .unwrap_or(true);

    let contains_match = contains_filter
        .map(|filter| log.message.contains(filter))
        .unwrap_or(true);

    component_match && level_match && contains_match
}

/// Determines if the only differences between two strings are JSON formatting/property order
/// This is used to prevent showing text diffs for messages that differ only in JSON formatting
pub fn is_only_json_formatting_difference(text1: &str, text2: &str) -> bool {
    // Extract all JSON objects from both texts
    let json_objects1 = extract_all_json_objects(text1);
    let json_objects2 = extract_all_json_objects(text2);

    // If we don't have the same number of JSON objects, this is not just a formatting difference
    if json_objects1.len() != json_objects2.len() {
        return false;
    }

    // If there are no JSON objects, we can't be dealing with a JSON formatting difference
    if json_objects1.is_empty() {
        return false;
    }

    // Compare each JSON object semantically
    for (json1, json2) in json_objects1.iter().zip(json_objects2.iter()) {
        if let (Ok(v1), Ok(v2)) = (
            serde_json::from_str::<Value>(json1),
            serde_json::from_str::<Value>(json2),
        ) {
            let differences = compare_json(&v1, &v2);
            if !differences.is_empty() {
                return false;
            }
        } else {
            // If we can't parse the JSON, assume it's not just a formatting difference
            return false;
        }
    }

    // If we get here, all JSON objects are semantically equivalent
    // Now check if the non-JSON parts of the strings are identical

    // Replace each JSON object with a placeholder in both strings
    let mut placeholder_text1 = text1.to_string();
    let mut placeholder_text2 = text2.to_string();

    for (i, json) in json_objects1.iter().enumerate() {
        placeholder_text1 = placeholder_text1.replace(json, &format!("__JSON_PLACEHOLDER_{}", i));
    }

    for (i, json) in json_objects2.iter().enumerate() {
        placeholder_text2 = placeholder_text2.replace(json, &format!("__JSON_PLACEHOLDER_{}", i));
    }

    // If the placeholdered texts are identical, then only the JSON formatting differed
    placeholder_text1 == placeholder_text2
}

/// Extracts all JSON objects from a string
pub fn extract_all_json_objects(text: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut start_indices = Vec::new();

    // Find all potential JSON object start positions
    for (i, c) in text.char_indices() {
        if c == '{' || c == '[' {
            start_indices.push(i);
        }
    }

    // For each start position, try to extract a valid JSON object
    for &start_idx in &start_indices {
        if let Some(end_idx) = find_json_end(text, start_idx) {
            let json_str = &text[start_idx..=end_idx];
            // Only add if it parses as valid JSON
            if serde_json::from_str::<Value>(json_str).is_ok() {
                results.push(json_str.to_string());
            }
        }
    }

    results
}

/// Finds the end index of a JSON object or array starting at start_idx
fn find_json_end(text: &str, start_idx: usize) -> Option<usize> {
    let first_char = text[start_idx..].chars().next()?;
    if first_char != '{' && first_char != '[' {
        return None;
    }

    let mut brace_count = 0;
    let mut bracket_count = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, c) in text[start_idx..].char_indices() {
        if in_string {
            if escape_next {
                escape_next = false;
                continue;
            }
            if c == '\\' {
                escape_next = true;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            continue;
        }

        match c {
            '"' => in_string = true,
            '{' => brace_count += 1,
            '}' => {
                brace_count -= 1;
                if brace_count == 0 && first_char == '{' && bracket_count == 0 {
                    return Some(start_idx + i);
                }
            }
            '[' => bracket_count += 1,
            ']' => {
                bracket_count -= 1;
                if bracket_count == 0 && first_char == '[' && brace_count == 0 {
                    return Some(start_idx + i);
                }
            }
            _ => {}
        }
    }

    None
}

pub fn display_log_info(logs: &[LogEntry]) {
    let mut components = std::collections::HashSet::new();
    let mut event_types = std::collections::HashSet::new();
    let mut commands = std::collections::HashSet::new();
    let mut requests = std::collections::HashSet::new();
    let mut levels = std::collections::HashSet::new();

    for log in logs {
        components.insert(&log.component);
        if let Some(event_type) = &log.event_type {
            event_types.insert(event_type);
        }
        if let Some(command) = &log.command {
            commands.insert(command);
        }
        if let Some(req) = &log.request {
            requests.insert(req);
        }
        levels.insert(&log.level);
    }

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
        (Value::Array(arr1), Value::Array(arr2)) => {
            // Special handling for arrays containing objects
            // If both arrays have the same length and contain only objects,
            // try to match objects by their content rather than their position
            if arr1.len() == arr2.len()
                && arr1.iter().all(|v| v.is_object())
                && arr2.iter().all(|v| v.is_object())
            {
                // Try to match objects between arrays
                let mut matched_indices = vec![false; arr2.len()];

                for (i, obj1) in arr1.iter().enumerate() {
                    let mut best_match_idx = None;
                    let mut fewest_differences = usize::MAX;

                    // Find the best matching object in arr2
                    for (j, obj2) in arr2.iter().enumerate() {
                        if !matched_indices[j] {
                            let mut temp_differences = Vec::new();
                            compare_json_recursive(
                                obj1,
                                obj2,
                                "temp".to_string(),
                                &mut temp_differences,
                            );

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
        (val1, val2) => {
            if val1 != val2 {
                differences.push((path, val1.clone(), val2.clone()));
            }
        }
    }
}
