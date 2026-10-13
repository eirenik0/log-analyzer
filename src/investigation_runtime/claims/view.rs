//! A bounded projection of only the evidence named by a claims document.
use super::{Assertion, Assertions, CountOperation, Result};
use crate::investigation_runtime::indexed_artifact::IndexedArtifact;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
pub(super) const LIMIT: u64 = 8 * 1024 * 1024;
pub(super) fn check_size(value: &Value) -> Result<u64> {
    struct Counter(u64);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 += bytes.len() as u64;
            if self.0 > LIMIT {
                return Err(std::io::Error::other("Claim observations exceed 8 MiB"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut size = Counter(0);
    serde_json::to_writer(&mut size, value)?;
    Ok(size.0)
}
pub(super) trait Evidence {
    fn metadata(&self) -> &Value;
    fn pointer(&self, pointer: &str) -> Option<&Value>;
    fn finding(&self, id: &str) -> Option<(usize, &Value)>;
    fn count(&self, pointer: &str, operation: Option<&CountOperation>) -> Option<usize>;
    fn array_exists(&self, pointer: &str) -> bool;
    fn exists(&self, pointer: &str) -> bool;
}
fn count_key(pointer: &str, operation: Option<&CountOperation>) -> String {
    json!([pointer, operation]).to_string()
}
fn selected(item: &Value, operation: Option<&CountOperation>) -> Option<bool> {
    let Some(operation) = operation else {
        return Some(true);
    };
    let classification = &item["fields"]["classification"];
    if classification["status"] != "event" {
        return None;
    }
    let semantics = &classification["semantics"];
    Some(semantics["kind"] == json!(operation.kind) && semantics["name"] == operation.name)
}
impl Evidence for Value {
    fn metadata(&self) -> &Value {
        self
    }
    fn pointer(&self, pointer: &str) -> Option<&Value> {
        Value::pointer(self, pointer)
    }
    fn finding(&self, id: &str) -> Option<(usize, &Value)> {
        self["findings"]
            .as_array()?
            .iter()
            .enumerate()
            .find(|(_, f)| f["id"] == id)
    }
    fn count(&self, pointer: &str, operation: Option<&CountOperation>) -> Option<usize> {
        let mut seen = BTreeSet::new();
        for item in self.pointer(pointer)?.as_array()? {
            if !selected(item, operation)? {
                continue;
            }
            let occurrence = item.get("occurrence")?;
            occurrence.get("evidence_ref")?;
            seen.insert(occurrence.to_string());
        }
        Some(seen.len())
    }
    fn array_exists(&self, pointer: &str) -> bool {
        self.pointer(pointer).is_some_and(Value::is_array)
    }
    fn exists(&self, pointer: &str) -> bool {
        self.pointer(pointer).is_some()
    }
}
pub(super) struct IndexedView<'a> {
    metadata: &'a Value,
    pointers: BTreeMap<String, Value>,
    findings: BTreeMap<String, (usize, Value)>,
    counts: BTreeMap<String, Option<usize>>,
    arrays: BTreeSet<String>,
    present: BTreeSet<String>,
}
impl<'a> IndexedView<'a> {
    pub fn new(indexed: &'a IndexedArtifact, assertions: &Assertions) -> Result<Self> {
        let mut view = Self {
            metadata: &indexed.base,
            pointers: BTreeMap::new(),
            findings: BTreeMap::new(),
            counts: BTreeMap::new(),
            arrays: BTreeSet::new(),
            present: BTreeSet::new(),
        };
        let mut pointers = BTreeSet::new();
        let mut findings = BTreeSet::new();
        let mut existence = BTreeSet::new();
        let mut counts = BTreeMap::new();
        for claim in &assertions.claims {
            match &claim.assertion {
                Assertion::Fact { pointer, .. } | Assertion::Interval { pointer, .. } => {
                    pointers.insert(pointer.as_str());
                }
                Assertion::Link { finding_id, .. } | Assertion::Absence { finding_id, .. } => {
                    findings.insert(finding_id.as_str());
                }
                Assertion::DistinctRecords {
                    pointer, operation, ..
                } => {
                    counts.insert(
                        count_key(pointer, operation.as_ref()),
                        (pointer.as_str(), operation.as_ref()),
                    );
                }
            }
        }
        for spec in &assertions.requirements {
            if let Some(id) = &spec.finding_id {
                findings.insert(id.as_str());
            }
            if let Some(field) = &spec.field {
                existence.insert(field.pointer.as_str());
            }
            if let Some(population) = &spec.population {
                counts.insert(
                    count_key(&population.pointer, population.operation.as_ref()),
                    (population.pointer.as_str(), population.operation.as_ref()),
                );
            }
        }
        let mut remaining = LIMIT.min(indexed.limits.item_bytes);
        for pointer in pointers {
            indexed.checkpoint()?;
            if let Some(node) = indexed.root.pointer(pointer)? {
                let value = node.read(remaining)?;
                remaining = remaining
                    .checked_sub(node.byte_len())
                    .ok_or("Claim evidence exceeds 8 MiB")?;
                view.pointers.insert(pointer.to_owned(), value);
            }
        }
        if !findings.is_empty() {
            for (ordinal, node) in indexed
                .root
                .field("findings")?
                .ok_or("missing findings")?
                .children()
                .enumerate()
            {
                indexed.checkpoint()?;
                let node = node?;
                let id = node.field("id")?.ok_or("missing finding id")?.read(65536)?;
                if let Some(id) = id.as_str().filter(|id| findings.contains(id)) {
                    let value = node.read(remaining)?;
                    remaining = remaining
                        .checked_sub(node.byte_len())
                        .ok_or("Claim evidence exceeds 8 MiB")?;
                    view.findings.insert(id.to_owned(), (ordinal, value));
                }
            }
        }
        for pointer in existence {
            indexed.checkpoint()?;
            if indexed.root.pointer(pointer)?.is_some() {
                view.present.insert(pointer.to_owned());
            }
        }
        for (key, (pointer, operation)) in counts {
            indexed.checkpoint()?;
            let count = (|| -> Result<Option<usize>> {
                let Some(array) = indexed.root.pointer(pointer)?.filter(|n| n.is_array()) else {
                    return Ok(None);
                };
                view.arrays.insert(pointer.to_owned());
                let mut seen = crate::disk_index::DiskIndex::new(
                    array.len(),
                    indexed.limits.state_bytes / 16,
                )?;
                for node in array.children() {
                    indexed.checkpoint()?;
                    let item = node?.read(indexed.limits.item_bytes.min(LIMIT))?;
                    let Some(selected) = selected(&item, operation) else {
                        return Ok(None);
                    };
                    if !selected {
                        continue;
                    }
                    let Some(occurrence) = item
                        .get("occurrence")
                        .filter(|o| o.get("evidence_ref").is_some())
                    else {
                        return Ok(None);
                    };
                    seen.insert(occurrence.to_string().as_bytes(), 1)?;
                }
                Ok(Some(usize::try_from(seen.len())?))
            })()?;
            view.counts.insert(key, count);
        }
        Ok(view)
    }
}
impl Evidence for IndexedView<'_> {
    fn metadata(&self) -> &Value {
        self.metadata
    }
    fn pointer(&self, pointer: &str) -> Option<&Value> {
        self.pointers.get(pointer)
    }
    fn finding(&self, id: &str) -> Option<(usize, &Value)> {
        self.findings.get(id).map(|(n, v)| (*n, v))
    }
    fn count(&self, pointer: &str, operation: Option<&CountOperation>) -> Option<usize> {
        self.counts
            .get(&count_key(pointer, operation))
            .copied()
            .flatten()
    }
    fn array_exists(&self, pointer: &str) -> bool {
        self.arrays.contains(pointer)
    }
    fn exists(&self, pointer: &str) -> bool {
        self.present.contains(pointer) || self.pointers.contains_key(pointer)
    }
}
