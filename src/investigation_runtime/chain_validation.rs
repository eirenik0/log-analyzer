//! Reuse the chain engine to check retained endpoint populations before sampling.
use super::chains;
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

pub(crate) fn validate(
    artifact: &Value,
    captures: &BTreeMap<u64, Vec<u8>>,
) -> Result<bool, String> {
    let findings = artifact["findings"].as_array().ok_or("missing findings")?;
    let chain_findings: Vec<_> = findings
        .iter()
        .filter(|f| f["details"]["event_chain"].is_object())
        .collect();
    if chain_findings.is_empty() {
        return Ok(true);
    }
    if artifact["effective_profile_omitted"] == true {
        return Ok(false);
    }
    let config: AnalyzerConfig =
        serde_json::from_value(artifact["effective_profile"].clone()).map_err(|e| e.to_string())?;
    config.validate_event_rules()?;
    let policy = config
        .investigation
        .as_ref()
        .ok_or("chain findings lack effective policy")?;
    let ordinals: BTreeSet<_> = chain_findings
        .iter()
        .map(|f| {
            f["details"]["event_chain"]["root"]["input_ordinal"]
                .as_u64()
                .ok_or("missing chain input")
        })
        .collect::<Result<_, _>>()?;
    let mut complete = true;
    for ordinal in ordinals {
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
        let mut parsed = parser::parse_capture(
            Path::new(input["file"].as_str().ok_or("missing input path")?),
            bytes,
            eof,
            &config,
            &mut budget,
        )
        .map_err(|e| format!("{e:?}"))?;
        parsed.entries.retain(|entry| {
            selected.contains_key(&(
                entry.source_line_number as u64,
                entry.source_row_path.clone(),
            ))
        });
        if parsed.entries.len() != selected.len() {
            return Err("retained chain population differs from captured records".into());
        }
        for entry in &mut parsed.entries {
            entry.source_input_ordinal = Some(0);
            let record = selected[&(
                entry.source_line_number as u64,
                entry.source_row_path.clone(),
            )];
            entry.classification =
                serde_json::from_value(record["fields"]["classification"].clone())
                    .map_err(|e| e.to_string())?;
        }
        let mut context = evidence::Context::default();
        context.observe(&parsed.coverage, &parsed.entries);
        let mut replayed = Vec::new();
        chains::calculate(
            policy,
            &config,
            &parsed.entries,
            &[],
            "validation",
            &context,
            &Value::Null,
            true,
            &mut budget,
            &mut replayed,
            None,
        );
        if budget.stop.is_some() {
            return Err("chain cardinality validation did not finish".into());
        }
        let expected: BTreeMap<_, _> = replayed
            .iter()
            .filter_map(|f| {
                let chain = &f["details"]["event_chain"];
                Some(((chain["id"].as_str()?, address(&chain["root"])?), chain))
            })
            .collect();
        for finding in chain_findings
            .iter()
            .filter(|f| f["details"]["event_chain"]["root"]["input_ordinal"] == ordinal)
        {
            let chain = &finding["details"]["event_chain"];
            let key = (
                chain["id"].as_str().ok_or("missing chain ID")?,
                address(&chain["root"]).ok_or("missing chain root")?,
            );
            let replay = expected
                .get(&key)
                .ok_or("chain root absent from retained population")?;
            if chain["root_occurrences"] != replay["root_occurrences"] {
                return Err("chain root cardinality differs from retained population".into());
            }
            for step in chain["steps"].as_array().ok_or("missing steps")? {
                let expected_step = replay["steps"]
                    .as_array()
                    .ok_or("missing replay steps")?
                    .iter()
                    .find(|s| s["id"] == step["id"])
                    .ok_or("chain step absent from effective policy")?;
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
                    if join["source_count"] != expected_join["source_count"]
                        || join["target_count"] != expected_join["target_count"]
                    {
                        return Err(
                            "chain endpoint cardinality differs from retained population".into(),
                        );
                    }
                }
            }
        }
    }
    Ok(complete)
}
