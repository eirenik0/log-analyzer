//! Replayable construction rows in plain temporary files, without a resident index.
use super::Result;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    cell::{Cell, RefCell},
    fs::File,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    marker::PhantomData,
    rc::Rc,
};

#[derive(Clone)]
pub(crate) struct Disk(Rc<(u64, Cell<u64>)>);
impl Disk {
    pub fn new(limit: u64) -> Self {
        Self(Rc::new((limit, Cell::new(0))))
    }
    pub fn reserve(&self, bytes: u64) -> Result<Reservation> {
        if bytes > self.remaining() {
            return Err("construction disk byte limit exceeded".into());
        }
        self.0.1.set(self.0.1.get() + bytes);
        Ok(Reservation {
            disk: self.clone(),
            bytes,
        })
    }
    pub fn remaining(&self) -> u64 {
        self.0.0.saturating_sub(self.0.1.get())
    }
}
pub(crate) struct Reservation {
    disk: Disk,
    bytes: u64,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.disk
            .0
            .1
            .set(self.disk.0.1.get().saturating_sub(self.bytes));
    }
}
#[derive(Clone, Copy)]
struct Span {
    offset: u64,
    count: usize,
}

pub(crate) struct Rows<T> {
    file: RefCell<File>,
    offsets: RefCell<File>,
    bytes: u64,
    count: usize,
    disk: Disk,
    item_limit: u64,
    failed: bool,
    marker: PhantomData<T>,
}
impl<T: Serialize + DeserializeOwned> Rows<T> {
    pub fn new(disk: Disk, item_limit: u64) -> Result<Self> {
        Ok(Self {
            file: RefCell::new(tempfile::tempfile()?),
            offsets: RefCell::new(tempfile::tempfile()?),
            bytes: 0,
            count: 0,
            disk,
            item_limit,
            failed: false,
            marker: PhantomData,
        })
    }
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn push(&mut self, value: &T) -> Result<()> {
        if self.failed {
            return Err("construction spool previously failed".into());
        }
        let result = self.append(value);
        self.failed = result.is_err();
        result
    }
    fn append(&mut self, value: &T) -> Result<()> {
        let remaining = self
            .disk
            .remaining()
            .checked_sub(16)
            .ok_or("construction disk byte limit exceeded")?;
        struct Limited<'a> {
            file: &'a mut File,
            written: u64,
            limit: u64,
        }
        impl Write for Limited<'_> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if bytes.len() as u64 > self.limit.saturating_sub(self.written) {
                    return Err(std::io::Error::other(
                        "construction item or disk byte limit exceeded",
                    ));
                }
                self.file.write_all(bytes)?;
                self.written += bytes.len() as u64;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                self.file.flush()
            }
        }
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(self.bytes))?;
        file.write_all(&[0; 8])?;
        let mut writer = Limited {
            file: &mut file,
            written: 0,
            limit: remaining.min(self.item_limit),
        };
        {
            let mut buffered = BufWriter::with_capacity(65536, &mut writer);
            serde_json::to_writer(&mut buffered, value)?;
            buffered.flush()?;
        }
        let length = writer.written;
        file.seek(SeekFrom::Start(self.bytes))?;
        file.write_all(&length.to_le_bytes())?;
        let mut offsets = self.offsets.borrow_mut();
        offsets.seek(SeekFrom::Start(self.count as u64 * 8))?;
        offsets.write_all(&self.bytes.to_le_bytes())?;
        self.disk.0.1.set(self.disk.0.1.get() + 16 + length);
        self.bytes += 8 + length;
        self.count += 1;
        Ok(())
    }
    pub fn get(&self, ordinal: usize) -> Result<T> {
        self.range_by_ordinal(ordinal, 1)?.next().unwrap()
    }
    pub fn range_by_ordinal(
        &self,
        start: usize,
        count: usize,
    ) -> Result<impl Iterator<Item = Result<T>> + '_> {
        if start > self.count || count > self.count - start {
            return Err("construction row ordinal out of bounds".into());
        }
        let offset = if count == 0 {
            self.bytes
        } else {
            let mut offsets = self.offsets.borrow_mut();
            offsets.seek(SeekFrom::Start(start as u64 * 8))?;
            let mut bytes = [0; 8];
            offsets.read_exact(&mut bytes)?;
            u64::from_le_bytes(bytes)
        };
        self.range(Span { offset, count })
    }
    pub fn iter(&self) -> Result<impl Iterator<Item = Result<T>> + '_> {
        self.range(Span {
            offset: 0,
            count: self.count,
        })
    }
    fn range(&self, span: Span) -> Result<impl Iterator<Item = Result<T>> + '_> {
        if self.failed {
            return Err("construction spool previously failed".into());
        }
        let mut offset = span.offset;
        Ok((0..span.count).map(move |_| {
            // Explicit seeks keep nested replays independent of the file cursor.
            let mut file = self.file.borrow_mut();
            file.seek(SeekFrom::Start(offset))?;
            let mut length = [0; 8];
            file.read_exact(&mut length)?;
            let length = u64::from_le_bytes(length);
            if length > self.item_limit
                || length > self.bytes.saturating_sub(offset).saturating_sub(8)
            {
                return Err("invalid construction row extent".into());
            }
            offset += 8 + length;
            Ok(serde_json::from_reader(BufReader::with_capacity(
                65536,
                (&mut *file).take(length),
            ))?)
        }))
    }
}

