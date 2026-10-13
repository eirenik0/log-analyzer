//! Stable priority-ordered findings with independently replayable large references.
use super::{
    Result,
    construction::{Disk, Rows},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, io::Write};

pub(crate) trait FindingSink {
    fn push(&mut self, finding: Value) -> Result<()>;
    fn push_with_ids(&mut self, mut finding: Value, ids: &Rows<String>) -> Result<()> {
        finding["details"]["measurement_ids"] = Value::Array(
            ids.iter()?
                .map(|id| id.map(Value::String))
                .collect::<Result<Vec<_>>>()?,
        );
        self.push(finding)
    }
}
impl FindingSink for Vec<Value> {
    fn push(&mut self, finding: Value) -> Result<()> {
        Vec::push(self, finding);
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct StoredFinding {
    pub value: Value,
    ids: Option<(usize, usize)>,
}
pub(super) struct Findings {
    buckets: Vec<Rows<StoredFinding>>,
    ids: Rows<String>,
    count: usize,
}
fn priority(finding: &Value) -> usize {
    let id = finding["id"].as_str().unwrap_or("");
    if finding["details"]["event_chain"].is_object() {
        return 0;
    }
    if id.contains("-chain-") && finding["kind"] == "unknown" {
        return 1;
    }
    match finding["details"]["resource_status"].as_str() {
        Some("fingerprint_match") => 1,
        _ if id.contains("observed-errors") => 2,
        Some("different_fingerprint" | "unconfigured_fingerprint") => 3,
        _ if id.contains("observed-warnings") => 4,
        Some("unavailable") => 5,
        Some("empty_manifest") => 7,
        _ => 6,
    }
}
impl Findings {
    pub fn new(disk: Disk, item_limit: u64) -> Result<Self> {
        Ok(Self {
            buckets: (0..8)
                .map(|_| Rows::new(disk.clone(), item_limit))
                .collect::<Result<_>>()?,
            ids: Rows::new(disk, item_limit)?,
            count: 0,
        })
    }
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn iter(&self) -> Result<impl Iterator<Item = Result<StoredFinding>> + '_> {
        let readers = self
            .buckets
            .iter()
            .map(Rows::iter)
            .collect::<Result<Vec<_>>>()?;
        Ok(readers.into_iter().flatten())
    }
    pub fn measurement_ids<'a>(
        &'a self,
        row: &'a StoredFinding,
    ) -> Result<Box<dyn Iterator<Item = Result<String>> + 'a>> {
        if let Some((start, count)) = row.ids {
            return Ok(Box::new(self.ids.range_by_ordinal(start, count)?));
        }
        Ok(Box::new(
            row.value["details"]["measurement_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|id| {
                    id.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| "invalid measurement ID".into())
                }),
        ))
    }
    pub fn transformed(
        &self,
        row: &StoredFinding,
        snapshot: &Value,
        redacted: bool,
        unverifiable: &BTreeSet<u64>,
    ) -> Value {
        let mut value = row.value.clone();
        super::presentation::bind_snapshot(&mut value, snapshot);
        if redacted || !unverifiable.is_empty() {
            let mut projection = json!({"findings":[value],"records":[],"populations":[],"memberships":[],"captured_inputs":[],"report_metadata":{"evidence":{"inputs":[]}},"verification":super::findings::verification(false,"available")});
            if redacted {
                super::artifact::redact(&mut projection);
            }
            if !unverifiable.is_empty() {
                super::artifact::source_verification_unavailable(
                    &mut projection,
                    unverifiable,
                    "A captured byte stream is not complete UTF-8; source text verification is unavailable.",
                );
            }
            value = projection["findings"].as_array_mut().unwrap().remove(0);
        }
        value
    }
    pub fn write_item(
        &self,
        row: &StoredFinding,
        value: &Value,
        writer: &mut dyn Write,
    ) -> Result<()> {
        let Some((start, count)) = row.ids else {
            serde_json::to_writer(writer, value)?;
            return Ok(());
        };
        writer.write_all(b"{")?;
        for (i, (key, field)) in value
            .as_object()
            .ok_or("finding must be an object")?
            .iter()
            .enumerate()
        {
            if i > 0 {
                writer.write_all(b",")?;
            }
            serde_json::to_writer(&mut *writer, key)?;
            writer.write_all(b":")?;
            if key != "details" {
                serde_json::to_writer(&mut *writer, field)?;
                continue;
            }
            writer.write_all(b"{")?;
            for (j, (key, field)) in field
                .as_object()
                .ok_or("finding details must be an object")?
                .iter()
                .enumerate()
            {
                if j > 0 {
                    writer.write_all(b",")?;
                }
                serde_json::to_writer(&mut *writer, key)?;
                writer.write_all(b":")?;
                if key != "measurement_ids" {
                    serde_json::to_writer(&mut *writer, field)?;
                    continue;
                }
                writer.write_all(b"[")?;
                for (k, id) in self.ids.range_by_ordinal(start, count)?.enumerate() {
                    if k > 0 {
                        writer.write_all(b",")?;
                    }
                    serde_json::to_writer(&mut *writer, &id?)?;
                }
                writer.write_all(b"]")?;
            }
            writer.write_all(b"}")?;
        }
        writer.write_all(b"}")?;
        Ok(())
    }
    pub fn write(
        &self,
        snapshot: &Value,
        redacted: bool,
        unverifiable: &BTreeSet<u64>,
        writer: &mut dyn Write,
    ) -> Result<()> {
        writer.write_all(b"[")?;
        for (i, row) in self.iter()?.enumerate() {
            if i > 0 {
                writer.write_all(b",")?;
            }
            let row = row?;
            let value = self.transformed(&row, snapshot, redacted, unverifiable);
            self.write_item(&row, &value, writer)?;
        }
        writer.write_all(b"]")?;
        Ok(())
    }
}
impl FindingSink for Findings {
    fn push(&mut self, value: Value) -> Result<()> {
        self.buckets[priority(&value)].push(&StoredFinding { value, ids: None })?;
        self.count += 1;
        Ok(())
    }
    fn push_with_ids(&mut self, mut value: Value, ids: &Rows<String>) -> Result<()> {
        let start = self.ids.len();
        for id in ids.iter()? {
            self.ids.push(&id?)?;
        }
        value["details"]["measurement_ids"] = json!([]);
        self.buckets[priority(&value)].push(&StoredFinding {
            value,
            ids: Some((start, ids.len())),
        })?;
        self.count += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn priority_is_stable_and_large_distribution_references_are_replayed() {
        let disk = Disk::new(16 << 20);
        let mut findings = Findings::new(disk.clone(), 4096).unwrap();
        let mut ids = Rows::new(disk, 128).unwrap();
        for n in 0..1000 {
            ids.push(&format!("measurement-{n}")).unwrap();
        }
        findings
            .push(json!({"id":"normal-a","kind":"observation","details":{}}))
            .unwrap();
        findings.push_with_ids(json!({"id":"distribution","kind":"calculated_fact","details":{"measurement_ids":[]}}),&ids).unwrap();
        findings
            .push(json!({"id":"normal-b","kind":"observation","details":{}}))
            .unwrap();
        findings
            .push(json!({"id":"observed-errors-count","kind":"calculated_fact","details":{}}))
            .unwrap();
        let order: Vec<_> = findings
            .iter()
            .unwrap()
            .map(|row| row.unwrap().value["id"].clone())
            .collect();
        assert_eq!(
            order,
            json!([
                "observed-errors-count",
                "normal-a",
                "distribution",
                "normal-b"
            ])
            .as_array()
            .unwrap()
            .clone()
        );
        let mut output = Vec::new();
        findings
            .write(&json!("snapshot"), false, &BTreeSet::new(), &mut output)
            .unwrap();
        let values: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            values[2]["details"]["measurement_ids"]
                .as_array()
                .unwrap()
                .len(),
            1000
        );
        assert_eq!(
            values[2]["details"]["measurement_ids"][999],
            "measurement-999"
        );
        let row = findings.iter().unwrap().nth(2).unwrap().unwrap();
        assert_eq!(row.value["details"]["measurement_ids"], json!([]));
        assert_eq!(findings.measurement_ids(&row).unwrap().count(), 1000);
    }
}
