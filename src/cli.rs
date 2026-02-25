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

#[derive(Debug, Clone, Copy, PartialEq, ValueEnum)]
pub enum OperationType {
    /// Request operations (send/receive)
    Request,
    /// Event operations (emit/receive)
    Event,
    /// Command operations (start/finish)
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, ValueEnum, Default)]
pub enum PerfSortOrder {
    /// Sort by duration (slowest first, default)
    #[default]
    Duration,
    /// Sort by operation count
    Count,
    /// Sort by operation name alphabetically
    Name,
}

/// A tool to analyze and compare two log files containing JSON objects
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(name = "log-analyzer")]
#[command(after_help = "FILTER EXPRESSION SYNTAX:
  --filter \"type:value [!type:value] ...\"

  Filter types (with aliases):
    component, comp, c    Filter by component name
    level, lvl, l         Filter by log level (INFO, ERROR, etc.)
    text, t               Filter by text in message
    direction, dir, d     Filter by direction (incoming/outgoing)

  Different filter types are AND-ed. Multiple values of the same type are OR-ed.
  Prefix with ! to exclude. Examples:
    --filter \"c:core-universal\"           Only core-universal component
    --filter \"l:ERROR\"                    Only ERROR level logs
    --filter \"c:core !l:DEBUG\"            Core component, exclude DEBUG
    --filter \"t:timeout d:incoming\"       Contains 'timeout', incoming only")]
pub struct Cli {
    /// Output format (text or json)
    #[arg(short = 'F', long, value_enum, default_value_t = OutputFormat::Text, global = true, group = "output_options", env = "LOG_ANALYZER_FORMAT")]
    pub format: OutputFormat,

    /// JSON output (LLM-friendly, implies --compact). Shorthand for -F json -c
    #[arg(
        short = 'j',
        long,
        global = true,
        group = "output_options",
        conflicts_with = "format",
        env = "LOG_ANALYZER_JSON"
    )]
    pub json: bool,

    /// Use compact mode for output (shorter keys, optimized structure)
    #[arg(
        short = 'c',
        long,
        global = true,
        group = "output_options",
        env = "LOG_ANALYZER_COMPACT"
    )]
    pub compact: bool,

    /// Filter expression (e.g., "c:core l:ERROR !t:timeout")
    #[arg(short = 'f', long, global = true, env = "LOG_ANALYZER_FILTER")]
    pub filter: Option<String>,

    /// Path to output file for results
    #[arg(short, long, global = true, env = "LOG_ANALYZER_OUTPUT")]
    pub output: Option<PathBuf>,

    /// Path to analyzer profile config (TOML)
    #[arg(long, global = true, env = "LOG_ANALYZER_CONFIG")]
    pub config: Option<PathBuf>,

    /// Control color output (auto, always, never)
    #[arg(long, value_enum, default_value_t = ColorMode::Auto, global = true, env = "LOG_ANALYZER_COLOR")]
    pub color: ColorMode,

    /// Increase verbosity level (can be used multiple times)
    #[arg(short, long, action = clap::ArgAction::Count, global = true, env = "LOG_ANALYZER_VERBOSE")]
    pub verbose: u8,

    /// Be quiet, show only errors
    #[arg(
        short,
        long,
        global = true,
        env = "LOG_ANALYZER_QUIET",
        conflicts_with = "verbose"
    )]
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

        /// Show only differences, skip matching objects
        #[arg(short = 'D', long)]
        diff_only: bool,

        /// Show full JSON objects, not just the differences
        #[arg(long)]
        full: bool,

        /// Sort output by given field
        #[arg(short = 's', long, value_enum, default_value_t = SortOrder::Time, env = "LOG_ANALYZER_SORT_BY")]
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

        /// Show full JSON objects, not just the differences
        #[arg(long)]
        full: bool,

        /// Sort output by given field
        #[arg(short = 's', long, value_enum, default_value_t = SortOrder::Time, env = "LOG_ANALYZER_SORT_BY")]
        sort_by: SortOrder,
    },

    /// List all components, event types, log levels, and detailed statistics in a log file
    #[command(alias = "i", alias = "inspect")]
    Info {
        /// One or more log files to analyze
        #[arg(required = true, num_args = 1..)]
        files: Vec<PathBuf>,

        /// Show sample log messages for each component
        #[arg(short, long)]
        samples: bool,

        /// Display detailed JSON schema information for event payloads
        #[arg(long)]
        json_schema: bool,

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

        /// Sort output by given field
        #[arg(short = 's', long, value_enum, default_value_t = SortOrder::Time, env = "LOG_ANALYZER_SORT_BY")]
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

        /// Sort output by given field
        #[arg(short = 's', long, value_enum, default_value_t = SortOrder::Time, env = "LOG_ANALYZER_SORT_BY")]
        sort_by: SortOrder,

        /// Maximum number of log entries to include (0 = unlimited)
        #[arg(long, default_value = "100")]
        limit: usize,

        /// Disable hiding of sensitive fields from JSON payloads (sanitization is enabled by default)
        #[arg(long)]
        no_sanitize: bool,
    },

    /// Analyze operation timing and identify performance bottlenecks
    Perf {
        /// One or more log files to analyze
        #[arg(required = true, num_args = 1..)]
        files: Vec<PathBuf>,

        /// Duration threshold in milliseconds for highlighting slow operations
        #[arg(long, default_value = "1000")]
        threshold_ms: u64,

        /// Number of slowest operations to display
        #[arg(long, default_value = "20")]
        top_n: usize,

        /// Show only orphan operations (started but never finished)
        #[arg(long)]
        orphans_only: bool,

        /// Filter by operation type (Request, Event, Command)
        #[arg(long)]
        op_type: Option<OperationType>,

        /// Sort results by field
        #[arg(short = 's', long, value_enum, default_value_t = PerfSortOrder::Duration)]
        sort_by: PerfSortOrder,
    },

    /// Analyze a log file and generate a TOML config profile
    #[command(alias = "gen-config")]
    GenerateConfig {
        /// Log file to analyze
        #[arg(required = true)]
        file: PathBuf,

        /// Name for the generated profile
        #[arg(long)]
        profile_name: Option<String>,

        /// Base template path or built-in name (base, custom-start, service-api, event-pipeline)
        #[arg(long)]
        template: Option<PathBuf>,
    },
}

impl Cli {
    /// Get the effective output format (handles -j shorthand)
    pub fn effective_format(&self) -> OutputFormat {
        if self.json {
            OutputFormat::Json
        } else {
            self.format
        }
    }

    /// Get the effective compact mode (handles -j shorthand)
    pub fn effective_compact(&self) -> bool {
        self.json || self.compact
    }
}

pub fn cli_parse() -> Cli {
    Cli::parse()
}
