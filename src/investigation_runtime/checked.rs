//! Publication boundary and a lossless projection of checked evidence, not a verifier.
use super::Result;
use crate::investigation::{OccurrenceId, validate_relations};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) struct CheckedResult(Value);

/// Store the checked projection on retained findings before artifact serialization.
pub(super) fn attach(retained: &mut Value, artifact_available: bool) {
    let records = retained["records"].clone();
    let scopes = retained["scopes"].clone();
    let populations = retained["populations"].clone();
    project(
        &mut retained["findings"],
        &records,
        &scopes,
        &populations,
        artifact_available,
    );
}

pub(crate) fn project(
    findings: &mut Value,
    records: &Value,
    scopes: &Value,
    populations: &Value,
    artifact_available: bool,
) {
    let records: BTreeMap<_, _> = records
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|record| {
            OccurrenceId::from_value(&record["occurrence"])
                .ok()
                .map(|id| (id, record))
        })
        .collect();
    let scopes: BTreeMap<_, _> = scopes
        .as_array()
        .into_iter()
        .flatten()
        .map(|scope| (scope["id"].clone().to_string(), scope))
        .collect();
    let populations: BTreeMap<_, _> = populations
        .as_array()
        .into_iter()
        .flatten()
        .map(|population| (population["id"].clone().to_string(), population))
        .collect();
    for finding in findings.as_array_mut().into_iter().flatten() {
        let source_checked =
            artifact_available && finding["verification"]["source_and_rules"] == "available";
        let chain = &finding["details"]["event_chain"];
        let subjects: Vec<_> = finding["evidence"].as_array().into_iter().flatten().filter(|excerpt| !chain.is_object() || excerpt["occurrence"] == chain["root"] || chain["steps"].as_array().into_iter().flatten().flat_map(|step| step["outcomes"].as_array().into_iter().flatten()).any(|outcome| outcome["occurrence"] == excerpt["occurrence"])).map(|excerpt| {
            let record = OccurrenceId::from_value(&excerpt["occurrence"]).ok().and_then(|id| records.get(&id));
            json!({"occurrence":excerpt["occurrence"], "event":record.and_then(|r| r.pointer("/fields/classification/semantics"))})
        }).collect();
        let has_link = chain["steps"].as_array().into_iter().flatten().any(|step| {
            step["join_samples"]
                .as_array()
                .is_some_and(|joins| !joins.is_empty())
        });
        finding["checked"] = json!({
            "version":1,
            "population_rule":crate::investigation::CHECKED_POPULATION_RULE,
            "scope":scopes.get(&finding["scope_id"].to_string()),
            "identity":chain.get("identity"),
            "subjects":subjects,
            "population":populations.get(&finding["details"]["population_id"].to_string()),
            "assurance":{
                "level":if !source_checked {"relations_only"} else if has_link {"link_exists"} else {"source_bound"},
                "source_binding":if source_checked {"validated"} else {"unavailable"},
                "subject_verification":if source_checked {"not_requested"} else {"unavailable"},
                "limits":crate::investigation::CHECKED_ASSURANCE_LIMITS
            }
        });
    }
}

pub(super) fn uncertainty(findings: &Value) -> Value {
    let findings = findings.as_array().into_iter().flatten();
    json!({
        "unknown_findings":findings.clone().filter(|f| f["kind"] == "unknown").count(),
        "incomplete_chains":findings.clone().filter(|f| f["details"]["event_chain"].is_object() && (f["details"]["event_chain"]["completeness"] != "complete" || f["details"]["event_chain"]["steps"].as_array().into_iter().flatten().any(|s| s["status"] != "observed"))).count(),
        "source_binding_unavailable_findings":findings.clone().filter(|f| f["checked"]["assurance"]["source_binding"] == "unavailable").count(),
        "subject_verification":if findings.clone().any(|f| f["checked"]["assurance"]["subject_verification"] == "unavailable") {"unavailable"} else {"not_requested"},
        "limits":crate::investigation::UNCERTAINTY_LIMITS
    })
}

/// Reconstructed reports need the same marker and aggregate as publication.
pub(super) fn mark(report: &mut Value) {
    if report["findings"].as_array().is_some_and(|findings| {
        findings
            .iter()
            .any(|finding| finding.get("checked").is_some())
    }) {
        report["presentation"]["checked_version"] = json!(1);
        report["presentation"]["uncertainty"] = uncertainty(&report["findings"]);
    }
}

impl CheckedResult {
    // @lat: [[contracts/investigation-contract#Investigation report contract version 1#Checked presentation]]
    pub(super) fn build(mut report: Value, retained: Value, bytes: Option<&[u8]>) -> Result<Self> {
        let scopes = report["scopes"].clone();
        let populations = report["populations"].clone();
        project(
            &mut report["findings"],
            &retained["records"],
            &scopes,
            &populations,
            bytes.is_some(),
        );
        mark(&mut report);
        drop(retained);
        validate_relations(&report, bytes)?;
        Ok(Self(report))
    }

    pub(super) fn into_report(self) -> Value {
        self.0
    }

    /// Pagination changes presentation only; recheck its references and omission accounting.
    pub(super) fn page(report: Value) -> Result<Self> {
        validate_relations(&report, None)?;
        Ok(Self(report))
    }

    pub(super) fn report(&self) -> &Value {
        &self.0
    }
}
