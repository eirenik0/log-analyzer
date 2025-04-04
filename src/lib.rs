pub mod cli;
pub mod comparator;
pub mod parser;

use crate::comparator::{LogFilter, display_log_summary};
pub use cli::{Commands, cli_parse};
pub use comparator::{ComparisonOptions, compare_json, compare_logs, display_comparison_results};
pub use parser::{LogEntry, LogEntryKind, ParseError, parse_log_entry, parse_log_file};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = cli_parse();

    match &cli.command {
        Commands::Compare {
            file1,
            file2,
            component,
            level,
            contains,
            diff_only,
            output,
            full,
        } => {
            // Parse log files with proper error handling - using {:?} for ParseError
            let logs1 = parse_log_file(file1)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file1.display(), e))?;

            let logs2 = parse_log_file(file2)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file2.display(), e))?;

            // Create filter with proper handling of Option<&str>
            let filter = LogFilter::new()
                .with_component(component.as_deref())
                .with_level(level.as_deref())
                .contains_text(contains.as_deref());

            // Create options with the correct method name
            let options = ComparisonOptions::new()
                .diff_only(*diff_only)
                .show_full_json(*full)
                .output_to_file(output.as_deref().map(|o| o.to_str().unwrap()));

            // Compare logs with proper error handling for ComparisonError
            let results = compare_logs(&logs1, &logs2, &filter, &options)
                .map_err(|e| format!("Comparison failed: {:?}", e))?;

            // Display results
            display_comparison_results(&results, &options);

            println!("\nComparison completed successfully.");
        }
        Commands::Info { file } => {
            // Parse log file with proper error handling
            let logs = parse_log_file(file)
                .map_err(|e| format!("Failed to parse log file '{}': {:?}", file.display(), e))?;

            // Display log summary
            display_log_summary(&logs);

            println!("\nLog analysis completed successfully.");
        }
    }

    Ok(())
}
