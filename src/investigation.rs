//! Cross-reference and arithmetic invariants for investigation contract 1.
//!
//! Validate document shape against the advertised JSON Schema first. These checks
//! verify retained identities and calculations; they do not establish event meaning,
//! clock synchronization, upstream completeness, or the truth of free-form claims.
use crate::evidence::digest;
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
pub const ARTIFACT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationSummary {
    pub artifact_checked: bool,
    pub deferred_relations: Vec<String>,
}

/// Source references identify evidence; this key identifies a declared occurrence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OccurrenceId {
    pub snapshot_id: String,
    pub input_ordinal: u64,
    pub reference_id: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{path}: {reason}")]
pub struct ContractError {
    pub path: String,
    pub reason: String,
}
fn error(path: &str, reason: impl Into<String>) -> ContractError {
    ContractError {
        path: path.into(),
        reason: reason.into(),
    }
}
fn get<'a>(v: &'a Value, p: &str) -> Result<&'a Value, ContractError> {
    v.pointer(p).ok_or_else(|| error(p, "missing value"))
}
fn text<'a>(v: &'a Value, p: &str) -> Result<&'a str, ContractError> {
    get(v, p)?
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| error(p, "expected nonempty string"))
}
fn number(v: &Value, p: &str) -> Result<u64, ContractError> {
    get(v, p)?
        .as_u64()
        .ok_or_else(|| error(p, "expected nonnegative integer"))
}
fn list<'a>(v: &'a Value, p: &str) -> Result<&'a [Value], ContractError> {
    get(v, p)?
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| error(p, "expected array"))
}
fn require(ok: bool, p: &str, reason: &str) -> Result<(), ContractError> {
    if ok { Ok(()) } else { Err(error(p, reason)) }
}
fn index<'a>(items: &'a [Value], p: &str) -> Result<BTreeMap<&'a str, &'a Value>, ContractError> {
    let mut result = BTreeMap::new();
    for item in items {
        let id = text(item, "/id")?;
        require(result.insert(id, item).is_none(), p, "duplicate id")?;
    }
    Ok(result)
}
impl OccurrenceId {
    pub fn from_value(value: &Value) -> Result<Self, ContractError> {
        Ok(Self {
            snapshot_id: text(value, "/snapshot_id")?.into(),
            input_ordinal: number(value, "/input_ordinal")?,
            reference_id: text(value, "/evidence_ref/reference_id")?.into(),
        })
    }
}

