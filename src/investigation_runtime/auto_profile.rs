//! Conservative built-in grammar inference on bounded, immutable capture prefixes.
use super::Capture;
use crate::{
    config::{self, AnalyzerConfig},
    event_rules::ClassifiedRecord,
    parser,
    processing::Budget,
};
use serde_json::{Value, json};
use std::path::PathBuf;

const INPUT_BYTES: usize = 64 * 1024;
const TOTAL_BYTES: usize = 256 * 1024;
const INPUT_LINES: usize = 128;

fn prefix(capture: &Capture, byte_limit: usize) -> usize {
    let limit = capture.data.len().min(byte_limit);
    let mut end = 0;
    let mut lines = 0;
    for (offset, byte) in capture.data[..limit].iter().enumerate() {
        if *byte == b'\n' {
            end = offset + 1;
            lines += 1;
            if lines == INPUT_LINES {
                return end;
            }
        }
    }
    if capture.complete && limit == capture.data.len() {
        limit
    } else {
        end
    }
}

pub(super) fn detect(
    paths: &[PathBuf],
    captures: &[Option<Capture>],
    budget: &mut Budget,
) -> (Option<AnalyzerConfig>, Value, usize) {
    let work_before = budget.work_units;
    let per_input = INPUT_BYTES.min(TOTAL_BYTES / paths.len().max(1));
    let lengths: Vec<_> = captures
        .iter()
        .map(|c| c.as_ref().map_or(0, |c| prefix(c, per_input)))
        .collect();
    let samples: Vec<_> = captures.iter().zip(&lengths).enumerate().map(|(ordinal, (capture, length))| {
        json!({"input_ordinal":ordinal,"sample_bytes":length,"captured_bytes":capture.as_ref().map(|c| c.data.len()),
            "entire_input":capture.as_ref().is_some_and(|c| c.complete && *length == c.data.len())})
    }).collect();
    let mut insufficient = captures.iter().zip(&lengths).any(|(c, len)| {
        c.as_ref()
            .is_none_or(|c| !c.complete || (!c.data.is_empty() && *len == 0))
    });
    let mut passes = 0;
    let mut records = 0;
    let mut candidates = Vec::new();
    let mut matches = Vec::new();
    for name in config::builtin_template_names()
        .iter()
        .filter(|name| **name != "base")
    {
        if !budget.checkpoint("profile_detection", 1) {
            break;
        }
        let memory_before = budget.memory_bytes;
        let records_before = budget.records;
        let expanded_before = budget.expanded_records;
        let candidate = config::load_builtin_template(name).expect("valid embedded profile");
        let bytes = serde_json::to_vec(&candidate)
            .expect("profile serializes")
            .len() as u64;
        let passes_before = passes;
        let mut parse_failures = 0;
        let mut matched = 0;
        let mut matched_inputs = 0;
        let mut invalid = 0;
        if budget.reserve("profile_detection", bytes.saturating_mul(16)) {
            for (ordinal, (capture, length)) in captures.iter().zip(&lengths).enumerate() {
                if budget.stop.is_some() {
                    break;
                }
                let Some(capture) = capture else {
                    continue;
                };
                if *length == 0 {
                    continue;
                }
                budget.active_scope = ordinal;
                // Scratch allocations are released after each probe. Work/time/cancellation
                // stay charged; probe records are reported separately from analysis records.
                let scratch_memory = budget.memory_bytes;
                passes += 1;
                match parser::parse_capture(
                    &paths[ordinal],
                    &capture.data[..*length],
                    capture.complete && *length == capture.data.len(),
                    &candidate,
                    budget,
                ) {
                    Ok(parsed) => {
                        records += parsed.entries.len();
                        insufficient |= parsed.coverage.is_unparsed()
                            || parsed.coverage.rejected_candidates > 0
                            || !parsed.coverage.normalization_diagnostics.is_empty();
                        let mut input_matches = 0;
                        for entry in &parsed.entries {
                            match &entry.classification {
                                Some(ClassifiedRecord::Event { semantics, .. })
                                    if semantics.phase.is_some() =>
                                {
                                    input_matches += 1
                                }
                                Some(
                                    ClassifiedRecord::Conflict { .. }
                                    | ClassifiedRecord::Invalid { .. },
                                ) => invalid += 1,
                                _ => {}
                            }
                        }
                        matched += input_matches;
                        matched_inputs += usize::from(input_matches > 0);
                    }
                    Err(_) => {
                        parse_failures += 1;
                        insufficient = true;
                    }
                }
                budget.memory_bytes = scratch_memory;
                budget.records = records_before;
                budget.expanded_records = expanded_before;
            }
        }
        insufficient |= invalid > 0;
        let performed = passes > passes_before;
        candidates.push(json!({"profile":name,
            "status":if !performed {"not_performed"} else if budget.stop.is_some() || parse_failures > 0 {"partial"} else {"complete"},
            "parse_passes":passes - passes_before,"parse_failures":parse_failures,
            "lifecycle_records":performed.then_some(matched),
            "matched_inputs":performed.then_some(matched_inputs),
            "invalid_or_conflicting_records":performed.then_some(invalid)}));
        if matched > 0 {
            matches.push((*name, matched_inputs));
        }
        drop(candidate);
        budget.memory_bytes = memory_before;
    }
    let nonempty_inputs = captures
        .iter()
        .filter(|c| c.as_ref().is_some_and(|c| !c.data.is_empty()))
        .count();
    let status = if budget.stop.is_some() {
        "budget_stopped"
    } else if insufficient {
        "insufficient_evidence"
    } else if matches.len() > 1 {
        "ambiguous"
    } else if matches.len() == 1 && matches[0].1 == nonempty_inputs {
        "selected"
    } else if matches.is_empty() {
        "no_match"
    } else {
        "insufficient_evidence"
    };
    let selected = (status == "selected")
        .then(|| config::load_builtin_template(matches[0].0).expect("valid embedded profile"));
    let metadata = json!({
        "status":status,"profile":selected.as_ref().map_or("base", |c| c.profile_name.as_str()),
        "method":"unique_builtin_lifecycle_grammar","candidates":candidates,"samples":samples,
        "limits":{"total_sample_bytes":TOTAL_BYTES,"sample_bytes_per_input":per_input,"physical_lines_per_input":INPUT_LINES},
        "probe_records":records,"work_units":budget.work_units.saturating_sub(work_before),
        "basis":"Bounded captured prefixes, using existing built-in parsers and classifiers. Every nonempty input must contain lifecycle evidence for the sole matching profile.",
        "limitations":"Grammar inference is not independent semantic validation or proof of completion. Unsampled records may differ. Ambiguous, unrecognized or insufficient evidence uses generic base analysis.",
        "next_step":"Inspect coverage, goal support and retained evidence. Override with --config or --preset (including base); use resolve-profile and validate-profile for a remaining semantic gap."
    });
    (selected, metadata, passes)
}