impl<T> Drop for Rows<T> {
    fn drop(&mut self) {
        self.disk.0.1.set(
            self.disk
                .0
                .1
                .get()
                .saturating_sub(self.bytes + self.count as u64 * 8),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_replays_do_not_share_positions() {
        let mut rows = Rows::new(Disk::new(4096), 128).unwrap();
        rows.push(&"first é".to_owned()).unwrap();
        rows.push(&"second".to_owned()).unwrap();
        let mut iter = rows.iter().unwrap();
        assert_eq!(iter.next().unwrap().unwrap(), "first é");
        assert_eq!(
            rows.iter().unwrap().collect::<Result<Vec<_>>>().unwrap(),
            ["first é", "second"]
        );
        assert_eq!(iter.next().unwrap().unwrap(), "second");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows.get(1).unwrap(), "second");
        assert_eq!(rows.get(0).unwrap(), "first é");
        assert!(rows.get(2).is_err());
    }
    #[test]
    fn failed_append_poisons_the_spool_before_any_delivery() {
        for (disk, item) in [(10, 128), (4096, 2)] {
            let mut rows = Rows::new(Disk::new(disk), item).unwrap();
            assert!(rows.push(&"too large".to_owned()).is_err());
            assert!(rows.iter().is_err());
            assert!(rows.push(&"x".to_owned()).is_err());
        }
    }
}

use serde_json::{Value, json};

pub(crate) trait MemberSink {
    fn len(&self) -> usize;
    #[cfg(test)]
    fn append(
        &mut self,
        id: &str,
        members: &mut dyn Iterator<Item = Value>,
    ) -> Result<(usize, String)> {
        self.append_fallible(id, &mut members.map(Ok))
    }
    fn append_fallible(
        &mut self,
        id: &str,
        members: &mut dyn Iterator<Item = Result<Value>>,
    ) -> Result<(usize, String)>;
}
impl MemberSink for Vec<Value> {
    fn len(&self) -> usize {
        Vec::len(self)
    }
    fn append_fallible(
        &mut self,
        id: &str,
        members: &mut dyn Iterator<Item = Result<Value>>,
    ) -> Result<(usize, String)> {
        let members: Vec<_> = members.collect::<Result<_>>()?;
        let count = members.len();
        let hash = crate::evidence::digest(&serde_json::to_vec(&members)?);
        self.push(json!({"population_id":id,"members":members}));
        Ok((count, hash))
    }
}

pub(super) struct Memberships {
    rows: Rows<Value>,
    groups: Vec<(String, Span)>,
}
impl Memberships {
    pub fn new(disk: Disk, item_limit: u64) -> Result<Self> {
        Ok(Self {
            rows: Rows::new(disk, item_limit)?,
            groups: Vec::new(),
        })
    }
    pub fn len(&self) -> usize {
        self.groups.len()
    }
    fn write_members(
        &self,
        span: Span,
        snapshot: &Value,
        redacted: bool,
        writer: &mut dyn Write,
    ) -> Result<()> {
        writer.write_all(b"[")?;
        for (index, member) in self.rows.range(span)?.enumerate() {
            let mut member = member?;
            super::presentation::bind_snapshot(&mut member, snapshot);
            if redacted {
                let mut projection = json!({"records":[],"findings":[],"populations":[],"memberships":[{"members":[member]}],"captured_inputs":[],"report_metadata":{"evidence":{"inputs":[]}}});
                super::artifact::redact(&mut projection);
                member = projection["memberships"][0]["members"]
                    .as_array_mut()
                    .unwrap()
                    .remove(0);
            }
            if index != 0 {
                writer.write_all(b",")?;
            }
            serde_json::to_writer(&mut *writer, &member)?;
        }
        writer.write_all(b"]")?;
        Ok(())
    }
    pub fn digest(&self, index: usize, snapshot: &Value, redacted: bool) -> Result<String> {
        use sha2::{Digest, Sha256};
        struct Hash(Sha256);
        impl Write for Hash {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.update(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut hash = Hash(Sha256::new());
        self.write_members(self.groups[index].1, snapshot, redacted, &mut hash)?;
        Ok(format!("{:x}", hash.0.finalize()))
    }
    pub fn write(&self, snapshot: &Value, redacted: bool, writer: &mut dyn Write) -> Result<()> {
        writer.write_all(b"[")?;
        for (index, (id, span)) in self.groups.iter().enumerate() {
            if index != 0 {
                writer.write_all(b",")?;
            }
            writer.write_all(b"{\"members\":")?;
            self.write_members(*span, snapshot, redacted, writer)?;
            writer.write_all(b",\"population_id\":")?;
            serde_json::to_writer(&mut *writer, id)?;
            writer.write_all(b"}")?;
        }
        writer.write_all(b"]")?;
        Ok(())
    }
}
impl MemberSink for Memberships {
    fn len(&self) -> usize {
        self.groups.len()
    }
    fn append_fallible(
        &mut self,
        id: &str,
        members: &mut dyn Iterator<Item = Result<Value>>,
    ) -> Result<(usize, String)> {
        let offset = self.rows.bytes;
        let before = self.rows.count;
        for member in members {
            self.rows.push(&member?)?;
        }
        let count = self.rows.count - before;
        self.groups.push((id.to_owned(), Span { offset, count }));
        // The final digest is bound to the complete snapshot and redaction state.
        Ok((count, String::new()))
    }
}

pub(super) struct Sequences {
    rows: Rows<Value>,
    headers: Vec<(Value, Span)>,
    disk: Disk,
}
impl Sequences {
    pub fn new(disk: Disk, item_limit: u64) -> Result<Self> {
        Ok(Self {
            rows: Rows::new(disk.clone(), item_limit)?,
            headers: Vec::new(),
            disk,
        })
    }
    pub fn len(&self) -> usize {
        self.headers.len()
    }
    pub fn headers_mut(&mut self) -> impl Iterator<Item = &mut Value> {
        self.headers.iter_mut().map(|(value, _)| value)
    }
    pub fn order(&self) -> Result<OrderIndex> {
        OrderIndex::new(self.disk.clone())
    }
    pub fn append(
        &mut self,
        header: Value,
        events: impl IntoIterator<Item = Result<Value>>,
    ) -> Result<()> {
        let offset = self.rows.bytes;
        let before = self.rows.count;
        for event in events {
            self.rows.push(&event?)?;
        }
        self.headers.push((
            header,
            Span {
                offset,
                count: self.rows.count - before,
            },
        ));
        Ok(())
    }
    pub fn write(&self, snapshot: &Value, redacted: bool, writer: &mut dyn Write) -> Result<()> {
        writer.write_all(b"[")?;
        for (index, (header, span)) in self.headers.iter().enumerate() {
            if index != 0 {
                writer.write_all(b",")?;
            }
            writer.write_all(b"{")?;
            for (field, (key, value)) in header.as_object().unwrap().iter().enumerate() {
                if field != 0 {
                    writer.write_all(b",")?;
                }
                serde_json::to_writer(&mut *writer, key)?;
                writer.write_all(b":")?;
                if key != "events" {
                    serde_json::to_writer(&mut *writer, value)?;
                    continue;
                }
                writer.write_all(b"[")?;
                for (ordinal, event) in self.rows.range(*span)?.enumerate() {
                    let mut event = event?;
                    super::presentation::bind_snapshot(&mut event, snapshot);
                    if redacted {
                        let mut projection = json!({"records":[],"findings":[],"populations":[],"memberships":[],"sequences":[event],"captured_inputs":[],"report_metadata":{"evidence":{"inputs":[]}}});
                        super::artifact::redact(&mut projection);
                        event = projection["sequences"].as_array_mut().unwrap().remove(0);
                    }
                    if ordinal != 0 {
                        writer.write_all(b",")?;
                    }
                    serde_json::to_writer(&mut *writer, &event)?;
                }
                writer.write_all(b"]")?;
            }
            writer.write_all(b"}")?;
        }
        writer.write_all(b"]")?;
        Ok(())
    }
    pub fn metadata(&self) -> Vec<Value> {
        self.headers
            .iter()
            .map(|(header, _)| header.clone())
            .collect()
    }
}

pub(super) struct OrderIndex {
    file: File,
    scratch: File,
    count: u64,
    disk: Disk,
}
impl Drop for OrderIndex {
    fn drop(&mut self) {
        self.disk
            .0
            .1
            .set(self.disk.0.1.get().saturating_sub(self.count * 64));
    }
}
impl OrderIndex {
    fn new(disk: Disk) -> Result<Self> {
        Ok(Self {
            file: tempfile::tempfile()?,
            scratch: tempfile::tempfile()?,
            count: 0,
            disk,
        })
    }
    pub fn push(&mut self, key: [u8; 32]) -> Result<()> {
        if self.disk.remaining() < 64 {
            return Err("construction ordering disk limit exceeded".into());
        }
        self.file.write_all(&key)?;
        self.count += 1;
        self.disk.0.1.set(self.disk.0.1.get() + 64);
        Ok(())
    }
    pub fn sorted(mut self, mut checkpoint: impl FnMut(u64) -> Result<()>) -> Result<Ordered> {
        // The event key occupies 21 bytes: missing timestamp, signed seconds,
        // nanoseconds and selected ordinal. Remaining bytes are zero.
        for byte in (0..22).step_by(2).rev() {
            let mut counts = vec![0u64; 65536];
            self.file.seek(SeekFrom::Start(0))?;
            {
                let mut input = BufReader::with_capacity(65536, &mut self.file);
                for index in 0..self.count {
                    if index % 8192 == 0 {
                        checkpoint((self.count - index).min(8192))?;
                    }
                    let mut key = [0; 32];
                    input.read_exact(&mut key)?;
                    counts[u16::from_be_bytes([key[byte], key[byte + 1]]) as usize] += 1;
                }
            }
            let mut offset = 0;
            for count in &mut counts {
                let len = *count;
                *count = offset;
                offset += len * 32;
            }
            self.scratch.set_len(self.count * 32)?;
            self.file.seek(SeekFrom::Start(0))?;
            {
                let mut input = BufReader::with_capacity(65536, &mut self.file);
                for index in 0..self.count {
                    if index % 8192 == 0 {
                        checkpoint((self.count - index).min(8192))?;
                    }
                    let mut key = [0; 32];
                    input.read_exact(&mut key)?;
                    let bucket = u16::from_be_bytes([key[byte], key[byte + 1]]) as usize;
                    self.scratch.seek(SeekFrom::Start(counts[bucket]))?;
                    self.scratch.write_all(&key)?;
                    counts[bucket] += 32;
                }
            }
            std::mem::swap(&mut self.file, &mut self.scratch);
        }
        self.file.seek(SeekFrom::Start(0))?;
        Ok(Ordered {
            remaining: self.count,
            reader: BufReader::with_capacity(65536, self.file.try_clone()?),
            _state: self,
        })
    }
}
pub(super) struct Ordered {
    remaining: u64,
    reader: BufReader<File>,
    _state: OrderIndex,
}
impl Iterator for Ordered {
    type Item = Result<[u8; 32]>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let mut key = [0; 32];
        Some(
            self.reader
                .read_exact(&mut key)
                .map(|_| key)
                .map_err(Into::into),
        )
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    #[test]
    fn shared_disk_limit_covers_multiple_row_files() {
        let disk = Disk::new(24);
        let mut first = Rows::new(disk.clone(), 128).unwrap();
        let mut second = Rows::new(disk, 128).unwrap();
        first.push(&json!("abc")).unwrap();
        assert!(second.push(&json!("abc")).is_err());
        assert_eq!(first.iter().unwrap().next().unwrap().unwrap(), "abc");
    }
    #[test]
    fn member_spans_and_redacted_digests_match_the_value_oracle() {
        let mut members = Memberships::new(Disk::new(1024 * 1024), 65536).unwrap();
        let input = vec![
            json!({"kind":"record","occurrence":{"snapshot_id":"pending-snapshot","input_ordinal":0,"evidence_ref":{"reference_id":"r","input_id":"i","line":1,"row_path":"/é","location_redacted":false}}}),
        ];
        members
            .append("p0", &mut input.clone().into_iter())
            .unwrap();
        members.append("p1", &mut std::iter::empty()).unwrap();
        for redacted in [false, true] {
            let mut oracle = json!({"report_metadata":{"evidence":{"inputs":[]}},"records":[],"findings":[],"captured_inputs":[],"memberships":[{"population_id":"p0","members":input},{"population_id":"p1","members":[]}],"populations":[]});
            super::super::presentation::bind_snapshot(&mut oracle, &json!("snapshot"));
            if redacted {
                super::super::artifact::redact(&mut oracle);
            }
            let mut output = Vec::new();
            members
                .write(&json!("snapshot"), redacted, &mut output)
                .unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&output).unwrap(),
                oracle["memberships"]
            );
            for index in 0..2 {
                assert_eq!(
                    members.digest(index, &json!("snapshot"), redacted).unwrap(),
                    crate::evidence::digest(
                        oracle["memberships"][index]["members"]
                            .to_string()
                            .as_bytes()
                    )
                );
            }
        }
    }
    #[test]
    fn external_sequence_order_matches_lexical_order_and_releases_scratch() {
        let disk = Disk::new(1024 * 1024);
        let mut order = OrderIndex::new(disk.clone()).unwrap();
        let mut expected = Vec::new();
        for i in (0..137u64).rev() {
            let mut key = [0; 32];
            key[0] = (i % 3 == 0) as u8;
            key[1..9].copy_from_slice(&((i.wrapping_sub(60)) ^ (1 << 63)).to_be_bytes());
            key[9..13].copy_from_slice(&((i % 11) as u32).to_be_bytes());
            key[13..21].copy_from_slice(&i.to_be_bytes());
            order.push(key).unwrap();
            expected.push(key);
        }
        expected.sort();
        let mut work = 0;
        let actual = order
            .sorted(|n| {
                work += n;
                Ok(())
            })
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(work, 137 * 22);
        assert_eq!(disk.remaining(), 1024 * 1024);
    }
}
