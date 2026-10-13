//! Disposable state indexes. Payloads are append-only files; SQLite is opened
//! only when the in-memory index reaches its fixed byte allowance.
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    cell::RefCell,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const PAGE: usize = 128;
#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub ordinal: u64,
    pub key: Vec<u8>,
    pub identity: String,
    offset: u64,
    length: u64,
}
#[derive(Clone, Copy)]
pub(crate) enum Order {
    Source,
    Key,
}
struct IndexRow {
    collection: String,
    row: Row,
}
pub(crate) struct Spool {
    payload: RefCell<File>,
    rows: Vec<IndexRow>,
    database: Option<Connection>,
    index_bytes: usize,
    memory_limit: usize,
    disk_limit: u64,
    item_limit: u64,
    payload_bytes: u64,
    next: u64,
    directory: tempfile::TempDir,
}
impl Spool {
    pub fn new(memory_limit: usize, disk_limit: u64, item_limit: u64) -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let payload = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(directory.path().join("payload"))?;
        Ok(Self {
            directory,
            payload: RefCell::new(payload),
            rows: Vec::new(),
            database: None,
            index_bytes: 0,
            memory_limit,
            disk_limit,
            item_limit,
            payload_bytes: 0,
            next: 0,
        })
    }
    pub fn spilled(&self) -> bool {
        self.database.is_some()
    }
    pub fn append(
        &mut self,
        collection: &str,
        key: &[u8],
        identity: &str,
        value: &impl Serialize,
    ) -> Result<u64> {
        struct Limited {
            data: Vec<u8>,
            limit: u64,
        }
        impl Write for Limited {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if bytes.len() as u64 > self.limit.saturating_sub(self.data.len() as u64) {
                    return Err(std::io::Error::other("spill item byte limit exceeded"));
                }
                self.data.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut encoded = Limited {
            data: Vec::new(),
            limit: self
                .item_limit
                .min(self.disk_limit.saturating_sub(self.payload_bytes)),
        };
        serde_json::to_writer(&mut encoded, value)?;
        self.append_bytes(collection, key, identity, &encoded.data)
    }
    pub fn append_bytes(
        &mut self,
        collection: &str,
        key: &[u8],
        identity: &str,
        bytes: &[u8],
    ) -> Result<u64> {
        if bytes.len() as u64 > self.item_limit {
            return Err("spill item byte limit exceeded".into());
        }
        if key
            .len()
            .saturating_add(identity.len())
            .saturating_add(collection.len())
            > 65536
        {
            return Err("spill index key byte limit exceeded (64 KiB)".into());
        }
        let cost =
            collection.len() + key.len() + identity.len() + std::mem::size_of::<IndexRow>() + 64;
        if self.database.is_none() && self.index_bytes.saturating_add(cost) > self.memory_limit {
            self.spill()?;
        }
        let length = bytes.len() as u64;
        let end = self
            .payload_bytes
            .checked_add(length)
            .ok_or("spill size overflow")?;
        if end > self.disk_limit {
            return Err("spill disk byte limit exceeded".into());
        }
        if let Some(database) = &self.database {
            self.bound_database(database, end)?;
        }
        let row = Row {
            ordinal: self.next,
            key: key.to_vec(),
            identity: identity.into(),
            offset: self.payload_bytes,
            length,
        };
        let mut file = self.payload.borrow_mut();
        file.seek(SeekFrom::Start(self.payload_bytes))?;
        if let Err(error) = file.write_all(bytes) {
            file.set_len(self.payload_bytes)?;
            return Err(error.into());
        }
        if let Some(database) = &self.database {
            if let Err(error) = database.execute(
                "INSERT INTO state VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    collection,
                    row.ordinal,
                    row.key,
                    row.identity,
                    row.offset,
                    row.length
                ],
            ) {
                file.set_len(self.payload_bytes)?;
                return Err(error.into());
            }
        } else {
            self.rows.push(IndexRow {
                collection: collection.into(),
                row: row.clone(),
            });
            self.index_bytes += cost;
        }
        self.payload_bytes = end;
        self.next += 1;
        Ok(row.ordinal)
    }
    fn bound_database(&self, database: &Connection, payload_bytes: u64) -> Result<()> {
        let pages: u64 = database.query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let page_size: u64 = database.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        let allowed = self.disk_limit.saturating_sub(payload_bytes) / page_size;
        if pages > allowed {
            return Err("spill disk byte limit exceeded".into());
        }
        database.pragma_update(None, "max_page_count", allowed.max(1))?;
        Ok(())
    }
    fn spill(&mut self) -> Result<()> {
        // The schema needs one table, three indexes and the schema page. Bound
        // SQLite before creating them, including for a zero/tiny disk allowance.
        if self.disk_limit.saturating_sub(self.payload_bytes) < 5 * 4096 {
            return Err("spill disk byte limit exceeded".into());
        }
        let mut database = Connection::open(self.directory.path().join("state.sqlite"))?;
        database.pragma_update(None, "page_size", 4096)?;
        self.bound_database(&database, self.payload_bytes)?;
        database.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF; PRAGMA temp_store=FILE; PRAGMA mmap_size=0; PRAGMA cache_size=-2048;
            CREATE TABLE state(collection TEXT NOT NULL, ordinal INTEGER PRIMARY KEY, sort_key BLOB NOT NULL, identity TEXT NOT NULL, offset INTEGER NOT NULL, length INTEGER NOT NULL);
            CREATE INDEX state_source ON state(collection, ordinal);
            CREATE INDEX state_sorted ON state(collection, sort_key, ordinal);
            CREATE INDEX state_identity ON state(collection, identity);")?;
        self.bound_database(&database, self.payload_bytes)?;
        let transaction = database.transaction()?;
        {
            let mut insert = transaction.prepare("INSERT INTO state VALUES (?1,?2,?3,?4,?5,?6)")?;
            for value in &self.rows {
                let row = &value.row;
                insert.execute(params![
                    value.collection,
                    row.ordinal,
                    row.key,
                    row.identity,
                    row.offset,
                    row.length
                ])?;
            }
        }
        transaction.commit()?;
        self.rows.clear();
        self.rows.shrink_to_fit();
        self.index_bytes = 0;
        self.database = Some(database);
        Ok(())
    }
    pub fn read<T: DeserializeOwned>(&self, row: &Row) -> Result<T> {
        if row.length > self.item_limit
            || row
                .offset
                .checked_add(row.length)
                .is_none_or(|end| end > self.payload_bytes)
        {
            return Err("invalid spill payload extent".into());
        }
        let mut file = self.payload.borrow_mut();
        file.seek(SeekFrom::Start(row.offset))?;
        let reader = std::io::BufReader::new((&mut *file).take(row.length));
        Ok(serde_json::from_reader(reader)?)
    }
    pub fn find(&self, collection: &str, identity: &str) -> Result<Option<Row>> {
        if let Some(database) = &self.database {
            Ok(database.query_row("SELECT ordinal,sort_key,identity,offset,length FROM state WHERE collection=?1 AND identity=?2 ORDER BY ordinal LIMIT 1", params![collection,identity], sql_row).optional()?)
        } else {
            Ok(self
                .rows
                .iter()
                .find(|row| row.collection == collection && row.row.identity == identity)
                .map(|row| row.row.clone()))
        }
    }
    pub fn page(&self, collection: &str, order: Order, after: Option<&Row>) -> Result<Vec<Row>> {
        if let Some(database) = &self.database {
            let (query, key) = match order {
                Order::Source => (
                    "SELECT ordinal,sort_key,identity,offset,length FROM state WHERE collection=?1 AND ordinal>?2 ORDER BY ordinal LIMIT 128",
                    false,
                ),
                Order::Key => (
                    "SELECT ordinal,sort_key,identity,offset,length FROM state WHERE collection=?1 AND (sort_key,ordinal)>(?3,?2) ORDER BY sort_key,ordinal LIMIT 128",
                    true,
                ),
            };
            let mut statement = database.prepare(query)?;
            let ordinal = after.map_or(-1, |row| row.ordinal as i64);
            let rows = if key {
                statement
                    .query_map(
                        params![
                            collection,
                            ordinal,
                            after.map_or(&[][..], |row| row.key.as_slice())
                        ],
                        sql_row,
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            } else {
                statement
                    .query_map(params![collection, ordinal], sql_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            Ok(rows)
        } else {
            // Borrow the bounded resident index while sorting; clone only a page.
            let mut rows: Vec<_> = self
                .rows
                .iter()
                .filter(|row| row.collection == collection)
                .filter(|row| {
                    after.is_none_or(|after| match order {
                        Order::Source => row.row.ordinal > after.ordinal,
                        Order::Key => (&row.row.key, row.row.ordinal) > (&after.key, after.ordinal),
                    })
                })
                .map(|row| &row.row)
                .collect();
            match order {
                Order::Source => rows.sort_by_key(|row| row.ordinal),
                Order::Key => rows.sort_by(|a, b| (&a.key, a.ordinal).cmp(&(&b.key, b.ordinal))),
            }
            let rows = rows.into_iter().take(PAGE).cloned().collect();
            Ok(rows)
        }
    }
    pub fn visit(
        &self,
        collection: &str,
        order: Order,
        mut consume: impl FnMut(&Row) -> Result<()>,
    ) -> Result<()> {
        let mut after = None;
        loop {
            let rows = self.page(collection, order, after.as_ref())?;
            if rows.is_empty() {
                break;
            }
            for row in &rows {
                consume(row)?;
            }
            after = rows.last().cloned();
        }
        Ok(())
    }
}
fn sql_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        ordinal: row.get(0)?,
        key: row.get(1)?,
        identity: row.get(2)?,
        offset: row.get(3)?,
        length: row.get(4)?,
    })
}

