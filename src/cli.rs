mod direction;

use clap::{Parser, Subcommand, ValueEnum};
pub use direction::Direction;
use std::path::PathBuf;

#[derive(serde::Serialize, Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable text output (default)
    Text,
    /// JSON output for LLM consumption
    Json,
}

#[derive(serde::Serialize, Debug, Clone, Copy, ValueEnum)]
pub enum ColorMode {
    /// Auto-detect color support (default)
    Auto,
    /// Always use colors
    Always,
    /// Never use colors
    Never,
}

#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, ValueEnum, Default)]
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

#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, ValueEnum, Default)]
pub enum ProcessSortOrder {
    /// Sort by timestamp (earliest first)
    #[default]
    Time,
    /// Sort by component name
    Component,
    /// Sort by log level severity (highest first)
    Level,
    /// Sort by entry type (Command, Event, Generic, Request)
    Type,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum MappingScope {
    Project,
    User,
}

#[derive(serde::Serialize, Debug, clap::Subcommand)]
pub enum MappingAction {
    /// Show stored metadata and entry digests; never reads logs or creates files
    Inspect,
    /// Remember a currently selected assertion-validated profile; refuse an existing source key
    Remember {
        #[arg(required = true, num_args = 1..)]
        files: Vec<PathBuf>,
        #[arg(long, value_enum)]
        kind: OperationType,
        #[arg(long, value_enum, default_value = "timing")]
        purpose: crate::profile_validation::Purpose,
        #[arg(long)]
        expected: PathBuf,
        #[arg(long)]
        candidate_config: Vec<PathBuf>,
    },
    /// Replace the inspected entry only if its digest still matches
    Replace {
        #[arg(long)]
        entry_id: String,
        #[arg(long)]
        if_digest: String,
        #[arg(required = true, num_args = 1..)]
        files: Vec<PathBuf>,
        #[arg(long, value_enum)]
        kind: OperationType,
        #[arg(long, value_enum, default_value = "timing")]
        purpose: crate::profile_validation::Purpose,
        #[arg(long)]
        expected: PathBuf,
        #[arg(long)]
        candidate_config: Vec<PathBuf>,
    },
    /// Forget an inspected entry without requiring its sources or profile to exist
    Forget {
        #[arg(long)]
        entry_id: String,
        #[arg(long)]
        if_digest: String,
    },
}

#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, ValueEnum)]
pub enum OperationType {
    /// Request operations (send/receive)
    Request,
    /// Event operations (emit/receive)
    Event,
    /// Command operations (start/finish)
    Command,
}

#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, ValueEnum, Default)]
pub enum PerfSortOrder {
    /// Sort by duration (slowest first, default)
    #[default]
    Duration,
    /// Sort by operation count
    Count,
    /// Sort by operation name alphabetically
    Name,
}

#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum ErrorsSortBy {
    /// Sort by cluster count (highest first, default)
    #[default]
    Count,
    /// Sort by most recent occurrence time
    Time,
    /// Sort by estimated impact (affected sessions / blocking duration)
    Impact,
}

#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SearchCountBy {
    /// Total number of matching entries (grep -c style)
    Matches,
    /// Group by component name
    Component,
    /// Group by log level
    Level,
    /// Group by structured log type (event/request/command/generic + subtype)
    Type,
    /// Group by parsed JSON payload/settings (or <none>)
    Payload,
}

/// Analyze, search, compare, and diagnose structured logs
#[derive(serde::Serialize, Parser)]
#[command(author, version = env!("LOG_ANALYZER_BUILD_VERSION"), about, long_about = None)]
#[command(name = "log-analyzer")]
#[command(after_help = "FILTER EXPRESSION SYNTAX:
  --filter \"type:value [!type:value] ...\"

  Filter types (with aliases):
    component, comp, c    Filter by component name
    level, lvl, l         Filter by log level (INFO, ERROR, etc.)
    text, t               Filter by text in message
    direction, dir, d     Filter by direction (incoming/outgoing/unknown)
    <field-name>          Filter by structured key=value field (trace_id, actor_kind, ...)

  Different filter types are AND-ed. Multiple values of the same type are OR-ed.
  Prefix with ! to exclude. Examples:
    --filter \"c:core-universal\"           Only core-universal component
    --filter \"l:ERROR\"                    Only ERROR level logs
    --filter \"c:core !l:DEBUG\"            Core component, exclude DEBUG
    --filter \"t:timeout d:incoming\"       Contains 'timeout', incoming only
    --filter \"actor_kind:switch\"          Structured field filter on tracing/json logs")]
