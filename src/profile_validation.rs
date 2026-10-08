//! Sample-scoped profile suitability; classification and timing use existing engines.
use crate::{
    comparator::LogFilter,
    config::{AnalyzerConfig, OperationKind},
    event_rules::{ClassifiedRecord, Phase},
    parser::LogEntry,
    perf_analyzer::{self, SourceLocation},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Clone, Copy, clap::ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Recognition,
    Timing,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expectations {
    version: u32,
    #[serde(default)]
    records: Vec<RecordFact>,
    #[serde(default)]
    pairs: Vec<PairFact>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Address {
    input: usize,
    line: usize,
    #[serde(deserialize_with = "required_row_path")]
    row_path: Option<String>,
}
fn required_row_path<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordFact {
    source: Address,
    checks: BTreeMap<String, Value>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairFact {
    start: Address,
    end: Address,
    duration_ms: Option<u64>,
}

pub fn load_expectations(
    path: Option<&Path>,
) -> Result<(Option<Expectations>, Option<String>), Box<dyn std::error::Error>> {
    let Some(path) = path else {
        return Ok((None, None));
    };
    let bytes = std::fs::read(path)?;
    let facts: Expectations = serde_json::from_slice(&bytes)?;
    if facts.version != 1 || facts.records.is_empty() && facts.pairs.is_empty() {
        return Err(
            "Expected facts require version 1 and at least one record or pair assertion".into(),
        );
    }
    for record in &facts.records {
        if record
            .checks
            .iter()
            .any(|(pointer, value)| !valid_assertion(pointer, value))
        {
            return Err("Expected classification assertions have unsupported pointers or invalid value types".into());
        }
        if record.checks.is_empty()
            || record.checks.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "/status"
                        | "/profile"
                        | "/rule_ids"
                        | "/kinds"
                        | "/semantics/kind"
                        | "/semantics/name"
                        | "/semantics/phase"
                        | "/semantics/outcome"
                        | "/semantics/correlation_id"
                        | "/semantics/scope"
                        | "/semantics/end_expected"
                )
            })
        {
            return Err(
                "Each expected record needs nonempty supported classification JSON Pointer checks"
                    .into(),
            );
        }
    }
    let addresses = facts
        .records
        .iter()
        .map(|f| &f.source)
        .chain(facts.pairs.iter().flat_map(|f| [&f.start, &f.end]));
    for address in addresses {
        if address.line == 0
            || address
                .row_path
                .as_ref()
                .is_some_and(|path| !path.is_empty() && !path.starts_with('/'))
        {
            return Err(
                "Expected sources require a physical line >=1 and a JSON Pointer row_path or null"
                    .into(),
            );
        }
    }
    Ok((Some(facts), Some(crate::evidence::digest(&bytes))))
}

fn valid_assertion(pointer: &str, value: &Value) -> bool {
    let choice = |choices: &[&str]| value.as_str().is_some_and(|s| choices.contains(&s));
    match pointer {
        "/status" => choice(&["event", "unclassified", "conflict", "invalid"]),
        "/semantics/kind" => choice(&["request", "event", "command"]),
        "/semantics/phase" => value.is_null() || choice(&["start", "end"]),
        "/semantics/outcome" => value.is_null() || choice(&["success", "failure"]),
        "/semantics/end_expected" => value.is_boolean(),
        "/semantics/correlation_id" => value.is_null() || value.is_string(),
        "/semantics/name" | "/profile" => value.is_string(),
        "/semantics/scope" | "/rule_ids" => value
            .as_array()
            .is_some_and(|a| a.iter().all(Value::is_string)),
        "/kinds" => value.as_array().is_some_and(|a| {
            a.iter().all(|v| {
                v.as_str()
                    .is_some_and(|s| ["request", "event", "command"].contains(&s))
            })
        }),
        _ => false,
    }
}

