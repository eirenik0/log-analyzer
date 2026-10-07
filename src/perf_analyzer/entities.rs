use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

/// Represents a completed timed operation (paired start/end)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedOperation {
    /// Type of operation: "Request", "Event", "Command"
    pub op_type: String,
    /// Name of the operation (e.g., "openEyes", "Core.makeManager", "close")
    pub name: String,
    /// Correlation ID used to match start/end
    pub correlation_id: Option<String>,
    /// Start time of the operation
    pub start_time: DateTime<Local>,
    /// End time of the operation
    pub end_time: DateTime<Local>,
    /// Duration in milliseconds
    pub duration_ms: i64,
    /// Component that started the operation
    pub start_component: String,
    /// Component that ended the operation
    pub end_component: String,
    pub start_source: SourceLocation,
    pub end_source: SourceLocation,
    pub scope: Vec<String>,
    /// Endpoint information (for requests)
    pub endpoint: Option<String>,
    /// HTTP status or result status
    pub status: Option<String>,
}

/// Represents an operation that was started but never completed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrphanOperation {
    /// Type of operation: "Request", "Event", "Command"
    pub op_type: String,
    /// Name of the operation
    pub name: String,
    /// Correlation ID
    pub correlation_id: Option<String>,
    /// Start time of the operation
    pub start_time: DateTime<Local>,
    /// Component that started the operation
    pub component: String,
    /// Session/component path (`component_id`) when present on the source log entry
    pub component_id: Option<String>,
    pub source: SourceLocation,
    /// Additional context about the operation
    pub context: String,
}

/// Aggregated statistics for a specific operation type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationStats {
    /// Type of operation
    pub op_type: String,
    /// Name of the operation
    pub name: String,
    /// Total count of this operation
    pub count: usize,
    /// Average duration in milliseconds
    pub avg_duration_ms: f64,
    /// Minimum duration in milliseconds
    pub min_duration_ms: i64,
    /// Maximum duration in milliseconds
    pub max_duration_ms: i64,
    /// 50th percentile (median) duration in milliseconds
    pub p50_duration_ms: i64,
    /// 95th percentile duration in milliseconds
    pub p95_duration_ms: i64,
    /// 99th percentile duration in milliseconds
    pub p99_duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    pub file: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnmatchedEvent {
    pub op_type: String,
    pub name: String,
    pub correlation_id: Option<String>,
    pub scope: Vec<String>,
    pub boundary: String,
    pub reason: String,
    pub timestamp: DateTime<Local>,
    pub component: String,
    pub source: SourceLocation,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbiguousGroup {
    pub op_type: String,
    pub name: String,
    pub correlation_id: String,
    pub scope: Vec<String>,
    pub events: Vec<UnmatchedEvent>,
}

/// Results of performance analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerfAnalysisResults {
    /// All completed timed operations
    pub operations: Vec<TimedOperation>,
    /// Operations that never completed
    pub orphans: Vec<OrphanOperation>,
    /// Aggregated statistics per operation type
    pub stats: Vec<OperationStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_timeline: Option<crate::timeline::TimelineReport>,
    pub unmatched_events: Vec<UnmatchedEvent>,
    pub ambiguous_groups: Vec<AmbiguousGroup>,
    /// Time range of the analyzed logs
    pub time_range: Option<(DateTime<Local>, DateTime<Local>)>,
    /// Total number of log entries analyzed
    pub total_entries: usize,
}

impl PerfAnalysisResults {
    /// Create a new empty results structure
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
            orphans: Vec::new(),
            stats: Vec::new(),
            event_timeline: None,
            unmatched_events: Vec::new(),
            ambiguous_groups: Vec::new(),
            time_range: None,
            total_entries: 0,
        }
    }

    /// Get operations exceeding a duration threshold
    pub fn operations_exceeding_threshold(&self, threshold_ms: u64) -> Vec<&TimedOperation> {
        self.operations
            .iter()
            .filter(|op| op.duration_ms >= threshold_ms as i64)
            .collect()
    }

    /// Get top N slowest operations
    pub fn top_slowest_operations(&self, n: usize) -> Vec<&TimedOperation> {
        let mut ops: Vec<&TimedOperation> = self.operations.iter().collect();
        ops.sort_by_key(|a| std::cmp::Reverse(a.duration_ms));
        ops.into_iter().take(n).collect()
    }

    /// Calculate statistics for all operations
    pub fn calculate_stats(&mut self) {
        use std::collections::HashMap;

        // Group operations by (op_type, name)
        let mut grouped: HashMap<(String, String), Vec<&TimedOperation>> = HashMap::new();
        for op in &self.operations {
            grouped
                .entry((op.op_type.clone(), op.name.clone()))
                .or_default()
                .push(op);
        }

        // Calculate statistics for each group
        self.stats = grouped
            .into_iter()
            .map(|((op_type, name), ops)| {
                let mut durations: Vec<i64> = ops.iter().map(|op| op.duration_ms).collect();
                durations.sort();

                let count = durations.len();
                let sum: i64 = durations.iter().sum();
                let avg = sum as f64 / count as f64;
                let min = *durations.first().unwrap();
                let max = *durations.last().unwrap();
                let p50 = durations[count * 50 / 100];
                let p95 = durations[count * 95 / 100];
                let p99 = durations[count * 99 / 100];

                OperationStats {
                    op_type,
                    name,
                    count,
                    avg_duration_ms: avg,
                    min_duration_ms: min,
                    max_duration_ms: max,
                    p50_duration_ms: p50,
                    p95_duration_ms: p95,
                    p99_duration_ms: p99,
                }
            })
            .collect();

        // Sort stats by average duration descending
        self.stats
            .sort_by(|a, b| b.avg_duration_ms.partial_cmp(&a.avg_duration_ms).unwrap());
    }
}

impl Default for PerfAnalysisResults {
    fn default() -> Self {
        Self::new()
    }
}
