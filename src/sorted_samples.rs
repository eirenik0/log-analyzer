//! External radix ordering for exact legacy floating-point summation order.
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub(crate) struct Samples {
    values: BufWriter<File>,
    scratch: File,
    count: u64,
    disk_bytes: u64,
    directory: tempfile::TempDir,
}
impl Samples {
    pub fn new(disk_bytes: u64) -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let open = |name| {
            File::options()
                .read(true)
                .write(true)
                .create_new(true)
                .open(directory.path().join(name))
        };
        Ok(Self {
            values: BufWriter::with_capacity(65536, open("values")?),
            scratch: open("scratch")?,
            count: 0,
            disk_bytes,
            directory,
        })
    }
    pub fn push(&mut self, value: u64) -> Result<()> {
        if self
            .count
            .checked_add(1)
            .and_then(|n| n.checked_mul(16))
            .is_none_or(|n| n > self.disk_bytes)
        {
            return Err("distribution sort disk limit exceeded".into());
        }
        self.values.write_all(&value.to_le_bytes())?;
        self.count += 1;
        Ok(())
    }
    pub fn sorted(mut self, mut checkpoint: impl FnMut() -> Result<()>) -> Result<Sorted> {
        self.values.flush()?;
        for shift in [0, 16, 32, 48] {
            let mut positions = vec![0u64; 65536];
            self.values.get_mut().seek(SeekFrom::Start(0))?;
            {
                let mut input = BufReader::with_capacity(65536, self.values.get_mut());
                for i in 0..self.count {
                    if i % 8192 == 0 {
                        checkpoint()?;
                    }
                    let mut bytes = [0; 8];
                    input.read_exact(&mut bytes)?;
                    positions[((u64::from_le_bytes(bytes) >> shift) & 65535) as usize] += 1;
                }
            }
            let mut offset = 0;
            for position in &mut positions {
                let count = *position;
                *position = offset;
                offset += count * 8;
            }
            self.scratch.set_len(self.count * 8)?;
            self.values.get_mut().seek(SeekFrom::Start(0))?;
            {
                let mut input = BufReader::with_capacity(65536, self.values.get_mut());
                for i in 0..self.count {
                    if i % 8192 == 0 {
                        checkpoint()?;
                    }
                    let mut bytes = [0; 8];
                    input.read_exact(&mut bytes)?;
                    let bucket = ((u64::from_le_bytes(bytes) >> shift) & 65535) as usize;
                    self.scratch.seek(SeekFrom::Start(positions[bucket]))?;
                    self.scratch.write_all(&bytes)?;
                    positions[bucket] += 8;
                }
            }
            std::mem::swap(self.values.get_mut(), &mut self.scratch);
        }
        let mut file = self.values.into_inner()?;
        file.seek(SeekFrom::Start(0))?;
        drop(self.scratch);
        Ok(Sorted {
            reader: BufReader::with_capacity(65536, file),
            remaining: self.count,
            _directory: self.directory,
        })
    }
}
pub(crate) struct Sorted {
    reader: BufReader<File>,
    remaining: u64,
    _directory: tempfile::TempDir,
}
impl Iterator for Sorted {
    type Item = Result<u64>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let mut bytes = [0; 8];
        match self.reader.read_exact(&mut bytes) {
            Ok(()) => {
                self.remaining -= 1;
                Some(Ok(u64::from_le_bytes(bytes)))
            }
            Err(error) => {
                self.remaining = 0;
                Some(Err(error.into()))
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_radix_order_matches_integer_sort() {
        let mut values: Vec<_> = (0..10000u64)
            .map(|i| i.wrapping_mul(6364136223846793005))
            .collect();
        values.extend([0, u64::MAX, 0, 1 << 53]);
        let mut samples = Samples::new(1024 * 1024).unwrap();
        for value in &values {
            samples.push(*value).unwrap();
        }
        values.sort_unstable();
        let sorted = samples
            .sorted(|| Ok(()))
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(values, sorted);
    }
    #[test]
    fn disk_exhaustion_is_explicit() {
        let mut samples = Samples::new(15).unwrap();
        assert!(samples.push(1).is_err());
    }
}