fn resolve<'a>(
    address: &Address,
    inputs: &'a [Vec<LogEntry>],
    filter: &LogFilter,
) -> Result<&'a LogEntry, &'static str> {
    let Some(input) = inputs.get(address.input) else {
        return Err("missing_input");
    };
    let matches: Vec<_> = input
        .iter()
        .filter(|entry| {
            entry.source_line_number == address.line && entry.source_row_path == address.row_path
        })
        .collect();
    if matches.is_empty() {
        return Err("missing_source");
    }
    if matches.len() != 1 {
        return Err("ambiguous_source");
    }
    if !filter.matches(matches[0]) {
        return Err("filtered_out_source");
    }
    Ok(matches[0])
}
fn same_source(source: &SourceLocation, entry: &LogEntry) -> bool {
    source.file == entry.source_file
        && source.line == entry.source_line_number
        && source.row_path == entry.source_row_path
}
fn relevant(entry: &LogEntry, kind: OperationKind) -> bool {
    match &entry.classification {
        Some(ClassifiedRecord::Event { semantics, .. }) => semantics.kind == kind,
        Some(
            ClassifiedRecord::Conflict { kinds, .. } | ClassifiedRecord::Invalid { kinds, .. },
        ) => kinds.is_empty() || kinds.contains(&kind),
        _ => false,
    }
}

