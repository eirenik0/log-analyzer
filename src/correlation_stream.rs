//! Bounded, input-scoped correlation for yielding parser consumers.
//!
//! Records and group decisions survive read chunks in private temporary files.
//! The resident index spills to SQLite only after its configured allowance is
//! reached. Output is final only after `finish` succeeds: a consumer should stage
//! its output and publish it atomically, since cancellation or I/O can fail late.
use crate::{
    config::AnalyzerConfig,
    event_rules::{ClassifiedRecord, Phase},
    parser::LogEntry,
    perf_analyzer::{self, TimedOperation, UnmatchedEvent},
    spill::{Order, Spool, ordered_key},
};
use serde::{Deserialize, Serialize};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Limits for the disposable correlation state, independent of artifact storage.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Total resident index allowance. SQLite additionally uses at most two
    /// 2 MiB page caches. Read pages contain at most 128 index entries each.
    pub index_bytes: usize,
    /// Total serialized payload and SQLite page allowance, split between records
    /// and group summaries. Temporary files are removed on success or failure.
    pub disk_bytes: u64,
    /// Maximum serialized record or group summary size, including provenance.
    /// This can exceed the parser's physical-record limit due to normalization.
    pub record_bytes: u64,
}

/// One finalized lifecycle decision. At most two source records are resident.
/// Unclassified and start-only records are emitted with neither result present.
/// Ambiguous records share a stable group ID; no tentative pair is emitted.
pub struct Decision {
    pub records: Vec<LogEntry>,
    pub operation: Option<TimedOperation>,
    pub unmatched: Option<UnmatchedEvent>,
    pub ambiguity_group: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    entry: LogEntry,
    group: Vec<u8>,
    paired: bool,
    start: bool,
}
#[derive(Default, Serialize, Deserialize)]
struct Summary {
    inferred: bool,
    overlap: bool,
    active: bool,
}

