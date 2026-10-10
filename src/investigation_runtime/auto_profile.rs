//! Conservative profile inference on bounded, immutable capture prefixes.
mod catalog;
use super::Capture;
use crate::{config::AnalyzerConfig, event_rules::ClassifiedRecord, parser, processing::Budget};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

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
    directory: &Path,
    optional_directory: bool,
) -> (Option<AnalyzerConfig>, Value, usize, Vec<PathBuf>) {
    let work_before = budget.work_units;
    let memory_before_catalog = budget.memory_bytes;
    let mut catalog = catalog::load(directory, optional_directory, budget);
    let per_input = INPUT_BYTES.min(TOTAL_BYTES / paths.len().max(1));
    let lengths: Vec<_> = captures
        .iter()
        .map(|c| c.as_ref().map_or(0, |c| prefix(c, per_input)))
        .collect();
    let samples: Vec<_> = captures.iter().zip(&lengths).enumerate().map(|(ordinal, (capture, length))| {
        json!({"input_ordinal":ordinal,"sample_bytes":length,"captured_bytes":capture.as_ref().map(|c| c.data.len()),
            "entire_input":capture.as_ref().is_some_and(|c| c.complete && *length == c.data.len())})
    }).collect();
    let insufficient = captures.iter().zip(&lengths).any(|(c, len)| {
        c.as_ref()
            .is_none_or(|c| !c.complete || (!c.data.is_empty() && *len == 0))
    });
    let mut passes = 0;
    let mut records = 0;
    let mut candidates = Vec::new();
    let mut matches = Vec::new();
    for (index, candidate) in catalog.candidates.iter().enumerate() {
        if !budget.checkpoint("profile_detection", 1) {
            break;
        }
        let memory_before = budget.memory_bytes;
        let records_before = budget.records;
        let expanded_before = budget.expanded_records;
        let mut structural_loss = false;
        let passes_before = passes;
        let mut parse_failures = 0;
        let mut matched = 0;
        let mut resource_records = 0;
        let mut matched_inputs = 0;
        let mut invalid = 0;
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
                &candidate.config,
                budget,
            ) {
                Ok(parsed) => {
                    records += parsed.entries.len();
                    structural_loss |= parsed.coverage.is_unparsed()
                        || parsed.coverage.rejected_candidates > 0
                        || !parsed.coverage.normalization_diagnostics.is_empty();
                    let mut input_matches = 0;
                    for entry in &parsed.entries {
                        if budget.stop.is_some() {
                            break;
                        }
                        let mut resource_match = false;
                        for rule in &candidate.config.resource_observations {
                            if !budget.checkpoint("profile_detection", 1) {
                                break;
                            }
                            resource_match |=
                                super::resource_observations::matches_sample(entry, rule, budget);
                        }
                        resource_records += usize::from(resource_match);
                        let mut lifecycle_match = false;
                        match &entry.classification {
                            Some(ClassifiedRecord::Event { semantics, .. })
                                if semantics.phase.is_some() =>
                            {
                                lifecycle_match = true
                            }
                            Some(
                                ClassifiedRecord::Conflict { .. }
                                | ClassifiedRecord::Invalid { .. },
                            ) => invalid += 1,
                            _ => {}
                        }
                        matched += usize::from(lifecycle_match);
                        input_matches += usize::from(lifecycle_match || resource_match);
                    }
                    matched_inputs += usize::from(input_matches > 0);
                }
                Err(_) => {
                    parse_failures += 1;
                    structural_loss = true;
                }
            }
            budget.memory_bytes = scratch_memory;
            budget.records = records_before;
            budget.expanded_records = expanded_before;
        }
        let performed = passes > passes_before;
        candidates.push(json!({"profile":candidate.config.profile_name,"profile_sha256":candidate.digest,"analysis_sha256":candidate.analysis_digest,"origins":candidate.origins,
            "status":if !performed {"not_performed"} else if budget.stop.is_some() || parse_failures > 0 {"partial"} else {"complete"},
            "parse_passes":passes - passes_before,"parse_failures":parse_failures,"structural_loss":structural_loss,
            "lifecycle_records":performed.then_some(matched),
            "resource_records":performed.then_some(resource_records),
            "matched_inputs":performed.then_some(matched_inputs),
            "invalid_or_conflicting_records":performed.then_some(invalid)}));
        if matched + resource_records > 0 {
            matches.push((
                index,
                matched_inputs,
                !structural_loss && invalid == 0 && parse_failures == 0,
            ));
        }
        budget.memory_bytes = memory_before;
    }
    let nonempty_inputs = captures
        .iter()
        .zip(&lengths)
        .filter(|(capture, length)| {
            capture.as_ref().is_some_and(|capture| {
                // Only a fully sampled input can be proven blank. A blank prefix
                // must not hide later records outside the detection allowance.
                !(capture.complete
                    && **length == capture.data.len()
                    && std::str::from_utf8(&capture.data[..**length])
                        .is_ok_and(|text| text.trim().is_empty()))
            })
        })
        .count();
    let status = if budget.stop.is_some() {
        "budget_stopped"
    } else if insufficient || !catalog.complete {
        "insufficient_evidence"
    } else if matches.len() > 1 {
        "ambiguous"
    } else if matches.len() == 1 && matches[0].1 == nonempty_inputs && matches[0].2 {
        "selected"
    } else if matches.is_empty() {
        "no_match"
    } else {
        "insufficient_evidence"
    };
    let selected =
        (status == "selected").then(|| catalog.candidates.swap_remove(matches[0].0).config);
    let metadata = json!({
        "status":status,"profile":selected.as_ref().map_or("base", |c| c.profile_name.as_str()),
        "method":"unique_profile_configured_grammar","candidates":candidates,"samples":samples,"discovery":catalog.metadata,
        "limits":{"total_sample_bytes":TOTAL_BYTES,"sample_bytes_per_input":per_input,"physical_lines_per_input":INPUT_LINES},
        "probe_records":records,"work_units":budget.work_units.saturating_sub(work_before),
        "basis":"Bounded captured prefixes, using existing profile parsers and classifiers. Every nonempty input must contain lifecycle or configured resource evidence for the sole matching profile.",
        "limitations":"Grammar inference is not independent semantic validation or proof of completion. Unsampled records may differ. Ambiguous, unrecognized or insufficient evidence uses generic base analysis.",
        "next_step":"Inspect coverage, goal support and retained evidence. Override with --profile (including base); use profile resolve and profile validate for a remaining semantic gap."
    });
    let sources = std::mem::take(&mut catalog.sources);
    drop(catalog);
    budget.memory_bytes = memory_before_catalog;
    (selected, metadata, passes, sources)
}

