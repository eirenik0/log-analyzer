use crate::ComparisonOptions;
use crate::comparator::ComparisonResults;
use crate::comparator::format_cmp::OutputFormatter;
use crate::comparator::format_cmp::format_comparison_results;
use comfy_table::Table;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

/// File output formatter implementation with improved structure
pub struct FileFormatter {
    file: File,
    content: Option<Vec<u8>>,
}

impl FileFormatter {
    /// Creates a new file formatter with the given path
    pub fn new(path: &Path) -> io::Result<Self> {
        let file = File::create(path)?;
        Ok(Self {
            file,
            content: crate::output::redaction_enabled().then(Vec::new),
        })
    }
    fn writer(&mut self) -> &mut dyn Write {
        if let Some(content) = self.content.as_mut() {
            content
        } else {
            &mut self.file
        }
    }
}

impl OutputFormatter for FileFormatter {
    fn write_header(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.writer(), "{}", text)
    }

    fn write_divider(&mut self, char: &str, count: usize) -> io::Result<()> {
        writeln!(self.writer(), "{}", char.repeat(count))
    }

    fn write_line(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.writer(), "{}", text)
    }

    fn write_source_file1(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.writer(), "[FILE1] {}", text)
    }

    fn write_source_file2(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.writer(), "[FILE2] {}", text)
    }

    fn write_highlight(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.writer(), "!!! {}", text)
    }

    fn write_label(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.writer(), "## {}", text)
    }

    // New methods for semantic organization
    fn write_success(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.writer(), "[SUCCESS] {}", text)
    }

    fn write_warning(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.writer(), "[WARNING] {}", text)
    }

    fn write_error(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.writer(), "[ERROR] {}", text)
    }

    fn write_info(&mut self, text: &str) -> io::Result<()> {
        writeln!(self.writer(), "[INFO] {}", text)
    }

    fn write_table(&mut self, table: &Table) -> io::Result<()> {
        writeln!(self.writer(), "{table}")
    }
}

/// Writes comparison results to a file
pub fn write_comparison_results(
    results: &ComparisonResults,
    options: &ComparisonOptions,
    output_path: &Path,
) -> io::Result<()> {
    let mut formatter = FileFormatter::new(output_path)?;
    format_comparison_results(&mut formatter, results, options)?;
    if let Some(content) = formatter.content {
        let content = String::from_utf8(content).expect("formatter writes UTF-8");
        formatter
            .file
            .write_all(crate::output::format_report(&content).as_bytes())
    } else {
        formatter
            .file
            .write_all(crate::output::text_metadata().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unredacted_comparison_lines_reach_disk_before_formatting_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stream.txt");
        let mut formatter = FileFormatter::new(&path).unwrap();
        formatter.write_line("first line").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first line\n");
        formatter.write_line("second line").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "first line\nsecond line\n"
        );
    }
}
