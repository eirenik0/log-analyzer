use crate::ComparisonOptions;
use crate::comparator::ComparisonResults;
use crate::comparator::format_cmp::OutputFormatter;
use crate::comparator::format_cmp::format_comparison_results;
use colored::Colorize;
use std::io;

/// Console output formatter implementation
pub struct ConsoleFormatter;

impl OutputFormatter for ConsoleFormatter {
    fn write_header(&mut self, text: &str) -> io::Result<()> {
        println!("{}", text.bold().bright_white());
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
        println!("{}", text.yellow());
        Ok(())
    }

    fn write_label(&mut self, text: &str) -> io::Result<()> {
        println!("{}", text.bold());
        Ok(())
    }
}

/// Formats and displays the comparison results to the console
pub fn display_comparison_results(results: &ComparisonResults, options: &ComparisonOptions) {
    let mut formatter = ConsoleFormatter;
    // Ignore the result since console output errors are rare and there's not much we can do about them
    let _ = format_comparison_results(&mut formatter, results, options);
}