/// Check relations in a schema-validated retained report and optional exact artifact bytes.
/// A missing artifact does not invalidate a self-contained presentation; exhaustive
/// membership and retained-source checks run only when artifact bytes are supplied.
/// Malformed inputs return errors rather than panicking.
pub fn validate_relations(
    report: &Value,
    artifact_bytes: Option<&[u8]>,
) -> Result<ValidationSummary, ContractError> {
    let mut summary = ValidationSummary {
        artifact_checked: artifact_bytes.is_some(),
        deferred_relations: Vec::new(),
    };
    if artifact_bytes.is_none() {
        summary
            .deferred_relations
            .push("retained_membership_and_source_verification".into());
    }
    require(
        number(report, "/contract_version")? == u64::from(CONTRACT_VERSION),
        "/contract_version",
        "unsupported investigation version",
    )?;
    let metadata = get(report, "/report_metadata")?;
    let manifest = get(metadata, "/evidence")?;
    let snapshot = get(manifest, "/snapshot_id")?;
    let inputs = list(manifest, "/inputs")?;
    if report["artifact"]["status"] == "unavailable" {
        require(
            report["artifact"]["verification"]["artifact_integrity"] == "unavailable",
            "/artifact/verification/artifact_integrity",
            "unavailable artifact cannot claim available integrity",
        )?;
    }

    // Manifest digests describe original identities, before presentation redaction.
    let input_ids: Vec<_> = inputs.iter().map(|input| &input["input_id"]).collect();
    let expected_snapshot = if inputs.is_empty() {
        Value::Null
    } else {
        Value::String(digest(
            serde_json::to_string(&input_ids)
                .map_err(|e| error("/report_metadata/evidence/snapshot_id", e.to_string()))?
                .as_bytes(),
        ))
    };
    require(
        *snapshot == expected_snapshot,
        "/report_metadata/evidence/snapshot_id",
        "snapshot identity digest mismatch",
    )?;
    if manifest["redaction"]["applied"] == true {
        require(
            manifest["query"]
                == serde_json::json!({
                    "command": "[REDACTED QUERY]", "filter": "[REDACTED FILTER]"
                }),
            "/report_metadata/evidence/query",
            "applied redaction requires query omission markers",
        )?;
        for input in inputs {
            require(
                input["file"] == "[REDACTED PATH]"
                    && input["coverage"]["file"] == "[REDACTED PATH]",
                "/report_metadata/evidence/inputs/file",
                "applied redaction requires path omission markers",
            )?;
        }
        summary
            .deferred_relations
            .push("unredacted_query_and_input_identity".into());
    } else {
        require(
            manifest["query_sha256"] == digest(get(manifest, "/query")?.to_string().as_bytes()),
            "/report_metadata/evidence/query_sha256",
            "query identity digest mismatch",
        )?;
        for input in inputs {
            let identity = serde_json::json!([input["file"], input["sha256"]]);
            require(
                input["input_id"] == digest(identity.to_string().as_bytes()),
                "/report_metadata/evidence/inputs/input_id",
                "input identity digest mismatch",
            )?;
        }
    }
    let mut parsed_total = 0u64;
    let mut selected_total = 0u64;
    let mut unparsed = false;
    for input in inputs {
        let parsed = number(input, "/coverage/parsed_entries")?;
        let selected = number(input, "/selected_entries")?;
        require(
            selected <= parsed,
            "/report_metadata/evidence/inputs",
            "selected coverage exceeds parsed coverage",
        )?;
        require(
            input["bytes"] == input["coverage"]["input_bytes"]
                && input["sha256"] == input["coverage"]["snapshot_sha256"],
            "/report_metadata/evidence/inputs/coverage",
            "coverage byte identity differs from input",
        )?;
        parsed_total = parsed_total.checked_add(parsed).ok_or_else(|| {
            error(
                "/report_metadata/evidence/scope",
                "parsed coverage overflow",
            )
        })?;
        selected_total = selected_total.checked_add(selected).ok_or_else(|| {
            error(
                "/report_metadata/evidence/scope",
                "selected coverage overflow",
            )
        })?;
        unparsed |= number(input, "/coverage/nonempty_lines")? > 0 && parsed == 0;
    }
    let expected_status = if inputs.is_empty() {
        "not_applicable"
    } else if unparsed {
        "unparsed_input"
    } else if parsed_total == 0 {
        "empty_input"
    } else if selected_total == 0 {
        "zero_filter_matches"
    } else {
        "parsed"
    };
    require(
        number(manifest, "/scope/parsed_entries")? == parsed_total
            && number(manifest, "/scope/selected_entries")? == selected_total
            && manifest["scope"]["status"] == expected_status,
        "/report_metadata/evidence/scope",
        "manifest parse coverage totals or status differ",
    )?;
    if manifest["redaction"]["applied"] == true && report["artifact"]["status"] != "unavailable" {
        require(
            report["artifact"]["content"] == "redacted"
                && report["artifact"]["verification"]["source_and_rules"] == "unavailable",
            "/artifact/content",
            "applied redaction requires redacted persistence and declared verification losses",
        )?;
    }
    let progress = list(report, "/processing/inputs")?;
    require(
        progress.len() >= inputs.len(),
        "/processing/inputs",
        "missing declared input progress",
    )?;
    for (ordinal, item) in progress.iter().enumerate() {
        require(
            number(item, "/input_ordinal")? == ordinal as u64,
            "/processing/inputs",
            "input ordinals must follow declaration order",
        )?;
        if let Some(input) = inputs.get(ordinal) {
            require(
                item["consumed_bytes"] == input["bytes"]
                    && item["consumed_sha256"] == input["sha256"],
                "/processing/inputs",
                "consumed stream differs from manifest",
            )?;
        } else {
            require(
                item["capture"] == "unread",
                "/processing/inputs",
                "captured input missing from manifest",
            )?;
        }
    }
    if !report["processing"]["usage"]["records"].is_null() {
        require(
            number(report, "/processing/usage/records")? == parsed_total,
            "/processing/usage/records",
            "record usage differs from parsed coverage",
        )?;
    }
    if !report["processing"]["usage"]["input_bytes"].is_null() {
        let consumed = progress.iter().try_fold(0u64, |total, input| {
            total
                .checked_add(number(input, "/consumed_bytes")?)
                .ok_or_else(|| {
                    error(
                        "/processing/usage/input_bytes",
                        "consumed byte count overflow",
                    )
                })
        })?;
        require(
            number(report, "/processing/usage/input_bytes")? == consumed,
            "/processing/usage/input_bytes",
            "input byte usage differs from consumed inputs",
        )?;
    }
    if report["processing"]["status"] == "complete" {
        require(
            progress.len() == inputs.len() && progress.iter().all(|i| i["capture"] == "complete"),
            "/processing",
            "complete processing contains unread or prefix input",
        )?;
    }
    let occurrence = |v: &Value| -> Result<OccurrenceId, ContractError> {
        let id = OccurrenceId::from_value(v)?;
        require(
            v["snapshot_id"] == *snapshot,
            "/occurrence/snapshot_id",
            "different input snapshot",
        )?;
        let ordinal = usize::try_from(id.input_ordinal)
            .map_err(|_| error("/occurrence/input_ordinal", "ordinal too large"))?;
        let input = inputs.get(ordinal).ok_or_else(|| {
            error(
                "/occurrence/input_ordinal",
                "input outside captured manifest",
            )
        })?;
        require(
            number(input, "/selected_entries")? > 0,
            "/occurrence/evidence_ref",
            "source occurrence has no selected parse coverage",
        )?;
        let reference = get(v, "/evidence_ref")?;
        require(
            reference["input_id"] == input["input_id"],
            "/occurrence/evidence_ref/input_id",
            "reference belongs to another input",
        )?;
        require(
            number(reference, "/line")? > 0,
            "/occurrence/evidence_ref/line",
            "physical lines are one-based",
        )?;
        if reference["location_redacted"] != true {
            let identity = serde_json::json!([
                reference["input_id"],
                reference["line"],
                reference["row_path"],
                reference.get("expansion").unwrap_or(&Value::Null)
            ]);
            let expected = digest(identity.to_string().as_bytes());
            require(
                id.reference_id == expected,
                "/occurrence/evidence_ref/reference_id",
                "reference does not match source address",
            )?;
        }
        Ok(id)
    };
    let can_retrieve = |kind: &str, id: &str| -> bool {
        report["retrieval"]["status"] == "available"
            && report["retrieval"]["targets"]
                .as_array()
                .is_some_and(|targets| {
                    targets
                        .iter()
                        .any(|target| target["kind"] == kind && target["id"] == id)
                })
    };
    let scopes = index(list(report, "/scopes")?, "/scopes")?;
    for scope in scopes.values() {
        let ordinals = list(scope, "/input_ordinals")?;
        let selected = ordinals.iter().try_fold(0u64, |total, ordinal| {
            let input = ordinal
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .and_then(|v| inputs.get(v));
            total
                .checked_add(
                    input
                        .and_then(|v| v["selected_entries"].as_u64())
                        .unwrap_or(0),
                )
                .ok_or_else(|| error("/scopes/semantic_coverage", "scoped coverage overflow"))
        })?;
        for counter in [
            "relevant_records",
            "classified_records",
            "paired_events",
            "unmatched_events",
            "ambiguous_events",
            "rejected_events",
        ] {
            if let Some(count) = scope["semantic_coverage"][counter].as_u64() {
                require(
                    count <= selected,
                    "/scopes/semantic_coverage",
                    "semantic coverage exceeds selected parse coverage",
                )?;
            }
        }
        for (part, whole) in [
            ("paired_events", "classified_records"),
            ("paired_events", "relevant_records"),
            ("unmatched_events", "relevant_records"),
            ("ambiguous_events", "unmatched_events"),
            ("rejected_events", "unmatched_events"),
        ] {
            if let (Some(part_count), Some(whole_count)) = (
                scope["semantic_coverage"][part].as_u64(),
                scope["semantic_coverage"][whole].as_u64(),
            ) {
                require(
                    part_count <= whole_count,
                    "/scopes/semantic_coverage",
                    "semantic event count exceeds its containing population",
                )?;
            }
        }
        let mut seen = BTreeSet::new();
        for ordinal in ordinals {
            let ordinal = ordinal
                .as_u64()
                .ok_or_else(|| error("/scopes/input_ordinals", "invalid ordinal"))?;
            require(
                ordinal < progress.len() as u64 && seen.insert(ordinal),
                "/scopes/input_ordinals",
                "duplicate or undeclared input",
            )?;
            if scope["completeness"] == "complete" {
                require(
                    progress[ordinal as usize]["capture"] == "complete",
                    "/scopes/completeness",
                    "complete scope includes unread or prefix input",
                )?;
            }
        }
    }
    if report["processing"]["status"] != "complete" {
        for id in list(report, "/processing/stop/scope_ids")? {
            let id = id
                .as_str()
                .ok_or_else(|| error("/processing/stop/scope_ids", "invalid scope id"))?;
            let scope = scopes
                .get(id)
                .ok_or_else(|| error("/processing/stop/scope_ids", "unknown affected scope"))?;
            if report["processing"]["stop"]["stage"] != "artifact_write" {
                require(
                    scope["analysis_completion"] != "complete",
                    "/scopes/analysis_completion",
                    "affected scope claims completed analysis after processing cutoff",
                )?;
            }
        }
    }
    let populations = index(list(report, "/populations")?, "/populations")?;
    for population in populations.values() {
        let scope = scopes
            .get(text(population, "/scope_id")?)
            .ok_or_else(|| error("/populations/scope_id", "unknown scope"))?;
        require(
            population["count"] == population["membership"]["count"],
            "/populations/membership/count",
            "population count differs from membership count",
        )?;
        if population["completeness"] == "complete" {
            require(
                scope["completeness"] == "complete",
                "/populations/completeness",
                "complete population has incomplete scope",
            )?;
        }
    }
    let mut findings = index(list(report, "/findings")?, "/findings")?;
    let artifact: Option<Value> = artifact_bytes
        .map(serde_json::from_slice)
        .transpose()
        .map_err(|e| error("/artifact", e.to_string()))?;
    let mut retained = BTreeMap::new();
    let mut population_samples: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut original_captures: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
    if let (Some(bytes), Some(artifact)) = (artifact_bytes, artifact.as_ref()) {
        require(
            number(artifact, "/contract_version")? == u64::from(ARTIFACT_VERSION),
            "/artifact/contract_version",
            "unsupported artifact version",
        )?;
        require(
            report["artifact"]["stored_sha256"] == digest(bytes),
            "/artifact/stored_sha256",
            "stored artifact digest mismatch",
        )?;
        for key in [
            "report_metadata",
            "processing",
            "scopes",
            "populations",
            "assessments",
        ] {
            require(
                report[key] == artifact[key],
                "/artifact",
                "artifact analysis identity or scope differs",
            )?;
        }
        require(
            report["artifact"]["content"] == artifact["content"],
            "/artifact/content",
            "redaction content differs",
        )?;
        require(
            report["artifact"]["verification"] == artifact["verification"],
            "/artifact/verification",
            "displayed verification differs from retained guarantees",
        )?;
        require(
            report["artifact"]["retention"] == artifact["retention"],
            "/artifact/retention",
            "displayed retention differs from retained policy",
        )?;
        if manifest["redaction"]["applied"] == true {
            require(
                artifact["effective_profile_omitted"] == true
                    && artifact["effective_profile"].is_null(),
                "/artifact/effective_profile",
                "applied redaction cannot persist original effective rules",
            )?;
        }
        let captures = list(artifact, "/captured_inputs")?;
        require(
            captures.len() == progress.len(),
            "/artifact/captured_inputs",
            "missing input capture state",
        )?;
        for (ordinal, capture) in captures.iter().enumerate() {
            require(
                number(capture, "/input_ordinal")? == ordinal as u64
                    && capture["capture"] == progress[ordinal]["capture"]
                    && capture["original_consumed_sha256"] == progress[ordinal]["consumed_sha256"],
                "/artifact/captured_inputs",
                "capture identity or extent differs",
            )?;
            if manifest["redaction"]["applied"] == true {
                require(
                    capture["data_omitted"] == true
                        && capture["data"].is_null()
                        && capture["stored_sha256"].is_null(),
                    "/artifact/captured_inputs",
                    "applied redaction must omit captured data regardless of claimed digests",
                )?;
            }
            if capture["data_omitted"] == false {
                let bytes = match text(capture, "/encoding")? {
                    "utf8" => get(capture, "/data")?
                        .as_str()
                        .ok_or_else(|| {
                            error("/artifact/captured_inputs/data", "expected UTF-8 text")
                        })?
                        .as_bytes()
                        .to_vec(),
                    "bytes" => list(capture, "/data")?
                        .iter()
                        .map(|v| {
                            v.as_u64()
                                .and_then(|v| u8::try_from(v).ok())
                                .ok_or_else(|| {
                                    error("/artifact/captured_inputs/data", "expected byte")
                                })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => {
                        return Err(error(
                            "/artifact/captured_inputs/encoding",
                            "unsupported encoding",
                        ));
                    }
                };
                let stored_digest = digest(&bytes);
                require(
                    capture["stored_sha256"] == stored_digest,
                    "/artifact/captured_inputs/stored_sha256",
                    "retained byte digest mismatch",
                )?;
                if capture["original_consumed_sha256"] == stored_digest {
                    require(
                        bytes.len() as u64 == number(&progress[ordinal], "/consumed_bytes")?,
                        "/artifact/captured_inputs/data",
                        "original byte count mismatch",
                    )?;
                    original_captures.insert(ordinal as u64, bytes);
                } else {
                    require(
                        artifact["content"] == "redacted",
                        "/artifact/captured_inputs/data",
                        "original capture differs from consumed bytes",
                    )?;
                }
            } else {
                require(
                    artifact["content"] == "redacted" || capture["capture"] == "unread",
                    "/artifact/captured_inputs/data_omitted",
                    "original captured input was omitted",
                )?;
            }
        }
        let mut retained_rows: BTreeMap<u64, BTreeMap<u64, BTreeSet<String>>> = BTreeMap::new();
        for record in list(artifact, "/records")? {
            if manifest["redaction"]["applied"] == true {
                require(
                    record["data_omitted"] == true
                        && record["raw_text"].is_null()
                        && record["message"].is_null()
                        && record["fields"]
                            .as_object()
                            .is_some_and(|fields| fields.is_empty()),
                    "/artifact/records",
                    "applied redaction must omit retained record payloads",
                )?;
            }
            let id = occurrence(get(record, "/occurrence")?)?;
            let source = get(record, "/occurrence/evidence_ref")?;
            let line = number(source, "/line")?;
            let rows = retained_rows
                .entry(id.input_ordinal)
                .or_default()
                .entry(line)
                .or_default();
            if !source["row_path"].is_null() {
                rows.insert(source["row_path"].to_string());
            }
            require(
                retained.insert(id, record).is_none(),
                "/artifact/records",
                "duplicate retained occurrence",
            )?;
        }
        for (ordinal, lines) in retained_rows {
            let selected = number(&inputs[ordinal as usize], "/selected_entries")?;
            let represented = lines.values().try_fold(0u64, |count, rows| {
                count
                    .checked_add(rows.len().max(1) as u64)
                    .ok_or_else(|| error("/artifact/records", "retained source count overflow"))
            })?;
            require(
                represented <= selected,
                "/artifact/records",
                "retained source records exceed selected parse coverage",
            )?;
        }
        let saved_findings = index(list(artifact, "/findings")?, "/artifact/findings")?;
        for (id, displayed) in &findings {
            let saved = saved_findings.get(id).ok_or_else(|| {
                error(
                    "/artifact/findings",
                    "displayed finding is absent from artifact",
                )
            })?;
            for key in [
                "kind",
                "details",
                "scope_id",
                "claim",
                "author",
                "limitations",
                "verification",
            ] {
                require(
                    displayed[key] == saved[key],
                    "/artifact/findings",
                    "displayed fact or material qualification differs",
                )?;
            }
            for excerpt in list(displayed, "/evidence")? {
                let source = get(excerpt, "/occurrence")?;
                let originals: Vec<_> = list(saved, "/evidence")?
                    .iter()
                    .filter(|v| v["occurrence"] == *source)
                    .collect();
                require(
                    !originals.is_empty(),
                    "/findings/evidence",
                    "displayed witness not in retained finding",
                )?;
                let displayed_text = text(excerpt, "/text")?;
                let omitted = number(excerpt, "/omitted_characters")?;
                let mut matched = false;
                let mut overflow = false;
                for original in originals {
                    let original_text = text(original, "/text")?;
                    if !original_text.starts_with(displayed_text)
                        || excerpt["verification"] != original["verification"]
                    {
                        continue;
                    }
                    let clipped =
                        (original_text.chars().count() - displayed_text.chars().count()) as u64;
                    match number(original, "/omitted_characters")?.checked_add(clipped) {
                        Some(expected) if omitted == expected => {
                            matched = true;
                            break;
                        }
                        None => overflow = true,
                        _ => {}
                    }
                }
                require(
                    matched,
                    "/findings/evidence",
                    if overflow {
                        "excerpt omission count overflow"
                    } else {
                        "invalid excerpt projection or verification"
                    },
                )?;
            }
        }
        // Validate displayed and retained bundles separately: presentation may clip
        // excerpts, but cannot change essential support or material qualifications.
        let displayed_findings = findings.clone();
        findings = saved_findings;
        for displayed in displayed_findings.values() {
            check_bundle(displayed, &occurrence)?;
        }
        let mut membership_ids = BTreeSet::new();
        for (i, membership) in list(artifact, "/memberships")?.iter().enumerate() {
            let id = text(membership, "/population_id")?;
            require(
                membership_ids.insert(id),
                "/artifact/memberships",
                "duplicate population membership",
            )?;
            let population = populations.get(id).ok_or_else(|| {
                error("/artifact/memberships/population_id", "unknown population")
            })?;
            let members = list(membership, "/members")?;
            require(
                number(population, "/count")? == members.len() as u64,
                "/populations/count",
                "count does not equal retained membership",
            )?;
            require(
                population["membership"]["collection"] == format!("/memberships/{i}/members"),
                "/populations/membership/collection",
                "membership pointer differs",
            )?;
            require(
                population["membership"]["sha256"]
                    == digest(
                        serde_json::to_string(members)
                            .map_err(|e| error("/memberships", e.to_string()))?
                            .as_bytes(),
                    ),
                "/populations/membership/sha256",
                "membership digest mismatch",
            )?;
            let scope = scopes
                .get(text(population, "/scope_id")?)
                .ok_or_else(|| error("/populations/scope_id", "unknown scope"))?;
            let entity = text(population, "/entity")?;
            let mut keys = BTreeSet::new();
            let mut contributing = BTreeSet::new();
            for member in members {
                let key = if member["kind"] == "record" {
                    require(
                        matches!(
                            entity,
                            "physical_records" | "normalized_records" | "extraction_rows"
                        ),
                        "/memberships/members/kind",
                        "record member in non-record population",
                    )?;
                    let source = get(member, "/occurrence")?;
                    let id = occurrence(source)?;
                    let record = retained.get(&id).ok_or_else(|| {
                        error("/memberships/members", "member has no retained source")
                    })?;
                    require(
                        record["entity"] == entity
                            && list(scope, "/input_ordinals")?.contains(&source["input_ordinal"]),
                        "/memberships/members",
                        "record entity or source scope differs",
                    )?;
                    let r = get(source, "/evidence_ref")?;
                    if entity == "physical_records" {
                        require(
                            r["row_path"].is_null() && r.get("expansion").is_none(),
                            "/memberships/members",
                            "derived row counted as physical record",
                        )?;
                    }
                    if entity == "extraction_rows" {
                        require(
                            r.get("expansion").is_some(),
                            "/memberships/members",
                            "extraction row lacks expansion identity",
                        )?;
                    }
                    serde_json::to_string(&id)
                        .map_err(|e| error("/memberships/members", e.to_string()))?
                } else {
                    let expected_kind = match entity {
                        "events" => "event",
                        "attempts" => "attempt",
                        "operations" => "operation",
                        "resources" => "resource",
                        _ => {
                            return Err(error(
                                "/memberships/members/kind",
                                "non-record member in record population",
                            ));
                        }
                    };
                    require(
                        member["kind"] == expected_kind,
                        "/memberships/members/kind",
                        "member kind differs from population entity",
                    )?;
                    let mut identity = BTreeMap::new();
                    for field in list(member, "/identity")? {
                        let name = text(field, "/field")?;
                        require(
                            identity.insert(name, text(field, "/value")?).is_none(),
                            "/memberships/members/identity",
                            "duplicate identity field",
                        )?;
                    }
                    for field in list(population, "/identity_fields")? {
                        require(
                            field
                                .as_str()
                                .is_some_and(|name| identity.contains_key(name)),
                            "/memberships/members/identity",
                            "declared identity field missing",
                        )?;
                    }
                    for field in list(scope, "/correlation_scope")? {
                        require(
                            identity.get(text(field, "/field")?).copied()
                                == Some(text(field, "/value")?),
                            "/memberships/members/identity",
                            "member correlation scope differs",
                        )?;
                    }
                    let mut sources = BTreeSet::new();
                    for source in list(member, "/source_occurrences")? {
                        let id = occurrence(source)?;
                        require(
                            retained.contains_key(&id)
                                && list(scope, "/input_ordinals")?
                                    .contains(&source["input_ordinal"]),
                            "/memberships/members",
                            "member source missing or outside scope",
                        )?;
                        require(
                            sources.insert(id),
                            "/memberships/members/source_occurrences",
                            "duplicate member source",
                        )?;
                    }
                    for measurement_id in list(member, "/measurement_ids")? {
                        let id = measurement_id.as_str().ok_or_else(|| {
                            error(
                                "/memberships/members/measurement_ids",
                                "invalid measurement id",
                            )
                        })?;
                        let measurement = findings.get(id).ok_or_else(|| {
                            error(
                                "/memberships/members/measurement_ids",
                                "measurement missing",
                            )
                        })?;
                        require(
                            measurement["kind"] == "measurement"
                                && measurement["scope_id"] == population["scope_id"],
                            "/memberships/members/measurement_ids",
                            "measurement kind or scope differs",
                        )?;
                        for boundary in ["start", "end"] {
                            let source = get(
                                measurement,
                                &format!("/details/boundaries/{boundary}/occurrence"),
                            )?;
                            require(
                                sources.contains(&occurrence(source)?),
                                "/memberships/members/measurement_ids",
                                "measurement boundaries not owned by member",
                            )?;
                        }
                        require(
                            contributing.insert(id),
                            "/memberships/members/measurement_ids",
                            "duplicate contributing measurement",
                        )?;
                    }
                    if entity == "resources" {
                        let mut declared = BTreeMap::new();
                        for field in list(population, "/identity_fields")? {
                            let name = field.as_str().ok_or_else(|| {
                                error("/populations/identity_fields", "invalid identity field")
                            })?;
                            declared.insert(name, identity[name]);
                        }
                        for field in list(scope, "/correlation_scope")? {
                            let name = text(field, "/field")?;
                            declared.insert(name, identity[name]);
                        }
                        serde_json::to_string(&declared)
                            .map_err(|e| error("/memberships/members/identity", e.to_string()))?
                    } else {
                        format!("{expected_kind}:{}", text(member, "/id")?)
                    }
                };
                require(
                    keys.insert(key),
                    "/memberships/members",
                    "duplicate population member or distinct resource identity",
                )?;
            }
            population_samples.insert(id, contributing);
        }
        require(
            membership_ids.len() == populations.len(),
            "/artifact/memberships",
            "missing exhaustive population membership",
        )?;
        for sequence in index(list(artifact, "/sequences")?, "/artifact/sequences")?.values() {
            let scope = scopes
                .get(text(sequence, "/scope_id")?)
                .ok_or_else(|| error("/artifact/sequences/scope_id", "unknown sequence scope"))?;
            if sequence["completeness"] == "complete" {
                require(
                    scope["completeness"] == "complete"
                        && scope["analysis_completion"] == "complete",
                    "/artifact/sequences/completeness",
                    "complete sequence has incomplete analysis",
                )?;
            }
            let mut seen = BTreeSet::new();
            let mut previous = None;
            for event in list(sequence, "/events")? {
                let source = get(event, "/occurrence")?;
                let id = occurrence(source)?;
                let record = retained.get(&id).ok_or_else(|| {
                    error("/artifact/sequences/events", "event source not retained")
                })?;
                require(
                    seen.insert(id.clone())
                        && list(scope, "/input_ordinals")?.contains(&source["input_ordinal"]),
                    "/artifact/sequences/events",
                    "duplicate event or event outside scope",
                )?;
                require(
                    event["timestamp"] == record["timestamp"],
                    "/artifact/sequences/events/timestamp",
                    "event timestamp differs from source",
                )?;
                let timestamp = event["timestamp"]
                    .as_str()
                    .map(DateTime::parse_from_rfc3339)
                    .transpose()
                    .map_err(|e| error("/artifact/sequences/events/timestamp", e.to_string()))?;
                let key = (
                    timestamp.is_none(),
                    timestamp,
                    id.input_ordinal,
                    number(event, "/record_ordinal")?,
                );
                require(
                    previous.as_ref().is_none_or(|previous| previous <= &key),
                    "/artifact/sequences/events",
                    "event sequence is not deterministically ordered",
                )?;
                previous = Some(key);
            }
        }
        if artifact["effective_profile_omitted"] == false {
            require(
                digest(get(artifact, "/effective_profile")?.to_string().as_bytes())
                    == manifest["profile_sha256"],
                "/artifact/effective_profile",
                "effective profile digest mismatch",
            )?;
        }
    }
    if let Some(artifact) = artifact.as_ref() {
        let profile_available = artifact["effective_profile_omitted"] == false;
        let mut indexed_sources: BTreeMap<u64, Vec<&str>> = BTreeMap::new();
        for record in retained.values() {
            if record["verification"]["source_and_rules"] == "available" {
                let source = get(record, "/occurrence")?;
                let ordinal = number(source, "/input_ordinal")?;
                let bytes = original_captures.get(&ordinal).ok_or_else(|| {
                    error(
                        "/artifact/records/verification",
                        "original source bytes not retained",
                    )
                })?;
                require(
                    profile_available
                        && source["evidence_ref"]["location_redacted"] != true
                        && record["data_omitted"] == false,
                    "/artifact/records/verification",
                    "source or rule verification prerequisites lost",
                )?;
                let raw = text(record, "/raw_text")?;
                let source_lines = match indexed_sources.entry(ordinal) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        let source_text = std::str::from_utf8(bytes)
                            .map_err(|e| error("/artifact/records/raw_text", e.to_string()))?;
                        entry.insert(source_text.lines().collect())
                    }
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                };
                let line = usize::try_from(number(source, "/evidence_ref/line")?)
                    .map_err(|_| error("/artifact/records/raw_text", "line too large"))?;
                require(
                    raw_lines_match(source_lines, line, raw),
                    "/artifact/records/raw_text",
                    "raw record differs from retained input",
                )?;
            }
        }
        if artifact["verification"]["source_and_rules"] == "available" {
            require(
                profile_available
                    && original_captures.len() == inputs.len()
                    && retained
                        .values()
                        .all(|r| r["verification"]["source_and_rules"] == "available"),
                "/artifact/verification",
                "artifact claims lost source or rule verification",
            )?;
        }
    }
    for finding in findings.values() {
        if report["artifact"]["status"] == "unavailable" {
            require(
                finding["verification"]["artifact_integrity"] == "unavailable",
                "/findings/verification/artifact_integrity",
                "finding claims integrity of unavailable artifact",
            )?;
        }
        check_bundle(finding, &occurrence)?;
        let scope = scopes
            .get(text(finding, "/scope_id")?)
            .ok_or_else(|| error("/findings/scope_id", "unknown scope"))?;
        if finding["verification"]["source_and_rules"] == "available" {
            require(
                manifest["redaction"]["applied"] != true,
                "/findings/verification",
                "finding claims source verification after applied redaction",
            )?;
            if let Some(artifact) = artifact.as_ref() {
                require(
                    artifact["effective_profile_omitted"] == false,
                    "/findings/verification",
                    "finding claims unavailable effective rules",
                )?;
                let mut dependencies = Vec::new();
                if finding["kind"] == "calculated_fact" {
                    let population = populations
                        .get(text(finding, "/details/population_id")?)
                        .ok_or_else(|| {
                            error("/findings/verification", "unknown finding population")
                        })?;
                    for member in list(artifact, text(population, "/membership/collection")?)? {
                        if member["kind"] == "record" {
                            dependencies.push(get(member, "/occurrence")?);
                        } else {
                            dependencies.extend(list(member, "/source_occurrences")?);
                        }
                    }
                    if finding["details"]["calculation"] == "distribution" {
                        for id in list(finding, "/details/measurement_ids")? {
                            if let Some(measurement) = id.as_str().and_then(|id| findings.get(id)) {
                                require(
                                    measurement["verification"]["source_and_rules"] == "available",
                                    "/findings/verification",
                                    "calculation claims unavailable sample source verification",
                                )?;
                            }
                        }
                    }
                } else {
                    for excerpt in list(finding, "/evidence")? {
                        dependencies.push(get(excerpt, "/occurrence")?);
                    }
                }
                for source in &dependencies {
                    let id = occurrence(source)?;
                    require(
                        original_captures.contains_key(&id.input_ordinal)
                            && retained.get(&id).is_some_and(|record| {
                                record["verification"]["source_and_rules"] == "available"
                            }),
                        "/findings/verification",
                        "finding claims unavailable dependent source verification",
                    )?;
                }
                if dependencies.is_empty() {
                    require(
                        list(scope, "/input_ordinals")?.iter().all(|ordinal| {
                            ordinal
                                .as_u64()
                                .is_some_and(|id| original_captures.contains_key(&id))
                        }),
                        "/findings/verification",
                        "absence claim lacks retained scoped captures",
                    )?;
                }
            }
        }
        for excerpt in list(finding, "/evidence")? {
            if report["artifact"]["status"] == "unavailable" {
                require(
                    excerpt["verification"]["artifact_integrity"] == "unavailable",
                    "/findings/evidence/verification/artifact_integrity",
                    "excerpt claims integrity of unavailable artifact",
                )?;
            }
            let source = get(excerpt, "/occurrence")?;
            let id = occurrence(source)?;
            require(
                list(scope, "/input_ordinals")?.contains(&source["input_ordinal"]),
                "/findings/evidence",
                "witness outside finding scope",
            )?;
            if manifest["redaction"]["applied"] == true {
                require(
                    excerpt["verification"]["source_and_rules"] == "unavailable",
                    "/findings/evidence/verification",
                    "excerpt claims source verification after applied redaction",
                )?;
                // Without original inputs there is no way to prove arbitrary text was redacted.
                let marker = "[REDACTED SOURCE]";
                let excerpt_text = text(excerpt, "/text")?;
                require(
                    marker.starts_with(excerpt_text)
                        && number(excerpt, "/omitted_characters")?
                            == marker
                                .chars()
                                .count()
                                .saturating_sub(excerpt_text.chars().count())
                                as u64,
                    "/findings/evidence/text",
                    "applied redaction requires the source omission marker",
                )?;
            }
            if source["evidence_ref"]["location_redacted"] == true {
                require(
                    excerpt["verification"]["source_and_rules"] == "unavailable",
                    "/findings/evidence/verification",
                    "redacted location claims source verification",
                )?;
            }
            if artifact.is_some() {
                let record = retained
                    .get(&id)
                    .ok_or_else(|| error("/findings/evidence", "witness not retained"))?;
                let excerpt_text = text(excerpt, "/text")?;
                let omitted = number(excerpt, "/omitted_characters")?;
                require(
                    (record["message"].is_null()
                        && record["raw_text"].is_null()
                        && excerpt["verification"]["source_and_rules"] == "unavailable")
                        || ["message", "raw_text"].iter().any(|field| {
                            record[*field].as_str().is_some_and(|original| {
                                original.starts_with(excerpt_text)
                                    && (original
                                        .chars()
                                        .count()
                                        .saturating_sub(excerpt_text.chars().count())
                                        as u64)
                                        == omitted
                            })
                        }),
                    "/findings/evidence/text",
                    "excerpt differs from retained record",
                )?;
                if excerpt["verification"]["source_and_rules"] == "available" {
                    require(
                        record["verification"]["source_and_rules"] == "available",
                        "/findings/evidence/verification",
                        "excerpt claims unavailable source verification",
                    )?;
                }
                if finding["verification"]["source_and_rules"] == "available" {
                    require(
                        excerpt["verification"]["source_and_rules"] == "available",
                        "/findings/verification",
                        "finding claims unavailable witness verification",
                    )?;
                }
            }
        }
        let details = get(finding, "/details")?;
        match text(finding, "/kind")? {
            "measurement" => {
                require(
                    details["profile_sha256"] == manifest["profile_sha256"],
                    "/findings/details/profile_sha256",
                    "measurement uses another profile",
                )?;
                let start = get(details, "/boundaries/start")?;
                let end = get(details, "/boundaries/end")?;
                require(
                    occurrence(get(start, "/occurrence")?)?
                        != occurrence(get(end, "/occurrence")?)?,
                    "/findings/details/boundaries",
                    "measurement requires distinct boundary occurrences",
                )?;
                let start_time =
                    DateTime::parse_from_rfc3339(text(start, "/timestamp")?).map_err(|e| {
                        error(
                            "/findings/details/boundaries/start/timestamp",
                            e.to_string(),
                        )
                    })?;
                let end_time =
                    DateTime::parse_from_rfc3339(text(end, "/timestamp")?).map_err(|e| {
                        error("/findings/details/boundaries/end/timestamp", e.to_string())
                    })?;
                let milliseconds = end_time
                    .signed_duration_since(start_time)
                    .num_milliseconds();
                require(
                    end_time >= start_time
                        && milliseconds >= 0
                        && details["value"].as_i64() == Some(milliseconds),
                    "/findings/details/value",
                    "duration differs from observed boundaries",
                )?;
                for boundary in [start, end] {
                    let source = get(boundary, "/occurrence")?;
                    let id = occurrence(source)?;
                    require(
                        list(scope, "/input_ordinals")?.contains(&source["input_ordinal"]),
                        "/findings/details/boundaries",
                        "boundary outside finding scope",
                    )?;
                    if let Some(record) = retained.get(&id) {
                        require(
                            boundary["timestamp"] == record["timestamp"]
                                && record["timestamp_year_source"] == "source"
                                && record["timestamp_offset_source"] == "source",
                            "/findings/details/boundaries",
                            "boundary differs from retained source timestamp or provenance",
                        )?;
                    } else {
                        require(
                            artifact.is_none(),
                            "/findings/details/boundaries",
                            "boundary not retained",
                        )?;
                    }
                }
            }
            "calculated_fact" => {
                let population = populations
                    .get(text(details, "/population_id")?)
                    .ok_or_else(|| {
                        error("/findings/details/population_id", "unknown population")
                    })?;
                require(
                    population["scope_id"] == finding["scope_id"],
                    "/findings/details/population_id",
                    "population belongs to another scope",
                )?;
                match text(details, "/calculation")? {
                    "count" => {
                        require(
                            details["value"] == population["count"],
                            "/findings/details/value",
                            "count differs from population",
                        )?;
                        let expected_unit = match text(population, "/entity")? {
                            "physical_records" | "normalized_records" => "records",
                            "extraction_rows" => "rows",
                            other => other,
                        };
                        require(
                            details["unit"] == expected_unit,
                            "/findings/details/unit",
                            "unit differs from counted entity",
                        )?;
                    }
                    "distribution" => {
                        let mut samples = Vec::new();
                        let mut seen = BTreeSet::new();
                        let mut deferred = false;
                        for id in list(details, "/measurement_ids")? {
                            let id = id.as_str().ok_or_else(|| {
                                error(
                                    "/findings/details/measurement_ids",
                                    "invalid measurement id",
                                )
                            })?;
                            require(
                                seen.insert(id),
                                "/findings/details/measurement_ids",
                                "duplicate measurement sample",
                            )?;
                            let Some(sample) = findings.get(id) else {
                                require(
                                    artifact.is_none() && can_retrieve("finding", id),
                                    "/findings/details/measurement_ids",
                                    "measurement must be inline, retained or explicitly retrievable",
                                )?;
                                summary.deferred_relations.push(format!("measurement:{id}"));
                                deferred = true;
                                continue;
                            };
                            require(
                                sample["kind"] == "measurement"
                                    && sample["scope_id"] == finding["scope_id"],
                                "/findings/details/measurement_ids",
                                "sample has incompatible kind or scope",
                            )?;
                            samples.push(number(sample, "/details/value")?);
                        }
                        if let Some(contributing) =
                            population_samples.get(text(details, "/population_id")?)
                        {
                            require(
                                &seen == contributing,
                                "/findings/details/measurement_ids",
                                "samples differ from population's contributing measurements",
                            )?;
                        }
                        if deferred {
                            require(
                                seen.len() as u64 == number(details, "/sample_count")?
                                    && details["sample_count"] == population["count"],
                                "/findings/details/sample_count",
                                "distribution cardinality differs",
                            )?;
                            continue;
                        }
                        samples.sort_unstable();
                        require(
                            !samples.is_empty()
                                && samples.len() as u64 == number(details, "/sample_count")?
                                && details["sample_count"] == population["count"],
                            "/findings/details/sample_count",
                            "distribution population differs from sample count",
                        )?;
                        check_distribution_value(details, &samples)?;
                    }
                    _ => {
                        return Err(error(
                            "/findings/details/calculation",
                            "unsupported calculation",
                        ));
                    }
                }
            }
            "observation" | "hypothesis" | "contrary_evidence" | "unknown" => {
                for source in list(details, "/supporting_occurrences")? {
                    let id = occurrence(source)?;
                    require(
                        list(scope, "/input_ordinals")?.contains(&source["input_ordinal"]),
                        "/findings/details/supporting_occurrences",
                        "support outside finding scope",
                    )?;
                    if artifact.is_some() {
                        require(
                            retained.contains_key(&id),
                            "/findings/details/supporting_occurrences",
                            "support not retained",
                        )?;
                    }
                }
                if finding["kind"] == "contrary_evidence" {
                    let against = text(details, "/against_finding_id")?;
                    require(
                        finding["id"] != against,
                        "/findings/details/against_finding_id",
                        "contrary evidence target is self-referential",
                    )?;
                    if let Some(target) = findings.get(against) {
                        require(
                            target["scope_id"] == finding["scope_id"],
                            "/findings/details/against_finding_id",
                            "contrary evidence target belongs to another scope",
                        )?;
                    } else {
                        require(
                            artifact.is_none() && can_retrieve("finding", against),
                            "/findings/details/against_finding_id",
                            "contrary evidence target missing and not retrievable",
                        )?;
                        summary
                            .deferred_relations
                            .push(format!("contrary_target:{against}"));
                    }
                }
            }
            _ => return Err(error("/findings/kind", "unsupported finding kind")),
        }
    }
    let mut assessment_keys = BTreeSet::new();
    for assessment in list(report, "/assessments")? {
        require(
            assessment_keys.insert((text(assessment, "/goal")?, text(assessment, "/scope_id")?)),
            "/assessments",
            "duplicate assessment for goal and scope",
        )?;
        let scope = scopes
            .get(text(assessment, "/scope_id")?)
            .ok_or_else(|| error("/assessments/scope_id", "unknown scope"))?;
        if assessment["status"] == "supported" {
            require(
                !list(assessment, "/finding_ids")?.is_empty(),
                "/assessments/finding_ids",
                "supported assessment requires a finding",
            )?;
        }
        if assessment["status"] == "supported" && scope["extent"] == "declared_input" {
            require(
                scope["completeness"] == "complete" && scope["analysis_completion"] == "complete",
                "/assessments/status",
                "partial scope cannot support a full-input goal",
            )?;
        }
        let mut positive = false;
        let mut deferred = false;
        for id in list(assessment, "/finding_ids")? {
            let id = id
                .as_str()
                .ok_or_else(|| error("/assessments/finding_ids", "invalid finding id"))?;
            let Some(finding) = findings.get(id) else {
                require(
                    artifact.is_none() && can_retrieve("finding", id),
                    "/assessments/finding_ids",
                    "finding must be inline, retained or explicitly retrievable",
                )?;
                deferred = true;
                summary.deferred_relations.push(format!("finding:{id}"));
                continue;
            };
            positive |= matches!(
                finding["kind"].as_str(),
                Some("observation" | "measurement" | "calculated_fact")
            );
            require(
                finding["scope_id"] == assessment["scope_id"],
                "/assessments/finding_ids",
                "finding belongs to another scope",
            )?;
        }
        if assessment["status"] == "supported" {
            require(
                positive || deferred,
                "/assessments/finding_ids",
                "supported assessment requires a positive finding",
            )?;
            if !positive && deferred {
                summary.deferred_relations.push(format!(
                    "supported_assessment:{}",
                    text(assessment, "/goal")?
                ));
            }
            for ordinal in list(scope, "/input_ordinals")? {
                if let Some(input) = ordinal.as_u64().and_then(|v| inputs.get(v as usize)) {
                    require(
                        !(number(input, "/coverage/nonempty_lines")? > 0
                            && number(input, "/coverage/parsed_entries")? == 0),
                        "/assessments/status",
                        "unparsed input cannot support a scoped assessment",
                    )?;
                }
            }
        }
    }
    require(
        number(report, "/presentation/displayed_findings")?
            == list(report, "/findings")?.len() as u64,
        "/presentation/displayed_findings",
        "displayed count differs from findings",
    )?;
    if let (Some(total), Some(omitted)) = (
        report["presentation"]["total_findings"].as_u64(),
        report["presentation"]["omitted_findings"].as_u64(),
    ) {
        let displayed = number(report, "/presentation/displayed_findings")?;
        require(
            displayed.checked_add(omitted) == Some(total),
            "/presentation",
            "finding totals do not reconcile",
        )?;
        if artifact.is_some() {
            require(
                total == findings.len() as u64,
                "/presentation/total_findings",
                "total differs from retained findings",
            )?;
        }
    }
    let mut collection_paths = BTreeSet::new();
    let mut omitted_collection_items = false;
    for collection in list(report, "/presentation/collections")? {
        let path = text(collection, "/path")?;
        require(
            collection_paths.insert(path),
            "/presentation/collections",
            "duplicate presentation collection",
        )?;
        let total = number(collection, "/total")?;
        let prior = number(collection, "/prior")?;
        let displayed = number(collection, "/displayed")?;
        let remaining = number(collection, "/remaining")?;
        require(
            prior
                .checked_add(displayed)
                .and_then(|n| n.checked_add(remaining))
                == Some(total),
            "/presentation/collections",
            "collection counts do not reconcile",
        )?;
        require(
            list(report, path)?.len() as u64 == displayed,
            "/presentation/collections",
            "collection displayed count differs from report",
        )?;
        omitted_collection_items |= prior > 0 || remaining > 0;
        if path == "/findings" {
            require(
                report["presentation"]["total_findings"] == total
                    && report["presentation"]["displayed_findings"] == displayed
                    && report["presentation"]["omitted_findings"]
                        == prior
                            .checked_add(remaining)
                            .map(Value::from)
                            .unwrap_or(Value::Null),
                "/presentation/collections",
                "finding collection differs from top-level counts",
            )?;
        }
        if let Some(artifact) = artifact.as_ref() {
            require(
                list(artifact, path)?.len() as u64 == total,
                "/presentation/collections",
                "collection total differs from retained artifact",
            )?;
        }
    }
    let total_known = !report["presentation"]["total_findings"].is_null();
    require(
        total_known == !report["presentation"]["omitted_findings"].is_null(),
        "/presentation",
        "finding total and omission availability differ",
    )?;
    require(
        !total_known || collection_paths.contains("/findings"),
        "/presentation/collections",
        "known finding totals require a collection entry",
    )?;
    let has_omissions = omitted_collection_items
        || report["presentation"]["omitted_findings"]
            .as_u64()
            .is_some_and(|v| v > 0);
    let presentation = get(report, "/presentation")?;
    let displayed = number(presentation, "/displayed_findings")?;
    let all_empty = list(presentation, "/collections")?
        .iter()
        .all(|collection| collection["displayed"] == 0);
    let remaining = list(presentation, "/collections")?
        .iter()
        .any(|collection| collection["remaining"].as_u64().is_some_and(|v| v > 0));
    let mut size_limited = false;
    let mut over_budget = false;
    for (budget_key, size_key) in [
        ("budget_bytes", "serialized_bytes"),
        ("budget_characters", "serialized_characters"),
    ] {
        if let Some(budget) = presentation[budget_key].as_u64() {
            size_limited = true;
            let size = presentation[size_key]
                .as_u64()
                .ok_or_else(|| error("/presentation", "size budget requires serialized usage"))?;
            over_budget |= size > budget;
        }
    }
    require(
        presentation["budget_items"]
            .as_u64()
            .is_none_or(|budget| displayed <= budget),
        "/presentation/budget_items",
        "displayed findings exceed item budget",
    )?;
    let status = text(presentation, "/status")?;
    match status {
        "complete" => require(
            total_known && !has_omissions,
            "/presentation/status",
            "complete presentation has omitted or unknown findings",
        )?,
        "page" => {
            require(
                has_omissions,
                "/presentation/status",
                "page presentation has no omissions",
            )?;
            require(
                displayed > 0,
                "/presentation/status",
                "page presentation makes no progress",
            )?;
        }
        "item_limit_zero" => require(
            presentation["budget_items"] == 0 && displayed == 0 && all_empty && remaining,
            "/presentation/status",
            "item_limit_zero requires zero item budget and no displayed items with remaining items",
        )?,
        "oversized_item" => require(
            size_limited
                && !over_budget
                && presentation["budget_items"] != 0
                && displayed == 0
                && all_empty
                && remaining,
            "/presentation/status",
            "oversized_item requires a size budget and no displayed items with remaining items",
        )?,
        "mandatory_metadata_over_budget" => require(
            over_budget && displayed == 0 && all_empty,
            "/presentation/status",
            "mandatory metadata status requires exceeded size budget and no displayed items",
        )?,
        _ => {
            return Err(error(
                "/presentation/status",
                "unsupported presentation status",
            ));
        }
    }
    require(
        !over_budget || status == "mandatory_metadata_over_budget",
        "/presentation/status",
        "serialized usage exceeds budget without mandatory metadata status",
    )?;
    if !presentation["serialized_bytes"].is_null()
        || !presentation["serialized_characters"].is_null()
    {
        let compact = report.to_string();
        // Equivalent floating-point values may use shorter scientific notation.
        // Integer spellings retain all digits required by exact integer checks.
        let discount = numeric_spelling_discount(report);
        for (key, minimum) in [
            ("serialized_bytes", compact.len() - discount + 1),
            (
                "serialized_characters",
                compact.chars().count() - discount + 1,
            ),
        ] {
            require(
                presentation[key]
                    .as_u64()
                    .is_none_or(|usage| usage >= minimum as u64),
                "/presentation",
                "serialized usage is below minimum document size including newline",
            )?;
        }
    }
    if report["retrieval"]["status"] == "available" {
        require(
            report["artifact"]["status"] != "unavailable",
            "/retrieval/status",
            "retrieval advertised without retained artifact",
        )?;
    }
    Ok(summary)
}

