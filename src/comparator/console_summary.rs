use crate::comparator::create_styled_table;
use crate::{LogEntry, LogEntryKind};
use chrono::{DateTime, Local};
use colored::ColoredString;
use colored::{Color, Colorize};
use comfy_table::Cell;
use std::collections::HashMap;

/// Displays all statistics about the logs with improved formatting and organization
///
/// # Arguments
/// * `logs` - The array of LogEntry objects to analyze
/// * `show_samples` - Whether to show sample log messages for each component
/// * `show_json_schema` - Whether to display JSON schema information for payloads
/// * `show_payload_stats` - Whether to show payload statistics
/// * `show_timeline` - Whether to show detailed timeline analysis
pub fn display_log_summary(
    logs: &[LogEntry],
    show_samples: bool,
    show_json_schema: bool,
    show_payload_stats: bool,
    show_timeline: bool,
) {
    // Count entries by type for better statistics
    let mut component_counts: HashMap<&str, usize> = HashMap::new();
    let mut level_counts: HashMap<&str, usize> = HashMap::new();
    let mut event_type_counts: HashMap<&str, usize> = HashMap::new();
    let mut command_counts: HashMap<&str, usize> = HashMap::new();
    let mut request_counts: HashMap<&str, usize> = HashMap::new();

    // For sample messages
    let mut component_samples: HashMap<&str, Vec<&LogEntry>> = HashMap::new();

    // For payload statistics
    let mut event_payload_sizes: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut command_payload_sizes: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut request_payload_sizes: HashMap<&str, Vec<usize>> = HashMap::new();

    // For JSON schema analysis
    let mut event_payload_keys: HashMap<&str, HashMap<String, usize>> = HashMap::new();
    let mut command_payload_keys: HashMap<&str, HashMap<String, usize>> = HashMap::new();
    let mut request_payload_keys: HashMap<&str, HashMap<String, usize>> = HashMap::new();

    // For timeline analysis
    let mut timestamps: Vec<DateTime<Local>> = Vec::new();
    let mut component_timeline: HashMap<&str, Vec<DateTime<Local>>> = HashMap::new();

    // Collect unique components, event types, commands, etc. with counts
    for log in logs {
        // Basic counts
        *component_counts.entry(&log.component).or_insert(0) += 1;
        *level_counts.entry(&log.level).or_insert(0) += 1;

        // Add to component samples (limited to 3 samples per component)
        if show_samples {
            let samples = component_samples.entry(&log.component).or_default();
            if samples.len() < 3 {
                samples.push(log);
            }
        }

        // Add timestamp for timeline analysis
        if show_timeline {
            timestamps.push(log.timestamp);
            component_timeline
                .entry(&log.component)
                .or_default()
                .push(log.timestamp);
        }

        match &log.kind {
            LogEntryKind::Event {
                event_type,
                payload,
                ..
            } => {
                *event_type_counts.entry(event_type).or_insert(0) += 1;

                // Collect payload sizes if requested
                if show_payload_stats && payload.is_some() {
                    let payload_size = serde_json::to_string(payload.as_ref().unwrap())
                        .map(|s| s.len())
                        .unwrap_or(0);
                    event_payload_sizes
                        .entry(event_type)
                        .or_default()
                        .push(payload_size);
                }

                // Collect JSON schema information
                if show_json_schema && payload.is_some() {
                    collect_json_keys(
                        payload.as_ref().unwrap(),
                        "",
                        event_payload_keys.entry(event_type).or_default(),
                    );
                }
            }
            LogEntryKind::Command {
                command, settings, ..
            } => {
                *command_counts.entry(command).or_insert(0) += 1;

                // Collect payload sizes if requested
                if show_payload_stats && settings.is_some() {
                    let payload_size = serde_json::to_string(settings.as_ref().unwrap())
                        .map(|s| s.len())
                        .unwrap_or(0);
                    command_payload_sizes
                        .entry(command)
                        .or_default()
                        .push(payload_size);
                }

                // Collect JSON schema information
                if show_json_schema && settings.is_some() {
                    collect_json_keys(
                        settings.as_ref().unwrap(),
                        "",
                        command_payload_keys.entry(command).or_default(),
                    );
                }
            }
            LogEntryKind::Request {
                request, payload, ..
            } => {
                *request_counts.entry(request).or_insert(0) += 1;

                // Collect payload sizes if requested
                if show_payload_stats && payload.is_some() {
                    let payload_size = serde_json::to_string(payload.as_ref().unwrap())
                        .map(|s| s.len())
                        .unwrap_or(0);
                    request_payload_sizes
                        .entry(request)
                        .or_default()
                        .push(payload_size);
                }

                // Collect JSON schema information
                if show_json_schema && payload.is_some() {
                    collect_json_keys(
                        payload.as_ref().unwrap(),
                        "",
                        request_payload_keys.entry(request).or_default(),
                    );
                }
            }
            LogEntryKind::Generic { .. } => {}
        }
    }

    // Calculate percentages for component and level distribution
    let total_entries = logs.len();

    // Display header
    println!("{}", "=".repeat(80).bright_white());
    println!("{}", "LOG SUMMARY REPORT".bold().bright_white());
    println!("{}", "=".repeat(80).bright_white());
    println!(
        "Total log entries: {}",
        total_entries.to_string().green().bold()
    );

    // Helper function to print sorted counts with percentages using styled tables
    let print_sorted_counts =
        |title: &str, counts: HashMap<&str, usize>, _color_fn: fn(&str) -> ColoredString| {
            // Skip if empty
            if counts.is_empty() {
                return;
            }

            // Convert to vec and sort by count (descending)
            let mut items: Vec<(&str, usize)> = counts.into_iter().collect();
            items.sort_by(|a, b| b.1.cmp(&a.1));

            println!("\n{}", title.bold());
            println!("{}", "-".repeat(80).bright_black());

            let mut table = create_styled_table(&["Name", "Count", "Percent", "Distribution"]);

            // Print each item with percentage and bar chart
            for (name, count) in items {
                let percentage = (count as f64 / total_entries as f64) * 100.0;
                let bar_length = (percentage.round() as usize).min(50);
                let bar = "█".repeat(bar_length);

                table.add_row(vec![
                    Cell::new(name),
                    Cell::new(count),
                    Cell::new(format!("{:>6.2}%", percentage)),
                    Cell::new(bar),
                ]);
            }

            println!("{table}");
        };

    // Display components with counts and percentages
    print_sorted_counts("LOG COMPONENTS", component_counts.clone(), |s| s.cyan());

    // Display log levels with counts and percentages
    print_sorted_counts("LOG LEVELS", level_counts, |s| {
        match s.to_lowercase().as_str() {
            "error" => s.red().bold(),
            "warn" | "warning" => s.yellow().bold(),
            "info" => s.green(),
            "debug" => s.bright_blue(),
            "trace" => s.bright_black(),
            _ => s.white(),
        }
    });

    // Display event types with counts
    print_sorted_counts("EVENT TYPES", event_type_counts.clone(), |s| s.yellow());

    // Display commands with counts
    print_sorted_counts("COMMANDS", command_counts.clone(), |s| s.magenta());

    // Display requests with counts
    print_sorted_counts("REQUESTS", request_counts.clone(), |s| s.bright_green());

    // Display timeline summary if we have timestamps in the logs
    if let Some((earliest, latest)) = get_time_range(logs) {
        println!("\n{}", "TIME RANGE".bold());
        println!("{}", "-".repeat(80).bright_black());
        println!("  Earliest: {}", earliest.to_string().cyan());
        println!("  Latest:   {}", latest.to_string().cyan());

        if let Ok(duration) = latest.signed_duration_since(earliest).to_std() {
            let duration_str = format_duration(duration);
            println!("  Span:     {}", duration_str.green());
        }

        // Enhanced timeline analysis if requested
        if show_timeline && timestamps.len() > 5 {
            display_timeline_analysis(&timestamps, &component_timeline);
        }
    }

    // Display component samples if requested
    if show_samples && !component_samples.is_empty() {
        println!("\n{}", "SAMPLE MESSAGES".bold());
        println!("{}", "-".repeat(80).bright_black());

        // Convert to vec and sort by count (using component_counts for ordering)
        let mut components: Vec<(&str, &Vec<&LogEntry>)> =
            component_samples.iter().map(|(k, v)| (*k, v)).collect();
        components.sort_by(|a, b| {
            let count_a = component_counts.get(a.0).unwrap_or(&0);
            let count_b = component_counts.get(b.0).unwrap_or(&0);
            count_b.cmp(count_a) // Sort by frequency (most frequent first)
        });

        for (component, samples) in components {
            println!(
                "\n  {} ({} entries):",
                component.cyan().bold(),
                component_counts.get(component).unwrap_or(&0)
            );

            for (i, sample) in samples.iter().enumerate() {
                let short_msg = if sample.message.len() > 100 {
                    format!("{}...", &sample.message[..97])
                } else {
                    sample.message.clone()
                };

                println!(
                    "    {}. [{}] {}",
                    (i + 1).to_string().bright_white(),
                    sample.level.as_str().color(get_level_color(&sample.level)),
                    short_msg
                );
            }
        }
    }

    // Display payload statistics if requested
    if show_payload_stats
        && (!event_payload_sizes.is_empty()
            || !command_payload_sizes.is_empty()
            || !request_payload_sizes.is_empty())
    {
        println!("\n{}", "PAYLOAD STATISTICS".bold());
        println!("{}", "-".repeat(80).bright_black());

        // Helper function to display payload stats for a specific type
        let display_payload_stats = |title: &str, stats_map: &HashMap<&str, Vec<usize>>| {
            if stats_map.is_empty() {
                return;
            }

            println!("\n  {}:", title.bright_white().bold());

            let mut table = create_styled_table(&["Name", "Count", "Avg (bytes)", "Min", "Max"]);

            // Convert to vec and sort by average size
            let mut items: Vec<(&str, &Vec<usize>)> =
                stats_map.iter().map(|(k, v)| (*k, v)).collect();
            items.sort_by(|a, b| {
                let avg_a = a.1.iter().sum::<usize>() as f64 / a.1.len() as f64;
                let avg_b = b.1.iter().sum::<usize>() as f64 / b.1.len() as f64;
                avg_b.partial_cmp(&avg_a).unwrap() // Sort by avg size (largest first)
            });

            for (name, sizes) in items {
                let (min, max, _sum, avg) = calculate_stats(sizes);
                let count = sizes.len();

                table.add_row(vec![
                    Cell::new(name),
                    Cell::new(count),
                    Cell::new(format!("{:.2}", avg)),
                    Cell::new(min),
                    Cell::new(max),
                ]);
            }

            println!("{table}");
        };

        display_payload_stats("EVENT PAYLOADS", &event_payload_sizes);
        display_payload_stats("COMMAND PAYLOADS", &command_payload_sizes);
        display_payload_stats("REQUEST PAYLOADS", &request_payload_sizes);
    }

    // Display JSON schema information if requested
    if show_json_schema
        && (!event_payload_keys.is_empty()
            || !command_payload_keys.is_empty()
            || !request_payload_keys.is_empty())
    {
        println!("\n{}", "JSON SCHEMA ANALYSIS".bold());
        println!("{}", "-".repeat(80).bright_black());

        // Helper function to display schema for a specific type
        let display_schema = |title: &str,
                              schema_map: &HashMap<&str, HashMap<String, usize>>,
                              occurrence_counts: &HashMap<&str, usize>,
                              name_color: fn(&str) -> ColoredString| {
            if schema_map.is_empty() {
                return;
            }

            println!("\n  {}:", title.bright_white().bold());

            // Convert to vec and sort by frequency
            let mut items: Vec<(&str, &HashMap<String, usize>)> =
                schema_map.iter().map(|(k, v)| (*k, v)).collect();
            items.sort_by(|a, b| {
                let count_a = occurrence_counts.get(a.0).unwrap_or(&0);
                let count_b = occurrence_counts.get(b.0).unwrap_or(&0);
                count_b.cmp(count_a) // Sort by frequency (most frequent first)
            });

            for (name, keys) in items {
                println!(
                    "\n    {} ({} occurrences):",
                    name_color(name).bold(),
                    occurrence_counts.get(name).unwrap_or(&0)
                );

                // Sort keys by occurrence count
                let mut sorted_keys: Vec<(&String, &usize)> = keys.iter().collect();
                sorted_keys.sort_by(|a, b| b.1.cmp(a.1));

                // Display top fields (max 10)
                let display_count = sorted_keys.len().min(10);
                for (i, (key, count)) in sorted_keys.iter().take(display_count).enumerate() {
                    println!(
                        "      {}. {} ({}/{})",
                        (i + 1).to_string().bright_white(),
                        key,
                        count.to_string().bright_white(),
                        occurrence_counts.get(name).unwrap_or(&0)
                    );
                }

                // If there are more fields than we displayed
                if sorted_keys.len() > display_count {
                    println!(
                        "      ... and {} more fields",
                        sorted_keys.len() - display_count
                    );
                }
            }
        };

        display_schema(
            "EVENT SCHEMAS",
            &event_payload_keys,
            &event_type_counts,
            |s| s.yellow(),
        );
        display_schema(
            "COMMAND SCHEMAS",
            &command_payload_keys,
            &command_counts,
            |s| s.magenta(),
        );
        display_schema(
            "REQUEST SCHEMAS",
            &request_payload_keys,
            &request_counts,
            |s| s.bright_green(),
        );
    }
}

