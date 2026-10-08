mod display;
mod entities;

pub use display::{
    display_perf_results, format_perf_results_json, format_perf_results_json_with_options,
    format_perf_results_text, truncate_string,
};
pub use entities::{
    AmbiguousGroup, CaptureWindow, OperationCoverage, OperationStats, OrphanOperation,
    PerfAnalysisResults, SourceLocation, SuppressedOperationType, TimedOperation, UnmatchedEvent,
};

use crate::comparator::LogFilter;
use crate::config::{AnalyzerConfig, PerfRules, default_config};
use crate::event_rules::{ClassifiedRecord, Phase};
use crate::parser::{LogEntry, LogEntryKind};

/// Extracts the request ID from a log message containing [request_id] pattern
/// The pattern is: Request "name" [id] where id contains "--" (e.g., "0--uuid" or "0--uuid#2")
pub fn extract_request_id(message: &str) -> Option<String> {
    // Look for pattern: Request "name" [id] where id contains "--"
    let req_prefix = r#"Request ""#;
    if let Some(start_idx) = message.find(req_prefix) {
        let after_prefix = start_idx + req_prefix.len();
        // Find closing quote of request name
        if let Some(name_end) = message[after_prefix..].find('"') {
            let after_name = after_prefix + name_end + 1;
            if after_name < message.len() {
                let rest = &message[after_name..];
                // Request ID should be immediately after: " [id]"
                if rest.starts_with(" [")
                    && let Some(id_end) = rest[2..].find(']')
                {
                    let potential_id = &rest[2..2 + id_end];
                    // Validate it looks like a request ID (contains --)
                    if potential_id.contains("--") && !potential_id.contains(' ') {
                        return Some(potential_id.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Extracts event key from event payload
pub fn extract_event_key(payload: &serde_json::Value) -> Option<String> {
    extract_event_key_with_rules(payload, &default_config().perf)
}

fn extract_event_key_with_rules(payload: &serde_json::Value, rules: &PerfRules) -> Option<String> {
    rules.event_correlation_keys.iter().find_map(|key| {
        payload
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    })
}

/// Analyzes logs for performance bottlenecks by tracking paired operations
pub fn analyze_performance(
    logs: &[LogEntry],
    filter: &LogFilter,
    op_type_filter: Option<&str>,
) -> PerfAnalysisResults {
    analyze_performance_with_config(logs, filter, op_type_filter, default_config())
}

/// Analyzes logs for performance bottlenecks using explicit analyzer config.
pub fn analyze_performance_with_config(
    logs: &[LogEntry],
    filter: &LogFilter,
    op_type_filter: Option<&str>,
    config: &AnalyzerConfig,
) -> PerfAnalysisResults {
    let mut results = PerfAnalysisResults::new();

    let filtered: Vec<_> = logs.iter().filter(|entry| filter.matches(entry)).collect();
    results.total_entries = filtered.len();
    let known_year =
        !filtered.is_empty() && filtered.iter().all(|entry| !entry.timestamp_year_inferred);
    results
        .operation_coverage
        .capture_window
        .timestamp_year_known = known_year;
    if known_year {
        results.time_range = filtered
            .iter()
            .map(|entry| entry.timestamp)
            .min()
            .zip(filtered.iter().map(|entry| entry.timestamp).max());
        let timestamps: Vec<_> = filtered
            .iter()
            .map(|entry| {
                entry
                    .source_timestamp
                    .unwrap_or_else(|| entry.timestamp.fixed_offset())
            })
            .collect();
        let window = &mut results.operation_coverage.capture_window;
        window.start = timestamps.iter().copied().min();
        window.end = timestamps.iter().copied().max();
        window.elapsed_ms = window
            .start
            .zip(window.end)
            .map(|(start, end)| end.signed_duration_since(start).num_milliseconds());
    } else if !filtered.is_empty() {
        results.operation_coverage.capture_window.limits.push("Timestamp years are inferred; absolute bounds and elapsed capture time are unavailable".into());
    }
    let mut groups: std::collections::BTreeMap<CorrelationKey, Vec<BoundaryEvent<'_>>> =
        std::collections::BTreeMap::new();
    let mut suppressions: std::collections::BTreeMap<(String, String), usize> =
        std::collections::BTreeMap::new();
    for entry in filtered {
        let evidence = &entry.classification;
        let counts = &mut results.operation_coverage.classification;
        counts.selected_records += 1;
        match evidence {
            Some(ClassifiedRecord::Event {
                semantics, legacy, ..
            }) => {
                counts.classified_records += 1;
                if semantics.phase.is_none() {
                    counts.identity_only_records += 1;
                }
                if *legacy {
                    counts.legacy_records += 1;
                }
            }
            Some(ClassifiedRecord::Unclassified) => {
                counts.unclassified_records += 1;
                results.operation_coverage.unclassified_command_records += 1;
            }
            Some(ClassifiedRecord::Conflict { .. }) => counts.conflicting_records += 1,
            Some(ClassifiedRecord::Invalid { .. }) => counts.invalid_records += 1,
            None => counts.unavailable_records += 1,
        }
        if let Some(
            ClassifiedRecord::Conflict { kinds, .. } | ClassifiedRecord::Invalid { kinds, .. },
        ) = evidence
        {
            let conflict = matches!(evidence, Some(ClassifiedRecord::Conflict { .. }));
            let applicable = op_type_filter.is_none_or(|selected| {
                kinds.is_empty() || kinds.iter().any(|kind| kind.label() == selected)
            });
            results.operation_coverage.relevant_events += 1;
            if !applicable {
                *suppressions
                    .entry(("Unknown".into(), "operation_type_filter".into()))
                    .or_default() += 1;
                continue;
            }
            results.unmatched_events.push(UnmatchedEvent {
                classification: evidence.clone(),
                op_type: if kinds.len() == 1 {
                    kinds[0].label()
                } else {
                    "Unknown"
                }
                .into(),
                name: "<unknown>".into(),
                correlation_id: None,
                scope: Vec::new(),
                boundary: if conflict { "conflicting" } else { "invalid" }.into(),
                reason: if conflict {
                    "conflicting_event_rules"
                } else {
                    "invalid_event_data"
                }
                .into(),
                timestamp: entry.timestamp,
                component: entry.component.clone(),
                source: source(entry),
                context: entry.raw_logline.clone(),
            });
            continue;
        }
        if let Some(ClassifiedRecord::Event { semantics, .. }) = evidence
            && semantics.phase == Some(Phase::Start)
            && !semantics.end_expected
        {
            if op_type_filter.is_none_or(|selected| selected == semantics.kind.label()) {
                results.operation_coverage.relevant_events += 1;
                results.operation_coverage.start_only_events += 1;
            }
            continue;
        }
        let (op_type, name, id, start, end) =
            if let Some(ClassifiedRecord::Event { semantics, .. }) = evidence {
                (
                    semantics.kind.label(),
                    semantics.name.as_str(),
                    semantics.correlation_id.clone(),
                    semantics.phase == Some(Phase::Start),
                    semantics.phase == Some(Phase::End),
                )
            } else if !matches!(entry.kind, LogEntryKind::Generic { .. }) {
                // Manually assembled library records must attach explicit or legacy evidence.
                (
                    entry.entry_type(),
                    entry.operation_name().unwrap_or("<unknown>"),
                    None,
                    false,
                    false,
                )
            } else {
                continue;
            };
        results.operation_coverage.relevant_events += 1;
        if !start && !end && op_type_filter.is_none_or(|selected| selected == op_type) {
            results.unmatched_events.push(UnmatchedEvent {
                classification: evidence.clone(),
                op_type: op_type.into(),
                name: name.into(),
                correlation_id: id,
                scope: correlation_scope(entry, config).unwrap_or_default(),
                boundary: "identity".into(),
                reason: if matches!(evidence, Some(ClassifiedRecord::Event { .. })) {
                    "identity_only"
                } else {
                    "unclassified_operation_record"
                }
                .into(),
                timestamp: entry.timestamp,
                component: entry.component.clone(),
                source: source(entry),
                context: entry.raw_logline.clone(),
            });
            continue;
        }
        let suppression = if op_type_filter.is_some_and(|selected| selected != op_type) {
            Some("operation_type_filter")
        } else if !start && !end {
            Some("no_recognized_boundary")
        } else {
            None
        };
        if let Some(reason) = suppression {
            *suppressions
                .entry((op_type.to_string(), reason.to_string()))
                .or_default() += 1;
            continue;
        }
        let event = BoundaryEvent {
            entry,
            name,
            id,
            start,
            op_type,
        };
        if start && end {
            results
                .unmatched_events
                .push(event.unmatched(Vec::new(), "ambiguous_boundary"));
            continue;
        }
        let Some(id) = event.id.clone().filter(|id| !id.is_empty()) else {
            results
                .unmatched_events
                .push(event.unmatched(Vec::new(), "missing_correlation_key"));
            continue;
        };
        let scope = correlation_scope(entry, config);
        let Some(scope) = scope else {
            results
                .unmatched_events
                .push(event.unmatched(Vec::new(), "missing_scope_field"));
            continue;
        };
        groups
            .entry(CorrelationKey {
                op_type: op_type.to_string(),
                name: name.to_string(),
                id,
                scope,
            })
            .or_default()
            .push(event);
    }
    let mut pending_groups: std::collections::VecDeque<_> = groups.into_iter().collect();
    while let Some((key, mut events)) = pending_groups.pop_front() {
        // Inferred years cannot establish boundary chronology (including New Year).
        if events
            .iter()
            .any(|event| event.entry.timestamp_year_inferred)
        {
            if events.len() == 2 && events.iter().filter(|event| event.start).count() == 1 {
                results.operation_coverage.rejected_pairs += 1;
            }
            for event in events {
                results
                    .unmatched_events
                    .push(event.unmatched(key.scope.clone(), "incomplete_timestamp_year"));
            }
            continue;
        }
        events.sort_by(|a, b| {
            a.entry
                .timestamp
                .cmp(&b.entry.timestamp)
                .then_with(|| a.entry.source_line_number.cmp(&b.entry.source_line_number))
        });
        let mut bucket_time = None;
        let mut bucket_file = None;
        let mut seen_rows = std::collections::HashSet::new();
        let mut unordered_timestamps = std::collections::HashSet::new();
        for event in &events {
            if bucket_time != Some(event.entry.timestamp) {
                bucket_time = Some(event.entry.timestamp);
                bucket_file = Some(&event.entry.source_file);
                seen_rows.clear();
            }
            if bucket_file != Some(&event.entry.source_file)
                || !seen_rows.insert((event.entry.source_line_number, &event.entry.source_row_path))
            {
                unordered_timestamps.insert(event.entry.timestamp);
            }
        }
        let tied = !unordered_timestamps.is_empty();
        if tied {
            // Close established lifecycles before isolating an unordered timestamp bucket.
            let mut segments = Vec::new();
            let mut segment = Vec::new();
            let mut outstanding = 0i64;
            let mut boundaries = events.into_iter().peekable();
            while let Some(event) = boundaries.next() {
                let timestamp = event.entry.timestamp;
                outstanding += if event.start { 1 } else { -1 };
                segment.push(event);
                if (!unordered_timestamps.contains(&timestamp)
                    || boundaries
                        .peek()
                        .is_none_or(|next| next.entry.timestamp != timestamp))
                    && outstanding <= 0
                {
                    segments.push(std::mem::take(&mut segment));
                    outstanding = 0;
                }
            }
            if !segment.is_empty() {
                segments.push(segment);
            }
            if segments.len() > 1 {
                for segment in segments.into_iter().rev() {
                    pending_groups.push_front((key.clone(), segment));
                }
                continue;
            }
            events = segments.pop().unwrap_or_default();
        }
        let mut active = false;
        let ambiguous = tied
            || events.iter().any(|event| {
                if event.start {
                    if active {
                        return true;
                    }
                    active = true;
                } else {
                    active = false;
                }
                false
            });
        if ambiguous {
            let preserved: Vec<_> = events
                .iter()
                .map(|event| {
                    event.unmatched(
                        key.scope.clone(),
                        if tied {
                            "ambiguous_timestamp_order"
                        } else {
                            "overlapping_starts"
                        },
                    )
                })
                .collect();
            for event in &events {
                if event.start && !tied {
                    results.orphans.push(event.orphan());
                }
            }
            results.unmatched_events.extend(preserved.clone());
            results.ambiguous_groups.push(AmbiguousGroup {
                op_type: key.op_type,
                name: key.name,
                correlation_id: key.id,
                scope: key.scope,
                events: preserved,
            });
            continue;
        }
        let mut pending: Option<BoundaryEvent<'_>> = None;
        for event in events {
            if event.start {
                pending = Some(event);
            } else if let Some(start) = pending.take() {
                let entry = event.entry;
                results.operations.push(TimedOperation {
                    op_type: key.op_type.clone(),
                    name: key.name.clone(),
                    correlation_id: Some(key.id.clone()),
                    start_time: start.entry.timestamp,
                    end_time: entry.timestamp,
                    duration_ms: entry
                        .timestamp
                        .signed_duration_since(start.entry.timestamp)
                        .num_milliseconds(),
                    start_component: start.entry.component.clone(),
                    end_component: entry.component.clone(),
                    start_classification: start.entry.classification.clone(),
                    end_classification: entry.classification.clone(),
                    start_source: source(start.entry),
                    end_source: source(entry),
                    scope: key.scope.clone(),
                    endpoint: match &entry.classification {
                        Some(ClassifiedRecord::Event { semantics, .. }) => semantics
                            .endpoint
                            .clone()
                            .or_else(|| match &start.entry.classification {
                                Some(ClassifiedRecord::Event { semantics, .. }) => {
                                    semantics.endpoint.clone()
                                }
                                _ => None,
                            }),
                        _ => None,
                    },
                    status: if let Some(ClassifiedRecord::Event { semantics, .. }) =
                        &entry.classification
                        && let Some(outcome) = semantics.outcome
                    {
                        Some(
                            match outcome {
                                crate::event_rules::Outcome::Success => "success",
                                crate::event_rules::Outcome::Failure => "failure",
                            }
                            .into(),
                        )
                    } else {
                        entry
                            .payload()
                            .and_then(|p| p.get("statusCode"))
                            .and_then(|v| v.as_i64())
                            .map(|v| v.to_string())
                    },
                });
            } else {
                results
                    .unmatched_events
                    .push(event.unmatched(key.scope.clone(), "missing_start"));
            }
        }
        if let Some(event) = pending {
            results.orphans.push(event.orphan());
            results
                .unmatched_events
                .push(event.unmatched(key.scope, "missing_end"));
        }
    }
    let coverage = &mut results.operation_coverage;
    coverage.suppressed_operation_types = suppressions
        .into_iter()
        .map(|((op_type, reason), events)| SuppressedOperationType {
            op_type,
            reason,
            events,
        })
        .collect();
    coverage.suppressed_events = coverage
        .suppressed_operation_types
        .iter()
        .map(|s| s.events)
        .sum();
    coverage.paired_events = results.operations.len() * 2;
    coverage.unmatched_events = results.unmatched_events.len();
    coverage.ambiguous_groups = results.ambiguous_groups.len();
    coverage.ambiguous_events = results
        .unmatched_events
        .iter()
        .filter(|e| {
            matches!(
                e.reason.as_str(),
                "overlapping_starts"
                    | "ambiguous_timestamp_order"
                    | "ambiguous_boundary"
                    | "conflicting_event_rules"
            )
        })
        .count();
    coverage.ambiguous_pairs = if coverage.ambiguous_events > 0 {
        None
    } else {
        Some(0)
    };
    coverage.rejected_events = results
        .unmatched_events
        .iter()
        .filter(|e| {
            matches!(
                e.reason.as_str(),
                "missing_correlation_key"
                    | "missing_scope_field"
                    | "incomplete_timestamp_year"
                    | "invalid_event_data"
            )
        })
        .count();
    coverage.status = if coverage.relevant_events == 0 {
        "no_applicable_events"
    } else if coverage.paired_events == 0 {
        "insufficient_evidence"
    } else if coverage.unmatched_events > 0 || coverage.suppressed_events > 0 {
        "partial_evidence"
    } else {
        "observed_pairs"
    }
    .into();
    results.calculate_stats();
    results
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CorrelationKey {
    op_type: String,
    name: String,
    id: String,
    scope: Vec<String>,
}

struct BoundaryEvent<'a> {
    op_type: &'static str,
    entry: &'a LogEntry,
    name: &'a str,
    id: Option<String>,
    start: bool,
}

fn correlation_scope(entry: &LogEntry, config: &AnalyzerConfig) -> Option<Vec<String>> {
    let scope = if let Some(ClassifiedRecord::Event {
        semantics, legacy, ..
    }) = &entry.classification
        && (!legacy || !semantics.scope.is_empty())
    {
        Some(semantics.scope.clone())
    } else {
        crate::parser::record_correlation_scope(entry, config)
    };
    scope.filter(|scope| {
        !scope.is_empty()
            && scope
                .iter()
                .all(|v| v.len() <= crate::event_rules::MAX_VALUE_BYTES)
    })
}

fn source(entry: &LogEntry) -> SourceLocation {
    SourceLocation {
        file: entry.source_file.clone(),
        line: entry.source_line_number,
        row_path: entry.source_row_path.clone(),
    }
}

impl BoundaryEvent<'_> {
    fn unmatched(&self, scope: Vec<String>, reason: &str) -> UnmatchedEvent {
        UnmatchedEvent {
            classification: self.entry.classification.clone(),
            op_type: self.op_type.to_string(),
            name: self.name.to_string(),
            correlation_id: self.id.clone(),
            scope,
            boundary: if reason == "ambiguous_boundary" {
                "ambiguous"
            } else if self.start {
                "start"
            } else {
                "end"
            }
            .to_string(),
            reason: reason.to_string(),
            timestamp: self.entry.timestamp,
            component: self.entry.component.clone(),
            source: source(self.entry),
            context: self.entry.raw_logline.clone(),
        }
    }
    fn orphan(&self) -> OrphanOperation {
        OrphanOperation {
            classification: self.entry.classification.clone(),
            op_type: self.op_type.to_string(),
            name: self.name.to_string(),
            correlation_id: self.id.clone(),
            start_time: self.entry.timestamp,
            component: self.entry.component.clone(),
            component_id: (!self.entry.component_id.is_empty())
                .then(|| self.entry.component_id.clone()),
            source: source(self.entry),
            context: self.entry.message.clone(),
        }
    }
}
