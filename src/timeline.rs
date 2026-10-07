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
pub struct TimelineReport {
    pub status: String,
    pub events: Vec<TimelineEvent>,
    pub sample_counts: BTreeMap<String, usize>,
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
    let capture_window = logs
        .iter()
        .map(|e| evidence_timestamp(e))
        .min()
        .zip(logs.iter().map(|e| evidence_timestamp(e)).max());
    let mut report = TimelineReport {
        status: "no_applicable_events".into(),
        events: Vec::new(),
        sample_counts: BTreeMap::new(),
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
            let Some(captures) = pattern.captures(&entry.raw_logline) else {
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
                source: SourceLocation {
                    file: entry.source_file.clone(),
                    line: entry.source_line_number,
                },
                raw: entry.raw_logline.clone(),
                gap_since_previous_match_ms: None,
            });
        }
    }
    report.events.sort_by_key(|e| e.timestamp);
    let mut previous = None;
    for event in &mut report.events {
        event.gap_since_previous_match_ms = previous.map(|time| {
            event
                .timestamp
                .signed_duration_since(time)
                .num_milliseconds()
        });
        previous = Some(event.timestamp);
    }
    for pair in &rules.pairs {
        let mut groups: BTreeMap<Vec<String>, Vec<&TimelineEvent>> = BTreeMap::new();
        for event in &report.events {
            if event.event_type != pair.start_event && event.event_type != pair.end_event {
                continue;
            }
            if let Some(key) = &event.key {
                groups.entry(key.clone()).or_default().push(event);
            } else {
                report.incomplete.push(IncompleteInterval {
                    pair: pair.name.clone(),
                    event: event.clone(),
                    reason: "missing_correlation_field".into(),
                });
            }
        }
        for (key, events) in groups {
            let mut active = false;
            let shared_boundary = events.windows(2).any(|pair| {
                pair[0].timestamp == pair[1].timestamp
                    && (pair[0].source.file != pair[1].source.file
                        || pair[0].source.line == pair[1].source.line)
            });
            if shared_boundary
                || events.iter().any(|event| {
                    let start = event.event_type == pair.start_event;
                    let overlap = start && active;
                    active = start;
                    overlap
                })
            {
                report
                    .ambiguous_groups
                    .push(events.iter().map(|e| (*e).clone()).collect());
                for event in events {
                    report.incomplete.push(IncompleteInterval {
                        pair: pair.name.clone(),
                        event: event.clone(),
                        reason: if shared_boundary {
                            "ambiguous_boundary"
                        } else {
                            "ambiguous_overlap"
                        }
                        .into(),
                    });
                }
                continue;
            }
            let mut pending: Option<&TimelineEvent> = None;
            for event in events {
                if event.event_type == pair.start_event {
                    pending = Some(event);
                } else if let Some(start) = pending.take() {
                    let gap = event
                        .timestamp
                        .signed_duration_since(start.timestamp)
                        .num_milliseconds();
                    let measured = (pair.timing == Timing::Measured).then_some(gap);
                    if let Some(ms) = measured {
                        *report.measured_work_sum_ms.get_or_insert(0) += ms;
                    }
                    report.intervals.push(TimelineInterval {
                        pair: pair.name.clone(),
                        key: key.clone(),
                        timing: pair.timing,
                        start: start.clone(),
                        end: event.clone(),
                        observed_gap_ms: gap,
                        measured_duration_ms: measured,
                    });
                } else {
                    report.incomplete.push(IncompleteInterval {
                        pair: pair.name.clone(),
                        event: event.clone(),
                        reason: "capture_started_after_start_or_missing_start".into(),
                    });
                }
            }
            if let Some(event) = pending {
                report.incomplete.push(IncompleteInterval {
                    pair: pair.name.clone(),
                    event: event.clone(),
                    reason: "capture_ended_before_end_or_missing_end".into(),
                });
            }
        }
    }
    if !report.events.is_empty() {
        report.status = if report.intervals.is_empty() || !report.incomplete.is_empty() {
            "insufficient_evidence"
        } else {
            "boundaries_available"
        }
        .into();
    }
    Ok(Some(report))
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
    for event in &report.events {
        let _ = writeln!(
            out,
            "{} [offset: {}] {} {:?}; gap since previous matched event {:?}ms at {}:{}",
            event.timestamp.to_rfc3339(),
            event.timestamp_offset_source,
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
