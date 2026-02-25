pub mod cli;
pub mod comparator;
pub mod config;
pub mod config_generator;
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
pub use parser::{
    LogEntry, LogEntryKind, ParseError, parse_log_entry, parse_log_entry_with_config,
    parse_log_file, parse_log_file_with_config,
};

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

fn list_preview(values: &std::collections::BTreeSet<String>, max_items: usize) -> String {
    let mut preview: Vec<String> = values.iter().take(max_items).cloned().collect();
    if values.len() > max_items {
        preview.push(format!("... +{} more", values.len() - max_items));
    }
    preview.join(", ")
}

fn print_profile_insights(logs: &[LogEntry], config: &crate::config::AnalyzerConfig) {
    if !config.has_profile_hints() {
        return;
    }

    let insights = crate::config::analyze_profile(logs, config);

    if insights.unknown_components.is_empty()
        && insights.unknown_commands.is_empty()
        && insights.unknown_requests.is_empty()
        && insights.primary_sessions.is_empty()
        && insights.secondary_sessions.is_empty()
    {
        return;
    }

    println!("\nProfile insights ({})", config.profile_name);
    if !insights.primary_sessions.is_empty() {
        println!(
            "  primary sessions: {}",
            list_preview(&insights.primary_sessions, 8)
        );
    }
    if !insights.secondary_sessions.is_empty() {
        println!(
            "  secondary sessions: {}",
            list_preview(&insights.secondary_sessions, 8)
        );
    }
    if !insights.unknown_components.is_empty() {
        println!(
            "  unknown components: {}",
            list_preview(&insights.unknown_components, 8)
        );
    }
    if !insights.unknown_commands.is_empty() {
        println!(
            "  unknown commands: {}",
            list_preview(&insights.unknown_commands, 8)
        );
    }
    if !insights.unknown_requests.is_empty() {
        println!(
            "  unknown requests: {}",
            list_preview(&insights.unknown_requests, 8)
        );
    }
}

fn write_output_file(
    path: &std::path::Path,
    content: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::write(path, content)
        .map_err(|e| format!("Failed to write output file '{}': {}", path.display(), e).into())
}

