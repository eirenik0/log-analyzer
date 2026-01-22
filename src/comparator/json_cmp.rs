use crate::ComparisonOptions;
use crate::comparator::ComparisonResults;
use crate::comparator::JsonDifference;
use crate::comparator::LogComparison;
use serde_json::{Value, json};
use std::collections::HashMap;

/// JSON output formatter for LLM consumption
pub struct JsonFormatter {
    pub output: Value,
}

impl JsonFormatter {
    /// Creates a new JSON formatter
    pub fn new() -> Self {
        Self {
            output: json!({
                "s": {}, // summary
                "u1": [], // unique_to_log1
                "u2": [], // unique_to_log2
                "c": []   // comparisons
            }),
        }
    }

    /// Formats comparison results as JSON
    pub fn format_results(
        &mut self,
        results: &ComparisonResults,
        options: &ComparisonOptions,
    ) -> Value {
        if options.readable_mode {
            return self.format_results_readable(results, options);
        } else if !options.compact_mode {
            return self.format_results_standard(results, options);
        }

        // Add summary statistics
        let unique_log1_count = results.unique_to_log1.len();
        let unique_log2_count = results.unique_to_log2.len();
        let shared_log_count = results.shared_comparisons.len();
        let total_diff_count = results
            .shared_comparisons
            .iter()
            .map(|c| c.json_differences.len())
            .sum::<usize>();

        let summary = json!({
            "u1c": unique_log1_count,      // unique_to_log1_count
            "u2c": unique_log2_count,      // unique_to_log2_count
            "sc": shared_log_count,        // shared_count
            "dc": total_diff_count,        // differences_count
            "hd": total_diff_count > 0     // has_differences
        });

        self.output["s"] = summary;

        // Add unique logs if not diff_only mode
        if !options.diff_only {
            self.add_unique_logs_compact(&results.unique_to_log1, &results.unique_to_log2);
        }

        // Add shared comparisons
        self.add_comparisons_compact(&results.shared_comparisons, options);

        self.output.clone()
    }

    /// Formats comparison results in a readable format with full field names and structure
    fn format_results_readable(
        &mut self,
        results: &ComparisonResults,
        options: &ComparisonOptions,
    ) -> Value {
        // Create readable format output
        let mut readable_output = json!({
            "summary": {},
            "unique_to_log1": [],
            "unique_to_log2": [],
            "comparisons": []
        });

        // Add summary statistics
        let unique_log1_count = results.unique_to_log1.len();
        let unique_log2_count = results.unique_to_log2.len();
        let shared_log_count = results.shared_comparisons.len();
        let total_diff_count = results
            .shared_comparisons
            .iter()
            .map(|c| c.json_differences.len())
            .sum::<usize>();

        let summary = json!({
            "unique_to_log1_count": unique_log1_count,
            "unique_to_log2_count": unique_log2_count,
            "shared_count": shared_log_count,
            "differences_count": total_diff_count,
            "has_differences": total_diff_count > 0
        });

        readable_output["summary"] = summary;

        // Add unique logs if not diff_only mode
        if !options.diff_only {
            let unique1: Vec<Value> = results
                .unique_to_log1
                .iter()
                .enumerate()
                .map(|(idx, key)| {
                    let parts: Vec<&str> = key.split('|').collect();
                    if parts.len() >= 3 {
                        json!({
                            "index": idx,
                            "component": parts[0],
                            "level": parts[1],
                            "kind": parts[2].trim(),
                            "details": if parts.len() > 3 { parts[3].trim() } else { "" },
                            "raw_key": key
                        })
                    } else {
                        json!({
                            "index": idx,
                            "raw_key": key
                        })
                    }
                })
                .collect();

            let unique2: Vec<Value> = results
                .unique_to_log2
                .iter()
                .enumerate()
                .map(|(idx, key)| {
                    let parts: Vec<&str> = key.split('|').collect();
                    if parts.len() >= 3 {
                        json!({
                            "index": idx,
                            "component": parts[0],
                            "level": parts[1],
                            "kind": parts[2].trim(),
                            "details": if parts.len() > 3 { parts[3].trim() } else { "" },
                            "raw_key": key
                        })
                    } else {
                        json!({
                            "index": idx,
                            "raw_key": key
                        })
                    }
                })
                .collect();

            readable_output["unique_to_log1"] = Value::Array(unique1);
            readable_output["unique_to_log2"] = Value::Array(unique2);
        }

        // Group comparisons by key
        let mut comparisons_array = Vec::new();
        let mut current_key = String::new();
        let mut current_group = Vec::new();

        for comparison in &results.shared_comparisons {
            // Filter out comparisons without differences if diff_only is set
            if options.diff_only
                && comparison.json_differences.is_empty()
                && comparison.text1.is_none()
            {
                continue;
            }

            if comparison.key != current_key {
                if !current_group.is_empty() {
                    // Add the previous group
                    let key_entry = self.format_key_group_readable(&current_key, &current_group);
                    comparisons_array.push(key_entry);
                    current_group = Vec::new();
                }
                current_key = comparison.key.clone();
            }
            current_group.push(comparison);
        }

        // Add the last group if it exists
        if !current_group.is_empty() {
            let key_entry = self.format_key_group_readable(&current_key, &current_group);
            comparisons_array.push(key_entry);
        }

        readable_output["comparisons"] = Value::Array(comparisons_array);

        readable_output
    }

