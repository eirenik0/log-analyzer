pub mod cli;
pub mod comparator;
pub mod config;
pub mod config_generator;
pub mod extract;
pub mod filter;
pub mod llm_processor;
pub mod parser;
pub mod perf_analyzer;
pub mod search;
pub mod trace;

pub use cli::{ColorMode, Commands, OutputFormat, SearchCountBy, SortOrder, cli_parse};
pub use comparator::{
    ComparisonOptions, compare_json, compare_logs, display_comparison_results, generate_json_output,
};
use comparator::{LogFilter, display_log_summary};
use extract::{format_extract_json, format_extract_text};
use filter::{FilterExpression, print_filter_warnings, to_log_filter};
pub use parser::{
    LogEntry, LogEntryKind, ParseError, parse_log_entry, parse_log_entry_with_config,
    parse_log_file, parse_log_file_with_config,
};
use search::{
    collect_match_indices, format_search_count_json, format_search_count_text, format_search_json,
    format_search_text,
};
use trace::{TraceSelector, collect_trace_entries, format_trace_json, format_trace_text};

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

fn pluralize_label(label: &str, count: usize) -> String {
    if count == 1 || label.ends_with('s') {
        label.to_string()
    } else {
        format!("{label}s")
    }
}

fn json_value_inline(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<invalid-json>".to_string())
}

fn print_session_insights(insights: &config::SessionInsights) {
    let visible_levels: Vec<_> = insights
        .levels
        .iter()
        .filter(|level| !level.sessions.is_empty())
        .collect();
    if visible_levels.is_empty() {
        return;
    }

    println!("  session insights:");
    for level in visible_levels {
        let total = level.sessions.len();
        let completed = level.completed_count();
        let incomplete = level.incomplete_count();
        let status = if incomplete == 0 { "OK" } else { "WARN" };

        println!(
            "    {} ({} sessions): {} completed, {} incomplete [{}]",
            level.config.name, total, completed, incomplete, status
        );

        for field in &level.config.summary_fields {
            if field.is_empty() {
                continue;
            }

            let values: std::collections::BTreeSet<String> = level
                .sessions
                .values()
                .filter_map(|session| session.summary_fields.get(field))
                .map(json_value_inline)
                .collect();

            if values.len() == 1
                && level
                    .sessions
                    .values()
                    .all(|s| s.summary_fields.contains_key(field))
            {
                let value = values.iter().next().expect("one value");
                println!(
                    "      {{{}: {}}} across all {}",
                    field,
                    value,
                    pluralize_label(&level.config.name, total)
                );
            }
        }
    }
}

