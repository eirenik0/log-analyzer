use super::selection::View;
use crate::{
    event_rules::ClassifiedRecord,
    evidence,
    parser::LogEntry,
    perf_analyzer::{PerfAnalysisResults, SourceLocation},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn verification(redacted: bool, arithmetic: &str) -> Value {
    json!({"artifact_integrity":"available","arithmetic":arithmetic,"source_and_rules":if redacted{"unavailable"}else{"available"},"losses":if redacted{vec!["Source payloads, fields, identities and effective profile omitted by redaction"]}else{Vec::<&str>::new()}})
}
pub(super) fn occurrence(entry: &LogEntry, context: &evidence::Context, snapshot: &Value) -> Value {
    let mut source = evidence::source(entry);
    context.attach_source(&mut source);
    json!({"snapshot_id":snapshot,"input_ordinal":entry.source_input_ordinal,"evidence_ref":source.evidence_ref})
}
pub(super) fn record(entry: &LogEntry, context: &evidence::Context, snapshot: &Value) -> Value {
    json!({"occurrence":occurrence(entry,context,snapshot),"entity":if entry.source_row_path.is_some(){"normalized_records"}else{"physical_records"},"timestamp":entry.source_timestamp.map(|time|time.to_rfc3339()),"timestamp_year_source":if entry.timestamp_year_inferred{"inferred"}else{"source"},"timestamp_offset_source":if entry.source_timestamp.is_some(){"source"}else{"assumed"},"raw_text":entry.raw_logline,"message":entry.message,"fields":{"structured_fields":entry.structured_fields,"envelope_payload":entry.envelope_payload,"payload":entry.payload(),"classification":entry.classification},"data_omitted":false,"verification":verification(false,"not_applicable")})
}
pub(super) fn excerpt(entry: &LogEntry, context: &evidence::Context, snapshot: &Value) -> Value {
    let text: String = entry.message.chars().take(512).collect();
    json!({"occurrence":occurrence(entry,context,snapshot),"text":text,"omitted_characters":entry.message.chars().count().saturating_sub(512),"verification":verification(false,"not_applicable")})
}
pub(super) fn rules(entry: &LogEntry) -> Vec<String> {
    match &entry.classification {
        Some(
            ClassifiedRecord::Event { rule_ids, .. } | ClassifiedRecord::Conflict { rule_ids, .. },
        ) => rule_ids.clone(),
        Some(ClassifiedRecord::Invalid { diagnostics, .. }) => diagnostics
            .iter()
            .filter_map(|d| d.rule_id.clone())
            .collect(),
        _ => Vec::new(),
    }
}
pub(super) fn fact(
    id: String,
    scope: &str,
    kind: &str,
    claim: &str,
    evidence: Vec<Value>,
    details: Value,
) -> Value {
    json!({"id":id,"scope_id":scope,"kind":kind,"claim":claim,"author":"rust","limitations":["Only the selected processed population is measured; upstream completeness and clock synchronization are unknown."],"verification":verification(false,if kind=="measurement" || kind=="calculated_fact"{"available"}else{"not_applicable"}),"evidence":evidence,"details":details})
}
#[allow(clippy::too_many_arguments)]
pub(super) fn population(
    scope: &str,
    suffix: &str,
    entity: &str,
    definition: &str,
    identity: Vec<String>,
    rule_ids: Vec<String>,
    members: Vec<Value>,
    findings: &mut Vec<Value>,
    populations: &mut Vec<Value>,
    memberships: &mut Vec<Value>,
) -> String {
    let id = format!("{scope}-{suffix}");
    let count = members.len();
    let hash = evidence::digest(serde_json::to_string(&members).unwrap().as_bytes());
    populations.push(json!({"id":id,"scope_id":scope,"entity":entity,"definition":definition,"identity_fields":identity,"rule_ids":rule_ids,"exclusions":[],"completeness":"complete","basis":"processed_population","count":count,"membership":{"collection":format!("/memberships/{}/members",memberships.len()),"count":count,"sha256":hash}}));
    memberships.push(json!({"population_id":id,"members":members}));
    findings.push(fact(format!("{id}-count"),scope,"calculated_fact",definition,Vec::new(),json!({"calculation":"count","population_id":id,"value":count,"unit":if entity.ends_with("records"){"records"}else{entity},"semantics":"cardinality_of_declared_processed_population","method":"cardinality"})));
    id
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    entries: &[LogEntry],
    perf: Option<&PerfAnalysisResults>,
    view: &View<'_>,
    scope: &str,
    context: &evidence::Context,
    snapshot: &Value,
    profile_digest: &Value,
    threshold: u64,
    budget: &mut crate::processing::Budget,
    findings: &mut Vec<Value>,
    populations: &mut Vec<Value>,
    memberships: &mut Vec<Value>,
    sequences: &mut Vec<Value>,
) {
    if !budget.checkpoint(
        "calculation",
        (entries.len() as u64).saturating_mul(u64::from(entries.len().max(1).ilog2()) + 16),
    ) {
        return;
    }
    let retained = &view.entries;
    for (entity, suffix) in [
        ("physical_records", "physical-records"),
        ("normalized_records", "normalized-records"),
    ] {
        let members: Vec<_> = retained
            .iter()
            .filter(|entry| (entry.source_row_path.is_some()) == (entity == "normalized_records"))
            .map(|entry| json!({"kind":"record","occurrence":occurrence(entry,context,snapshot)}))
            .collect();
        if entity == "physical_records" || !members.is_empty() {
            population(
                scope,
                suffix,
                entity,
                "Count of exact-selected parsed source occurrences; normalized rows are separate from physical records.",
                Vec::new(),
                Vec::new(),
                members,
                findings,
                populations,
                memberships,
            );
        }
    }
    let mut events: Vec<_> = retained.iter().enumerate().map(|(ordinal,entry)| {
        let status = match entry.classification {Some(ClassifiedRecord::Event{..})=>"classified",Some(ClassifiedRecord::Conflict{..})=>"conflicting",Some(ClassifiedRecord::Invalid{..})=>"invalid",_=>"unclassified"};
        (entry.source_timestamp,ordinal,json!({"occurrence":occurrence(entry,context,snapshot),"record_ordinal":ordinal,"timestamp":entry.source_timestamp.map(|t|t.to_rfc3339()),"classification_status":status,"rule_ids":rules(entry)}))
    }).collect();
    events.sort_by_key(|(timestamp, ordinal, _)| (timestamp.is_none(), *timestamp, *ordinal));
    sequences.push(json!({"id":format!("{scope}-events"),"scope_id":scope,"completeness":"complete","ordering":"observed_timestamp_then_input_ordinal_then_record_ordinal","events":events.into_iter().map(|(_,_,event)|event).collect::<Vec<_>>()}));
    let Some(perf) = perf else {
        findings.push(fact(format!("{scope}-lifecycle-unavailable"),scope,"unknown","Lifecycle calculations are unavailable.",Vec::new(),json!({"reason":"No supported explicit lifecycle profile or correlation processing was interrupted.","supporting_occurrences":[]})));
        return;
    };
    let by_source: BTreeMap<_, _> = entries
        .iter()
        .map(|entry| {
            (
                (entry.source_line_number, entry.source_row_path.as_deref()),
                entry,
            )
        })
        .collect();
    let lookup = |source: &SourceLocation| {
        by_source
            .get(&(source.line, source.row_path.as_deref()))
            .copied()
    };
    let mut operation_members = Vec::new();
    let mut measurement_ids = Vec::new();
    let mut durations = Vec::new();
    let mut operation_rules = BTreeSet::new();
    for (index, operation) in perf.operations.iter().enumerate() {
        if !budget.checkpoint("calculation", 1) {
            return;
        }
        let (Some(start), Some(end)) = (
            lookup(&operation.start_source),
            lookup(&operation.end_source),
        ) else {
            continue;
        };
        if !view.contains(start) || !view.contains(end) {
            continue;
        }
        let rule_ids: Vec<_> = rules(start).into_iter().chain(rules(end)).collect();
        operation_rules.extend(rule_ids);
        let measurement_id = format!("{scope}-interval-{index}");
        let reliable = !start.timestamp_year_inferred
            && !end.timestamp_year_inferred
            && start.source_timestamp.is_some()
            && end.source_timestamp.is_some();
        operation_members.push(json!({"kind":"operation","id":format!("{scope}-operation-{index}"),"identity":[{"field":"kind","value":operation.op_type},{"field":"name","value":operation.name},{"field":"correlation_id","value":operation.correlation_id},{"field":"scope","value":serde_json::to_string(&operation.scope).unwrap()}],"source_occurrences":[occurrence(start,context,snapshot),occurrence(end,context,snapshot)],"measurement_ids":if reliable{vec![measurement_id.clone()]}else{Vec::new()}}));
        if !reliable {
            findings.push(fact(format!("{scope}-interval-unavailable-{index}"),scope,"unknown","A paired lifecycle lacks source timestamp provenance required for an elapsed measurement.",vec![excerpt(start,context,snapshot),excerpt(end,context,snapshot)],json!({"reason":"Both boundaries require explicit source years and UTC offsets.","supporting_occurrences":[occurrence(start,context,snapshot),occurrence(end,context,snapshot)]})));
            continue;
        }
        let boundary = |entry: &LogEntry, event: &str| json!({"occurrence":occurrence(entry,context,snapshot),"timestamp":entry.source_timestamp.unwrap().to_rfc3339(),"timestamp_year_source":"source","timestamp_offset_source":"source","event":event,"rule_id":rules(entry).first().cloned().unwrap_or_else(||"legacy-markers".into())});
        findings.push(fact(measurement_id.clone(),scope,"measurement",if operation.duration_ms as u64 >= threshold {"Observed paired lifecycle meets the configured slow threshold; this elapsed interval does not measure CPU time."}else{"Observed elapsed interval between paired lifecycle boundaries."},vec![excerpt(start,context,snapshot),excerpt(end,context,snapshot)],json!({"value":operation.duration_ms,"unit":"ms","semantics":"start_to_finish","profile_sha256":profile_digest,"boundaries":{"start":boundary(start,"start"),"end":boundary(end,"end")},"clock_relationship":"unknown"})));
        measurement_ids.push(measurement_id);
        durations.push(operation.duration_ms);
    }
    // An empty observation can be a measured zero only when the profile supplies rules.
    if operation_rules.is_empty() {
        operation_rules.extend(retained.iter().flat_map(|entry| rules(entry)));
    }
    if !operation_rules.is_empty() {
        let population_id = population(
            scope,
            "paired-lifecycles",
            "operations",
            "Existing scoped start/end pairs; these are not logical retry attempts or distinct resources.",
            vec![
                "kind".into(),
                "name".into(),
                "correlation_id".into(),
                "scope".into(),
            ],
            operation_rules.into_iter().collect(),
            operation_members,
            findings,
            populations,
            memberships,
        );
        if !durations.is_empty() {
            durations.sort_unstable();
            let count = durations.len();
            for (statistic, value, method) in [
                ("minimum", durations[0], "minimum"),
                ("maximum", durations[count - 1], "maximum"),
                (
                    "p50",
                    durations[count * 50 / 100],
                    "sorted_index_floor_n_times_percentile_over_100",
                ),
                (
                    "p95",
                    durations[count * 95 / 100],
                    "sorted_index_floor_n_times_percentile_over_100",
                ),
                (
                    "p99",
                    durations[count * 99 / 100],
                    "sorted_index_floor_n_times_percentile_over_100",
                ),
            ] {
                findings.push(fact(format!("{population_id}-{statistic}"),scope,"calculated_fact","Distribution of source-backed elapsed intervals in the declared paired population.",Vec::new(),json!({"calculation":"distribution","population_id":population_id,"measurement_ids":measurement_ids,"sample_count":count,"value":value,"unit":"ms","statistic":statistic,"method":method})));
            }
        }
    }
    for (index, event) in perf.unmatched_events.iter().enumerate() {
        if !budget.checkpoint("calculation", 1) {
            return;
        }
        let Some(entry) = lookup(&event.source) else {
            continue;
        };
        if !view.contains(entry) {
            continue;
        }
        findings.push(fact(format!("{scope}-unmatched-{index}"),scope,"observation",match event.reason.as_str(){"missing_end"=>"A start has no observed end in this selected capture; this does not establish a hang.","missing_start"=>"An end has no observed start in this selected capture.","overlapping_starts"=>"Repeated overlapping starts prevent an unambiguous lifecycle pairing.","conflicting_event_rules"=>"Explicit rules assign conflicting event semantics.",_=>"The event could not form a reliable scoped lifecycle pair."},vec![excerpt(entry,context,snapshot)],json!({"supporting_occurrences":[occurrence(entry,context,snapshot)]})));
    }
    for (suffix, description) in [
        (
            "starts",
            "Classified starts, including repeated starts; no logical-attempt grouping is inferred.",
        ),
        (
            "ends",
            "Classified end records; an observed end alone does not prove a completed attempt.",
        ),
        (
            "failures",
            "Explicit failure outcomes; cached failures, poll responses and downstream effects are not inferred.",
        ),
        (
            "successes",
            "Explicit success outcomes; this is an event count, not a distinct-resource count.",
        ),
    ] {
        if matches!(suffix, "failures" | "successes") && !view.outcomes_supported {
            findings.push(fact(format!("{scope}-{suffix}-unavailable"), scope, "unknown", "Outcome counts are unavailable without an applicable selected outcome mapping.", Vec::new(), json!({"reason":"No selected classification rule declares outcome semantics for this processed population.","supporting_occurrences":[]})));
            continue;
        }
        if !budget.checkpoint("calculation", entries.len() as u64) {
            return;
        }
        let selected_events: Vec<_> = retained
            .iter()
            .filter(|entry| {
                if let Some(ClassifiedRecord::Event { semantics, .. }) = &entry.classification {
                    match suffix {
                        "starts" => semantics.phase == Some(crate::event_rules::Phase::Start),
                        "ends" => semantics.phase == Some(crate::event_rules::Phase::End),
                        "failures" => {
                            semantics.outcome == Some(crate::event_rules::Outcome::Failure)
                        }
                        _ => semantics.outcome == Some(crate::event_rules::Outcome::Success),
                    }
                } else {
                    false
                }
            })
            .collect();
        let rule_ids: BTreeSet<_> = retained.iter().flat_map(|entry| rules(entry)).collect();
        let members = selected_events.iter().map(|entry|{let source=occurrence(entry,context,snapshot);json!({"kind":"event","id":format!("{scope}-{suffix}-{}",source["evidence_ref"]["reference_id"].as_str().unwrap()),"identity":[{"field":"reference_id","value":source["evidence_ref"]["reference_id"]}],"source_occurrences":[source],"measurement_ids":[]})}).collect();
        population(
            scope,
            suffix,
            "events",
            description,
            vec!["reference_id".into()],
            rule_ids.into_iter().collect(),
            members,
            findings,
            populations,
            memberships,
        );
    }
    findings.push(fact(format!("{scope}-domain-grouping-unavailable"),scope,"unknown","Domain groupings and relationships outside explicit policy declarations remain unavailable.",Vec::new(),json!({"reason":"Operation names, repeated IDs and timestamp proximity do not establish domain grouping or causal relationships.","supporting_occurrences":[]})));
}

pub(super) fn scope_aliases(
    aliases: &[Value],
    entries: &[LogEntry],
    scope: &str,
    context: &evidence::Context,
    snapshot: &Value,
    budget: &mut crate::processing::Budget,
    findings: &mut Vec<Value>,
) {
    let by_source: BTreeMap<_, _> = entries
        .iter()
        .map(|entry| {
            (
                (entry.source_line_number, entry.source_row_path.clone()),
                entry,
            )
        })
        .collect();
    for (index, alias) in aliases.iter().enumerate() {
        let mut witnesses = Vec::new();
        for source in alias["witnesses"].as_array().unwrap() {
            if !budget.checkpoint("calculation", 1) {
                return;
            }
            if let Some(entry) = source["line"].as_u64().and_then(|line| {
                by_source.get(&(
                    line as usize,
                    source["row_path"].as_str().map(str::to_owned),
                ))
            }) {
                witnesses.push(*entry);
            }
        }
        if witnesses.is_empty() {
            continue;
        }
        findings.push(fact(format!("{scope}-scope-adequacy-{index}"),scope,"unknown","The selected population intersects a shared effective correlation key; all source-identity witnesses are retained as context, including any outside selection. Intended scope adequacy is unknown.",witnesses.iter().map(|entry|excerpt(entry,context,snapshot)).collect(),json!({"reason":"Source-identity witnesses do not prove intended domain scope. Review the explicit scope before relying on paired lifecycle meaning.","supporting_occurrences":witnesses.iter().map(|entry|occurrence(entry,context,snapshot)).collect::<Vec<_>>()})));
    }
}
