mod direction;

use clap::{Parser, Subcommand, ValueEnum};
pub use direction::Direction;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable text output (default)
    Text,
    /// JSON output for LLM consumption
    Json,
}

/// A tool to analyze and compare two log files containing JSON objects
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Compare two log files and show differences between JSON objects
    Compare {
        /// First log file
        #[arg(index = 1, required = true)]
        file1: PathBuf,

        /// Second log file
        #[arg(index = 2, required = true)]
        file2: PathBuf,

        /// Filter logs by component (e.g. "core-universal", "socket")
        #[arg(long)]
        component: Option<String>,

        /// Filter logs by log level (e.g. "INFO", "ERROR")
        #[arg(short, long)]
        level: Option<String>,

        /// Filter logs by containing a specific text
        #[arg(long)]
        contains: Option<String>,

        /// Filter logs by communication direction
        #[arg(long)]
        direction: Option<Direction>,

        /// Show only differences, skip matching objects
        #[arg(short, long)]
        diff_only: bool,

        /// Path to output file for the differences
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Show full JSON objects, not just the differences
        #[arg(short, long)]
        full: bool,

        /// Output format (text or json)
        #[arg(short = 'F', long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,

        /// Use compact mode for JSON output (shorter keys, optimized structure)
        #[arg(short = 'c', long)]
        compact: bool,
    },
    /// List all components, event types, and log levels in a log file
    Info {
        /// Log file to analyze
        #[arg(index = 1, required = true)]
        file: PathBuf,
    },
}

pub fn cli_parse() -> Cli {
    Cli::parse()
}