/// Owns state for one or more independent inputs. Every record must have an input
/// ordinal and a unique `(ordinal, physical line, normalized row path)` address.
/// One ordinal must refer to one source file. These requirements prevent inferred
/// ordering of duplicate addresses or unrelated sources at the same timestamp.
pub struct CorrelationStream {
    records: Spool,
    groups: Spool,
    config: AnalyzerConfig,
    serial: u64,
    failed: bool,
}
impl CorrelationStream {
    pub fn new(config: AnalyzerConfig, limits: Limits) -> Result<Self> {
        Ok(Self {
            records: Spool::new(
                limits.index_bytes / 2,
                limits.disk_bytes / 2,
                limits.record_bytes,
            )?,
            groups: Spool::new(
                limits.index_bytes / 2,
                limits.disk_bytes / 2,
                limits.record_bytes,
            )?,
            config,
            serial: 0,
            failed: false,
        })
    }
    pub fn spilled(&self) -> bool {
        self.records.spilled() || self.groups.spilled()
    }
    pub fn len(&self) -> u64 {
        self.serial
    }
    pub fn is_empty(&self) -> bool {
        self.serial == 0
    }
    pub fn push(&mut self, entry: LogEntry) -> Result<()> {
        if self.failed {
            return Err("correlation stream previously failed".into());
        }
        let result = self.push_record(entry);
        self.failed |= result.is_err();
        result
    }
    fn push_record(&mut self, entry: LogEntry) -> Result<()> {
        let ordinal = entry
            .source_input_ordinal
            .ok_or("stream correlation requires an input ordinal")?;
        if entry.source_line_number == 0 {
            return Err("stream correlation requires a physical source line".into());
        }
        let input_id = ordinal.to_string();
        if let Some(row) = self.records.find("inputs", &input_id)? {
            let file: Option<String> = self.records.read(&row)?;
            if file != entry.source_file {
                return Err("input ordinal refers to multiple source files".into());
            }
        } else {
            self.records
                .append("inputs", &[], &input_id, &entry.source_file)?;
        }
        let address = crate::evidence::digest(
            serde_json::to_string(&(ordinal, entry.source_line_number, &entry.source_row_path))?
                .as_bytes(),
        );
        if self.records.find("records", &address)?.is_some() {
            return Err("duplicate source address in correlation stream".into());
        }
        let mut group = Vec::new();
        group.extend_from_slice(&(ordinal as u64).to_be_bytes());
        let boundary = match &entry.classification {
            Some(ClassifiedRecord::Event { semantics, .. })
                if semantics.phase.is_some()
                    && !(semantics.phase == Some(Phase::Start) && !semantics.end_expected)
                    && semantics
                        .correlation_id
                        .as_ref()
                        .is_some_and(|id| !id.is_empty()) =>
            {
                perf_analyzer::correlation_scope(&entry, &self.config).map(|scope| {
                    let mut fields = vec![
                        semantics.kind.label().to_owned(),
                        semantics.name.clone(),
                        semantics.correlation_id.clone().unwrap(),
                    ];
                    fields.extend(scope);
                    (ordered_key(&fields), semantics.phase == Some(Phase::Start))
                })
            }
            _ => None,
        };
        let paired = boundary.is_some();
        let start = boundary.as_ref().is_some_and(|(_, start)| *start);
        if let Some((key, _)) = boundary {
            group.push(1);
            group.extend(key);
        } else {
            group.push(0);
            group.extend_from_slice(&self.serial.to_be_bytes());
        }
        let mut key = group.clone();
        key.extend_from_slice(&((entry.timestamp.timestamp() as u64) ^ (1 << 63)).to_be_bytes());
        key.extend_from_slice(&entry.timestamp.timestamp_subsec_nanos().to_be_bytes());
        key.extend_from_slice(&(entry.source_line_number as u64).to_be_bytes());
        self.records.append(
            "records",
            &key,
            &address,
            &Stored {
                entry,
                group,
                paired,
                start,
            },
        )?;
        self.serial += 1;
        Ok(())
    }
    /// Replay source order without materializing the record collection.
    pub fn visit_records(&self, mut consume: impl FnMut(LogEntry) -> Result<()>) -> Result<()> {
        if self.failed {
            return Err("correlation stream previously failed".into());
        }
        self.records.visit("records", Order::Source, |row| {
            consume(self.records.read::<Stored>(row)?.entry)
        })
    }
    /// Finalize after the source is exhausted. The first ordered pass establishes
    /// whole-key ambiguity; the second yields decisions. Checkpoints run for every
    /// visited record. Returning false stops immediately with an error.
    pub fn finish(
        mut self,
        mut checkpoint: impl FnMut() -> bool,
        mut consume: impl FnMut(Decision) -> Result<()>,
    ) -> Result<()> {
        if self.failed {
            return Err("correlation stream previously failed".into());
        }
        let mut current: Option<Vec<u8>> = None;
        let mut summary = Summary::default();
        let records = &self.records;
        let groups = &mut self.groups;
        records.visit("records", Order::Key, |row| {
            if !checkpoint() {
                return Err("correlation cancelled".into());
            }
            let stored: Stored = records.read(row)?;
            if !stored.paired {
                return Ok(());
            }
            if current.as_ref() != Some(&stored.group) {
                if let Some(key) = current.take() {
                    groups.append("groups", &key, &crate::evidence::digest(&key), &summary)?;
                }
                current = Some(stored.group.clone());
                summary = Summary::default();
            }
            summary.inferred |= stored.entry.timestamp_year_inferred;
            summary.overlap |= stored.start && summary.active;
            summary.active = stored.start;
            Ok(())
        })?;
        if let Some(key) = current {
            groups.append("groups", &key, &crate::evidence::digest(&key), &summary)?;
        }
        let mut current = None;
        let mut summary = Summary::default();
        let mut pending: Option<LogEntry> = None;
        records.visit("records", Order::Key, |row| {
            if !checkpoint() {
                return Err("correlation cancelled".into());
            }
            let stored: Stored = records.read(row)?;
            if current.as_ref() != Some(&stored.group) {
                if let Some(entry) = pending.take() {
                    emit_single(entry, &self.config, None, None, &mut consume)?;
                }
                current = Some(stored.group.clone());
                summary = if stored.paired {
                    let group = groups
                        .find("groups", &crate::evidence::digest(&stored.group))?
                        .ok_or("missing correlation group")?;
                    groups.read(&group)?
                } else {
                    Summary::default()
                };
            }
            if !stored.paired {
                emit_single(stored.entry, &self.config, None, None, &mut consume)?;
            } else if summary.inferred || summary.overlap {
                let reason = if summary.inferred {
                    "incomplete_timestamp_year"
                } else {
                    "overlapping_starts"
                };
                let ambiguity = (!summary.inferred).then(|| crate::evidence::digest(&stored.group));
                emit_single(
                    stored.entry,
                    &self.config,
                    Some(reason),
                    ambiguity,
                    &mut consume,
                )?;
            } else if stored.start {
                pending = Some(stored.entry);
            } else if let Some(start) = pending.take() {
                let entries = vec![start, stored.entry];
                let mut result = perf_analyzer::analyze_performance_with_config(
                    &entries,
                    &crate::comparator::LogFilter::new(),
                    None,
                    &self.config,
                );
                let operation = result
                    .operations
                    .pop()
                    .ok_or("paired stream boundaries did not produce an operation")?;
                consume(Decision {
                    records: entries,
                    operation: Some(operation),
                    unmatched: None,
                    ambiguity_group: None,
                })?;
            } else {
                emit_single(stored.entry, &self.config, None, None, &mut consume)?;
            }
            Ok(())
        })?;
        if let Some(entry) = pending {
            emit_single(entry, &self.config, None, None, &mut consume)?;
        }
        Ok(())
    }
}
fn emit_single(
    entry: LogEntry,
    config: &AnalyzerConfig,
    reason: Option<&str>,
    ambiguity_group: Option<String>,
    consume: &mut impl FnMut(Decision) -> Result<()>,
) -> Result<()> {
    let entries = vec![entry];
    let mut result = perf_analyzer::analyze_performance_with_config(
        &entries,
        &crate::comparator::LogFilter::new(),
        None,
        config,
    );
    let mut unmatched = result.unmatched_events.pop();
    if let (Some(reason), Some(event)) = (reason, &mut unmatched) {
        event.reason = reason.into();
    }
    consume(Decision {
        records: entries,
        operation: None,
        unmatched,
        ambiguity_group,
    })
}