fn numeric_spelling_discount(value: &Value) -> usize {
    match value {
        Value::Number(number) if number.is_f64() => {
            let decimal = number.to_string();
            let scientific = format!("{:e}", number.as_f64().unwrap());
            let integral = decimal.strip_suffix(".0").unwrap_or(&decimal);
            let shortest = decimal.len().min(scientific.len()).min(integral.len());
            decimal.len() - shortest
        }
        Value::Array(values) => values.iter().map(numeric_spelling_discount).sum(),
        Value::Object(values) => values.values().map(numeric_spelling_discount).sum(),
        _ => 0,
    }
}

fn check_bundle(
    finding: &Value,
    occurrence: &impl Fn(&Value) -> Result<OccurrenceId, ContractError>,
) -> Result<(), ContractError> {
    let witnesses = list(finding, "/evidence")?
        .iter()
        .map(|v| occurrence(get(v, "/occurrence")?))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let required = if finding["kind"] == "measurement" {
        vec![
            get(finding, "/details/boundaries/start/occurrence")?,
            get(finding, "/details/boundaries/end/occurrence")?,
        ]
    } else if matches!(
        finding["kind"].as_str(),
        Some("observation" | "hypothesis" | "contrary_evidence")
    ) {
        list(finding, "/details/supporting_occurrences")?
            .iter()
            .collect()
    } else {
        Vec::new()
    };
    for source in required {
        require(
            witnesses.contains(&occurrence(source)?),
            "/findings/evidence",
            "essential supporting witness is absent from finding bundle",
        )?;
    }
    Ok(())
}

