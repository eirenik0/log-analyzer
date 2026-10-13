//! Publication boundary and a lossless projection of checked evidence, not a verifier.
use super::Result;
use crate::investigation::{OccurrenceId, validate_relations};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) struct CheckedResult(Value);

impl CheckedResult {
    // @lat: [[contracts/investigation-contract#Investigation report contract version 1#Checked presentation]]
    pub(super) fn build(mut report: Value, retained: Value, bytes: Option<&[u8]>) -> Result<Self> {
        let records: BTreeMap<_, _> = retained["records"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|record| Ok((OccurrenceId::from_value(&record["occurrence"])?, record)))
            .collect::<Result<_>>()?;
        let scopes: BTreeMap<_, _> = report["scopes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|scope| (scope["id"].clone().to_string(), scope.clone()))
            .collect();
        let populations: BTreeMap<_, _> = report["populations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|population| (population["id"].clone().to_string(), population.clone()))
            .collect();
        for finding in report["findings"].as_array_mut().unwrap() {
            let source_checked =
                bytes.is_some() && finding["verification"]["source_and_rules"] == "available";
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
                "population_rule":"Counts retain their named population. Chain steps count distinct source occurrences (snapshot_id, input_ordinal, reference_id) within this input and chain scope; observed and linked populations are separate. No logical request or failure deduplication is inferred.",
                "scope":scopes.get(&finding["scope_id"].to_string()),
                "identity":chain.get("identity"),
                "subjects":subjects,
                "population":populations.get(&finding["details"]["population_id"].to_string()),
                "assurance":{
                    "level":if !source_checked {"relations_only"} else if has_link {"link_exists"} else {"source_bound"},
                    "source_binding":if source_checked {"validated"} else {"unavailable"},
                    "subject_verification":if source_checked {"not_requested"} else {"unavailable"},
                    "limits":["Source binding checks retained evidence consistency, not intended question meaning or independent profile semantics. subject_verified requires independent expected subjects through claim verification."]
                }
            });
        }
        let findings = report["findings"].as_array().unwrap();
        report["presentation"]["uncertainty"] = json!({
            "unknown_findings":findings.iter().filter(|f| f["kind"] == "unknown").count(),
            "incomplete_chains":findings.iter().filter(|f| f["details"]["event_chain"].is_object() && (f["details"]["event_chain"]["completeness"] != "complete" || f["details"]["event_chain"]["steps"].as_array().into_iter().flatten().any(|s| s["status"] != "observed"))).count(),
            "source_binding_unavailable_findings":findings.iter().filter(|f| f["checked"]["assurance"]["source_binding"] == "unavailable").count(),
            "subject_verification":if findings.iter().any(|f| f["checked"]["assurance"]["subject_verification"] == "unavailable") {"unavailable"} else {"not_requested"},
            "limits":["Counts above describe all processed findings, including omitted detail. Missing endpoints remain unavailable within the selected capture. Upstream completeness, clock synchronization and causality are unknown.", "An existing link does not establish intended subject meaning. Independent expected subjects are required for subject_verified assurance."]
        });
        drop(records);
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
