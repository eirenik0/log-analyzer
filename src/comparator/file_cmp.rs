use crate::ComparisonOptions;
use crate::comparator::ComparisonResults;
use crate::comparator::format_cmp::OutputFormatter;
use crate::comparator::format_cmp::format_comparison_results;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

/// File output formatter implementation with improved structure
pub struct FileFormatter {
    file: File,
}

impl FileFormatter {
    /// Creates a new file formatter with the given path
    pub fn new(path: &Path) -> io::Result<Self> {
        let file = File::create(path)?;
        Ok(Self { file })
    }
}

impl OutputFormatter for FileFormatter {
    fn write_header(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.file, "{}", text)
    }

    fn write_divider(&mut self, char: &str, count: usize) -> io::Result<()> {
        writeln!(self.file, "{}", char.repeat(count))
    }

    fn write_line(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.file, "{}", text)
    }

    fn write_source_file1(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.file, "[FILE1] {}", text)
    }

    fn write_source_file2(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.file, "[FILE2] {}", text)
    }

    fn write_highlight(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.file, "!!! {}", text)
    }

    fn write_label(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.file, "## {}", text)
    }

    // New methods for semantic organization
    fn write_success(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.file, "[SUCCESS] {}", text)
    }

    fn write_warning(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.file, "[WARNING] {}", text)
    }

    fn write_error(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.file, "[ERROR] {}", text)
    }

    fn write_info(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.file, "[INFO] {}", text)
    }
}

/// Writes comparison results to a file
pub fn write_comparison_results(
    results: &ComparisonResults,
    options: &ComparisonOptions,
    output_path: &Path,
) -> io::Result<()> {
    let mut formatter = FileFormatter::new(output_path)?;
    format_comparison_results(&mut formatter, results, options)
}
