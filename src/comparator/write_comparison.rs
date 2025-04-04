use crate::comparator::{ComparisonOptions, ComparisonResults, JsonDifference, LogComparison};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

/// Writes comparison results to a file with the same formatting as console output
pub fn write_results_to_file(
    results: &ComparisonResults,
    options: &ComparisonOptions,
    path: &Path,
) -> io::Result<()> {
    let mut file = File::create(path)?;

    // Write header
    writeln!(file, "{}", "=".repeat(80))?;
    writeln!(file, "LOG COMPARISON SUMMARY")?;
    writeln!(file, "{}", "=".repeat(80))?;

    writeln!(
        file,
        "{} unique log types in file 1 (source) [src:1]",
        results.unique_to_log1.len()
    )?;
    writeln!(
        file,
        "{} unique log types in file 2 (target) [src:2]",
        results.unique_to_log2.len()
    )?;
    writeln!(
        file,
        "{} shared log types with {} comparisons [src:3]",
        results.shared_comparisons.len(),
        results
            .shared_comparisons
            .iter()
            .map(|c| c.json_differences.len())
            .sum::<usize>()
    )?;

    // Write unique keys
    if !options.diff_only {
        if !results.unique_to_log1.is_empty() {
            writeln!(file, "\nLOGS UNIQUE TO FILE 1 [src:10]")?;
            writeln!(file, "{}", "-".repeat(80))?;
            for (index, key) in results.unique_to_log1.iter().enumerate() {
                let parts: Vec<&str> = key.split('|').collect();
                if parts.len() >= 3 {
                    writeln!(
                        file,
                        "[L{}] {}  {}  {}",
                        index + 1,
                        parts[0],
                        parts[1],
                        parts[2].trim()
                    )?;
                } else {
                    writeln!(file, "[L{}] {}", index + 1, key)?;
                }
            }
        }

        if !results.unique_to_log2.is_empty() {
            writeln!(file, "\nLOGS UNIQUE TO FILE 2 [src:30]")?;
            writeln!(file, "{}", "-".repeat(80))?;
            for (index, key) in results.unique_to_log2.iter().enumerate() {
                let parts: Vec<&str> = key.split('|').collect();
                if parts.len() >= 3 {
                    writeln!(
                        file,
                        "[L{}] {}  {}  {}",
                        index + 1,
                        parts[0],
                        parts[1],
                        parts[2].trim()
                    )?;
                } else {
                    writeln!(file, "[L{}] {}", index + 1, key)?;
                }
            }
        }
    }

    // Group comparisons by key for better organization
    let mut grouped_comparisons: HashMap<&String, Vec<&LogComparison>> = HashMap::new();
    for comparison in &results.shared_comparisons {
        grouped_comparisons
            .entry(&comparison.key)
            .or_default()
            .push(comparison);
    }

    // Order keys for consistent display
    let mut keys: Vec<&String> = grouped_comparisons.keys().copied().collect();
    keys.sort();

    // Write shared key comparisons
    if !results.shared_comparisons.is_empty() {
        writeln!(file, "\nSHARED LOGS WITH DIFFERENCES [src:50]")?;
        writeln!(file, "{}", "=".repeat(80))?;

        for (key_idx, key) in keys.iter().enumerate() {
            let comparisons = grouped_comparisons.get(key).unwrap();
            let parts: Vec<&str> = key.split('|').collect();

            // Write key header with source line reference
            writeln!(file, "\n{}", "▼".repeat(80))?;
            if parts.len() >= 3 {
                writeln!(
                    file,
                    "[K{}] {} {} {} {} ({} instances)",
                    key_idx + 1,
                    parts[0],
                    parts[1],
                    parts[2].trim(),
                    parts[3].trim(),
                    comparisons.len()
                )?;
            } else {
                writeln!(
                    file,
                    "[K{}] {} ({} instances)",
                    key_idx + 1,
                    key,
                    comparisons.len()
                )?;
            }
            writeln!(file, "{}", "▲".repeat(80))?;

            // Write each comparison for this key
            for (idx, comparison) in comparisons.iter().enumerate() {
                writeln!(
                    file,
                    "\n{}/{}. FILE1 #{} [S:{}] ↔ FILE2 #{} [T:{}]",
                    idx + 1,
                    comparisons.len(),
                    comparison.log1_index,
                    comparison.log1_index + 100, // Source file line reference
                    comparison.log2_index,
                    comparison.log2_index + 200 // Target file line reference
                )?;

                if options.show_full_json {
                    write_full_json_to_file(&mut file, comparison)?;
                } else {
                    write_json_differences_to_file(&mut file, comparison)?;
                }

                if let Some(text_diff) = &comparison.text_difference {
                    writeln!(file, "\nTEXT DIFFERENCES: [src:70]")?;
                    writeln!(file, "{}", text_diff)?;
                }

                // Add separator between comparisons
                if idx < comparisons.len() - 1 {
                    writeln!(file, "\n{}", "-".repeat(40))?;
                }
            }
        }
    }

    Ok(())
}

