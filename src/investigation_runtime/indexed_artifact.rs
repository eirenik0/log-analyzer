//! Bounded navigation and cross-reference indexes over a verified private artifact.
//! JSON offsets and reference indexes are plain temporary files, not SQLite.
use crate::{
    disk_index::DiskIndex,
    investigation::{OccurrenceId, ValidationSession},
    json_index::Node,
};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{BufWriter, Read, Seek, SeekFrom, Write},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub item_bytes: u64,
    pub index_bytes: u64,
    pub state_bytes: u64,
    pub elapsed_ms: u64,
    pub work_units: u64,
}
struct Budget {
    deadline: std::time::Instant,
    remaining: std::cell::Cell<u64>,
}
impl Budget {
    fn check(&self, work: u64) -> Result<()> {
        if std::time::Instant::now() >= self.deadline {
            return Err("artifact validation time limit exceeded".into());
        }
        self.remaining.set(
            self.remaining
                .get()
                .checked_sub(work)
                .ok_or("artifact validation work limit exceeded")?,
        );
        Ok(())
    }
}
pub(super) struct IndexedArtifact {
    pub root: Node,
    pub base: Value,
    records: DiskIndex,
    findings: DiskIndex,
    populations: DiskIndex,
    sources: Vec<Option<tempfile::NamedTempFile>>,
    session: ValidationSession,
    pub limits: Limits,
    budget: Budget,
}
fn required(root: &Node, pointer: &str) -> Result<Node> {
    root.pointer(pointer)?
        .ok_or_else(|| format!("missing artifact value {pointer}").into())
}
fn array(root: &Node, pointer: &str) -> Result<Node> {
    let node = required(root, pointer)?;
    if !node.is_array() {
        return Err(format!("expected array at {pointer}").into());
    }
    Ok(node)
}
fn occurrence_key(value: &Value) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&OccurrenceId::from_value(value)?)?)
}
impl IndexedArtifact {
    pub fn checkpoint(&self) -> Result<()> {
        self.budget.check(1)
    }
    pub fn open(snapshot: File, limits: Limits) -> Result<Self> {
        let budget = Budget {
            deadline: std::time::Instant::now()
                .checked_add(std::time::Duration::from_millis(limits.elapsed_ms))
                .ok_or("artifact validation deadline is too large")?,
            remaining: std::cell::Cell::new(limits.work_units),
        };
        budget.check(0)?;
        let root = Node::build(
            snapshot,
            limits.index_bytes,
            Some(budget.deadline),
            limits.work_units,
        )?;
        budget.check(root.node_count())?;
        if !root.is_object() || root.len() > 128 {
            return Err("artifact metadata field limit exceeded".into());
        }
        let mut base = root.read_without(
            &[
                "records",
                "findings",
                "populations",
                "memberships",
                "sequences",
                "captured_inputs",
                "assessments",
            ],
            limits.item_bytes,
        )?;
        if base["contract_version"] != 1 || base["investigation_contract_version"] != 1 {
            return Err("unsupported evidence artifact version".into());
        }
        let record_nodes = array(&root, "/records")?;
        let finding_nodes = array(&root, "/findings")?;
        let population_nodes = array(&root, "/populations")?;
        let mut records = DiskIndex::new(record_nodes.len(), limits.state_bytes / 16)?;
        let mut findings = DiskIndex::new(finding_nodes.len(), limits.state_bytes / 16)?;
        let mut populations = DiskIndex::new(population_nodes.len(), limits.state_bytes / 16)?;
        for record in record_nodes.children() {
            let record = record?;
            budget.check(1)?;
            if !record.is_object() || record.len() > 1024 {
                return Err("record metadata field limit exceeded".into());
            }
            let id = required(&record, "/occurrence")?.read(65536)?;
            if records
                .insert(&occurrence_key(&id)?, record.ordinal())?
                .is_some()
            {
                return Err("duplicate retained occurrence".into());
            }
        }
        for (nodes, index) in [
            (&finding_nodes, &mut findings),
            (&population_nodes, &mut populations),
        ] {
            for node in nodes.children() {
                let node = node?;
                budget.check(1)?;
                if !node.is_object() || node.len() > 1024 {
                    return Err("item metadata field limit exceeded".into());
                }
                let id = required(&node, "/id")?.read(65536)?;
                let id = id
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or("missing artifact item identity")?;
                if index.insert(id.as_bytes(), node.ordinal())?.is_some() {
                    return Err("duplicate artifact item identity".into());
                }
            }
        }
        let captures = array(&root, "/captured_inputs")?;
        if captures.len() > 32 {
            return Err("artifact input limit exceeded".into());
        }
        let mut sources = Vec::new();
        let mut capture_metadata = Vec::new();
        for capture in captures.children() {
            budget.check(1)?;
            let capture = capture?;
            let mut metadata = capture.read_without(&["data"], 65536)?;
            if metadata["data_omitted"] == false {
                if base["report_metadata"]["evidence"]["redaction"]["applied"] == true {
                    return Err("applied redaction must omit captured data".into());
                }
                let data = required(&capture, "/data")?;
                let mut file = tempfile::NamedTempFile::new()?;
                if metadata["encoding"] == "utf8" {
                    data.copy_string(file.as_file_mut(), || budget.check(1))?;
                } else if metadata["encoding"] == "bytes" {
                    if !data.is_array() {
                        return Err("expected captured byte array".into());
                    }
                    let mut output = BufWriter::new(file.as_file_mut());
                    for byte in data.children() {
                        budget.check(1)?;
                        let byte = byte?
                            .read(32)?
                            .as_u64()
                            .and_then(|n| u8::try_from(n).ok())
                            .ok_or("invalid captured byte")?;
                        output.write_all(&[byte])?;
                    }
                    output.flush()?;
                } else {
                    return Err("unsupported captured encoding".into());
                }
                use sha2::{Digest, Sha256};
                file.seek(SeekFrom::Start(0))?;
                let mut hash = Sha256::new();
                let mut buffer = [0; 65536];
                loop {
                    budget.check(1)?;
                    let read = file.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    hash.update(&buffer[..read]);
                }
                let digest = format!("{:x}", hash.finalize());
                if metadata["stored_sha256"] != digest {
                    return Err("retained byte digest mismatch".into());
                }
                if metadata["original_consumed_sha256"] == digest {
                    sources.push(Some(file));
                } else {
                    if base["content"] != "redacted" {
                        return Err("original capture differs from consumed bytes".into());
                    }
                    // Stored redacted bytes were verified above, but cannot prove
                    // original source claims in the bounded contract projection.
                    metadata["data_omitted"] = json!(true);
                    sources.push(None);
                }
            } else {
                if required(&capture, "/data")?.read(65536)? != Value::Null {
                    return Err("omitted capture contains data".into());
                }
                sources.push(None);
            }
            metadata["data"] = Value::Null;
            capture_metadata.push(metadata);
        }
        for name in [
            "records",
            "findings",
            "populations",
            "memberships",
            "sequences",
            "assessments",
        ] {
            base[name] = json!([]);
        }
        base["captured_inputs"] = Value::Array(capture_metadata);
        let mut session = ValidationSession::bounded(record_nodes.len(), limits.state_bytes / 16)?;
        session.deadline = Some(budget.deadline);
        Ok(Self {
            root,
            base,
            records,
            findings,
            populations,
            sources,
            session,
            limits,
            budget,
        })
    }
    fn validate_records(&mut self) -> Result<()> {
        self.validate_part(&self.base.clone())?;
        let nodes = array(&self.root, "/records")?;
        let mut lines = DiskIndex::new(nodes.len(), self.limits.state_bytes / 16)?;
        let mut rows = DiskIndex::new(nodes.len(), self.limits.state_bytes / 16)?;
        let inputs = self.base["report_metadata"]["evidence"]["inputs"]
            .as_array()
            .ok_or("missing artifact inputs")?
            .clone();
        let mut represented = vec![0u64; inputs.len()];
        let mut part = self.base.clone();
        let mut chunk = Vec::new();
        let mut chunk_bytes = 0;
        for node in nodes.children() {
            self.budget.check(1)?;
            let node = node?;
            let record = node.read(self.limits.item_bytes)?;
            let id = OccurrenceId::from_value(&record["occurrence"])?;
            let ordinal = usize::try_from(id.input_ordinal)?;
            if ordinal >= represented.len() {
                return Err("record input is outside manifest".into());
            }
            let reference = &record["occurrence"]["evidence_ref"];
            let line = reference["line"]
                .as_u64()
                .filter(|line| *line > 0)
                .ok_or("invalid physical source line")?;
            let line_key = serde_json::to_vec(&(ordinal, line))?;
            if lines.insert(&line_key, 0)?.is_none() {
                represented[ordinal] += 1;
            }
            if !reference["row_path"].is_null() {
                let row_key = serde_json::to_vec(&(ordinal, line, &reference["row_path"]))?;
                if rows.insert(&row_key, 1)?.is_none() {
                    let previous = lines.get(&line_key)?.ok_or("missing source line state")?;
                    lines.replace(&line_key, previous + 1)?;
                    if previous > 0 {
                        represented[ordinal] += 1;
                    }
                }
            }
            chunk_bytes += node.byte_len();
            chunk.push(record);
            if chunk_bytes >= 1024 * 1024 || chunk.len() >= 128 {
                part["records"] = Value::Array(std::mem::take(&mut chunk));
                self.validate_part(&part)?;
                part["records"] = json!([]);
                chunk_bytes = 0;
            }
        }
        if !chunk.is_empty() {
            part["records"] = Value::Array(chunk);
            self.validate_part(&part)?;
        }
        for (ordinal, count) in represented.iter().enumerate() {
            self.budget.check(1)?;
            if *count
                > inputs[ordinal]["selected_entries"]
                    .as_u64()
                    .ok_or("missing selected coverage")?
            {
                return Err("retained source records exceed selected parse coverage".into());
            }
        }
        Ok(())
    }
    fn add_sources(
        &self,
        part: &mut Value,
        sources: impl IntoIterator<Item = Value>,
    ) -> Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        let mut retained = Vec::new();
        let mut remaining = self.limits.item_bytes;
        for source in sources {
            self.budget.check(1)?;
            let key = occurrence_key(&source)?;
            if !seen.insert(key.clone()) {
                continue;
            }
            let node = self.node_by_id(&self.records, &key)?;
            if node.byte_len() > remaining {
                return Err("artifact witness bundle exceeds item byte limit".into());
            }
            remaining -= node.byte_len();
            retained.push(node.read(self.limits.item_bytes)?);
        }
        part["records"] = Value::Array(retained);
        Ok(())
    }
    fn finding_sources(&self, finding: &Value) -> Result<Vec<Value>> {
        let mut sources = Vec::new();
        for excerpt in finding["evidence"]
            .as_array()
            .ok_or("missing finding evidence")?
        {
            sources.push(excerpt["occurrence"].clone());
        }
        if let Some(support) = finding["details"].get("supporting_occurrences") {
            sources.extend(
                support
                    .as_array()
                    .ok_or("invalid supporting occurrences")?
                    .iter()
                    .cloned(),
            );
        }
        if finding["kind"] == "measurement" {
            for boundary in ["start", "end"] {
                self.budget.check(1)?;
                sources.push(finding["details"]["boundaries"][boundary]["occurrence"].clone());
            }
        }
        Ok(sources)
    }
    fn scope(&self, id: &Value) -> Result<&Value> {
        self.base["scopes"]
            .as_array()
            .ok_or("missing artifact scopes")?
            .iter()
            .find(|scope| scope["id"] == *id)
            .ok_or_else(|| "unknown artifact scope".into())
    }
    fn member_key(&self, member: &Value, population: &Value) -> Result<Vec<u8>> {
        if member["kind"] == "record" {
            return occurrence_key(&member["occurrence"]);
        }
        if population["entity"] == "resources" {
            let mut identity = std::collections::BTreeMap::new();
            for field in member["identity"]
                .as_array()
                .ok_or("missing member identity")?
            {
                let name = field["field"].as_str().ok_or("invalid identity field")?;
                let value = field["value"].as_str().ok_or("invalid identity value")?;
                if identity.insert(name, value).is_some() {
                    return Err("duplicate identity field".into());
                }
            }
            let mut declared = std::collections::BTreeMap::new();
            for field in population["identity_fields"]
                .as_array()
                .ok_or("missing population identity")?
            {
                let name = field.as_str().ok_or("invalid declared identity field")?;
                declared.insert(
                    name,
                    *identity
                        .get(name)
                        .ok_or("missing declared identity field")?,
                );
            }
            for field in self.scope(&population["scope_id"])?["correlation_scope"]
                .as_array()
                .ok_or("missing correlation scope")?
            {
                let name = field["field"].as_str().ok_or("invalid scope field")?;
                declared.insert(
                    name,
                    *identity.get(name).ok_or("missing scope identity field")?,
                );
            }
            return Ok(serde_json::to_vec(&declared)?);
        }
        Ok(serde_json::to_vec(&(
            member["kind"].as_str().ok_or("missing member kind")?,
            member["id"].as_str().ok_or("missing member id")?,
        ))?)
    }
    fn validate_memberships(&mut self) -> Result<()> {
        use sha2::{Digest, Sha256};
        let memberships = array(&self.root, "/memberships")?;
        let mut identities = DiskIndex::new(self.populations.len(), self.limits.state_bytes / 16)?;
        for (ordinal, membership) in memberships.children().enumerate() {
            self.budget.check(1)?;
            let membership = membership?;
            let id = required(&membership, "/population_id")?.read(65536)?;
            let id = id.as_str().ok_or("invalid membership population id")?;
            if identities
                .insert(id.as_bytes(), membership.ordinal())?
                .is_some()
            {
                return Err("duplicate population membership".into());
            }
            let population = self.population(id)?;
            let members = array(&membership, "/members")?;
            if population["count"].as_u64() != Some(members.len())
                || population["membership"]["count"] != population["count"]
            {
                return Err("population count differs from retained membership".into());
            }
            if population["membership"]["collection"] != format!("/memberships/{ordinal}/members") {
                return Err("population membership pointer differs".into());
            }
            let mut member_ids = DiskIndex::new(members.len(), self.limits.state_bytes / 16)?;
            let mut contributing =
                DiskIndex::new(self.findings.len(), self.limits.state_bytes / 16)?;
            let mut hash = HashWriter(Sha256::new());
            hash.write_all(b"[")?;
            for (member_ordinal, member) in members.children().enumerate() {
                self.budget.check(1)?;
                let member = member?.read(self.limits.item_bytes)?;
                if member_ordinal != 0 {
                    hash.write_all(b",")?;
                }
                serde_json::to_writer(&mut hash, &member)?;
                if member_ids
                    .insert(&self.member_key(&member, &population)?, 1)?
                    .is_some()
                {
                    return Err("duplicate population member or resource identity".into());
                }
                let mut sources = Vec::new();
                let mut findings = Vec::new();
                let mut remaining = self.limits.item_bytes;
                if member["kind"] == "record" {
                    sources.push(member["occurrence"].clone());
                } else {
                    sources.extend(
                        member["source_occurrences"]
                            .as_array()
                            .ok_or("missing member sources")?
                            .iter()
                            .cloned(),
                    );
                    for measurement in member["measurement_ids"]
                        .as_array()
                        .ok_or("missing contributing measurements")?
                    {
                        let id = measurement.as_str().ok_or("invalid measurement id")?;
                        if contributing.insert(id.as_bytes(), 1)?.is_some() {
                            return Err("duplicate contributing measurement".into());
                        }
                        let node = self.finding(id)?;
                        if node.byte_len() > remaining {
                            return Err("member measurement bundle exceeds item byte limit".into());
                        }
                        remaining -= node.byte_len();
                        let finding = node.read(self.limits.item_bytes)?;
                        sources.extend(self.finding_sources(&finding)?);
                        findings.push(finding);
                    }
                }
                let mut part = self.base.clone();
                let mut header = population.clone();
                header["count"] = json!(1);
                header["membership"]["count"] = json!(1);
                header["membership"]["collection"] = json!("/memberships/0/members");
                let items = json!([member]);
                header["membership"]["sha256"] =
                    json!(crate::evidence::digest(items.to_string().as_bytes()));
                part["populations"] = json!([header]);
                part["memberships"] = json!([{"population_id":id,"members":items}]);
                part["findings"] = Value::Array(findings);
                self.add_sources(&mut part, sources)?;
                self.validate_part(&part)?;
            }
            hash.write_all(b"]")?;
            if population["membership"]["sha256"] != format!("{:x}", hash.0.finalize()) {
                return Err("population membership digest mismatch".into());
            }
            if members.len() == 0 {
                let mut part = self.base.clone();
                let mut header = population.clone();
                header["membership"]["collection"] = json!("/memberships/0/members");
                part["populations"] = json!([header]);
                part["memberships"] = json!([{"population_id":id,"members":[]}]);
                self.validate_part(&part)?;
            }
        }
        if identities.len() != self.populations.len() {
            return Err("missing exhaustive population membership".into());
        }
        Ok(())
    }
    fn population_dependencies(&self, population: &Value, finding: &Value) -> Result<()> {
        if finding["verification"]["source_and_rules"] != "available" {
            return Ok(());
        }
        if self.base["report_metadata"]["evidence"]["redaction"]["applied"] == true
            || self.base["effective_profile_omitted"] != false
        {
            return Err("finding claims unavailable source rules".into());
        }
        if finding["evidence"]
            .as_array()
            .ok_or("missing finding evidence")?
            .iter()
            .any(|excerpt| excerpt["verification"]["source_and_rules"] != "available")
        {
            return Err("finding claims unavailable witness verification".into());
        }
        let mut dependencies = false;
        let pointer = population["membership"]["collection"]
            .as_str()
            .ok_or("missing population membership")?;
        for member in array(&self.root, pointer)?.children() {
            self.budget.check(1)?;
            let member = member?.read(self.limits.item_bytes)?;
            let sources = if member["kind"] == "record" {
                vec![member["occurrence"].clone()]
            } else {
                member["source_occurrences"]
                    .as_array()
                    .ok_or("missing member sources")?
                    .clone()
            };
            for source in sources {
                self.budget.check(1)?;
                dependencies = true;
                let id = OccurrenceId::from_value(&source)?;
                let original = self
                    .sources
                    .get(usize::try_from(id.input_ordinal)?)
                    .is_some_and(Option::is_some);
                let record = self.node_by_id(&self.records, &occurrence_key(&source)?)?;
                if !original
                    || required(&record, "/verification/source_and_rules")?.read(128)?
                        != "available"
                {
                    return Err("finding claims unavailable dependent source verification".into());
                }
            }
        }
        if !dependencies {
            for ordinal in self.scope(&finding["scope_id"])?["input_ordinals"]
                .as_array()
                .ok_or("missing scoped inputs")?
            {
                if !ordinal
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .and_then(|n| self.sources.get(n))
                    .is_some_and(Option::is_some)
                {
                    return Err("absence claim lacks retained scoped captures".into());
                }
            }
        }
        Ok(())
    }
    fn distribution(&self, node: &Node, finding: &Value, population: &Value) -> Result<()> {
        let details = &finding["details"];
        let ids = array(node, "/details/measurement_ids")?;
        if ids.len() == 0
            || details["sample_count"].as_u64() != Some(ids.len())
            || details["sample_count"] != population["count"]
        {
            return Err("distribution sample cardinality differs".into());
        }
        let mut contributing = DiskIndex::new(self.findings.len(), self.limits.state_bytes / 16)?;
        let pointer = population["membership"]["collection"]
            .as_str()
            .ok_or("missing population membership")?;
        for member in array(&self.root, pointer)?.children() {
            self.budget.check(1)?;
            let member = member?;
            if required(&member, "/kind")?.read(128)? == "record" {
                continue;
            }
            let Some(measurements) = member.field("measurement_ids")? else {
                continue;
            };
            if !measurements.is_array() {
                return Err("invalid measurement membership".into());
            }
            for id in measurements.children() {
                self.budget.check(1)?;
                let id = id?.read(65536)?;
                let id = id.as_str().ok_or("invalid measurement id")?;
                if contributing.insert(id.as_bytes(), 0)?.is_some() {
                    return Err("duplicate contributing measurement".into());
                }
            }
        }
        if contributing.len() != ids.len() {
            return Err("distribution samples differ from contributing measurements".into());
        }
        let statistic = details["statistic"]
            .as_str()
            .ok_or("missing distribution statistic")?;
        let claimed = details["value"].as_u64();
        let mut less = 0;
        let mut equal = 0;
        let mut minimum = u64::MAX;
        let mut maximum = 0;
        let mut sum = Some(0u64);
        let mut mean = if statistic == "mean" {
            Some(crate::sorted_samples::Samples::new(
                self.limits.state_bytes / 16,
            )?)
        } else {
            None
        };
        for id in ids.children() {
            self.budget.check(1)?;
            let id = id?.read(65536)?;
            let id = id.as_str().ok_or("invalid measurement id")?;
            if contributing.get(id.as_bytes())? != Some(0) {
                return Err("duplicate or non-contributing distribution sample".into());
            }
            contributing.replace(id.as_bytes(), 1)?;
            let sample = self.finding(id)?;
            if required(&sample, "/kind")?.read(128)? != "measurement"
                || required(&sample, "/scope_id")?.read(65536)? != finding["scope_id"]
            {
                return Err("distribution sample kind or scope differs".into());
            }
            if finding["verification"]["source_and_rules"] == "available"
                && required(&sample, "/verification/source_and_rules")?.read(128)? != "available"
            {
                return Err("calculation claims unavailable sample source verification".into());
            }
            let value = required(&sample, "/details/value")?
                .read(128)?
                .as_u64()
                .ok_or("invalid measurement value")?;
            minimum = minimum.min(value);
            maximum = maximum.max(value);
            sum = sum.and_then(|sum| sum.checked_add(value));
            less += u64::from(claimed.is_some_and(|claimed| value < claimed));
            equal += u64::from(claimed == Some(value));
            if let Some(values) = &mut mean {
                values.push(value)?;
            }
        }
        let (matches, method) = match statistic {
            "sum" => (
                claimed == Some(sum.ok_or("integer distribution sum overflow")?),
                "sum",
            ),
            "mean" => {
                let mut sum = 0f64;
                for value in mean
                    .ok_or("missing mean samples")?
                    .sorted(|| self.budget.check(1))?
                {
                    self.budget.check(1)?;
                    sum += value? as f64;
                }
                (
                    details["value"].as_f64() == Some(sum / ids.len() as f64),
                    "arithmetic_mean",
                )
            }
            "minimum" => (claimed == Some(minimum), "minimum"),
            "maximum" => (claimed == Some(maximum), "maximum"),
            "p50" | "p95" | "p99" => {
                let percentile = statistic[1..].parse::<u64>()?;
                let rank = (ids.len() / 100) * percentile + (ids.len() % 100) * percentile / 100;
                (
                    claimed.is_some() && less <= rank && rank < less + equal,
                    "sorted_index_floor_n_times_percentile_over_100",
                )
            }
            _ => return Err("unsupported distribution statistic".into()),
        };
        if !matches || details["method"] != method {
            return Err("distribution value or method differs".into());
        }
        Ok(())
    }
    fn validate_findings(&mut self) -> Result<()> {
        for node in array(&self.root, "/findings")?.children() {
            self.budget.check(1)?;
            let node = node?;
            let kind = required(&node, "/kind")?.read(128)?;
            let distribution = kind == "calculated_fact"
                && required(&node, "/details/calculation")?.read(128)? == "distribution";
            let mut finding = if distribution {
                let mut finding = node.read_without(&["details"], self.limits.item_bytes)?;
                finding["details"] = required(&node, "/details")?
                    .read_without(&["measurement_ids"], self.limits.item_bytes)?;
                finding
            } else {
                node.read(self.limits.item_bytes)?
            };
            let sources = self.finding_sources(&finding)?;
            let mut part = self.base.clone();
            if kind == "calculated_fact" {
                let id = finding["details"]["population_id"]
                    .as_str()
                    .ok_or("missing finding population")?
                    .to_owned();
                let population = self.population(&id)?;
                if population["scope_id"] != finding["scope_id"] {
                    return Err("finding population belongs to another scope".into());
                }
                self.population_dependencies(&population, &finding)?;
                if distribution {
                    self.distribution(&node, &finding, &population)?;
                } else if finding["details"]["calculation"] != "count"
                    || finding["details"]["value"] != population["count"]
                {
                    return Err("count differs from population".into());
                }
                // The complete membership and original arithmetic were checked
                // above. Reuse the contract validator for the bounded common
                // finding/metadata/provenance projection, with an empty count.
                // Source dependencies were checked against the original membership.
                // An empty projection must not invent an absence claim over all inputs.
                finding["verification"]
                    .as_object_mut()
                    .ok_or("invalid finding verification")?
                    .insert("source_and_rules".into(), json!("unavailable"));
                let mut header = population.clone();
                header["count"] = json!(0);
                header["membership"] = json!({"collection":"/memberships/0/members","count":0,"sha256":crate::evidence::digest(b"[]")});
                part["populations"] = json!([header]);
                part["memberships"] = json!([{"population_id":id,"members":[]}]);
                finding["details"]["calculation"] = json!("count");
                finding["details"]["value"] = json!(0);
                if distribution {
                    finding["details"]["unit"] = match population["entity"].as_str() {
                        Some("physical_records" | "normalized_records") => json!("records"),
                        Some("extraction_rows") => json!("rows"),
                        _ => population["entity"].clone(),
                    };
                }
            } else if kind == "contrary_evidence" {
                let target = finding["details"]["against_finding_id"]
                    .as_str()
                    .ok_or("missing contrary target")?;
                if finding["id"] == target
                    || required(&self.finding(target)?, "/scope_id")?.read(65536)?
                        != finding["scope_id"]
                {
                    return Err(
                        "contrary target is self-referential or belongs to another scope".into(),
                    );
                }
                // The target relation is checked directly; validating the common
                // observation shape avoids recursive contrary-target expansion.
                finding["kind"] = json!("observation");
            }
            part["findings"] = json!([finding]);
            self.add_sources(&mut part, sources)?;
            self.validate_part(&part)?;
        }
        Ok(())
    }
    fn validate_sequences(&mut self) -> Result<()> {
        let sequences = array(&self.root, "/sequences")?;
        let mut identities = DiskIndex::new(sequences.len(), self.limits.state_bytes / 16)?;
        for sequence in sequences.children() {
            self.budget.check(1)?;
            let sequence = sequence?;
            let header = sequence.read_without(&["events"], self.limits.item_bytes)?;
            let id = header["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or("invalid sequence id")?;
            if identities.insert(id.as_bytes(), 1)?.is_some() {
                return Err("duplicate sequence id".into());
            }
            let events = array(&sequence, "/events")?;
            let mut seen = DiskIndex::new(events.len(), self.limits.state_bytes / 16)?;
            let mut previous = None;
            let mut part = self.base.clone();
            let mut singleton = header.clone();
            singleton["events"] = json!([]);
            part["sequences"] = json!([singleton]);
            self.validate_part(&part)?;
            for event in events.children() {
                self.budget.check(1)?;
                let event = event?.read(self.limits.item_bytes)?;
                let source = &event["occurrence"];
                let id = OccurrenceId::from_value(source)?;
                if seen.insert(&occurrence_key(source)?, 1)?.is_some() {
                    return Err("duplicate event occurrence".into());
                }
                let timestamp = event["timestamp"]
                    .as_str()
                    .map(chrono::DateTime::parse_from_rfc3339)
                    .transpose()?;
                let key = (
                    timestamp.is_none(),
                    timestamp,
                    id.input_ordinal,
                    event["record_ordinal"]
                        .as_u64()
                        .ok_or("invalid record ordinal")?,
                );
                if previous.as_ref().is_some_and(|previous| previous > &key) {
                    return Err("event sequence is not deterministically ordered".into());
                }
                previous = Some(key);
                self.add_sources(&mut part, [source.clone()])?;
                let mut singleton = header.clone();
                singleton["events"] = json!([event]);
                part["sequences"] = json!([singleton]);
                self.validate_part(&part)?;
            }
        }
        Ok(())
    }
    fn validate_assessments(&self) -> Result<()> {
        let assessments = array(&self.root, "/assessments")?;
        let mut seen = DiskIndex::new(assessments.len(), self.limits.state_bytes / 16)?;
        for assessment in assessments.children() {
            self.budget.check(1)?;
            let assessment = assessment?;
            let header = assessment.read_without(&["finding_ids"], self.limits.item_bytes)?;
            let goal = header["goal"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("missing assessment goal")?;
            let scope_id = header["scope_id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("missing assessment scope")?;
            if seen
                .insert(&serde_json::to_vec(&(goal, scope_id))?, 1)?
                .is_some()
            {
                return Err("duplicate assessment for goal and scope".into());
            }
            let scope = self.scope(&header["scope_id"])?;
            let ids = array(&assessment, "/finding_ids")?;
            let supported = header["status"] == "supported";
            if supported
                && (ids.len() == 0
                    || (scope["extent"] == "declared_input"
                        && (scope["completeness"] != "complete"
                            || scope["analysis_completion"] != "complete")))
            {
                return Err("supported assessment lacks complete scope or findings".into());
            }
            let mut positive = false;
            for id in ids.children() {
                self.budget.check(1)?;
                let id = id?.read(65536)?;
                let id = id.as_str().ok_or("invalid assessment finding id")?;
                let finding = self.finding(id)?;
                if required(&finding, "/scope_id")?.read(65536)? != header["scope_id"] {
                    return Err("assessment finding belongs to another scope".into());
                }
                positive |= matches!(
                    required(&finding, "/kind")?.read(128)?.as_str(),
                    Some("observation" | "measurement" | "calculated_fact")
                );
            }
            if supported {
                if !positive {
                    return Err("supported assessment requires a positive finding".into());
                }
                let inputs = self.base["report_metadata"]["evidence"]["inputs"]
                    .as_array()
                    .ok_or("missing inputs")?;
                for ordinal in scope["input_ordinals"]
                    .as_array()
                    .ok_or("missing scoped inputs")?
                {
                    let ordinal = ordinal.as_u64().ok_or("invalid input ordinal")?;
                    if let Some(input) = usize::try_from(ordinal)
                        .ok()
                        .and_then(|ordinal| inputs.get(ordinal))
                        && input["coverage"]["nonempty_lines"]
                            .as_u64()
                            .is_some_and(|count| count > 0)
                        && input["coverage"]["parsed_entries"] == 0
                    {
                        return Err("unparsed input cannot support a scoped assessment".into());
                    }
                }
            }
        }
        Ok(())
    }
    pub fn validate(&mut self) -> Result<()> {
        self.validate_records()?;
        self.validate_memberships()?;
        self.validate_findings()?;
        self.validate_sequences()?;
        self.validate_assessments()
    }
    fn node_by_id(&self, index: &DiskIndex, key: &[u8]) -> Result<Node> {
        self.root.at_ordinal(
            index
                .get(key)?
                .ok_or("artifact reference is not retained")?,
        )
    }
    pub(super) fn find_node(&self, id: &str) -> Result<Option<Node>> {
        self.findings
            .get(id.as_bytes())?
            .map(|ordinal| self.root.at_ordinal(ordinal))
            .transpose()
    }
    fn finding(&self, id: &str) -> Result<Node> {
        self.node_by_id(&self.findings, id.as_bytes())
    }
    fn population(&self, id: &str) -> Result<Value> {
        self.node_by_id(&self.populations, id.as_bytes())?
            .read(self.limits.item_bytes)
    }
    fn validate_part(&mut self, part: &Value) -> Result<()> {
        self.budget.check(1)?;
        let report = super::artifact::report(
            part,
            std::path::Path::new("private-snapshot"),
            Some("private-validated-snapshot"),
        );
        let sources = self
            .sources
            .iter()
            .map(|file| file.as_ref().map(|file| file.path().to_path_buf()))
            .collect::<Vec<_>>();
        crate::investigation::validate_session(&report, Some(part), &sources, &mut self.session)?;
        Ok(())
    }
}

struct HashWriter(sha2::Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        use sha2::Digest;
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../examples/investigation-contract/supported.artifact.json"
        ))
        .unwrap()
    }
    fn limits() -> Limits {
        Limits {
            item_bytes: 8 << 20,
            index_bytes: 64 << 20,
            state_bytes: 64 << 20,
            elapsed_ms: 30000,
            work_units: 10_000_000,
        }
    }
    fn indexed(value: &Value, limits: Limits) -> Result<()> {
        let file = tempfile::NamedTempFile::new()?;
        serde_json::to_writer(file.as_file(), value)?;
        IndexedArtifact::open(file.reopen()?, limits)?.validate()
    }
    fn parity(value: &Value, expected: bool) {
        let report = super::super::artifact::report(
            value,
            std::path::Path::new("fixture.json"),
            Some("verified"),
        );
        let legacy = crate::investigation::validate_retained(&report, Some(value));
        assert_eq!(legacy.is_ok(), expected, "legacy: {legacy:?}");
        let result = indexed(value, limits());
        assert_eq!(result.is_ok(), expected, "indexed: {result:?}");
    }
    fn membership_hashes(value: &mut Value) {
        for i in 0..value["populations"].as_array().unwrap().len() {
            value["populations"][i]["membership"]["sha256"] = json!(crate::evidence::digest(
                value["memberships"][i]["members"].to_string().as_bytes()
            ));
        }
    }
    #[test]
    fn contract_fixtures_have_identical_validation_results() {
        for name in [
            "supported",
            "redacted",
            "processing-limited",
            "zero-filter-matches",
            "unsupported",
            "measured-zero",
            "insufficient",
            "conflicting",
        ] {
            let value: Value = serde_json::from_slice(
                &std::fs::read(format!(
                    "{}/examples/investigation-contract/{name}.artifact.json",
                    env!("CARGO_MANIFEST_DIR")
                ))
                .unwrap(),
            )
            .unwrap();
            parity(&value, true);
        }
    }
    #[test]
    fn global_tampering_is_rejected_even_when_membership_hashes_are_refreshed() {
        let cases: &[fn(&mut Value)] = &[
            |v| v["records"][1] = v["records"][0].clone(),
            |v| v["records"][0]["raw_text"] = json!("invented source"),
            |v| v["records"][0]["timestamp"] = json!("2030-01-01T00:00:00Z"),
            |v| v["findings"][1]["id"] = v["findings"][0]["id"].clone(),
            |v| v["memberships"][0]["members"][1] = v["memberships"][0]["members"][0].clone(),
            |v| {
                v["memberships"][0]["members"][1]["measurement_ids"] =
                    v["memberships"][0]["members"][0]["measurement_ids"].clone()
            },
            |v| v["populations"][0]["count"] = json!(99),
            |v| v["populations"][0]["membership"]["collection"] = json!("/memberships/1/members"),
            |v| {
                v["findings"][4]["details"]["measurement_ids"][1] =
                    v["findings"][4]["details"]["measurement_ids"][0].clone()
            },
            |v| v["findings"][4]["details"]["value"] = json!(999999),
            |v| v["findings"][4]["details"]["sample_count"] = json!(2),
            |v| v["findings"][0]["verification"]["source_and_rules"] = json!("unavailable"),
            |v| v["sequences"][0]["events"][1] = v["sequences"][0]["events"][0].clone(),
            |v| {
                v["sequences"][0]["events"]
                    .as_array_mut()
                    .unwrap()
                    .reverse()
            },
            |v| v["sequences"][0]["events"][0]["timestamp"] = json!("2030-01-01T00:00:00Z"),
            |v| v["captured_inputs"][0]["stored_sha256"] = json!("wrong"),
            |v| v["assessments"][0]["finding_ids"] = json!(["missing"]),
        ];
        for (i, mutate) in cases.iter().enumerate() {
            let mut value = fixture();
            mutate(&mut value);
            membership_hashes(&mut value);
            eprintln!("tamper case {i}");
            parity(&value, false);
        }
    }
    #[test]
    fn all_distribution_statistics_preserve_exact_arithmetic() {
        let base = fixture();
        let mut samples: Vec<_> = base["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["kind"] == "measurement")
            .map(|f| f["details"]["value"].as_u64().unwrap())
            .collect();
        samples.sort_unstable();
        for (statistic, method, expected) in [
            ("minimum", "minimum", json!(samples[0])),
            ("maximum", "maximum", json!(samples[2])),
            ("sum", "sum", json!(samples.iter().sum::<u64>())),
            (
                "mean",
                "arithmetic_mean",
                json!(samples.iter().map(|v| *v as f64).sum::<f64>() / 3.0),
            ),
            (
                "p50",
                "sorted_index_floor_n_times_percentile_over_100",
                json!(samples[1]),
            ),
        ] {
            let mut value = base.clone();
            value["findings"][4]["details"]["statistic"] = json!(statistic);
            value["findings"][4]["details"]["method"] = json!(method);
            value["findings"][4]["details"]["value"] = expected;
            parity(&value, true);
            value["findings"][4]["details"]["value"] = json!(1234567);
            parity(&value, false);
        }
    }
    #[test]
    fn record_members_cannot_authorize_measurement_samples() {
        let mut value = fixture();
        let records = value["records"].as_array().unwrap().clone();
        value["memberships"][0]["members"] = Value::Array(records[..3].iter().enumerate().map(|(i,r)| json!({"kind":"record","occurrence":r["occurrence"],"measurement_ids":[format!("measurement-{i}")]})).collect());
        value["populations"][0]["entity"] = json!("physical_records");
        value["findings"][3]["details"]["unit"] = json!("records");
        membership_hashes(&mut value);
        parity(&value, false);
    }
    #[test]
    fn redacted_captured_bytes_do_not_supply_original_source_verification() {
        let mut value = fixture();
        value["content"] = json!("redacted");
        fn unavailable(value: &mut Value) {
            match value {
                Value::Object(map) => {
                    if map.contains_key("source_and_rules") {
                        map.insert("source_and_rules".into(), json!("unavailable"));
                    }
                    for child in map.values_mut() {
                        unavailable(child);
                    }
                }
                Value::Array(items) => {
                    for item in items {
                        unavailable(item);
                    }
                }
                _ => (),
            }
        }
        unavailable(&mut value);
        value["captured_inputs"][0]["data"] = json!("redacted bytes é\n");
        value["captured_inputs"][0]["stored_sha256"] =
            json!(crate::evidence::digest("redacted bytes é\n".as_bytes()));
        parity(&value, true);
        value["records"][0]["verification"]["source_and_rules"] = json!("available");
        parity(&value, false);
    }
    #[test]
    fn validation_limits_fail_explicitly() {
        let value = fixture();
        for limits in [
            Limits {
                item_bytes: 1,
                ..limits()
            },
            Limits {
                index_bytes: 1,
                ..limits()
            },
            Limits {
                state_bytes: 1,
                ..limits()
            },
            Limits {
                elapsed_ms: 0,
                ..limits()
            },
            Limits {
                work_units: 1,
                ..limits()
            },
        ] {
            assert!(
                indexed(&value, limits)
                    .unwrap_err()
                    .to_string()
                    .contains("limit")
            );
        }
    }
    #[test]
    fn shuffled_retained_records_and_byte_captures_preserve_source_checks() {
        let mut value = fixture();
        value["records"].as_array_mut().unwrap().reverse();
        let data = value["captured_inputs"][0]["data"]
            .as_str()
            .unwrap()
            .as_bytes()
            .to_vec();
        value["captured_inputs"][0]["encoding"] = json!("bytes");
        value["captured_inputs"][0]["data"] = json!(data);
        parity(&value, true);
    }
}
