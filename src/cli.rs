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
    /// Output format (text or json)
    #[arg(short = 'F', long, value_enum, default_value_t = OutputFormat::Text, global = true, group = "output_options")]
    pub format: OutputFormat,

    /// Use compact mode for output (shorter keys, optimized structure)
    #[arg(short = 'c', long, global = true, group = "output_options")]
    pub compact: bool,

    /// Path to output file for results
    #[arg(short, long, global = true)]
    pub output: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Compare two log files and show differences between JSON objects
    Compare {
        /// First log file
        #[arg(required = true)]
        file1: PathBuf,

        /// Second log file
        #[arg(required = true)]
        file2: PathBuf,

        /// Filter logs by component (e.g. "core-universal", "socket")
        #[arg(short = 'C', long, group = "filters")]
        component: Option<String>,

        /// Filter logs by log level (e.g. "INFO", "ERROR")
        #[arg(short = 'l', long, group = "filters")]
        level: Option<String>,

        /// Filter logs by containing a specific text
        #[arg(short = 't', long, group = "filters")]
        contains: Option<String>,

        /// Filter logs by communication direction (Incoming or Outgoing)
        #[arg(short = 'd', long, group = "filters")]
        direction: Option<Direction>,

        /// Show only differences, skip matching objects
        #[arg(short = 'D', long, group = "display_options")]
        diff_only: bool,

        /// Show full JSON objects, not just the differences
        #[arg(short, long, group = "display_options")]
        full: bool,
    },
    /// List all components, event types, and log levels in a log file
    Info {
        /// Log file to analyze
        #[arg(required = true)]
        file: PathBuf,
    },
}

pub fn cli_parse() -> Cli {
    Cli::parse()
}
