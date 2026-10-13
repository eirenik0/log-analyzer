//! Checked report pages written from replayable rows instead of a retained tree.
use super::{
    Result,
    construction::{Disk, Rows},
    finding_store::{Findings, StoredFinding},
};
use crate::cli::Cli;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Seek, SeekFrom, Write},
};

#[derive(Serialize, Deserialize)]
pub(super) struct Item {
    pub ordinal: usize,
    pub finding: StoredFinding,
}
#[derive(Default, Clone, Copy)]
struct Size {
    bytes: usize,
    chars: usize,
}
impl Write for Size {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes += bytes.len();
        // JSON output is valid UTF-8; count leading bytes across writer chunks.
        self.chars += bytes.iter().filter(|byte| **byte & 0xc0 != 0x80).count();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Size {
    fn json(value: &Value) -> Result<Self> {
        let mut size = Self::default();
        serde_json::to_writer(&mut size, value)?;
        Ok(size)
    }
    fn add(&mut self, size: Self, comma: bool) {
        self.bytes += size.bytes + usize::from(comma);
        self.chars += size.chars + usize::from(comma);
    }
}
pub(super) struct Page {
    pub metadata: Value,
    pub rows: Rows<Item>,
}
pub(super) struct Options<'a> {
    pub disk: Disk,
    pub item_limit: u64,
    pub snapshot: &'a Value,
    pub redacted: bool,
    pub unverifiable: &'a BTreeSet<u64>,
}
impl Page {
    pub fn build(
        mut metadata: Value,
        findings: &Findings,
        cli: &Cli,
        options: Options<'_>,
    ) -> Result<Self> {
        let Options {
            disk,
            item_limit,
            snapshot,
            redacted,
            unverifiable,
        } = options;
        let total = findings.len();
        let maximum = if cli.complete_output {
            total
        } else {
            cli.report_max_items.unwrap_or(20)
        };
        metadata["findings"] = json!([]);
        if metadata["artifact"]["status"] != "unavailable" {
            let required: BTreeSet<_> = metadata["assessments"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|assessment| assessment["finding_ids"].as_array().into_iter().flatten())
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            for (ordinal, row) in findings.iter()?.enumerate() {
                let row = row?;
                if row.value["id"]
                    .as_str()
                    .is_some_and(|id| required.contains(id))
                {
                    metadata["retrieval"]["targets"]
                        .as_array_mut()
                        .unwrap()
                        .push(target(ordinal, &row.value));
                }
            }
        }
        let mut page = Self {
            metadata,
            rows: Rows::new(disk.clone(), item_limit)?,
        };
        let mut payload = Size::default();
        let mut targets = Size::default();
        let base_targets = page.metadata["retrieval"]["targets"]
            .as_array()
            .map_or(0, Vec::len);
        let mut guidance_samples = Vec::new();
        let mut workflow = super::guidance::WorkflowSummary::default();
        page.refresh(
            cli,
            total,
            maximum,
            if total == 0 {
                "complete"
            } else if maximum == 0 {
                "item_limit_zero"
            } else {
                "oversized_item"
            },
        );
        let sizes = page.measure(cli, payload, targets, &guidance_samples, &workflow)?;
        if over(cli, sizes) {
            page.metadata["presentation"]["status"] = json!("mandatory_metadata_over_budget");
            page.measure(cli, payload, targets, &guidance_samples, &workflow)?;
            page.reconcile_unavailable(findings, cli, disk, item_limit)?;
            return Ok(page);
        }
        for (ordinal, row) in findings.iter()?.enumerate().take(maximum) {
            let mut row = row?;
            row.value = findings.transformed(&row, snapshot, redacted, unverifiable);
            if page.metadata["artifact"]["status"] == "unavailable" {
                row.value["verification"]["artifact_integrity"] = json!("unavailable");
                for excerpt in row.value["evidence"].as_array_mut().into_iter().flatten() {
                    excerpt["verification"]["artifact_integrity"] = json!("unavailable");
                }
            }
            let mut item_size = Size::default();
            findings.write_item(&row, &row.value, &mut item_size)?;
            let mut candidate = payload;
            candidate.add(item_size, page.rows.len() > 0);
            let mut candidate_targets = targets;
            if page.metadata["retrieval"]["status"] == "available"
                && !has_target(&page.metadata, ordinal)
            {
                candidate_targets.add(
                    Size::json(&target(ordinal, &row.value))?,
                    base_targets + page.rows.len() > 0,
                );
            }
            let mut samples = guidance_samples.clone();
            if samples.len() < 20 {
                samples.push(row.value.clone());
            }
            let mut candidate_workflow = workflow.clone();
            candidate_workflow.observe(&page.metadata, &row.value);
            let count = page.rows.len() + 1;
            refresh(
                &mut page.metadata,
                cli,
                total,
                maximum,
                count,
                if count == total { "complete" } else { "page" },
            );
            let sizes = page.measure(
                cli,
                candidate,
                candidate_targets,
                &samples,
                &candidate_workflow,
            )?;
            if over(cli, sizes) {
                page.refresh(
                    cli,
                    total,
                    maximum,
                    if page.rows.len() == 0 {
                        "oversized_item"
                    } else {
                        "page"
                    },
                );
                page.measure(cli, payload, targets, &guidance_samples, &workflow)?;
                break;
            }
            page.rows.push(&Item {
                ordinal,
                finding: row,
            })?;
            payload = candidate;
            targets = candidate_targets;
            guidance_samples = samples;
            workflow = candidate_workflow;
        }
        page.reconcile_unavailable(findings, cli, disk, item_limit)?;
        Ok(page)
    }
    fn refresh(&mut self, cli: &Cli, total: usize, maximum: usize, status: &str) {
        refresh(
            &mut self.metadata,
            cli,
            total,
            maximum,
            self.rows.len(),
            status,
        );
    }
    fn measure(
        &mut self,
        cli: &Cli,
        payload: Size,
        targets: Size,
        samples: &[Value],
        workflow: &super::guidance::WorkflowSummary,
    ) -> Result<Size> {
        self.metadata["findings"] = json!(samples);
        self.metadata["guidance"] = super::guidance::build(&self.metadata, cli);
        self.metadata["findings"] = json!([]);
        workflow.apply(&mut self.metadata["guidance"]);
        for _ in 0..8 {
            let mut size = Size::json(&self.metadata)?;
            size.add(payload, false);
            size.add(targets, false);
            size.bytes += 1;
            size.chars += 1;
            if self.metadata["presentation"]["serialized_bytes"] == size.bytes
                && self.metadata["presentation"]["serialized_characters"] == size.chars
            {
                return Ok(size);
            }
            self.metadata["presentation"]["serialized_bytes"] = json!(size.bytes);
            self.metadata["presentation"]["serialized_characters"] = json!(size.chars);
        }
        Err("Could not stabilize serialized report size".into())
    }
    fn reconcile_unavailable(
        &mut self,
        findings: &Findings,
        cli: &Cli,
        disk: Disk,
        item_limit: u64,
    ) -> Result<()> {
        if self.metadata["artifact"]["status"] != "unavailable" {
            return Ok(());
        }
        let key_bytes = self
            .rows
            .iter()?
            .try_fold(0u64, |sum, item| -> Result<u64> {
                Ok(sum
                    + item?.finding.value["id"]
                        .as_str()
                        .ok_or("finding ID missing")?
                        .len() as u64)
            })?;
        let capacity = (self.rows.len().max(1) as u64)
            .checked_mul(2)
            .and_then(u64::checked_next_power_of_two)
            .ok_or("finding index capacity overflow")?;
        let bytes = capacity
            .checked_mul(64)
            .and_then(|slots| slots.checked_add(key_bytes))
            .ok_or("finding index byte overflow")?;
        let _reservation = disk.reserve(bytes)?;
        let mut index = crate::disk_index::DiskIndex::new(self.rows.len() as u64, bytes)?;
        for (ordinal, item) in self.rows.iter()?.enumerate() {
            let item = item?;
            let id = item.finding.value["id"]
                .as_str()
                .ok_or("finding ID missing")?;
            if index.insert(id.as_bytes(), ordinal as u64)?.is_some() {
                return Err("duplicate finding ID".into());
            }
        }
        let keep = |item: &Item| -> Result<bool> {
            for id in findings.measurement_ids(&item.finding)? {
                if index.get(id?.as_bytes())?.is_none() {
                    return Ok(false);
                }
            }
            if let Some(against) = item.finding.value["details"]["against_finding_id"].as_str()
                && index.get(against.as_bytes())?.is_none()
            {
                return Ok(false);
            }
            Ok(true)
        };
        let mut removed = false;
        for item in self.rows.iter()? {
            if !keep(&item?)? {
                removed = true;
                break;
            }
        }
        if removed {
            let mut retained = Rows::new(disk.clone(), item_limit)?;
            for item in self.rows.iter()? {
                let item = item?;
                if keep(&item)? {
                    retained.push(&item)?;
                }
            }
            self.rows = retained;
        }
        let required: BTreeSet<_> = self.metadata["assessments"]
            .as_array()
            .ok_or("assessments missing")?
            .iter()
            .flat_map(|assessment| assessment["finding_ids"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let mut displayed = BTreeSet::new();
        for item in self.rows.iter()? {
            let item = item?;
            let id = item.finding.value["id"]
                .as_str()
                .ok_or("finding ID missing")?;
            if required.contains(id)
                && matches!(
                    item.finding.value["kind"].as_str(),
                    Some("observation" | "measurement" | "calculated_fact")
                )
            {
                displayed.insert(id.to_owned());
            }
        }
        for assessment in self.metadata["assessments"]
            .as_array_mut()
            .ok_or("assessments missing")?
        {
            assessment["finding_ids"]
                .as_array_mut()
                .ok_or("assessment references missing")?
                .retain(|id| id.as_str().is_some_and(|id| displayed.contains(id)));
            if assessment["finding_ids"].as_array().unwrap().is_empty()
                && assessment["status"] == "supported"
            {
                assessment["status"] = json!("insufficient_evidence");
                assessment["reason"] = json!(
                    "Evidence artifact unavailable and bounded presentation retained no supporting findings for this goal"
                );
            }
        }
        self.remeasure(findings, cli)?;
        if over(
            cli,
            Size {
                bytes: self.metadata["presentation"]["serialized_bytes"]
                    .as_u64()
                    .unwrap_or(0) as usize,
                chars: self.metadata["presentation"]["serialized_characters"]
                    .as_u64()
                    .unwrap_or(0) as usize,
            },
        ) {
            self.rows = Rows::new(disk, item_limit)?;
            for assessment in self.metadata["assessments"].as_array_mut().unwrap() {
                assessment["finding_ids"] = json!([]);
                if assessment["status"] == "supported" {
                    assessment["status"] = json!("insufficient_evidence");
                    assessment["reason"] = json!(
                        "Evidence artifact unavailable and bounded presentation retained no supporting findings"
                    );
                }
            }
            self.metadata["presentation"]["status"] = json!("mandatory_metadata_over_budget");
            self.remeasure(findings, cli)?;
        }
        Ok(())
    }
    fn remeasure(&mut self, findings: &Findings, cli: &Cli) -> Result<()> {
        let mut payload = Size::default();
        let mut samples = Vec::new();
        let mut workflow = super::guidance::WorkflowSummary::default();
        for (i, item) in self.rows.iter()?.enumerate() {
            let item = item?;
            let mut size = Size::default();
            findings.write_item(&item.finding, &item.finding.value, &mut size)?;
            payload.add(size, i > 0);
            workflow.observe(&self.metadata, &item.finding.value);
            if samples.len() < 20 {
                samples.push(item.finding.value);
            }
        }
        let total = self.metadata["presentation"]["total_findings"]
            .as_u64()
            .ok_or("total findings missing")? as usize;
        let maximum = self.metadata["presentation"]["budget_items"]
            .as_u64()
            .unwrap_or(0) as usize;
        let status = self.metadata["presentation"]["status"]
            .as_str()
            .ok_or("page status missing")?
            .to_owned();
        self.refresh(cli, total, maximum, &status);
        self.measure(cli, payload, Size::default(), &samples, &workflow)?;
        Ok(())
    }
    pub fn values(&self) -> Result<impl Iterator<Item = Result<Value>> + '_> {
        Ok(self
            .rows
            .iter()?
            .map(|item| item.map(|item| item.finding.value)))
    }
    pub fn write_json(&self, findings: &Findings, writer: &mut dyn Write) -> Result<()> {
        writer.write_all(b"{")?;
        for (i, (key, value)) in self
            .metadata
            .as_object()
            .ok_or("report must be object")?
            .iter()
            .enumerate()
        {
            if i > 0 {
                writer.write_all(b",")?;
            }
            serde_json::to_writer(&mut *writer, key)?;
            writer.write_all(b":")?;
            match key.as_str() {
                "findings" => {
                    writer.write_all(b"[")?;
                    for (j, item) in self.rows.iter()?.enumerate() {
                        if j > 0 {
                            writer.write_all(b",")?;
                        }
                        let item = item?;
                        findings.write_item(&item.finding, &item.finding.value, writer)?;
                    }
                    writer.write_all(b"]")?;
                }
                "retrieval" => {
                    writer.write_all(b"{")?;
                    for (j, (key, value)) in value.as_object().unwrap().iter().enumerate() {
                        if j > 0 {
                            writer.write_all(b",")?;
                        }
                        serde_json::to_writer(&mut *writer, key)?;
                        writer.write_all(b":")?;
                        if key == "targets" {
                            writer.write_all(b"[")?;
                            let mut first = true;
                            for target in value.as_array().unwrap() {
                                if !first {
                                    writer.write_all(b",")?;
                                }
                                first = false;
                                serde_json::to_writer(&mut *writer, target)?;
                            }
                            for item in self.rows.iter()? {
                                let item = item?;
                                if self.metadata["retrieval"]["status"] != "available"
                                    || has_target(&self.metadata, item.ordinal)
                                {
                                    continue;
                                }
                                if !first {
                                    writer.write_all(b",")?;
                                }
                                first = false;
                                serde_json::to_writer(
                                    &mut *writer,
                                    &target(item.ordinal, &item.finding.value),
                                )?;
                            }
                            writer.write_all(b"]")?;
                        } else {
                            serde_json::to_writer(&mut *writer, value)?;
                        }
                    }
                    writer.write_all(b"}")?;
                }
                _ => serde_json::to_writer(&mut *writer, value)?,
            }
        }
        writer.write_all(b"}\n")?;
        Ok(())
    }
    fn write_brief(
        &self,
        brief: &Value,
        findings: &Findings,
        inline: bool,
        writer: &mut dyn Write,
    ) -> Result<()> {
        writer.write_all(b"{")?;
        for (i, (key, value)) in brief
            .as_object()
            .ok_or("brief must be object")?
            .iter()
            .enumerate()
        {
            if i > 0 {
                writer.write_all(b",")?;
            }
            serde_json::to_writer(&mut *writer, key)?;
            writer.write_all(b":")?;
            if key != "findings" {
                serde_json::to_writer(&mut *writer, value)?;
                continue;
            }
            writer.write_all(b"{")?;
            for (j, (key, value)) in value
                .as_object()
                .ok_or("brief findings must be object")?
                .iter()
                .enumerate()
            {
                if j > 0 {
                    writer.write_all(b",")?;
                }
                serde_json::to_writer(&mut *writer, key)?;
                writer.write_all(b":")?;
                if key != "items" || !inline {
                    serde_json::to_writer(&mut *writer, value)?;
                    continue;
                }
                writer.write_all(b"[")?;
                for (k, item) in self.rows.iter()?.enumerate() {
                    if k > 0 {
                        writer.write_all(b",")?;
                    }
                    let item = item?;
                    findings.write_item(&item.finding, &item.finding.value, writer)?;
                }
                writer.write_all(b"]")?;
            }
            writer.write_all(b"}")?;
        }
        writer.write_all(b"}\n")?;
        Ok(())
    }
    pub fn render(
        &self,
        findings: &Findings,
        cli: &Cli,
        brief: bool,
        overview: Option<&super::presentation::FindingOverview>,
    ) -> Result<File> {
        let mut file = tempfile::tempfile()?;
        {
            let mut writer = std::io::BufWriter::with_capacity(65536, &mut file);
            if brief {
                let inline = cli.summary || self.metadata["artifact"]["status"] == "unavailable";
                let mut brief = super::guidance::brief(&self.metadata, cli);
                brief["findings"]["displayed"] = json!(if inline { self.rows.len() } else { 0 });
                for _ in 0..8 {
                    let mut size = Size::default();
                    self.write_brief(&brief, findings, inline, &mut size)?;
                    let status = if over(cli, size) {
                        "mandatory_metadata_over_budget"
                    } else {
                        "complete"
                    };
                    if brief["presentation"]["serialized_bytes"] == size.bytes
                        && brief["presentation"]["serialized_characters"] == size.chars
                        && brief["presentation"]["status"] == status
                    {
                        break;
                    }
                    brief["presentation"]["serialized_bytes"] = json!(size.bytes);
                    brief["presentation"]["serialized_characters"] = json!(size.chars);
                    brief["presentation"]["status"] = json!(status);
                }
                self.write_brief(&brief, findings, inline, &mut writer)?;
            } else if matches!(cli.effective_format(), crate::cli::OutputFormat::Json) {
                self.write_json(findings, &mut writer)?;
            } else {
                super::presentation::text(
                    &self.metadata,
                    overview.ok_or("missing text overview")?,
                    self.values()?,
                    &mut writer,
                )?;
            }
            writer.flush()?;
        }
        file.seek(SeekFrom::Start(0))?;
        Ok(file)
    }
}
fn target(ordinal: usize, finding: &Value) -> Value {
    json!({"kind":"finding","id":finding["id"],"collection":format!("/findings/{ordinal}")})
}
fn over(cli: &Cli, size: Size) -> bool {
    cli.report_max_bytes.is_some_and(|limit| size.bytes > limit)
        || cli.report_max_chars.is_some_and(|limit| size.chars > limit)
}
fn refresh(
    report: &mut Value,
    cli: &Cli,
    total: usize,
    maximum: usize,
    count: usize,
    status: &str,
) {
    report["presentation"]["status"] = json!(status);
    report["presentation"]["budget_bytes"] = json!(cli.report_max_bytes);
    report["presentation"]["budget_characters"] = json!(cli.report_max_chars);
    report["presentation"]["budget_items"] = json!(maximum);
    report["presentation"]["total_findings"] = json!(total);
    report["presentation"]["displayed_findings"] = json!(count);
    report["presentation"]["omitted_findings"] = json!(total - count);
    report["presentation"]["collections"] = json!([{"path":"/findings","total":total,"prior":0,"displayed":count,"remaining":total-count}]);
    report["retrieval"]["next_cursor"] =
        if count < total && report["retrieval"]["status"] == "available" {
            json!(format!(
                "v1:{}:{count}",
                crate::evidence::digest(
                    json!([report["retrieval"]["binding_sha256"], "/findings", null])
                        .to_string()
                        .as_bytes()
                )
            ))
        } else {
            Value::Null
        };
}

fn has_target(metadata: &Value, ordinal: usize) -> bool {
    let path = format!("/findings/{ordinal}");
    metadata["retrieval"]["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|target| target["collection"] == path)
}

#[cfg(test)]
mod tests {
    use super::super::finding_store::FindingSink;
    use super::*;
    use clap::Parser;
    fn metadata() -> Value {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../examples/investigation-contract/supported.report.json"
        ))
        .unwrap();
        value["findings"] = json!([]);
        value["retrieval"]["targets"] = json!([]);
        value["assessments"] = json!([]);
        value
    }
    #[test]
    fn complete_rows_and_unicode_counts_match_actual_serialization() {
        let cli = Cli::try_parse_from([
            "log-analyzer",
            "--json",
            "--complete-output",
            "investigate",
            "input.log",
        ])
        .unwrap();
        let disk = Disk::new(16 << 20);
        let mut findings = Findings::new(disk.clone(), 4096).unwrap();
        let mut ids = Rows::new(disk.clone(), 128).unwrap();
        for n in 0..1000 {
            ids.push(&format!("measurement-{n}")).unwrap();
        }
        for n in 0..40 {
            findings.push(json!({"id":format!("f-{n}"),"scope_id":"scope-0","kind":"observation","claim":"🧪 café é", "details":{},"evidence":[]})).unwrap();
        }
        findings.push_with_ids(json!({"id":"distribution","scope_id":"scope-0","kind":"calculated_fact","details":{"measurement_ids":[]},"evidence":[]}),&ids).unwrap();
        let snapshot = json!("snapshot");
        let unavailable = BTreeSet::new();
        let page = Page::build(
            metadata(),
            &findings,
            &cli,
            Options {
                disk,
                item_limit: 4096,
                snapshot: &snapshot,
                redacted: false,
                unverifiable: &unavailable,
            },
        )
        .unwrap();
        let mut bytes = Vec::new();
        page.write_json(&findings, &mut bytes).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["findings"].as_array().unwrap().len(), 41);
        assert_eq!(
            value["findings"][40]["details"]["measurement_ids"]
                .as_array()
                .unwrap()
                .len(),
            1000
        );
        assert_eq!(value["presentation"]["serialized_bytes"], bytes.len());
        assert_eq!(
            value["presentation"]["serialized_characters"],
            std::str::from_utf8(&bytes).unwrap().chars().count()
        );
        assert_eq!(page.metadata["findings"], json!([]));
    }
    #[test]
    fn zero_item_page_keeps_checked_off_page_assessment_witness() {
        let cli = Cli::try_parse_from([
            "log-analyzer",
            "--json",
            "--report-max-items",
            "0",
            "investigate",
            "input.log",
        ])
        .unwrap();
        let disk = Disk::new(1 << 20);
        let mut findings = Findings::new(disk.clone(), 4096).unwrap();
        findings.push(json!({"id":"measured","scope_id":"scope-0","kind":"measurement","details":{},"evidence":[]})).unwrap();
        let mut value = metadata();
        value["assessments"] =
            json!([{"scope_id":"scope-0","status":"supported","finding_ids":["measured"]}]);
        let snapshot = json!("snapshot");
        let unavailable = BTreeSet::new();
        let page = Page::build(
            value,
            &findings,
            &cli,
            Options {
                disk,
                item_limit: 4096,
                snapshot: &snapshot,
                redacted: false,
                unverifiable: &unavailable,
            },
        )
        .unwrap();
        assert_eq!(page.rows.len(), 0);
        assert_eq!(page.metadata["assessments"][0]["status"], "supported");
        assert_eq!(page.metadata["retrieval"]["targets"][0]["id"], "measured");
        let mut bytes = Vec::new();
        page.write_json(&findings, &mut bytes).unwrap();
        assert_eq!(
            page.metadata["presentation"]["serialized_bytes"],
            bytes.len()
        );
    }
}