    /// Formats a key group in readable format
    fn format_key_group_readable(&self, key: &str, comparisons: &[&LogComparison]) -> Value {
        let parts: Vec<&str> = key.split('|').collect();

        let key_info = if parts.len() >= 3 {
            json!({
                "component": parts[0],
                "level": parts[1],
                "kind": parts[2].trim(),
                "details": if parts.len() > 3 { parts[3].trim() } else { "" },
                "raw_key": key
            })
        } else {
            json!({
                "raw_key": key
            })
        };

        // Group differences by path
        let mut path_groups: HashMap<String, Vec<(&JsonDifference, usize, usize)>> = HashMap::new();

        // Collect all differences by path
        for comparison in comparisons {
            for diff in &comparison.json_differences {
                let entry = path_groups.entry(diff.path.clone()).or_default();
                entry.push((diff, comparison.log1_index, comparison.log2_index));
            }
        }

        // Create comparison instances with references to differences
        let comparison_values: Vec<Value> = comparisons
            .iter()
            .map(|comparison| {
                json!({
                    "log1_index": comparison.log1_index,
                    "log2_index": comparison.log2_index,
                    "text1": comparison.text1,
                    "text2": comparison.text2,
                    "log1_line": comparison.log1_line_number,
                    "log2_line": comparison.log2_line_number,
                    "diff_count": comparison.json_differences.len()
                })
            })
            .collect();

        // Create path-grouped differences
        let mut differences = Vec::new();
        for (path, diffs) in path_groups {
            let mut values1 = Vec::new();
            let mut values2 = Vec::new();
            let mut indexes = Vec::new();

            for (diff, log1_idx, log2_idx) in diffs {
                values1.push(diff.value1.clone());
                values2.push(diff.value2.clone());
                indexes.push(json!([log1_idx, log2_idx]));
            }

            differences.push(json!({
                "path": path,
                "value1": values1,
                "value2": values2,
                "indexes": indexes
            }));
        }

        json!({
            "key": key_info,
            "instances": comparison_values,
            "instance_count": comparisons.len(),
            "differences": differences
        })
    }

    /// Formats comparison results as standard JSON (original format)
    fn format_results_standard(
        &mut self,
        results: &ComparisonResults,
        options: &ComparisonOptions,
    ) -> Value {
        // Create standard format output
        let mut standard_output = json!({
            "summary": {},
            "unique_to_log1": [],
            "unique_to_log2": [],
            "comparisons": []
        });

        // Add summary statistics
        let unique_log1_count = results.unique_to_log1.len();
        let unique_log2_count = results.unique_to_log2.len();
        let shared_log_count = results.shared_comparisons.len();
        let total_diff_count = results
            .shared_comparisons
            .iter()
            .map(|c| c.json_differences.len())
            .sum::<usize>();

        let summary = json!({
            "unique_to_log1_count": unique_log1_count,
            "unique_to_log2_count": unique_log2_count,
            "shared_log_count": shared_log_count,
            "total_differences_count": total_diff_count,
            "has_differences": total_diff_count > 0
        });

        standard_output["summary"] = summary;

        // Add unique logs if not diff_only mode
        if !options.diff_only {
            let unique1: Vec<Value> = results
                .unique_to_log1
                .iter()
                .enumerate()
                .map(|(idx, key)| {
                    let parts: Vec<&str> = key.split('|').collect();
                    if parts.len() >= 3 {
                        json!({
                            "index": idx,
                            "component": parts[0],
                            "level": parts[1],
                            "kind": parts[2].trim(),
                            "details": if parts.len() > 3 { parts[3].trim() } else { "" },
                            "raw_key": key
                        })
                    } else {
                        json!({
                            "index": idx,
                            "raw_key": key
                        })
                    }
                })
                .collect();

            let unique2: Vec<Value> = results
                .unique_to_log2
                .iter()
                .enumerate()
                .map(|(idx, key)| {
                    let parts: Vec<&str> = key.split('|').collect();
                    if parts.len() >= 3 {
                        json!({
                            "index": idx,
                            "component": parts[0],
                            "level": parts[1],
                            "kind": parts[2].trim(),
                            "details": if parts.len() > 3 { parts[3].trim() } else { "" },
                            "raw_key": key
                        })
                    } else {
                        json!({
                            "index": idx,
                            "raw_key": key
                        })
                    }
                })
                .collect();

            standard_output["unique_to_log1"] = Value::Array(unique1);
            standard_output["unique_to_log2"] = Value::Array(unique2);
        }

        // Add shared comparisons
        let mut comparisons_array = Vec::new();

        // Group comparisons by key
        let mut current_key = String::new();
        let mut current_group = Vec::new();

        for comparison in &results.shared_comparisons {
            // Filter out comparisons without differences if diff_only is set
            if options.diff_only
                && comparison.json_differences.is_empty()
                && comparison.text1.is_none()
            {
                continue;
            }

            if comparison.key != current_key {
                if !current_group.is_empty() {
                    // Add the previous group
                    let key_entry = self.format_key_group_standard(&current_key, &current_group);
                    comparisons_array.push(key_entry);
                    current_group = Vec::new();
                }
                current_key = comparison.key.clone();
            }
            current_group.push(comparison);
        }

        // Add the last group if it exists
        if !current_group.is_empty() {
            let key_entry = self.format_key_group_standard(&current_key, &current_group);
            comparisons_array.push(key_entry);
        }

        standard_output["comparisons"] = Value::Array(comparisons_array);

        standard_output
    }

