use super::{finding_store::FindingSink, selection::View, source_store::Entries};
use crate::{event_rules::ClassifiedRecord, evidence, parser::LogEntry};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(super) fn verification(redacted: bool, arithmetic: &str) -> Value {
    json!({"artifact_integrity":"available","arithmetic":arithmetic,"source_and_rules":if redacted{"unavailable"}else{"available"},"losses":if redacted{vec!["Source payloads, fields, identities and effective profile omitted by redaction"]}else{Vec::<&str>::new()}})
}
pub(super) fn occurrence(entry: &LogEntry, context: &evidence::Context, snapshot: &Value) -> Value {
    let mut source = evidence::source(entry);
    context.attach_source(&mut source);
    json!({"snapshot_id":snapshot,"input_ordinal":entry.source_input_ordinal,"evidence_ref":source.evidence_ref})
}
pub(super) fn record(entry: &LogEntry, context: &evidence::Context, snapshot: &Value) -> Value {
    json!({"occurrence":occurrence(entry,context,snapshot),"entity":if entry.source_row_path.is_some(){"normalized_records"}else{"physical_records"},"timestamp":entry.source_timestamp.map(|time|time.to_rfc3339()),"timestamp_year_source":if entry.timestamp_year_inferred{"inferred"}else{"source"},"timestamp_offset_source":if entry.source_timestamp.is_some(){"source"}else{"assumed"},"raw_text":entry.raw_logline,"message":entry.message,"fields":{"level":entry.level,"structured_fields":entry.structured_fields,"envelope_payload":entry.envelope_payload,"payload":entry.payload(),"classification":entry.classification},"data_omitted":false,"verification":verification(false,"not_applicable")})
}
pub(super) fn excerpt(entry: &LogEntry, context: &evidence::Context, snapshot: &Value) -> Value {
    let text: String = entry.message.chars().take(512).collect();
    let mut result = json!({"occurrence":occurrence(entry,context,snapshot),"text":text,"omitted_characters":entry.message.chars().count().saturating_sub(512),"verification":verification(false,"not_applicable")});
    if let Some(fields) = context.finding_fields(entry) {
        result["fields"] = fields;
    }
    result
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
    members: impl IntoIterator<Item = Value>,
    findings: &mut dyn FindingSink,
    populations: &mut Vec<Value>,
    memberships: &mut dyn super::construction::MemberSink,
) -> super::Result<String> {
    population_stream(
        scope,
        suffix,
        entity,
        definition,
        identity,
        rule_ids,
        members.into_iter().map(Ok),
        findings,
        populations,
        memberships,
    )
}
#[allow(clippy::too_many_arguments)]
pub(super) fn population_stream(
    scope: &str,
    suffix: &str,
    entity: &str,
    definition: &str,
    identity: Vec<String>,
    rule_ids: Vec<String>,
    members: impl IntoIterator<Item = super::Result<Value>>,
    findings: &mut dyn FindingSink,
    populations: &mut Vec<Value>,
    memberships: &mut dyn super::construction::MemberSink,
) -> super::Result<String> {
    let id = format!("{scope}-{suffix}");
    let membership_index = memberships.len();
    let (count, hash) = memberships.append_fallible(&id, &mut members.into_iter())?;
    populations.push(json!({"id":id,"scope_id":scope,"entity":entity,"definition":definition,"identity_fields":identity,"rule_ids":rule_ids,"exclusions":[],"completeness":"complete","basis":"processed_population","count":count,"membership":{"collection":format!("/memberships/{membership_index}/members"),"count":count,"sha256":hash}}));
    findings.push(fact(format!("{id}-count"),scope,"calculated_fact",definition,Vec::new(),json!({"calculation":"count","population_id":id,"value":count,"unit":if entity.ends_with("records"){"records"}else{entity},"semantics":"cardinality_of_declared_processed_population","method":"cardinality"})))?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    entries: &Entries,
    perf: Option<&super::Correlated>,
    view: &View,
    scope: &str,
    context: &evidence::Context,
    snapshot: &Value,
    profile_digest: &Value,
    threshold: u64,
    budget: &mut crate::processing::Budget,
    findings: &mut dyn FindingSink,
    populations: &mut Vec<Value>,
    memberships: &mut dyn super::construction::MemberSink,
    sequences: &mut super::construction::Sequences,
) -> super::Result<()> {
    if !budget.checkpoint(
        "calculation",
        (entries.len() as u64).saturating_mul(u64::from(entries.len().max(1).ilog2()) + 16),
    ) {
        return Ok(());
    }
    let retained = &view.entries;
    for (entity, suffix) in [
        ("physical_records", "physical-records"),
        ("normalized_records", "normalized-records"),
    ] {
        let normalized = entity == "normalized_records";
        let mut count = 0;
        for entry in retained.iter()? {
            if entry?.source_row_path.is_some() == normalized {
                count += 1;
            }
        }
        if !normalized || count > 0 {
            let members = retained.iter()?.filter_map(move |entry| match entry {
                Ok(entry) if entry.source_row_path.is_some() == normalized => Some(Ok(
                    json!({"kind":"record","occurrence":occurrence(&entry,context,snapshot)}),
                )),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            });
            population_stream(
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
            )?;
        }
    }
    let mut order = sequences.order()?;
    for (ordinal, entry) in retained.iter()?.enumerate() {
        let entry = entry?;
        let mut key = [0; 32];
        key[0] = u8::from(entry.source_timestamp.is_none());
        if let Some(timestamp) = entry.source_timestamp {
            key[1..9].copy_from_slice(&((timestamp.timestamp() as u64) ^ (1 << 63)).to_be_bytes());
            key[9..13].copy_from_slice(&timestamp.timestamp_subsec_nanos().to_be_bytes());
        }
        key[13..21].copy_from_slice(&(ordinal as u64).to_be_bytes());
        order.push(key)?;
    }
    let sorted = match order.sorted(|work| {
        if budget.checkpoint("calculation", work) {
            Ok(())
        } else {
            Err("sequence ordering interrupted".into())
        }
    }) {
        Ok(sorted) => sorted,
        Err(_) if budget.halted => return Ok(()),
        Err(error) => return Err(error),
    };
    let events = sorted.map(|key| -> super::Result<Value> {
        let key = key?;
        let ordinal = u64::from_be_bytes(key[13..21].try_into().unwrap()) as usize;
        let entry = retained.get(ordinal)?;
        let status = match entry.classification {Some(ClassifiedRecord::Event{..})=>"classified",Some(ClassifiedRecord::Conflict{..})=>"conflicting",Some(ClassifiedRecord::Invalid{..})=>"invalid",_=>"unclassified"};
        Ok(json!({"occurrence":occurrence(&entry,context,snapshot),"record_ordinal":ordinal,"timestamp":entry.source_timestamp.map(|t|t.to_rfc3339()),"classification_status":status,"rule_ids":rules(&entry)}))
    });
    sequences.append(json!({"id":format!("{scope}-events"),"scope_id":scope,"completeness":"complete","ordering":"observed_timestamp_then_input_ordinal_then_record_ordinal","events":[]}), events)?;
    let Some(perf) = perf else {
        findings.push(fact(format!("{scope}-lifecycle-unavailable"),scope,"unknown","Lifecycle calculations are unavailable.",Vec::new(),json!({"reason":"No supported explicit lifecycle profile or correlation processing was interrupted.","supporting_occurrences":[]})))?;
        return Ok(());
    };
    let mut operation_members = entries.rows::<Value>()?;
    let mut measurement_ids = entries.rows::<String>()?;
    let mut durations = crate::sorted_samples::Samples::new(entries.state_bytes)?;
    let mut duration_count = 0usize;
    let mut operation_rules = BTreeSet::new();
    for (index, operation) in perf.operations.iter()?.enumerate() {
        let operation = operation?;
        if !budget.checkpoint("calculation", 1) {
            return Ok(());
        }
        let (Some(start), Some(end)) = (
            entries.lookup(&operation.start_source)?,
            entries.lookup(&operation.end_source)?,
        ) else {
            continue;
        };
        if !view.contains(&start)? || !view.contains(&end)? {
            continue;
        }
        let rule_ids: Vec<_> = rules(&start).into_iter().chain(rules(&end)).collect();
        operation_rules.extend(rule_ids);
        let measurement_id = format!("{scope}-interval-{index}");
        let reliable = !start.timestamp_year_inferred
            && !end.timestamp_year_inferred
            && start.source_timestamp.is_some()
            && end.source_timestamp.is_some();
        operation_members.push(&json!({"kind":"operation","id":format!("{scope}-operation-{index}"),"identity":[{"field":"kind","value":operation.op_type},{"field":"name","value":operation.name},{"field":"correlation_id","value":operation.correlation_id},{"field":"scope","value":serde_json::to_string(&operation.scope).unwrap()}],"source_occurrences":[occurrence(&start,context,snapshot),occurrence(&end,context,snapshot)],"measurement_ids":if reliable{vec![measurement_id.clone()]}else{Vec::new()}}))?;
        if !reliable {
            findings.push(fact(format!("{scope}-interval-unavailable-{index}"),scope,"unknown","A paired lifecycle lacks source timestamp provenance required for an elapsed measurement.",vec![excerpt(&start,context,snapshot),excerpt(&end,context,snapshot)],json!({"reason":"Both boundaries require explicit source years and UTC offsets.","supporting_occurrences":[occurrence(&start,context,snapshot),occurrence(&end,context,snapshot)]})))?;
            continue;
        }
        let boundary = |entry: &LogEntry, event: &str| json!({"occurrence":occurrence(entry,context,snapshot),"timestamp":entry.source_timestamp.unwrap().to_rfc3339(),"timestamp_year_source":"source","timestamp_offset_source":"source","event":event,"rule_id":rules(entry).first().cloned().unwrap_or_else(||"legacy-markers".into())});
        findings.push(fact(measurement_id.clone(),scope,"measurement",if operation.duration_ms as u64 >= threshold {"Observed paired lifecycle meets the configured slow threshold; this elapsed interval does not measure CPU time."}else{"Observed elapsed interval between paired lifecycle boundaries."},vec![excerpt(&start,context,snapshot),excerpt(&end,context,snapshot)],json!({"value":operation.duration_ms,"unit":"ms","semantics":"start_to_finish","profile_sha256":profile_digest,"boundaries":{"start":boundary(&start,"start"),"end":boundary(&end,"end")},"clock_relationship":"unknown"})))?;
        measurement_ids.push(&measurement_id)?;
        durations.push((operation.duration_ms as u64) ^ (1 << 63))?;
        duration_count += 1;
    }
    if view.classification_loss > 0 {
        let mut unresolved = None;
        for entry in retained.iter()? {
            let entry = entry?;
            if matches!(
                entry.classification,
                Some(ClassifiedRecord::Conflict { .. } | ClassifiedRecord::Invalid { .. })
            ) {
                unresolved = Some(entry);
                break;
            }
        }
        let unresolved = unresolved
            .as_ref()
            .ok_or("classification witness disappeared")?;
        findings.push(fact(format!("{scope}-classification-unavailable"),scope,"unknown","Selected conflicting or invalid classifications prevent complete semantic populations.",vec![excerpt(unresolved,context,snapshot)],json!({"reason":format!("{} selected source records have unresolved semantics; this is not a missing event, operation or resource count.", view.classification_loss),"supporting_occurrences":[occurrence(unresolved,context,snapshot)]})))?;
    }
    // An empty observation can be a measured zero only when the profile supplies rules.
    if operation_rules.is_empty() && view.pair_supported {
        for entry in retained.iter()? {
            let entry = entry?;
            if matches!(&entry.classification, Some(ClassifiedRecord::Event { semantics, .. }) if semantics.phase.is_some())
            {
                operation_rules.extend(rules(&entry));
            }
        }
    }
    if !operation_rules.is_empty() {
        let paired_count = operation_members.len();
        let population_id = population_stream(
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
            operation_members.iter()?,
            findings,
            populations,
            memberships,
        )?;
        if !view.pair_supported
            && let Some(population) = populations.last_mut()
        {
            population["completeness"] = json!("partial");
            population["exclusions"] = json!([{"reason":"Selected classification loss or incomplete boundary recognition prevents complete semantic coverage; only recognized pairs are retained.","count":null}]);
        }
        if duration_count != paired_count {
            findings.push(fact(format!("{population_id}-distribution-unavailable"),scope,"unknown","Elapsed distributions are unavailable for this paired population.",Vec::new(),json!({"reason":"Not every paired member has source timestamp provenance; individual reliable measurements remain available.","supporting_occurrences":[]})))?;
        } else if duration_count != 0 {
            let count = duration_count;
            let mut statistics = [0i64; 5];
            let sorted = match durations.sorted(|| {
                if budget.checkpoint("calculation", 8192) {
                    Ok(())
                } else {
                    Err("distribution ordering interrupted".into())
                }
            }) {
                Ok(sorted) => sorted,
                Err(_) if budget.halted => return Ok(()),
                Err(error) => return Err(error),
            };
            for (index, value) in sorted.enumerate() {
                let value = (value? ^ (1 << 63)) as i64;
                for (slot, target) in [
                    0,
                    count - 1,
                    count * 50 / 100,
                    count * 95 / 100,
                    count * 99 / 100,
                ]
                .into_iter()
                .enumerate()
                {
                    if index == target {
                        statistics[slot] = value;
                    }
                }
            }
            for (statistic, value, method) in [
                ("minimum", statistics[0], "minimum"),
                ("maximum", statistics[1], "maximum"),
                (
                    "p50",
                    statistics[2],
                    "sorted_index_floor_n_times_percentile_over_100",
                ),
                (
                    "p95",
                    statistics[3],
                    "sorted_index_floor_n_times_percentile_over_100",
                ),
                (
                    "p99",
                    statistics[4],
                    "sorted_index_floor_n_times_percentile_over_100",
                ),
            ] {
                findings.push_with_ids(fact(format!("{population_id}-{statistic}"),scope,"calculated_fact","Distribution of source-backed elapsed intervals in the declared paired population.",Vec::new(),json!({"calculation":"distribution","population_id":population_id,"measurement_ids":[],"sample_count":count,"value":value,"unit":"ms","statistic":statistic,"method":method})), &measurement_ids)?;
            }
        }
    }
    for (index, event) in perf.unmatched_events.iter()?.enumerate() {
        let event = event?;
        if !budget.checkpoint("calculation", 1) {
            return Ok(());
        }
        let Some(entry) = entries.lookup(&event.source)? else {
            continue;
        };
        if !view.contains(&entry)? {
            continue;
        }
        let missing_capability = (event.reason == "missing_end"
            && !view.boundary_supported(&entry, crate::event_rules::Phase::End)?)
            || (event.reason == "missing_start"
                && !view.boundary_supported(&entry, crate::event_rules::Phase::Start)?);
        if missing_capability {
            findings.push(fact(format!("{scope}-unmatched-{index}"),scope,"unknown","Opposite-boundary recognition is unavailable; this observation does not establish a missing lifecycle boundary.",vec![excerpt(&entry,context,snapshot)],json!({"reason":"No compatible opposite-boundary recognition is established for this selected operation.","supporting_occurrences":[occurrence(&entry,context,snapshot)]})))?;
            continue;
        }
        findings.push(fact(format!("{scope}-unmatched-{index}"),scope,"observation",match event.reason.as_str(){"missing_end"=>"A start has no observed end in this selected capture; this does not establish a hang.","missing_start"=>"An end has no observed start in this selected capture.","overlapping_starts"=>"Repeated overlapping starts prevent an unambiguous lifecycle pairing.","conflicting_event_rules"=>"Explicit rules assign conflicting event semantics.",_=>"The event could not form a reliable scoped lifecycle pair."},vec![excerpt(&entry,context,snapshot)],json!({"supporting_occurrences":[occurrence(&entry,context,snapshot)]})))?;
    }
    if view.boundaries == 0 {
        findings.push(fact(format!("{scope}-lifecycle-unavailable"),scope,"unknown","Lifecycle calculations are unavailable without selected boundary semantics.",Vec::new(),json!({"reason":"Selected identity-only rules do not define lifecycle phases.","supporting_occurrences":[]})))?;
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
        if !budget.checkpoint("calculation", entries.len() as u64) {
            return Ok(());
        }
        let matches = |entry: &LogEntry| {
            if let Some(ClassifiedRecord::Event { semantics, .. }) = &entry.classification {
                match suffix {
                    "starts" => semantics.phase == Some(crate::event_rules::Phase::Start),
                    "ends" => semantics.phase == Some(crate::event_rules::Phase::End),
                    "failures" => semantics.outcome == Some(crate::event_rules::Outcome::Failure),
                    _ => semantics.outcome == Some(crate::event_rules::Outcome::Success),
                }
            } else {
                false
            }
        };
        let mut selected_count = 0;
        let mut rule_ids = BTreeSet::new();
        for entry in retained.iter()? {
            let entry = entry?;
            selected_count += usize::from(matches(&entry));
            rule_ids.extend(rules(&entry));
        }
        let capability = match suffix {
            "starts" => view.starts_supported,
            "ends" => view.ends_supported,
            "failures" => view.failures_supported,
            _ => view.successes_supported,
        };
        if !capability && selected_count == 0 {
            findings.push(fact(format!("{scope}-{suffix}-unavailable"),scope,"unknown","This event count is unavailable without recognition capability for the selected operation population.",Vec::new(),json!({"reason":"Explicit selected operation families do not all establish the required phase or outcome recognition.","supporting_occurrences":[]})))?;
            continue;
        }
        let members = retained.iter()?.filter_map(|entry| match entry {
            Ok(entry) if matches(&entry) => { let source = occurrence(&entry,context,snapshot); Some(Ok(json!({"kind":"event","id":format!("{scope}-{suffix}-{}",source["evidence_ref"]["reference_id"].as_str().unwrap()),"identity":[{"field":"reference_id","value":source["evidence_ref"]["reference_id"]}],"source_occurrences":[source],"measurement_ids":[]}))) },
            Ok(_) => None, Err(error) => Some(Err(error)),
        });
        population_stream(
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
        )?;
        if !capability && let Some(population) = populations.last_mut() {
            population["completeness"] = json!("partial");
            population["exclusions"] = json!([{"reason":"Selected classification loss or incomplete recognition prevents complete semantic coverage; positive classified occurrences are retained, absent occurrences are unknown.","count":null}]);
        }
    }
    findings.push(fact(format!("{scope}-domain-grouping-unavailable"),scope,"unknown","Domain groupings and relationships outside explicit policy declarations remain unavailable.",Vec::new(),json!({"reason":"Operation names, repeated IDs and timestamp proximity do not establish domain grouping or causal relationships.","supporting_occurrences":[]})))?;
    Ok(())
}

