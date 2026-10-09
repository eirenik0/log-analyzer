use super::Result;
use crate::{
    cli::{Cli, InvestigationEvidenceArgs},
    evidence::digest,
    profile_mappings::{destination_identity, same_destination},
};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(super) fn protect(artifact: &Path, report: Option<&Path>, sources: &[PathBuf]) -> Result<()> {
    let artifact = destination_identity(artifact)?;
    if artifact.exists() {
        return Err("Artifact destination already exists; choose a new file".into());
    }
    if let Some(report) = report
        && same_destination(&artifact, &destination_identity(report)?, false)
    {
        return Err("Artifact and report destinations must be separate".into());
    }
    for source in sources {
        let source = destination_identity(source)?;
        if same_destination(&artifact, &source, false)
            || report.is_some_and(|report| same_destination(report, &source, true))
        {
            return Err(
                "Artifact/report destination conflicts with an input, profile or cancellation file"
                    .into(),
            );
        }
    }
    Ok(())
}
pub(super) fn stage(path: &Path) -> Result<tempfile::NamedTempFile> {
    Ok(tempfile::NamedTempFile::new_in(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?)
}
struct LimitedWriter {
    bytes: Vec<u8>,
    limit: u64,
}
impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.limit.saturating_sub(self.bytes.len() as u64) {
            return Err(std::io::Error::other("artifact byte limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(super) fn bounded_serialization(value: &Value, limit: u64) -> Result<Option<Vec<u8>>> {
    let mut writer = LimitedWriter {
        bytes: Vec::new(),
        limit,
    };
    match serde_json::to_writer(&mut writer, value) {
        Ok(()) => Ok(Some(writer.bytes)),
        Err(error) if error.is_io() => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn opaque(value: &mut Value) {
    if let Some(text) = value.as_str() {
        *value = json!(format!("opaque-{}", digest(text.as_bytes())));
    }
}
pub(super) fn redact(artifact: &mut Value) {
    artifact["content"] = json!("redacted");
    artifact["effective_profile"] = Value::Null;
    artifact["effective_profile_omitted"] = json!(true);
    artifact["verification"] = super::findings::verification(true, "available");
    artifact["report_metadata"]["active_profile"] = json!("[REDACTED PROFILE]");
    let manifest = &mut artifact["report_metadata"]["evidence"];
    manifest["query"] = json!({"command":"[REDACTED QUERY]","filter":"[REDACTED FILTER]"});

    for input in manifest["inputs"].as_array_mut().unwrap() {
        input["file"] = json!("[REDACTED PATH]");
        input["coverage"]["file"] = json!("[REDACTED PATH]");
        input["coverage"]["profile"] = json!("[REDACTED PROFILE]");
        if let Some(diagnostics) = input["coverage"]["normalization_diagnostics"].as_array_mut() {
            for diagnostic in diagnostics {
                diagnostic["row_path"] = json!("[REDACTED PATH]");
                diagnostic["field"] = json!("[REDACTED FIELD]");
            }
        }
    }
    for capture in artifact["captured_inputs"].as_array_mut().unwrap() {
        capture["data"] = Value::Null;
        capture["stored_sha256"] = Value::Null;
        capture["data_omitted"] = json!(true);
    }
    for record in artifact["records"].as_array_mut().unwrap() {
        record["raw_text"] = Value::Null;
        record["message"] = Value::Null;
        record["fields"] = json!({});
        record["data_omitted"] = json!(true);
        record["verification"] = super::findings::verification(true, "not_applicable");
    }
    for finding in artifact["findings"].as_array_mut().unwrap() {
        let arithmetic = finding["verification"]["arithmetic"]
            .as_str()
            .unwrap()
            .to_owned();
        finding["verification"] = super::findings::verification(true, &arithmetic);
        for excerpt in finding["evidence"].as_array_mut().unwrap() {
            excerpt["text"] = json!("[REDACTED SOURCE]");
            excerpt["omitted_characters"] = json!(0);
            excerpt["verification"] = super::findings::verification(true, "not_applicable");
        }
    }
    // All retained domain identities/rule names become stable opaque values.
    fn identifiers(value: &mut Value) {
        match value {
            Value::Object(map) => {
                if map.contains_key("reference_id") && map.contains_key("input_id") {
                    map.insert("location_redacted".into(), json!(true));
                    map.insert("row_path".into(), Value::Null);
                    map.remove("expansion");
                }
                if map.get("field").is_some() && map.get("value").is_some() {
                    opaque(map.get_mut("value").unwrap());
                    opaque(map.get_mut("field").unwrap());
                }
                if let Some(Value::Array(fields)) = map.get_mut("identity_fields") {
                    for field in fields {
                        opaque(field);
                    }
                }
                if let Some(rule) = map.get_mut("rule_id") {
                    opaque(rule);
                }
                if let Some(Value::Array(rules)) = map.get_mut("rule_ids") {
                    for rule in rules {
                        opaque(rule);
                    }
                }
                for (key, child) in map {
                    if !matches!(key.as_str(), "report_metadata") {
                        identifiers(child);
                    }
                }
            }
            Value::Array(values) => {
                for child in values {
                    identifiers(child);
                }
            }
            _ => (),
        }
    }
    identifiers(artifact);
    let hashes: Vec<_> = artifact["memberships"]
        .as_array()
        .unwrap()
        .iter()
        .map(|membership| digest(membership["members"].to_string().as_bytes()))
        .collect();
    for (population, hash) in artifact["populations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(hashes)
    {
        population["membership"]["sha256"] = json!(hash);
    }
}
fn binding(artifact: &Value, hash: &str) -> String {
    digest(
        json!([
            hash,
            artifact["report_metadata"]["evidence"]["snapshot_id"],
            artifact["report_metadata"]["evidence"]["profile_sha256"],
            artifact["report_metadata"]["evidence"]["query_sha256"],
            artifact["report_metadata"]["evidence"]["redaction"]
        ])
        .to_string()
        .as_bytes(),
    )
}
pub(super) fn report(artifact: &Value, path: &Path, bytes: Option<&[u8]>) -> Value {
    let hash = bytes.map(digest);
    let mut report = json!({"contract_version":1});
    for key in [
        "report_metadata",
        "processing",
        "scopes",
        "assessments",
        "populations",
        "findings",
    ] {
        report[key] = artifact[key].clone();
    }
    report["artifact"] = json!({"contract_version":1,"status":if bytes.is_some(){if artifact["processing"]["status"]=="complete"{"complete"}else{"partial"}}else{"unavailable"},"location":if bytes.is_some(){Some(if artifact["content"] == "redacted" { "[REDACTED ARTIFACT PATH]".into() } else {crate::evidence::path_label(path)})}else{None},"stored_sha256":hash,"content":if bytes.is_some(){artifact["content"].clone()}else{json!("unavailable")},"retention":artifact["retention"],"verification":artifact["verification"]});
    let mut targets = Vec::new();
    for (index, finding) in artifact["findings"].as_array().unwrap().iter().enumerate() {
        targets.push(
            json!({"kind":"finding","id":finding["id"],"collection":format!("/findings/{index}")}),
        );
    }
    for population in artifact["populations"].as_array().unwrap() {
        targets.push(json!({"kind":"population","id":population["id"],"collection":population["membership"]["collection"]}));
    }
    for (index, sequence) in artifact["sequences"].as_array().unwrap().iter().enumerate() {
        targets.push(json!({"kind":"event_sequence","id":sequence["id"],"collection":format!("/sequences/{index}")}));
    }
    report["retrieval"] = json!({"interface":"artifact","contract_version":1,"status":if bytes.is_some(){"available"}else{"unavailable"},"binding_sha256":hash.as_deref().map(|hash|binding(artifact,hash)),"next_cursor":null,"targets":targets,"reason":if bytes.is_some(){None}else{Some("Artifact byte limit exceeded")}});
    report["presentation"] = json!({"status":"complete","budget_bytes":null,"budget_characters":null,"budget_items":null,"serialized_bytes":null,"serialized_characters":null,"total_findings":artifact["findings"].as_array().unwrap().len(),"displayed_findings":artifact["findings"].as_array().unwrap().len(),"omitted_findings":0,"collections":[{"path":"/findings","total":artifact["findings"].as_array().unwrap().len(),"prior":0,"displayed":artifact["findings"].as_array().unwrap().len(),"remaining":0}],"units":"serialized_UTF8_bytes_Unicode_scalars_and_atomic_finding_bundles"});
    report
}
pub(super) fn unavailable(report: &mut Value, reason: &str) {
    report["processing"]["status"] = json!("partial");
    if report["processing"]["stop"].is_null() {
        report["processing"]["stop"] = json!({"stage":"artifact_write","reason":if reason.contains("limit"){"storage_limit"}else{"io_error"},"limit_name":if reason.contains("limit"){Some("artifact_bytes")}else{None},"scope_ids":report["scopes"].as_array().unwrap().iter().map(|scope|scope["id"].clone()).collect::<Vec<_>>()});
    }
    report["artifact"]["status"] = json!("unavailable");
    report["artifact"]["content"] = json!("unavailable");
    report["artifact"]["location"] = Value::Null;
    report["artifact"]["stored_sha256"] = Value::Null;
    report["artifact"]["verification"]["artifact_integrity"] = json!("unavailable");
    report["retrieval"]["status"] = json!("unavailable");
    report["retrieval"]["reason"] = json!(reason);
    report["retrieval"]["binding_sha256"] = Value::Null;
    report["retrieval"]["targets"] = json!([]);
    for finding in report["findings"].as_array_mut().unwrap() {
        finding["verification"]["artifact_integrity"] = json!("unavailable");
        for excerpt in finding["evidence"].as_array_mut().unwrap() {
            excerpt["verification"]["artifact_integrity"] = json!("unavailable");
        }
    }
}
pub(super) fn source_verification_unavailable(artifact: &mut Value, reason: &str) {
    fn loss(value: &mut Value, reason: &str) {
        match value {
            Value::Object(map) => {
                if let Some(verification) = map.get_mut("verification") {
                    verification["source_and_rules"] = json!("unavailable");
                    verification["losses"] = json!([reason]);
                }
                for (key, child) in map {
                    if key != "verification" {
                        loss(child, reason);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    loss(value, reason);
                }
            }
            _ => (),
        }
    }
    loss(artifact, reason);
}
pub(super) fn reconcile_unavailable(report: &mut Value, cli: &Cli) -> Result<()> {
    let mut ids: std::collections::BTreeSet<_> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|finding| finding["id"].as_str().map(str::to_owned))
        .collect();
    report["findings"]
        .as_array_mut()
        .unwrap()
        .retain(|finding| {
            finding
                .pointer("/details/measurement_ids")
                .and_then(Value::as_array)
                .is_none_or(|measurements| {
                    measurements
                        .iter()
                        .all(|id| id.as_str().is_some_and(|id| ids.contains(id)))
                })
        });
    ids = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|finding| finding["id"].as_str().map(str::to_owned))
        .collect();
    for assessment in report["assessments"].as_array_mut().unwrap() {
        assessment["finding_ids"]
            .as_array_mut()
            .unwrap()
            .retain(|id| id.as_str().is_some_and(|id| ids.contains(id)));
        if assessment["finding_ids"].as_array().unwrap().is_empty()
            && assessment["status"] == "supported"
        {
            assessment["status"] = json!("insufficient_evidence");
            assessment["reason"] = json!(
                "Evidence artifact unavailable and bounded presentation retained no supporting findings"
            );
        }
    }
    let count = report["findings"].as_array().unwrap().len();
    let total = report["presentation"]["total_findings"].as_u64().unwrap() as usize;
    report["presentation"]["displayed_findings"] = json!(count);
    report["presentation"]["omitted_findings"] = json!(total - count);
    report["presentation"]["collections"][0]["displayed"] = json!(count);
    report["presentation"]["collections"][0]["remaining"] = json!(total - count);
    if count == 0 && total > 0 && report["presentation"]["status"] == "page" {
        report["presentation"]["status"] = json!("oversized_item");
    }
    let sizes = size(report)?;
    if over(cli, sizes) {
        report["findings"] = json!([]);
        for assessment in report["assessments"].as_array_mut().unwrap() {
            assessment["finding_ids"] = json!([]);
            if assessment["status"] == "supported" {
                assessment["status"] = json!("insufficient_evidence");
            }
        }
        report["presentation"]["displayed_findings"] = json!(0);
        report["presentation"]["omitted_findings"] = json!(total);
        report["presentation"]["collections"][0]["displayed"] = json!(0);
        report["presentation"]["collections"][0]["remaining"] = json!(total);
        report["presentation"]["status"] = json!("mandatory_metadata_over_budget");
        size(report)?;
    }
    Ok(())
}
fn size(report: &mut Value) -> Result<(usize, usize)> {
    // Size fields themselves count; converge on their exact serialized lengths.
    for _ in 0..8 {
        let text = serde_json::to_string(report)?;
        let sizes = (text.len() + 1, text.chars().count() + 1);
        if report["presentation"]["serialized_bytes"] == sizes.0
            && report["presentation"]["serialized_characters"] == sizes.1
        {
            return Ok(sizes);
        }
        report["presentation"]["serialized_bytes"] = json!(sizes.0);
        report["presentation"]["serialized_characters"] = json!(sizes.1);
    }
    Err("Could not stabilize serialized report size".into())
}
fn over(cli: &Cli, sizes: (usize, usize)) -> bool {
    cli.report_max_bytes.is_some_and(|limit| sizes.0 > limit)
        || cli.report_max_chars.is_some_and(|limit| sizes.1 > limit)
}
pub(super) fn present(report: &mut Value, cli: &Cli, prior: usize) -> Result<()> {
    let all = std::mem::take(report["findings"].as_array_mut().unwrap());
    let total = all.len();
    let max = if cli.complete_output {
        total
    } else {
        cli.report_max_items.unwrap_or(20)
    };
    report["presentation"]["budget_bytes"] = json!(cli.report_max_bytes);
    report["presentation"]["budget_characters"] = json!(cli.report_max_chars);
    report["presentation"]["budget_items"] = json!(max);
    let refresh = |report: &mut Value, count: usize, status: &str| {
        report["presentation"]["status"] = json!(status);
        report["presentation"]["total_findings"] = json!(total);
        report["presentation"]["displayed_findings"] = json!(count);
        report["presentation"]["omitted_findings"] = json!(total - count);
        report["presentation"]["collections"] = json!([{"path":"/findings","total":total,"prior":prior,"displayed":count,"remaining":total.saturating_sub(prior+count)}]);
        report["retrieval"]["next_cursor"] =
            if prior + count < total && report["retrieval"]["status"] == "available" {
                json!(format!(
                    "v1:{}:{}",
                    digest(
                        json!([report["retrieval"]["binding_sha256"], "/findings", null])
                            .to_string()
                            .as_bytes()
                    ),
                    prior + count
                ))
            } else {
                Value::Null
            };
    };
    refresh(
        report,
        0,
        if total == 0 {
            "complete"
        } else if max == 0 {
            "item_limit_zero"
        } else {
            "oversized_item"
        },
    );
    if over(cli, size(report)?) {
        refresh(report, 0, "mandatory_metadata_over_budget");
        size(report)?;
        return Ok(());
    }
    for finding in all.into_iter().skip(prior).take(max) {
        report["findings"].as_array_mut().unwrap().push(finding);
        let count = report["findings"].as_array().unwrap().len();
        refresh(
            report,
            count,
            if count == total { "complete" } else { "page" },
        );
        if over(cli, size(report)?) {
            report["findings"].as_array_mut().unwrap().pop();
            let count = count - 1;
            refresh(
                report,
                count,
                if count == 0 { "oversized_item" } else { "page" },
            );
            size(report)?;
            break;
        }
    }
    size(report)?;
    Ok(())
}

pub(super) fn retrieve(cli: &Cli, args: &InvestigationEvidenceArgs) -> Result<()> {
    let mut file = File::open(&args.artifact)?;
    if file.metadata()?.len() > args.artifact_max_bytes {
        return Err("Artifact exceeds read limit".into());
    }
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if (bytes.len() as u64).saturating_add(count as u64) > args.artifact_max_bytes {
            return Err("Artifact exceeds read limit".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let hash = digest(&bytes);
    if hash != args.expected_sha256 {
        return Err(
            "Artifact digest differs from the original report; retained evidence was changed"
                .into(),
        );
    }
    let retained: Value = serde_json::from_slice(&bytes)?;
    if retained["contract_version"] != 1 || retained["investigation_contract_version"] != 1 {
        return Err("Unsupported evidence artifact version".into());
    }
    for key in ["findings", "populations", "sequences"] {
        if !retained.get(key).is_some_and(Value::is_array) {
            return Err("Malformed artifact: required collections are absent".into());
        }
    }
    let full_report = report(&retained, &args.artifact, Some(&bytes));
    crate::investigation::validate_relations(&full_report, Some(&bytes))?;
    let allowed = [
        "/findings",
        "/records",
        "/populations",
        "/memberships",
        "/sequences",
        "/captured_inputs",
    ];
    if !allowed.contains(&args.collection.as_str())
        && !args.collection.starts_with("/memberships/")
        && !args.collection.starts_with("/sequences/")
    {
        return Err("Unsupported retained evidence collection".into());
    }
    let collection = retained
        .pointer(&args.collection)
        .and_then(Value::as_array)
        .ok_or("Collection is not a retained array")?;
    let selected: Vec<_> = collection
        .iter()
        .filter(|item| {
            args.id.as_ref().is_none_or(|id| {
                item["id"].as_str() == Some(id)
                    || item["population_id"].as_str() == Some(id)
                    || item
                        .pointer("/occurrence/evidence_ref/reference_id")
                        .and_then(Value::as_str)
                        == Some(id)
            })
        })
        .cloned()
        .collect();
    let base = binding(&retained, &hash);
    let query_binding = digest(
        json!([base, args.collection, args.id])
            .to_string()
            .as_bytes(),
    );
    let prior = if let Some(cursor) = &cli.report_cursor {
        let parts: Vec<_> = cursor.split(':').collect();
        if parts.len() != 3 || parts[0] != "v1" || parts[1] != query_binding {
            return Err(
                "Invalid cursor: artifact/profile/query/redaction/selector identity changed".into(),
            );
        }
        parts[2].parse::<usize>()?
    } else {
        0
    };
    if prior > selected.len() {
        return Err("Cursor is outside the retained collection".into());
    }
    let source_state = if args.verify_sources {
        verify_sources(&retained, args.artifact_max_bytes)
    } else {
        json!({"status":"not_requested","facts":"retained_snapshot","reason":"Current source files were not read"})
    };
    let max = if cli.complete_output {
        selected.len()
    } else {
        cli.report_max_items.unwrap_or(20)
    };
    let mut output = json!({"artifact_retrieval":{"contract_version":1,"artifact_sha256":hash,"binding_sha256":query_binding,"collection":args.collection,"id":args.id,"source_verification":source_state,"parse_passes":0,"correlation_passes":0,"total":selected.len(),"prior":prior,"displayed":0,"remaining":selected.len()-prior,"next_cursor":null,"status":"complete","items":[]}});
    let refresh = |output: &mut Value, count: usize, status: &str| {
        let section = &mut output["artifact_retrieval"];
        section["displayed"] = json!(count);
        section["remaining"] = json!(selected.len() - prior - count);
        section["status"] = json!(status);
        section["next_cursor"] = if prior + count < selected.len() {
            json!(format!("v1:{query_binding}:{}", prior + count))
        } else {
            Value::Null
        };
    };
    refresh(
        &mut output,
        0,
        if selected.is_empty() {
            "complete"
        } else if max == 0 {
            "item_limit_zero"
        } else {
            "oversized_item"
        },
    );
    let fits = |value: &Value| -> Result<bool> {
        let text = serde_json::to_string(value)?;
        Ok(!over(cli, (text.len() + 1, text.chars().count() + 1)))
    };
    if !fits(&output)? {
        refresh(&mut output, 0, "mandatory_metadata_over_budget");
    } else {
        for item in selected.iter().skip(prior).take(max) {
            output["artifact_retrieval"]["items"]
                .as_array_mut()
                .unwrap()
                .push(item.clone());
            let count = output["artifact_retrieval"]["items"]
                .as_array()
                .unwrap()
                .len();
            refresh(
                &mut output,
                count,
                if prior + count == selected.len() {
                    "complete"
                } else {
                    "page"
                },
            );
            if !fits(&output)? {
                output["artifact_retrieval"]["items"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
                refresh(
                    &mut output,
                    count - 1,
                    if count == 1 { "oversized_item" } else { "page" },
                );
                break;
            }
        }
    }
    if cli.redact && retained["report_metadata"]["evidence"]["redaction"]["applied"] != true {
        return Err("Retrieve an artifact created with --redact; original evidence cannot be presented as a redacted retained artifact".into());
    }
    if let Some(path) = &cli.output {
        if same_destination(path, &args.artifact, true) {
            return Err("Report destination conflicts with the retained artifact".into());
        }
        for input in retained["report_metadata"]["evidence"]["inputs"]
            .as_array()
            .unwrap()
        {
            if let Some(file) = input["file"].as_str()
                && same_destination(path, Path::new(file), true)
            {
                return Err("Report destination conflicts with a captured source".into());
            }
        }
        let mut temporary = stage(path)?;
        serde_json::to_writer(&mut temporary, &output)?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        temporary.persist_noclobber(path).map_err(
            |_| "Evidence retrieval saves only new report files; choose a new --output path",
        )?;
    }
    std::io::stdout()
        .lock()
        .write_all(format!("{}\n", serde_json::to_string(&output)?).as_bytes())?;
    Ok(())
}
fn verify_source(retained: &Value, ordinal: usize, input: &Value, remaining: &mut u64) -> Value {
    use sha2::{Digest, Sha256};
    let unavailable =
        |reason| json!({"input_ordinal":ordinal,"status":"unavailable","reason":reason});
    if retained["report_metadata"]["evidence"]["redaction"]["applied"] == true {
        return unavailable("Source paths omitted by redaction");
    }
    let Some(path) = retained
        .pointer("/report_metadata/evidence/query/execution/source_locations")
        .and_then(Value::as_array)
        .and_then(|paths| paths.get(ordinal))
        .and_then(Value::as_str)
    else {
        return unavailable("Capture-time native absolute source path is unavailable");
    };
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) => {
            return json!({"input_ordinal":ordinal,"status":if error.kind()==std::io::ErrorKind::NotFound{"missing"}else{"unavailable"}});
        }
    };
    let prefix = retained["processing"]["inputs"][ordinal]["capture"] == "prefix";
    let captured = input["bytes"].as_u64().unwrap();
    if prefix && captured == 0 {
        return json!({"input_ordinal":ordinal,"status":"prefix_empty","reason":"No bytes captured; current input equality is unavailable","facts":"retained_snapshot"});
    }
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut chunk = [0u8; 8192];
    loop {
        if prefix && bytes == captured {
            break;
        }
        if *remaining == 0 {
            return unavailable("Source verification byte limit");
        }
        let mut capacity = (*remaining).min(chunk.len() as u64) as usize;
        if prefix {
            capacity = capacity.min((captured - bytes).min(usize::MAX as u64) as usize);
        }
        let count = match file.read(&mut chunk[..capacity]) {
            Ok(count) => count,
            Err(_) => return unavailable("Source verification read failed"),
        };
        if count == 0 {
            break;
        }
        bytes += count as u64;
        *remaining -= count as u64;
        hash.update(&chunk[..count]);
    }
    let current = format!("{:x}", hash.finalize());
    let status = if prefix {
        if bytes < captured {
            "prefix_shortened"
        } else if current == input["sha256"] {
            "prefix_unchanged"
        } else {
            "prefix_changed"
        }
    } else if current == input["sha256"] && bytes == captured {
        "unchanged"
    } else {
        "changed"
    };
    json!({"input_ordinal":ordinal,"status":status,"current_sha256":current,"verified_bytes":bytes,"extent":if prefix{"consumed_prefix"}else{"current_file"},"facts":"retained_snapshot"})
}
fn verify_sources(retained: &Value, max_bytes: u64) -> Value {
    let inputs = retained["report_metadata"]["evidence"]["inputs"]
        .as_array()
        .unwrap();
    let mut remaining = max_bytes;
    let states: Vec<_> = inputs
        .iter()
        .enumerate()
        .map(|(ordinal, input)| verify_source(retained, ordinal, input, &mut remaining))
        .collect();
    json!({"status":"checked","inputs":states,"verified_bytes":max_bytes-remaining,"facts":"retained_snapshot","reason":"Current file identity does not rewrite retained evidence"})
}
