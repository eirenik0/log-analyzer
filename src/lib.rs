// Route report output through a CLI-scoped presentation layer.
macro_rules! report_print {
    ($($arg:tt)*) => { crate::output::print(format_args!($($arg)*)) };
}
macro_rules! report_eprintln {
    ($($arg:tt)*) => { std::eprintln!("{}", crate::output::diagnostic(&format!($($arg)*))) };
}
macro_rules! report_println {
    () => { crate::output::print(format_args!("\n")) };
    ($($arg:tt)*) => { crate::output::print(format_args!("{}\n", format_args!($($arg)*))) };
}

pub mod build_info;
pub mod cli;
pub mod comparator;
pub mod config;
pub mod config_generator;
pub mod errors;
pub mod event_rules;
pub mod evidence;
pub mod extract;
pub mod filter;
pub mod llm_processor;
pub mod normalize;
mod output;
pub mod parser;
pub mod perf_analyzer;
pub mod profile_validation;
mod report_budget;
pub mod search;
pub mod timeline;
pub mod trace;

pub use cli::{
    ColorMode, Commands, ErrorsSortBy, OutputFormat, SearchCountBy, SortOrder, cli_parse,
};
pub use comparator::{
    ComparisonOptions, compare_json, compare_logs, display_comparison_results, generate_json_output,
};
use comparator::{LogFilter, display_log_summary};
use errors::{
    ErrorReportLimits, ErrorsOptions, analyze_errors_with_config,
    format_bounded_errors_text_with_prefix, format_errors_json, format_errors_text,
};
use extract::{format_extract_json, format_extract_rows, format_extract_text};
use filter::{FilterExpression, print_filter_warnings, to_log_filter};
pub use parser::{
    LogEntry, LogEntryKind, ParseError, detect_log_format, parse_log_entry,
    parse_log_entry_with_config, parse_log_file, parse_log_file_with_config,
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

    report_println!("  session insights:");
    for level in visible_levels {
        let total = level.sessions.len();
        let completed = level.completed_count();
        let incomplete = level.incomplete_count();
        let status = if incomplete == 0 { "OK" } else { "WARN" };

        report_println!(
            "    {} ({} sessions): {} completed, {} incomplete [{}]",
            level.config.name,
            total,
            completed,
            incomplete,
            status
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
                report_println!(
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

    report_println!("\nProfile insights ({})", config.profile_name);
    print_session_insights(&insights.sessions);
    if !insights.unknown_components.is_empty() {
        report_println!(
            "  unknown components: {}",
            list_preview(&insights.unknown_components, 8)
        );
    }
    if !insights.unknown_commands.is_empty() {
        report_println!(
            "  unknown commands: {}",
            list_preview(&insights.unknown_commands, 8)
        );
    }
    if !insights.unknown_requests.is_empty() {
        report_println!(
            "  unknown requests: {}",
            list_preview(&insights.unknown_requests, 8)
        );
    }
}

fn write_output_file(
    path: &std::path::Path,
    content: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if output::defer_output_file() {
        return Ok(());
    }
    std::fs::write(path, output::format_report(content))
        .map_err(|e| format!("Failed to write output file '{}': {}", path.display(), e).into())
}

#[derive(serde::Serialize)]
struct AnalysisCoverage {
    files: Vec<parser::ParseCoverage>,
    parsed_entries: usize,
    filter_matches: usize,
    status: &'static str,
}

struct AnalysisFiles {
    inputs: Vec<Vec<LogEntry>>,
    coverage: AnalysisCoverage,
}

/// Parse every declared input before rejecting unavailable analysis. Keep per-file
/// vectors so comparisons never pair records from the wrong side.
fn read_analysis_files(
    files: &[std::path::PathBuf],
    config: &config::AnalyzerConfig,
    filter: &LogFilter,
    format: OutputFormat,
    output: Option<&std::path::Path>,
) -> Result<AnalysisFiles, Box<dyn std::error::Error>> {
    read_analysis_files_impl(files, config, filter, format, output, false)
}

fn read_analysis_files_impl(
    files: &[std::path::PathBuf],
    config: &config::AnalyzerConfig,
    filter: &LogFilter,
    format: OutputFormat,
    output: Option<&std::path::Path>,
    allow_unparsed: bool,
) -> Result<AnalysisFiles, Box<dyn std::error::Error>> {
    let mut inputs = Vec::new();
    let mut coverage = AnalysisCoverage {
        files: Vec::new(),
        parsed_entries: 0,
        filter_matches: 0,
        status: "parsed",
    };
    for file in files {
        let parsed = parser::parse_log_file_report(file, config)
            .map_err(|e| format!("Failed to parse log file '{}': {:?}", file.display(), e))?;
        output::observe_input(&parsed.coverage, &parsed.entries);
        output::observe_entries(&parsed.entries);
        coverage.parsed_entries += parsed.entries.len();
        coverage.filter_matches += parsed
            .entries
            .iter()
            .filter(|entry| filter.matches(entry))
            .count();
        inputs.push(parsed.entries);
        coverage.files.push(parsed.coverage);
    }
    for file in &coverage.files {
        for diagnostic in &file.normalization_diagnostics {
            report_eprintln!(
                "Normalization skipped {}:{} row {} field {}: {}",
                output::source_path(&file.file),
                diagnostic.line,
                output::source_path(&diagnostic.row_path),
                diagnostic.field,
                diagnostic.reason
            );
        }
    }
    coverage.status = if coverage
        .files
        .iter()
        .any(parser::ParseCoverage::is_unparsed)
    {
        "unparsed_input"
    } else if coverage.parsed_entries == 0 {
        "empty_input"
    } else if coverage.filter_matches == 0 {
        "zero_filter_matches"
    } else {
        "parsed"
    };
    if coverage.status == "unparsed_input" && !allow_unparsed {
        let rendered = render_analysis_report("", format, &coverage)?;
        report_print!("{rendered}");
        if let Some(path) = output {
            write_output_file(path, &rendered)?;
        }
        return Err("Nonempty input has no recognized log entries; inspect the selected parser/profile and rejected candidates".into());
    }
    Ok(AnalysisFiles { inputs, coverage })
}

fn read_cli_log_file(
    file: &std::path::Path,
    config: &config::AnalyzerConfig,
    filter: &LogFilter,
    format: OutputFormat,
    output: Option<&std::path::Path>,
) -> Result<Vec<LogEntry>, Box<dyn std::error::Error>> {
    let AnalysisFiles { mut inputs, .. } =
        read_analysis_files(&[file.to_path_buf()], config, filter, format, output)?;
    Ok(inputs.pop().unwrap())
}

fn read_analysis_inputs(
    files: &[std::path::PathBuf],
    config: &config::AnalyzerConfig,
    filter: &LogFilter,
    format: OutputFormat,
    output: Option<&std::path::Path>,
) -> Result<(Vec<LogEntry>, AnalysisCoverage), Box<dyn std::error::Error>> {
    let AnalysisFiles { inputs, coverage } =
        read_analysis_files(files, config, filter, format, output)?;
    let mut logs: Vec<_> = inputs.into_iter().flatten().collect();
    logs.sort_by_key(|entry| entry.timestamp);
    Ok((logs, coverage))
}

fn coverage_text(coverage: &AnalysisCoverage) -> String {
    use std::fmt::Write;
    let mut text = String::from("Parse coverage\n");
    for file in &coverage.files {
        for diagnostic in &file.normalization_diagnostics {
            let _ = writeln!(
                text,
                "  Normalization skipped line {} row {} field {}: {}",
                diagnostic.line,
                output::diagnostic(&diagnostic.row_path),
                output::diagnostic(&diagnostic.field),
                output::diagnostic(&diagnostic.reason)
            );
        }
        let parser = serde_json::to_value(file.selected_parser).expect("parser serializes");
        let _ = writeln!(
            text,
            "  {}: parser={}, profile={}, input={} bytes, parsed={} entries, rejected={} candidates",
            output::source_path(&file.file),
            parser.as_str().unwrap_or("unknown"),
            output::diagnostic(&file.profile),
            file.input_bytes,
            file.parsed_entries,
            file.rejected_candidates
        );
    }
    let _ = writeln!(
        text,
        "  Status: {}; parsed entries: {}; filter matches: {}\n",
        coverage.status, coverage.parsed_entries, coverage.filter_matches
    );
    text
}

fn render_analysis_report(
    report: &str,
    format: OutputFormat,
    coverage: &AnalysisCoverage,
) -> Result<String, Box<dyn std::error::Error>> {
    match format {
        OutputFormat::Text => Ok(format!("{}{report}", coverage_text(coverage))),
        OutputFormat::Json => {
            let mut value = if report.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str::<serde_json::Value>(report)?
            };
            value["coverage"] = serde_json::to_value(coverage)?;
            Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
        }
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut cli = cli_parse();
    cli.prepare_common_reports()?;
    // Capability schemas are static binary content, never user log data.
    let mut output_guard = output::OutputGuard::new(
        cli.redact && !matches!(&cli.command, Commands::Capabilities),
        &cli.mask_id,
        cli.effective_compact() || matches!(&cli.command, Commands::LlmDiff { .. }),
        (matches!(cli.effective_format(), OutputFormat::Json)
            && !matches!(&cli.command, Commands::GenerateConfig { .. }))
            || matches!(
                &cli.command,
                Commands::Process { .. }
                    | Commands::LlmDiff { .. }
                    | Commands::Schema { .. }
                    | Commands::Capabilities
                    | Commands::ValidateProfile { .. }
            ),
    );
    if cli.common_reports() {
        output::set_budget(report_budget::Policy::from_cli(&cli), cli.output.clone());
    }
    let result = run_with_cli(&cli);
    let result = if cli.redact {
        result.map_err(|error| output::diagnostic(&error.to_string()).into())
    } else {
        result
    };
    output_guard.finish().and(result)
}

fn run_with_cli(cli: &cli::Cli) -> Result<(), Box<dyn std::error::Error>> {
    if matches!(&cli.command, Commands::Capabilities) {
        let capabilities = build_info::capabilities();
        let rendered = if cli.effective_compact() {
            serde_json::to_string(&capabilities)?
        } else {
            serde_json::to_string_pretty(&capabilities)?
        };
        report_println!("{rendered}");
        if let Some(path) = &cli.output {
            write_output_file(path, &rendered)?;
        }
        return Ok(());
    }
    let analyzer_config = config::load_config(cli.config.as_deref(), cli.preset.as_deref())
        .map_err(|e| format!("Failed to load config: {}", e))?;
    output::set_metadata(
        build_info::metadata(&analyzer_config.profile_name),
        matches!(&cli.command, Commands::GenerateConfig { .. }),
    );
    output::set_evidence(evidence::Context::new(cli, &analyzer_config)?);
    let format = if matches!(
        &cli.command,
        Commands::Process { .. }
            | Commands::LlmDiff { .. }
            | Commands::Schema { .. }
            | Commands::ValidateProfile { .. }
    ) {
        OutputFormat::Json
    } else {
        cli.effective_format()
    };
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
        report_eprintln!("Verbosity level: {}", verbose);
        report_eprintln!("Color mode: {:?}", color_mode);
        if let Some(out_path) = output {
            report_eprintln!("Output will be written to: {}", out_path.display());
        }
        if let Some(ref filter_expr) = cli.filter {
            report_eprintln!("Filter: {}", filter_expr);
        }
        report_eprintln!("Config profile: {}", analyzer_config.profile_name);
        if let Some(config_path) = &cli.config {
            report_eprintln!("Config file: {}", config_path.display());
        }
        if let Some(preset) = &cli.preset {
            report_eprintln!("Config preset: {}", preset);
        }
    }

    // Build the filter from the global --filter expression
    let filter = build_filter(&cli.filter)?;

    match &cli.command {
        Commands::Capabilities => unreachable!("capabilities returned before config loading"),
        Commands::ValidateProfile {
            files,
            kind,
            purpose,
            expected,
        } => {
            let (expectations, expected_digest) =
                profile_validation::load_expectations(expected.as_deref())?;
            let AnalysisFiles {
                mut inputs,
                coverage,
            } = read_analysis_files_impl(
                files,
                &analyzer_config,
                &filter,
                format,
                output.as_deref(),
                true,
            )?;
            for (ordinal, entries) in inputs.iter_mut().enumerate() {
                for entry in entries {
                    entry.source_input_ordinal = Some(ordinal);
                }
            }
            let kind = match kind {
                cli::OperationType::Request => config::OperationKind::Request,
                cli::OperationType::Command => config::OperationKind::Command,
                cli::OperationType::Event => config::OperationKind::Event,
            };
            let mut report = profile_validation::analyze(
                &inputs,
                &analyzer_config,
                &filter,
                kind,
                *purpose,
                expectations.as_ref(),
                expected_digest,
            );
            if coverage.status != "parsed" {
                report["profile_validation"]["suitability"]["status"] =
                    serde_json::json!("insufficient_evidence");
                report["profile_validation"]["suitability"]["reason"] =
                    serde_json::json!(coverage.status);
            }
            let supported = report["profile_validation"]["suitability"]["status"] == "supported";
            report["coverage"] = serde_json::to_value(coverage)?;
            let rendered = serde_json::to_string_pretty(&report)?;
            report_println!("{rendered}");
            if let Some(path) = output {
                write_output_file(path, &rendered)?;
            }
            if !supported {
                return Err("Selected profile does not establish the requested analysis on this sample; inspect profile_validation diagnostics and expected results".into());
            }
        }
        Commands::Schema { file, samples } => {
            let preview = normalize::schema_preview(file, *samples as usize)?;
            let rendered = serde_json::to_string_pretty(&preview)?;
            report_println!("{rendered}");
            if let Some(path) = output {
                write_output_file(path, &rendered)?;
            }
        }

        Commands::Compare {
            file1,
            file2,
            diff_only,
            full,
            sort_by,
        } => {
            // Parse log files with proper error handling
            let AnalysisFiles { mut inputs, .. } = read_analysis_files(
                &[file1.clone(), file2.clone()],
                &analyzer_config,
                &filter,
                format,
                output.as_deref(),
            )?;
            let logs2 = inputs.pop().unwrap();
            let logs1 = inputs.pop().unwrap();

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
            let mut results = compare_logs(&logs1, &logs2, &filter, &options)
                .map_err(|e| format!("Comparison failed: {:?}", e))?;
            output::redact_comparison(&mut results);

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
                    report_println!("{}", json_output);
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
            let AnalysisFiles { mut inputs, .. } = read_analysis_files(
                &[file1.clone(), file2.clone()],
                &analyzer_config,
                &filter,
                format,
                output.as_deref(),
            )?;
            let logs2 = inputs.pop().unwrap();
            let logs1 = inputs.pop().unwrap();

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
            let mut results = compare_logs(&logs1, &logs2, &filter, &options)
                .map_err(|e| format!("Comparison failed: {:?}", e))?;
            output::redact_comparison(&mut results);

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
                    report_println!("{}", json_output);
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
            let AnalysisFiles { mut inputs, .. } = read_analysis_files(
                &[file1.clone(), file2.clone()],
                &analyzer_config,
                &filter,
                format,
                output.as_deref(),
            )?;
            let mut logs2 = inputs.pop().unwrap();
            let mut logs1 = inputs.pop().unwrap();

            // Apply sanitization if enabled (default behavior unless --no-sanitize is used)
            if !no_sanitize && !cli.redact {
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
            let mut results = compare_logs(&logs1, &logs2, &filter, &options)
                .map_err(|e| format!("Comparison failed: {:?}", e))?;
            output::redact_comparison(&mut results);

            // Output as JSON (fixed format for LlmDiff)
            let json_output = generate_json_output(&results, &options);
            report_println!("{}", json_output);
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
            let (logs, coverage) =
                read_analysis_inputs(files, &analyzer_config, &filter, format, output.as_deref())?;

            // Filter logs if filter is provided
            let filtered_logs: Vec<_> = if cli.filter.is_some() {
                logs.iter()
                    .filter(|log| filter.matches(log))
                    .cloned()
                    .collect()
            } else {
                logs
            };

            if matches!(format, OutputFormat::Json) {
                let mut levels = std::collections::BTreeMap::new();
                let mut components = std::collections::BTreeMap::new();
                for entry in &filtered_logs {
                    *levels.entry(&entry.level).or_insert(0usize) += 1;
                    *components.entry(&entry.component).or_insert(0usize) += 1;
                }
                let report = serde_json::json!({"info": {
                    "total_entries": filtered_logs.len(), "levels": levels, "components": components,
                }}).to_string();
                let rendered = render_analysis_report(&report, format, &coverage)?;
                report_print!("{rendered}");
                if let Some(path) = output {
                    write_output_file(path, &rendered)?;
                }
                return Ok(());
            }
            report_print!("{}", coverage_text(&coverage));

            // Display log summary with enhanced options
            display_log_summary(&filtered_logs, *samples, *json_schema, *payloads, *timeline);
            print_profile_insights(&filtered_logs, &analyzer_config);

            // Show filtering information if applied
            if let Some(ref filter_expr) = cli.filter {
                if !filtered_logs.is_empty() {
                    report_println!(
                        "\nShowing {} log entries after applying filter: {}",
                        filtered_logs.len(),
                        filter_expr
                    );
                } else {
                    report_println!("\nNo log entries match the filter: {}", filter_expr);
                }
            }

            report_println!("\nLog analysis completed successfully.");
        }
        Commands::Process {
            file,
            sort_by,
            limit,
            no_sanitize,
        } => {
            // Parse log file with proper error handling
            let logs =
                read_cli_log_file(file, &analyzer_config, &filter, format, output.as_deref())
                    .map_err(|e| {
                        format!("Failed to parse log file '{}': {:?}", file.display(), e)
                    })?;

            // Filter logs
            let mut filtered_logs: Vec<_> = logs
                .iter()
                .filter(|log| filter.matches(log))
                .cloned()
                .collect();

            llm_processor::sort_logs(&mut filtered_logs, *sort_by);

            output::prepare_process_entries(&mut filtered_logs);

            // Process logs for LLM consumption (sanitize by default, unless --no-sanitize is used)
            let llm_output = llm_processor::process_logs_for_llm_complete(
                &filtered_logs,
                *limit,
                !no_sanitize && !cli.redact,
                cli.common_reports(),
            );

            // Output as JSON
            match serde_json::to_string_pretty(&llm_output) {
                Ok(json) => {
                    report_println!("{}", json);
                    if let Some(path) = output {
                        write_output_file(path, &json)?;
                    }
                }
                Err(e) => report_eprintln!("Error serializing output: {}", e),
            }
        }
        Commands::Search {
            file,
            context,
            payloads,
            count_by,
        } => {
            let logs =
                read_cli_log_file(file, &analyzer_config, &filter, format, output.as_deref())
                    .map_err(|e| {
                        format!("Failed to parse log file '{}': {:?}", file.display(), e)
                    })?;
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

            report_print!("{rendered}");
            if let Some(path) = output {
                write_output_file(path, &rendered)?;
            }
        }
        Commands::Errors {
            files,
            top_n,
            warn,
            sessions,
            sort_by,
            bounded,
            complete: _,
            max_sample_chars,
            max_stack_frames,
            max_output_chars,
        } => {
            let (logs, coverage) =
                read_analysis_inputs(files, &analyzer_config, &filter, format, output.as_deref())?;
            let error_options = ErrorsOptions {
                top_n: *top_n,
                include_warn: *warn,
                show_sessions: *sessions,
                sort_by: *sort_by,
                file_count: files.len(),
                limits: if *bounded
                    || max_sample_chars.is_some()
                    || max_stack_frames.is_some()
                    || max_output_chars.is_some()
                {
                    let defaults = ErrorReportLimits::default();
                    Some(ErrorReportLimits {
                        max_sample_chars: max_sample_chars.unwrap_or(defaults.max_sample_chars),
                        max_stack_frames: max_stack_frames.unwrap_or(defaults.max_stack_frames),
                        max_output_chars: max_output_chars.unwrap_or(defaults.max_output_chars),
                    })
                } else {
                    None
                },
            };

            let mut report =
                analyze_errors_with_config(&logs, &filter, &analyzer_config, &error_options);
            output::prepare_errors(&mut report);
            let rendered = match format {
                OutputFormat::Text => {
                    if let Some(limits) = error_options.limits {
                        format_bounded_errors_text_with_prefix(
                            &report,
                            &error_options,
                            limits,
                            &output::diagnostic(&coverage_text(&coverage)),
                            &output::report_prefix(),
                        )
                    } else {
                        render_analysis_report(
                            &format_errors_text(&report, &error_options),
                            format,
                            &coverage,
                        )?
                    }
                }
                OutputFormat::Json => render_analysis_report(
                    &format_errors_json(&report, &error_options),
                    format,
                    &coverage,
                )?,
            };

            let rendered = if error_options.limits.is_some() {
                output::prepare_bounded_output(&rendered, matches!(format, OutputFormat::Json))
            } else {
                rendered
            };
            report_print!("{rendered}");
            if let Some(path) = output {
                write_output_file(path, &rendered)?;
            }
        }
        Commands::Extract {
            file,
            field,
            rows,
            expand_array,
        } => {
            let logs =
                read_cli_log_file(file, &analyzer_config, &filter, format, output.as_deref())
                    .map_err(|e| {
                        format!("Failed to parse log file '{}': {:?}", file.display(), e)
                    })?;
            let match_indices = collect_match_indices(&logs, &filter);

            let rendered = if *rows || field.len() > 1 || expand_array.is_some() {
                format_extract_rows(
                    file,
                    &logs,
                    &match_indices,
                    field,
                    expand_array.as_deref(),
                    matches!(format, OutputFormat::Json),
                )
            } else {
                match format {
                    OutputFormat::Text => format_extract_text(&logs, &match_indices, &field[0]),
                    OutputFormat::Json => {
                        format_extract_json(file, &logs, &match_indices, &field[0])
                    }
                }
            };

            report_print!("{rendered}");
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
            let (logs, coverage) =
                read_analysis_inputs(files, &analyzer_config, &filter, format, output.as_deref())?;

            // Convert op_type filter to string
            let op_type_filter = op_type.map(|t| match t {
                cli::OperationType::Request => "Request",
                cli::OperationType::Event => "Event",
                cli::OperationType::Command => "Command",
            });

            // Analyze performance
            let mut results = perf_analyzer::analyze_performance_with_config(
                &logs,
                &filter,
                op_type_filter,
                &analyzer_config,
            );

            let selected: Vec<_> = logs.iter().filter(|entry| filter.matches(entry)).collect();
            results.event_timeline = timeline::analyze(&selected, &analyzer_config.timeline)?;

            output::prepare_performance(&mut results);

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
                    output::prepare_performance_text(&text);
                    let text = render_analysis_report(&text, format, &coverage)?;
                    report_print!("{text}");
                    if let Some(path) = output {
                        write_output_file(path, &text)?;
                    }
                }
                OutputFormat::Json => {
                    let json = perf_analyzer::format_perf_results_json_with_options(
                        &results,
                        *threshold_ms,
                        *top_n,
                        *orphans_only,
                        *sort_by,
                    );
                    let json = render_analysis_report(&json, format, &coverage)?;
                    report_print!("{}", json);
                    if let Some(path) = output {
                        write_output_file(path, &json)?;
                    }
                }
            }
        }
        Commands::Trace { files, id, session } => {
            let (logs, _) =
                read_analysis_inputs(files, &analyzer_config, &filter, format, output.as_deref())?;

            let selector = if let Some(id) = id {
                TraceSelector::Id(id.clone())
            } else if let Some(session) = session {
                TraceSelector::Session(session.clone())
            } else {
                return Err("Trace requires either --id or --session".into());
            };

            output::register_selector(selector.value());
            let entries = collect_trace_entries(&logs, &filter, &selector);

            let event_timeline = timeline::analyze(&entries, &analyzer_config.timeline)?;

            match format {
                OutputFormat::Text => {
                    let mut text = format_trace_text(&entries, &selector);
                    if let Some(report) = &event_timeline {
                        text.push_str(&timeline::format_text(report));
                    }
                    report_print!("{text}");
                    if let Some(path) = output {
                        write_output_file(path, &text)?;
                    }
                }
                OutputFormat::Json => {
                    let mut report: serde_json::Value =
                        serde_json::from_str(&format_trace_json(&entries, &selector))?;
                    if let Some(timeline) = &event_timeline {
                        report["trace"]["event_timeline"] = serde_json::to_value(timeline)?;
                    }
                    let json = serde_json::to_string_pretty(&report)?;
                    report_println!("{}", json);
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

            output::set_metadata(build_info::metadata(&base_config.profile_name), true);
            let detected_formats: Vec<_> = files
                .iter()
                .filter_map(|file| detect_log_format(file, &base_config).ok())
                .collect();
            let (logs, _) = read_analysis_inputs(
                files,
                &base_config,
                &filter,
                OutputFormat::Text,
                output.as_deref(),
            )?;

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

            let mut generated = config_generator::generate_config(
                &logs,
                &base_config,
                &config_generator::GenerateConfigOptions { profile_name },
            );
            if let Some(first_format) = detected_formats.first().copied()
                && detected_formats
                    .iter()
                    .all(|format| *format == first_format)
            {
                generated.parser.format = first_format;
            }

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

            report_print!("{output_text}");
            if let Some(path) = output {
                write_output_file(path, &output_text)?;
            }
        }
    }

    Ok(())
}
