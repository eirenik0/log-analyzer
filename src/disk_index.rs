//! Fixed-size on-disk hash slots for temporary artifact reference indexes.
//! Keys are compared exactly after hashing; neither keys nor slots are mmap'ed.
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const SLOT: u64 = 64;
pub(crate) struct DiskIndex {
    slots: RefCell<File>,
    keys: RefCell<File>,
    capacity: u64,
    entries: u64,
    max_entries: u64,
    key_bytes: u64,
    disk_bytes: u64,
    _directory: tempfile::TempDir,
}
impl DiskIndex {
    pub fn new(max_entries: u64, disk_bytes: u64) -> Result<Self> {
        let capacity = max_entries
            .max(1)
            .checked_mul(2)
            .and_then(u64::checked_next_power_of_two)
            .ok_or("disk index capacity overflow")?;
        let slot_bytes = capacity
            .checked_mul(SLOT)
            .ok_or("disk index byte overflow")?;
        if slot_bytes > disk_bytes {
            return Err("disk index byte limit exceeded".into());
        }
        let directory = tempfile::tempdir()?;
        let slots = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(directory.path().join("slots"))?;
        slots.set_len(slot_bytes)?;
        let keys = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(directory.path().join("keys"))?;
        Ok(Self {
            slots: RefCell::new(slots),
            keys: RefCell::new(keys),
            capacity,
            entries: 0,
            max_entries,
            key_bytes: 0,
            disk_bytes,
            _directory: directory,
        })
    }
    fn locate(&self, key: &[u8]) -> Result<(u64, Option<u64>, [u8; 32])> {
        if key.len() > 65536 {
            return Err("disk index key byte limit exceeded".into());
        }
        let hash: [u8; 32] = Sha256::digest(key).into();
        let mut slot = u64::from_le_bytes(hash[..8].try_into().unwrap()) & (self.capacity - 1);
        let mut slots = self.slots.borrow_mut();
        let mut keys = self.keys.borrow_mut();
        for _ in 0..self.capacity {
            let mut bytes = [0; 64];
            slots.seek(SeekFrom::Start(slot * SLOT))?;
            slots.read_exact(&mut bytes)?;
            if bytes[56] == 0 {
                return Ok((slot, None, hash));
            }
            if bytes[..32] == hash {
                let offset = u64::from_le_bytes(bytes[32..40].try_into().unwrap());
                let len = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
                if len == key.len() as u64 {
                    if offset
                        .checked_add(len)
                        .is_none_or(|end| end > self.key_bytes)
                    {
                        return Err("invalid disk index key extent".into());
                    }
                    let mut saved = vec![0; key.len()];
                    keys.seek(SeekFrom::Start(offset))?;
                    keys.read_exact(&mut saved)?;
                    if saved == key {
                        return Ok((
                            slot,
                            Some(u64::from_le_bytes(bytes[48..56].try_into().unwrap())),
                            hash,
                        ));
                    }
                }
            }
            slot = (slot + 1) & (self.capacity - 1);
        }
        Err("disk index has no free slot".into())
    }
    pub fn get(&self, key: &[u8]) -> Result<Option<u64>> {
        Ok(self.locate(key)?.1)
    }
    /// Insert without replacement; a duplicate returns the retained value.
    pub fn insert(&mut self, key: &[u8], value: u64) -> Result<Option<u64>> {
        let (slot, previous, hash) = self.locate(key)?;
        if previous.is_some() {
            return Ok(previous);
        }
        if self.entries >= self.max_entries {
            return Err("disk index entry limit exceeded".into());
        }
        let end = self
            .key_bytes
            .checked_add(key.len() as u64)
            .ok_or("disk index byte overflow")?;
        if end > self.disk_bytes - self.capacity * SLOT {
            return Err("disk index byte limit exceeded".into());
        }
        {
            let mut keys = self.keys.borrow_mut();
            keys.seek(SeekFrom::Start(self.key_bytes))?;
            keys.write_all(key)?;
        }
        let mut bytes = [0; 64];
        bytes[..32].copy_from_slice(&hash);
        bytes[32..40].copy_from_slice(&self.key_bytes.to_le_bytes());
        bytes[40..48].copy_from_slice(&(key.len() as u64).to_le_bytes());
        bytes[48..56].copy_from_slice(&value.to_le_bytes());
        bytes[56] = 1;
        {
            let mut slots = self.slots.borrow_mut();
            slots.seek(SeekFrom::Start(slot * SLOT))?;
            slots.write_all(&bytes)?;
        }
        self.key_bytes = end;
        self.entries += 1;
        Ok(None)
    }
    pub fn replace(&mut self, key: &[u8], value: u64) -> Result<Option<u64>> {
        let (slot, previous, _) = self.locate(key)?;
        if previous.is_none() {
            return self.insert(key, value);
        }
        let mut slots = self.slots.borrow_mut();
        slots.seek(SeekFrom::Start(slot * SLOT + 48))?;
        slots.write_all(&value.to_le_bytes())?;
        Ok(previous)
    }
    pub fn len(&self) -> u64 {
        self.entries
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn references_and_duplicate_detection_survive_slot_collisions() {
        let mut index = DiskIndex::new(1000, 1024 * 1024).unwrap();
        for i in 0..1000 {
            assert_eq!(
                index.insert(format!("key-{i}").as_bytes(), i).unwrap(),
                None
            );
        }
        assert_eq!(index.len(), 1000);
        for i in 0..1000 {
            assert_eq!(index.get(format!("key-{i}").as_bytes()).unwrap(), Some(i));
            assert_eq!(
                index.insert(format!("key-{i}").as_bytes(), 9999).unwrap(),
                Some(i)
            );
        }
        assert_eq!(index.get(b"absent").unwrap(), None);
        assert!(index.insert(b"overflow", 1).is_err());
    }
    #[test]
    fn byte_caps_and_cleanup_are_explicit() {
        assert!(DiskIndex::new(1000, 100).is_err());
        let mut index = DiskIndex::new(1, 128).unwrap();
        let path = index._directory.path().to_owned();
        assert!(index.insert(b"not-enough-space", 0).is_err());
        drop(index);
        assert!(!path.exists());
    }
}
