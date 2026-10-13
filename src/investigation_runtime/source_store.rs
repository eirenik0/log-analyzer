//! Replayable source evidence and completed correlations in plain temporary files.
use super::{
    Result,
    construction::{Disk, Rows},
};
use crate::{
    disk_index::DiskIndex,
    parser::LogEntry,
    perf_analyzer::{SourceLocation, TimedOperation, UnmatchedEvent},
};

pub(super) struct Entries {
    rows: Rows<LogEntry>,
    addresses: Option<DiskIndex>,
    disk: Disk,
    pub item_bytes: u64,
    pub state_bytes: u64,
}
impl Entries {
    pub fn new(disk: Disk, item_bytes: u64, state_bytes: u64) -> Result<Self> {
        Ok(Self {
            rows: Rows::new(disk.clone(), item_bytes)?,
            addresses: None,
            disk,
            item_bytes,
            state_bytes,
        })
    }
    pub fn empty_like(&self) -> Result<Self> {
        Self::new(self.disk.clone(), self.item_bytes, self.state_bytes)
    }
    pub fn rows<T: serde::Serialize + serde::de::DeserializeOwned>(&self) -> Result<Rows<T>> {
        Rows::new(self.disk.clone(), self.item_bytes)
    }
    pub fn push(&mut self, entry: &LogEntry) -> Result<()> {
        if self.addresses.is_some() {
            return Err("cannot append to sealed source evidence".into());
        }
        self.rows.push(entry)
    }
    pub fn seal(&mut self) -> Result<()> {
        self.seal_with(|| true)
    }
    pub fn seal_with(&mut self, mut checkpoint: impl FnMut() -> bool) -> Result<()> {
        let mut addresses = DiskIndex::new(self.len() as u64, self.state_bytes)?;
        for (ordinal, entry) in self.iter()?.enumerate() {
            if !checkpoint() {
                return Err("source index construction interrupted".into());
            }
            let entry = entry?;
            addresses.insert(
                &address(entry.source_line_number, entry.source_row_path.as_deref())?,
                ordinal as u64,
            )?;
        }
        self.addresses = Some(addresses);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn iter(&self) -> Result<impl Iterator<Item = Result<LogEntry>> + '_> {
        self.rows.iter()
    }
    pub fn get(&self, ordinal: usize) -> Result<LogEntry> {
        self.rows.get(ordinal)
    }
    pub fn ordinal(&self, line: usize, row: Option<&str>) -> Result<Option<usize>> {
        Ok(self
            .addresses
            .as_ref()
            .ok_or("source evidence is not sealed")?
            .get(&address(line, row)?)?
            .map(|value| value as usize))
    }
    pub fn lookup(&self, source: &SourceLocation) -> Result<Option<LogEntry>> {
        self.ordinal(source.line, source.row_path.as_deref())?
            .map(|ordinal| self.get(ordinal))
            .transpose()
    }
    pub fn contains(&self, entry: &LogEntry) -> Result<bool> {
        Ok(self
            .ordinal(entry.source_line_number, entry.source_row_path.as_deref())?
            .is_some())
    }
}
pub(super) fn address(line: usize, row: Option<&str>) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&(line, row))?)
}

pub(super) struct Correlated {
    pub operations: Rows<TimedOperation>,
    pub unmatched_events: Rows<UnmatchedEvent>,
}
impl Correlated {
    pub fn new(entries: &Entries) -> Result<Self> {
        Ok(Self {
            operations: entries.rows()?,
            unmatched_events: entries.rows()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sealed_sources_replay_independently_and_resolve_normalized_addresses() {
        let mut entries = Entries::new(Disk::new(1024 * 1024), 65536, 65536).unwrap();
        for row in [None, Some("/events/0"), Some("/events/1")] {
            let mut entry =
                crate::parser::parse_log_entry("2025-01-01T00:00:00Z INFO app: replay", 7).unwrap();
            entry.source_row_path = row.map(str::to_owned);
            entries.push(&entry).unwrap();
        }
        entries.seal().unwrap();
        assert_eq!(entries.ordinal(7, None).unwrap(), Some(0));
        assert_eq!(entries.ordinal(7, Some("/events/1")).unwrap(), Some(2));
        assert_eq!(entries.ordinal(7, Some("/events/2")).unwrap(), None);
        let mut replay = entries.iter().unwrap();
        assert!(replay.next().unwrap().unwrap().source_row_path.is_none());
        assert_eq!(
            entries.get(2).unwrap().source_row_path.as_deref(),
            Some("/events/1")
        );
        assert_eq!(
            replay.next().unwrap().unwrap().source_row_path.as_deref(),
            Some("/events/0")
        );
        drop(replay);
        assert!(entries.push(&entries.get(0).unwrap()).is_err());
        assert!(entries.get(3).is_err());
    }
    #[test]
    fn interrupted_index_never_exposes_a_partially_resolved_source_set() {
        let mut entries = Entries::new(Disk::new(1024 * 1024), 65536, 65536).unwrap();
        for line in 1..=3 {
            entries
                .push(
                    &crate::parser::parse_log_entry("2025-01-01T00:00:00Z INFO app: replay", line)
                        .unwrap(),
                )
                .unwrap();
        }
        let mut work = 0;
        assert!(
            entries
                .seal_with(|| {
                    work += 1;
                    work < 2
                })
                .is_err()
        );
        assert!(entries.ordinal(1, None).is_err());
        assert_eq!(entries.iter().unwrap().count(), 3);
        entries.seal().unwrap();
        assert_eq!(entries.ordinal(3, None).unwrap(), Some(2));
    }
    #[test]
    fn address_index_exhaustion_does_not_publish_an_incomplete_index() {
        let mut entries = Entries::new(Disk::new(1024 * 1024), 65536, 128).unwrap();
        entries
            .push(
                &crate::parser::parse_log_entry("2025-01-01T00:00:00Z INFO app: replay", 1)
                    .unwrap(),
            )
            .unwrap();
        assert!(entries.seal().is_err());
        assert!(entries.ordinal(1, None).is_err());
    }
}