    fn format_key_group_standard(&self, key: &str, comparisons: &[&LogComparison]) -> Value {
        let parts: Vec<&str> = key.split('|').collect();

        let key_info = if parts.len() >= 3 {
            json!({
                "component": parts[0],
                "level": parts[1],
                "kind": parts[2].trim(),
                "details": if parts.len() > 3 { parts[3].trim() } else { "" },
                "raw_key": key
            })
        } else {
            json!({
                "raw_key": key
            })
        };

        let comparison_values: Vec<Value> = comparisons
            .iter()
            .map(|comparison| {
                let diffs = self.format_json_differences_standard(&comparison.json_differences);
                json!({
                    "log1_index": comparison.log1_index,
                    "log2_index": comparison.log2_index,
                    "json_differences": diffs,
                    "text1": comparison.text1,
                    "text2": comparison.text2,
                    "log1_line": comparison.log1_line_number,
                    "log2_line": comparison.log2_line_number,
                    "diff_count": comparison.json_differences.len()
                })
            })
            .collect();

        json!({
            "key": key_info,
            "instances": comparison_values,
            "instance_count": comparisons.len()
        })
    }

    fn format_json_differences_standard(&self, differences: &[JsonDifference]) -> Value {
        let diffs: Vec<Value> = differences
            .iter()
            .map(|diff| {
                let change_type_str = match diff.change_type {
                    crate::comparator::ChangeType::Added => "added",
                    crate::comparator::ChangeType::Removed => "removed",
                    crate::comparator::ChangeType::Modified => "modified",
                };
                json!({
                    "path": diff.path,
                    "value1": diff.value1,
                    "value2": diff.value2,
                    "change_type": change_type_str
                })
            })
            .collect();
        Value::Array(diffs)
    }

    /// Adds unique logs in compact format
    fn add_unique_logs_compact(&mut self, unique_to_log1: &[String], unique_to_log2: &[String]) {
        let unique1: Vec<Value> = unique_to_log1
            .iter()
            .enumerate()
            .map(|(idx, key)| {
                let parts: Vec<&str> = key.split('|').collect();
                if parts.len() >= 3 {
                    json!({
                        "i": idx,               // index
                        "c": parts[0],          // component
                        "l": parts[1],          // level
                        "k": parts[2].trim(),   // kind
                        "d": if parts.len() > 3 { parts[3].trim() } else { "" }, // details
                        "r": key                // raw_key
                    })
                } else {
                    json!({
                        "i": idx,               // index
                        "r": key                // raw_key
                    })
                }
            })
            .collect();

        let unique2: Vec<Value> = unique_to_log2
            .iter()
            .enumerate()
            .map(|(idx, key)| {
                let parts: Vec<&str> = key.split('|').collect();
                if parts.len() >= 3 {
                    json!({
                        "i": idx,               // index
                        "c": parts[0],          // component
                        "l": parts[1],          // level
                        "k": parts[2].trim(),   // kind
                        "d": if parts.len() > 3 { parts[3].trim() } else { "" }, // details
                        "r": key                // raw_key
                    })
                } else {
                    json!({
                        "i": idx,               // index
                        "r": key                // raw_key
                    })
                }
            })
            .collect();

        self.output["u1"] = Value::Array(unique1);
        self.output["u2"] = Value::Array(unique2);
    }

