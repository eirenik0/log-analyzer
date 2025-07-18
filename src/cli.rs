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

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ColorMode {
    /// Auto-detect color support (default)
    Auto,
    /// Always use colors
    Always,
    /// Never use colors
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, ValueEnum, Default)]
pub enum SortOrder {
    /// Sort by timestamp (default)
    #[default]
    Time,
    /// Sort by component name
    Component,
    /// Sort by log level severity
    Level,
    /// Sort by event/message type
    Type,
    /// Sort by difference count
    DiffCount,
}

/// A tool to analyze and compare two log files containing JSON objects
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(name = "log-analyzer")]
pub struct Cli {
    /// Output format (text or json)
    #[arg(short = 'F', long, value_enum, default_value_t = OutputFormat::Text, global = true, group = "output_options", env = "FORMAT")]
    pub format: OutputFormat,

    /// Use compact mode for output (shorter keys, optimized structure)
    #[arg(
        short = 'c',
        long,
        global = true,
        group = "output_options",
        env = "COMPACT"
    )]
    pub compact: bool,

    /// Path to output file for results
    #[arg(short, long, global = true, env = "OUTPUT_FILE")]
    pub output: Option<PathBuf>,

    /// Control color output (auto, always, never)
    #[arg(long, value_enum, default_value_t = ColorMode::Auto, global = true, env = "COLOR")]
    pub color: ColorMode,

    /// Increase verbosity level (can be used multiple times)
    #[arg(short, long, action = clap::ArgAction::Count, global = true, env = "VERBOSE")]
    pub verbose: u8,

    /// Be quiet, show only errors
    #[arg(short, long, global = true, env = "QUIET", conflicts_with = "verbose")]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Compare two log files and show differences between JSON objects
    #[command(alias = "cmp")]
    Compare {
        /// First log file
        #[arg(required = true)]
        file1: PathBuf,

        /// Second log file
        #[arg(required = true)]
        file2: PathBuf,

        /// Filter logs by component (e.g. "core-universal", "socket")
        #[arg(short = 'C', long, group = "include_filters", env = "COMPONENT")]
        component: Option<String>,

        /// Exclude logs by component (e.g. "legacy", "debug")
        #[arg(
            long = "exclude-component",
            group = "exclude_filters",
            env = "EXCLUDE_COMPONENT"
        )]
        exclude_component: Option<String>,

        /// Filter logs by log level (e.g. "INFO", "ERROR")
        #[arg(short = 'l', long, group = "include_filters", env = "LEVEL")]
        level: Option<String>,

        /// Exclude logs by log level (e.g. "DEBUG", "TRACE")
        #[arg(
            long = "exclude-level",
            group = "exclude_filters",
            env = "EXCLUDE_LEVEL"
        )]
        exclude_level: Option<String>,

        /// Filter logs by containing a specific text
        #[arg(short = 't', long, group = "include_filters", env = "CONTAINS")]
        contains: Option<String>,

        /// Exclude logs containing a specific text
        #[arg(long = "exclude-text", group = "exclude_filters", env = "EXCLUDE_TEXT")]
        exclude_text: Option<String>,

        /// Filter logs by communication direction (Incoming or Outgoing)
        #[arg(short = 'd', long, group = "include_filters", env = "DIRECTION")]
        direction: Option<Direction>,

        /// Show only differences, skip matching objects
        #[arg(short = 'D', long, group = "display_options")]
        diff_only: bool,

        /// Show full JSON objects, not just the differences
        #[arg(short, long, group = "display_options")]
        full: bool,

        /// Sort output by given field
        #[arg(short = 's', long, value_enum, default_value_t = SortOrder::Time, group = "sorting", env = "SORT_BY")]
        sort_by: SortOrder,
    },

    /// Compare two log files showing only differences (shortcut for compare --diff-only)
    Diff {
        /// First log file
        #[arg(required = true)]
        file1: PathBuf,

        /// Second log file
        #[arg(required = true)]
        file2: PathBuf,

        /// Filter logs by component (e.g. "core-universal", "socket")
        #[arg(short = 'C', long, group = "include_filters", env = "COMPONENT")]
        component: Option<String>,

        /// Exclude logs by component (e.g. "legacy", "debug")
        #[arg(
            long = "exclude-component",
            group = "exclude_filters",
            env = "EXCLUDE_COMPONENT"
        )]
        exclude_component: Option<String>,

        /// Filter logs by log level (e.g. "INFO", "ERROR")
        #[arg(short = 'l', long, group = "include_filters", env = "LEVEL")]
        level: Option<String>,

        /// Exclude logs by log level (e.g. "DEBUG", "TRACE")
        #[arg(
            long = "exclude-level",
            group = "exclude_filters",
            env = "EXCLUDE_LEVEL"
        )]
        exclude_level: Option<String>,

        /// Filter logs by containing a specific text
        #[arg(short = 't', long, group = "include_filters", env = "CONTAINS")]
        contains: Option<String>,

        /// Exclude logs containing a specific text
        #[arg(long = "exclude-text", group = "exclude_filters", env = "EXCLUDE_TEXT")]
        exclude_text: Option<String>,

        /// Filter logs by communication direction (Incoming or Outgoing)
        #[arg(short = 'd', long, group = "include_filters", env = "DIRECTION")]
        direction: Option<Direction>,

        /// Show full JSON objects, not just the differences
        #[arg(short, long, group = "display_options")]
        full: bool,

        /// Sort output by given field
        #[arg(short = 's', long, value_enum, default_value_t = SortOrder::Time, group = "sorting", env = "SORT_BY")]
        sort_by: SortOrder,
    },
    /// List all components, event types, log levels, and detailed statistics in a log file
    #[command(alias = "i", alias = "inspect")]
    Info {
        /// Log file to analyze
        #[arg(required = true)]
        file: PathBuf,

        /// Show sample log messages for each component
        #[arg(short, long)]
        samples: bool,

        /// Display detailed JSON schema information for event payloads
        #[arg(short, long)]
        json_schema: bool,

        /// Filter logs by component (e.g. "core-universal", "socket")
        #[arg(short = 'C', long, env = "COMPONENT")]
        component: Option<String>,

        /// Filter logs by log level (e.g. "INFO", "ERROR")
        #[arg(short = 'l', long, env = "LEVEL")]
        level: Option<String>,

        /// Show payload statistics for each event/command/request type
        #[arg(short = 'p', long)]
        payloads: bool,

        /// Show detailed timeline analysis with event distribution
        #[arg(short = 't', long)]
        timeline: bool,
    },

    /// Generate LLM-friendly compact JSON output of differences (shortcut for compare --diff-only -F json -c)
    LlmDiff {
        /// First log file
        #[arg(required = true)]
        file1: PathBuf,

        /// Second log file
        #[arg(required = true)]
        file2: PathBuf,

        /// Filter logs by component (e.g. "core-universal", "socket")
        #[arg(short = 'C', long, group = "include_filters", env = "COMPONENT")]
        component: Option<String>,

        /// Exclude logs by component (e.g. "legacy", "debug")
        #[arg(
            long = "exclude-component",
            group = "exclude_filters",
            env = "EXCLUDE_COMPONENT"
        )]
        exclude_component: Option<String>,

        /// Filter logs by log level (e.g. "INFO", "ERROR")
        #[arg(short = 'l', long, group = "include_filters", env = "LEVEL")]
        level: Option<String>,

        /// Exclude logs by log level (e.g. "DEBUG", "TRACE")
        #[arg(
            long = "exclude-level",
            group = "exclude_filters",
            env = "EXCLUDE_LEVEL"
        )]
        exclude_level: Option<String>,

        /// Filter logs by containing a specific text
        #[arg(short = 't', long, group = "include_filters", env = "CONTAINS")]
        contains: Option<String>,

        /// Exclude logs containing a specific text
        #[arg(long = "exclude-text", group = "exclude_filters", env = "EXCLUDE_TEXT")]
        exclude_text: Option<String>,

        /// Filter logs by communication direction (Incoming or Outgoing)
        #[arg(short = 'd', long, group = "include_filters", env = "DIRECTION")]
        direction: Option<Direction>,

        /// Sort output by given field
        #[arg(short = 's', long, value_enum, default_value_t = SortOrder::Time, group = "sorting", env = "SORT_BY")]
        sort_by: SortOrder,

        /// Disable hiding of sensitive fields from JSON payloads (sanitization is enabled by default)
        #[arg(long)]
        no_sanitize: bool,
    },

    /// Generate LLM-friendly compact JSON output of a single log file with sanitized content
    #[command(visible_alias = "llm")]
    Process {
        /// Log file to process
        #[arg(required = true)]
        file: PathBuf,

        /// Filter logs by component (e.g. "core-universal", "socket")
        #[arg(short = 'C', long, group = "include_filters", env = "COMPONENT")]
        component: Option<String>,

        /// Exclude logs by component (e.g. "legacy", "debug")
        #[arg(
            long = "exclude-component",
            group = "exclude_filters",
            env = "EXCLUDE_COMPONENT"
        )]
        exclude_component: Option<String>,

        /// Filter logs by log level (e.g. "INFO", "ERROR")
        #[arg(short = 'l', long, group = "include_filters", env = "LEVEL")]
        level: Option<String>,

        /// Exclude logs by log level (e.g. "DEBUG", "TRACE")
        #[arg(
            long = "exclude-level",
            group = "exclude_filters",
            env = "EXCLUDE_LEVEL"
        )]
        exclude_level: Option<String>,

        /// Filter logs by containing a specific text
        #[arg(short = 't', long, group = "include_filters", env = "CONTAINS")]
        contains: Option<String>,

        /// Exclude logs containing a specific text
        #[arg(long = "exclude-text", group = "exclude_filters", env = "EXCLUDE_TEXT")]
        exclude_text: Option<String>,

        /// Filter logs by communication direction (Incoming or Outgoing)
        #[arg(short = 'd', long, group = "include_filters", env = "DIRECTION")]
        direction: Option<Direction>,

        /// Sort output by given field
        #[arg(short = 's', long, value_enum, default_value_t = SortOrder::Time, group = "sorting", env = "SORT_BY")]
        sort_by: SortOrder,

        /// Maximum number of log entries to include (0 = unlimited)
        #[arg(long, default_value = "100")]
        limit: usize,

        /// Disable hiding of sensitive fields from JSON payloads (sanitization is enabled by default)
        #[arg(long)]
        no_sanitize: bool,
    },
}

pub fn cli_parse() -> Cli {
    Cli::parse()
}