/// Bounded source-severity inventory. Replays rows and retains one representative
/// per exact message in a disk-backed set, independently of domain semantics.
#[allow(clippy::too_many_arguments)]
pub(super) fn streamed_observed_levels(
    entries: &Entries,
    scope: &str,
    context: &evidence::Context,
    snapshot: &Value,
    complete: bool,
    budget: &mut crate::processing::Budget,
    findings: &mut dyn FindingSink,
    populations: &mut Vec<Value>,
    memberships: &mut dyn super::construction::MemberSink,
) -> super::Result<()> {
    for (severity, aliases) in [
        ("errors", &["ERROR", "FATAL"][..]),
        ("warnings", &["WARN", "WARNING"][..]),
    ] {
        let scratch = context
            .finding_fields_allowance()
            .saturating_add(16384 * 16);
        if !budget.checkpoint("calculation", 1) || !budget.reserve("calculation", scratch) {
            break;
        }
        let mut members = [entries.rows::<Value>()?, entries.rows::<Value>()?];
        let mut observations = entries.rows::<Value>()?;
        let mut seen =
            crate::disk_index::DiskIndex::new(entries.len() as u64, entries.state_bytes)?;
        let mut index = 0;
        for (ordinal, entry) in entries.iter()?.enumerate() {
            if !budget.checkpoint("calculation", 1) {
                break;
            }
            let entry = entry?;
            if !aliases
                .iter()
                .any(|level| entry.level.eq_ignore_ascii_case(level))
            {
                continue;
            }
            members[usize::from(entry.source_row_path.is_some())]
                .push(&json!({"kind":"record","occurrence":occurrence(&entry,context,snapshot)}))?;
            if !budget.checkpoint("calculation", entry.message.len() as u64) {
                break;
            }
            let key = evidence::digest(entry.message.as_bytes());
            let repeated = if let Some(previous) = seen.get(key.as_bytes())? {
                if entries.get(previous as usize)?.message == entry.message {
                    true
                } else {
                    // Digest collisions must not merge distinct source messages.
                    let mut repeated = false;
                    for previous in entries.iter()?.take(ordinal) {
                        let previous = previous?;
                        if previous.message == entry.message
                            && aliases
                                .iter()
                                .any(|level| previous.level.eq_ignore_ascii_case(level))
                        {
                            repeated = true;
                            break;
                        }
                    }
                    repeated
                }
            } else {
                seen.insert(key.as_bytes(), ordinal as u64)?;
                false
            };
            if !repeated {
                observations.push(&fact(format!("{scope}-observed-{severity}-{index}"), scope, "observation",
                    &format!("Observed {severity} source record; domain outcome and cause remain unproven."),
                    vec![excerpt(&entry,context,snapshot)], json!({"supporting_occurrences":[occurrence(&entry,context,snapshot)]})))?;
            }
            index += 1;
        }
        budget.memory_bytes = budget.memory_bytes.saturating_sub(scratch);
        for (index, members) in members.into_iter().enumerate() {
            let (entity, suffix) = if index == 0 {
                ("physical_records", "")
            } else {
                ("normalized_records", "-normalized")
            };
            if members.len() == 0 && (index != 0 || !complete || budget.halted) {
                continue;
            }
            population_stream(
                scope,
                &format!("observed-{severity}{suffix}"),
                entity,
                &format!(
                    "Observed {severity} severity records; source severity does not establish domain failure or cause."
                ),
                Vec::new(),
                Vec::new(),
                members.iter()?,
                findings,
                populations,
                memberships,
            )?;
            if !complete || budget.halted {
                populations.last_mut().unwrap()["completeness"] = json!("partial");
            }
        }
        for finding in observations.iter()? {
            findings.push(finding?)?;
        }
        if budget.halted {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod severity_budget_tests {
    use super::*;
    fn sources() -> Entries {
        let mut entries = Entries::new(
            super::super::construction::Disk::new(4 * 1024 * 1024),
            65536,
            1024 * 1024,
        )
        .unwrap();
        for index in 0..100 {
            entries
                .push(
                    &crate::parser::parse_log_entry(
                        &format!("2025-01-01T00:00:00Z ERROR app: failure {index}"),
                        index + 1,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        entries.seal().unwrap();
        entries
    }
    #[test]
    fn severity_retention_obeys_memory_work_time_and_cancellation_limits() {
        let entries = sources();
        let temp = tempfile::tempdir().unwrap();
        let cancel = temp.path().join("cancel");
        std::fs::write(&cancel, "cancel").unwrap();
        for reason in ["memory_limit", "work_limit", "time_limit", "cancelled"] {
            let mut budget = crate::processing::test_budget();
            match reason {
                "memory_limit" => budget.limits.memory_bytes = 16384,
                "work_limit" => budget.limits.work_units = 60,
                "time_limit" => budget.limits.elapsed_ms = 0,
                _ => budget.limits.cancel_file = Some(cancel.clone()),
            }
            let (mut findings, mut populations, mut memberships) =
                (Vec::new(), Vec::new(), Vec::new());
            streamed_observed_levels(
                &entries,
                "scope-0",
                &evidence::Context::default(),
                &Value::Null,
                true,
                &mut budget,
                &mut findings,
                &mut populations,
                &mut memberships,
            )
            .unwrap();
            assert_eq!(budget.stop.as_ref().unwrap()["reason"], reason);
            assert!(budget.memory_bytes <= budget.limits.memory_bytes);
            assert!(findings.len() < 100);
            assert!(
                populations
                    .iter()
                    .all(|population| population["completeness"] == "partial")
            );
            if reason == "work_limit" {
                assert!(
                    !populations.is_empty(),
                    "processed prefix must remain available"
                );
                assert_eq!(
                    populations[0]["count"],
                    memberships[0]["members"].as_array().unwrap().len()
                );
            } else {
                assert!(findings.is_empty());
                assert!(populations.is_empty());
            }
        }
    }
    #[test]
    fn distinct_selected_messages_do_not_accumulate_memory_reservations() {
        let entries = sources();
        let mut budget = crate::processing::test_budget();
        budget.limits.memory_bytes = 1024 * 1024;
        let (mut findings, mut populations, mut memberships) = (Vec::new(), Vec::new(), Vec::new());
        streamed_observed_levels(
            &entries,
            "scope-0",
            &evidence::Context::default(),
            &Value::Null,
            true,
            &mut budget,
            &mut findings,
            &mut populations,
            &mut memberships,
        )
        .unwrap();
        assert!(!budget.halted);
        assert_eq!(budget.memory_bytes, 0);
        assert_eq!(populations[0]["count"], 100);
        assert_eq!(memberships[0]["members"].as_array().unwrap().len(), 100);
        assert_eq!(
            findings
                .iter()
                .filter(|finding| finding["kind"] == "observation")
                .count(),
            100
        );
        assert!(
            populations
                .iter()
                .all(|population| population["completeness"] == "complete")
        );
    }
}
