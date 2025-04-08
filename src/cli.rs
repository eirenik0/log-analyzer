mod direction;

use clap::{Parser, Subcommand};
pub use direction::Direction;
use std::path::PathBuf;

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
        #[arg(short = 'a', long)]
        file1: PathBuf,

        /// Second log file
        #[arg(short = 'b', long)]
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
    },
    /// List all components, event types, and log levels in a log file
    Info {
        /// Log file to analyze
        #[arg(short, long)]
        file: PathBuf,
    },
}

pub fn cli_parse() -> Cli {
    Cli::parse()
}
