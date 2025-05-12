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

/// Represents the difference between two JSON values
#[derive(Debug, Clone)]
pub struct JsonDifference {
    pub path: String,
    pub value1: Value,
    pub value2: Value,
}

/// Represents a comparison between two log entries
#[derive(Debug)]
pub struct LogComparison {
    pub key: String,
    pub log1_index: usize,
    pub log2_index: usize,
    pub json_differences: Vec<JsonDifference>,
    pub text_difference: Option<String>,
}

/// Represents filtering criteria for logs
#[derive(Default, Clone)]
pub struct LogFilter {
    component: Option<String>,
    level: Option<String>,
    message_contains: Option<String>,
    direction: Option<Direction>,
}

impl LogFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_component(mut self, component: Option<impl Into<String>>) -> Self {
        self.component = component.map(|c| c.into());
        self
    }

    pub fn with_level(mut self, level: Option<impl Into<String>>) -> Self {
        self.level = level.map(|l| l.into());
        self
    }

    pub fn contains_text(mut self, text: Option<impl Into<String>>) -> Self {
        self.message_contains = text.map(|t| t.into());
        self
    }

    pub fn with_direction(mut self, direction: &Option<Direction>) -> Self {
        self.direction = direction.clone();
        self
    }

    pub fn matches(&self, log: &LogEntry) -> bool {
        let component_match = self
            .component
            .as_ref()
            .map(|filter| log.component.contains(filter))
            .unwrap_or(true);

        let level_match = self
            .level
            .as_ref()
            .map(|filter| log.level.contains(filter))
            .unwrap_or(true);

        let contains_match = self
            .message_contains
            .as_ref()
            .map(|filter| log.message.contains(filter))
            .unwrap_or(true);

        let direction_match = self
            .direction
            .as_ref()
            .map(|filter| match &log.kind {
                LogEntryKind::Event { direction, .. } => {
                    // Convert event direction to Direction for comparison
                    let event_as_direction: Direction = direction.clone().into();
                    // Compare with the filter (which is already a Direction)
                    &event_as_direction == filter
                }
                LogEntryKind::Request { direction, .. } => {
                    // Convert request direction to Direction for comparison
                    let request_as_direction: Direction = direction.clone().into();
                    // Compare with the filter (which is already a Direction)
                    &request_as_direction == filter
                }
                LogEntryKind::Command { .. } => {
                    // For commands, check if the filter direction is outgoing
                    matches!(filter, Direction::Outgoing)
                }
                LogEntryKind::Generic { .. } => false,
            })
            .unwrap_or(true);
        component_match && direction_match && level_match && contains_match
    }
}

/// Options for controlling the comparison output
#[derive(Default)]
pub struct ComparisonOptions {
    pub diff_only: bool,
    pub show_full_json: bool,
    pub output_path: Option<String>,
    pub compact_mode: bool,
    pub readable_mode: bool,
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

    pub fn output_to_file(mut self, path: Option<impl Into<String>>) -> Self {
        self.output_path = path.map(|p| p.into());
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