fn print_profile_insights(logs: &[LogEntry], config: &config::AnalyzerConfig) {
    if !config.has_profile_hints() {
        return;
    }

    let insights = config::analyze_profile(logs, config);

    if insights.unknown_components.is_empty()
        && insights.unknown_commands.is_empty()
        && insights.unknown_requests.is_empty()
        && insights.sessions.is_empty()
    {
        return;
    }

    println!("\nProfile insights ({})", config.profile_name);
    print_session_insights(&insights.sessions);
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
    analyzer_config: &config::AnalyzerConfig,
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
    let analyzer_config = config::load_config(cli.config.as_deref())
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
                        comparator::write_comparison_results(&results, &options, path).map_err(
                            |e| format!("Failed to write output file '{}': {}", path.display(), e),
                        )?;
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
                        comparator::write_comparison_results(&results, &options, path).map_err(
                            |e| format!("Failed to write output file '{}': {}", path.display(), e),
                        )?;
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
                llm_processor::sanitize_logs(&mut logs1);
                llm_processor::sanitize_logs(&mut logs2);
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
                llm_processor::process_logs_for_llm(&filtered_logs, *limit, !no_sanitize);

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
        Commands::Search {
            file,
            context,
            payloads,
            count_by,
        } => {
            let logs = parse_log_file_with_config(file, &analyzer_config)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file.display(), e))?;
            let match_indices = collect_match_indices(&logs, &filter);

            let rendered = if let Some(count_by) = count_by {
                match format {
                    OutputFormat::Text => {
                        format_search_count_text(&logs, &match_indices, *count_by)
                    }
                    OutputFormat::Json => {
                        format_search_count_json(file, &logs, &match_indices, *count_by)
                    }
                }
            } else {
                match format {
                    OutputFormat::Text => {
                        format_search_text(&logs, &match_indices, *context, *payloads)
                    }
                    OutputFormat::Json => {
                        format_search_json(file, &logs, &match_indices, *context, *payloads)
                    }
                }
            };

            print!("{rendered}");
            if let Some(path) = output {
                write_output_file(path, &rendered)?;
            }
        }
        Commands::Extract { file, field } => {
            let logs = parse_log_file_with_config(file, &analyzer_config)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file.display(), e))?;
            let match_indices = collect_match_indices(&logs, &filter);

            let rendered = match format {
                OutputFormat::Text => format_extract_text(&logs, &match_indices, field),
                OutputFormat::Json => format_extract_json(file, &logs, &match_indices, field),
            };

            print!("{rendered}");
            if let Some(path) = output {
                write_output_file(path, &rendered)?;
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
                cli::OperationType::Request => "Request",
                cli::OperationType::Event => "Event",
                cli::OperationType::Command => "Command",
            });

            // Analyze performance
            let results = perf_analyzer::analyze_performance_with_config(
                &logs,
                &filter,
                op_type_filter,
                &analyzer_config,
            );

            // Display results based on format
            match format {
                OutputFormat::Text => {
                    let text = perf_analyzer::format_perf_results_text(
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
                    let json = perf_analyzer::format_perf_results_json(&results);
                    println!("{}", json);
                    if let Some(path) = output {
                        write_output_file(path, &json)?;
                    }
                }
            }
        }
        Commands::Trace { files, id, session } => {
            let logs = parse_and_merge_log_files_with_config(files, &analyzer_config)?;

            let selector = if let Some(id) = id {
                TraceSelector::Id(id.clone())
            } else if let Some(session) = session {
                TraceSelector::Session(session.clone())
            } else {
                return Err("Trace requires either --id or --session".into());
            };

            let entries = collect_trace_entries(&logs, &filter, &selector);

            match format {
                OutputFormat::Text => {
                    let text = format_trace_text(&entries, &selector);
                    print!("{text}");
                    if let Some(path) = output {
                        write_output_file(path, &text)?;
                    }
                }
                OutputFormat::Json => {
                    let json = format_trace_json(&entries, &selector);
                    println!("{}", json);
                    if let Some(path) = output {
                        write_output_file(path, &json)?;
                    }
                }
            }
        }
        Commands::GenerateConfig {
            files,
            profile_name,
            template,
        } => {
            let base_config = if let Some(template_path) = template {
                if template_path.exists() {
                    config::load_config_from_path(template_path).map_err(|e| {
                        format!(
                            "Failed to load template config '{}': {}",
                            template_path.display(),
                            e
                        )
                    })?
                } else {
                    let template_name = template_path.to_string_lossy();
                    config::load_builtin_template(&template_name).ok_or_else(|| {
                        format!(
                            "Template '{}' not found as file path or built-in template. Built-ins: {}",
                            template_path.display(),
                            config::builtin_template_names().join(", ")
                        )
                    })?
                }
            } else {
                analyzer_config.clone()
            };

            let logs = parse_and_merge_log_files_with_config(files, &base_config)?;

            let profile_name = profile_name.clone().unwrap_or_else(|| {
                if files.len() == 1 {
                    files[0]
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .filter(|stem| !stem.is_empty())
                        .unwrap_or("generated-profile")
                        .to_string()
                } else {
                    "generated-profile".to_string()
                }
            });

            let generated = config_generator::generate_config(
                &logs,
                &base_config,
                &config_generator::GenerateConfigOptions { profile_name },
            );

            let body = toml::to_string_pretty(&generated)
                .map_err(|e| format!("Failed to serialize generated config: {}", e))?;
            let mut header = String::from("# Generated by log-analyzer generate-config\n");
            if files.len() == 1 {
                header.push_str(&format!("# Source: {}\n", files[0].display()));
            } else {
                header.push_str(&format!("# Sources ({}):\n", files.len()));
                for file in files {
                    header.push_str(&format!("# - {}\n", file.display()));
                }
            }
            header.push_str(&format!(
                "# Date: {}\n\n",
                chrono::Local::now().format("%Y-%m-%d")
            ));
            let output_text = format!("{header}{body}");

            print!("{output_text}");
            if let Some(path) = output {
                write_output_file(path, &output_text)?;
            }
        }
    }

    Ok(())
}
