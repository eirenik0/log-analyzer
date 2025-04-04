use crate::comparator::format_cmp::{OutputFormatter, format_comparison_results};
use crate::comparator::{ComparisonOptions, ComparisonResults};
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

/// File output formatter implementation
pub struct FileFormatter {
    file: File,
}

impl FileFormatter {
    pub fn new(path: &Path) -> io::Result<Self> {
        Ok(FileFormatter {
            file: File::create(path)?,
        })
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
        // In file output, we don't have colors, so just write the plain text
        writeln!(self.file, "{}", text)
    }

    fn write_source_file2(&mut self, text: &str) -> io::Result<()> {
        // In file output, we don't have colors, so just write the plain text
        writeln!(self.file, "{}", text)
    }

    fn write_highlight(&mut self, text: &str) -> io::Result<()> {
        // In file output, we don't have colors, so just write the plain text
        writeln!(self.file, "{}", text)
    }

    fn write_label(&mut self, text: &str) -> io::Result<()> {
        // In file output, we don't have colors, so just write the plain text
        writeln!(self.file, "{}", text)
    }
}

/// Writes comparison results to a file with the same formatting as console output
pub fn write_results_to_file(
    results: &ComparisonResults,
    options: &ComparisonOptions,
    path: &Path,
) -> io::Result<()> {
    let mut formatter = FileFormatter::new(path)?;
    format_comparison_results(&mut formatter, results, options)
}