pub struct Cli {
    /// Bound the final JSON report by Unicode scalar characters (not model tokens); implies JSON
    #[arg(long, global = true, conflicts_with = "complete_output")]
    pub report_max_chars: Option<usize>,

    /// Bound the final JSON report by UTF-8 bytes, including metadata and newline; implies JSON
    #[arg(long, global = true, conflicts_with = "complete_output")]
    pub report_max_bytes: Option<usize>,

    /// Maximum presentation items per JSON page; implies JSON (0 allows no items)
    #[arg(long, global = true, conflicts_with = "complete_output")]
    pub report_max_items: Option<usize>,

    /// Resume unchanged input/profile/query/redaction from a previous report cursor; implies JSON
    #[arg(long, global = true, conflicts_with = "complete_output")]
    pub report_cursor: Option<String>,

    /// Return all evidence and analytic collections as JSON without legacy display clipping
    #[arg(long, global = true)]
    pub complete_output: bool,

    /// Redact sensitive report fields, message fragments, and URL query values
    #[arg(long, global = true)]
    pub redact: bool,

    /// Additionally mask this identifier field with stable replacements (repeatable)
    #[arg(long, global = true, requires = "redact")]
    pub mask_id: Vec<String>,

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
    #[arg(short = 'c', long, global = true, env = "LOG_ANALYZER_COMPACT")]
    pub compact: bool,

    /// Filter expression (e.g., "c:core l:ERROR !t:timeout")
    #[arg(short = 'f', long, global = true, env = "LOG_ANALYZER_FILTER")]
    pub filter: Option<String>,

    /// Path to output file for results
    #[arg(short, long, global = true, env = "LOG_ANALYZER_OUTPUT")]
    #[serde(skip_serializing)]
    pub output: Option<PathBuf>,

    /// Path to a TOML profile; supports `extends` inheritance
    ///
    /// A top-level `extends` names a built-in profile or a parent file resolved
    /// relative to the child profile. Tables merge with child values winning;
    /// arrays and scalars replace inherited values. Chains allow at most eight
    /// profiles, counting the child and every parent, including built-ins.
    /// Cycles and unknown parents are errors.
    #[arg(long, global = true, env = "LOG_ANALYZER_CONFIG")]
    #[serde(skip_serializing)]
    pub config: Option<PathBuf>,

    /// Built-in preset/profile to use instead of --config (base, eyes, custom-start, service-api, event-pipeline)
    #[arg(
        long,
        global = true,
        env = "LOG_ANALYZER_PRESET",
        conflicts_with = "config"
    )]
    pub preset: Option<String>,

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