/// Writes JSON differences to file with proper JSON formatting
fn write_json_differences_to_file(file: &mut File, comparison: &LogComparison) -> io::Result<()> {
    if comparison.json_differences.is_empty() {
        writeln!(file, "  [No JSON differences]")?;
        return Ok(());
    }

    writeln!(file, "  JSON DIFFERENCES: [src:90]")?;

    // Group differences by path prefix for better organization
    let mut grouped_diffs: HashMap<String, Vec<&JsonDifference>> = HashMap::new();

    for diff in &comparison.json_differences {
        let path_parts: Vec<&str> = diff.path.split('.').collect();
        let prefix = if path_parts.len() > 1 {
            path_parts[0].to_string()
        } else {
            "root".to_string()
        };

        grouped_diffs.entry(prefix).or_default().push(diff);
    }

    // Sort prefixes for consistent display
    let mut prefixes: Vec<String> = grouped_diffs.keys().cloned().collect();
    prefixes.sort();

    for (prefix_idx, prefix) in prefixes.iter().enumerate() {
        let diffs = grouped_diffs.get(prefix).unwrap();

        if prefix != "root" {
            writeln!(file, "  {} [P:{}]", prefix, prefix_idx + 1)?;
        }

        for (diff_idx, diff) in diffs.iter().enumerate() {
            let path_display = if prefix == "root" {
                diff.path.clone()
            } else {
                diff.path
                    .strip_prefix(&format!("{}.", prefix))
                    .unwrap_or(&diff.path)
                    .to_string()
            };

            // Format values as proper JSON
            let value1_str = match serde_json::to_string(&diff.value1) {
                Ok(s) => s,
                Err(_) => format!("{:?}", diff.value1), // Fallback
            };

            let value2_str = match serde_json::to_string(&diff.value2) {
                Ok(s) => s,
                Err(_) => format!("{:?}", diff.value2), // Fallback
            };

            // Truncate very long values
            let max_len = 50;
            let value1_display = if value1_str.len() > max_len {
                format!("{}...", &value1_str[0..max_len])
            } else {
                value1_str
            };

            let value2_display = if value2_str.len() > max_len {
                format!("{}...", &value2_str[0..max_len])
            } else {
                value2_str
            };

            writeln!(
                file,
                "    [D:{}] {} :\n      {} ➔\n      {}",
                diff_idx + 1,
                path_display,
                value1_display,
                value2_display
            )?;
        }
    }

    Ok(())
}

/// Writes full JSON comparison to file with proper formatting
fn write_full_json_to_file(file: &mut File, comparison: &LogComparison) -> io::Result<()> {
    if !comparison.json_differences.is_empty() {
        writeln!(file, "Log file 1 [src:130]:")?;
        match serde_json::to_string_pretty(&comparison.json_differences[0].value1) {
            Ok(json) => writeln!(file, "{}", json)?,
            Err(_) => writeln!(file, "Error formatting JSON")?,
        }

        writeln!(file, "\nLog file 2 [src:140]:")?;
        match serde_json::to_string_pretty(&comparison.json_differences[0].value2) {
            Ok(json) => writeln!(file, "{}", json)?,
            Err(_) => writeln!(file, "Error formatting JSON")?,
        }
    }

    Ok(())
}
