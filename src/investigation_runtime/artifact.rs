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
    let report = report.map(destination_identity).transpose()?;
    if artifact.exists() {
        return Err("Artifact destination already exists; choose a new file".into());
    }
    if let Some(report) = &report
        && same_destination(&artifact, report, false)
    {
        return Err("Artifact and report destinations must be separate".into());
    }
    for source in sources {
        let source = destination_identity(source)?;
        if same_destination(&artifact, &source, false)
            || report
                .as_ref()
                .is_some_and(|report| same_destination(report, &source, true))
        {
            return Err(
                "Artifact/report destination conflicts with an input, profile or cancellation file"
                    .into(),
            );
        }
    }
    Ok(())
}
// A bounded walk cannot enumerate every profile. Reserve TOML destinations
// under its root as well, even if discovery never reaches them.
pub(super) fn protect_profile_directory(
    artifact: &Path,
    report: Option<&Path>,
    directory: &Path,
) -> Result<()> {
    let directory = destination_identity(directory)?;
    for path in std::iter::once(artifact).chain(report) {
        let absolute = std::env::current_dir()?.join(path);
        let resolved = destination_identity(path)?;
        for path in [&absolute, &resolved] {
            if !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
            {
                continue;
            }
            let parent = destination_identity(path.parent().unwrap_or(Path::new(".")))?;
            for ancestor in parent.ancestors() {
                if same_destination(ancestor, &directory, false) {
                    return Err("Artifact/report destination conflicts with the profile discovery directory".into());
                }
            }
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
struct LimitedWriter<W> {
    writer: W,
    bytes: u64,
    limit: u64,
    exceeded: bool,
}
impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.limit.saturating_sub(self.bytes) {
            self.exceeded = true;
            return Err(std::io::Error::other("artifact byte limit exceeded"));
        }
        let count = self.writer.write(bytes)?;
        self.bytes += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}
pub(super) fn write_json(
    value: &Value,
    captures: &[Option<crate::streaming::Capture>],
    file: &mut File,
    limit: u64,
) -> Result<bool> {
    let mut writer = LimitedWriter {
        writer: std::io::BufWriter::new(file),
        bytes: 0,
        limit,
        exceeded: false,
    };
    let result = write_artifact(value, captures, &mut writer);
    match result {
        Ok(()) => {
            writer.flush()?;
            Ok(true)
        }
        Err(_) if writer.exceeded => Ok(false),
        Err(error) => Err(error),
    }
}
fn write_artifact(
    value: &Value,
    captures: &[Option<crate::streaming::Capture>],
    writer: &mut impl Write,
) -> Result<()> {
    writer.write_all(b"{")?;
    for (index, (key, value)) in value
        .as_object()
        .ok_or("Artifact must be an object")?
        .iter()
        .enumerate()
    {
        if index != 0 {
            writer.write_all(b",")?;
        }
        serde_json::to_writer(&mut *writer, key)?;
        writer.write_all(b":")?;
        if key != "captured_inputs" {
            serde_json::to_writer(&mut *writer, value)?;
            continue;
        }
        let rows = value.as_array().ok_or("Captures must be an array")?;
        if rows.len() != captures.len() {
            return Err("Capture count differs".into());
        }
        writer.write_all(b"[")?;
        for (ordinal, row) in rows.iter().enumerate() {
            if ordinal != 0 {
                writer.write_all(b",")?;
            }
            writer.write_all(b"{")?;
            for (index, (key, value)) in row
                .as_object()
                .ok_or("Capture must be an object")?
                .iter()
                .enumerate()
            {
                if index != 0 {
                    writer.write_all(b",")?;
                }
                serde_json::to_writer(&mut *writer, key)?;
                writer.write_all(b":")?;
                if key != "data" || row["data_omitted"] != false {
                    serde_json::to_writer(&mut *writer, value)?;
                    continue;
                }
                let capture = captures[ordinal]
                    .as_ref()
                    .ok_or("Missing captured source")?;
                let mut source = capture.file.reopen()?;
                if capture.utf8 {
                    crate::streaming::write_json_string(source, writer)?;
                } else {
                    writer.write_all(b"[")?;
                    let mut buffer = [0u8; crate::streaming::CHUNK_BYTES];
                    let mut first = true;
                    loop {
                        let count = source.read(&mut buffer)?;
                        if count == 0 {
                            break;
                        }
                        for byte in &buffer[..count] {
                            if !first {
                                writer.write_all(b",")?;
                            }
                            first = false;
                            serde_json::to_writer(&mut *writer, byte)?;
                        }
                    }
                    writer.write_all(b"]")?;
                }
            }
            writer.write_all(b"}")?;
        }
        writer.write_all(b"]")?;
    }
    writer.write_all(b"}")?;
    Ok(())
}
pub(super) fn file_digest(file: &mut File) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::{Seek, SeekFrom};
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(format!("{:x}", hash.finalize()))
}
fn opaque(value: &mut Value) {
    if let Some(text) = value.as_str() {
        *value = json!(format!("opaque-{}", digest(text.as_bytes())));
    }
}
fn redact_claim_verification(verification: &mut Value) {
    fn identifier(value: &mut Value) {
        if !value.as_str().is_some_and(|s| {
            s.strip_prefix("opaque-")
                .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        }) {
            opaque(value);
        }
    }
    fn identifiers(value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    if matches!(key.as_str(), "id" | "finding_id") {
                        identifier(child);
                    } else if key.ends_with("_claim_ids") || key == "unresolved_question_ids" {
                        for id in child.as_array_mut().into_iter().flatten() {
                            identifier(id);
                        }
                    } else {
                        identifiers(child);
                    }
                }
            }
            Value::Array(values) => values.iter_mut().for_each(identifiers),
            _ => (),
        }
    }
    for question in verification["questions"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        *question = json!("[REDACTED QUESTION]");
    }
    for check in verification["checks"].as_array_mut().into_iter().flatten() {
        check["label"] = Value::Null;
        if check["identity"].is_object() {
            check["identity"]["expected"] = Value::Null;
        }
    }
    for unknown in verification["unresolved"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        unknown["reason"] = json!("[REDACTED REASON]");
    }
    if verification["final_review"].is_object() {
        for issue in verification["final_review"]["issues"]
            .as_array_mut()
            .into_iter()
            .flatten()
        {
            *issue = json!("[REDACTED REVIEW ISSUE]");
        }
        // Additional caller-defined review dimensions may contain private text.
        verification["final_review"]["acknowledged_prose_checks"] = Value::Null;
    }
    identifiers(verification);
}
pub(super) fn checklist(artifact: &Value) -> &Value {
    artifact
        .get("question_checklist")
        .unwrap_or(&artifact["report_metadata"]["evidence"]["query"]["command"]["Investigate"])
}
pub(super) fn redact(artifact: &mut Value) {
    let original = checklist(artifact);
    let questions: Vec<_> = original["question"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|_| "[REDACTED QUESTION]")
        .collect();
    let required: Vec<_> = original["required_relationship"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|id| format!("opaque-{}", digest(id.as_bytes())))
        .collect();
    artifact["question_checklist"] = json!({"question":questions,"required_relationship":required});
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
        if finding["id"]
            .as_str()
            .is_some_and(|id| id.contains("-chain-"))
            && finding["kind"] == "unknown"
        {
            finding["details"]["reason"] = json!(
                "Relationship recognition is unavailable for some or all roots; private selectors and rule labels omitted by redaction."
            );
        }
        if let Some(status) = finding["details"]["resource_status"].as_str() {
            finding["claim"] = json!(match status {
                "fingerprint_match" =>
                    "Resource matches a profile-declared fingerprint; private labels, digests and dimensions are omitted by redaction. This observation does not establish displayed content or cause.",
                "different_fingerprint" =>
                    "Resource has different bytes from the profile-declared fingerprints; private dimensions are omitted by redaction. This observation does not establish displayed content or cause.",
                "unconfigured_fingerprint" =>
                    "Resource has a digest but no profile-declared fingerprints; private dimensions are omitted by redaction. This observation does not establish displayed content or cause.",
                "unavailable" =>
                    "Resource digest is unavailable or ambiguous; private dimensions are omitted by redaction. This observation does not establish displayed content or cause.",
                "empty_manifest" =>
                    "Manifest lists no resources; absent displayed content cannot be established from this list.",
                _ =>
                    "Resource observation; private details are omitted by redaction. This observation does not establish displayed content or cause.",
            });
            if finding["details"].get("viewport").is_some() {
                finding["details"]["viewport"] = Value::Null;
            }
        }
        if finding["details"]["fingerprint"].is_object() {
            finding["details"]["fingerprint"] = Value::Null;
        }
        if let Some(chain) = finding["details"].get_mut("event_chain") {
            for key in ["id", "title", "root_event_name"] {
                opaque(&mut chain[key]);
            }
            for key in ["scope_fields"] {
                for value in chain[key].as_array_mut().unwrap() {
                    opaque(value);
                }
            }
            for step in chain["steps"].as_array_mut().unwrap() {
                for key in ["id", "label", "event_type"] {
                    opaque(&mut step[key]);
                }
                if step["from"] != "$root" {
                    opaque(&mut step["from"]);
                }
                for value in step["join_fields"].as_array_mut().unwrap() {
                    opaque(value);
                }
                if let Some(recognition) = step.get_mut("recognition") {
                    opaque(&mut recognition["role"]);
                    for rule in recognition["rule_ids"].as_array_mut().unwrap() {
                        opaque(rule);
                    }
                    for selector in recognition["failed_selectors"].as_array_mut().unwrap() {
                        opaque(&mut selector["field"]);
                        selector["equals"] = json!(format!(
                            "opaque-{}",
                            digest(selector["equals"].to_string().as_bytes())
                        ));
                    }
                }
                for join in step["join_samples"].as_array_mut().unwrap() {
                    if let Some(interval) = join.get_mut("endpoint_interval") {
                        opaque(&mut interval["start_operation"]);
                        opaque(&mut interval["end_operation"]);
                    }
                }
                for outcome in step["outcomes"].as_array_mut().unwrap() {
                    opaque(&mut outcome["level"]);
                    if !outcome["value"].is_null() {
                        outcome["value"] = json!(format!(
                            "opaque-{}",
                            digest(outcome["value"].to_string().as_bytes())
                        ));
                    }
                }
            }
            fn redact_excerpts(value: &mut Value) {
                if let Some(map) = value.as_object_mut() {
                    if map.contains_key("field")
                        && map
                            .get("value")
                            .is_some_and(|v| !v.is_string() && !v.is_null())
                    {
                        let value = &map["value"];
                        let masked =
                            json!(format!("opaque-{}", digest(value.to_string().as_bytes())));
                        map.insert("value".into(), masked);
                    }
                    if map.contains_key("occurrence") && map.contains_key("text") {
                        map.insert("text".into(), json!("[REDACTED SOURCE]"));
                        map.insert("omitted_characters".into(), json!(0));
                        if map.contains_key("fields") {
                            map.insert("fields".into(), json!({}));
                        }
                        map.insert(
                            "verification".into(),
                            super::findings::verification(true, "not_applicable"),
                        );
                    }
                    for child in map.values_mut() {
                        redact_excerpts(child);
                    }
                } else if let Some(values) = value.as_array_mut() {
                    for child in values {
                        redact_excerpts(child);
                    }
                }
            }
            redact_excerpts(chain);
        }
        let arithmetic = finding["verification"]["arithmetic"]
            .as_str()
            .unwrap()
            .to_owned();
        finding["verification"] = super::findings::verification(true, &arithmetic);
        for excerpt in finding["evidence"].as_array_mut().unwrap() {
            excerpt["text"] = json!("[REDACTED SOURCE]");
            if excerpt.get("fields").is_some() {
                excerpt["fields"] = json!({});
            }
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
pub(super) fn report(artifact: &Value, path: &Path, hash: Option<&str>) -> Value {
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
    report["artifact"] = json!({"contract_version":1,"status":if hash.is_some(){if artifact["processing"]["status"]=="complete"{"complete"}else{"partial"}}else{"unavailable"},"location":if hash.is_some(){Some(if artifact["content"] == "redacted" { "[REDACTED ARTIFACT PATH]".into() } else {crate::evidence::path_label(path)})}else{None},"stored_sha256":hash,"content":if hash.is_some(){artifact["content"].clone()}else{json!("unavailable")},"retention":artifact["retention"],"verification":artifact["verification"]});
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
    report["retrieval"] = json!({"interface":"artifact","contract_version":1,"status":if hash.is_some(){"available"}else{"unavailable"},"binding_sha256":hash.map(|hash|binding(artifact,hash)),"next_cursor":null,"targets":targets,"reason":if hash.is_some(){None}else{Some("Artifact byte limit exceeded")}});
    report["presentation"] = json!({"status":"complete","budget_bytes":null,"budget_characters":null,"budget_items":null,"serialized_bytes":null,"serialized_characters":null,"total_findings":artifact["findings"].as_array().unwrap().len(),"displayed_findings":artifact["findings"].as_array().unwrap().len(),"omitted_findings":0,"collections":[{"path":"/findings","total":artifact["findings"].as_array().unwrap().len(),"prior":0,"displayed":artifact["findings"].as_array().unwrap().len(),"remaining":0}],"units":"serialized_UTF8_bytes_Unicode_scalars_and_atomic_finding_bundles"});
    report
}
pub(super) fn unavailable(report: &mut Value, reason: &str) {
    report["processing"]["status"] = json!("partial");
    if report["processing"]["stop"].is_null() {
        report["processing"]["stop"] = json!({"stage":"artifact_write","reason":if reason.contains("cancelled"){"cancelled"}else if reason.contains("limit"){"storage_limit"}else{"io_error"},"limit_name":if reason.contains("limit"){Some("artifact_bytes")}else{None},"scope_ids":report["scopes"].as_array().unwrap().iter().map(|scope|scope["id"].clone()).collect::<Vec<_>>()});
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
pub(super) fn source_verification_unavailable(
    artifact: &mut Value,
    inputs: &std::collections::BTreeSet<u64>,
    reason: &str,
) {
    fn loss(value: &mut Value, reason: &str) {
        match value {
            Value::Object(map) => {
                if let Some(verification) = map.get_mut("verification") {
                    verification["source_and_rules"] = json!("unavailable");
                    let losses = verification["losses"].as_array_mut().unwrap();
                    if !losses.iter().any(|loss| loss == reason) {
                        losses.push(json!(reason));
                    }
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
    artifact["verification"]["source_and_rules"] = json!("unavailable");
    let losses = artifact["verification"]["losses"].as_array_mut().unwrap();
    if !losses.iter().any(|loss| loss == reason) {
        losses.push(json!(reason));
    }
    for record in artifact["records"].as_array_mut().unwrap() {
        if record["occurrence"]["input_ordinal"]
            .as_u64()
            .is_some_and(|ordinal| inputs.contains(&ordinal))
        {
            loss(record, reason);
        }
    }
    for finding in artifact["findings"].as_array_mut().unwrap() {
        if finding["scope_id"]
            .as_str()
            .and_then(|scope| scope.strip_prefix("scope-"))
            .and_then(|ordinal| ordinal.parse().ok())
            .is_some_and(|ordinal| inputs.contains(&ordinal))
        {
            loss(finding, reason);
        }
    }
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
    let sizes = size(report, cli)?;
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
        size(report, cli)?;
    }
    Ok(())
}
fn size(report: &mut Value, cli: &Cli) -> Result<(usize, usize)> {
    report["guidance"] = super::guidance::build(report, cli);
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
    if over(cli, size(report, cli)?) {
        refresh(report, 0, "mandatory_metadata_over_budget");
        size(report, cli)?;
        return Ok(());
    }
    // Serialize the fixed metadata once, and each atomic finding once. Only
    // the small presentation/cursor fields change as the page grows.
    let measure = |value: &Value| -> Result<(usize, usize)> {
        let text = serde_json::to_string(value)?;
        Ok((text.len(), text.chars().count()))
    };
    let header = measure(report)?;
    let presentation = measure(&report["presentation"])?;
    let cursor = measure(&report["retrieval"]["next_cursor"])?;
    let guidance = measure(&report["guidance"])?;
    let fixed = (
        header.0 - presentation.0 - cursor.0 - guidance.0 + 1,
        header.1 - presentation.1 - cursor.1 - guidance.1 + 1,
    );
    let exact = |report: &mut Value, payload: (usize, usize)| -> Result<(usize, usize)> {
        report["guidance"] = super::guidance::build(report, cli);
        let guidance = measure(&report["guidance"])?;
        for _ in 0..8 {
            let presentation = measure(&report["presentation"])?;
            let cursor = measure(&report["retrieval"]["next_cursor"])?;
            let sizes = (
                fixed.0 + presentation.0 + cursor.0 + guidance.0 + payload.0,
                fixed.1 + presentation.1 + cursor.1 + guidance.1 + payload.1,
            );
            if report["presentation"]["serialized_bytes"] == sizes.0
                && report["presentation"]["serialized_characters"] == sizes.1
            {
                return Ok(sizes);
            }
            report["presentation"]["serialized_bytes"] = json!(sizes.0);
            report["presentation"]["serialized_characters"] = json!(sizes.1);
        }
        Err("Could not stabilize serialized report size".into())
    };
    let mut displayed = Vec::new();
    let mut payload = (0usize, 0usize);
    for finding in all.into_iter().skip(prior).take(max) {
        let item = measure(&finding)?;
        let comma = usize::from(!displayed.is_empty());
        let candidate = (payload.0 + item.0 + comma, payload.1 + item.1 + comma);
        let count = displayed.len() + 1;
        refresh(
            report,
            count,
            if prior + count == total {
                "complete"
            } else {
                "page"
            },
        );
        if over(cli, exact(report, candidate)?) {
            refresh(
                report,
                count - 1,
                if count == 1 { "oversized_item" } else { "page" },
            );
            exact(report, payload)?;
            break;
        }
        payload = candidate;
        displayed.push(finding);
    }
    report["findings"] = Value::Array(displayed);
    size(report, cli)?;
    Ok(())
}

pub(super) fn retrieve(cli: &Cli, args: &InvestigationEvidenceArgs) -> Result<()> {
    use super::indexed_artifact::{IndexedArtifact, Limits};
    use std::io::{Seek, SeekFrom};
    if args.compact && (args.collection != "/chain-bundles" || args.id.is_none()) {
        return Err("--compact requires --collection /chain-bundles and --id".into());
    }
    let (snapshot, hash) = crate::streaming::verified_snapshot(
        &args.artifact,
        args.artifact_max_bytes,
        &args.expected_sha256,
    )?;
    let mut indexed = IndexedArtifact::open(
        snapshot.reopen()?,
        Limits {
            item_bytes: args.artifact_max_item_bytes,
            index_bytes: args.artifact_max_index_bytes,
            state_bytes: args.artifact_max_state_bytes,
            elapsed_ms: args.artifact_max_validation_ms,
            work_units: args.artifact_max_validation_work,
        },
    )?;
    indexed.validate()?;
    let retained = &indexed.base;
    if cli.redact && retained["report_metadata"]["evidence"]["redaction"]["applied"] != true {
        return Err("Retrieve an artifact created with --redact; original evidence cannot be presented as a redacted retained artifact".into());
    }
    if args.draft_claims {
        let draft = super::claims::draft_indexed(&indexed, args.id.as_deref())?;
        let path = cli
            .output
            .as_ref()
            .ok_or("--draft-claims requires --output")?;
        if same_destination(path, &args.artifact, true) {
            return Err("Claim draft destination conflicts with the retained artifact".into());
        }
        for input in retained["report_metadata"]["evidence"]["inputs"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if input["file"]
                .as_str()
                .is_some_and(|file| same_destination(path, Path::new(file), true))
            {
                return Err("Claim draft destination conflicts with a captured source".into());
            }
        }
        let mut temporary = stage(path)?;
        serde_json::to_writer_pretty(&mut temporary, &draft)?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        temporary
            .persist_noclobber(path)
            .map_err(|_| "Claim drafts save only new files; choose a new --output path")?;
        println!(
            "{}",
            json!({"status":"draft_requires_review","claims_path":crate::evidence::path_label(path),"candidate_claims":draft["claims"].as_array().map_or(0,Vec::len),"limitations":["Draft expectations come from retained evidence, not independent truth. Match the question and source semantics, remove unrelated claims, map checklist IDs, and acknowledge final review only after inspection. Missing endings are not drafted as absence claims."],"next_action":{"kind":"review_then_verify_claims","argv":[std::env::current_exe()?.to_string_lossy(),"evidence",crate::evidence::path_label(&args.artifact),"--expected-sha256",hash,"--artifact-max-bytes",args.artifact_max_bytes.to_string(),"--claims",crate::evidence::path_label(path)]}})
        );
        return Ok(());
    }
    let allowed = [
        "/findings",
        "/records",
        "/populations",
        "/memberships",
        "/sequences",
        "/captured_inputs",
        "/chain-bundles",
    ];
    if !allowed.contains(&args.collection.as_str())
        && !args.collection.starts_with("/memberships/")
        && !args.collection.starts_with("/sequences/")
    {
        return Err("Unsupported retained evidence collection".into());
    }
    let bundles = args.collection == "/chain-bundles";
    let collection = indexed
        .root
        .pointer(if bundles {
            "/findings"
        } else {
            &args.collection
        })?
        .filter(|node| node.is_array())
        .ok_or("Collection is not a retained array")?;
    let selected = |node: &crate::json_index::Node| -> Result<bool> {
        if bundles && node.pointer("/details/event_chain")?.is_none() {
            return Ok(false);
        }
        let Some(id) = &args.id else {
            return Ok(true);
        };
        for pointer in if bundles {
            &["/id"][..]
        } else {
            &[
                "/id",
                "/population_id",
                "/occurrence/evidence_ref/reference_id",
            ][..]
        } {
            if let Some(value) = node.pointer(pointer)?
                && value.is_string()
                && value.read(65536)?.as_str() == Some(id.as_str())
            {
                return Ok(true);
            }
        }
        Ok(false)
    };
    // Finding IDs are unique and validated before this direct offset lookup.
    let exact = if matches!(args.collection.as_str(), "/findings" | "/chain-bundles") {
        args.id
            .as_deref()
            .map(|id| indexed.find_node(id))
            .transpose()?
    } else {
        None
    };
    let nodes = || -> Box<dyn Iterator<Item = Result<crate::json_index::Node>>> {
        match &exact {
            Some(node) => Box::new(node.clone().into_iter().map(Ok)),
            None => Box::new(collection.children()),
        }
    };
    let total = if args.id.is_none() && !bundles {
        collection.len()
    } else {
        let mut count = 0u64;
        for node in nodes() {
            indexed.checkpoint()?;
            count += u64::from(selected(&node?)?);
        }
        count
    };
    let mut claim_verification = args
        .claims
        .as_ref()
        .map(|path| super::claims::verify_indexed(&indexed, path))
        .transpose()?;
    let claims_failed = claim_verification.as_ref().is_some_and(|value| {
        value["status"] != "verified" && value["status"] != "verified_with_unknowns"
    });
    let base = binding(retained, &hash);
    let query = if !args.compact && claim_verification.is_none() {
        json!([base, args.collection, args.id])
    } else {
        json!([
            base,
            args.collection,
            args.id,
            args.compact,
            claim_verification
        ])
    };
    let query_binding = digest(query.to_string().as_bytes());
    if retained["content"] == "redacted"
        && let Some(verification) = &mut claim_verification
    {
        redact_claim_verification(verification);
    }
    let prior = if let Some(cursor) = &cli.report_cursor {
        let parts: Vec<_> = cursor.split(':').collect();
        if parts.len() != 3 || parts[0] != "v1" || parts[1] != query_binding {
            return Err(
                "Invalid cursor: artifact/profile/query/redaction/selector identity changed".into(),
            );
        }
        parts[2].parse::<u64>()?
    } else {
        0
    };
    if prior > total {
        return Err("Cursor is outside the retained collection".into());
    }
    let source_state = if args.verify_sources {
        verify_sources(retained, args.artifact_max_bytes)
    } else {
        json!({"status":"not_requested","facts":"retained_snapshot","reason":"Current source files were not read"})
    };
    let max = if cli.complete_output {
        total
    } else {
        cli.report_max_items.unwrap_or(20) as u64
    };
    let mut output = json!({"artifact_retrieval":{"contract_version":1,"artifact_sha256":hash,"binding_sha256":query_binding,"collection":args.collection,"id":args.id,"source_verification":source_state,"parse_passes":0,"correlation_passes":0,"total":total,"prior":prior,"displayed":0,"remaining":total-prior,"next_cursor":null,"status":"complete","items":[]}});
    if args.compact {
        output["artifact_retrieval"]["view"] = json!("compact");
    }
    let mut declared_questions: Vec<String> = claim_verification
        .as_ref()
        .and_then(|v| v["questions"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    let mut declared_required: Vec<String> = claim_verification
        .as_ref()
        .and_then(|v| v["required_claim_ids"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    let checklist = checklist(retained);
    if declared_questions.is_empty() {
        declared_questions = checklist["question"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
    }
    let original_required: Vec<String> = checklist["required_relationship"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    for id in original_required {
        if !declared_required.contains(&id) {
            declared_required.push(id);
        }
    }
    let mut workflow = workflow_indexed(&indexed, &declared_questions, &declared_required)?;
    if let Some(verification) = claim_verification {
        workflow["declared_claims"] = verification["status"].clone();
        workflow["claim_results"] = json!(
            verification["checks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| json!({"id":c["id"],"status":c["status"],"assurance":c["assurance"]}))
                .collect::<Vec<_>>()
        );
        let missing: Vec<_> = declared_required
            .iter()
            .filter(|id| {
                !verification["unresolved_required_claim_ids"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|u| u == *id)
                    && !verification["checks"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|c| c["id"] == **id)
            })
            .collect();
        if !missing.is_empty() {
            workflow["declared_claims"] = json!("needs_revision");
        }
        workflow["missing_required_claim_ids"] = json!(missing);
        workflow["unresolved_required_claim_ids"] =
            verification["unresolved_required_claim_ids"].clone();
        workflow["requirement_results"] = verification["requirement_results"].clone();
        output["artifact_retrieval"]["claim_verification"] = verification;
    }
    output["artifact_retrieval"]["workflow"] = workflow;
    let selected_finding_id = if matches!(args.collection.as_str(), "/findings" | "/chain-bundles")
    {
        match args.id.as_deref() {
            Some(id)
                if indexed
                    .find_node(id)?
                    .map(|n| n.pointer("/details/event_chain"))
                    .transpose()?
                    .flatten()
                    .is_some() =>
            {
                Some(id)
            }
            _ => None,
        }
    } else {
        None
    };
    let has_chains = output["artifact_retrieval"]["workflow"]["chain_finding_ids"]
        .as_array()
        .is_some_and(|ids| !ids.is_empty());
    let refresh = |output: &mut Value, count: u64, status: &str| {
        let section = &mut output["artifact_retrieval"];
        section["displayed"] = json!(count);
        section["remaining"] = json!(total - prior - count);
        section["status"] = json!(status);
        section["next_cursor"] = if prior + count < total {
            json!(format!("v1:{query_binding}:{}", prior + count))
        } else {
            Value::Null
        };
        section["workflow"]["evidence_delivery"] = json!({"collection":args.collection,"finding_id":selected_finding_id,"delivered_this_page":count,"omitted_this_collection":total-prior-count,"prior_items":prior});
        let mut argv = vec![
            std::env::current_exe()
                .ok()
                .and_then(|p| p.to_str().map(str::to_owned))
                .unwrap_or_else(|| "log-analyzer".into()),
            "evidence".into(),
            crate::evidence::path_label(&args.artifact),
            "--expected-sha256".into(),
            hash.clone(),
            "--artifact-max-bytes".into(),
            args.artifact_max_bytes.to_string(),
        ];
        let draft_base = argv.clone();
        argv.extend([
            "--collection".into(),
            args.collection.clone(),
            "--report-max-items".into(),
            max.clamp(1, 5).to_string(),
        ]);
        if let Some(id) = &args.id {
            argv.extend(["--id".into(), id.clone()]);
        }
        if args.compact {
            argv.push("--compact".into());
        }
        if let Some(claims) = &args.claims {
            argv.extend(["--claims".into(), crate::evidence::path_label(claims)]);
        }
        if total > prior && count == 0 {
            section["workflow"]["next_action"] = json!({"kind":"retrieval_stopped","argv":null,"reason":"No evidence delivered within this page budget. Stop; revise limits only within the agreed budget. Do not loop the unchanged query."});
        } else if let Some(cursor) = section["next_cursor"].as_str() {
            argv.extend(["--report-cursor".into(), cursor.into()]);
            section["workflow"]["next_action"] =
                json!({"kind":"retrieve_omitted_evidence","argv":argv});
        } else if args.claims.is_none()
            && retained["content"] == "original"
            && count > 0
            && has_chains
        {
            let mut argv = draft_base;
            if let Some(id) = selected_finding_id {
                argv.extend(["--id".into(), id.into()]);
            }
            argv.extend([
                "--draft-claims".into(),
                "--output".into(),
                crate::evidence::path_label(&args.artifact.with_file_name("claims-draft.json")),
            ]);
            section["workflow"]["next_action"] = json!({"kind":"review_then_draft_claims","argv":argv,"reason":"Inspect delivered subjects and gaps first. Save editable assertions, map question checklist IDs, remove unrelated intervals, then run the returned verification argv. Drafting does not establish truth or review."});
        } else {
            let reason = if retained["content"] == "redacted" {
                "Redacted evidence cannot produce source-bound drafts. Preserve unknowns and report identity and source-verification limits."
            } else {
                "Inspect exact subjects, terminal/contrary/later events and omissions. Declare source-bound claims after review; do not repeat completed retrieval without a specific remaining gap."
            };
            section["workflow"]["next_action"] = json!({"kind":"inspect_delivered_evidence_and_verification","argv":null,"reason":reason});
        }
    };
    refresh(
        &mut output,
        0,
        if total == prior {
            "complete"
        } else if max == 0 {
            "item_limit_zero"
        } else {
            "oversized_item"
        },
    );
    let header = serde_json::to_string(&output)?;
    let mut payload = tempfile::tempfile()?;
    let mut payload_size = (0usize, 0usize);
    let mut displayed = 0u64;
    if over(cli, (header.len() + 1, header.chars().count() + 1)) {
        refresh(&mut output, 0, "mandatory_metadata_over_budget");
    } else {
        let mut position = 0u64;
        for node in nodes() {
            indexed.checkpoint()?;
            let node = node?;
            if !selected(&node)? {
                continue;
            }
            position += 1;
            if position <= prior {
                continue;
            }
            if displayed >= max {
                break;
            }
            // A single atomic item has an explicit materialization allowance.
            // Larger membership/sequence arrays remain retrievable by their
            // nested collection pointers rather than as one enormous item.
            if node.byte_len() > args.artifact_max_item_bytes {
                refresh(
                    &mut output,
                    displayed,
                    if displayed == 0 {
                        "oversized_item"
                    } else {
                        "page"
                    },
                );
                break;
            }
            let mut item = node.read(args.artifact_max_item_bytes)?;
            if bundles {
                item = json!({"id":item["id"],"scope_id":item["scope_id"],"chain":item["details"]["event_chain"],"evidence":item["evidence"],"limitations":item["limitations"],"verification":item["verification"]});
                if args.compact {
                    compact_bundle(&mut item);
                }
            }
            let text = serde_json::to_string(&item)?;
            let comma = usize::from(displayed != 0);
            let candidate = (
                payload_size
                    .0
                    .checked_add(text.len() + comma)
                    .ok_or("retrieval byte count overflow")?,
                payload_size
                    .1
                    .checked_add(text.chars().count() + comma)
                    .ok_or("retrieval character count overflow")?,
            );
            refresh(
                &mut output,
                displayed + 1,
                if prior + displayed + 1 == total {
                    "complete"
                } else {
                    "page"
                },
            );
            let header = serde_json::to_string(&output)?;
            if over(
                cli,
                (
                    header.len().saturating_add(candidate.0).saturating_add(1),
                    header
                        .chars()
                        .count()
                        .saturating_add(candidate.1)
                        .saturating_add(1),
                ),
            ) {
                refresh(
                    &mut output,
                    displayed,
                    if displayed == 0 {
                        "oversized_item"
                    } else {
                        "page"
                    },
                );
                break;
            }
            if candidate.0 as u64
                > args
                    .artifact_max_bytes
                    .saturating_mul(8)
                    .max(16 * 1024 * 1024)
            {
                return Err("retrieval output disk limit exceeded".into());
            }
            if comma != 0 {
                payload.write_all(b",")?;
            }
            payload.write_all(text.as_bytes())?;
            payload_size = candidate;
            displayed += 1;
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
            .ok_or("missing retained inputs")?
        {
            if let Some(file) = input["file"].as_str()
                && same_destination(path, Path::new(file), true)
            {
                return Err("Report destination conflicts with a captured source".into());
            }
        }
    }
    let mut delivery = if let Some(path) = &cli.output {
        stage(path)?
    } else {
        tempfile::NamedTempFile::new()?
    };
    delivery.write_all(b"{\"artifact_retrieval\":{")?;
    for (ordinal, (key, value)) in output["artifact_retrieval"]
        .as_object()
        .ok_or("invalid retrieval envelope")?
        .iter()
        .enumerate()
    {
        if ordinal != 0 {
            delivery.write_all(b",")?;
        }
        serde_json::to_writer(&mut delivery, key)?;
        delivery.write_all(b":")?;
        if key == "items" {
            delivery.write_all(b"[")?;
            payload.seek(SeekFrom::Start(0))?;
            std::io::copy(&mut payload, &mut delivery)?;
            delivery.write_all(b"]")?;
        } else {
            serde_json::to_writer(&mut delivery, value)?;
        }
    }
    delivery.write_all(b"}}\n")?;
    let mut delivered = if let Some(path) = &cli.output {
        delivery.as_file().sync_all()?;
        delivery.persist_noclobber(path).map_err(
            |_| "Evidence retrieval saves only new report files; choose a new --output path",
        )?
    } else {
        delivery.reopen()?
    };
    delivered.seek(SeekFrom::Start(0))?;
    std::io::copy(&mut delivered, &mut std::io::stdout().lock())?;
    if claims_failed {
        return Err(
            "Claim verification failed or unavailable; inspect claim_verification before answering"
                .into(),
        );
    }
    Ok(())
}

fn compact_bundle(bundle: &mut Value) {
    fn compact(value: &mut Value) {
        match value {
            Value::Object(object) => {
                if let Some(fields) = object.remove("fields") {
                    object.insert(
                        "omitted_fields_bytes".into(),
                        json!(fields.to_string().len()),
                    );
                    object.insert("record_retrieval".into(), json!({"collection":"/records","id":object.get("occurrence").and_then(|o|o.pointer("/evidence_ref/reference_id"))}));
                }
                for child in object.values_mut() {
                    compact(child);
                }
            }
            Value::Array(items) => {
                for item in items {
                    compact(item);
                }
            }
            _ => {}
        }
    }
    compact(bundle);
    // Excerpts are already retained once in the top-level evidence inventory.
    if let Some(steps) = bundle["chain"]["steps"].as_array_mut() {
        for step in steps {
            for key in ["evidence", "ambiguous_evidence"] {
                if let Some(items) = step[key].as_array_mut() {
                    for item in items {
                        *item = json!({"occurrence":item["occurrence"]});
                    }
                }
            }
        }
    }
    bundle["view_limitations"] = json!([
        "Payload fields omitted; retrieve /records by reference ID before asserting field semantics. Existing sample omissions still apply. This view makes no new joins."
    ]);
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
    let states: Vec<_> = retained["processing"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(ordinal, progress)| {
            if progress["capture"] == "unread" {
                return json!({"input_ordinal":ordinal,"status":"unavailable","reason":"Source was not captured; no retained snapshot to verify","facts":"retained_snapshot"});
            }
            match inputs.get(ordinal) {
                Some(input) => verify_source(retained, ordinal, input, &mut remaining),
                None => json!({"input_ordinal":ordinal,"status":"unavailable","reason":"Retained source manifest is unavailable","facts":"retained_snapshot"}),
            }
        })
        .collect();
    json!({"status":"checked","inputs":states,"verified_bytes":max_bytes-remaining,"facts":"retained_snapshot","reason":"Current file identity does not rewrite retained evidence"})
}

fn workflow_indexed(
    indexed: &super::indexed_artifact::IndexedArtifact,
    questions: &[String],
    required: &[String],
) -> Result<Value> {
    let mut workflow = super::guidance::workflow(&indexed.base, questions, required, &[]);
    let mut chain_ids = Vec::new();
    let mut chain_count = 0u64;
    let mut root_gaps = Vec::new();
    let mut step_gaps = Vec::new();
    let mut gap_count = 0u64;
    let mut sample_bytes = 0u64;
    for node in indexed
        .root
        .field("findings")?
        .ok_or("missing findings")?
        .children()
    {
        indexed.checkpoint()?;
        let node = node?;
        let chain = node.pointer("/details/event_chain")?.is_some();
        if !chain && node.pointer("/details/gap_classification")?.is_none() {
            continue;
        }
        let finding = node.read(indexed.limits.item_bytes)?;
        if chain {
            chain_count += 1;
            if chain_ids.len() < 5 {
                chain_ids.push(finding["id"].clone());
            }
        }
        let mut part = indexed.base.clone();
        part["findings"] = json!([finding]);
        let sample = super::guidance::workflow(&part, questions, required, &[]);
        let gaps = sample["gaps"].as_array().ok_or("missing workflow gaps")?;
        gap_count += gaps.len() as u64 + sample["omitted_gaps"].as_u64().unwrap_or(0);
        for gap in gaps {
            let destination = if gap["step_id"] == "$root" {
                &mut root_gaps
            } else {
                &mut step_gaps
            };
            if destination.len() < 5 {
                sample_bytes += gap.to_string().len() as u64;
                if sample_bytes > indexed.limits.item_bytes {
                    return Err("artifact workflow metadata exceeds item byte limit".into());
                }
                destination.push(gap.clone());
            }
        }
    }
    root_gaps.extend(
        step_gaps
            .into_iter()
            .take(5usize.saturating_sub(root_gaps.len())),
    );
    workflow["chain_finding_ids"] = json!(chain_ids);
    workflow["omitted_chain_finding_ids"] =
        json!(chain_count.saturating_sub(chain_ids.len() as u64));
    workflow["omitted_gaps"] = json!(gap_count.saturating_sub(root_gaps.len() as u64));
    workflow["gaps"] = json!(root_gaps);
    Ok(workflow)
}

#[cfg(test)]
mod indexed_workflow_tests {
    use super::*;
    #[test]
    fn streamed_workflow_preserves_global_gap_order_and_omission_counts() {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../examples/investigation-contract/supported.artifact.json"
        ))
        .unwrap();
        let findings: Vec<_> = (0..9).map(|i| {
            let steps: Vec<_> = (0..8).map(|j| json!({"id":format!("step-{j}"),"status":"not_observed","recognition":{"role_matched_records":0,"rule_matched_records":1},"omitted_occurrences":0,"ambiguity":{}})).collect();
            let mut finding = json!({"id":format!("f-{i}"),"details":{"event_chain":{"ambiguity":"none","steps":steps}}});
            if i % 2 == 1 { finding["details"]["gap_classification"] = json!("recognition_unresolved"); }
            finding
        }).collect();
        value["findings"] = json!(findings);
        let file = tempfile::NamedTempFile::new().unwrap();
        serde_json::to_writer(file.as_file(), &value).unwrap();
        let indexed = super::super::indexed_artifact::IndexedArtifact::open(
            file.reopen().unwrap(),
            super::super::indexed_artifact::Limits {
                item_bytes: 8 << 20,
                index_bytes: 64 << 20,
                state_bytes: 64 << 20,
                elapsed_ms: 30000,
                work_units: 1_000_000,
            },
        )
        .unwrap();
        assert_eq!(
            workflow_indexed(&indexed, &[], &[]).unwrap(),
            super::super::guidance::workflow(&value, &[], &[], &[])
        );
    }
}