/// Byte encoding with the same ordering as a sequence of Rust strings.
pub(crate) fn ordered_key(parts: &[String]) -> Vec<u8> {
    let mut key = Vec::new();
    for part in parts {
        key.push(1);
        for byte in part.bytes() {
            if byte == 0 {
                key.extend_from_slice(&[0, 255]);
            } else {
                key.push(byte);
            }
        }
        key.extend_from_slice(&[0, 0]);
    }
    key.push(0);
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resident_and_spilled_indexes_have_identical_order_and_payloads() {
        let mut resident = Spool::new(1024 * 1024, 8 * 1024 * 1024, 1024).unwrap();
        let mut spilled = Spool::new(2048, 8 * 1024 * 1024, 1024).unwrap();
        for ordinal in 0..400 {
            let key = ordered_key(&[format!("key-{}", ordinal % 7), "\0é".into()]);
            for spool in [&mut resident, &mut spilled] {
                spool
                    .append(
                        "items",
                        &key,
                        &ordinal.to_string(),
                        &json!({"ordinal":ordinal}),
                    )
                    .unwrap();
            }
        }
        assert!(!resident.spilled());
        assert!(spilled.spilled());
        for order in [Order::Source, Order::Key] {
            let mut outputs = Vec::new();
            for spool in [&resident, &spilled] {
                let mut output = Vec::new();
                spool
                    .visit("items", order, |row| {
                        output.push(spool.read::<serde_json::Value>(row)?);
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(output.len(), 400);
                outputs.push(output);
            }
            assert_eq!(outputs[0], outputs[1]);
        }
        let row = spilled.find("items", "257").unwrap().unwrap();
        assert_eq!(
            spilled.read::<serde_json::Value>(&row).unwrap(),
            json!({"ordinal":257})
        );
    }

    #[test]
    fn limits_fail_explicitly_and_temporary_files_are_removed() {
        let mut spool = Spool::new(0, 1024 * 1024, 100).unwrap();
        let directory = spool.directory.path().to_owned();
        assert!(spool.append("items", b"", "0", &"x".repeat(101)).is_err());
        spool.append("items", b"", "0", &42).unwrap();
        assert!(spool.spilled());
        drop(spool);
        assert!(!directory.exists());
        let mut too_small = Spool::new(0, 100, 100).unwrap();
        assert!(too_small.append("items", b"", "0", &42).is_err());
        let mut payload_limit = Spool::new(1024, 3, 100).unwrap();
        payload_limit.append("items", b"", "0", &123).unwrap();
        assert!(payload_limit.append("items", b"", "1", &4).is_err());
    }

    #[test]
    fn ordered_strings_preserve_prefix_nul_and_unicode_order() {
        let mut values = vec![
            vec![],
            vec!["".into()],
            vec!["a".into()],
            vec!["a\0".into()],
            vec!["a".into(), "".into()],
            vec!["é".into()],
        ];
        values.sort();
        let keys: Vec<_> = values.iter().map(|v| ordered_key(v)).collect();
        assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