#[derive(serde::Serialize, Subcommand)]
pub enum Commands {
    /// Print build identity, commands, formats, presets, report schemas and contract availability as JSON
    Capabilities,
    /// Preview JSON row types and JSON Pointer paths without processing or decoding strings
    Schema {
        #[serde(serialize_with = "crate::evidence::serialize_path")]
        file: PathBuf,
        #[arg(long, default_value="3", value_parser=clap::value_parser!(u32).range(1..=20))]
        samples: u32,
    },
    /// Validate the selected preset or editable TOML candidate against sample evidence (JSON)
    ValidateProfile {
        #[arg(required = true, num_args = 1..)]
        #[serde(serialize_with = "crate::evidence::serialize_paths")]
        files: Vec<PathBuf>,
        /// Requested operation kind; other kinds remain in the global inventory
        #[arg(long, value_enum)]
        kind: OperationType,
        /// Timing requires reliable paired boundaries; recognition only checks classification
        #[arg(long, value_enum, default_value = "timing")]
        purpose: crate::profile_validation::Purpose,
        /// Strict source-addressed positive/negative classification and pair assertions
        #[arg(long)]
        #[serde(serialize_with = "crate::evidence::serialize_optional_path")]
        expected: Option<PathBuf>,
    },
    /// Save a separate editable candidate and report sample validation plus missing domain knowledge (JSON)
    PrepareProfile {
        #[arg(required = true, num_args = 1..)]
        #[serde(serialize_with = "crate::evidence::serialize_paths")]
        files: Vec<PathBuf>,
        /// New candidate file; existing files are never replaced
        #[arg(long)]
        #[serde(serialize_with = "crate::evidence::serialize_path")]
        candidate_output: PathBuf,
        #[arg(long, value_enum)]
        kind: OperationType,
        #[arg(long, value_enum, default_value = "timing")]
        purpose: crate::profile_validation::Purpose,
        /// Independently established source-addressed assertions, never generated from the candidate
        #[arg(long)]
        #[serde(serialize_with = "crate::evidence::serialize_optional_path")]
        expected: Option<PathBuf>,
        /// Existing TOML or built-in starting point; defaults to base
        #[arg(long, conflicts_with_all = ["config", "preset"])]
        #[serde(serialize_with = "crate::evidence::serialize_optional_path")]
        template: Option<PathBuf>,
        #[arg(long)]
        profile_name: Option<String>,
        /// Maximum representative records per diagnostic array; omitted counts remain visible
        #[arg(long, default_value = "20", value_parser = clap::value_parser!(u32).range(1..=100))]
        witness_limit: u32,
    },
    /// Resolve profiles deterministically; automatic choices require supplied semantic assertions (JSON)
    ResolveProfile {
        #[arg(required = true, num_args = 1..)]
        #[serde(serialize_with = "crate::evidence::serialize_paths")]
        files: Vec<PathBuf>,
        #[arg(long, value_enum)]
        kind: OperationType,
        #[arg(long, value_enum, default_value = "timing")]
        purpose: crate::profile_validation::Purpose,
        /// Source-addressed expected facts; counts or profile labels do not authorize automatic selection
        #[arg(long)]
        #[serde(serialize_with = "crate::evidence::serialize_optional_path")]
        expected: Option<PathBuf>,
        /// Additional editable TOML alternatives (repeatable, at most 16); never activated or modified
        #[arg(long)]
        #[serde(serialize_with = "crate::evidence::serialize_paths")]
        candidate_config: Vec<PathBuf>,
        /// Read-only version-1 source/profile association; persistence management is separate
        #[arg(long)]
        #[serde(serialize_with = "crate::evidence::serialize_optional_path")]
        association: Option<PathBuf>,
        /// Project root for mapping lookup; defaults to the current directory
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Override project mapping registry; lookup never creates it
        #[arg(long)]
        project_mappings: Option<PathBuf>,
        /// Override user mapping registry; lookup never creates it
        #[arg(long)]
        user_mappings: Option<PathBuf>,
        /// Skip persistent mapping lookup
        #[arg(long)]
        no_mappings: bool,
    },
    /// Inspect or explicitly manage assertion-validated source/profile mappings (JSON)
    ProfileMappings {
        /// Project paths are root-relative; user mappings also bind absolute project context
        #[arg(long, value_enum, default_value = "project")]
        scope: MappingScope,
        /// Explicit project root; defaults to the current directory, with no ancestor search
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Override the selected scope's registry file
        #[arg(long)]
        registry: Option<PathBuf>,
        #[command(subcommand)]
        action: MappingAction,
    },
    /// Compare two log files and show differences between JSON objects
    #[command(alias = "cmp")]
    Compare {
        /// First log file
        #[arg(required = true)]
        #[serde(serialize_with = "crate::evidence::serialize_path")]
        file1: PathBuf,

        /// Second log file
        #[arg(required = true)]
        #[serde(serialize_with = "crate::evidence::serialize_path")]
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
        #[serde(serialize_with = "crate::evidence::serialize_path")]
        file1: PathBuf,

        /// Second log file
        #[arg(required = true)]
        #[serde(serialize_with = "crate::evidence::serialize_path")]
        file2: PathBuf,

        /// Show full JSON objects, not just the differences
        #[arg(long)]
        full: bool,

        /// Sort output by given field
        #[arg(short = 's', long, value_enum, default_value_t = SortOrder::Time, env = "LOG_ANALYZER_SORT_BY")]
        sort_by: SortOrder,
    },

