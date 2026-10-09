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
    pub configuration_bytes: u64,
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
            configuration_bytes: 0,
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
        true
    }
    pub fn physical_size(&mut self, bytes: usize) -> bool {
        if bytes > self.limits.record_bytes {
            return self.stop("parse", "record_limit", Some("record_bytes"));
        }
        self.checkpoint("parse", 1)
    }
    /// Reserve before parsing/cloning. The multiplier includes JSON node overhead,
    /// raw/normalized records, classification, correlation, artifact and report copies.
    /// Field mapping can duplicate the entire row once per configured mapping.
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
            .saturating_add(4096)
            .saturating_mul(128u64.saturating_add((mappings as u64).saturating_mul(64)));
        let allowance = allowance.saturating_add(self.configuration_bytes.saturating_mul(16));
        if !self.reserve("parse", allowance) {
            return false;
        }
        self.records += 1;
        self.expanded_records += u64::from(expanded);
        true
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
            "memory_bytes":self.memory_bytes,"artifact_bytes":null})
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
