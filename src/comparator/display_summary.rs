use crate::{LogEntry, LogEntryKind};
use chrono::{DateTime, Local};
use colored::ColoredString;
use colored::{Color, Colorize};
use std::collections::HashMap;

/// Displays all statistics about the logs with improved formatting and organization
pub fn display_log_summary(logs: &[LogEntry]) {
    // Count entries by type for better statistics
    let mut component_counts: HashMap<&str, usize> = HashMap::new();
    let mut level_counts: HashMap<&str, usize> = HashMap::new();
    let mut event_type_counts: HashMap<&str, usize> = HashMap::new();
    let mut command_counts: HashMap<&str, usize> = HashMap::new();
    let mut request_counts: HashMap<&str, usize> = HashMap::new();

    // Collect unique components, event types, commands, etc. with counts
    for log in logs {
        *component_counts.entry(&log.component).or_insert(0) += 1;
        *level_counts.entry(&log.level).or_insert(0) += 1;

        match &log.kind {
            LogEntryKind::Event { event_type, .. } => {
                *event_type_counts.entry(event_type).or_insert(0) += 1;
            }
            LogEntryKind::Command { command, .. } => {
                *command_counts.entry(command).or_insert(0) += 1;
            }
            LogEntryKind::Request { request, .. } => {
                *request_counts.entry(request).or_insert(0) += 1;
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

    // Helper function to print sorted counts with percentages
    let print_sorted_counts =
        |title: &str, counts: HashMap<&str, usize>, color_fn: fn(&str) -> ColoredString| {
            // Skip if empty
            if counts.is_empty() {
                return;
            }

            // Convert to vec and sort by count (descending)
            let mut items: Vec<(&str, usize)> = counts.into_iter().collect();
            items.sort_by(|a, b| b.1.cmp(&a.1));

            println!("\n{}", title.bold());
            println!("{}", "-".repeat(80).bright_black());

            // Calculate the longest item name for better alignment
            let max_name_len = items.iter().map(|(name, _)| name.len()).max().unwrap_or(10);
            let count_width = total_entries.to_string().len();

            // Print header row
            println!(
                "  {:<width$} │ {:<count_width$} │ {:<8} │ {}",
                "Name",
                "Count",
                "Percent",
                "Distribution",
                width = max_name_len,
                count_width = count_width,
            );
            println!(
                "  {}-│-{}-│-{}-│-{}",
                "-".repeat(max_name_len),
                "-".repeat(count_width),
                "-".repeat(8),
                "-".repeat(20)
            );

            // Print each item with percentage and bar chart
            for (name, count) in items {
                let percentage = (count as f64 / total_entries as f64) * 100.0;
                let bar_length = (percentage.round() as usize).min(50);
                let bar = "█".repeat(bar_length);

                println!(
                    "  {:<width$} │ {:<count_width$} │ {:>6.2}% │ {}",
                    color_fn(name),
                    count.to_string().bright_white(),
                    percentage,
                    bar.color(get_gradient_color(percentage)),
                    width = max_name_len,
                    count_width = count_width,
                );
            }
        };

    // Display components with counts and percentages
    print_sorted_counts("LOG COMPONENTS", component_counts, |s| s.cyan());

    // Display log levels with counts and percentages
    print_sorted_counts("LOG LEVELS", level_counts, |s| {
        match s.to_lowercase().as_str() {
            "error" => s.red().bold(),
            "warn" | "warning" => s.yellow().bold(),
            "info" => s.green(),
            "debug" => s.bright_blue(),
            "trace" => s.bright_black(),
            _ => s.white().into(),
        }
    });

    // Display event types with counts
    print_sorted_counts("EVENT TYPES", event_type_counts, |s| s.yellow());

    // Display commands with counts
    print_sorted_counts("COMMANDS", command_counts, |s| s.magenta());

    // Display requests with counts
    print_sorted_counts("REQUESTS", request_counts, |s| s.bright_green());

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

    if earliest.is_some() && latest.is_some() {
        Some((earliest.unwrap(), latest.unwrap()))
    } else {
        None
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