    /// List components, event types, log levels, and statistics in one or more log files
    #[command(alias = "i", alias = "inspect")]
    Info {
        /// One or more log files to analyze
        #[arg(required = true, num_args = 1..)]
        #[serde(serialize_with = "crate::evidence::serialize_paths")]
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

    /// Search a log file and print matching entries (structured grep replacement)
    Search {
        /// Log file to search
        #[arg(required = true)]
        #[serde(serialize_with = "crate::evidence::serialize_path")]
        file: PathBuf,

        /// Show N matching context entries before/after each match
        #[arg(long, default_value_t = 0)]
        context: usize,

        /// Show parsed payload/settings JSON for each displayed entry
        #[arg(long)]
        payloads: bool,

        /// Count matches grouped by a structured field instead of printing entries
        #[arg(long, value_enum)]
        count_by: Option<SearchCountBy>,
    },

    /// Diagnose clustered errors/warnings and affected sessions across one or more logs
    Errors {
        /// One or more log files to analyze (supports shell-expanded globs)
        #[arg(required = true, num_args = 1..)]
        #[serde(serialize_with = "crate::evidence::serialize_paths")]
        files: Vec<PathBuf>,

        /// Number of clusters to show (0 = all)
        #[arg(long, default_value = "10")]
        top_n: usize,

        /// Include WARN entries (default is ERROR only)
        #[arg(long)]
        warn: bool,

        /// Show affected sessions for each cluster (cross-reference by component_id)
        #[arg(long)]
        sessions: bool,

        /// Bound samples and text output (defaults: 600 sample chars, 5 frames, 12000 output chars)
        #[arg(long, conflicts_with = "complete")]
        bounded: bool,

        /// Show complete details (default); combine with --top-n 0 for every cluster
        #[arg(long, conflicts_with_all = ["bounded", "max_sample_chars", "max_stack_frames", "max_output_chars"])]
        complete: bool,

        /// Maximum Unicode characters per sample/pattern (implies --bounded; 0 omits samples)
        #[arg(long)]
        max_sample_chars: Option<usize>,

        /// Maximum stack-frame lines per sample (implies --bounded; 0 omits frames)
        #[arg(long)]
        max_stack_frames: Option<usize>,

        /// Text report budget in Unicode characters including newlines; metadata may exceed it
        #[arg(long)]
        max_output_chars: Option<usize>,

        /// Sort clusters by field
        #[arg(short = 's', long, value_enum, default_value_t = ErrorsSortBy::Count)]
        sort_by: ErrorsSortBy,
    },

    /// Extract payload/settings fields as aggregate values or correlated rows
    Extract {
        /// Log file to analyze
        #[arg(required = true)]
        #[serde(serialize_with = "crate::evidence::serialize_path")]
        file: PathBuf,

        /// Field name/path to extract from payload JSON (supports dot paths, e.g. "foo.bar")
        #[arg(long, required = true)]
        field: Vec<String>,

        /// Emit one correlated row per matching entry (automatic for multiple fields)
        #[arg(long)]
        rows: bool,

        /// Expand one array at this dot path; selected fields are relative to each item
        #[arg(long)]
        expand_array: Option<String>,
    },

    /// Generate LLM-friendly compact JSON output of differences (shortcut for compare --diff-only -F json -c)
    LlmDiff {
        /// First log file
        #[arg(required = true)]
        #[serde(serialize_with = "crate::evidence::serialize_path")]
        file1: PathBuf,

        /// Second log file
        #[arg(required = true)]
        #[serde(serialize_with = "crate::evidence::serialize_path")]
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
        #[serde(serialize_with = "crate::evidence::serialize_path")]
        file: PathBuf,

        /// Sort output by given field
        #[arg(short = 's', long, value_enum, default_value_t = ProcessSortOrder::Time, env = "LOG_ANALYZER_SORT_BY")]
        sort_by: ProcessSortOrder,

        /// Maximum number of log entries to include (0 = unlimited)
        #[arg(long, default_value = "100")]
        limit: usize,

        /// Disable hiding of sensitive fields from JSON payloads (sanitization is enabled by default)
        #[arg(long)]
        no_sanitize: bool,
    },

    /// Analyze operation timing and report incomplete lifecycle evidence
    #[command(
        long_about = "Analyze operation timing and report incomplete lifecycle evidence. Shipped profiles use versioned event_rules for commands, requests and events, classified once before payload cleanup. Phases, identities and scopes are cached; transport direction does not imply a phase. Classification coverage counts selected parsed records before operation-type selection and display limits. Start-only, end-only, identity-only, conflicting and invalid evidence remains diagnostic; no completion elsewhere is required to report a start. Version-2 start rules may set end_expected = false when no end record is emitted; perf counts them as start_only_events rather than orphans, without measuring a duration. Operation-type filters retain these events in suppression totals. Legacy custom marker profiles retain their semantics. Record-field scopes over 4096 bytes use bounded prefix/length/digest keys; short values containing the reserved digest marker are encoded too. Digest collisions remain possible. Explicit event-rule scope mappings use the same encoding after validation and retain their input limit. See README for the supported lifecycle grammar and migration."
    )]
    Perf {
        /// One or more log files to analyze
        #[arg(required = true, num_args = 1..)]
        #[serde(serialize_with = "crate::evidence::serialize_paths")]
        files: Vec<PathBuf>,

        /// Duration threshold in milliseconds for highlighting slow operations
        #[arg(long, default_value = "1000", conflicts_with = "orphans_only")]
        threshold_ms: u64,

        /// Maximum rows per performance section (0 = unlimited)
        #[arg(long, default_value = "20")]
        top_n: usize,

        /// Show only orphan operations (started but never finished)
        #[arg(long)]
        orphans_only: bool,

        /// Filter by operation type (Request, Event, Command)
        #[arg(long)]
        op_type: Option<OperationType>,

        /// Sort completed operations and statistics by field
        #[arg(short = 's', long, value_enum, default_value_t = PerfSortOrder::Duration, conflicts_with = "orphans_only")]
        sort_by: PerfSortOrder,
    },

    /// Trace matching events by ID substring or session path (may include multiple lifecycles)
    Trace {
        /// One or more log files to search (supports shell-expanded globs)
        #[arg(required = true, num_args = 1..)]
        #[serde(serialize_with = "crate::evidence::serialize_paths")]
        files: Vec<PathBuf>,

        /// Correlation/request ID substring to trace (matches raw log lines)
        #[arg(long, conflicts_with = "session", required_unless_present = "session")]
        id: Option<String>,

        /// component_id/session path substring to trace (matches hierarchy)
        #[arg(long, conflicts_with = "id", required_unless_present = "id")]
        session: Option<String>,
    },

    /// Generate editable TOML from samples; check it with validate-profile before selecting it
    #[command(alias = "gen-config")]
    GenerateConfig {
        /// One or more log files to analyze (supports shell-expanded globs)
        #[arg(required = true, num_args = 1..)]
        #[serde(serialize_with = "crate::evidence::serialize_paths")]
        files: Vec<PathBuf>,

        /// Name for the generated profile
        #[arg(long)]
        profile_name: Option<String>,

        /// Base template path or built-in name (base, eyes, custom-start, service-api, event-pipeline)
        #[arg(long)]
        #[serde(serialize_with = "crate::evidence::serialize_optional_path")]
        template: Option<PathBuf>,
    },
}