// Keep only analyzer-owned diagnostics; profile labels, paths and rules never
// enter this summary, so it can survive omission of the original query.
pub(super) fn summary(selection: &Value) -> Value {
    fn pick(value: &Value, keys: &[&str]) -> Value {
        let mut result = json!({});
        for key in keys {
            if let Some(value) = value.get(key) {
                result[*key] = value.clone();
            }
        }
        result
    }
    let mut result = pick(
        selection,
        &["status", "method", "limits", "probe_records", "work_units"],
    );
    for (collection, keys) in [
        (
            "samples",
            &[
                "input_ordinal",
                "sample_bytes",
                "captured_bytes",
                "entire_input",
            ][..],
        ),
        (
            "candidates",
            &[
                "status",
                "parse_passes",
                "parse_failures",
                "structural_loss",
                "lifecycle_records",
                "resource_records",
                "matched_inputs",
                "invalid_or_conflicting_records",
            ][..],
        ),
    ] {
        if let Some(values) = selection[collection].as_array() {
            result[collection] = values.iter().map(|value| pick(value, keys)).collect();
        }
    }
    if selection["discovery"].is_object() {
        result["discovery"] = pick(
            &selection["discovery"],
            &["complete", "limits", "read_bytes"],
        );
        result["discovery"]["diagnostics"] = selection["discovery"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| pick(value, &["reason"]))
            .collect();
    }
    result
}
