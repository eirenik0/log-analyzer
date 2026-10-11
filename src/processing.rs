//! Cooperative investigation budgets. Accounting bounds retained data, not process RSS.
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Debug)]
pub(crate) struct Limits {
    pub input_bytes: u64,
    pub records: u64,
    pub expanded_records: u64,
    pub work_units: u64,
    pub elapsed_ms: u64,
    pub memory_bytes: u64,
    pub artifact_bytes: u64,
    pub record_bytes: usize,
    pub cancel_file: Option<PathBuf>,
}

pub(crate) struct Budget {
    pub limits: Limits,
    pub input_bytes: u64,
    pub records: u64,
    pub expanded_records: u64,
    pub work_units: u64,
    pub memory_bytes: u64,
    peak_memory_bytes: u64,
    pub stop: Option<Value>,
    pub halted: bool,
    pub active_scope: usize,
    pub affected_scopes: BTreeSet<usize>,
    started: Instant,
}
impl Budget {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            input_bytes: 0,
            records: 0,
            expanded_records: 0,
            work_units: 0,
            memory_bytes: 0,
            peak_memory_bytes: 0,
            stop: None,
            halted: false,
            active_scope: 0,
            affected_scopes: BTreeSet::new(),
            started: Instant::now(),
        }
    }
    pub fn begin_stage(&mut self) {
        self.halted = false;
    }
    pub fn stop(&mut self, stage: &str, reason: &str, limit: Option<&str>) -> bool {
        self.halted = true;
        self.affected_scopes.insert(self.active_scope);
        if self.stop.is_none() {
            self.stop = Some(
                json!({"stage":stage,"reason":reason,"limit_name":limit,"scope_ids":["scope-0"]}),
            );
        }
        false
    }
    pub fn checkpoint(&mut self, stage: &str, work: u64) -> bool {
        if self.halted {
            return false;
        }
        if self
            .limits
            .cancel_file
            .as_ref()
            .is_some_and(|path| path.exists())
        {
            return self.stop(stage, "cancelled", None);
        }
        if self.started.elapsed().as_millis() >= u128::from(self.limits.elapsed_ms) {
            return self.stop(stage, "time_limit", Some("elapsed_ms"));
        }
        if work > self.limits.work_units.saturating_sub(self.work_units) {
            return self.stop(stage, "work_limit", Some("work_units"));
        }
        self.work_units += work;
        true
    }
    pub fn reserve(&mut self, stage: &str, bytes: u64) -> bool {
        if bytes > self.limits.memory_bytes.saturating_sub(self.memory_bytes) {
            return self.stop(stage, "memory_limit", Some("memory_bytes"));
        }
        self.memory_bytes += bytes;
        self.peak_memory_bytes = self.peak_memory_bytes.max(self.memory_bytes);
        true
    }
    pub fn physical_size(&mut self, bytes: usize) -> bool {
        if bytes > self.limits.record_bytes {
            return self.stop("parse", "record_limit", Some("record_bytes"));
        }
        self.checkpoint("parse", 1)
    }
    /// Reserve before parsing/cloning. Source amplification covers JSON nodes,
    /// raw/normalized records and downstream copies. Fixed record/evidence metadata
    /// is charged separately, not amplified as if it were source text. Field mapping
    /// can duplicate the entire row once per configured mapping.
    pub fn record(&mut self, bytes: usize, mappings: usize, expanded: bool) -> bool {
        if !self.checkpoint("classification", (bytes as u64).saturating_add(1)) {
            return false;
        }
        if expanded && self.expanded_records >= self.limits.expanded_records {
            return self.stop("parse", "expansion_limit", Some("expanded_records"));
        }
        if self.records >= self.limits.records {
            return self.stop("parse", "record_limit", Some("records"));
        }
        let allowance = (bytes as u64)
            .saturating_mul(128u64.saturating_add((mappings as u64).saturating_mul(64)))
            .saturating_add(16 * 1024);
        if !self.reserve("parse", allowance) {
            return false;
        }
        self.records += 1;
        self.expanded_records += u64::from(expanded);
        true
    }
    /// Compiled rules are shared. Charge only the owned classification retained on
    /// this record (including literal values, rule IDs and invalid/conflict details).
    /// The one-time configuration reservation covers scratch classification storage.
    pub fn retain_classification(
        &mut self,
        classification: &Option<crate::event_rules::ClassifiedRecord>,
    ) -> bool {
        #[derive(Default)]
        struct Size(u64);
        impl std::io::Write for Size {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0 = self.0.saturating_add(bytes.len() as u64);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut size = Size::default();
        serde_json::to_writer(&mut size, classification)
            .expect("classification serializes to a counting writer");
        self.reserve("classification", size.0.saturating_mul(32))
    }
    /// Parsing reserves worst-case JSON amplification before allocating. Once a
    /// native text record is parsed, retain text copies and its actual structured
    /// storage instead. JSON envelopes and normalization keep their original charge.
    pub fn settle_text_record(&mut self, bytes: usize, entry: &crate::parser::LogEntry) {
        fn json_storage(value: &Value) -> u64 {
            let heap = match value {
                Value::String(text) => text.capacity() as u64,
                Value::Array(items) => items.iter().fold(
                    (items.capacity() as u64).saturating_mul(std::mem::size_of::<Value>() as u64),
                    |total, item| total.saturating_add(json_storage(item)),
                ),
                Value::Object(fields) => fields.iter().fold(0u64, |total, (key, value)| {
                    total
                        .saturating_add(128)
                        .saturating_add(key.capacity() as u64)
                        .saturating_add(json_storage(value))
                }),
                _ => 0,
            };
            heap.saturating_add(std::mem::size_of::<Value>() as u64)
        }
        let fields = entry.structured_fields.iter().fold(
            (entry.structured_fields.capacity() as u64).saturating_mul(128),
            |total, (key, value)| {
                total
                    .saturating_add(key.capacity() as u64)
                    .saturating_add(value.capacity() as u64)
            },
        );
        let structured = fields
            .saturating_add(entry.payload().map_or(0, json_storage))
            .saturating_add(entry.envelope_payload.as_ref().map_or(0, json_storage));
        // Parsed entries, correlation clones and artifact trees coexist. Eight
        // structured copies leave headroom over those trees; escaped text has a
        // separate allowance. Fixed metadata and classification stay reserved.
        let retained = (bytes as u64)
            .saturating_mul(32)
            .saturating_add(structured.saturating_mul(8));
        let excess = (bytes as u64).saturating_mul(128).saturating_sub(retained);
        self.memory_bytes = self.memory_bytes.saturating_sub(excess);
    }
    pub fn limits_json(&self) -> Value {
        json!({"input_bytes":self.limits.input_bytes,"records":self.limits.records,
            "expanded_records":self.limits.expanded_records,"work_units":self.limits.work_units,
            "elapsed_ms":self.limits.elapsed_ms,"memory_bytes":self.limits.memory_bytes,
            "artifact_bytes":self.limits.artifact_bytes})
    }
    pub fn usage_json(&self) -> Value {
        json!({"input_bytes":self.input_bytes,"records":self.records,
            "expanded_records":self.expanded_records,"work_units":self.work_units,
            "elapsed_ms":self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            "memory_bytes":self.peak_memory_bytes,"artifact_bytes":null})
    }
}