impl Cli {
    /// Get the effective output format (handles -j shorthand)
    pub fn common_reports(&self) -> bool {
        self.complete_output
            || self.report_max_chars.is_some()
            || self.report_max_bytes.is_some()
            || self.report_max_items.is_some()
            || self.report_cursor.is_some()
    }

    pub fn prepare_common_reports(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.common_reports() {
            return Ok(());
        }
        match &mut self.command {
            Commands::Capabilities | Commands::GenerateConfig { .. } | Commands::Schema { .. } | Commands::ProfileMappings { .. } | Commands::PrepareProfile { .. } =>
                return Err("Common report budgets support info/search/extract/perf/trace/process/comparisons/errors; this command is unsupported".into()),
            Commands::Process { limit, .. } => *limit = 0,
            Commands::Perf { top_n, .. } => *top_n = 0,
            Commands::Errors { top_n, bounded, max_sample_chars, max_stack_frames, max_output_chars, .. } => {
                if *bounded || max_sample_chars.is_some() || max_stack_frames.is_some() || max_output_chars.is_some() {
                    return Err("Common report retrieval conflicts with legacy error sample/stack/output clipping; omit legacy bounded flags".into());
                }
                *top_n = 0;
            }
            _ => (),
        }
        Ok(())
    }

    pub fn effective_format(&self) -> OutputFormat {
        if self.json || self.common_reports() {
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
