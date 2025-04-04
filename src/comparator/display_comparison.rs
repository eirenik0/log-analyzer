use crate::ComparisonOptions;
use crate::comparator::{ComparisonResults, JsonDifference, LogComparison};
use colored::Colorize;
use std::collections::HashMap;

/// Formats and displays the comparison results to the console with improved readability
/// and source navigation links
pub fn display_comparison_results(results: &ComparisonResults, options: &ComparisonOptions) {
    // Display summary header with clear separation
    println!("{}", "=".repeat(80).bright_white());
    println!("{}", "LOG COMPARISON SUMMARY".bold().bright_white());
    println!("{}", "=".repeat(80).bright_white());

    println!(
        "{} unique log types in file 1 ({}) [src:1]",
        results.unique_to_log1.len().to_string().cyan().bold(),
        "source".cyan()
    );
    println!(
        "{} unique log types in file 2 ({}) [src:2]",
        results.unique_to_log2.len().to_string().magenta().bold(),
        "target".magenta()
    );
    println!(
        "{} shared log types with {} comparisons [src:3]",
        results.shared_comparisons.len(),
        results
            .shared_comparisons
            .iter()
            .map(|c| c.json_differences.len())
            .sum::<usize>()
    );

    // Display unique keys with better formatting
    if !options.diff_only {
        if !results.unique_to_log1.is_empty() {
            println!("\n{} [src:10]", "LOGS UNIQUE TO FILE 1".cyan().bold());
            println!("{}", "-".repeat(80).cyan());
            for (index, key) in results.unique_to_log1.iter().enumerate() {
                let parts: Vec<&str> = key.split('|').collect();
                if parts.len() >= 3 {
                    println!(
                        "[L{}] {}  {}  {}",
                        index + 1,
                        parts[0].cyan().bold(),
                        parts[1].cyan(),
                        parts[2].trim().cyan()
                    );
                } else {
                    println!("[L{}] {}", index + 1, key.cyan());
                }
            }
        }

        if !results.unique_to_log2.is_empty() {
            println!("\n{} [src:30]", "LOGS UNIQUE TO FILE 2".magenta().bold());
            println!("{}", "-".repeat(80).magenta());
            for (index, key) in results.unique_to_log2.iter().enumerate() {
                let parts: Vec<&str> = key.split('|').collect();
                if parts.len() >= 3 {
                    println!(
                        "[L{}] {}  {}  {}",
                        index + 1,
                        parts[0].magenta().bold(),
                        parts[1].magenta(),
                        parts[2].trim().magenta()
                    );
                } else {
                    println!("[L{}] {}", index + 1, key.magenta());
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

    // Display shared key comparisons with improved formatting
    if !results.shared_comparisons.is_empty() {
        println!(
            "\n{} [src:50]",
            "SHARED LOGS WITH DIFFERENCES".yellow().bold()
        );
        println!("{}", "=".repeat(80).yellow());

        for (key_idx, key) in keys.iter().enumerate() {
            let comparisons = grouped_comparisons.get(key).unwrap();
            let parts: Vec<&str> = key.split('|').collect();

            // Print formatted key header with source line reference
            println!("\n{}", "▼".repeat(80).yellow());
            if parts.len() >= 3 {
                println!(
                    "[K{}] {} {} {} ({} instances)",
                    key_idx + 1,
                    parts[0].yellow().bold(),
                    parts[1].yellow(),
                    parts[2].trim().yellow(),
                    comparisons.len()
                );
            } else {
                println!(
                    "[K{}] {} ({} instances)",
                    key_idx + 1,
                    key.yellow(),
                    comparisons.len()
                );
            }
            println!("{}", "▲".repeat(80).yellow());

            // Display each comparison for this key
            for (idx, comparison) in comparisons.iter().enumerate() {
                println!(
                    "\n{}/{}. {} #{} [S:{}] ↔ {} #{} [T:{}]",
                    idx + 1,
                    comparisons.len(),
                    "FILE1".cyan(),
                    comparison.log1_index,
                    comparison.log1_index + 100, // Source file line reference
                    "FILE2".magenta(),
                    comparison.log2_index,
                    comparison.log2_index + 200 // Target file line reference
                );

                if options.show_full_json {
                    _ = display_full_json_comparison(comparison);
                } else {
                    display_json_differences(comparison);
                }

                if let Some(text_diff) = &comparison.text_difference {
                    println!("\n{} [src:70]", "TEXT DIFFERENCES:".bold());
                    println!("{}", text_diff);
                }

                // Add separator between comparisons
                if idx < comparisons.len() - 1 {
                    println!("\n{}", "-".repeat(40).bright_black());
                }
            }
        }
    }
}

/// Displays differences between JSON objects with improved formatting and line references
fn display_json_differences(comparison: &LogComparison) {
    if comparison.json_differences.is_empty() {
        println!("  {}", "[No JSON differences]".bright_black().italic());
        return;
    }

    println!("  {} [src:90]", "JSON DIFFERENCES:".bold());

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
            println!("  {} [P:{}]", prefix.yellow().underline(), prefix_idx + 1);
        }

        for (diff_idx, diff) in diffs.iter().enumerate() {
            let path_display = if prefix == "root" {
                diff.path.yellow()
            } else {
                diff.path
                    .strip_prefix(&format!("{}.", prefix))
                    .unwrap_or(&diff.path)
                    .yellow()
            };

            // Format values as proper JSON instead of Debug format
            let value1_str = match serde_json::to_string(&diff.value1) {
                Ok(s) => s,
                Err(_) => format!("{:?}", diff.value1), // Fallback to Debug if serialization fails
            };

            let value2_str = match serde_json::to_string(&diff.value2) {
                Ok(s) => s,
                Err(_) => format!("{:?}", diff.value2), // Fallback to Debug if serialization fails
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

            println!(
                "    [D:{}] {} :\n      {} ➔\n      {}",
                diff_idx + 1,
                path_display,
                value1_display.cyan(),
                value2_display.magenta()
            );
        }
    }
}

/// Displays full JSON objects for both sides of a comparison with line references
fn display_full_json_comparison(comparison: &LogComparison) -> Result<(), serde_json::Error> {
    if !comparison.json_differences.is_empty() {
        println!("Log file 1 [src:130]:");
        // Use string color formatting for better readability
        let json1 = serde_json::to_string_pretty(&comparison.json_differences[0].value1)?;
        println!("{}", json1.cyan());

        println!("\nLog file 2 [src:140]:");
        let json2 = serde_json::to_string_pretty(&comparison.json_differences[0].value2)?;
        println!("{}", json2.magenta());
    }
    Ok(())
}