fn parse_and_merge_log_files_with_config(
    files: &[std::path::PathBuf],
    analyzer_config: &crate::config::AnalyzerConfig,
) -> Result<Vec<LogEntry>, Box<dyn std::error::Error>> {
    let mut logs = Vec::new();

    for file in files {
        let mut parsed = parse_log_file_with_config(file, analyzer_config)
            .map_err(|e| format!("Failed to parse log file '{}': {:?}", file.display(), e))?;
        logs.append(&mut parsed);
    }

    logs.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    Ok(logs)
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = cli_parse();
    let analyzer_config = crate::config::load_config(cli.config.as_deref())
        .map_err(|e| format!("Failed to load config: {}", e))?;
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
        eprintln!("Config profile: {}", analyzer_config.profile_name);
        if let Some(config_path) = &cli.config {
            eprintln!("Config file: {}", config_path.display());
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
            let logs1 = parse_log_file_with_config(file1, &analyzer_config)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file1.display(), e))?;

            let logs2 = parse_log_file_with_config(file2, &analyzer_config)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file2.display(), e))?;

            // Create options
            let options = ComparisonOptions::new()
                .diff_only(*diff_only)
                .show_full_json(*full)
                .compact_mode(compact)
                .readable_mode(true)
                .sort_by(*sort_by)
                .verbosity(verbose)
                .quiet_mode(quiet);

            // Compare logs with proper error handling
            let results = compare_logs(&logs1, &logs2, &filter, &options)
                .map_err(|e| format!("Comparison failed: {:?}", e))?;

            // Display results in the selected format
            match format {
                OutputFormat::Text => {
                    display_comparison_results(&results, &options);
                    if let Some(path) = output {
                        crate::comparator::write_comparison_results(&results, &options, path)
                            .map_err(|e| {
                                format!("Failed to write output file '{}': {}", path.display(), e)
                            })?;
                    }
                }
                OutputFormat::Json => {
                    let json_output = generate_json_output(&results, &options);
                    println!("{}", json_output);
                    if let Some(path) = output {
                        write_output_file(path, &json_output)?;
                    }
                }
            }
        }
        Commands::Diff {
            file1,
            file2,
            full,
            sort_by,
        } => {
            // Parse log files with proper error handling
            let logs1 = parse_log_file_with_config(file1, &analyzer_config)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file1.display(), e))?;

            let logs2 = parse_log_file_with_config(file2, &analyzer_config)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file2.display(), e))?;

            // Create options with diff_only=true
            let options = ComparisonOptions::new()
                .diff_only(true)
                .show_full_json(*full)
                .compact_mode(compact)
                .readable_mode(true)
                .sort_by(*sort_by)
                .verbosity(verbose)
                .quiet_mode(quiet);

            // Compare logs with proper error handling
            let results = compare_logs(&logs1, &logs2, &filter, &options)
                .map_err(|e| format!("Comparison failed: {:?}", e))?;

            // Display results in the selected format
            match format {
                OutputFormat::Text => {
                    display_comparison_results(&results, &options);
                    if let Some(path) = output {
                        crate::comparator::write_comparison_results(&results, &options, path)
                            .map_err(|e| {
                                format!("Failed to write output file '{}': {}", path.display(), e)
                            })?;
                    }
                }
                OutputFormat::Json => {
                    let json_output = generate_json_output(&results, &options);
                    println!("{}", json_output);
                    if let Some(path) = output {
                        write_output_file(path, &json_output)?;
                    }
                }
            }
        }
        Commands::LlmDiff {
            file1,
            file2,
            sort_by,
            no_sanitize,
        } => {
            // Parse log files with proper error handling
            let mut logs1 = parse_log_file_with_config(file1, &analyzer_config)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file1.display(), e))?;

            let mut logs2 = parse_log_file_with_config(file2, &analyzer_config)
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
                .quiet_mode(quiet);

            // Compare logs with proper error handling
            let results = compare_logs(&logs1, &logs2, &filter, &options)
                .map_err(|e| format!("Comparison failed: {:?}", e))?;

            // Output as JSON (fixed format for LlmDiff)
            let json_output = generate_json_output(&results, &options);
            println!("{}", json_output);
            if let Some(path) = output {
                write_output_file(path, &json_output)?;
            }
        }
        Commands::Info {
            files,
            samples,
            json_schema,
            payloads,
            timeline,
        } => {
            // Parse and merge log files, then sort by timestamp for session-wide analysis
            let logs = parse_and_merge_log_files_with_config(files, &analyzer_config)?;

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
            print_profile_insights(&filtered_logs, &analyzer_config);

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
            let logs = parse_log_file_with_config(file, &analyzer_config)
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
                Ok(json) => {
                    println!("{}", json);
                    if let Some(path) = output {
                        write_output_file(path, &json)?;
                    }
                }
                Err(e) => eprintln!("Error serializing output: {}", e),
            }
        }
        Commands::Perf {
            files,
            threshold_ms,
            top_n,
            orphans_only,
            op_type,
            sort_by,
        } => {
            // Parse and merge log files, then sort by timestamp for cross-file pairing
            let logs = parse_and_merge_log_files_with_config(files, &analyzer_config)?;

            // Convert op_type filter to string
            let op_type_filter = op_type.map(|t| match t {
                crate::cli::OperationType::Request => "Request",
                crate::cli::OperationType::Event => "Event",
                crate::cli::OperationType::Command => "Command",
            });

            // Analyze performance
            let results = crate::perf_analyzer::analyze_performance_with_config(
                &logs,
                &filter,
                op_type_filter,
                &analyzer_config,
            );

            // Display results based on format
            match format {
                OutputFormat::Text => {
                    let text = crate::perf_analyzer::format_perf_results_text(
                        &results,
                        *threshold_ms,
                        *top_n,
                        *orphans_only,
                        *sort_by,
                    );
                    print!("{text}");
                    if let Some(path) = output {
                        write_output_file(path, &text)?;
                    }
                }
                OutputFormat::Json => {
                    let json = crate::perf_analyzer::format_perf_results_json(&results);
                    println!("{}", json);
                    if let Some(path) = output {
                        write_output_file(path, &json)?;
                    }
                }
            }
        }
        Commands::GenerateConfig {
            file,
            profile_name,
            template,
        } => {
            let base_config = if let Some(template_path) = template {
                if template_path.exists() {
                    crate::config::load_config_from_path(template_path).map_err(|e| {
                        format!(
                            "Failed to load template config '{}': {}",
                            template_path.display(),
                            e
                        )
                    })?
                } else {
                    let template_name = template_path.to_string_lossy();
                    crate::config::load_builtin_template(&template_name).ok_or_else(|| {
                        format!(
                            "Template '{}' not found as file path or built-in template. Built-ins: {}",
                            template_path.display(),
                            crate::config::builtin_template_names().join(", ")
                        )
                    })?
                }
            } else {
                analyzer_config.clone()
            };

            let logs = parse_log_file_with_config(file, &base_config)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file.display(), e))?;

            let profile_name = profile_name.clone().unwrap_or_else(|| {
                file.file_stem()
                    .and_then(|stem| stem.to_str())
                    .filter(|stem| !stem.is_empty())
                    .unwrap_or("generated-profile")
                    .to_string()
            });

            let generated = crate::config_generator::generate_config(
                &logs,
                &base_config,
                &crate::config_generator::GenerateConfigOptions { profile_name },
            );

            let body = toml::to_string_pretty(&generated)
                .map_err(|e| format!("Failed to serialize generated config: {}", e))?;
            let header = format!(
                "# Generated by log-analyzer generate-config\n# Source: {}\n# Date: {}\n\n",
                file.display(),
                chrono::Local::now().format("%Y-%m-%d")
            );
            let output_text = format!("{header}{body}");

            print!("{output_text}");
            if let Some(path) = output {
                write_output_file(path, &output_text)?;
            }
        }
    }

    Ok(())
}