    /// Adds comparisons in compact format
    fn add_comparisons_compact(
        &mut self,
        comparisons: &[LogComparison],
        options: &ComparisonOptions,
    ) {
        let mut current_key = String::new();
        let mut current_group = Vec::new();

        for comparison in comparisons {
            // Filter out comparisons without differences if diff_only is set
            if options.diff_only
                && comparison.json_differences.is_empty()
                && comparison.text1.is_none()
            {
                continue;
            }

            if comparison.key != current_key {
                if !current_group.is_empty() {
                    // Add the previous group
                    self.add_key_group_compact(&current_key, &current_group);
                    current_group = Vec::new();
                }
                current_key = comparison.key.clone();
            }
            current_group.push(comparison);
        }

        // Add the last group if it exists
        if !current_group.is_empty() {
            self.add_key_group_compact(&current_key, &current_group);
        }
    }

    /// Creates a JSON group for comparisons with the same key in compact format
    fn add_key_group_compact(&mut self, key: &str, comparisons: &[&LogComparison]) {
        let parts: Vec<&str> = key.split('|').collect();

        let key_info = if parts.len() >= 3 {
            json!({
                "c": parts[0],                  // component
                "l": parts[1],                  // level
                "k": parts[2].trim(),           // kind
                "d": if parts.len() > 3 { parts[3].trim() } else { "" }, // details
                "r": key                        // raw_key
            })
        } else {
            json!({
                "r": key                        // raw_key
            })
        };

        // Group differences by path
        let mut path_groups: HashMap<String, Vec<(&JsonDifference, usize, usize)>> = HashMap::new();

        // Collect all differences by path
        for comparison in comparisons.iter() {
            for diff in &comparison.json_differences {
                let entry = path_groups.entry(diff.path.clone()).or_default();
                entry.push((diff, comparison.log1_index, comparison.log2_index));
            }
        }

        // Create comparison instances with references to differences
        let comparison_values: Vec<Value> = comparisons
            .iter()
            .map(|comparison| {
                json!({
                    "l1": comparison.log1_index, // log1_index
                    "l2": comparison.log2_index, // log2_index
                    "t1": comparison.text1,  // text1
                    "t2": comparison.text2,  // text2
                    "ln1": comparison.log1_line_number, // log1_line_number
                    "ln2": comparison.log2_line_number, // log2_line_number
                    "dc": comparison.json_differences.len() // diff_count
                })
            })
            .collect();

        // Create path-grouped differences
        let mut differences = Vec::new();
        for (path, diffs) in path_groups {
            let mut values1 = Vec::new();
            let mut values2 = Vec::new();
            let mut indexes = Vec::new();

            for (diff, log1_idx, log2_idx) in diffs {
                values1.push(diff.value1.clone());
                values2.push(diff.value2.clone());
                indexes.push(json!([log1_idx, log2_idx]));
            }

            differences.push(json!({
                "p": path,           // path
                "v1": values1,       // value1 array
                "v2": values2,       // value2 array
                "i": indexes         // indexes of comparisons
            }));
        }

        let key_entry = json!({
            "k": key_info,           // key information
            "i": comparison_values,  // instances
            "ic": comparisons.len(), // instance_count
            "d": differences         // path-grouped differences
        });

        // Add to the comparisons array
        let mut comparisons_array = self.output["c"].as_array().unwrap().clone();
        comparisons_array.push(key_entry);
        self.output["c"] = Value::Array(comparisons_array);
    }
}

/// Generates JSON representation of comparison results for LLM consumption
pub fn generate_json_output(results: &ComparisonResults, options: &ComparisonOptions) -> String {
    let mut formatter = JsonFormatter::new();
    let json_value = formatter.format_results(results, options);

    // Format JSON based on options
    if options.compact_mode {
        // For compact mode, use to_string instead of to_string_pretty to save more space
        serde_json::to_string(&json_value).unwrap_or_else(|_| "Error formatting JSON".to_string())
    } else if options.readable_mode {
        // For readable mode, use to_string_pretty to format with newlines and proper indentation
        serde_json::to_string_pretty(&json_value)
            .unwrap_or_else(|_| "Error formatting JSON".to_string())
    } else {
        // Standard format uses pretty printing
        serde_json::to_string_pretty(&json_value)
            .unwrap_or_else(|_| "Error formatting JSON".to_string())
    }
}
