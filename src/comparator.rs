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

                            // Show text diff for non-JSON parts
                            let text1 = log1.message.clone();
                            let text2 = log2.message.clone();
                            if text1 != text2 {
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
            String::new()
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

pub fn display_log_info(logs: &[LogEntry]) {
    let mut components = std::collections::HashSet::new();
    let mut event_types = std::collections::HashSet::new();
    let mut levels = std::collections::HashSet::new();

    for log in logs {
        components.insert(&log.component);
        if let Some(event_type) = &log.event_type {
            event_types.insert(event_type);
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

    println!("\nTotal log entries: {}", logs.len());
}

fn compare_json(json1: &Value, json2: &Value) -> Vec<(String, Value, Value)> {
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
            // Check keys that exist in both objects
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

            // Check keys that only exist in obj2
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
