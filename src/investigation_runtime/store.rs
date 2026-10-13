//! Indexed storage of already validated evidence; no inference occurs on retrieval.
use super::Result;
use crate::cli::{Cli, InvestigationEvidenceArgs};
use rusqlite::{Connection, OpenFlags, params};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

const APPLICATION_ID: i64 = 0x4c414531;
const COLLECTIONS: &[&str] = &[
    "findings",
    "records",
    "memberships",
    "sequences",
    "captured_inputs",
    "populations",
];

fn configure(connection: &Connection, cache: usize) -> Result<()> {
    if cache < 4096 || cache > i32::MAX as usize {
        return Err("Evidence cache must be between 4096 and 2147483647 bytes".into());
    }
    connection.pragma_update(None, "mmap_size", 0)?;
    connection.pragma_update(None, "cache_size", -((cache / 1024) as i64))?;
    connection.pragma_update(None, "temp_store", "FILE")?;
    Ok(())
}

pub(super) fn write(
    path: &Path,
    retained: &Value,
    limit: u64,
    cancel: Option<&Path>,
) -> Result<()> {
    let mut connection = Connection::open(path)?;
    configure(&connection, 8 * 1024 * 1024)?;
    connection.pragma_update(None, "page_size", 4096)?;
    connection.pragma_update(
        None,
        "max_page_count",
        (limit / 4096).clamp(1, i32::MAX as u64) as i64,
    )?;
    connection.pragma_update(None, "journal_mode", "DELETE")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.pragma_update(None, "application_id", APPLICATION_ID)?;
    connection.pragma_update(None, "user_version", 1)?;
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "CREATE TABLE metadata (value TEXT NOT NULL, bytes INTEGER NOT NULL);
        CREATE TABLE collections (path TEXT PRIMARY KEY, count INTEGER NOT NULL);
        CREATE TABLE items (path TEXT NOT NULL, ordinal INTEGER NOT NULL,
            value TEXT NOT NULL, bytes INTEGER NOT NULL, PRIMARY KEY (path, ordinal)) WITHOUT ROWID;
        CREATE TABLE identities (path TEXT NOT NULL, id TEXT NOT NULL, ordinal INTEGER NOT NULL,
            PRIMARY KEY (path, id, ordinal)) WITHOUT ROWID;",
    )?;
    let mut metadata = serde_json::Map::new();
    for (key, value) in retained.as_object().ok_or("Artifact is not an object")? {
        if !COLLECTIONS.contains(&key.as_str()) {
            metadata.insert(key.clone(), value.clone());
        }
    }
    let metadata = serde_json::to_string(&metadata)?;
    transaction.execute(
        "INSERT INTO metadata VALUES (?1, ?2)",
        params![metadata, metadata.len() as i64],
    )?;
    fn insert<'a>(
        transaction: &rusqlite::Transaction<'_>,
        path: &str,
        values: impl ExactSizeIterator<Item = &'a Value>,
        cancel: Option<&Path>,
    ) -> Result<()> {
        transaction.execute(
            "INSERT INTO collections VALUES (?1, ?2)",
            params![path, values.len() as i64],
        )?;
        for (ordinal, value) in values.enumerate() {
            if cancel.is_some_and(Path::exists) {
                return Err("Artifact write cancelled".into());
            }
            let text = serde_json::to_string(value)?;
            transaction.execute(
                "INSERT INTO items VALUES (?1, ?2, ?3, ?4)",
                params![path, ordinal as i64, text, text.len() as i64],
            )?;
            let mut ids = std::collections::BTreeSet::new();
            for id in [
                value.get("id"),
                value.get("population_id"),
                value.pointer("/occurrence/evidence_ref/reference_id"),
            ]
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            {
                ids.insert(id);
            }
            for id in ids {
                transaction.execute(
                    "INSERT INTO identities VALUES (?1, ?2, ?3)",
                    params![path, id, ordinal as i64],
                )?;
            }
        }
        Ok(())
    }
    let chains: Vec<_> = retained["findings"]
        .as_array()
        .ok_or("Missing findings")?
        .iter()
        .filter(|finding| finding["details"].get("event_chain").is_some())
        .collect();
    insert(&transaction, "/chain-bundles", chains.into_iter(), cancel)?;
    for key in COLLECTIONS {
        let values = retained[key]
            .as_array()
            .ok_or("Missing artifact collection")?;
        insert(&transaction, &format!("/{key}"), values.iter(), cancel)?;
        let child = match *key {
            "memberships" => Some("members"),
            "sequences" => Some("events"),
            _ => None,
        };
        if let Some(child) = child {
            for (ordinal, value) in values.iter().enumerate() {
                insert(
                    &transaction,
                    &format!("/{key}/{ordinal}/{child}"),
                    value[child]
                        .as_array()
                        .ok_or("Missing nested artifact collection")?
                        .iter(),
                    cancel,
                )?;
            }
        }
    }
    transaction.commit()?;
    connection.close().map_err(|(_, error)| error)?;
    if std::fs::metadata(path)?.len() > limit {
        return Err("Artifact byte limit exceeded".into());
    }
    Ok(())
}

