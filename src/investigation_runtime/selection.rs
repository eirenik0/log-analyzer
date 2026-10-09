//! Exact selected view of already-correlated occurrences; never pairs again.
use super::{Selector, selected};
use crate::{
    config::AnalyzerConfig,
    event_rules::ClassifiedRecord,
    parser::LogEntry,
    perf_analyzer::{PerfAnalysisResults, SourceLocation},
    processing::Budget,
};
use serde_json::Value;
use std::collections::BTreeSet;
pub(super) struct View<'a> {
    pub entries: Vec<&'a LogEntry>,
    sources: BTreeSet<(usize, Option<&'a str>)>,
    pub relevant: usize,
    pub classified: usize,
    pub boundaries: usize,
    pub identity_only: usize,
    pub conflicting: usize,
    pub pairs: usize,
    pub unmatched: usize,
    pub ambiguous: usize,
    pub rejected: usize,
    pub excluded_boundaries: usize,
    pub reliable: bool,
    pub outcomes_supported: bool,
}
impl<'a> View<'a> {
    pub fn new(
        entries: &'a [LogEntry],
        selectors: &[Selector],
        config: &AnalyzerConfig,
        perf: Option<&PerfAnalysisResults>,
        budget: &mut Budget,
    ) -> Self {
        let mut view = Self {
            entries: Vec::new(),
            sources: BTreeSet::new(),
            relevant: 0,
            classified: 0,
            boundaries: 0,
            identity_only: 0,
            conflicting: 0,
            pairs: 0,
            unmatched: 0,
            ambiguous: 0,
            rejected: 0,
            excluded_boundaries: 0,
            reliable: true,
            outcomes_supported: false,
        };
        for entry in entries {
            if !budget.checkpoint("calculation", 1) {
                break;
            }
            if !selected(entry, selectors, config) {
                continue;
            }
            view.entries.push(entry);
            view.sources
                .insert((entry.source_line_number, entry.source_row_path.as_deref()));
            match &entry.classification {
                Some(ClassifiedRecord::Event {
                    rule_ids,
                    semantics,
                    ..
                }) => {
                    view.classified += 1;
                    if semantics.phase.is_some() {
                        view.boundaries += 1;
                    } else {
                        view.identity_only += 1;
                    }
                    view.relevant += 1;
                    view.outcomes_supported |=
                        config.event_classifier().is_some_and(|classifier| {
                            classifier.schema().rules.iter().any(|rule| {
                                rule_ids.contains(&rule.id) && rule.mapping.outcome.is_some()
                            })
                        });
                }
                Some(ClassifiedRecord::Conflict { .. }) => {
                    view.conflicting += 1;
                    view.relevant += 1;
                }
                Some(ClassifiedRecord::Invalid { .. }) => {
                    view.relevant += 1;
                }
                _ => {}
            }
        }
        if let Some(perf) = perf {
            for operation in &perf.operations {
                if !budget.checkpoint("calculation", 1) {
                    break;
                }
                let start = view.source_selected(&operation.start_source);
                let end = view.source_selected(&operation.end_source);
                if start && end {
                    view.pairs += 1;
                } else if start || end {
                    view.excluded_boundaries += 1;
                }
            }
            for event in &perf.unmatched_events {
                if !budget.checkpoint("calculation", 1) {
                    break;
                }
                if !view.source_selected(&event.source) {
                    continue;
                }
                view.unmatched += 1;
                if matches!(
                    event.reason.as_str(),
                    "overlapping_starts"
                        | "ambiguous_timestamp_order"
                        | "ambiguous_boundary"
                        | "conflicting_event_rules"
                ) {
                    view.ambiguous += 1;
                }
                if matches!(
                    event.reason.as_str(),
                    "missing_correlation_key"
                        | "missing_scope_field"
                        | "incomplete_timestamp_year"
                        | "invalid_event_data"
                ) {
                    view.rejected += 1;
                }
            }
        }
        // Only selected lifecycle evidence affects chronology support.
        view.reliable = view
            .entries
            .iter()
            .filter(|entry| matches!(entry.classification, Some(ClassifiedRecord::Event { .. })))
            .all(|entry| !entry.timestamp_year_inferred && entry.source_timestamp.is_some());
        view
    }
    fn source_selected(&self, source: &SourceLocation) -> bool {
        self.sources
            .contains(&(source.line, source.row_path.as_deref()))
    }
    pub fn contains(&self, entry: &LogEntry) -> bool {
        self.sources
            .contains(&(entry.source_line_number, entry.source_row_path.as_deref()))
    }
    pub fn intersects_alias(&self, alias: &Value) -> bool {
        alias["witnesses"].as_array().is_some_and(|witnesses| {
            witnesses.iter().any(|source| {
                source["line"].as_u64().is_some_and(|line| {
                    self.sources
                        .contains(&(line as usize, source["row_path"].as_str()))
                })
            })
        })
    }
}
