//! Spillable keyed populations; source payloads remain in the replayable source file.
use super::{Result, source_store::Entries};
use crate::{
    parser::LogEntry,
    spill::{Order, Row, Spool, ordered_key},
};

pub(super) struct Groups {
    index: Spool,
}
impl Groups {
    pub fn new(entries: &Entries) -> Result<Self> {
        Ok(Self {
            index: Spool::new(1024 * 1024, entries.state_bytes, entries.item_bytes)?,
        })
    }
    pub fn insert(&mut self, collection: &str, key: &[String], ordinal: usize) -> Result<()> {
        let encoded = ordered_key(key);
        let identity = serde_json::to_string(key)?;
        let keys = format!("keys:{collection}");
        if self.index.find(&keys, &identity)?.is_none() {
            self.index.append(&keys, &encoded, &identity, &key)?;
        }
        self.index
            .append(&format!("rows:{collection}"), &encoded, "", &ordinal)?;
        Ok(())
    }
    pub fn put<T: serde::Serialize>(
        &mut self,
        collection: &str,
        id: &str,
        value: &T,
    ) -> Result<()> {
        self.index
            .append(&format!("values:{collection}"), id.as_bytes(), id, value)?;
        Ok(())
    }
    pub fn get<T: serde::de::DeserializeOwned>(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<Option<T>> {
        self.index
            .find(&format!("values:{collection}"), id)?
            .map(|row| self.index.read(&row))
            .transpose()
    }
    pub fn latest<T: serde::de::DeserializeOwned>(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<Option<T>> {
        self.index
            .find_last(&format!("values:{collection}"), id)?
            .map(|row| self.index.read(&row))
            .transpose()
    }
    /// Source-order rank lets later-activity counts use an indexed boundary seek.
    pub fn insert_ranked(
        &mut self,
        collection: &str,
        key: &[String],
        ordinal: usize,
    ) -> Result<()> {
        let identity = serde_json::to_string(&(collection, key))?;
        let previous = self.latest::<(usize, usize)>("rank-count", &identity)?;
        if previous.is_some_and(|(_, last)| ordinal <= last) {
            return Err("ranked state requires increasing source ordinals".into());
        }
        let rank = previous.map_or(1, |(rank, _)| rank + 1);
        self.index.append(
            &format!("rank:{identity}"),
            &(ordinal as u64).to_be_bytes(),
            "",
            &(ordinal, rank),
        )?;
        self.put("rank-count", &identity, &(rank, ordinal))
    }
    pub fn suffix(
        &self,
        collection: &str,
        key: &[String],
        after: usize,
    ) -> Result<Option<(usize, Vec<usize>)>> {
        let identity = serde_json::to_string(&(collection, key))?;
        let Some((count, _)) = self.latest::<(usize, usize)>("rank-count", &identity)? else {
            return Ok(None);
        };
        let collection = format!("rank:{identity}");
        let boundary = (after as u64).to_be_bytes();
        let before = self
            .index
            .last_before_key(&collection, &boundary)?
            .map(|row| self.index.read::<(usize, usize)>(&row))
            .transpose()?
            .map_or(0, |(_, rank)| rank);
        let mut tail = self
            .index
            .tail_after_key(&collection, &boundary, 3)?
            .iter()
            .map(|row| {
                self.index
                    .read::<(usize, usize)>(row)
                    .map(|(ordinal, _)| ordinal)
            })
            .collect::<Result<Vec<_>>>()?;
        tail.reverse();
        Ok(Some((
            count
                .checked_sub(before)
                .ok_or("invalid source-order rank")?,
            tail,
        )))
    }
    pub fn node(&mut self, collection: &str, ordinal: usize) -> Result<()> {
        self.insert(collection, &[format!("{ordinal:020}")], ordinal)
    }
    pub fn node_ordinals<'a>(
        &'a self,
        collection: &str,
    ) -> impl Iterator<Item = Result<usize>> + 'a {
        self.keys(collection, false).map(|key| Ok(key?[0].parse()?))
    }
    pub fn keys(&self, collection: &str, source_order: bool) -> Keys<'_> {
        Keys {
            index: &self.index,
            collection: format!("keys:{collection}"),
            order: if source_order {
                Order::Source
            } else {
                Order::Key
            },
            after: None,
            page: Vec::new().into_iter(),
            done: false,
        }
    }
    pub fn ordinals(&self, collection: &str, key: &[String]) -> Ordinals<'_> {
        Ordinals {
            index: &self.index,
            collection: format!("rows:{collection}"),
            key: ordered_key(key),
            after: None,
            page: Vec::new().into_iter(),
            done: false,
        }
    }
    pub fn count(
        &self,
        collection: &str,
        key: &[String],
        budget: &mut crate::processing::Budget,
    ) -> Result<usize> {
        self.ordinals(collection, key).try_fold(0, |count, row| {
            row?;
            check(budget)?;
            Ok(count + 1)
        })
    }
    pub fn entries<'a>(
        &'a self,
        entries: &'a Entries,
        collection: &str,
        key: &[String],
    ) -> impl Iterator<Item = Result<(usize, LogEntry)>> + 'a {
        self.ordinals(collection, key).map(|ordinal| {
            let ordinal = ordinal?;
            Ok((ordinal, entries.get(ordinal)?))
        })
    }
}
pub(super) struct Keys<'a> {
    index: &'a Spool,
    collection: String,
    order: Order,
    after: Option<Row>,
    page: std::vec::IntoIter<Row>,
    done: bool,
}
impl Iterator for Keys<'_> {
    type Item = Result<Vec<String>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if let Some(row) = self.page.next() {
            let result = self.index.read(&row);
            self.done = result.is_err();
            return Some(result);
        }
        match self
            .index
            .page(&self.collection, self.order, self.after.as_ref())
        {
            Err(error) => {
                self.done = true;
                Some(Err(error))
            }
            Ok(page) => {
                self.after = page.last().cloned();
                self.done = page.is_empty();
                self.page = page.into_iter();
                if self.done { None } else { self.next() }
            }
        }
    }
}
pub(super) struct Ordinals<'a> {
    index: &'a Spool,
    collection: String,
    key: Vec<u8>,
    after: Option<Row>,
    page: std::vec::IntoIter<Row>,
    done: bool,
}
impl Iterator for Ordinals<'_> {
    type Item = Result<usize>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if let Some(row) = self.page.next() {
            let result = self.index.read(&row);
            self.done = result.is_err();
            return Some(result);
        }
        match self
            .index
            .group_page(&self.collection, &self.key, self.after.as_ref())
        {
            Err(error) => {
                self.done = true;
                Some(Err(error))
            }
            Ok(page) => {
                self.after = page.last().cloned();
                self.done = page.is_empty();
                self.page = page.into_iter();
                if self.done { None } else { self.next() }
            }
        }
    }
}