/// Verify into a private immutable-by-convention snapshot. Opening the original
/// after hashing would allow a concurrent writer to replace verified evidence.
pub(super) fn snapshot(
    path: &Path,
    limit: u64,
    expected: &str,
) -> Result<(tempfile::NamedTempFile, String, bool)> {
    let mut source = File::open(path)?;
    if source.metadata()?.len() > limit {
        return Err("Artifact exceeds read limit".into());
    }
    let mut snapshot = tempfile::NamedTempFile::new()?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .ok_or("Artifact size overflow")?;
        if bytes > limit {
            return Err("Artifact exceeds read limit".into());
        }
        hash.update(&buffer[..count]);
        snapshot.write_all(&buffer[..count])?;
    }
    let hash = format!("{:x}", hash.finalize());
    if hash != expected {
        return Err(
            "Artifact digest differs from the original report; retained evidence was changed"
                .into(),
        );
    }
    snapshot.as_file_mut().seek(SeekFrom::Start(0))?;
    let mut magic = [0u8; 16];
    let count = snapshot.read(&mut magic)?;
    snapshot.as_file_mut().seek(SeekFrom::Start(0))?;
    Ok((
        snapshot,
        hash,
        count == 16 && &magic == b"SQLite format 3\0",
    ))
}

pub(super) struct Page {
    pub retained: Value,
    pub items: Vec<Value>,
    pub total: usize,
    pub prior: usize,
}

