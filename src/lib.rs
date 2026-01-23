pub mod cli;
pub mod comparator;
pub mod filter;
pub mod llm_processor;
pub mod parser;
pub mod perf_analyzer;

use crate::comparator::{LogFilter, display_log_summary};
use crate::filter::{FilterExpression, print_filter_warnings, to_log_filter};
pub use cli::{ColorMode, Commands, OutputFormat, SortOrder, cli_parse};
pub use comparator::{
    ComparisonOptions, compare_json, compare_logs, display_comparison_results, generate_json_output,
};
pub use parser::{LogEntry, LogEntryKind, ParseError, parse_log_entry, parse_log_file};

/// Build a LogFilter from the --filter expression
fn build_filter(filter_expr: &Option<String>) -> Result<LogFilter, Box<dyn std::error::Error>> {
    if let Some(expr_str) = filter_expr {
        let expr = FilterExpression::parse(expr_str)
            .map_err(|e| format!("Invalid filter expression: {}", e))?;
        print_filter_warnings(&expr);
        Ok(to_log_filter(&expr))
    } else {
        Ok(LogFilter::new())
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = cli_parse();
    let format = cli.effective_format();
    let compact = cli.effective_compact();
    let output = &cli.output;
    let color_mode = cli.color;
    let verbose = cli.verbose;
    let quiet = cli.quiet;

    // Set up color handling based on user preference
    match color_mode {
        ColorMode::Always => {
            // Force colors on
            unsafe {
                std::env::set_var("CLICOLOR_FORCE", "1");
            }
        }
        ColorMode::Never => {
            // Disable colors
            unsafe {
                std::env::set_var("NO_COLOR", "1");
            }
        }
        ColorMode::Auto => {
            // Default behavior - let the terminal decide
        }
    }

    // If in verbose mode, display some diagnostic information
    if verbose > 0 && !quiet {
        eprintln!("Verbosity level: {}", verbose);
        eprintln!("Color mode: {:?}", color_mode);
        if let Some(out_path) = output {
            eprintln!("Output will be written to: {}", out_path.display());
        }
        if let Some(ref filter_expr) = cli.filter {
            eprintln!("Filter: {}", filter_expr);
        }
    }

    // Build the filter from the global --filter expression
    let filter = build_filter(&cli.filter)?;

    match &cli.command {
        Commands::Compare {
            file1,
            file2,
            diff_only,
            full,
            sort_by,
        } => {
            // Parse log files with proper error handling
            let logs1 = parse_log_file(file1)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file1.display(), e))?;

            let logs2 = parse_log_file(file2)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file2.display(), e))?;

            // Create options
            let options = ComparisonOptions::new()
                .diff_only(*diff_only)
                .show_full_json(*full)
                .compact_mode(compact)
                .readable_mode(true)
                .sort_by(*sort_by)
                .verbosity(verbose)
                .quiet_mode(quiet)
                .output_to_file(output.as_deref().map(|o| o.to_str().unwrap()));

            // Compare logs with proper error handling
            let results = compare_logs(&logs1, &logs2, &filter, &options)
                .map_err(|e| format!("Comparison failed: {:?}", e))?;

            // Display results in the selected format
            match format {
                OutputFormat::Text => display_comparison_results(&results, &options),
                OutputFormat::Json => println!("{}", generate_json_output(&results, &options)),
            }
        }
        Commands::Diff {
            file1,
            file2,
            full,
            sort_by,
        } => {
            // Parse log files with proper error handling
            let logs1 = parse_log_file(file1)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file1.display(), e))?;

            let logs2 = parse_log_file(file2)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file2.display(), e))?;

            // Create options with diff_only=true
            let options = ComparisonOptions::new()
                .diff_only(true)
                .show_full_json(*full)
                .compact_mode(compact)
                .readable_mode(true)
                .sort_by(*sort_by)
                .verbosity(verbose)
                .quiet_mode(quiet)
                .output_to_file(output.as_deref().map(|o| o.to_str().unwrap()));

            // Compare logs with proper error handling
            let results = compare_logs(&logs1, &logs2, &filter, &options)
                .map_err(|e| format!("Comparison failed: {:?}", e))?;

            // Display results in the selected format
            match format {
                OutputFormat::Text => display_comparison_results(&results, &options),
                OutputFormat::Json => println!("{}", generate_json_output(&results, &options)),
            }
        }
        Commands::LlmDiff {
            file1,
            file2,
            sort_by,
            no_sanitize,
        } => {
            // Parse log files with proper error handling
            let mut logs1 = parse_log_file(file1)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file1.display(), e))?;

            let mut logs2 = parse_log_file(file2)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file2.display(), e))?;

            // Apply sanitization if enabled (default behavior unless --no-sanitize is used)
            if !no_sanitize {
                crate::llm_processor::sanitize_logs(&mut logs1);
                crate::llm_processor::sanitize_logs(&mut logs2);
            }

            // Create options for LlmDiff with fixed parameters
            let options = ComparisonOptions::new()
                .diff_only(true)
                .show_full_json(false)
                .compact_mode(true)
                .readable_mode(true)
                .sort_by(*sort_by)
                .verbosity(verbose)
                .quiet_mode(quiet)
                .output_to_file(output.as_deref().map(|o| o.to_str().unwrap()));

            // Compare logs with proper error handling
            let results = compare_logs(&logs1, &logs2, &filter, &options)
                .map_err(|e| format!("Comparison failed: {:?}", e))?;

            // Output as JSON (fixed format for LlmDiff)
            println!("{}", generate_json_output(&results, &options));
        }
        Commands::Info {
            file,
            samples,
            json_schema,
            payloads,
            timeline,
        } => {
            // Parse log file with proper error handling
            let logs = parse_log_file(file)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file.display(), e))?;

            // Filter logs if filter is provided
            let filtered_logs: Vec<_> = if cli.filter.is_some() {
                logs.iter()
                    .filter(|log| filter.matches(log))
                    .cloned()
                    .collect()
            } else {
                logs
            };

            // Display log summary with enhanced options
            display_log_summary(&filtered_logs, *samples, *json_schema, *payloads, *timeline);

            // Show filtering information if applied
            if let Some(ref filter_expr) = cli.filter {
                if !filtered_logs.is_empty() {
                    println!(
                        "\nShowing {} log entries after applying filter: {}",
                        filtered_logs.len(),
                        filter_expr
                    );
                } else {
                    println!("\nNo log entries match the filter: {}", filter_expr);
                }
            }

            println!("\nLog analysis completed successfully.");
        }
        Commands::Process {
            file,
            sort_by: _,
            limit,
            no_sanitize,
        } => {
            // Parse log file with proper error handling
            let logs = parse_log_file(file)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file.display(), e))?;

            // Filter logs
            let filtered_logs: Vec<_> = logs
                .iter()
                .filter(|log| filter.matches(log))
                .cloned()
                .collect();

            // Process logs for LLM consumption (sanitize by default, unless --no-sanitize is used)
            let llm_output =
                crate::llm_processor::process_logs_for_llm(&filtered_logs, *limit, !no_sanitize);

            // Output as JSON
            match serde_json::to_string_pretty(&llm_output) {
                Ok(json) => println!("{}", json),
                Err(e) => eprintln!("Error serializing output: {}", e),
            }
        }
        Commands::Perf {
            file,
            threshold_ms,
            top_n,
            orphans_only,
            op_type,
            sort_by,
        } => {
            // Parse log file with proper error handling
            let logs = parse_log_file(file)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file.display(), e))?;

            // Convert op_type filter to string
            let op_type_filter = op_type.map(|t| match t {
                crate::cli::OperationType::Request => "Request",
                crate::cli::OperationType::Event => "Event",
                crate::cli::OperationType::Command => "Command",
            });

            // Analyze performance
            let results = crate::perf_analyzer::analyze_performance(&logs, &filter, op_type_filter);

            // Display results based on format
            match format {
                OutputFormat::Text => {
                    crate::perf_analyzer::display_perf_results(
                        &results,
                        *threshold_ms,
                        *top_n,
                        *orphans_only,
                        *sort_by,
                    );
                }
                OutputFormat::Json => {
                    let json = crate::perf_analyzer::format_perf_results_json(&results);
                    println!("{}", json);
                }
            }
        }
    }

    Ok(())
}
