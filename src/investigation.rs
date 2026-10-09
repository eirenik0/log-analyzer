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
        for record in list(artifact, "/records")? {
            let id = occurrence(get(record, "/occurrence")?)?;
            require(
                retained.insert(id, record).is_none(),
                "/artifact/records",
                "duplicate retained occurrence",
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
                let original = list(saved, "/evidence")?
                    .iter()
                    .find(|v| v["occurrence"] == *source)
                    .ok_or_else(|| {
                        error(
                            "/findings/evidence",
                            "displayed witness not in retained finding",
                        )
                    })?;
                let displayed_text = text(excerpt, "/text")?;
                let original_text = text(original, "/text")?;
                require(
                    original_text.starts_with(displayed_text)
                        && excerpt["verification"] == original["verification"]
                        && number(excerpt, "/omitted_characters")?
                            == number(original, "/omitted_characters")?
                                .checked_add(
                                    original_text
                                        .chars()
                                        .count()
                                        .saturating_sub(displayed_text.chars().count())
                                        as u64,
                                )
                                .ok_or_else(|| {
                                    error(
                                        "/findings/evidence/omitted_characters",
                                        "excerpt omission count overflow",
                                    )
                                })?,
                    "/findings/evidence",
                    "invalid excerpt projection or verification",
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
        for record in retained.values() {
            if record["verification"]["source_and_rules"] == "available" {
                let source = get(record, "/occurrence")?;
                let bytes = original_captures
                    .get(&number(source, "/input_ordinal")?)
                    .ok_or_else(|| {
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
                let source_text = std::str::from_utf8(bytes)
                    .map_err(|e| error("/artifact/records/raw_text", e.to_string()))?;
                let line = usize::try_from(number(source, "/evidence_ref/line")?)
                    .map_err(|_| error("/artifact/records/raw_text", "line too large"))?;
                let fragment = source_text
                    .lines()
                    .skip(line - 1)
                    .take(raw.lines().count())
                    .collect::<Vec<_>>()
                    .join("\n");
                require(
                    fragment == raw.trim_end_matches('\n'),
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
        check_bundle(finding, &occurrence)?;
        let scope = scopes
            .get(text(finding, "/scope_id")?)
            .ok_or_else(|| error("/findings/scope_id", "unknown scope"))?;
        for excerpt in list(finding, "/evidence")? {
            let source = get(excerpt, "/occurrence")?;
            let id = occurrence(source)?;
            require(
                list(scope, "/input_ordinals")?.contains(&source["input_ordinal"]),
                "/findings/evidence",
                "witness outside finding scope",
            )?;
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
                        let (expected, method) = match text(details, "/statistic")? {
                            "sum" => (samples.iter().map(|v| *v as f64).sum(), "sum"),
                            "mean" => (
                                samples.iter().map(|v| *v as f64).sum::<f64>()
                                    / samples.len() as f64,
                                "arithmetic_mean",
                            ),
                            "minimum" => (samples[0] as f64, "minimum"),
                            "maximum" => (samples[samples.len() - 1] as f64, "maximum"),
                            name @ ("p50" | "p95" | "p99") => {
                                let percentile: usize = name[1..].parse().map_err(|_| {
                                    error("/findings/details/statistic", "invalid percentile")
                                })?;
                                (
                                    samples[samples.len() * percentile / 100] as f64,
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
                            details["value"].as_f64() == Some(expected)
                                && details["method"] == method,
                            "/findings/details",
                            "distribution value or method differs",
                        )?;
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
                        findings.contains_key(against) && finding["id"] != against,
                        "/findings/details/against_finding_id",
                        "contrary evidence target missing or self-referential",
                    )?;
                }
            }
            _ => return Err(error("/findings/kind", "unsupported finding kind")),
        }
    }
    for assessment in list(report, "/assessments")? {
        let scope = scopes
            .get(text(assessment, "/scope_id")?)
            .ok_or_else(|| error("/assessments/scope_id", "unknown scope"))?;
        if assessment["status"] == "supported" && scope["extent"] == "declared_input" {
            require(
                scope["completeness"] == "complete" && scope["analysis_completion"] == "complete",
                "/assessments/status",
                "partial scope cannot support a full-input goal",
            )?;
        }
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
                summary.deferred_relations.push(format!("finding:{id}"));
                continue;
            };
            require(
                finding["scope_id"] == assessment["scope_id"],
                "/assessments/finding_ids",
                "finding belongs to another scope",
            )?;
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
    if report["retrieval"]["status"] == "available" {
        require(
            report["artifact"]["status"] != "unavailable",
            "/retrieval/status",
            "retrieval advertised without retained artifact",
        )?;
    }
    Ok(summary)
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
