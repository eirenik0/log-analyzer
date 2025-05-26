pub mod cli;
pub mod comparator;
pub mod parser;

use crate::comparator::{LogFilter, display_log_summary};
pub use cli::{ColorMode, Commands, Direction, OutputFormat, SortOrder, cli_parse};
pub use comparator::{
    ComparisonOptions, compare_json, compare_logs, display_comparison_results, generate_json_output,
};
pub use parser::{LogEntry, LogEntryKind, ParseError, parse_log_entry, parse_log_file};
use std::path::{Path, PathBuf};

struct CompareParams<'a> {
    file1: &'a Path,
    file2: &'a Path,
    component: &'a Option<String>,
    exclude_component: &'a Option<String>,
    level: &'a Option<String>,
    exclude_level: &'a Option<String>,
    contains: &'a Option<String>,
    exclude_text: &'a Option<String>,
    direction: &'a Option<Direction>,
    diff_only: bool,
    full: bool,
    format: OutputFormat,
    compact: bool,
    sort_by: SortOrder,
    verbose: u8,
    quiet: bool,
    output: &'a Option<PathBuf>,
}

fn handle_compare(params: CompareParams) -> Result<(), Box<dyn std::error::Error>> {
    // Parse log files with proper error handling - using {:?} for ParseError
    let logs1 = parse_log_file(params.file1).map_err(|e| {
        format!(
            "Failed to parse log file '{}': {:?}",
            params.file1.display(),
            e
        )
    })?;

    let logs2 = parse_log_file(params.file2).map_err(|e| {
        format!(
            "Failed to parse log file '{}': {:?}",
            params.file2.display(),
            e
        )
    })?;

    // Create filter with proper handling of Option<&str>
    let filter = LogFilter::new()
        .with_component(params.component.as_deref())
        .exclude_component(params.exclude_component.as_deref())
        .with_level(params.level.as_deref())
        .exclude_level(params.exclude_level.as_deref())
        .contains_text(params.contains.as_deref())
        .excludes_text(params.exclude_text.as_deref())
        .with_direction(params.direction);

    // Create options
    let options = ComparisonOptions::new()
        .diff_only(params.diff_only)
        .show_full_json(params.full)
        .compact_mode(params.compact)
        .readable_mode(true)
        .sort_by(params.sort_by)
        .verbosity(params.verbose)
        .quiet_mode(params.quiet)
        .output_to_file(params.output.as_deref().map(|o| o.to_str().unwrap()));

    // Compare logs with proper error handling for ComparisonError
    let results = compare_logs(&logs1, &logs2, &filter, &options)
        .map_err(|e| format!("Comparison failed: {:?}", e))?;

    // Display results in the selected format
    match params.format {
        OutputFormat::Text => display_comparison_results(&results, &options),
        OutputFormat::Json => println!("{}", generate_json_output(&results, &options)),
    }

    Ok(())
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = cli_parse();
    let format = cli.format;
    let compact = cli.compact;
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
    }

    match &cli.command {
        Commands::Compare {
            file1,
            file2,
            component,
            exclude_component,
            level,
            exclude_level,
            contains,
            exclude_text,
            direction,
            diff_only,
            full,
            sort_by,
        } => {
            handle_compare(CompareParams {
                file1,
                file2,
                component,
                exclude_component,
                level,
                exclude_level,
                contains,
                exclude_text,
                direction,
                diff_only: *diff_only,
                full: *full,
                format,
                compact,
                sort_by: *sort_by,
                verbose,
                quiet,
                output,
            })?;
        }
        Commands::Diff {
            file1,
            file2,
            component,
            exclude_component,
            level,
            exclude_level,
            contains,
            exclude_text,
            direction,
            full,
            sort_by,
        } => {
            // For Diff command, always use diff_only=true
            handle_compare(CompareParams {
                file1,
                file2,
                component,
                exclude_component,
                level,
                exclude_level,
                contains,
                exclude_text,
                direction,
                diff_only: true, // diff_only fixed to true
                full: *full,
                format,
                compact,
                sort_by: *sort_by,
                verbose,
                quiet,
                output,
            })?;
        }
        Commands::LlmDiff {
            file1,
            file2,
            component,
            exclude_component,
            level,
            exclude_level,
            contains,
            exclude_text,
            direction,
            sort_by,
        } => {
            // For LlmDiff command, customize several parameters
            handle_compare(CompareParams {
                file1,
                file2,
                component,
                exclude_component,
                level,
                exclude_level,
                contains,
                exclude_text,
                direction,
                diff_only: true,            // diff_only fixed to true
                full: false,                // full fixed to false
                format: OutputFormat::Json, // Fixed to JSON
                compact: true,              // compact fixed to true
                sort_by: *sort_by,
                verbose,
                quiet,
                output,
            })?;
        }
        Commands::Info {
            file,
            samples,
            json_schema,
            component,
            level,
            payloads,
            timeline,
        } => {
            // Parse log file with proper error handling
            let logs = parse_log_file(file)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file.display(), e))?;

            // Create filter based on provided options
            let filter = if component.is_some() || level.is_some() {
                Some(
                    LogFilter::new()
                        .with_component(component.as_deref())
                        .with_level(level.as_deref()),
                )
            } else {
                None
            };

            // Filter logs if needed
            let filtered_logs = if let Some(ref filter) = filter {
                logs.iter()
                    .filter(|log| filter.matches(log))
                    .cloned()
                    .collect::<Vec<_>>()
            } else {
                logs
            };

            // Display log summary with enhanced options
            display_log_summary(&filtered_logs, *samples, *json_schema, *payloads, *timeline);

            // Show filtering information if applied
            if let Some(ref _filter) = filter {
                if !filtered_logs.is_empty() {
                    println!(
                        "\nShowing {} log entries after applying filters.",
                        filtered_logs.len()
                    );

                    if component.is_some() {
                        println!("Component filter: {}", component.as_ref().unwrap());
                    }

                    if level.is_some() {
                        println!("Level filter: {}", level.as_ref().unwrap());
                    }
                } else {
                    println!("\nNo log entries match the specified filters.");
                }
            }

            println!("\nLog analysis completed successfully.");
        }
    }

    Ok(())
}