pub(super) fn read(path: &Path, cli: &Cli, args: &InvestigationEvidenceArgs) -> Result<Page> {
    if args.claims.is_some() {
        return Err(
            "Claim verification currently requires a JSON artifact; SQLite claims are unavailable"
                .into(),
        );
    }
    if args.evidence_record_max_bytes == 0 {
        return Err("Evidence record limit must be positive".into());
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure(&connection, args.evidence_cache_max_bytes)?;
    connection.set_limit(
        rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH,
        args.evidence_record_max_bytes
            .saturating_add(4096)
            .min(i32::MAX as usize) as i32,
    )?;
    connection.pragma_update(None, "trusted_schema", false)?;
    let application: i64 = connection.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if application != APPLICATION_ID || version != 1 {
        return Err("Unsupported indexed evidence storage version".into());
    }
    let metadata_size: i64 =
        connection.query_row("SELECT bytes FROM metadata", [], |r| r.get(0))?;
    if metadata_size < 0 || metadata_size as u64 > args.evidence_record_max_bytes as u64 {
        return Err("Evidence metadata exceeds record limit".into());
    }
    let metadata: String = connection.query_row("SELECT value FROM metadata", [], |r| r.get(0))?;
    if metadata.len() != metadata_size as usize {
        return Err("Malformed indexed metadata size".into());
    }
    let mut retained: Value = serde_json::from_str(&metadata)?;
    if retained["contract_version"] != 1 || retained["investigation_contract_version"] != 1 {
        return Err("Unsupported evidence artifact version".into());
    }
    for key in COLLECTIONS {
        retained[key] = Value::Array(Vec::new());
    }
    if !retained["report_metadata"]["evidence"]["inputs"].is_array()
        || !retained["processing"]["inputs"].is_array()
    {
        return Err("Malformed indexed evidence metadata".into());
    }
    for input in retained["report_metadata"]["evidence"]["inputs"]
        .as_array()
        .unwrap()
    {
        if input["bytes"].as_u64().is_none() || !input["sha256"].is_string() {
            return Err("Malformed indexed source manifest".into());
        }
    }
    let path = &args.collection;
    // All caller-controlled values are bound, including collection paths and IDs.
    let count: i64 =
        connection.query_row("SELECT count FROM collections WHERE path=?1", [path], |r| {
            r.get(0)
        })?;
    if count < 0 {
        return Err("Malformed indexed evidence count".into());
    }
    let total: i64 = if let Some(id) = &args.id {
        connection.query_row(
            "SELECT count(*) FROM identities WHERE path=?1 AND id=?2",
            params![path, id],
            |r| r.get(0),
        )?
    } else {
        count
    };
    // Binding is checked by the common retrieval code before any output.
    let prior = cli
        .report_cursor
        .as_ref()
        .map(|cursor| cursor.rsplit(':').next().unwrap_or("").parse::<usize>())
        .transpose()?
        .unwrap_or(0);
    if prior > total as usize {
        return Err("Cursor is outside the retained collection".into());
    }
    let sql = if args.id.is_some() {
        "SELECT items.ordinal, bytes FROM identities JOIN items USING(path, ordinal) WHERE identities.path=?1 AND id=?2 ORDER BY ordinal LIMIT ?3 OFFSET ?4"
    } else {
        "SELECT ordinal, bytes FROM items WHERE path=?1 AND (?2 IS NULL) ORDER BY ordinal LIMIT ?3 OFFSET ?4"
    };
    let max = if cli.complete_output {
        usize::MAX
    } else {
        cli.report_max_items.unwrap_or(20)
    };
    let mut statement = connection.prepare(sql)?;
    let mut rows = statement.query(params![
        path,
        args.id,
        max.min(i64::MAX as usize) as i64,
        prior as i64
    ])?;
    let mut items = Vec::new();
    let mut page_bytes = 0usize;
    while let Some(row) = rows.next()? {
        let ordinal: i64 = row.get(0)?;
        let size: i64 = row.get(1)?;
        if size < 0 || size as u64 > args.evidence_record_max_bytes as u64 {
            return Err(
                "Evidence item exceeds record limit; no evidence was omitted silently".into(),
            );
        }
        let size = size as usize;
        // JSON tree amplification is bounded conservatively, separately from
        // SQLite's page cache. This is an allocation allowance, not measured RSS.
        let allowance = size.saturating_mul(32).saturating_add(4096);
        if allowance > args.evidence_cache_max_bytes.saturating_sub(page_bytes) {
            if items.is_empty() {
                return Err("Evidence item exceeds retrieval page memory budget".into());
            }
            break;
        }
        let text: String = connection.query_row(
            "SELECT value FROM items WHERE path=?1 AND ordinal=?2",
            params![path, ordinal],
            |r| r.get(0),
        )?;
        if text.len() != size {
            return Err("Malformed indexed item size".into());
        }
        items.push(serde_json::from_str::<Value>(&text)?);
        page_bytes += allowance;
    }
    if items.len() > (total as usize).saturating_sub(prior)
        || (items.is_empty() && max > 0 && prior < total as usize)
    {
        return Err("Malformed indexed collection count or missing items".into());
    }
    Ok(Page {
        retained,
        items,
        total: total as usize,
        prior,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_snapshot_is_independent_of_subsequent_source_changes() {
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), b"original snapshot").unwrap();
        let expected = crate::evidence::digest(b"original snapshot");
        let (mut captured, hash, indexed) = snapshot(source.path(), 1024, &expected).unwrap();
        std::fs::write(source.path(), b"changed snapshot").unwrap();
        let mut bytes = Vec::new();
        captured.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"original snapshot");
        assert_eq!(hash, expected);
        assert!(!indexed);
        assert!(snapshot(source.path(), 1024, &expected).is_err());
    }
}
