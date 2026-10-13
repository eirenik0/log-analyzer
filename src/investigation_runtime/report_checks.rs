//! Validate streamed presentation as an exact projection of the independently
//! checked private artifact, including when publication is unavailable.
use super::{
    Result, construction::Disk, finding_store::Findings, indexed_artifact::IndexedArtifact,
    report_stream::Page,
};
use crate::{disk_index::DiskIndex, json_index::Node};
use serde_json::{Value, json};
use std::{collections::BTreeSet, io::Write};

fn require(condition: bool, reason: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(format!("streamed report validation: {reason}").into())
    }
}
fn field(node: &Node, key: &str) -> Result<Node> {
    node.field(key)?
        .ok_or_else(|| format!("missing checked artifact {key}").into())
}
fn number(value: &Value, key: &str) -> Result<u64> {
    value[key]
        .as_u64()
        .ok_or_else(|| format!("missing report count {key}").into())
}
fn positive(value: &Value) -> bool {
    matches!(
        value["kind"].as_str(),
        Some("observation" | "measurement" | "calculated_fact")
    )
}

pub(super) fn validate(
    page: &Page,
    findings: &Findings,
    checked: &IndexedArtifact,
    expected_sha256: &str,
    disk: &Disk,
) -> Result<()> {
    let metadata = &page.metadata;
    let available = metadata["artifact"]["status"] != "unavailable";
    let limit = checked.limits.item_bytes;
    let manifest = &metadata["report_metadata"]["evidence"];
    let binding = crate::evidence::digest(
        json!([
            expected_sha256,
            manifest["snapshot_id"],
            manifest["profile_sha256"],
            manifest["query_sha256"],
            manifest["redaction"]
        ])
        .to_string()
        .as_bytes(),
    );
    require(metadata["contract_version"] == 1, "unsupported contract")?;
    for key in ["report_metadata", "scopes", "populations"] {
        require(
            metadata[key] == field(&checked.root, key)?.read(limit)?,
            "analysis metadata differs from checked artifact",
        )?;
    }
    let processing = field(&checked.root, "processing")?.read(limit)?;
    if available {
        require(
            metadata["processing"] == processing,
            "processing differs from checked artifact",
        )?;
        require(
            metadata["assessments"] == field(&checked.root, "assessments")?.read(limit)?,
            "assessments differ from checked artifact",
        )?;
        require(
            metadata["artifact"]["stored_sha256"] == expected_sha256
                && metadata["retrieval"]["binding_sha256"] == binding,
            "artifact binding differs",
        )?;
        require(
            metadata["retrieval"]["status"] == "available",
            "retained artifact cannot lose retrieval status",
        )?;
        require(
            metadata["artifact"]["content"] == field(&checked.root, "content")?.read(limit)?,
            "artifact content differs",
        )?;
    } else {
        let mut actual = metadata["processing"].clone();
        actual["status"] = processing["status"].clone();
        actual["stop"] = processing["stop"].clone();
        require(
            actual == processing && metadata["processing"]["status"] == "partial",
            "unavailable processing differs",
        )?;
        require(
            metadata["artifact"]["stored_sha256"].is_null()
                && metadata["artifact"]["location"].is_null()
                && metadata["retrieval"]["binding_sha256"].is_null()
                && metadata["retrieval"]["status"] == "unavailable",
            "unavailable artifact advertises retrieval",
        )?;
        require(
            metadata["artifact"]["verification"]["artifact_integrity"] == "unavailable",
            "unavailable artifact claims integrity",
        )?;
    }
    let mut target_keys = BTreeSet::new();
    for target in metadata["retrieval"]["targets"]
        .as_array()
        .ok_or("missing retrieval targets")?
    {
        require(available, "unavailable artifact advertises target")?;
        let kind = target["kind"]
            .as_str()
            .ok_or("missing retrieval target kind")?;
        let id = target["id"].as_str().ok_or("missing retrieval target id")?;
        let pointer = target["collection"]
            .as_str()
            .ok_or("missing retrieval target pointer")?;
        require(target_keys.insert((kind, id)), "duplicate retrieval target")?;
        let resolved = checked
            .root
            .pointer(pointer)?
            .ok_or("retrieval target is absent")?;
        match kind {
            "finding" | "event_sequence" => {
                require(
                    pointer.starts_with(if kind == "finding" {
                        "/findings/"
                    } else {
                        "/sequences/"
                    }),
                    "retrieval target collection has wrong kind",
                )?;
                require(
                    field(&resolved, "id")?.read(limit)? == id,
                    "retrieval target points to another identity",
                )?;
            }
            "population" => {
                let population = metadata["populations"]
                    .as_array()
                    .ok_or("missing populations")?
                    .iter()
                    .find(|population| population["id"] == id)
                    .ok_or("retrieval population is absent")?;
                require(
                    population["membership"]["collection"] == pointer && resolved.is_array(),
                    "retrieval target points to another population",
                )?;
            }
            _ => return Err("unknown retrieval target kind".into()),
        }
    }
    let mut key_bytes = 0u64;
    for item in page.rows.iter()? {
        checked.checkpoint()?;
        let item = item?;
        key_bytes += item.finding.value["id"]
            .as_str()
            .ok_or("missing finding id")?
            .len() as u64;
    }
    let slots = (page.rows.len() as u64)
        .max(1)
        .checked_mul(2)
        .and_then(u64::checked_next_power_of_two)
        .and_then(|n| n.checked_mul(64))
        .ok_or("report index capacity overflow")?;
    let state_bytes = slots
        .checked_add(key_bytes)
        .ok_or("report index byte overflow")?;
    let _reservation = disk.reserve(state_bytes)?;
    let mut ids = DiskIndex::new(page.rows.len() as u64, state_bytes)?;
    let all_findings = field(&checked.root, "findings")?;
    let mut original = all_findings.children().enumerate();
    let mut last_ordinal = None;
    for (index, item) in page.rows.iter()?.enumerate() {
        checked.checkpoint()?;
        let item = item?;
        require(
            last_ordinal.is_none_or(|prior| item.ordinal > prior),
            "finding order is not stable",
        )?;
        last_ordinal = Some(item.ordinal);
        let expected = loop {
            let (ordinal, node) = original
                .next()
                .ok_or("page finding ordinal is not retained")?;
            if ordinal == item.ordinal {
                break node?;
            }
        };
        let mut expected_value = expected.read_without(&["details"], limit)?;
        let expected_details = field(&expected, "details")?;
        expected_value["details"] = expected_details.read_without(&["measurement_ids"], limit)?;
        let mut actual_value = item.finding.value.clone();
        actual_value["details"]
            .as_object_mut()
            .ok_or("missing finding details")?
            .remove("measurement_ids");
        if !available {
            expected_value["verification"]["artifact_integrity"] = json!("unavailable");
            for excerpt in expected_value["evidence"]
                .as_array_mut()
                .into_iter()
                .flatten()
            {
                excerpt["verification"]["artifact_integrity"] = json!("unavailable");
            }
        }
        require(
            actual_value == expected_value,
            "finding differs from checked artifact",
        )?;
        let id = actual_value["id"].as_str().ok_or("missing finding id")?;
        require(
            ids.insert(id.as_bytes(), index as u64)?.is_none(),
            "duplicate displayed finding id",
        )?;
        let mut actual_ids = findings.measurement_ids(&item.finding)?;
        if let Some(expected_ids) = expected_details.field("measurement_ids")? {
            require(expected_ids.is_array(), "measurement ids is not an array")?;
            for expected_id in expected_ids.children() {
                checked.checkpoint()?;
                let expected_id = expected_id?.read(limit)?;
                require(
                    actual_ids.next().transpose()?.as_deref() == expected_id.as_str(),
                    "measurement reference differs from checked artifact",
                )?;
            }
        }
        require(
            actual_ids.next().transpose()?.is_none(),
            "extra measurement reference",
        )?;
    }
    let can_retrieve = |id: &str| {
        available
            && metadata["retrieval"]["targets"]
                .as_array()
                .is_some_and(|targets| {
                    targets
                        .iter()
                        .any(|target| target["kind"] == "finding" && target["id"] == id)
                })
    };
    for item in page.rows.iter()? {
        checked.checkpoint()?;
        let item = item?;
        for id in findings.measurement_ids(&item.finding)? {
            let id = id?;
            require(
                ids.get(id.as_bytes())?.is_some() || can_retrieve(&id),
                "distribution has omitted, non-retrievable measurements",
            )?;
        }
        if item.finding.value["kind"] == "contrary_evidence" {
            let target = item.finding.value["details"]["against_finding_id"]
                .as_str()
                .ok_or("contrary evidence has no target")?;
            require(
                ids.get(target.as_bytes())?.is_some() || can_retrieve(target),
                "contrary evidence has omitted, non-retrievable target",
            )?;
        }
    }
    let original_assessments = field(&checked.root, "assessments")?.read(limit)?;
    require(
        metadata["assessments"].as_array().map(Vec::len)
            == original_assessments.as_array().map(Vec::len),
        "assessment inventory changed",
    )?;
    let mut assessment_keys = BTreeSet::new();
    for assessment in metadata["assessments"]
        .as_array()
        .ok_or("missing assessments")?
    {
        let goal = assessment["goal"]
            .as_str()
            .ok_or("missing assessment goal")?;
        let scope = assessment["scope_id"]
            .as_str()
            .ok_or("missing assessment scope")?;
        require(
            assessment_keys.insert((goal, scope)),
            "duplicate assessment",
        )?;
        let original = original_assessments
            .as_array()
            .ok_or("missing original assessments")?
            .iter()
            .find(|original| original["goal"] == goal && original["scope_id"] == scope)
            .ok_or("assessment identity changed")?;
        require(
            assessment["status"] == original["status"]
                || (original["status"] == "supported"
                    && assessment["status"] == "insufficient_evidence"
                    && !available),
            "assessment certainty increased",
        )?;
        let mut supporting = false;
        for id in assessment["finding_ids"]
            .as_array()
            .ok_or("missing assessment references")?
        {
            let id = id.as_str().ok_or("invalid assessment reference")?;
            let value = if let Some(ordinal) = ids.get(id.as_bytes())? {
                page.rows.get(ordinal as usize)?.finding.value
            } else if can_retrieve(id) {
                let original = checked
                    .find_node(id)?
                    .ok_or("assessment reference is not retained")?;
                original.read_without(&["details", "evidence"], limit)?
            } else {
                return Err("unavailable assessment references omitted finding".into());
            };
            require(
                value["scope_id"] == assessment["scope_id"],
                "assessment crosses scope",
            )?;
            supporting |= positive(&value);
        }
        require(
            assessment["status"] != "supported" || supporting,
            "supported assessment lacks positive displayed basis",
        )?;
    }
    presentation(page, findings, all_findings.len(), available, &binding)?;
    Ok(())
}