fn check_distribution_value(details: &Value, samples: &[u64]) -> Result<(), ContractError> {
    require(
        !samples.is_empty(),
        "/findings/details/sample_count",
        "distribution has no samples",
    )?;
    let (matches, method) = match text(details, "/statistic")? {
        "sum" => {
            let sum = samples.iter().try_fold(0u64, |sum, value| {
                sum.checked_add(*value).ok_or_else(|| {
                    error(
                        "/findings/details/value",
                        "integer distribution sum overflow",
                    )
                })
            })?;
            (details["value"].as_u64() == Some(sum), "sum")
        }
        "mean" => (
            details["value"].as_f64()
                == Some(samples.iter().map(|v| *v as f64).sum::<f64>() / samples.len() as f64),
            "arithmetic_mean",
        ),
        "minimum" => (details["value"].as_u64() == Some(samples[0]), "minimum"),
        "maximum" => (
            details["value"].as_u64() == Some(samples[samples.len() - 1]),
            "maximum",
        ),
        name @ ("p50" | "p95" | "p99") => {
            let percentile: usize = name[1..]
                .parse()
                .map_err(|_| error("/findings/details/statistic", "invalid percentile"))?;
            let index =
                (samples.len() / 100) * percentile + (samples.len() % 100) * percentile / 100;
            (
                details["value"].as_u64() == Some(samples[index]),
                "sorted_index_floor_n_times_percentile_over_100",
            )
        }
        _ => {
            return Err(error(
                "/findings/details/statistic",
                "unsupported statistic",
            ));
        }
    };
    require(
        matches && details["method"] == method,
        "/findings/details",
        "distribution value or method differs",
    )
}