/// Display detailed timeline analysis with distribution over time
fn display_timeline_analysis(
    timestamps: &[DateTime<Local>],
    component_timeline: &HashMap<&str, Vec<DateTime<Local>>>,
) {
    // Only proceed if we have enough timestamps
    if timestamps.len() < 5 {
        return;
    }

    // Find the earliest and latest timestamps
    let earliest = timestamps.iter().min().unwrap();
    let latest = timestamps.iter().max().unwrap();

    // Calculate time span and decide on appropriate bucket size
    let time_span = latest.signed_duration_since(*earliest).to_std().unwrap();
    let total_seconds = time_span.as_secs();

    // Determine appropriate bucket size based on time span
    let (bucket_size, bucket_unit) = if total_seconds < 60 {
        // Less than a minute, use 5-second buckets
        (std::time::Duration::from_secs(5), "5 sec")
    } else if total_seconds < 3600 {
        // Less than an hour, use 1-minute buckets
        (std::time::Duration::from_secs(60), "1 min")
    } else if total_seconds < 86400 {
        // Less than a day, use 10-minute buckets
        (std::time::Duration::from_secs(600), "10 min")
    } else {
        // More than a day, use 1-hour buckets
        (std::time::Duration::from_secs(3600), "1 hour")
    };

    // Calculate number of buckets
    let num_buckets = (time_span.as_secs() / bucket_size.as_secs()).max(1) as usize;

    // Create buckets for overall timeline
    let mut timeline_buckets = vec![0; num_buckets];

    // Fill buckets
    for timestamp in timestamps {
        let bucket_idx = calculate_bucket_index(earliest, timestamp, bucket_size, num_buckets);
        timeline_buckets[bucket_idx] += 1;
    }

    // Calculate max count for scaling
    let max_count = *timeline_buckets.iter().max().unwrap_or(&1);

    // Display timeline header
    println!(
        "\n  {}",
        "EVENT DISTRIBUTION OVER TIME".bright_white().bold()
    );
    println!("  (each bucket represents {})", bucket_unit.bright_black());
    println!("  {}", "-".repeat(70).bright_black());

    // Display overall timeline histogram
    for (i, count) in timeline_buckets.iter().enumerate() {
        let bar_length = ((count * 40) / max_count).max(1);
        let bar = "█".repeat(bar_length);

        // Calculate time for this bucket
        let bucket_time =
            *earliest + chrono::Duration::from_std(bucket_size.mul_f64(i as f64)).unwrap();
        let time_str = bucket_time.format("%H:%M:%S").to_string();

        println!(
            "  {}: {:4} events |{}",
            time_str.bright_blue(),
            count.to_string().bright_white(),
            bar.color(get_gradient_color(*count as f64 * 100.0 / max_count as f64))
        );
    }

    // Display component distribution
    println!(
        "\n  {}",
        "COMPONENT ACTIVITY DISTRIBUTION".bright_white().bold()
    );
    println!("  {}", "-".repeat(70).bright_black());

    // Sort components by total count
    let mut components: Vec<(&str, &Vec<DateTime<Local>>)> =
        component_timeline.iter().map(|(k, v)| (*k, v)).collect();
    components.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    // Display top 5 components
    for (name, timestamps) in components.iter().take(5) {
        println!("  {}: {} events", name.cyan(), timestamps.len());

        // Calculate component buckets
        let mut comp_buckets = vec![0; num_buckets];
        for timestamp in *timestamps {
            let bucket_idx = calculate_bucket_index(earliest, timestamp, bucket_size, num_buckets);
            comp_buckets[bucket_idx] += 1;
        }

        // Find max count for this component
        let comp_max = *comp_buckets.iter().max().unwrap_or(&1);

        // Display simplified histogram (max 5 buckets)
        let display_buckets = num_buckets.min(5);
        let step = if num_buckets > display_buckets {
            num_buckets / display_buckets
        } else {
            1
        };

        for i in (0..num_buckets).step_by(step) {
            if i < comp_buckets.len() {
                let count = comp_buckets[i];
                let bar_length = ((count * 20) / comp_max).max(if count > 0 { 1 } else { 0 });
                let bar = if bar_length > 0 {
                    "█".repeat(bar_length)
                } else {
                    "".to_string()
                };

                // Calculate time for this bucket
                let bucket_time =
                    *earliest + chrono::Duration::from_std(bucket_size.mul_f64(i as f64)).unwrap();
                let time_str = bucket_time.format("%H:%M:%S").to_string();

                if count > 0 {
                    println!(
                        "    {}: {:3} |{}",
                        time_str.bright_blue(),
                        count.to_string().bright_white(),
                        bar.color(get_gradient_color(count as f64 * 100.0 / comp_max as f64))
                    );
                }
            }
        }
        println!();
    }
}

