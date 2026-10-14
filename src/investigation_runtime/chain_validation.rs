//! Replay retained source metadata and excerpts, then check configured chain populations.
use super::{Selector, chains, findings as source_findings, selected as is_selected};
use crate::{
    config::AnalyzerConfig,
    evidence, parser,
    processing::{Budget, Limits},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

type Address = (u64, Option<String>);
fn address(source: &Value) -> Option<Address> {
    Some((
        source["evidence_ref"]["line"].as_u64()?,
        source["evidence_ref"]["row_path"]
            .as_str()
            .map(str::to_owned),
    ))
}

fn samples(
    value: &Value,
    source_records: Option<&BTreeMap<Address, &Value>>,
) -> Result<Vec<Value>, String> {
    value
        .as_array()
        .ok_or("missing chain samples")?
        .iter()
        .map(|sample| {
            let location = address(&sample["occurrence"]).ok_or("missing sample address")?;
            let mut payload = sample.clone();
            if let Some(record) = source_records.and_then(|records| records.get(&location))
                && record["verification"]["source_and_rules"] == "unavailable"
                && payload["verification"].is_object()
            {
                // Stored sources may explicitly withhold verification after capture/redaction.
                for field in ["source_and_rules", "losses"] {
                    payload["verification"][field] = record["verification"][field].clone();
                }
            }
            // Snapshot and input identity are assigned by the per-input replay context.
            payload["occurrence"] = serde_json::to_value(location).map_err(|e| e.to_string())?;
            Ok(payload)
        })
        .collect()
}

pub(crate) fn validate(
    artifact: &Value,
    captures: &BTreeMap<u64, Vec<u8>>,
) -> Result<Vec<&'static str>, String> {
    let findings = artifact["findings"].as_array().ok_or("missing findings")?;
    let chain_findings: Vec<_> = findings
        .iter()
        .filter(|f| f["details"]["event_chain"].is_object())
        .collect();
    if artifact["effective_profile_omitted"] == true {
        return Ok(if chain_findings.is_empty() {
            Vec::new()
        } else {
            vec!["chain_cardinalities_require_retained_profile_and_capture"]
        });
    }
    let config: AnalyzerConfig =
        serde_json::from_value(artifact["effective_profile"].clone()).map_err(|e| e.to_string())?;
    config.validate_event_rules()?;
    let policy = config.investigation.as_ref();
    let has_chains =
        policy.is_some_and(|policy| !policy.chains.is_empty()) || !chain_findings.is_empty();
    if policy.is_none() && !chain_findings.is_empty() {
        return Err("chain findings lack effective policy".into());
    }
    let query = &artifact["report_metadata"]["evidence"]["query"];
    let filter = crate::build_filter(&query["filter"].as_str().map(str::to_owned))
        .map_err(|e| e.to_string())?;
    let selectors: Vec<Selector> = if let Some(command) = query["command"].get("Investigate") {
        command["select"]
            .as_array()
            .ok_or("missing retained selectors")?
            .iter()
            .map(|selector| {
                serde_json::from_str(selector.as_str().ok_or("invalid retained selector")?)
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<_, _>>()?
    } else {
        Vec::new()
    };
    let mut ordinals: BTreeSet<_> = chain_findings
        .iter()
        .map(|f| {
            f["details"]["event_chain"]["root"]["input_ordinal"]
                .as_u64()
                .ok_or("missing chain input")
        })
        .collect::<Result<_, _>>()?;
    for record in artifact["records"].as_array().ok_or("missing records")? {
        ordinals.insert(
            record["occurrence"]["input_ordinal"]
                .as_u64()
                .ok_or("missing record input")?,
        );
    }
    ordinals.extend(
        0..artifact["report_metadata"]["evidence"]["inputs"]
            .as_array()
            .ok_or("missing inputs")?
            .len() as u64,
    );
    let mut complete = true;
    // Cutoff metadata does not record whether chain calculation had already completed.
    // Artifact writing is the exception: it runs only after chain analysis.
    let mut completeness_deferred = false;
    for ordinal in ordinals {
        let scope = artifact["scopes"]
            .as_array()
            .ok_or("missing scopes")?
            .iter()
            .find(|scope| {
                scope["input_ordinals"]
                    .as_array()
                    .is_some_and(|inputs| inputs.iter().any(|input| *input == ordinal))
            })
            .ok_or("missing chain input scope")?;
        let stop = &artifact["processing"]["stop"];
        let capture_completed = artifact["captured_inputs"]
            .get(ordinal as usize)
            .is_some_and(|input| input["capture"] == "complete")
            && artifact["processing"]["inputs"]
                .get(ordinal as usize)
                .is_some_and(|input| input["capture"] == "complete");
        let listed = stop["scope_ids"]
            .as_array()
            .is_some_and(|scopes| scopes.contains(&scope["id"]));
        // A capture stop cannot explain missing chain results when retained
        // capture and progress both say this input was read completely.
        let chain_cutoff = listed
            && match stop["stage"].as_str() {
                Some("capture") => !capture_completed,
                Some(
                    "profile_discovery" | "profile_detection" | "parse" | "classification"
                    | "correlation" | "calculation" | "relationships",
                ) => true,
                _ => false,
            };
        let completeness_available = !chain_cutoff;
        completeness_deferred |= has_chains && chain_cutoff;
        let Some(bytes) = captures.get(&ordinal) else {
            complete = false;
            continue;
        };
        let selected: BTreeMap<_, _> = artifact["records"]
            .as_array()
            .ok_or("missing records")?
            .iter()
            .filter(|r| r["occurrence"]["input_ordinal"] == ordinal)
            .map(|r| {
                address(&r["occurrence"])
                    .map(|address| (address, r))
                    .ok_or("missing record address")
            })
            .collect::<Result<_, _>>()?;
        // Validation covers the already-retained population, independently of generation cutoffs.
        let allowance = |name: &str, minimum: u64| {
            minimum
                .max(artifact["processing"]["limits"][name].as_u64().unwrap_or(0))
                .max(artifact["processing"]["usage"][name].as_u64().unwrap_or(0))
        };
        let mut budget = Budget::new(Limits {
            input_bytes: allowance("input_bytes", bytes.len() as u64),
            records: allowance("records", bytes.len() as u64),
            expanded_records: allowance("expanded_records", 100_000.max(selected.len() as u64)),
            work_units: allowance("work_units", 500_000_000),
            elapsed_ms: allowance("elapsed_ms", 180_000),
            memory_bytes: allowance(
                "memory_bytes",
                (bytes.len() as u64)
                    .saturating_mul(512)
                    .clamp(512 * 1024 * 1024, 32 * 1024 * 1024 * 1024),
            ),
            artifact_bytes: u64::MAX,
            record_bytes: usize::MAX,
            cancel_file: None,
        });
        let input = &artifact["report_metadata"]["evidence"]["inputs"][ordinal as usize];
        let eof = artifact["captured_inputs"][ordinal as usize]["capture"] == "complete";
        let parsed = parser::parse_capture(
            Path::new(input["file"].as_str().ok_or("missing input path")?),
            bytes,
            eof,
            &config,
            &mut budget,
        );
        let mut parsed = match parsed {
            Ok(parsed) => parsed,
            Err(_)
                if selected.is_empty()
                    && input["coverage"]["parsed_entries"] == 0
                    && !chain_findings.iter().any(|f| {
                        f["details"]["event_chain"]["root"]["input_ordinal"] == ordinal
                    }) =>
            {
                complete = false;
                continue;
            }
            Err(error) => return Err(format!("{error:?}")),
        };
        let input_complete = parsed.coverage.rejected_candidates == 0
            && parsed.coverage.normalization_diagnostics.is_empty()
            && !parsed.coverage.is_unparsed();
        for entry in &mut parsed.entries {
            entry.source_input_ordinal = Some(ordinal as usize);
        }
        parsed
            .entries
            .retain(|entry| filter.matches(entry) && is_selected(entry, &selectors, &config));
        if completeness_available {
            let source_population: BTreeSet<_> = parsed
                .entries
                .iter()
                .map(|entry| {
                    (
                        entry.source_line_number as u64,
                        entry.source_row_path.clone(),
                    )
                })
                .collect();
            let retained_population = selected
                .iter()
                .filter(|(location, record)| {
                    source_population.contains(*location)
                        || !(record["data_omitted"] == true
                            && record["verification"]["source_and_rules"] == "unavailable")
                })
                .map(|(location, _)| location.clone())
                .collect();
            if source_population != retained_population {
                return Err("retained records differ from selected captured population".into());
            }
        }
        parsed.entries.retain(|entry| {
            selected.contains_key(&(
                entry.source_line_number as u64,
                entry.source_row_path.clone(),
            ))
        });
        let parsed_addresses: BTreeSet<_> = parsed
            .entries
            .iter()
            .map(|entry| {
                (
                    entry.source_line_number as u64,
                    entry.source_row_path.clone(),
                )
            })
            .collect();
        if selected.iter().any(|(location, record)| {
            !parsed_addresses.contains(location)
                && !(record["data_omitted"] == true
                    && record["verification"]["source_and_rules"] == "unavailable")
        }) {
            return Err("retained chain population differs from captured records".into());
        }
        for entry in &mut parsed.entries {
            entry.source_input_ordinal = Some(0);
            let record = selected[&(
                entry.source_line_number as u64,
                entry.source_row_path.clone(),
            )];
            if record["data_omitted"] == true
                && record["verification"]["source_and_rules"] == "unavailable"
            {
                continue;
            }
            for (field, expected) in [
                (
                    "timestamp",
                    serde_json::json!(entry.source_timestamp.map(|time| time.to_rfc3339())),
                ),
                (
                    "timestamp_year_source",
                    serde_json::json!(if entry.timestamp_year_inferred {
                        "inferred"
                    } else {
                        "source"
                    }),
                ),
                (
                    "timestamp_offset_source",
                    serde_json::json!(if entry.source_timestamp.is_some() {
                        "source"
                    } else {
                        "assumed"
                    }),
                ),
            ] {
                if record[field] != expected {
                    return Err(format!("retained {field} differs from captured source"));
                }
            }
            if serde_json::to_value(&entry.classification).map_err(|e| e.to_string())?
                != record["fields"]["classification"]
            {
                return Err(
                    "retained classification differs from captured source and profile".into(),
                );
            }
            let replay =
                source_findings::record(entry, &evidence::Context::default(), &Value::Null);
            for field in ["entity", "raw_text", "message"] {
                if record[field] != replay[field] {
                    return Err(format!("retained {field} differs from parsed record"));
                }
            }
            let fields = record["fields"]
                .as_object()
                .ok_or("missing retained fields")?;
            if ["level", "structured_fields", "envelope_payload", "payload"]
                .iter()
                .any(|key| fields.contains_key(*key))
            {
                if record["fields"] != replay["fields"] {
                    return Err("retained fields differs from parsed record".into());
                }
            } else {
                // Earlier contract fixtures retain a projection of source fields.
                for (field, value) in fields {
                    let expected = if field == "classification" {
                        replay["fields"][field].clone()
                    } else {
                        crate::relationship_rules::field(entry, field).unwrap_or(Value::Null)
                    };
                    if *value != expected {
                        return Err("retained fields differs from parsed record".into());
                    }
                }
            }
        }
        let mut context = evidence::Context::for_investigation_replay(&config);
        context.observe(&parsed.coverage, &parsed.entries);
        let source_entries: BTreeMap<_, _> = parsed
            .entries
            .iter()
            .map(|entry| {
                (
                    (
                        entry.source_line_number as u64,
                        entry.source_row_path.clone(),
                    ),
                    entry,
                )
            })
            .collect();
        for finding in findings
            .iter()
            .filter(|f| !f["details"]["event_chain"].is_object())
        {
            for excerpt in finding["evidence"]
                .as_array()
                .ok_or("missing finding evidence")?
            {
                if excerpt["occurrence"]["input_ordinal"] != ordinal {
                    continue;
                }
                let location = address(&excerpt["occurrence"]).ok_or("missing excerpt address")?;
                if selected.get(&location).is_some_and(|record| {
                    record["data_omitted"] == true
                        && record["verification"]["source_and_rules"] == "unavailable"
                }) && excerpt["verification"]["source_and_rules"] == "unavailable"
                {
                    continue;
                }
                let entry = source_entries
                    .get(&location)
                    .ok_or("excerpt absent from parsed population")?;
                let mut replay = source_findings::excerpt(entry, &context, &Value::Null);
                let text = excerpt["text"].as_str().ok_or("missing excerpt text")?;
                if ![entry.message.as_str(), entry.raw_logline.as_str()]
                    .iter()
                    .any(|original| {
                        original.starts_with(text)
                            && excerpt["omitted_characters"].as_u64()
                                == Some((original.chars().count() - text.chars().count()) as u64)
                    })
                {
                    return Err("excerpt differs from retained record or replayed source".into());
                }
                // Legacy artifacts may retain different valid excerpt bounds.
                replay["text"] = excerpt["text"].clone();
                replay["omitted_characters"] = excerpt["omitted_characters"].clone();
                if samples(&serde_json::json!([excerpt]), None)?
                    != samples(&serde_json::json!([replay]), Some(&selected))?
                {
                    return Err("finding evidence differs from replayed source".into());
                }
            }
        }
        let Some(policy) = policy.filter(|policy| !policy.chains.is_empty()) else {
            continue;
        };
        let mut replayed = Vec::new();
        // Parsing may already have stopped at its own limit. Chain replay is a later
        // stage, matching publication, and only its own halt is a validation failure.
        budget.begin_stage();
        let parse_stop = budget.stop.clone();
        chains::calculate(
            policy,
            &config,
            &parsed.entries,
            &[],
            "validation",
            &context,
            &Value::Null,
            input_complete,
            &mut budget,
            &mut replayed,
            None,
        );
        if budget.halted || budget.stop != parse_stop {
            return Err("chain cardinality validation did not finish".into());
        }
        let expected: BTreeMap<_, _> = replayed
            .iter()
            .filter_map(|f| {
                let chain = &f["details"]["event_chain"];
                Some(((chain["id"].as_str()?, address(&chain["root"])?), f))
            })
            .collect();
        if completeness_available {
            let retained_roots: BTreeSet<_> = chain_findings
                .iter()
                .filter(|f| f["details"]["event_chain"]["root"]["input_ordinal"] == ordinal)
                .map(|finding| {
                    let chain = &finding["details"]["event_chain"];
                    Ok((
                        chain["id"].as_str().ok_or("missing chain ID")?,
                        address(&chain["root"]).ok_or("missing chain root")?,
                    ))
                })
                .collect::<Result<_, String>>()?;
            if retained_roots != expected.keys().cloned().collect() {
                return Err("retained chain findings differ from replayed population".into());
            }
            let gap = |finding: &Value| -> Result<Option<Value>, String> {
                let Some(class) = finding["details"]["gap_classification"].as_str() else {
                    return Ok(None);
                };
                if !matches!(
                    class,
                    "recognition_unresolved" | "identity_or_scope_unavailable"
                ) {
                    return Ok(None);
                }
                let mut occurrences = finding["details"]["supporting_occurrences"]
                    .as_array()
                    .ok_or("missing gap occurrences")?
                    .iter()
                    .map(|occurrence| address(occurrence).ok_or("missing gap occurrence"))
                    .collect::<Result<Vec<_>, _>>()?;
                occurrences.sort();
                Ok(Some(serde_json::json!([
                    class,
                    finding["claim"],
                    finding["details"]["reason"],
                    occurrences
                ])))
            };
            let mut retained_gaps = findings
                .iter()
                .filter(|finding| finding["scope_id"] == scope["id"])
                .map(&gap)
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            let mut replayed_gaps = replayed
                .iter()
                .map(gap)
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            retained_gaps.sort_by_key(|left| left.to_string());
            replayed_gaps.sort_by_key(|left| left.to_string());
            if retained_gaps != replayed_gaps {
                return Err("retained chain gap findings differ from replayed population".into());
            }
        }
        for finding in chain_findings
            .iter()
            .filter(|f| f["details"]["event_chain"]["root"]["input_ordinal"] == ordinal)
        {
            let chain = &finding["details"]["event_chain"];
            let key = (
                chain["id"].as_str().ok_or("missing chain ID")?,
                address(&chain["root"]).ok_or("missing chain root")?,
            );
            let replay_finding = expected
                .get(&key)
                .ok_or("chain root absent from retained population")?;
            if finding["claim"] != replay_finding["claim"] {
                return Err("chain claim differs from retained population".into());
            }
            if finding["limitations"] != replay_finding["limitations"] {
                return Err("chain limitations differ from retained population".into());
            }
            if samples(&finding["evidence"], None)?
                != samples(&replay_finding["evidence"], Some(&selected))?
            {
                return Err("chain evidence differs from retained population".into());
            }
            let replay = &replay_finding["details"]["event_chain"];
            if chain["root_occurrences"] != replay["root_occurrences"] {
                return Err("chain root cardinality differs from retained population".into());
            }
            for field in [
                "title",
                "evidence_policy",
                "identity",
                "context_identity",
                "scope_fields",
                "root_event_name",
                "ambiguity",
                "later_activity_observed_count",
                "omitted_later_activity",
            ] {
                if chain[field] != replay[field] {
                    return Err(format!("chain {field} differs from retained population"));
                }
            }
            if samples(&chain["later_activity"], None)?
                != samples(&replay["later_activity"], Some(&selected))?
            {
                return Err("chain later_activity differs from retained population".into());
            }
            if completeness_available && chain["completeness"] != replay["completeness"] {
                return Err("chain completeness differs from retained population".into());
            }
            let steps = chain["steps"].as_array().ok_or("missing steps")?;
            let replay_steps = replay["steps"].as_array().ok_or("missing replay steps")?;
            if steps.iter().map(|s| &s["id"]).collect::<Vec<_>>()
                != replay_steps.iter().map(|s| &s["id"]).collect::<Vec<_>>()
            {
                return Err("chain steps differ from effective policy".into());
            }
            for step in steps {
                let expected_step = replay["steps"]
                    .as_array()
                    .ok_or("missing replay steps")?
                    .iter()
                    .find(|s| s["id"] == step["id"])
                    .ok_or("chain step absent from effective policy")?;
                for field in [
                    "label",
                    "status",
                    "observed_count",
                    "linked_count",
                    "ambiguity",
                    "omitted_occurrences",
                    "omitted_outcomes",
                    "recognition",
                    "from",
                    "join_fields",
                    "cardinality",
                    "event_type",
                ] {
                    if step[field] != expected_step[field] {
                        return Err(format!(
                            "chain step {field} differs from retained population"
                        ));
                    }
                }
                if completeness_available
                    && step["count_completeness"] != expected_step["count_completeness"]
                {
                    return Err("chain step completeness differs from retained population".into());
                }
                for field in ["evidence", "ambiguous_evidence", "outcomes"] {
                    if samples(&step[field], None)?
                        != samples(&expected_step[field], Some(&selected))?
                    {
                        return Err(format!(
                            "chain step {field} differs from retained population"
                        ));
                    }
                }
                for join in step["join_samples"].as_array().ok_or("missing joins")? {
                    let expected_join = expected_step["join_samples"]
                        .as_array()
                        .ok_or("missing replay joins")?
                        .iter()
                        .find(|j| {
                            address(&j["source"]) == address(&join["source"])
                                && address(&j["target"]) == address(&join["target"])
                        })
                        .ok_or("chain join absent from retained population")?;
                    if join["identity"] != expected_join["identity"] {
                        return Err("chain join identity differs from retained population".into());
                    }
                    if join["source_count"] != expected_join["source_count"]
                        || join["target_count"] != expected_join["target_count"]
                    {
                        return Err(
                            "chain endpoint cardinality differs from retained population".into(),
                        );
                    }
                    let left = &join["endpoint_interval"];
                    let right = &expected_join["endpoint_interval"];
                    if (!left.is_object() || !right.is_object()) && left != right {
                        return Err("chain join interval differs from retained population".into());
                    }
                }
                let joins = |step: &Value| -> Result<Vec<_>, String> {
                    let mut pairs = step["join_samples"]
                        .as_array()
                        .ok_or("missing joins")?
                        .iter()
                        .map(|join| {
                            Ok((
                                address(&join["source"]).ok_or("missing join source")?,
                                address(&join["target"]).ok_or("missing join target")?,
                            ))
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    pairs.sort();
                    Ok(pairs)
                };
                if joins(step)? != joins(expected_step)? {
                    return Err("chain join samples differ from retained population".into());
                }
            }
        }
    }
    let mut deferred = Vec::new();
    if has_chains && !complete {
        deferred.push("chain_cardinalities_require_retained_profile_and_capture");
    }
    if completeness_deferred {
        deferred.push("chain_completeness_requires_processing_stage_history");
    }
    Ok(deferred)
}