fn raw_lines_match(source_lines: &[&str], line: usize, raw: &str) -> bool {
    source_lines
        .get(line.saturating_sub(1)..)
        .is_some_and(|remaining| {
            remaining
                .iter()
                .copied()
                .take(raw.lines().count())
                .eq(raw.lines())
        })
}

#[cfg(test)]
mod tests {
    use super::{check_distribution_value, raw_lines_match};
    use serde_json::json;

    #[test]
    fn integer_statistics_do_not_round_large_values() {
        let exact = (1u64 << 53) + 1;
        for (statistic, method) in [
            ("sum", "sum"),
            ("minimum", "minimum"),
            ("maximum", "maximum"),
            ("p95", "sorted_index_floor_n_times_percentile_over_100"),
        ] {
            let mut details = json!({"statistic":statistic,"method":method,"value":exact});
            check_distribution_value(&details, &[exact]).unwrap();
            details["value"] = json!(exact - 1);
            assert!(check_distribution_value(&details, &[exact]).is_err());
        }
        let details = json!({"statistic":"sum","method":"sum","value":0});
        assert!(
            check_distribution_value(&details, &[u64::MAX, 1])
                .unwrap_err()
                .to_string()
                .contains("sum overflow")
        );
    }
    use std::io::Write;

    #[test]
    fn native_blank_continuation_lines_preserve_source_verification() {
        let input = "worker | 2026-10-07T10:00:00Z [INFO ] started\n\n\n";
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(input.as_bytes()).unwrap();
        let entries = crate::parser::parse_log_file(file.path().to_str().unwrap()).unwrap();
        assert_eq!(entries.len(), 1);
        let raw = &entries[0].raw_logline;
        assert!(raw.ends_with("\n\n"));
        let source_lines: Vec<_> = input.lines().collect();
        assert!(raw_lines_match(&source_lines, 1, raw));
        assert!(!raw_lines_match(
            &source_lines,
            1,
            &raw.replace("started", "invented")
        ));
    }
}