#[cfg(test)]
pub(crate) fn test_budget() -> Budget {
    Budget::new(Limits {
        input_bytes: 1024 * 1024,
        records: 1000,
        expanded_records: 1000,
        work_units: 10_000_000,
        elapsed_ms: 60_000,
        memory_bytes: 128 * 1024 * 1024,
        artifact_bytes: 16 * 1024 * 1024,
        record_bytes: 262144,
        cancel_file: None,
    })
}

#[cfg(test)]
#[path = "../tests/unit/text_memory.rs"]
mod text_memory_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_rules::ClassifiedRecord;

    #[test]
    fn retained_classification_cost_scales_with_owned_provenance() {
        let mut budget = test_budget();
        assert!(budget.record(200, 0, false));
        let source_cost = budget.memory_bytes;
        assert!(budget.retain_classification(&Some(ClassifiedRecord::Unclassified)));
        let small_cost = budget.memory_bytes - source_cost;
        let before = budget.memory_bytes;
        assert!(
            budget.retain_classification(&Some(ClassifiedRecord::Conflict {
                kinds: Vec::new(),
                profile: "synthetic".into(),
                rule_ids: vec!["r".repeat(4096); 8],
            }))
        );
        assert!(budget.memory_bytes - before > small_cost * 100);
    }

    #[test]
    fn classification_cutoff_does_not_overrun_memory_allowance() {
        let mut budget = test_budget();
        assert!(budget.record(200, 0, false));
        let before = budget.memory_bytes;
        budget.limits.memory_bytes = before + 1;
        assert!(!budget.retain_classification(&Some(ClassifiedRecord::Unclassified)));
        assert_eq!(budget.memory_bytes, before);
        assert!(budget.halted);
        let stop = budget.stop.unwrap();
        assert_eq!(stop["stage"], "classification");
        assert_eq!(stop["reason"], "memory_limit");
    }
}
