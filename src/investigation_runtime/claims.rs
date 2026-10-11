//! Checks explicit snapshot assertions, never free-text conclusions or causes.
use super::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assertions {
    version: u32,
    required_claim_ids: Vec<String>,
    claims: Vec<Claim>,
}
#[derive(Deserialize)]
struct Claim {
    id: String,
    #[serde(flatten)]
    assertion: Assertion,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Assertion {
    Fact { pointer: String, equals: Value },
    Interval { pointer: String, expected_ms: f64 },
    Link { finding_id: String, step_id: String },
    DistinctRecords { pointer: String, expected: usize },
    Absence { finding_id: String, step_id: String },
}
fn check(retained: &Value, assertion: &Assertion) -> (Option<bool>, Value, &'static str) {
    match assertion {
        Assertion::Fact { pointer, equals } => {
            let observed = retained.pointer(pointer);
            (
                observed.map(|v| v == equals),
                observed.cloned().unwrap_or(Value::Null),
                "Exact retained value; no interpretation of outcome semantics or cause",
            )
        }
        Assertion::Interval {
            pointer,
            expected_ms,
        } => {
            let interval = retained.pointer(pointer);
            let measured = interval.filter(|v| v["status"] == "measured" && v["unit"] == "ms");
            let actual = measured.and_then(|v| {
                let start =
                    chrono::DateTime::parse_from_rfc3339(v["start_timestamp"].as_str()?).ok()?;
                let end =
                    chrono::DateTime::parse_from_rfc3339(v["end_timestamp"].as_str()?).ok()?;
                let nanos = end.signed_duration_since(start).num_nanoseconds()?;
                (nanos >= 0).then_some(nanos as f64 / 1_000_000.0)
            });
            (
                actual.map(|v| (v - expected_ms).abs() < 0.000001),
                interval.cloned().unwrap_or(Value::Null),
                "Recalculated from the retained named endpoints, including offsets; unavailable intervals cannot become durations",
            )
        }
        Assertion::Link {
            finding_id,
            step_id,
        }
        | Assertion::Absence {
            finding_id,
            step_id,
        } => {
            let chain = retained["findings"]
                .as_array()
                .and_then(|items| items.iter().find(|f| f["id"] == *finding_id))
                .map(|f| &f["details"]["event_chain"]);
            let step = chain
                .and_then(|c| c["steps"].as_array())
                .and_then(|items| items.iter().find(|s| s["id"] == *step_id));
            let result = chain.zip(step).and_then(|(c, s)| {
                if c["ambiguity"] != "none"
                    || s["status"] == "ambiguous"
                    || s["status"] == "unavailable"
                    || s["count_completeness"] != "complete"
                {
                    return None;
                }
                s["linked_count"].as_u64().map(|n| n > 0)
            });
            if matches!(assertion, Assertion::Absence { .. }) {
                return (
                    if result == Some(true) {
                        Some(false)
                    } else {
                        None
                    },
                    step.cloned().unwrap_or(Value::Null),
                    "Observed links contradict absence; missing configured links cannot prove it. Inspect later records, omitted samples and recognition gaps",
                );
            }
            (
                result,
                step.cloned().unwrap_or(Value::Null),
                "Observed scoped relationship under the retained profile; association does not establish cause",
            )
        }
        Assertion::DistinctRecords { pointer, expected } => {
            let count = retained
                .pointer(pointer)
                .and_then(Value::as_array)
                .and_then(|items| {
                    let mut identities = BTreeSet::new();
                    for item in items {
                        let occurrence = item.get("occurrence")?;
                        occurrence.get("evidence_ref")?;
                        identities.insert(occurrence.to_string());
                    }
                    Some(identities.len())
                });
            (
                count.map(|n| n == *expected),
                json!(count),
                "Distinct source occurrences in this retained array, not an exhaustive count of omitted records or attempts",
            )
        }
    }
}
pub(super) fn verify(retained: &Value, path: &Path) -> Result<Value> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > 1_048_576 {
        return Err("Claims file exceeds 1 MiB".into());
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    file.take(1_048_577).read_to_end(&mut bytes)?;
    if bytes.len() > 1_048_576 {
        return Err("Claims file exceeds 1 MiB".into());
    }
    let assertions: Assertions = serde_json::from_slice(&bytes)?;
    if assertions.version != 1
        || assertions.required_claim_ids.is_empty()
        || assertions.required_claim_ids.len() > 100
        || assertions.claims.is_empty()
        || assertions.claims.len() > 100
    {
        return Err("Claims require version 1, 1..100 required IDs and 1..100 assertions".into());
    }
    let required: BTreeSet<_> = assertions.required_claim_ids.iter().collect();
    if required.len() != assertions.required_claim_ids.len()
        || required.iter().any(|id| id.is_empty())
    {
        return Err("Required claim IDs must be nonempty and unique".into());
    }
    let mut ids = BTreeSet::new();
    let mut checks = Vec::new();
    for claim in &assertions.claims {
        if claim.id.is_empty() || !ids.insert(claim.id.as_str()) {
            return Err("Claim IDs must be nonempty and unique".into());
        }
        match &claim.assertion {
            Assertion::Fact { pointer, .. } | Assertion::DistinctRecords { pointer, .. } | Assertion::Interval { pointer, .. } if !pointer.starts_with('/') => return Err("Claim pointers must address the retained artifact with a JSON Pointer starting with /".into()),
            Assertion::Interval { expected_ms, .. } if !expected_ms.is_finite() || *expected_ms < 0.0 => return Err("Expected interval must be finite and nonnegative".into()),
            _ => {}
        }
        let (result, observed, reason) = check(retained, &claim.assertion);
        checks.push(json!({"id":claim.id,"status":match result {Some(true)=>"verified",Some(false)=>"contradicted",None=>"unavailable"},"observed":observed,"reason":reason}));
    }
    let missing: Vec<_> = assertions
        .required_claim_ids
        .iter()
        .filter(|id| !ids.contains(id.as_str()))
        .collect();
    Ok(
        json!({"version":1,"status":if missing.is_empty() && checks.iter().all(|c|c["status"]=="verified"){"verified"}else{"needs_revision"},"missing_required_claim_ids":missing,"checks":checks,"limitations":["Only declared assertions are checked against the retained snapshot. This does not validate free-text claims, profile meaning, upstream completeness or causality. Required IDs must be derived from the question before drafting the answer."]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intervals_recalculate_offsets_and_reject_unavailable() {
        let retained = json!({"i":{"status":"measured","unit":"ms","start_timestamp":"2026-01-01T02:00:00.010+02:00","end_timestamp":"2026-01-01T00:00:00.025Z"}});
        assert_eq!(
            check(
                &retained,
                &Assertion::Interval {
                    pointer: "/i".into(),
                    expected_ms: 15.0
                }
            )
            .0,
            Some(true)
        );
        assert_eq!(
            check(
                &retained,
                &Assertion::Interval {
                    pointer: "/i".into(),
                    expected_ms: 14.0
                }
            )
            .0,
            Some(false)
        );
        assert_eq!(
            check(
                &json!({"i":{"status":"unavailable"}}),
                &Assertion::Interval {
                    pointer: "/i".into(),
                    expected_ms: 15.0
                }
            )
            .0,
            None
        );
    }
    #[test]
    fn duplicated_associations_are_not_distinct_records() {
        let record = json!({"occurrence":{"evidence_ref":{"line":1},"input_ordinal":0}});
        assert_eq!(
            check(
                &json!({"records":[record.clone(),record]}),
                &Assertion::DistinctRecords {
                    pointer: "/records".into(),
                    expected: 1
                }
            )
            .0,
            Some(true)
        );
    }
    #[test]
    fn ambiguous_links_and_absence_remain_unavailable() {
        let retained = json!({"findings":[{"id":"f","details":{"event_chain":{"ambiguity":"reused_root_identity","steps":[{"id":"end","status":"observed","linked_count":1,"count_completeness":"complete"}]}}}]});
        assert_eq!(
            check(
                &retained,
                &Assertion::Link {
                    finding_id: "f".into(),
                    step_id: "end".into()
                }
            )
            .0,
            None
        );
        assert_eq!(
            check(
                &retained,
                &Assertion::Absence {
                    finding_id: "f".into(),
                    step_id: "end".into()
                }
            )
            .0,
            None
        );
    }
}