/// Calculate bucket index for a timestamp
fn calculate_bucket_index(
    earliest: &DateTime<Local>,
    timestamp: &DateTime<Local>,
    bucket_size: std::time::Duration,
    num_buckets: usize,
) -> usize {
    let duration = timestamp.signed_duration_since(*earliest).to_std().unwrap();
    let bucket_idx = (duration.as_secs() / bucket_size.as_secs()) as usize;
    bucket_idx.min(num_buckets - 1) // Ensure we don't go out of bounds
}

/// Get color for a log level
fn get_level_color(level: &str) -> Color {
    match level.to_lowercase().as_str() {
        "error" => Color::Red,
        "warn" | "warning" => Color::Yellow,
        "info" => Color::Green,
        "debug" => Color::Blue,
        "trace" => Color::BrightBlack,
        _ => Color::White,
    }
}

/// Helper function to get a color from a gradient based on percentage
fn get_gradient_color(percentage: f64) -> Color {
    if percentage < 1.0 {
        // Very rare entries (use dark gray)
        Color::TrueColor {
            r: 100,
            g: 100,
            b: 100,
        }
    } else if percentage < 5.0 {
        // Uncommon entries (blue to cyan gradient)
        Color::TrueColor {
            r: 0,
            g: 180,
            b: 200,
        }
    } else if percentage < 20.0 {
        // Moderate entries (cyan to green gradient)
        Color::TrueColor {
            r: 0,
            g: 200,
            b: 100,
        }
    } else if percentage < 50.0 {
        // Common entries (green to yellow gradient)
        Color::TrueColor {
            r: 180,
            g: 200,
            b: 0,
        }
    } else {
        // Very common entries (yellow to red gradient)
        Color::TrueColor {
            r: 230,
            g: 150,
            b: 0,
        }
    }
}

