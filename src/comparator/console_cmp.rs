use crate::ComparisonOptions;
use crate::comparator::ComparisonResults;
use crate::comparator::format_cmp::OutputFormatter;
use crate::comparator::format_cmp::format_comparison_results;
use colored::Colorize;
use comfy_table::Table;
use std::io;

// Helper function to determine if we should print based on verbosity level
pub fn should_print(options: &ComparisonOptions, required_level: u8) -> bool {
    if options.quiet {
        // In quiet mode, only print errors (level 0)
        required_level == 0
    } else {
        // Otherwise print if verbosity level is high enough
        options.verbosity >= required_level
    }
}

/// Console output formatter implementation with improved styling
pub struct ConsoleFormatter;

impl OutputFormatter for ConsoleFormatter {
    fn write_header(&mut self, text: &str) -> io::Result<()> {
        println!("{}", text.bold().bright_white().on_bright_black());
        Ok(())
    }

    fn write_divider(&mut self, char: &str, count: usize) -> io::Result<()> {
        println!("{}", char.repeat(count).bright_white());
        Ok(())
    }

    fn write_line(&mut self, text: &str) -> io::Result<()> {
        println!("{}", text);
        Ok(())
    }

    fn write_source_file1(&mut self, text: &str) -> io::Result<()> {
        println!("{}", text.cyan());
        Ok(())
    }

    fn write_source_file2(&mut self, text: &str) -> io::Result<()> {
        println!("{}", text.magenta());
        Ok(())
    }

    fn write_highlight(&mut self, text: &str) -> io::Result<()> {
        println!("{}", text.yellow().bold());
        Ok(())
    }

    fn write_label(&mut self, text: &str) -> io::Result<()> {
        println!("{}", text.bold().bright_blue());
        Ok(())
    }

    // New methods with semantic coloring
    fn write_success(&mut self, text: &str) -> io::Result<()> {
        println!("{}", text.green().bold());
        Ok(())
    }

    fn write_warning(&mut self, text: &str) -> io::Result<()> {
        println!("{}", text.yellow().bold());
        Ok(())
    }

    fn write_error(&mut self, text: &str) -> io::Result<()> {
        println!("{}", text.red().bold());
        Ok(())
    }

    fn write_info(&mut self, text: &str) -> io::Result<()> {
        println!("{}", text.bright_white().bold());
        Ok(())
    }

    fn write_table(&mut self, table: &Table) -> io::Result<()> {
        println!("{table}");
        Ok(())
    }
}

/// Formats and displays the comparison results to the console
pub fn display_comparison_results(results: &ComparisonResults, options: &ComparisonOptions) {
    let mut formatter = ConsoleFormatter;
    // Ignore the result since console output errors are rare and there's not much we can do about them
    let _ = format_comparison_results(&mut formatter, results, options);
}
