use crate::parser::LogEntry;
use crate::perf_analyzer::SourceLocation;
use chrono::{DateTime, FixedOffset};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TimelineRules {
    pub events: Vec<EventRule>,
    pub pairs: Vec<PairRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRule {
    pub name: String,
    pub pattern: String,
    pub correlation_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairRule {
    pub name: String,
    pub start_event: String,
    pub end_event: String,
    pub timing: Timing,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Timing {
    Measured,
    InferredSleep,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub event_type: String,
    pub key: Option<Vec<String>>,
    pub timestamp: DateTime<FixedOffset>,
    pub timestamp_offset_source: String,
    pub timestamp_year_source: String,
    pub source: SourceLocation,
    pub raw: String,
    pub gap_since_previous_match_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineInterval {
    pub pair: String,
    pub key: Vec<String>,
    pub timing: Timing,
    pub start: TimelineEvent,
    pub end: TimelineEvent,
    pub observed_gap_ms: i64,
    pub measured_duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncompleteInterval {
    pub pair: String,
    pub event: TimelineEvent,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairCoverage {
    pub status: String,
    pub start_samples: usize,
    pub end_samples: usize,
    pub completed_intervals: usize,
    pub timing: Timing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineReport {
    pub status: String,
    pub events: Vec<TimelineEvent>,
    pub sample_counts: BTreeMap<String, usize>,
    pub pair_coverage: BTreeMap<String, PairCoverage>,
    pub intervals: Vec<TimelineInterval>,
    pub incomplete: Vec<IncompleteInterval>,
    pub ambiguous_groups: Vec<Vec<TimelineEvent>>,
    pub capture_window: Option<(DateTime<FixedOffset>, DateTime<FixedOffset>)>,
    pub upstream_capture_completeness: String,
    pub measured_work_sum_ms: Option<i64>,
    pub elapsed_capture_ms: Option<i64>,
}

fn evidence_timestamp(entry: &LogEntry) -> DateTime<FixedOffset> {
    entry
        .source_timestamp
        .unwrap_or_else(|| entry.timestamp.fixed_offset())
}

pub fn analyze(
    logs: &[&LogEntry],
    rules: &TimelineRules,
) -> Result<Option<TimelineReport>, String> {
    if rules.events.is_empty() && rules.pairs.is_empty() {
        return Ok(None);
    }
    let mut names = BTreeSet::new();
    let mut compiled = Vec::new();
    for rule in &rules.events {
        if rule.pattern.trim().is_empty()
            || rule.name.trim().is_empty()
            || !names.insert(rule.name.clone())
            || rule.correlation_fields.is_empty()
        {
            return Err("Timeline event names must be unique and nonempty, with explicit correlation fields".into());
        }
        compiled.push((
            rule,
            Regex::new(&rule.pattern)
                .map_err(|e| format!("Invalid timeline pattern {}: {e}", rule.name))?,
        ));
    }
    let mut pair_names = BTreeSet::new();
    for pair in &rules.pairs {
        if pair.name.trim().is_empty()
            || !pair_names.insert(pair.name.clone())
            || pair.start_event == pair.end_event
            || !names.contains(&pair.start_event)
            || !names.contains(&pair.end_event)
        {
            return Err(
                "Timeline pairs need unique nonempty names and two distinct configured event types"
                    .into(),
            );
        }
        let start = rules
            .events
            .iter()
            .find(|r| r.name == pair.start_event)
            .unwrap();
        let end = rules
            .events
            .iter()
            .find(|r| r.name == pair.end_event)
            .unwrap();
        if start.correlation_fields != end.correlation_fields {
            return Err(format!(
                "Timeline pair {} must use the same composite correlation fields",
                pair.name
            ));
        }
    }
    let capture_window = if logs.iter().any(|entry| entry.timestamp_year_inferred) {
        None
    } else {
        logs.iter()
            .map(|e| evidence_timestamp(e))
            .min()
            .zip(logs.iter().map(|e| evidence_timestamp(e)).max())
    };
    let mut report = TimelineReport {
        status: "no_applicable_events".into(),
        events: Vec::new(),
        sample_counts: rules
            .events
            .iter()
            .map(|rule| (rule.name.clone(), 0))
            .collect(),
        pair_coverage: BTreeMap::new(),
        intervals: Vec::new(),
        incomplete: Vec::new(),
        ambiguous_groups: Vec::new(),
        capture_window,
        upstream_capture_completeness: "unknown".into(),
        measured_work_sum_ms: None,
        elapsed_capture_ms: capture_window
            .map(|(a, b)| b.signed_duration_since(a).num_milliseconds()),
    };
    for entry in logs {
        for (rule, pattern) in &compiled {
            let Some(captures) = pattern.captures(
                entry
                    .normalized_record
                    .as_deref()
                    .unwrap_or(&entry.raw_logline),
            ) else {
                continue;
            };
            let key = rule
                .correlation_fields
                .iter()
                .map(|field| {
                    match field.as_str() {
                        "component_id" => Some(entry.component_id.clone()),
                        "component" => Some(entry.component.clone()),
                        _ => captures
                            .name(field)
                            .map(|v| v.as_str().to_string())
                            .or_else(|| entry.structured_field(field).map(str::to_owned))
                            .or_else(|| {
                                entry
                                    .envelope_payload
                                    .as_ref()
                                    .and_then(|p| p.get(field))
                                    .or_else(|| entry.payload().and_then(|p| p.get(field)))
                                    .filter(|v| !v.is_null() && !v.is_object() && !v.is_array())
                                    .map(|v| {
                                        v.as_str()
                                            .map(str::to_owned)
                                            .unwrap_or_else(|| v.to_string())
                                    })
                            }),
                    }
                    .filter(|v| !v.trim().is_empty() && v != "null")
                })
                .collect::<Option<Vec<_>>>();
            *report.sample_counts.entry(rule.name.clone()).or_default() += 1;
            report.events.push(TimelineEvent {
                event_type: rule.name.clone(),
                key,
                timestamp: evidence_timestamp(entry),
                timestamp_offset_source: if entry.source_timestamp.is_some() {
                    "source"
                } else {
                    "host_assumed"
                }
                .into(),
                timestamp_year_source: if entry.timestamp_year_inferred {
                    "inferred_year"
                } else {
                    "source"
                }
                .into(),
                source: SourceLocation {
                    evidence_ref: None,
                    file: entry.source_file.clone(),
                    line: entry.source_line_number,
                    row_path: entry.source_row_path.clone(),
                },
                raw: entry.raw_logline.clone(),
                gap_since_previous_match_ms: None,
            });
        }
    }
    let ordering_known = report
        .events
        .iter()
        .all(|event| event.timestamp_year_source == "source");
    if ordering_known {
        report.events.sort_by_key(|e| e.timestamp);
    } else {
        report.events.sort_by(|a, b| {
            a.source
                .file
                .cmp(&b.source.file)
                .then_with(|| a.source.line.cmp(&b.source.line))
        });
    }
    let mut previous = None;
    for event in &mut report.events {
        event.gap_since_previous_match_ms = previous.filter(|_| ordering_known).map(|time| {
            event
                .timestamp
                .signed_duration_since(time)
                .num_milliseconds()
        });
        previous = (event.timestamp_year_source == "source").then_some(event.timestamp);
    }
    for pair in &rules.pairs {
        let mut groups: BTreeMap<Vec<String>, Vec<TimelineEvent>> = BTreeMap::new();
        for event in &report.events {
            if event.event_type != pair.start_event && event.event_type != pair.end_event {
                continue;
            }
            if let Some(key) = &event.key {
                groups.entry(key.clone()).or_default().push(event.clone());
            } else {
                report.incomplete.push(IncompleteInterval {
                    pair: pair.name.clone(),
                    event: event.clone(),
                    reason: "missing_correlation_field".into(),
                });
            }
        }
        for (key, mut events) in groups {
            if events
                .iter()
                .any(|event| event.timestamp_year_source != "source")
            {
                for event in events {
                    report.incomplete.push(IncompleteInterval {
                        pair: pair.name.clone(),
                        event: event.clone(),
                        reason: "incomplete_timestamp_year".into(),
                    });
                }
                continue;
            }
            events.sort_by_key(|event| event.timestamp);
            let mut segment = Vec::new();
            let mut outstanding = 0i64;
            let mut reason = None;
            let mut index = 0;
            while index < events.len() {
                let mut end = index + 1;
                while end < events.len() && events[end].timestamp == events[index].timestamp {
                    end += 1;
                }
                let bucket = &events[index..end];
                let unordered = bucket.iter().enumerate().any(|(i, a)| {
                    bucket[i + 1..].iter().any(|b| {
                        a.source.file != b.source.file
                            || (a.source.line == b.source.line
                                && a.source.row_path == b.source.row_path)
                    })
                });
                if unordered {
                    reason = Some("ambiguous_boundary");
                    outstanding += bucket
                        .iter()
                        .map(|event| {
                            if event.event_type == pair.start_event {
                                1
                            } else {
                                -1
                            }
                        })
                        .sum::<i64>();
                    segment.extend(bucket.iter().cloned());
                    if outstanding <= 0 {
                        emit_segment(&mut report, pair, &key, &segment, reason);
                        segment.clear();
                        outstanding = 0;
                        reason = None;
                    }
                } else {
                    for event in bucket {
                        if event.event_type == pair.start_event {
                            if outstanding > 0 {
                                reason = Some("ambiguous_overlap");
                            }
                            outstanding += 1;
                            segment.push(event.clone());
                        } else if outstanding == 0 {
                            emit_segment(
                                &mut report,
                                pair,
                                &key,
                                std::slice::from_ref(event),
                                None,
                            );
                        } else {
                            outstanding -= 1;
                            segment.push(event.clone());
                            if outstanding == 0 {
                                emit_segment(&mut report, pair, &key, &segment, reason);
                                segment.clear();
                                reason = None;
                            }
                        }
                    }
                }
                index = end;
            }
            if !segment.is_empty() {
                emit_segment(&mut report, pair, &key, &segment, reason);
            }
        }
    }
    for pair in &rules.pairs {
        let start_samples = report.sample_counts[&pair.start_event];
        let end_samples = report.sample_counts[&pair.end_event];
        let completed_intervals = report
            .intervals
            .iter()
            .filter(|interval| interval.pair == pair.name)
            .count();
        let status = if start_samples + end_samples == 0 {
            "no_applicable_events"
        } else if completed_intervals == 0
            || report.incomplete.iter().any(|item| item.pair == pair.name)
        {
            "insufficient_evidence"
        } else {
            "boundaries_available"
        };
        report.pair_coverage.insert(
            pair.name.clone(),
            PairCoverage {
                status: status.into(),
                start_samples,
                end_samples,
                completed_intervals,
                timing: pair.timing,
            },
        );
    }
    if !report.events.is_empty() {
        report.status = if report.pair_coverage.is_empty()
            || report
                .pair_coverage
                .values()
                .any(|pair| pair.status != "boundaries_available")
        {
            "insufficient_evidence"
        } else {
            "boundaries_available"
        }
        .into();
    }
    Ok(Some(report))
}

fn emit_segment(
    report: &mut TimelineReport,
    pair: &PairRule,
    key: &[String],
    events: &[TimelineEvent],
    reason: Option<&str>,
) {
    if let Some(reason) = reason {
        report.ambiguous_groups.push(events.to_vec());
        for event in events {
            report.incomplete.push(IncompleteInterval {
                pair: pair.name.clone(),
                event: event.clone(),
                reason: reason.into(),
            });
        }
    } else if events.len() == 2 {
        let gap = events[1]
            .timestamp
            .signed_duration_since(events[0].timestamp)
            .num_milliseconds();
        let measured = (pair.timing == Timing::Measured).then_some(gap);
        if let Some(ms) = measured {
            *report.measured_work_sum_ms.get_or_insert(0) += ms;
        }
        report.intervals.push(TimelineInterval {
            pair: pair.name.clone(),
            key: key.to_vec(),
            timing: pair.timing,
            start: events[0].clone(),
            end: events[1].clone(),
            observed_gap_ms: gap,
            measured_duration_ms: measured,
        });
    } else {
        for event in events {
            report.incomplete.push(IncompleteInterval {
                pair: pair.name.clone(),
                event: event.clone(),
                reason: if event.event_type == pair.start_event {
                    "capture_ended_before_end_or_missing_end"
                } else {
                    "capture_started_after_start_or_missing_start"
                }
                .into(),
            });
        }
    }
}

pub fn format_text(report: &TimelineReport) -> String {
    let mut out = format!(
        "EVENT TIMELINE: {}\nSamples: {:?}\nCapture window: {:?}; upstream completeness: unknown\nMeasured work sum: {:?}ms; elapsed capture: {:?}ms (parallel intervals can overlap)\n",
        report.status,
        report.sample_counts,
        report.capture_window,
        report.measured_work_sum_ms,
        report.elapsed_capture_ms
    );
    for (name, pair) in &report.pair_coverage {
        let _ = writeln!(
            out,
            "Pair {name}: {}, {} start samples, {} end samples, {} intervals ({:?})",
            pair.status,
            pair.start_samples,
            pair.end_samples,
            pair.completed_intervals,
            pair.timing
        );
    }
    for event in &report.events {
        let _ = writeln!(
            out,
            "{} [offset: {}; year: {}] {} {:?}; gap since previous matched event {:?}ms at {}:{}",
            event.timestamp.to_rfc3339(),
            event.timestamp_offset_source,
            event.timestamp_year_source,
            event.event_type,
            event.key,
            event.gap_since_previous_match_ms,
            event.source.file.as_deref().unwrap_or("<unknown>"),
            event.source.line
        );
    }
    for interval in &report.intervals {
        let _ = writeln!(
            out,
            "{} {:?}: {:?}, observed gap {}ms, measured {:?}ms; {}:{} → {}:{}",
            interval.pair,
            interval.key,
            interval.timing,
            interval.observed_gap_ms,
            interval.measured_duration_ms,
            interval.start.source.file.as_deref().unwrap_or("<unknown>"),
            interval.start.source.line,
            interval.end.source.file.as_deref().unwrap_or("<unknown>"),
            interval.end.source.line
        );
    }
    for event in &report.incomplete {
        let _ = writeln!(
            out,
            "{}: {} at {}:{}",
            event.pair,
            event.reason,
            event.event.source.file.as_deref().unwrap_or("<unknown>"),
            event.event.source.line
        );
    }
    out
}