pub(super) fn check(budget: &mut crate::processing::Budget) -> Result<()> {
    if budget.checkpoint("calculation", 1) {
        Ok(())
    } else {
        Err("keyed state replay interrupted".into())
    }
}
pub(super) fn last(
    mut rows: impl Iterator<Item = Result<usize>>,
    budget: &mut crate::processing::Budget,
) -> Result<Option<usize>> {
    rows.try_fold(None, |_, row| {
        check(budget)?;
        row.map(Some)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config,
        evidence::Context,
        processing::{Budget, Limits},
    };
    use serde_json::{Value, json};
    fn budget() -> Budget {
        Budget::new(Limits {
            input_bytes: u64::MAX,
            records: u64::MAX,
            expanded_records: u64::MAX,
            work_units: 100_000_000,
            elapsed_ms: 120_000,
            memory_bytes: 1024 * 1024 * 1024,
            artifact_bytes: u64::MAX,
            record_bytes: usize::MAX,
            cancel_file: None,
        })
    }
    fn fixture(
        events: &[Value],
        config: &config::AnalyzerConfig,
    ) -> (Vec<LogEntry>, Entries, Context) {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            events.iter().map(|v| format!("{v}\n")).collect::<String>(),
        )
        .unwrap();
        let parsed = crate::parser::parse_log_file_report(file.path(), config).unwrap();
        let mut context = Context::default();
        context.observe(&parsed.coverage, &parsed.entries);
        let mut entries = Entries::new(
            super::super::construction::Disk::new(128 * 1024 * 1024),
            8 * 1024 * 1024,
            128 * 1024 * 1024,
        )
        .unwrap();
        for entry in &parsed.entries {
            entries.push(entry).unwrap();
        }
        entries.seal().unwrap();
        (parsed.entries, entries, context)
    }
    fn event(name: &str, session: &str, capture: &str, poll: usize) -> Value {
        json!({"timestamp":"2026-01-01T02:00:00+02:00","level":"INFO","message":name,"record_type":"capture","event_type":name,"session_id":session,"capture_id":capture,"request_id":"request-1","broker_id":"broker-1","poll_id":poll.to_string(),"payload":{"asExpected":poll.is_multiple_of(2),"status":if poll.is_multiple_of(2){"Passed"}else{"Failed"}}})
    }
    #[test]
    fn ranked_suffix_matches_partitioned_source_order_before_and_after_spill() {
        for memory in [0, 1024 * 1024] {
            let mut groups = Groups {
                index: Spool::new(memory, 32 * 1024 * 1024, 1024).unwrap(),
            };
            for ordinal in 0..1000 {
                groups
                    .insert_ranked("later", &[format!("run-{}", ordinal % 2)], ordinal)
                    .unwrap();
            }
            for parity in 0..2 {
                let population: Vec<_> = (0..1000).filter(|n| n % 2 == parity).collect();
                for boundary in [0, 1, 13, 899, 998, 999, 1000] {
                    let expected: Vec<_> = population
                        .iter()
                        .copied()
                        .filter(|ordinal| *ordinal > boundary)
                        .collect();
                    let tail = expected[expected.len().saturating_sub(3)..].to_vec();
                    assert_eq!(
                        groups
                            .suffix("later", &[format!("run-{parity}")], boundary)
                            .unwrap(),
                        Some((expected.len(), tail))
                    );
                }
            }
            assert_eq!(
                groups.suffix("later", &["missing".into()], 0).unwrap(),
                None
            );
            assert!(groups.insert_ranked("later", &["run-0".into()], 0).is_err());
        }
    }
    #[test]
    fn keyed_count_interruption_never_returns_a_partial_cardinality() {
        let config = config::AnalyzerConfig::default();
        let (_, entries, _) = fixture(&[event("checkpoint", "run-1", "capture-1", 0)], &config);
        let mut groups = Groups::new(&entries).unwrap();
        for ordinal in 0..4 {
            groups.insert("group", &[], ordinal).unwrap();
        }
        let mut limited = budget();
        limited.limits.work_units = 2;
        assert!(groups.count("group", &[], &mut limited).is_err());
        assert!(limited.halted);
        assert_eq!(limited.stop.unwrap()["reason"], "work_limit");
        assert_eq!(groups.count("group", &[], &mut budget()).unwrap(), 4);
    }
    #[test]
    fn group_names_do_not_alias_internal_collections() {
        let config = config::AnalyzerConfig::default();
        let (_, entries, _) = fixture(&[event("checkpoint", "run-1", "capture-1", 0)], &config);
        let mut groups = Groups::new(&entries).unwrap();
        for (ordinal, name) in ["foo", "foo:keys", "keys:foo", "rows:foo", "values:foo"]
            .iter()
            .enumerate()
        {
            groups.insert(name, &["key".into()], ordinal).unwrap();
            groups.put(name, "key", &ordinal).unwrap();
        }
        for (ordinal, name) in ["foo", "foo:keys", "keys:foo", "rows:foo", "values:foo"]
            .iter()
            .enumerate()
        {
            assert_eq!(
                groups
                    .ordinals(name, &["key".into()])
                    .collect::<Result<Vec<_>>>()
                    .unwrap(),
                vec![ordinal]
            );
            assert_eq!(
                groups
                    .keys(name, false)
                    .collect::<Result<Vec<_>>>()
                    .unwrap(),
                vec![vec!["key"]]
            );
            assert_eq!(groups.get::<usize>(name, "key").unwrap(), Some(ordinal));
        }
    }
    #[test]
    fn streamed_chains_match_legacy_with_reused_roots_ambiguous_joins_and_later_activity() {
        let config = config::load_config_from_path(std::path::Path::new(
            "examples/investigations/chains/profile.toml",
        ))
        .unwrap();
        let mut events = vec![
            event("checkpoint", "run-1", "capture-1", 0),
            event("submitted", "run-1", "capture-1", 0),
            event("accepted", "run-1", "capture-1", 0),
        ];
        events.extend((0..350).map(|i| event("poll_send", "run-1", "capture-1", i)));
        events.extend([
            event("poll_response", "run-1", "capture-1", 1),
            event("poll_response", "run-1", "capture-1", 1),
            event("check_end", "run-1", "capture-1", 0),
            event("later_activity", "run-1", "capture-2", 0),
            event("checkpoint", "run-2", "capture-1", 0),
            event("checkpoint", "run-2", "capture-1", 0),
            event("submitted", "run-2", "capture-1", 0),
        ]);
        let mut missing = event("checkpoint", "run-3", "capture-3", 0);
        missing.as_object_mut().unwrap().remove("capture_id");
        events.push(missing);
        let (resident, entries, context) = fixture(&events, &config);
        let policy = config.investigation.as_ref().unwrap();
        let mut legacy = Vec::new();
        let mut streamed = Vec::new();
        super::super::chains::calculate(
            policy,
            &config,
            &resident,
            &[],
            "scope-0",
            &context,
            &json!("snapshot"),
            true,
            &mut budget(),
            &mut legacy,
            None,
        );
        super::super::chains::calculate_stream(
            policy,
            &config,
            &entries,
            &[],
            "scope-0",
            &context,
            &json!("snapshot"),
            true,
            &mut budget(),
            &mut streamed,
        )
        .unwrap();
        assert!(!legacy.is_empty());
        assert_eq!(streamed, legacy);
    }
    #[test]
    fn streamed_policy_matches_legacy_identity_populations_and_scoped_cardinality() {
        let config = config::load_config_from_path(std::path::Path::new(
            "examples/investigations/chains/profile.toml",
        ))
        .unwrap();
        let mut policy = config.investigation.clone().unwrap();
        policy.populations=serde_json::from_value(json!([{"id":"attempts","entity":"attempts","roles":["checkpoint","submitted"],"identity_fields":["capture_id"],"grouping":"identity"}])).unwrap();
        policy.relationships=serde_json::from_value(json!([{"id":"joins","source_role":"checkpoint","target_role":"submitted","join_fields":["capture_id"],"required_scope_fields":["session_id"],"cardinality":"one_to_many"}])).unwrap();
        let mut events = vec![
            event("checkpoint", "run-1", "capture-1", 0),
            event("checkpoint", "run-2", "capture-1", 0),
            event("checkpoint", "run-2", "capture-1", 0),
            event("submitted", "run-2", "capture-1", 0),
        ];
        events.extend((0..350).map(|i| event("submitted", "run-1", "capture-1", i)));
        let (resident, entries, context) = fixture(&events, &config);
        let (mut legacy, mut lp, mut lm) = (Vec::new(), Vec::new(), Vec::new());
        let (mut streamed, mut sp, mut sm) = (Vec::new(), Vec::new(), Vec::new());
        super::super::policy::calculate(
            &policy,
            &config,
            &resident,
            &[],
            "scope-0",
            &context,
            &json!("snapshot"),
            &mut budget(),
            &mut legacy,
            &mut lp,
            &mut lm,
            None,
        )
        .unwrap();
        super::super::policy::calculate_stream(
            &policy,
            &config,
            &entries,
            &[],
            "scope-0",
            &context,
            &json!("snapshot"),
            &mut budget(),
            &mut streamed,
            &mut sp,
            &mut sm,
        )
        .unwrap();
        assert_eq!(streamed, legacy);
        assert_eq!(sp, lp);
        assert_eq!(sm, lm);
    }
    #[test]
    fn streamed_resources_match_legacy_owners_hashes_and_viewport_resets() {
        let config = config::AnalyzerConfig::default();
        let rule=serde_json::from_value(json!({"id":"assets","scope_field":"context","scope_separator":":","namespace_prefix":"run-","owner_prefix":"operation-","manifest_marker":"manifest listing","url_contains":"asset:","resource_markers":["asset batch"],"resources_path":"bundle.resources","entries_field":"items","url_field":"address","hash_field":"digest","viewport_marker":"dimensions","viewport_reset_marker":"new snapshot","width_field":"w","height_field":"h","fingerprints":[{"sha256":"a".repeat(64),"label":"reference"}]})).unwrap();
        let line = |scope: &str, message: &str, payload: Value| json!({"timestamp":"2026-01-01T02:00:00+02:00","level":"INFO","message":message,"context":scope,"payload":payload});
        let events = vec![
            line("run-a:operation-1", "dimensions", json!({"w":100,"h":200})),
            line(
                "run-a:operation-1",
                "manifest listing",
                json!(["asset:one", "asset:shared"]),
            ),
            line(
                "run-a:operation-1",
                "asset batch",
                json!([{"bundle":{"resources":{"items":[{"address":"asset:one","digest":"a".repeat(64)},{"address":"asset:one","digest":"a".repeat(64)},{"address":"asset:shared","digest":"b".repeat(64)}]}}}]),
            ),
            line("run-a:operation-1", "new snapshot", Value::Null),
            line(
                "run-a:operation-1",
                "manifest listing",
                json!(["asset:one"]),
            ),
            line(
                "run-a:operation-2",
                "manifest listing",
                json!(["asset:shared"]),
            ),
            line("run-a:operation-2", "manifest listing", json!([])),
        ];
        let (resident, entries, context) = fixture(&events, &config);
        let (mut legacy, mut streamed) = (Vec::new(), Vec::new());
        super::super::resource_observations::analyze(
            &resident.iter().collect::<Vec<_>>(),
            "scope-0",
            &context,
            &json!("snapshot"),
            &rule,
            &mut budget(),
            &mut legacy,
        );
        super::super::resource_observations::analyze_stream(
            &entries,
            "scope-0",
            &context,
            &json!("snapshot"),
            &rule,
            &mut budget(),
            &mut streamed,
        )
        .unwrap();
        assert!(!legacy.is_empty());
        assert_eq!(streamed, legacy);
    }
}