fn presentation(
    page: &Page,
    findings: &Findings,
    total: u64,
    available: bool,
    hash: &str,
) -> Result<()> {
    let metadata = &page.metadata;
    let value = &metadata["presentation"];
    let displayed = page.rows.len() as u64;
    require(
        number(value, "displayed_findings")? == displayed,
        "displayed count differs",
    )?;
    require(
        number(value, "total_findings")? == total
            && displayed.checked_add(number(value, "omitted_findings")?) == Some(total),
        "finding totals differ",
    )?;
    let mut paths = BTreeSet::new();
    let mut remaining = false;
    for collection in value["collections"]
        .as_array()
        .ok_or("missing presentation collections")?
    {
        let path = collection["path"]
            .as_str()
            .ok_or("missing collection path")?;
        require(paths.insert(path), "duplicate presentation collection")?;
        let length = if path == "/findings" {
            displayed
        } else {
            metadata
                .pointer(path)
                .and_then(Value::as_array)
                .ok_or("invalid presentation collection")?
                .len() as u64
        };
        require(
            number(collection, "displayed")? == length
                && number(collection, "prior")?
                    .checked_add(length)
                    .and_then(|n| n.checked_add(number(collection, "remaining").ok()?))
                    == Some(number(collection, "total")?),
            "collection counts differ",
        )?;
        if path == "/findings" {
            require(
                number(collection, "total")? == total && number(collection, "prior")? == 0,
                "finding collection differs",
            )?;
        }
        remaining |= number(collection, "remaining")? > 0;
    }
    require(paths.contains("/findings"), "missing findings collection")?;
    let mut size = Size::default();
    page.write_json(findings, &mut size)?;
    require(
        number(value, "serialized_bytes")? == size.bytes
            && number(value, "serialized_characters")? == size.chars,
        "serialized size differs",
    )?;
    let mut size_limited = false;
    let mut over_budget = false;
    for (key, usage) in [
        ("budget_bytes", size.bytes),
        ("budget_characters", size.chars),
    ] {
        if let Some(limit) = value[key].as_u64() {
            size_limited = true;
            over_budget |= usage > limit;
        }
    }
    require(
        value["budget_items"]
            .as_u64()
            .is_none_or(|limit| displayed <= limit),
        "display exceeds item budget",
    )?;
    let status = value["status"]
        .as_str()
        .ok_or("missing presentation status")?;
    let status_valid = match status {
        "complete" => displayed == total && !remaining,
        "page" => displayed > 0 && displayed < total,
        "item_limit_zero" => value["budget_items"] == 0 && displayed == 0 && remaining,
        "oversized_item" => {
            size_limited
                && !over_budget
                && value["budget_items"] != 0
                && displayed == 0
                && remaining
        }
        "mandatory_metadata_over_budget" => over_budget && displayed == 0,
        _ => false,
    };
    require(
        status_valid && (!over_budget || status == "mandatory_metadata_over_budget"),
        "presentation status differs from budget/counts",
    )?;
    let cursor = if available && displayed < total {
        json!(format!(
            "v1:{}:{displayed}",
            crate::evidence::digest(json!([hash, "/findings", null]).to_string().as_bytes())
        ))
    } else {
        Value::Null
    };
    require(
        metadata["retrieval"]["next_cursor"] == cursor,
        "retrieval cursor differs",
    )?;
    Ok(())
}
#[derive(Default)]
struct Size {
    bytes: u64,
    chars: u64,
}
impl Write for Size {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes += bytes.len() as u64;
        self.chars += bytes.iter().filter(|byte| **byte & 0xc0 != 0x80).count() as u64;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presentation_checks_actual_serialization_counts_budgets_and_cursor() {
        let disk = Disk::new(1024 * 1024);
        let findings = Findings::new(disk.clone(), 65536).unwrap();
        let mut page = Page {
            metadata: json!({"findings":[],"retrieval":{"targets":[],"next_cursor":null},"presentation":{"status":"complete","total_findings":0,"displayed_findings":0,"omitted_findings":0,"budget_items":20,"budget_bytes":null,"budget_characters":null,"serialized_bytes":0,"serialized_characters":0,"collections":[{"path":"/findings","total":0,"prior":0,"displayed":0,"remaining":0}]}}),
            rows: super::super::construction::Rows::new(disk, 65536).unwrap(),
        };
        for _ in 0..8 {
            let mut size = Size::default();
            page.write_json(&findings, &mut size).unwrap();
            page.metadata["presentation"]["serialized_bytes"] = json!(size.bytes);
            page.metadata["presentation"]["serialized_characters"] = json!(size.chars);
        }
        presentation(&page, &findings, 0, false, "").unwrap();
        let original = page.metadata.clone();
        for (pointer, value) in [
            ("/presentation/displayed_findings", json!(1)),
            ("/presentation/serialized_bytes", json!(1)),
            ("/presentation/budget_bytes", json!(1)),
            ("/retrieval/next_cursor", json!("invented")),
            ("/presentation/status", json!("page")),
        ] {
            page.metadata = original.clone();
            *page.metadata.pointer_mut(pointer).unwrap() = value;
            assert!(
                presentation(&page, &findings, 0, false, "").is_err(),
                "{pointer}"
            );
        }
    }
    #[test]
    fn changed_measurements_counts_and_retrieval_targets_are_rejected() {
        use super::super::{
            construction::Rows, finding_store::FindingSink, indexed_artifact::Limits,
            report_stream::Item,
        };
        use std::io::{Seek, SeekFrom};
        let bytes = include_bytes!("../../examples/investigation-contract/supported.artifact.json");
        let artifact: Value = serde_json::from_slice(bytes).unwrap();
        let hash = crate::evidence::digest(bytes);
        let report = super::super::artifact::report(
            &artifact,
            std::path::Path::new("fixture-artifact.json"),
            Some(&hash),
        );
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(bytes).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut checked = IndexedArtifact::open(
            file,
            Limits {
                item_bytes: 8 * 1024 * 1024,
                index_bytes: 16 * 1024 * 1024,
                state_bytes: 16 * 1024 * 1024,
                elapsed_ms: 60000,
                work_units: 10000000,
            },
        )
        .unwrap();
        checked.validate().unwrap();
        let hash = crate::evidence::digest(bytes);
        for unavailable in [false, true] {
            for change in ["none", "measurement", "count", "target"] {
                if unavailable && change == "target" {
                    continue;
                }
                let disk = Disk::new(16 * 1024 * 1024);
                let mut findings = Findings::new(disk.clone(), 65536).unwrap();
                let mut page = Page {
                    metadata: report.clone(),
                    rows: Rows::new(disk.clone(), 65536).unwrap(),
                };
                page.metadata["findings"] = json!([]);
                if unavailable {
                    super::super::artifact::unavailable(
                        &mut page.metadata,
                        "artifact byte limit exceeded",
                    );
                }
                for (ordinal, finding) in
                    artifact["findings"].as_array().unwrap().iter().enumerate()
                {
                    findings.push(finding.clone()).unwrap();
                    let mut value = finding.clone();
                    if change == "measurement" && ordinal == 0 {
                        value["details"]["value"] = json!(999999);
                    }
                    if unavailable {
                        value["verification"]["artifact_integrity"] = json!("unavailable");
                        for excerpt in value["evidence"].as_array_mut().unwrap() {
                            excerpt["verification"]["artifact_integrity"] = json!("unavailable");
                        }
                    }
                    page.rows
                        .push(&Item {
                            ordinal,
                            finding: serde_json::from_value(json!({"value":value,"ids":null}))
                                .unwrap(),
                        })
                        .unwrap();
                }
                if change == "count" {
                    page.metadata["presentation"]["displayed_findings"] = json!(99);
                }
                if change == "target" {
                    page.metadata["retrieval"]["targets"][0]["collection"] = json!("/findings/1");
                }
                for _ in 0..8 {
                    let mut size = Size::default();
                    page.write_json(&findings, &mut size).unwrap();
                    page.metadata["presentation"]["serialized_bytes"] = json!(size.bytes);
                    page.metadata["presentation"]["serialized_characters"] = json!(size.chars);
                }
                let result = validate(&page, &findings, &checked, &hash, &disk);
                if change == "none" {
                    result.unwrap();
                } else {
                    assert!(result.is_err(), "{change}, unavailable={unavailable}");
                }
            }
        }
    }
}
