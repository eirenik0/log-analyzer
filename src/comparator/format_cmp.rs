use crate::comparator::{ComparisonOptions, ComparisonResults, JsonDifference, LogComparison};
use comfy_table::{Attribute, Cell, ContentArrangement, Table};
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
    // Semantic formatting methods
    fn write_success(&mut self, text: &str) -> std::io::Result<()>;
    fn write_warning(&mut self, text: &str) -> std::io::Result<()>;
    fn write_error(&mut self, text: &str) -> std::io::Result<()>;
    fn write_info(&mut self, text: &str) -> std::io::Result<()>;
    // Table support
    fn write_table(&mut self, table: &Table) -> std::io::Result<()>;
}

/// Creates a styled table with consistent formatting
pub fn create_styled_table(headers: &[&str]) -> Table {
    let mut table = Table::new();
    table
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(120)
        .set_header(
            headers
                .iter()
                .map(|h| Cell::new(*h).add_attribute(Attribute::Bold))
                .collect::<Vec<_>>(),
        );
    table
}

/// Formats comparison results using the provided formatter
pub fn format_comparison_results<F: OutputFormatter>(
    formatter: &mut F,
    results: &ComparisonResults,
    options: &ComparisonOptions,
) -> std::io::Result<()> {
    // Check if we can print based on verbosity
    if crate::comparator::console_cmp::should_print(options, 1) {
        // Display summary header with clear separation
        formatter.write_divider("=", 80)?;
        formatter.write_header("LOG COMPARISON SUMMARY")?;
        formatter.write_divider("=", 80)?;
    }

    // Improved summary statistics with colorization
    let unique_log1_count = results.unique_to_log1.len();
    let unique_log2_count = results.unique_to_log2.len();
    let shared_log_count = results.shared_comparisons.len();
    let total_comparisons = results
        .shared_comparisons
        .iter()
        .map(|c| c.json_differences.len())
        .sum::<usize>();

    // Always show basic summary information (verbosity level 0, even in quiet mode)
    formatter.write_info(&format!(
        "{} unique log types in file 1 (source), {} unique in file 2 (target), {} shared",
        unique_log1_count, unique_log2_count, shared_log_count
    ))?;

    // Show more detailed info at higher verbosity levels
    if crate::comparator::console_cmp::should_print(options, 2) {
        formatter.write_info(&format!(
            "{} total comparisons across shared log types",
            total_comparisons
        ))?;
    }

    // Display unique keys with better formatting - only in normal/verbose mode
    if crate::comparator::console_cmp::should_print(options, 1) {
        if !results.unique_to_log1.is_empty() {
            formatter.write_divider("=", 80)?;
            formatter.write_header("LOGS UNIQUE TO FILE 1")?;
            formatter.write_divider("=", 80)?;

            for (index, key) in results.unique_to_log1.iter().enumerate() {
                let parts: Vec<&str> = key.split('|').collect();
                if parts.len() >= 3 {
                    let details = if parts.len() > 3 {
                        parts[3..].join("|").trim().to_string()
                    } else {
                        String::new()
                    };
                    formatter.write_source_file1(&format!(
                        "[L{}] {}  {}  {}{}{}",
                        index + 1,
                        parts[0],
                        parts[1],
                        parts[2].trim(),
                        if details.is_empty() { "" } else { ": " },
                        details
                    ))?;
                } else {
                    formatter.write_source_file1(&format!("[L{}] {}", index + 1, key))?;
                }
            }
        }

        if !results.unique_to_log2.is_empty() {
            formatter.write_divider("=", 80)?;
            formatter.write_header("LOGS UNIQUE TO FILE 2")?;
            formatter.write_divider("=", 80)?;

            for (index, key) in results.unique_to_log2.iter().enumerate() {
                let parts: Vec<&str> = key.split('|').collect();
                if parts.len() >= 3 {
                    let details = if parts.len() > 3 {
                        parts[3..].join("|").trim().to_string()
                    } else {
                        String::new()
                    };
                    formatter.write_source_file2(&format!(
                        "[L{}] {}  {}  {}{}{}",
                        index + 1,
                        parts[0],
                        parts[1],
                        parts[2].trim(),
                        if details.is_empty() { "" } else { ": " },
                        details
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

    // Filter out comparisons without differences if diff_only is set
    let mut has_differences = false;
    for comparisons in grouped_comparisons.values() {
        for comparison in comparisons.iter() {
            if !comparison.json_differences.is_empty() || comparison.text1.is_some() {
                has_differences = true;
                break;
            }
        }
        if has_differences {
            break;
        }
    }

    // Display shared key comparisons with improved formatting
    if has_differences {
        formatter.write_divider("=", 80)?;
        formatter.write_header("SHARED LOGS WITH DIFFERENCES")?;
        formatter.write_divider("=", 80)?;

        for (key_idx, key) in keys.iter().enumerate() {
            let comparisons = grouped_comparisons.get(key).unwrap();

            // Skip this key if there are no differences and diff_only is set
            if options.diff_only {
                let has_key_differences = comparisons.iter().any(|comparison| {
                    !comparison.json_differences.is_empty() || comparison.text1.is_some()
                });
                if !has_key_differences {
                    continue;
                }
            }

            let parts: Vec<&str> = key.split('|').collect();

            // Print formatted key header with clearer structure
            formatter.write_line("")?;
            formatter.write_divider("▼", 80)?;

            if parts.len() >= 3 {
                // Format for component|level|kind|details
                let component = parts[0];
                let level = parts[1];
                let kind = parts[2].trim();
                let details = if parts.len() > 3 {
                    parts[3..].join("|").trim().to_string()
                } else {
                    String::new()
                };

                formatter.write_highlight(&format!(
                    "[K{}] {} {} {}{}{} ({} instances)",
                    key_idx + 1,
                    component,
                    level,
                    kind,
                    if details.is_empty() { "" } else { ": " },
                    details,
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
                // Skip if there are no differences and diff_only is set
                if options.diff_only
                    && comparison.json_differences.is_empty()
                    && comparison.text1.is_none()
                {
                    continue;
                }

                formatter.write_line(&format!(
                    "\n{}/{}. FILE1 #{} (line {}) ↔ FILE2 #{} (line {})",
                    idx + 1,
                    comparisons.len(),
                    comparison.log1_index,
                    comparison.log1_line_number,
                    comparison.log2_index,
                    comparison.log2_line_number
                ))?;

                if options.show_full_json {
                    format_full_json_comparison(formatter, comparison)?;
                } else {
                    format_json_differences(formatter, comparison)?;
                }

                if comparison.text1.is_some() || comparison.text2.is_some() {
                    formatter.write_label("  TEXT DIFFERENCES:")?;
                    if let Some(text1) = &comparison.text1 {
                        formatter.write_source_file1(&format!("    File 1: {}", text1))?;
                    }
                    if let Some(text2) = &comparison.text2 {
                        formatter.write_source_file2(&format!("    File 2: {}", text2))?;
                    }
                }

                // Add separator between comparisons
                if idx < comparisons.len() - 1 {
                    formatter.write_divider("-", 40)?;
                }
            }
        }
    } else if !options.diff_only {
        formatter.write_divider("=", 80)?;
        formatter.write_success("Logs are identical - no differences found")?;
        formatter.write_divider("=", 80)?;
    }

    formatter.write_line("\nComparison completed successfully.")?;
    Ok(())
}

/// Formats differences between JSON objects with improved readability
pub fn format_json_differences<F: OutputFormatter>(
    formatter: &mut F,
    comparison: &LogComparison,
) -> std::io::Result<()> {
    if comparison.json_differences.is_empty() {
        return Ok(());
    }

    formatter.write_label("  JSON DIFFERENCES:")?;

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

            // Determine if values are truncated
            let max_len = 50;
            let value1_truncated = value1_str.len() > max_len;
            let value2_truncated = value2_str.len() > max_len;

            let value1_display = if value1_truncated {
                format!("{}...", &value1_str[0..max_len])
            } else {
                value1_str.clone()
            };

            let value2_display = if value2_truncated {
                format!("{}...", &value2_str[0..max_len])
            } else {
                value2_str.clone()
            };

            // Determine change type indicator
            let change_indicator = match diff.change_type {
                crate::comparator::ChangeType::Added => "+",
                crate::comparator::ChangeType::Removed => "-",
                crate::comparator::ChangeType::Modified => "~",
            };

            // Improved formatting for differences
            formatter.write_line(&format!(
                "    [D:{}] [{}] {} :",
                diff_idx + 1,
                change_indicator,
                path_display
            ))?;
            formatter.write_source_file1(&format!(
                "      {}{}",
                value1_display,
                if value1_truncated { " (truncated)" } else { "" }
            ))?;
            formatter.write_line("      ➔")?;
            formatter.write_source_file2(&format!(
                "      {}{}",
                value2_display,
                if value2_truncated { " (truncated)" } else { "" }
            ))?;
        }
    }

    Ok(())
}

/// Formats full JSON comparison with better indentation and structure
pub fn format_full_json_comparison<F: OutputFormatter>(
    formatter: &mut F,
    comparison: &LogComparison,
) -> std::io::Result<()> {
    if let (Some(log1_payload), Some(log2_payload)) =
        (&comparison.log1_payload, &comparison.log2_payload)
    {
        formatter.write_label("  FULL JSON COMPARISON:")?;

        formatter.write_source_file1("  LOG FILE 1:")?;
        match serde_json::to_string_pretty(log1_payload) {
            Ok(json) => {
                // Indent each line for better readability
                for line in json.lines() {
                    formatter.write_source_file1(&format!("    {}", line))?;
                }
            }
            Err(_) => formatter.write_error("    Error formatting JSON")?,
        };

        formatter.write_source_file2("\n  LOG FILE 2:")?;
        match serde_json::to_string_pretty(log2_payload) {
            Ok(json) => {
                // Indent each line for better readability
                for line in json.lines() {
                    formatter.write_source_file2(&format!("    {}", line))?;
                }
            }
            Err(_) => formatter.write_error("    Error formatting JSON")?,
        };
    }

    Ok(())
}
