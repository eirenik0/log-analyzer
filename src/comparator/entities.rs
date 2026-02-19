use crate::LogEntryKind;
use crate::cli::Direction;
use crate::parser::LogEntry;
use serde_json::Value;

/// Error types for comparison operations
#[derive(Debug)]
pub enum ComparisonError {
    IoError(std::io::Error),
    JsonError(serde_json::Error),
}

impl From<std::io::Error> for ComparisonError {
    fn from(err: std::io::Error) -> Self {
        ComparisonError::IoError(err)
    }
}

impl From<serde_json::Error> for ComparisonError {
    fn from(err: serde_json::Error) -> Self {
        ComparisonError::JsonError(err)
    }
}

/// Represents the type of change detected in a JSON comparison
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    /// Key/value added in log2 (null → value)
    Added,
    /// Key/value removed in log2 (value → null)
    Removed,
    /// Value changed between log1 and log2
    Modified,
}

/// Represents the difference between two JSON values
#[derive(Debug, Clone)]
pub struct JsonDifference {
    pub path: String,
    pub value1: Value,
    pub value2: Value,
    pub change_type: ChangeType,
}

/// Represents a comparison between two log entries
#[derive(Debug)]
pub struct LogComparison {
    pub key: String,
    pub log1_index: usize,
    pub log2_index: usize,
    pub json_differences: Vec<JsonDifference>,
    pub text1: Option<String>,
    pub text2: Option<String>,
    pub log1_line_number: usize,
    pub log2_line_number: usize,
    pub log1_payload: Option<Value>,
    pub log2_payload: Option<Value>,
}

/// Represents filtering criteria for logs
#[derive(Default, Clone)]
pub struct LogFilter {
    include_components: Vec<String>,
    exclude_components: Vec<String>,
    include_levels: Vec<String>,
    exclude_levels: Vec<String>,
    include_text: Vec<String>,
    exclude_text: Vec<String>,
    include_directions: Vec<Direction>,
    exclude_directions: Vec<Direction>,
}

impl LogFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_component(mut self, component: Option<impl Into<String>>) -> Self {
        if let Some(component) = component {
            self.include_components.push(component.into());
        }
        self
    }

    pub fn exclude_component(mut self, component: Option<impl Into<String>>) -> Self {
        if let Some(component) = component {
            self.exclude_components.push(component.into());
        }
        self
    }

    pub fn with_level(mut self, level: Option<impl Into<String>>) -> Self {
        if let Some(level) = level {
            self.include_levels.push(level.into());
        }
        self
    }

    pub fn exclude_level(mut self, level: Option<impl Into<String>>) -> Self {
        if let Some(level) = level {
            self.exclude_levels.push(level.into());
        }
        self
    }

    pub fn contains_text(mut self, text: Option<impl Into<String>>) -> Self {
        if let Some(text) = text {
            self.include_text.push(text.into());
        }
        self
    }

    pub fn excludes_text(mut self, text: Option<impl Into<String>>) -> Self {
        if let Some(text) = text {
            self.exclude_text.push(text.into());
        }
        self
    }

    pub fn with_direction(mut self, direction: &Option<Direction>) -> Self {
        if let Some(direction) = direction.clone() {
            self.include_directions.push(direction);
        }
        self
    }

    pub fn exclude_direction(mut self, direction: &Option<Direction>) -> Self {
        if let Some(direction) = direction.clone() {
            self.exclude_directions.push(direction);
        }
        self
    }

    pub fn matches(&self, log: &LogEntry) -> bool {
        fn contains_ci(haystack: &str, needle: &str) -> bool {
            haystack.to_lowercase().contains(&needle.to_lowercase())
        }

        let component_match = self.include_components.is_empty()
            || self
                .include_components
                .iter()
                .any(|filter| contains_ci(&log.component, filter));
        let level_match = self.include_levels.is_empty()
            || self
                .include_levels
                .iter()
                .any(|filter| contains_ci(&log.level, filter));
        let contains_match = self.include_text.is_empty()
            || self
                .include_text
                .iter()
                .any(|filter| contains_ci(&log.message, filter));

        // Exclude filters (log must NOT match any of these)
        let exclude_component_match = self
            .exclude_components
            .iter()
            .all(|filter| !contains_ci(&log.component, filter));
        let exclude_level_match = self
            .exclude_levels
            .iter()
            .all(|filter| !contains_ci(&log.level, filter));
        let excludes_match = self
            .exclude_text
            .iter()
            .all(|filter| !contains_ci(&log.message, filter));

        let log_direction = match &log.kind {
            LogEntryKind::Event { direction, .. } => Some(Direction::from(direction.clone())),
            LogEntryKind::Request { direction, .. } => Some(Direction::from(direction.clone())),
            // Commands are operationally outgoing
            LogEntryKind::Command { .. } => Some(Direction::Outgoing),
            LogEntryKind::Generic { .. } => None,
        };

        let include_direction_match = self.include_directions.is_empty()
            || self
                .include_directions
                .iter()
                .any(|filter| log_direction.as_ref() == Some(filter));
        let exclude_direction_match = self
            .exclude_directions
            .iter()
            .all(|filter| log_direction.as_ref() != Some(filter));

        component_match
            && include_direction_match
            && exclude_direction_match
            && level_match
            && contains_match
            && exclude_component_match
            && exclude_level_match
            && excludes_match
    }
}

use crate::cli::SortOrder;

/// Options for controlling the comparison output
#[derive(Default)]
pub struct ComparisonOptions {
    pub diff_only: bool,
    pub show_full_json: bool,
    pub compact_mode: bool,
    pub readable_mode: bool,
    pub sort_order: SortOrder,
    pub verbosity: u8, // 0: quiet, 1: normal, 2+: verbose
    pub quiet: bool,
}

impl ComparisonOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn diff_only(mut self, value: bool) -> Self {
        self.diff_only = value;
        self
    }

    pub fn show_full_json(mut self, value: bool) -> Self {
        self.show_full_json = value;
        self
    }

    pub fn compact_mode(mut self, value: bool) -> Self {
        self.compact_mode = value;
        self
    }

    pub fn readable_mode(mut self, value: bool) -> Self {
        self.readable_mode = value;
        self
    }

    pub fn sort_by(mut self, order: SortOrder) -> Self {
        self.sort_order = order;
        self
    }

    pub fn verbosity(mut self, level: u8) -> Self {
        self.verbosity = level;
        self
    }

    pub fn quiet_mode(mut self, value: bool) -> Self {
        self.quiet = value;
        self
    }
}

/// Results of comparing two sets of logs
#[derive(Debug)]
pub struct ComparisonResults {
    pub unique_to_log1: Vec<String>,
    pub unique_to_log2: Vec<String>,
    pub shared_comparisons: Vec<LogComparison>,
}

impl ComparisonResults {
    pub fn summary(&self) -> String {
        format!(
            "Unique to log file 1: {}\nUnique to log file 2: {}\nShared log types: {}",
            self.unique_to_log1.len(),
            self.unique_to_log2.len(),
            self.shared_comparisons.len()
        )
    }
}
