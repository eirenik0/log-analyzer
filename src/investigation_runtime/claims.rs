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
    #[serde(default)]
    requirements: Vec<Requirement>,
    #[serde(default)]
    unresolved: Vec<Unresolved>,
    #[serde(default)]
    questions: Vec<String>,
    #[serde(default)]
    final_review: Option<FinalReview>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Requirement {
    id: String,
    kind: String,
    #[serde(default)]
    finding_id: Option<String>,
    #[serde(default)]
    pointer: Option<String>,
    #[serde(default)]
    source: Option<Value>,
    #[serde(default)]
    target: Option<Value>,
    #[serde(default)]
    start_operation: Option<String>,
    #[serde(default)]
    end_operation: Option<String>,
    #[serde(default)]
    population: Option<CountPopulation>,
    #[serde(default)]
    step_id: Option<String>,
    #[serde(default)]
    field: Option<FactField>,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Unresolved {
    id: String,
    reason: String,
}
#[derive(Deserialize)]
struct Claim {
    id: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(flatten)]
    assertion: Assertion,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Assertion {
    Fact {
        pointer: String,
        equals: Value,
    },
    Interval {
        pointer: String,
        expected_ms: f64,
        #[serde(default)]
        subject: Option<Subject>,
    },
    Link {
        finding_id: String,
        step_id: String,
        #[serde(default)]
        subject: Option<Subject>,
    },
    DistinctRecords {
        pointer: String,
        expected: usize,
        #[serde(default)]
        operation: Option<CountOperation>,
    },
    Absence {
        finding_id: String,
        step_id: String,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FinalReview {
    summary_claim_ids: Vec<String>,
    finding_claim_ids: Vec<String>,
    checks: BTreeSet<String>,
    #[serde(default)]
    unresolved_question_ids: Vec<String>,
}

fn review_final(
    review: &FinalReview,
    checks: &[Value],
    required: &[String],
    unresolved: &[Unresolved],
) -> Value {
    let mut issues = Vec::new();
    for id in review
        .summary_claim_ids
        .iter()
        .chain(&review.finding_claim_ids)
    {
        match checks.iter().find(|c| c["id"] == *id) {
            None => issues.push(format!("undeclared final claim: {id}")),
            Some(c) if c["status"] != "verified" => {
                issues.push(format!("unresolved or contradicted final claim: {id}"))
            }
            Some(c) if c["assurance"] == "link_exists" || c["assurance"] == "retained_interval" => {
                issues.push(format!("final event claim lacks subject binding: {id}"))
            }
            _ => {}
        }
    }
    for id in &review.summary_claim_ids {
        if !review.finding_claim_ids.contains(id) {
            issues.push(format!("summary claim has no detailed finding: {id}"));
        }
    }
    for id in required {
        if !review.finding_claim_ids.contains(id) && !unresolved.iter().any(|u| &u.id == id) {
            issues.push(format!(
                "required claim absent from detailed findings: {id}"
            ));
        }
    }
    let declared: BTreeSet<_> = review.unresolved_question_ids.iter().collect();
    let expected: BTreeSet<_> = unresolved.iter().map(|u| &u.id).collect();
    if declared != expected || declared.len() != review.unresolved_question_ids.len() {
        issues.push("Final unresolved question IDs must match the explicit unresolved list".into());
    }
    for dimension in [
        "event_subjects",
        "duration_endpoints_and_units",
        "operation_types_and_count_semantics",
        "terminal_later_cached_contrary",
        "summary_consistency",
        "causal_support",
    ] {
        if !review.checks.contains(dimension) {
            issues.push(format!(
                "final review dimension not acknowledged: {dimension}"
            ));
        }
    }
    json!({"status":if issues.is_empty() && unresolved.is_empty(){"declared_checks_passed"}else if issues.is_empty(){"declared_checks_passed_with_unknowns"}else{"needs_revision"},"issues":issues,"summary_claim_ids":review.summary_claim_ids,"finding_claim_ids":review.finding_claim_ids,"unresolved_question_ids":review.unresolved_question_ids,"acknowledged_prose_checks":review.checks,"limitations":["Claim mappings are checked deterministically; prose checks are caller acknowledgments, not validated prose or causal proof. Unknown facts must be reported explicitly outside the verified observation/measurement claim list."]})
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct CountOperation {
    kind: crate::config::OperationKind,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CountPopulation {
    pointer: String,
    operation: Option<CountOperation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FactField {
    pointer: String,
    equals: Value,
}

/// Labels describe the answer; these fields identify its evidence subject.
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Subject {
    snapshot_id: Value,
    input_ordinal: u64,
    #[serde(default)]
    scope_id: Option<String>,
    #[serde(default)]
    root: Option<Value>,
    #[serde(default)]
    identity: Option<Value>,
    #[serde(default)]
    event_name: Option<String>,
    source: Value,
    target: Value,
    #[serde(default)]
    start_operation: Option<String>,
    #[serde(default)]
    end_operation: Option<String>,
}

fn occurrence_anchor(value: &Value) -> bool {
    !value["snapshot_id"].is_null()
        && value["input_ordinal"].is_u64()
        && value["evidence_ref"]["reference_id"]
            .as_str()
            .is_some_and(|v| !v.is_empty())
        && value["evidence_ref"]["input_id"]
            .as_str()
            .is_some_and(|v| !v.is_empty())
        && value["evidence_ref"]["line"]
            .as_u64()
            .is_some_and(|v| v > 0)
        && value["evidence_ref"].get("row_path").is_some()
}
fn complete_occurrence(value: &Value) -> bool {
    occurrence_anchor(value) && value["evidence_ref"]["location_redacted"] != true
}

/// Editable proposals only: never infer missing endings or acknowledge caller review.
pub(super) fn draft(retained: &Value, selected_id: Option<&str>) -> Result<Value> {
    if retained["content"] == "redacted" {
        return Err("Source-bound claim drafts require unredacted occurrence identity".into());
    }
    let mut claims = Vec::new();
    for (fi, finding) in retained["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        if selected_id.is_some_and(|id| finding["id"] != id) {
            continue;
        }
        let chain = &finding["details"]["event_chain"];
        if chain["ambiguity"] != "none" || !complete_occurrence(&chain["root"]) {
            continue;
        }
        for (si, step) in chain["steps"].as_array().into_iter().flatten().enumerate() {
            if step["status"] != "observed" || step["count_completeness"] != "complete" {
                continue;
            }
            let pointer = format!("/findings/{fi}/details/event_chain/steps/{si}");
            for (ji, join) in step["join_samples"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                if !complete_occurrence(&join["source"]) || !complete_occurrence(&join["target"]) {
                    continue;
                }
                let id = format!(
                    "{}-{}-{ji}",
                    finding["id"].as_str().unwrap_or("finding"),
                    step["id"].as_str().unwrap_or("step")
                );
                let mut subject = json!({"snapshot_id":chain["root"]["snapshot_id"],"input_ordinal":chain["root"]["input_ordinal"],"scope_id":finding["scope_id"],"root":chain["root"],"identity":chain["identity"],"source":join["source"],"target":join["target"]});
                if chain["root_event_name"].is_string() {
                    subject["event_name"] = chain["root_event_name"].clone();
                }
                claims.push(json!({"id":format!("{id}-link"),"label":format!("Candidate {} -> {} link; confirm requested subject",chain["root_event_name"],step["id"]),"kind":"link","finding_id":finding["id"],"step_id":step["id"],"subject":subject}));
                let interval = &join["endpoint_interval"];
                if interval["status"] == "measured"
                    && interval["unit"] == "ms"
                    && complete_occurrence(&interval["source"])
                    && complete_occurrence(&interval["target"])
                    && interval["start_operation"].is_string()
                    && interval["end_operation"].is_string()
                {
                    claims.push(json!({"id":format!("{id}-interval"),"label":"Candidate endpoint interval; independently confirm operation meaning", "kind":"interval","pointer":format!("{pointer}/join_samples/{ji}/endpoint_interval"),"expected_ms":interval["value"],"subject":{"snapshot_id":interval["source"]["snapshot_id"],"input_ordinal":interval["source"]["input_ordinal"],"source":interval["source"],"target":interval["target"],"start_operation":interval["start_operation"],"end_operation":interval["end_operation"]}}));
                }
            }
            let evidence = step["evidence"].as_array();
            if step["omitted_occurrences"] == 0
                && evidence.is_some_and(|items| {
                    items.iter().all(|e| complete_occurrence(&e["occurrence"]))
                })
            {
                let unique: BTreeSet<_> = evidence
                    .into_iter()
                    .flatten()
                    .map(|e| e["occurrence"].to_string())
                    .collect();
                claims.push(json!({"id":format!("{}-{}-count",finding["id"].as_str().unwrap_or("finding"),step["id"].as_str().unwrap_or("step")),"label":"Candidate distinct scoped endpoint records; not total attempts","kind":"distinct_records","pointer":format!("{pointer}/evidence"),"expected":unique.len()}));
            }
            if claims.len() > 100 {
                return Err("More than 100 draft assertions; select one finding with --id".into());
            }
        }
    }
    if claims.is_empty() {
        return Err(
            "No complete supported subject witnesses to draft; retrieve gaps and preserve unknowns"
                .into(),
        );
    }
    let checklist = super::artifact::checklist(retained);
    let mut required = checklist["required_relationship"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if required.is_empty() {
        required = claims.iter().map(|c| c["id"].clone()).collect();
    }
    Ok(
        json!({"version":1,"questions":checklist["question"].as_array().cloned().unwrap_or_default(),"required_claim_ids":required,"claims":claims,"requirements":[],"unresolved":[],"final_review":{"summary_claim_ids":[],"finding_claim_ids":[],"unresolved_question_ids":[],"checks":[]}}),
    )
}

fn compare_identity(expected: &Value, observed: &Value, keys: &[&str]) -> (Option<bool>, String) {
    // Incomplete asserted addresses identify no subject, even if other fields differ.
    for key in keys {
        if matches!(*key, "root" | "source" | "target") && !complete_occurrence(&expected[*key]) {
            return (
                None,
                format!("subject.{key} expected source address unavailable"),
            );
        }
    }
    if keys.contains(&"identity")
        && !expected["identity"]
            .as_array()
            .is_some_and(|items| !items.is_empty() && items.iter().all(|v| !v["value"].is_null()))
    {
        return (
            None,
            "subject.identity expected scope or event ID unavailable".into(),
        );
    }
    // A known mismatch wins over an unrelated unavailable dimension.
    for key in keys {
        if !expected[*key].is_null()
            && !observed[*key].is_null()
            && expected[*key] != observed[*key]
        {
            return (Some(false), format!("subject.{key} mismatch"));
        }
    }
    for key in keys {
        if expected[*key].is_null() || observed[*key].is_null() {
            return (None, format!("subject.{key} identity unavailable"));
        }
        if matches!(*key, "root" | "source" | "target") && !complete_occurrence(&observed[*key]) {
            return (None, format!("subject.{key} source address unavailable"));
        }
        if *key == "identity"
            && !observed[*key].as_array().is_some_and(|items| {
                !items.is_empty() && items.iter().all(|v| !v["value"].is_null())
            })
        {
            return (
                None,
                "subject.identity scope or event ID unavailable".into(),
            );
        }
    }
    (Some(true), "expected subject identity matched".into())
}

fn subject_check(
    retained: &Value,
    assertion: &Assertion,
) -> Option<(Option<bool>, Value, Value, String)> {
    let (subject, mut observed, mut keys) = match assertion {
        Assertion::Link {
            finding_id,
            step_id,
            subject: Some(subject),
        } => {
            let finding = retained["findings"]
                .as_array()
                .and_then(|items| items.iter().find(|f| f["id"] == *finding_id));
            let chain = finding
                .map(|f| &f["details"]["event_chain"])
                .unwrap_or(&Value::Null);
            let step = chain["steps"]
                .as_array()
                .and_then(|items| items.iter().find(|s| s["id"] == *step_id));
            let joins = step.and_then(|s| s["join_samples"].as_array());
            // Do not borrow endpoints from different supported joins.
            let join = joins.and_then(|items| {
                items
                    .iter()
                    .find(|j| j["source"] == subject.source && j["target"] == subject.target)
            });
            let mut observed = json!({"snapshot_id":chain["root"]["snapshot_id"],"input_ordinal":chain["root"]["input_ordinal"],"scope_id":finding.map(|f| &f["scope_id"]),"root":chain["root"],"identity":chain["identity"],"event_name":chain["root_event_name"],"source":join.map(|j| &j["source"]),"target":join.map(|j| &j["target"])});
            if join.is_none() {
                observed["candidate_endpoints"] = json!(joins.map(|items| {
                    items
                        .iter()
                        .map(|j| json!({"source":j["source"],"target":j["target"]}))
                        .collect::<Vec<_>>()
                }));
            }
            let expected = serde_json::to_value(subject).unwrap();
            let keys = [
                "snapshot_id",
                "input_ordinal",
                "scope_id",
                "root",
                "identity",
            ];
            let (result, reason) = compare_identity(&expected, &observed, &keys);
            if result != Some(true) {
                return Some((result, expected, observed, reason));
            }
            if !complete_occurrence(&subject.source) || !complete_occurrence(&subject.target) {
                return Some((
                    None,
                    expected,
                    observed,
                    "expected source/target identity unavailable".into(),
                ));
            }
            if join.is_none() {
                let omitted = step.is_none_or(|s| {
                    s["omitted_occurrences"].as_u64().unwrap_or(0) > 0
                        || s["status"] == "unavailable"
                        || s["status"] == "ambiguous"
                        || s["count_completeness"] != "complete"
                });
                return Some((
                    if omitted { None } else { Some(false) },
                    expected,
                    observed,
                    if omitted {
                        "expected source/target pair unavailable in retained samples"
                    } else {
                        "expected source/target pair is not a supported join for this subject"
                    }
                    .into(),
                ));
            }
            (
                subject,
                observed,
                vec![
                    "snapshot_id",
                    "input_ordinal",
                    "scope_id",
                    "root",
                    "identity",
                    "source",
                    "target",
                ],
            )
        }
        Assertion::Interval {
            pointer,
            subject: Some(subject),
            ..
        } => {
            let interval = retained.pointer(pointer).unwrap_or(&Value::Null);
            (
                subject,
                json!({"snapshot_id":interval["source"]["snapshot_id"],"input_ordinal":interval["source"]["input_ordinal"],"source":interval["source"],"target":interval["target"],"start_operation":interval["start_operation"],"end_operation":interval["end_operation"]}),
                vec![
                    "snapshot_id",
                    "input_ordinal",
                    "source",
                    "target",
                    "start_operation",
                    "end_operation",
                ],
            )
        }
        _ => return None,
    };
    let expected = serde_json::to_value(subject).unwrap();
    if subject.event_name.is_some() {
        keys.push("event_name");
    }
    // Extra supplied identity dimensions must also be checked, never silently ignored.
    for key in [
        "scope_id",
        "root",
        "identity",
        "start_operation",
        "end_operation",
    ] {
        if !expected[key].is_null() && !keys.contains(&key) {
            keys.push(key);
        }
    }
    if retained["content"] == "redacted" {
        observed = Value::Null;
    }
    let (result, reason) = compare_identity(&expected, &observed, &keys);
    Some((result, expected, observed, reason))
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
            ..
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
            ..
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
        Assertion::DistinctRecords {
            pointer,
            expected,
            operation,
        } => {
            let operation_kind = operation.as_ref().map(|o| json!(o.kind));
            let count = retained
                .pointer(pointer)
                .and_then(Value::as_array)
                .and_then(|items| {
                    let mut identities = BTreeSet::new();
                    for item in items {
                        if let Some(operation) = operation {
                            let classification = &item["fields"]["classification"];
                            if classification["status"] != "event" {
                                return None;
                            }
                            let semantics = &classification["semantics"];
                            if semantics["kind"] != *operation_kind.as_ref().unwrap()
                                || semantics["name"] != operation.name
                            {
                                continue;
                            }
                        }
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
fn check_requirements(
    retained: &Value,
    assertions: &Assertions,
    checks: &[Value],
) -> Result<(Vec<Value>, BTreeSet<String>)> {
    let required: BTreeSet<_> = assertions
        .required_claim_ids
        .iter()
        .map(String::as_str)
        .collect();
    let mut specs = BTreeSet::new();
    for spec in &assertions.requirements {
        if !required.contains(spec.id.as_str())
            || !specs.insert(spec.id.as_str())
            || !matches!(
                spec.kind.as_str(),
                "interval" | "link" | "fact" | "distinct_records"
            )
        {
            return Err(
                "Typed requirements must have unique required IDs and supported claim kinds".into(),
            );
        }
        if spec.kind == "interval"
            && (spec.finding_id.as_ref().is_none_or(String::is_empty)
                || spec.start_operation.as_ref().is_none_or(String::is_empty)
                || spec.end_operation.as_ref().is_none_or(String::is_empty))
        {
            return Err(
                "Interval requirements need finding_id and exact start/end operation names".into(),
            );
        }
        if spec.kind == "distinct_records"
            && spec.population.as_ref().is_none_or(|p| {
                !p.pointer.starts_with('/')
                    || p.operation.as_ref().is_some_and(|o| o.name.is_empty())
            })
        {
            return Err("Count requirements need a retained population pointer and optional operation filter".into());
        }
        if spec.kind == "link"
            && (spec.finding_id.as_ref().is_none_or(String::is_empty)
                || spec.step_id.as_ref().is_none_or(String::is_empty))
        {
            return Err("Link requirements need a finding_id and step_id".into());
        }
        if spec.kind == "fact"
            && (spec
                .field
                .as_ref()
                .is_some_and(|f| !f.pointer.starts_with('/'))
                || (spec.field.is_none() && assertions.claims.iter().any(|c| c.id == spec.id)))
        {
            return Err(
                "Supported fact requirements need a retained field pointer and expected value"
                    .into(),
            );
        }
    }
    if !specs.is_empty() && specs != required {
        return Err(
            "Typed requirements must cover every required ID, including unknown questions".into(),
        );
    }
    let mut unknowns = BTreeSet::new();
    for unknown in &assertions.unresolved {
        if !required.contains(unknown.id.as_str())
            || !unknowns.insert(unknown.id.clone())
            || unknown.reason.trim().is_empty()
            || unknown.reason.chars().count() > 4096
            || assertions.claims.iter().any(|c| c.id == unknown.id)
        {
            return Err(
                "Unresolved questions need unique required IDs, a reason, and no same-ID assertion"
                    .into(),
            );
        }
        if !specs.contains(unknown.id.as_str()) {
            return Err(
                "Unresolved questions require a typed requirement and retained finding anchor"
                    .into(),
            );
        }
    }
    let mut results = Vec::new();
    if assertions.final_review.is_some() {
        for id in required.difference(&specs) {
            results.push(json!({"id":id,"status":"needs_revision","reason":"typed_question_requirement_missing"}));
        }
    }
    for spec in &assertions.requirements {
        let finding = retained["findings"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .find(|(_, f)| spec.finding_id.as_ref().is_some_and(|id| f["id"] == *id));
        let chain = finding.map(|(_, f)| &f["details"]["event_chain"]);
        let root_matches = chain.is_some_and(|c| {
            complete_occurrence(&c["root"])
                && (spec.kind == "interval"
                    || spec
                        .start_operation
                        .as_ref()
                        .is_none_or(|name| c["root_event_name"] == *name))
        });
        let (ok, reason) = if unknowns.contains(&spec.id) {
            let redacted_anchor = retained["content"] == "redacted"
                && chain.is_some_and(|c| {
                    occurrence_anchor(&c["root"])
                        && c["root"]["evidence_ref"]["location_redacted"] == true
                        && (spec.kind == "interval"
                            || spec
                                .start_operation
                                .as_ref()
                                .is_none_or(|name| c["root_event_name"] == *name))
                });
            let root_matches = root_matches || redacted_anchor;
            let mut intervals = Vec::new();
            if spec.kind == "interval"
                && let Some((fi, finding)) = finding
            {
                for (si, step) in finding["details"]["event_chain"]["steps"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    for (ji, join) in step["join_samples"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .enumerate()
                    {
                        let interval = &join["endpoint_interval"];
                        if spec.start_operation.as_ref().is_some_and(|name| interval["start_operation"] == *name)
                            && spec.end_operation.as_ref().is_some_and(|name| interval["end_operation"] == *name)
                            && spec.pointer.as_ref().is_none_or(|pointer| *pointer == format!("/findings/{fi}/details/event_chain/steps/{si}/join_samples/{ji}/endpoint_interval")) {
                            intervals.push(interval);
                        }
                    }
                }
            }
            let available = !redacted_anchor && intervals.iter().any(|i| i["status"] == "measured");
            let step = chain.and_then(|c| c["steps"].as_array()).and_then(|steps| {
                steps
                    .iter()
                    .find(|step| spec.step_id.as_ref().is_some_and(|id| step["id"] == *id))
            });
            let (anchor_valid, available, reason) = match spec.kind.as_str() {
                "interval" => (
                    root_matches && (spec.pointer.is_none() || !intervals.is_empty()),
                    available,
                    "requested_interval_available_cannot_declare_unknown",
                ),
                "link" => {
                    let endpoint_anchor = if redacted_anchor {
                        occurrence_anchor
                    } else {
                        complete_occurrence
                    };
                    let scoped_source = spec.source.as_ref().is_some_and(|source| {
                        endpoint_anchor(source)
                            && chain.is_some_and(|c| {
                                c["root"] == *source
                                    || c["steps"].as_array().into_iter().flatten().any(|s| {
                                        s["join_samples"].as_array().into_iter().flatten().any(
                                            |j| j["source"] == *source || j["target"] == *source,
                                        )
                                    })
                            })
                    });
                    let endpoints = spec.source.is_some() || spec.target.is_some();
                    let exact_available = step.is_some_and(|s| {
                        s["status"] == "observed"
                            && s["count_completeness"] == "complete"
                            && s["join_samples"].as_array().into_iter().flatten().any(|j| {
                                Some(&j["source"]) == spec.source.as_ref()
                                    && Some(&j["target"]) == spec.target.as_ref()
                            })
                    });
                    (
                        root_matches
                            && step.is_some()
                            && (!endpoints
                                || (scoped_source
                                    && spec.target.as_ref().is_some_and(endpoint_anchor))),
                        !redacted_anchor
                            && if endpoints {
                                exact_available
                            } else {
                                check(
                                    retained,
                                    &Assertion::Link {
                                        finding_id: spec.finding_id.clone().unwrap(),
                                        step_id: spec.step_id.clone().unwrap(),
                                        subject: None,
                                    },
                                )
                                .0 == Some(true)
                            },
                        "requested_link_available_cannot_declare_unknown",
                    )
                }
                "distinct_records" => {
                    let population = spec.population.as_ref().unwrap();
                    let measurable = check(
                        retained,
                        &Assertion::DistinctRecords {
                            pointer: population.pointer.clone(),
                            expected: 0,
                            operation: population.operation.clone(),
                        },
                    )
                    .0
                    .is_some();
                    (
                        root_matches
                            && retained
                                .pointer(&population.pointer)
                                .is_some_and(Value::is_array),
                        measurable,
                        "requested_count_available_cannot_declare_unknown",
                    )
                }
                "fact" => (
                    root_matches,
                    spec.field
                        .as_ref()
                        .is_some_and(|f| retained.pointer(&f.pointer).is_some()),
                    "requested_field_available_cannot_declare_unknown",
                ),
                _ => unreachable!(),
            };
            (
                anchor_valid && !available,
                if !anchor_valid {
                    "unresolved_finding_anchor_unavailable_or_wrong_subject"
                } else if available {
                    reason
                } else {
                    "explicit_unknown_anchored_not_verified_as_a_fact"
                },
            )
        } else if let Some(claim) = assertions.claims.iter().find(|c| c.id == spec.id) {
            let check = checks.iter().find(|c| c["id"] == spec.id).unwrap();
            let kind = match claim.assertion {
                Assertion::Fact { .. } => "fact",
                Assertion::Interval { .. } => "interval",
                Assertion::Link { .. } => "link",
                Assertion::DistinctRecords { .. } => "distinct_records",
                Assertion::Absence { .. } => "absence",
            };
            let bound = match &claim.assertion {
                Assertion::Interval { pointer, .. } => {
                    chain.is_some_and(|c| complete_occurrence(&c["root"]))
                        && retained["content"] != "redacted"
                        && spec.pointer.as_ref() == Some(pointer)
                        && finding.is_some_and(|(index, _)| {
                            pointer.starts_with(&format!(
                                "/findings/{index}/details/event_chain/steps/"
                            ))
                        })
                        && spec
                            .start_operation
                            .as_ref()
                            .is_none_or(|name| check["observed"]["start_operation"] == *name)
                        && spec
                            .end_operation
                            .as_ref()
                            .is_none_or(|name| check["observed"]["end_operation"] == *name)
                }
                Assertion::Link {
                    finding_id,
                    step_id,
                    ..
                } => {
                    retained["content"] != "redacted"
                        && spec.finding_id.as_ref() == Some(finding_id)
                        && spec.step_id.as_ref() == Some(step_id)
                        && finding.is_some_and(|(fi, _)| {
                            chain.is_some_and(|c| c["steps"].as_array().into_iter().flatten().enumerate().any(|(si, step)| {
                                step["id"] == *step_id && step["join_samples"].as_array().into_iter().flatten().enumerate().any(|(ji, join)| {
                                    spec.pointer.as_deref() == Some(format!("/findings/{fi}/details/event_chain/steps/{si}/join_samples/{ji}").as_str())
                                        && complete_occurrence(&join["source"])
                                        && complete_occurrence(&join["target"])
                                        && join["source"] == check["identity"]["observed"]["source"]
                                        && join["target"] == check["identity"]["observed"]["target"]
                                })
                            }))
                        })
                }
                Assertion::DistinctRecords {
                    pointer, operation, ..
                } => spec
                    .population
                    .as_ref()
                    .is_some_and(|p| p.pointer == *pointer && p.operation == *operation),
                Assertion::Fact { pointer, equals } => spec
                    .field
                    .as_ref()
                    .is_some_and(|f| f.pointer == *pointer && f.equals == *equals),
                Assertion::Absence { .. } => false,
            };
            (
                kind == spec.kind && bound && check["status"] == "verified",
                "claim_kind_and_requested_subject_must_match",
            )
        } else {
            (
                false,
                "required_question_has_no_assertion_or_explicit_unknown",
            )
        };
        results.push(json!({"id":spec.id,"kind":spec.kind,"finding_id":spec.finding_id,"status":if !ok {"needs_revision"} else if unknowns.contains(&spec.id) {"declared_unknown"} else {"verified"},"reason":reason}));
    }
    Ok((results, unknowns))
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
    let mut assertions: Assertions = serde_json::from_slice(&bytes)?;
    // The initial checklist cannot be silently dropped by a later claims file.
    let checklist = super::artifact::checklist(retained);
    for id in checklist["required_relationship"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !assertions
            .required_claim_ids
            .iter()
            .any(|required| required == id)
        {
            assertions.required_claim_ids.push(id.to_owned());
        }
    }
    if assertions.questions.is_empty() {
        assertions.questions = checklist["question"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
    }
    if assertions.questions.len() > 100 {
        return Err("At most 100 questions are supported".into());
    }
    if assertions.version != 1
        || assertions.required_claim_ids.is_empty()
        || assertions.required_claim_ids.len() > 100
        || (assertions.claims.is_empty() && assertions.unresolved.is_empty())
        || assertions.claims.len() > 100
    {
        return Err("Claims require version 1, 1..100 required IDs, at most 100 assertions and at least one assertion or explicit unknown".into());
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
        let (mut result, observed, reason) = check(retained, &claim.assertion);
        let mut reason = reason.to_owned();
        let mut identity = Value::Null;
        let mut assurance = match claim.assertion {
            Assertion::Link { .. } => "link_exists",
            Assertion::Interval { .. } => "retained_interval",
            _ => "retained_assertion",
        };
        if let Some((subject_result, expected, actual, subject_reason)) =
            subject_check(retained, &claim.assertion)
        {
            identity = json!({"expected":expected,"observed":actual,"status":match subject_result {Some(true)=>"matched",Some(false)=>"mismatch",None=>"unavailable"},"reason":subject_reason});
            if subject_result != Some(true) {
                result = subject_result;
                reason = subject_reason;
            }
            assurance = if result == Some(true) {
                "subject_verified"
            } else {
                "subject_unverified"
            };
        }
        let citations = if identity.is_object() {
            json!({"root":identity["observed"]["root"],"source":identity["observed"]["source"],"target":identity["observed"]["target"],"candidate_endpoints":identity["observed"]["candidate_endpoints"]})
        } else {
            Value::Null
        };
        checks.push(json!({"id":claim.id,"label":claim.label,"status":match result {Some(true)=>"verified",Some(false)=>"contradicted",None=>"unavailable"},"assurance":assurance,"identity":identity,"source_citations":citations,"observed":observed,"reason":reason}));
    }
    let (requirement_results, unresolved_ids) = check_requirements(retained, &assertions, &checks)?;
    let missing: Vec<_> = assertions
        .required_claim_ids
        .iter()
        .filter(|id| !ids.contains(id.as_str()) && !unresolved_ids.contains(id.as_str()))
        .collect();
    let final_review = assertions.final_review.as_ref().map(|review| {
        let mut result = review_final(
            review,
            &checks,
            &assertions.required_claim_ids,
            &assertions.unresolved,
        );
        for requirement in &requirement_results {
            if requirement["status"] == "needs_revision" {
                result["issues"].as_array_mut().unwrap().push(json!(format!(
                    "question requirement not satisfied: {}",
                    requirement["id"]
                )));
                result["status"] = json!("needs_revision");
            }
        }
        result
    });
    let final_ok = final_review.as_ref().is_none_or(|r| {
        r["status"] == "declared_checks_passed"
            || r["status"] == "declared_checks_passed_with_unknowns"
    });
    Ok(
        json!({"version":1,"final_review":final_review,"questions":assertions.questions,"required_claim_ids":assertions.required_claim_ids,"status":if !final_ok || !missing.is_empty() || checks.iter().any(|c|c["status"]!="verified") || requirement_results.iter().any(|r|r["status"]=="needs_revision"){"needs_revision"}else if !unresolved_ids.is_empty(){"verified_with_unknowns"}else{"verified"},"missing_required_claim_ids":missing,"unresolved_required_claim_ids":unresolved_ids,"unresolved":assertions.unresolved,"requirement_results":requirement_results,"checks":checks,"limitations":["Only declared assertions are checked against the retained snapshot. This does not validate free-text claims, profile meaning, upstream completeness or causality. Required IDs must be derived from the question before drafting the answer."]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn draft_legacy_witnesses_omit_unavailable_names_and_interval_addresses() {
        let source = json!({"snapshot_id":"snapshot","input_ordinal":0,"evidence_ref":{"reference_id":"source","input_id":"input","line":1,"row_path":null,"location_redacted":false}});
        let target = json!({"snapshot_id":"snapshot","input_ordinal":0,"evidence_ref":{"reference_id":"target","input_id":"input","line":2,"row_path":null,"location_redacted":false}});
        let mut retained = json!({"findings":[{"id":"f","scope_id":"s","details":{"event_chain":{"root":source,"identity":[{"field":"id","value":"a"}],"ambiguity":"none","steps":[{"id":"end","status":"observed","count_completeness":"complete","join_samples":[{"source":source,"target":target,"endpoint_interval":{"status":"measured","unit":"ms","value":10,"start_operation":"start","end_operation":"end"}}]}]}}}]});
        let negative = draft(&retained, None).unwrap();
        assert_eq!(negative["claims"].as_array().unwrap().len(), 1);
        assert!(negative["claims"][0]["subject"].get("event_name").is_none());
        let schema: Value =
            serde_json::from_str(include_str!("../../schemas/claim-assertions.schema.json"))
                .unwrap();
        jsonschema::validator_for(&schema)
            .unwrap()
            .validate(&negative)
            .unwrap();
        let interval = &mut retained["findings"][0]["details"]["event_chain"]["steps"][0]["join_samples"]
            [0]["endpoint_interval"];
        interval["source"] = source;
        interval["target"] = target;
        let positive = draft(&retained, None).unwrap();
        assert_eq!(positive["claims"].as_array().unwrap().len(), 2);
        jsonschema::validator_for(&schema)
            .unwrap()
            .validate(&positive)
            .unwrap();
    }
    #[test]
    fn intervals_recalculate_offsets_and_reject_unavailable() {
        let retained = json!({"i":{"status":"measured","unit":"ms","start_timestamp":"2026-01-01T02:00:00.010+02:00","end_timestamp":"2026-01-01T00:00:00.025Z"}});
        assert_eq!(
            check(
                &retained,
                &Assertion::Interval {
                    pointer: "/i".into(),
                    expected_ms: 15.0,
                    subject: None
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
                    expected_ms: 14.0,
                    subject: None
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
                    expected_ms: 15.0,
                    subject: None
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
                    expected: 1,
                    operation: None
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
                    step_id: "end".into(),
                    subject: None
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

#[cfg(test)]
mod final_review_tests {
    use super::*;
    fn review() -> FinalReview {
        FinalReview {
            summary_claim_ids: vec!["end".into()],
            finding_claim_ids: vec!["end".into()],
            unresolved_question_ids: Vec::new(),
            checks: [
                "event_subjects",
                "duration_endpoints_and_units",
                "operation_types_and_count_semantics",
                "terminal_later_cached_contrary",
                "summary_consistency",
                "causal_support",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        }
    }
    #[test]
    fn final_summary_requires_supported_subject_and_detailed_mapping() {
        let supported = json!({"id":"end","status":"verified","assurance":"subject_verified"});
        assert_eq!(
            review_final(&review(), &[supported.clone()], &["end".into()], &[])["status"],
            "declared_checks_passed"
        );
        for status in ["contradicted", "unavailable"] {
            let mut wrong = supported.clone();
            wrong["status"] = json!(status);
            assert_eq!(
                review_final(&review(), &[wrong], &["end".into()], &[])["status"],
                "needs_revision"
            );
            assert_eq!(
                review_final(&review(), &[supported.clone()], &["end".into()], &[])["status"],
                "declared_checks_passed"
            );
        }
        let mut legacy = supported.clone();
        legacy["assurance"] = json!("link_exists");
        assert_eq!(
            review_final(&review(), &[legacy], &["end".into()], &[])["status"],
            "needs_revision"
        );
        let mut wrong = review();
        wrong.finding_claim_ids.clear();
        assert_eq!(
            review_final(&wrong, &[supported.clone()], &["end".into()], &[])["status"],
            "needs_revision"
        );
        let mut wrong = review();
        wrong.checks.remove("causal_support");
        assert_eq!(
            review_final(&wrong, &[supported.clone()], &["end".into()], &[])["status"],
            "needs_revision"
        );
        assert_eq!(
            review_final(&review(), &[supported], &["end".into()], &[])["status"],
            "declared_checks_passed"
        );
    }
    #[test]
    fn interval_requirements_bind_intermediate_sources_to_exact_findings_and_names() {
        let root = json!({"snapshot_id":"s","input_ordinal":0,"evidence_ref":{"input_id":"i","reference_id":"r","line":1,"row_path":null}});
        let mut source = root.clone();
        source["evidence_ref"]["line"] = json!(2);
        let interval = json!({"status":"measured","unit":"ms","source":source,"start_operation":"submitted","end_operation":"end",
            "start_timestamp":"2026-01-01T00:00:00Z","end_timestamp":"2026-01-01T00:00:00.010Z"});
        let finding = |id| {
            json!({"id":id,"details":{"event_chain":{"root":root,"root_event_name":"start",
            "steps":[{"join_samples":[{"endpoint_interval":interval}]}]}}})
        };
        let mut retained = json!({"findings":[finding("a"),finding("b")]});
        let mut other = interval.clone();
        other["source"]["evidence_ref"]["line"] = json!(3);
        other["end_timestamp"] = json!("2026-01-01T00:00:00.020Z");
        retained["findings"][0]["details"]["event_chain"]["steps"][0]["join_samples"]
            .as_array_mut()
            .unwrap()
            .push(json!({"endpoint_interval":other}));
        let mut assertions = json!({"version":1,"required_claim_ids":["duration"],
            "requirements":[{"id":"duration","kind":"interval","finding_id":"a","pointer":"/findings/0/details/event_chain/steps/0/join_samples/0/endpoint_interval","start_operation":"submitted","end_operation":"end"}],
            "claims":[{"id":"duration","kind":"interval","pointer":"/findings/0/details/event_chain/steps/0/join_samples/0/endpoint_interval","expected_ms":10}]});
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("interval.json");
        let check = |a: &Value| {
            fs::write(&path, a.to_string()).unwrap();
            verify(&retained, &path).unwrap()
        };
        assert_eq!(check(&assertions)["status"], "verified");
        assertions["claims"][0]["pointer"] =
            json!("/findings/0/details/event_chain/steps/0/join_samples/1/endpoint_interval");
        assertions["claims"][0]["expected_ms"] = json!(20);
        let swapped = check(&assertions);
        assert_eq!(swapped["checks"][0]["status"], "verified");
        assert_eq!(swapped["status"], "needs_revision");
        assertions["claims"][0]["expected_ms"] = json!(10);
        assertions["claims"][0]["pointer"] =
            json!("/findings/1/details/event_chain/steps/0/join_samples/0/endpoint_interval");
        let wrong = check(&assertions);
        assert_eq!(wrong["checks"][0]["status"], "verified");
        assert_eq!(wrong["status"], "needs_revision");
        assertions["requirements"][0]["finding_id"] = json!("b");
        assertions["requirements"][0]["pointer"] = assertions["claims"][0]["pointer"].clone();
        assert_eq!(check(&assertions)["status"], "verified");
        for key in ["start_operation", "end_operation"] {
            let expected = assertions["requirements"][0][key].clone();
            assertions["requirements"][0][key] = json!("wrong-operation");
            assert_eq!(check(&assertions)["status"], "needs_revision");
            assertions["requirements"][0][key] = expected;
        }
    }

    #[test]
    fn intermediate_unknowns_reject_available_intervals_and_count_unicode_characters() {
        let root = json!({"snapshot_id":"s","input_ordinal":0,"evidence_ref":{"input_id":"i","reference_id":"r","line":1,"row_path":null}});
        let mut source = root.clone();
        source["evidence_ref"]["line"] = json!(2);
        let interval = json!({"status":"measured","unit":"ms","source":source,"start_operation":"capture-start","end_operation":"capture-end"});
        let mut retained = json!({"findings":[{"id":"f","details":{"event_chain":{"root":root,"root_event_name":"checkpoint",
            "steps":[{"join_samples":[{"endpoint_interval":interval}]}]}}}]});
        let mut assertions = json!({"version":1,"required_claim_ids":["duration"],"claims":[],
            "requirements":[{"id":"duration","kind":"interval","finding_id":"f","start_operation":"capture-start","end_operation":"capture-end"}],
            "unresolved":[{"id":"duration","reason":"界".repeat(4096)}],
            "final_review":{"finding_claim_ids":[],"summary_claim_ids":[],"unresolved_question_ids":["duration"],
                "checks":["event_subjects","duration_endpoints_and_units","operation_types_and_count_semantics","terminal_later_cached_contrary","summary_consistency","causal_support"]}});
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("unknown.json");
        let schema: Value =
            serde_json::from_str(include_str!("../../schemas/claim-assertions.schema.json"))
                .unwrap();
        let schema = jsonschema::validator_for(&schema).unwrap();
        schema.validate(&assertions).unwrap();
        let verify_case = |r: &Value, a: &Value| {
            fs::write(&path, a.to_string()).unwrap();
            verify(r, &path)
        };
        let available = verify_case(&retained, &assertions).unwrap();
        assert_eq!(available["status"], "needs_revision");
        assert_eq!(
            available["requirement_results"][0]["reason"],
            "requested_interval_available_cannot_declare_unknown"
        );
        let mut unavailable = interval.clone();
        unavailable["source"]["evidence_ref"]["line"] = json!(3);
        unavailable["status"] = json!("unavailable");
        retained["findings"][0]["details"]["event_chain"]["steps"][0]["join_samples"]
            .as_array_mut()
            .unwrap()
            .push(json!({"endpoint_interval":unavailable}));
        assertions["requirements"][0]["pointer"] =
            json!("/findings/0/details/event_chain/steps/0/join_samples/1/endpoint_interval");
        let unknown = verify_case(&retained, &assertions).unwrap();
        assert_eq!(unknown["status"], "verified_with_unknowns");
        assert_eq!(
            unknown["final_review"]["status"],
            "declared_checks_passed_with_unknowns"
        );
        for pointer in [
            "/findings/0/details/event_chain/steps/0/join_samples/0/endpoint_interval",
            "/missing",
        ] {
            let mut wrong = assertions.clone();
            wrong["requirements"][0]["pointer"] = json!(pointer);
            assert_eq!(
                verify_case(&retained, &wrong).unwrap()["status"],
                "needs_revision"
            );
        }
        assertions["requirements"][0]["finding_id"] = json!("wrong");
        assert_eq!(
            verify_case(&retained, &assertions).unwrap()["status"],
            "needs_revision"
        );
        assertions["requirements"][0]["finding_id"] = json!("f");
        assertions["unresolved"][0]["reason"] = json!("界".repeat(4097));
        assert!(!schema.is_valid(&assertions));
        assert!(verify_case(&retained, &assertions).is_err());
    }

    #[test]
    fn redacted_roots_anchor_unknowns_but_not_supported_events() {
        let root = json!({"snapshot_id":"s","input_ordinal":0,"evidence_ref":{"input_id":"i","reference_id":"r","line":1,"row_path":null,"location_redacted":true}});
        let interval = json!({"status":"measured","unit":"ms","source":root,"start_operation":"start","end_operation":"end",
            "start_timestamp":"2026-01-01T00:00:00Z","end_timestamp":"2026-01-01T00:00:00.010Z"});
        let retained = json!({"content":"redacted","findings":[{"id":"root","details":{"event_chain":{"root":root,"root_event_name":"start","ambiguity":"none",
            "steps":[{"id":"end","status":"observed","linked_count":1,"count_completeness":"complete","join_samples":[{"endpoint_interval":interval}]}]}}}],
            "records":[{"occurrence":root}]});
        let mut assertions = json!({"version":1,"required_claim_ids":["question"],"claims":[],
            "requirements":[],"unresolved":[{"id":"question","reason":"Source identity redacted"}]});
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("redacted.json");
        let verify_status = |a: &Value| {
            fs::write(&path, a.to_string()).unwrap();
            verify(&retained, &path).unwrap()["status"].clone()
        };
        for selector in [
            json!({"kind":"interval","start_operation":"start","end_operation":"end"}),
            json!({"kind":"link","step_id":"end"}),
            json!({"kind":"link","step_id":"end","source":root,"target":root}),
            json!({"kind":"distinct_records","population":{"pointer":"/records","operation":{"kind":"request","name":"send"}}}),
            json!({"kind":"fact"}),
        ] {
            let mut spec = selector;
            spec["id"] = json!("question");
            spec["finding_id"] = json!("root");
            assertions["requirements"] = json!([spec]);
            assert_eq!(verify_status(&assertions), "verified_with_unknowns");
            assertions["requirements"][0]["finding_id"] = json!("missing");
            assert_eq!(verify_status(&assertions), "needs_revision");
        }
        assertions["requirements"] = json!([{"id":"question","kind":"distinct_records","finding_id":"root","population":{"pointer":"/records"}}]);
        assert_eq!(verify_status(&assertions), "needs_revision");
        assertions["requirements"] =
            json!([{"id":"question","kind":"link","finding_id":"root","step_id":"missing"}]);
        assert_eq!(verify_status(&assertions), "needs_revision");
        assertions["requirements"][0]["step_id"] = json!("end");
        assertions["unresolved"] = json!([]);
        assertions["claims"] =
            json!([{"id":"question","kind":"link","finding_id":"root","step_id":"end"}]);
        assert_eq!(verify_status(&assertions), "needs_revision");
        assertions["requirements"] = json!([{"id":"question","kind":"interval","finding_id":"root","start_operation":"start","end_operation":"end"}]);
        assertions["claims"] = json!([{"id":"question","kind":"interval","pointer":"/findings/0/details/event_chain/steps/0/join_samples/0/endpoint_interval","expected_ms":10}]);
        assert_eq!(verify_status(&assertions), "needs_revision");
    }

    #[test]
    fn unresolved_subjects_reject_available_or_invalid_evidence() {
        let root = json!({"snapshot_id":"s","input_ordinal":0,"evidence_ref":{"input_id":"i","reference_id":"r","line":1,"row_path":null}});
        let mut retained = json!({"findings":[{"id":"root","details":{"event_chain":{"root":root,"ambiguity":"none",
            "steps":[{"id":"end","status":"not_observed","linked_count":0,"count_completeness":"complete"}]}}}],
            "records":[{"occurrence":root,"fields":{"classification":{"status":"unclassified"}}}],"known":null});
        let mut assertions = json!({"version":1,"required_claim_ids":["question"],"claims":[],
            "requirements":[{"id":"question","kind":"link","finding_id":"root","step_id":"end"}],
            "unresolved":[{"id":"question","reason":"No supported relationship"}],
            "final_review":{"summary_claim_ids":[],"finding_claim_ids":[],"unresolved_question_ids":["question"],
                "checks":["event_subjects","duration_endpoints_and_units","operation_types_and_count_semantics","terminal_later_cached_contrary","summary_consistency","causal_support"]}});
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("unknown.json");
        let schema: Value =
            serde_json::from_str(include_str!("../../schemas/claim-assertions.schema.json"))
                .unwrap();
        let schema = jsonschema::validator_for(&schema).unwrap();
        let status = |a: &Value, r: &Value| {
            schema.validate(a).unwrap();
            fs::write(&path, a.to_string()).unwrap();
            verify(r, &path).unwrap()["status"].clone()
        };
        for step_status in ["not_observed", "ambiguous", "unavailable"] {
            retained["findings"][0]["details"]["event_chain"]["steps"][0]["status"] =
                json!(step_status);
            retained["findings"][0]["details"]["event_chain"]["steps"][0]["linked_count"] =
                json!(if step_status == "not_observed" { 0 } else { 1 });
            assert_eq!(status(&assertions, &retained), "verified_with_unknowns");
        }
        assertions["requirements"][0]["step_id"] = json!("nonexistent");
        assert_eq!(status(&assertions, &retained), "needs_revision");
        assertions["requirements"][0]["step_id"] = json!("end");
        let step = &mut retained["findings"][0]["details"]["event_chain"]["steps"][0];
        step["status"] = json!("observed");
        step["linked_count"] = json!(1);
        assert_eq!(status(&assertions, &retained), "needs_revision");
        let mut target = root.clone();
        target["evidence_ref"]["line"] = json!(2);
        retained["findings"][0]["details"]["event_chain"]["steps"][0]["join_samples"] =
            json!([{"source":root,"target":target}]);
        assertions["requirements"][0]["source"] = root.clone();
        assertions["requirements"][0]["target"] = target.clone();
        assert_eq!(status(&assertions, &retained), "needs_revision");
        assertions["requirements"][0]["target"]["evidence_ref"]["line"] = json!(3);
        assert_eq!(status(&assertions, &retained), "verified_with_unknowns");
        assertions["requirements"][0]["source"]["evidence_ref"]["line"] = json!(99);
        assert_eq!(status(&assertions, &retained), "needs_revision");
        assertions["requirements"][0] = json!({"id":"question","kind":"distinct_records","finding_id":"root", "population":{"pointer":"/records","operation":{"kind":"request","name":"send"}}});
        assert_eq!(status(&assertions, &retained), "verified_with_unknowns");
        retained["records"][0]["fields"]["classification"] =
            json!({"status":"event","semantics":{"kind":"request","name":"send"}});
        assert_eq!(status(&assertions, &retained), "needs_revision");
        assertions["requirements"][0]["population"]["pointer"] = json!("/nonexistent");
        assert_eq!(status(&assertions, &retained), "needs_revision");
        assertions["requirements"][0] = json!({"id":"question","kind":"fact","finding_id":"root", "field":{"pointer":"/missing","equals":null}});
        assert_eq!(status(&assertions, &retained), "verified_with_unknowns");
        assertions["requirements"][0]["field"]["pointer"] = json!("/known");
        assert_eq!(status(&assertions, &retained), "needs_revision");
        assertions["requirements"][0]
            .as_object_mut()
            .unwrap()
            .remove("field");
        assert_eq!(status(&assertions, &retained), "verified_with_unknowns");
    }

    #[test]
    fn link_and_fact_requirements_bind_steps_fields_and_values() {
        let occurrence = |line| json!({"snapshot_id":"s","input_ordinal":0,"evidence_ref":{"input_id":"i","reference_id":"r","line":line,"row_path":null}});
        let source = occurrence(1);
        let step = |id, line| json!({"id":id,"status":"observed","linked_count":1,"count_completeness":"complete","join_samples":[{"source":source,"target":occurrence(line)}]});
        let mut retained = json!({"findings":[{"id":"root","scope_id":"scope","details":{"event_chain":{"root":source,"identity":[{"field":"id","value":"a"}],"ambiguity":"none","steps":[step("end",2),step("cache",3)]}}}],"first":true,"second":false,"same":true});
        retained["findings"][0]["details"]["event_chain"]["steps"][0]["linked_count"] = json!(2);
        retained["findings"][0]["details"]["event_chain"]["steps"][0]["join_samples"]
            .as_array_mut()
            .unwrap()
            .push(json!({"source":source,"target":occurrence(4)}));
        let subject = |line| json!({"snapshot_id":"s","input_ordinal":0,"scope_id":"scope","root":source,"identity":[{"field":"id","value":"a"}],"source":source,"target":occurrence(line)});
        let assertions = json!({"version":1,"required_claim_ids":["completion","failure","first","second"],
            "claims":[
                {"id":"completion","kind":"link","finding_id":"root","step_id":"end","subject":subject(2)},
                {"id":"failure","kind":"link","finding_id":"root","step_id":"cache","subject":subject(3)},
                {"id":"first","kind":"fact","pointer":"/first","equals":true},
                {"id":"second","kind":"fact","pointer":"/second","equals":false}],
            "requirements":[
                {"id":"completion","kind":"link","finding_id":"root","step_id":"end","pointer":"/findings/0/details/event_chain/steps/0/join_samples/0"},
                {"id":"failure","kind":"link","finding_id":"root","step_id":"cache","pointer":"/findings/0/details/event_chain/steps/1/join_samples/0"},
                {"id":"first","kind":"fact","field":{"pointer":"/first","equals":true}},
                {"id":"second","kind":"fact","field":{"pointer":"/second","equals":false}}]});
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("claims.json");
        let schema: Value =
            serde_json::from_str(include_str!("../../schemas/claim-assertions.schema.json"))
                .unwrap();
        let schema = jsonschema::validator_for(&schema).unwrap();
        let verified = |a: &Value| {
            schema.validate(a).unwrap();
            fs::write(&path, a.to_string()).unwrap();
            verify(&retained, &path).unwrap()
        };
        assert_eq!(verified(&assertions)["status"], "verified");
        let mut links = assertions.clone();
        links["claims"][0]["id"] = json!("failure");
        links["claims"][1]["id"] = json!("completion");
        let mut other_target = assertions.clone();
        other_target["claims"][0]["subject"] = subject(4);
        let mut facts = assertions.clone();
        facts["claims"][2]["id"] = json!("second");
        facts["claims"][3]["id"] = json!("first");
        let mut same_value = assertions.clone();
        same_value["claims"][2]["pointer"] = json!("/same");
        let mut wrong_value = assertions.clone();
        wrong_value["requirements"][2]["field"]["equals"] = json!(false);
        for wrong in [links, other_target, facts, same_value, wrong_value] {
            let result = verified(&wrong);
            assert!(
                result["checks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|c| c["status"] == "verified")
            );
            assert_eq!(result["status"], "needs_revision");
            assert_eq!(verified(&assertions)["status"], "verified");
        }
        let mut legacy = assertions.clone();
        legacy.as_object_mut().unwrap().remove("requirements");
        assert_eq!(verified(&legacy)["status"], "verified");
        let mut unbound = assertions;
        unbound["requirements"][2]
            .as_object_mut()
            .unwrap()
            .remove("field");
        fs::write(&path, unbound.to_string()).unwrap();
        assert!(verify(&retained, &path).is_err());
    }

    #[test]
    fn count_questions_bind_operation_and_population() {
        let record = |id, kind, name| json!({"occurrence":{"snapshot_id":"s","input_ordinal":0,"evidence_ref":{"reference_id":id}},"fields":{"classification":{"status":"event","semantics":{"kind":kind,"name":name}}}});
        let records = json!([
            record("a", "request", "send"),
            record("b", "request", "response"),
            record("c", "request", "response"),
            record("d", "event", "send")
        ]);
        let retained = json!({"east":records,"west":records});
        let claim = |id, name, expected| json!({"id":id,"kind":"distinct_records","pointer":"/east","operation":{"kind":"request","name":name},"expected":expected});
        let requirement = |id, name| json!({"id":id,"kind":"distinct_records","population":{"pointer":"/east","operation":{"kind":"request","name":name}}});
        let assertions = json!({"version":1,"required_claim_ids":["send-count","response-count"],
            "claims":[claim("send-count","send",1),claim("response-count","response",2)],
            "requirements":[requirement("send-count","send"),requirement("response-count","response")],
            "final_review":{"summary_claim_ids":["send-count","response-count"],"finding_claim_ids":["send-count","response-count"],
                "checks":["event_subjects","duration_endpoints_and_units","operation_types_and_count_semantics","terminal_later_cached_contrary","summary_consistency","causal_support"]}});
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("claims.json");
        let schema: Value =
            serde_json::from_str(include_str!("../../schemas/claim-assertions.schema.json"))
                .unwrap();
        let schema = jsonschema::validator_for(&schema).unwrap();
        let verified = |a: &Value| {
            schema.validate(a).unwrap();
            fs::write(&path, a.to_string()).unwrap();
            verify(&retained, &path).unwrap()
        };
        assert_eq!(verified(&assertions)["status"], "verified");
        let mut swapped = assertions.clone();
        swapped["claims"][0]["id"] = json!("response-count");
        swapped["claims"][1]["id"] = json!("send-count");
        let mut other_population = assertions.clone();
        other_population["claims"][0]["pointer"] = json!("/west");
        let mut other_kind = assertions.clone();
        other_kind["claims"][0]["operation"]["kind"] = json!("event");
        let mut no_filter = assertions.clone();
        no_filter["claims"][0]
            .as_object_mut()
            .unwrap()
            .remove("operation");
        no_filter["claims"][0]["expected"] = json!(4);
        for wrong in [swapped, other_population, other_kind, no_filter] {
            let result = verified(&wrong);
            assert!(
                result["checks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|c| c["status"] == "verified")
            );
            assert_eq!(result["status"], "needs_revision");
            assert_eq!(result["final_review"]["status"], "needs_revision");
            assert_eq!(verified(&assertions)["status"], "verified");
        }
        let mut zero = assertions.clone();
        zero["claims"][0]["operation"]["kind"] = json!("command");
        zero["claims"][0]["expected"] = json!(0);
        zero["requirements"][0]["population"]["operation"]["kind"] = json!("command");
        assert_eq!(verified(&zero)["status"], "verified");
        for kind in ["reqest", "unsupported", "Request", ""] {
            let mut invalid = assertions.clone();
            invalid["claims"][0]["operation"]["kind"] = json!(kind);
            invalid["claims"][0]["expected"] = json!(0);
            invalid["requirements"][0]["population"]["operation"]["kind"] = json!(kind);
            assert!(!schema.is_valid(&invalid));
            fs::write(&path, invalid.to_string()).unwrap();
            assert!(verify(&retained, &path).is_err());
        }
        let mut unbound = assertions.clone();
        unbound["requirements"][0]
            .as_object_mut()
            .unwrap()
            .remove("population");
        assert!(!schema.is_valid(&unbound));
        fs::write(&path, unbound.to_string()).unwrap();
        assert!(verify(&retained, &path).is_err());
    }

    #[test]
    fn typed_counts_keep_mixed_requests_and_duplicate_serialization_separate() {
        let record = |name| json!({"occurrence":{"snapshot_id":"s","input_ordinal":0,"evidence_ref":{"reference_id":name}},"fields":{"classification":{"status":"event","semantics":{"kind":"request","name":name}}}});
        let a = record("send");
        let b = record("response");
        let retained = json!({"records":[a.clone(),a.clone(),b]});
        let count = |name: &str, expected| Assertion::DistinctRecords {
            pointer: "/records".into(),
            expected,
            operation: Some(CountOperation {
                kind: crate::config::OperationKind::Request,
                name: name.into(),
            }),
        };
        assert_eq!(check(&retained, &count("send", 1)).0, Some(true));
        assert_eq!(check(&retained, &count("send", 2)).0, Some(false));
        assert_eq!(check(&retained, &count("response", 1)).0, Some(true));
        assert_eq!(
            check(
                &json!({"records":[a,{"fields":{"classification":{"status":"unclassified"}}}]}),
                &count("send", 1)
            )
            .0,
            None
        );
        assert_eq!(check(&retained, &count("send", 1)).0, Some(true));
    }
}
