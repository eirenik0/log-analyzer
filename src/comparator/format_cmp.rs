use crate::comparator::{ComparisonOptions, ComparisonResults, JsonDifference, LogComparison};
use std::collections::HashMap;

/// Output formatter trait that abstracts over console and file output
pub trait OutputFormatter {
    fn write_header(&mut self, text: &str) -> std::io::Result<()>;
    fn write_divider(&mut self, char: &str, count: usize) -> std::io::Result<()>;
    fn write_line(&mut self, text: &str) -> std::io::Result<()>;
    fn write_source_file1(&mut self, text: &str) -> std::io::Result<()>;
    fn write_source_file2(&mut self, text: &str) -> std::io::Result<()>;
    fn write_highlight(&mut self, text: &str) -> std::io::Result<()>;
    fn write_label(&mut self, text: &str) -> std::io::Result<()>;
}

/// Formats comparison results using the provided formatter
pub fn format_comparison_results<F: OutputFormatter>(
    formatter: &mut F,
    results: &ComparisonResults,
    options: &ComparisonOptions,
) -> std::io::Result<()> {
    // Display summary header with clear separation
    formatter.write_divider("=", 80)?;
    formatter.write_header("LOG COMPARISON SUMMARY")?;
    formatter.write_divider("=", 80)?;

    formatter.write_line(&format!(
        "{} unique log types in file 1 (source) [src:1]",
        results.unique_to_log1.len()
    ))?;
    formatter.write_line(&format!(
        "{} unique log types in file 2 (target) [src:2]",
        results.unique_to_log2.len()
    ))?;
    formatter.write_line(&format!(
        "{} shared log types with {} comparisons [src:3]",
        results.shared_comparisons.len(),
        results
            .shared_comparisons
            .iter()
            .map(|c| c.json_differences.len())
            .sum::<usize>()
    ))?;

    // Display unique keys with better formatting
    if !options.diff_only {
        if !results.unique_to_log1.is_empty() {
            formatter.write_line("\nLOGS UNIQUE TO FILE 1 [src:10]")?;
            formatter.write_divider("-", 80)?;
            for (index, key) in results.unique_to_log1.iter().enumerate() {
                let parts: Vec<&str> = key.split('|').collect();
                if parts.len() >= 3 {
                    formatter.write_source_file1(&format!(
                        "[L{}] {}  {}  {}",
                        index + 1,
                        parts[0],
                        parts[1],
                        parts[2].trim()
                    ))?;
                } else {
                    formatter.write_source_file1(&format!("[L{}] {}", index + 1, key))?;
                }
            }
        }

        if !results.unique_to_log2.is_empty() {
            formatter.write_line("\nLOGS UNIQUE TO FILE 2 [src:30]")?;
            formatter.write_divider("-", 80)?;
            for (index, key) in results.unique_to_log2.iter().enumerate() {
                let parts: Vec<&str> = key.split('|').collect();
                if parts.len() >= 3 {
                    formatter.write_source_file2(&format!(
                        "[L{}] {}  {}  {}",
                        index + 1,
                        parts[0],
                        parts[1],
                        parts[2].trim()
                    ))?;
                } else {
                    formatter.write_source_file2(&format!("[L{}] {}", index + 1, key))?;
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
        formatter.write_line("\nSHARED LOGS WITH DIFFERENCES [src:50]")?;
        formatter.write_divider("=", 80)?;

        for (key_idx, key) in keys.iter().enumerate() {
            let comparisons = grouped_comparisons.get(key).unwrap();
            let parts: Vec<&str> = key.split('|').collect();

            // Print formatted key header with source line reference
            formatter.write_line("")?;
            formatter.write_divider("▼", 80)?;
            if parts.len() >= 3 {
                formatter.write_highlight(&format!(
                    "[K{}] {} {} {} {} ({} instances)",
                    key_idx + 1,
                    parts[0],
                    parts[1],
                    parts[2].trim(),
                    parts[3].trim(),
                    comparisons.len()
                ))?;
            } else {
                formatter.write_highlight(&format!(
                    "[K{}] {} ({} instances)",
                    key_idx + 1,
                    key,
                    comparisons.len()
                ))?;
            }
            formatter.write_divider("▲", 80)?;

            // Display each comparison for this key
            for (idx, comparison) in comparisons.iter().enumerate() {
                formatter.write_line(&format!(
                    "\n{}/{}. FILE1 #{} [S:{}] ↔ FILE2 #{} [T:{}]",
                    idx + 1,
                    comparisons.len(),
                    comparison.log1_index,
                    comparison.log1_index + 100, // Source file line reference
                    comparison.log2_index,
                    comparison.log2_index + 200 // Target file line reference
                ))?;

                if options.show_full_json {
                    format_full_json_comparison(formatter, comparison)?;
                } else {
                    format_json_differences(formatter, comparison)?;
                }

                if let Some(text_diff) = &comparison.text_difference {
                    formatter.write_line("\nTEXT DIFFERENCES: [src:70]")?;
                    formatter.write_line(text_diff)?;
                }

                // Add separator between comparisons
                if idx < comparisons.len() - 1 {
                    formatter.write_divider("-", 40)?;
                }
            }
        }
    }

    Ok(())
}

/// Formats differences between JSON objects
pub fn format_json_differences<F: OutputFormatter>(
    formatter: &mut F,
    comparison: &LogComparison,
) -> std::io::Result<()> {
    if comparison.json_differences.is_empty() {
        formatter.write_line("  [No JSON differences]")?;
        return Ok(());
    }

    formatter.write_label("  JSON DIFFERENCES: [src:90]")?;

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
            formatter.write_highlight(&format!("  {} [P:{}]", prefix, prefix_idx + 1))?;
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
                value1_str.clone()
            };

            let value2_display = if value2_str.len() > max_len {
                format!("{}...", &value2_str[0..max_len])
            } else {
                value2_str.clone()
            };

            formatter.write_line(&format!("    [D:{}] {} :", diff_idx + 1, path_display))?;
            formatter.write_source_file1(&format!("      {}", value1_display))?;
            formatter.write_line("      ➔")?;
            formatter.write_source_file2(&format!("      {}", value2_display))?;
        }
    }

    Ok(())
}

/// Formats full JSON comparison
pub fn format_full_json_comparison<F: OutputFormatter>(
    formatter: &mut F,
    comparison: &LogComparison,
) -> std::io::Result<()> {
    if !comparison.json_differences.is_empty() {
        formatter.write_line("Log file 1 [src:130]:")?;
        match serde_json::to_string_pretty(&comparison.json_differences[0].value1) {
            Ok(json) => formatter.write_source_file1(&json)?,
            Err(_) => formatter.write_line("Error formatting JSON")?,
        }

        formatter.write_line("\nLog file 2 [src:140]:")?;
        match serde_json::to_string_pretty(&comparison.json_differences[0].value2) {
            Ok(json) => formatter.write_source_file2(&json)?,
            Err(_) => formatter.write_line("Error formatting JSON")?,
        }
    }

    Ok(())
}