pub fn analyze(
    inputs: &[Vec<LogEntry>],
    config: &AnalyzerConfig,
    filter: &LogFilter,
    kind: OperationKind,
    purpose: Purpose,
    expectations: Option<&Expectations>,
    expected_digest: Option<String>,
) -> Value {
    let logs: Vec<_> = inputs
        .iter()
        .flatten()
        .filter(|entry| filter.matches(entry))
        .cloned()
        .collect();
    let requested: Vec<_> = logs
        .iter()
        .filter(|entry| relevant(entry, kind))
        .cloned()
        .collect();
    let global =
        perf_analyzer::analyze_performance_with_config(&logs, &LogFilter::new(), None, config);
    let mut analysis =
        perf_analyzer::analyze_performance_with_config(&requested, &LogFilter::new(), None, config);
    analysis.operations.sort_by(|a, b| {
        a.start_time.cmp(&b.start_time).then_with(|| {
            (
                &a.start_source.file,
                a.start_source.line,
                &a.start_source.row_path,
            )
                .cmp(&(
                    &b.start_source.file,
                    b.start_source.line,
                    &b.start_source.row_path,
                ))
        })
    });
    let mut records = Vec::new();
    for (input, entries) in inputs.iter().enumerate() {
        for entry in entries.iter().filter(|entry| filter.matches(entry)) {
            let scope_origin = match &entry.classification {
                Some(ClassifiedRecord::Event { legacy: true, .. }) => "legacy_record_fields",
                Some(ClassifiedRecord::Event { rule_ids, .. })
                    if config.event_classifier().is_some_and(|rules| {
                        rules.schema().rules.iter().any(|rule| {
                            rule_ids.contains(&rule.id) && !rule.mapping.scope.is_empty()
                        })
                    }) =>
                {
                    "explicit_rule"
                }
                Some(ClassifiedRecord::Event { .. }) => "inherited_record_fields",
                _ => "unavailable",
            };
            records.push(json!({"input_ordinal":input,"source_file":entry.source_file,"source_line_number":entry.source_line_number,"source_row_path":entry.source_row_path,
                "timestamp":entry.source_timestamp.map(|t|t.to_rfc3339()).unwrap_or_else(||entry.timestamp.to_rfc3339()),"timestamp_year_inferred":entry.timestamp_year_inferred,
                "timestamp_offset_source":if entry.source_timestamp.is_some(){"source"}else{"local_assumption"},"classification":entry.classification,"requested_kind":relevant(entry,kind),"scope_origin":scope_origin}));
        }
    }
    let mut diagnostics = Vec::new();
    for event in &analysis.unmatched_events {
        diagnostics.push(json!({"reason":event.reason,"source":event.source,"classification":event.classification}));
    }
    for group in &analysis.ambiguous_groups {
        diagnostics.push(json!({"reason":"ambiguous_pairing","witnesses":group.events.iter().map(|event| &event.source).collect::<Vec<_>>()}));
    }
    for entry in &requested {
        if let Some(ClassifiedRecord::Event { semantics, .. }) = &entry.classification
            && semantics.phase == Some(Phase::Start)
            && !semantics.end_expected
        {
            diagnostics.push(json!({"reason":"intentional_start_only","source":crate::evidence::source(entry),"classification":entry.classification}));
        }
    }
    // Different source identities under one explicit pairing key are witnesses of
    // possible aliasing, not proof that component IDs are the intended domain scope.
    type ScopeKey = (String, String, Vec<String>);
    let mut identities: BTreeMap<ScopeKey, (BTreeSet<Vec<String>>, Vec<SourceLocation>)> =
        BTreeMap::new();
    let mut witness_config = config.clone();
    for entry in &requested {
        if let Some(ClassifiedRecord::Event {
            semantics,
            legacy: false,
            ..
        }) = &entry.classification
            && let Some(id) = &semantics.correlation_id
        {
            let group = identities
                .entry((semantics.name.clone(), id.clone(), semantics.scope.clone()))
                .or_default();
            let mut source_identity = vec![entry.component_id.clone()];
            for field in &config.perf.correlation_scope_fields {
                witness_config.perf.correlation_scope_fields = vec![field.clone()];
                let value = crate::parser::record_correlation_scope(entry, &witness_config)
                    .and_then(|values| values.into_iter().next());
                source_identity.push(format!(
                    "{field}={}",
                    value.as_deref().unwrap_or("<unavailable>")
                ));
            }
            group.0.insert(source_identity);
            group.1.push(crate::evidence::source(entry));
        }
    }
    let mut scope_aliases = 0;
    for ((name, correlation_id, scope), (identities, witnesses)) in identities {
        if identities.len() > 1 {
            scope_aliases += 1;
            diagnostics.push(json!({"reason":"scope_adequacy_unknown","name":name,"correlation_id":correlation_id,"scope":scope,"source_identities":identities,"witnesses":witnesses}));
        }
    }
    let mut checks = Vec::new();
    if let Some(facts) = expectations {
        for fact in &facts.records {
            match resolve(&fact.source, inputs, filter) {
                Err(reason) => checks.push(json!({"type":"record","address":fact.source,"status":"failed","reason":reason})),
                Ok(entry) => {
                    let observed = serde_json::to_value(&entry.classification).unwrap();
                    for (pointer, expected) in &fact.checks {
                        // end_expected defaults to true and is omitted by typed serialization.
                        let actual = if pointer == "/semantics/end_expected" && observed.get("semantics").is_some() { Some(observed.pointer(pointer).cloned().unwrap_or(json!(true))) } else { observed.pointer(pointer).cloned() };
                        let passed = actual.as_ref() == Some(expected);
                        checks.push(json!({"type":"record","address":fact.source,"source":crate::evidence::source(entry),"pointer":pointer,"expected":expected,"observed":actual,"observed_available":actual.is_some(),"status":if passed {"passed"} else {"failed"},"reason":if passed {"matches_expected_fact"}else if actual.is_none(){"missing_classification_field"}else{"semantic_mismatch"}}));
                    }
                }
            }
        }
        for fact in &facts.pairs {
            let resolved = resolve(&fact.start, inputs, filter)
                .and_then(|start| resolve(&fact.end, inputs, filter).map(|end| (start, end)));
            match resolved {
                Err(reason) => checks.push(json!({"type":"pair","start_address":fact.start,"end_address":fact.end,"status":"failed","reason":reason})),
                Ok((start,end)) => {
                    let matches:Vec<_> = analysis.operations.iter().filter(|op|same_source(&op.start_source,start)&&same_source(&op.end_source,end)).collect();
                    let passed = matches.len()==1 && fact.duration_ms.is_none_or(|expected|u64::try_from(matches[0].duration_ms).ok()==Some(expected));
                    checks.push(json!({"type":"pair","start_address":fact.start,"end_address":fact.end,"start_source":crate::evidence::source(start),"end_source":crate::evidence::source(end),"expected_duration_ms":fact.duration_ms,"observed_duration_ms":if matches.len()==1{Some(matches[0].duration_ms)}else{None},"status":if passed{"passed"}else{"failed"},"reason":if passed{"matches_expected_pair"}else{"pair_mismatch_or_unavailable"}}));
                }
            }
        }
    }
    let failed = checks
        .iter()
        .filter(|check| check["status"] == "failed")
        .count();
    let coverage = &analysis.operation_coverage;
    let c = &coverage.classification;
    let chronology_assumed = requested
        .iter()
        .any(|entry| entry.timestamp_year_inferred || entry.source_timestamp.is_none());
    let status = if c.conflicting_records > 0
        || matches!(purpose, Purpose::Timing) && coverage.ambiguous_groups > 0
    {
        "conflicting"
    } else if failed > 0 {
        "unsupported"
    } else if logs.is_empty() {
        "insufficient_evidence"
    } else if c.classified_records == 0 && c.invalid_records == 0 {
        "unsupported"
    } else if c.invalid_records > 0 || matches!(purpose, Purpose::Timing) && scope_aliases > 0 {
        "insufficient_evidence"
    } else if matches!(purpose, Purpose::Recognition) {
        "supported"
    } else if analysis.operations.is_empty()
        && coverage.start_only_events > 0
        && coverage.unmatched_events == 0
    {
        "unsupported"
    } else if analysis.operations.is_empty() || coverage.unmatched_events > 0 || chronology_assumed
    {
        "insufficient_evidence"
    } else {
        "supported"
    };
    let reason = match status {
        "supported" => "requested_analysis_supported_on_observed_sample",
        "conflicting" => "contradictory_rules_or_pairing_boundaries",
        "unsupported" if failed > 0 => "expected_facts_failed",
        "unsupported" if c.classified_records == 0 => "requested_lifecycle_not_recognized",
        "unsupported" => "intentional_start_only_has_no_timing_boundary",
        _ => "identity_scope_boundary_or_capture_evidence_insufficient",
    };
    let mut suggestions: Vec<Value> = diagnostics.iter().map(|diagnostic| json!({"action":"review_rule_or_scope_in_separate_candidate","reason":diagnostic["reason"],"source":diagnostic.get("source"),"witnesses":diagnostic.get("witnesses"),"unknown":"Intended lifecycle and scope require user or agent review"})).collect();
    if suggestions.is_empty()
        && status != "supported"
        && let Some(entry) = logs.first()
    {
        suggestions.push(json!({"action":"review_requested_lifecycle_in_separate_candidate","reason":reason,"source":crate::evidence::source(entry),"unknown":"No lifecycle meaning is inferred from wording similarity"}));
    }
    json!({"profile_validation":{"version":1,"requested":{"kind":kind,"purpose":purpose},"suitability":{"status":status,"reason":reason,"basis":"observed_sample_only","semantic_correctness":"not_established_by_match_count"},
        "global_classification":global.operation_coverage.classification,"requested_coverage":coverage,
        "totals":{"records":records.len(),"diagnostics":diagnostics.len(),"operations":analysis.operations.len(),"expected_checks":checks.len(),"failed_expected_checks":failed,"scope_alias_groups":scope_aliases},
        "expected_facts":{"sha256":expected_digest,"status":if expectations.is_some(){if failed==0{"passed"}else{"failed"}}else{"not_supplied"}},
        "effective_rules":{"event_rules":config.event_rules,"command_rules":config.command_rules,"inherited_scope_fields":config.perf.correlation_scope_fields,"legacy_marker_compatibility":true},
        "records":records,"diagnostics":diagnostics,"operations":analysis.operations,"expected_results":checks,
        "suggestions":suggestions,
        "limitations":["Support describes selected observed sample, not all future logs","Upstream capture completeness and intended domain scope remain unknown","Parsing or rule-match counts alone do not establish semantic correctness"]}})
}