/// Helper function to extract the time range from logs if available
fn get_time_range(logs: &[LogEntry]) -> Option<(DateTime<Local>, DateTime<Local>)> {
    let mut earliest: Option<DateTime<Local>> = None;
    let mut latest: Option<DateTime<Local>> = None;

    for log in logs {
        if earliest.is_none() || log.timestamp < earliest.unwrap() {
            earliest = Some(log.timestamp);
        }

        if latest.is_none() || log.timestamp > latest.unwrap() {
            latest = Some(log.timestamp);
        }
    }

    match (earliest, latest) {
        (Some(e), Some(l)) => Some((e, l)),
        _ => None,
    }
}

/// Helper function to format duration in a human-readable way
fn format_duration(duration: std::time::Duration) -> String {
    let total_seconds = duration.as_secs();

    if total_seconds < 60 {
        format!("{} seconds", total_seconds)
    } else if total_seconds < 3600 {
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        format!("{} minutes, {} seconds", minutes, seconds)
    } else if total_seconds < 86400 {
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        format!("{} hours, {} minutes", hours, minutes)
    } else {
        let days = total_seconds / 86400;
        let hours = (total_seconds % 86400) / 3600;
        format!("{} days, {} hours", days, hours)
    }
}

/// Helper function to collect JSON keys from a Value, recursively traversing objects
fn collect_json_keys(
    value: &serde_json::Value,
    prefix: &str,
    keys_map: &mut HashMap<String, usize>,
) {
    match value {
        serde_json::Value::Object(obj) => {
            for (key, val) in obj {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };

                // Increment the count for this path
                *keys_map.entry(path.clone()).or_insert(0) += 1;

                // Recursively collect keys from nested objects
                collect_json_keys(val, &path, keys_map);
            }
        }
        serde_json::Value::Array(arr) => {
            // For arrays, we just note the existence of an array at this path
            // and recursively process each element
            *keys_map.entry(format!("{}[]", prefix)).or_insert(0) += 1;

            for (idx, val) in arr.iter().enumerate() {
                // Only traverse deeper if not primitive types
                if val.is_object() || val.is_array() {
                    let path = format!("{}[{}]", prefix, idx);
                    collect_json_keys(val, &path, keys_map);
                }
            }
        }
        // For primitive types, we just record their existence at this path
        _ => {
            let type_name = match value {
                serde_json::Value::Null => "null",
                serde_json::Value::Bool(_) => "boolean",
                serde_json::Value::Number(_) => "number",
                serde_json::Value::String(_) => "string",
                _ => unreachable!(),
            };

            *keys_map
                .entry(format!("{} ({})", prefix, type_name))
                .or_insert(0) += 1;
        }
    }
}

/// Helper function to calculate statistics for a collection of values
fn calculate_stats(values: &[usize]) -> (usize, usize, usize, f64) {
    if values.is_empty() {
        return (0, 0, 0, 0.0);
    }

    let min = *values.iter().min().unwrap();
    let max = *values.iter().max().unwrap();
    let sum: usize = values.iter().sum();
    let avg = sum as f64 / values.len() as f64;

    (min, max, sum, avg)
}
