//! Bounded source I/O and immutable temporary snapshots shared by ingestion and delivery.
use crate::processing::Budget;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, BufRead, Read, Seek, SeekFrom, Write},
    path::Path,
};

pub(crate) const CHUNK_BYTES: usize = 64 * 1024;
const SAMPLE_BYTES: usize = 64 * 1024;

pub(crate) struct Capture {
    pub file: tempfile::NamedTempFile,
    /// Bounded profile-detection prefix, never the complete input working set.
    pub data: Vec<u8>,
    pub bytes: u64,
    pub sha256: String,
    pub complete: bool,
    pub utf8: bool,
    pub parseable_utf8: bool,
}
impl Capture {
    pub fn read(path: &Path, budget: &mut Budget) -> io::Result<Self> {
        let mut source = File::open(path)?;
        let mut file = tempfile::NamedTempFile::new()?;
        let scratch = (CHUNK_BYTES * 2 + 4) as u64;
        if !budget.reserve("capture", SAMPLE_BYTES as u64 + scratch) {
            return Err(io::Error::other("capture memory limit exceeded"));
        }
        let mut sample = Vec::with_capacity(SAMPLE_BYTES);
        let mut bytes = 0;
        let mut hash = Sha256::new();
        let mut validator = Utf8::default();
        let mut complete = false;
        let mut buffer = [0u8; CHUNK_BYTES];
        loop {
            if !budget.checkpoint("capture", 1) {
                break;
            }
            let remaining = budget.limits.input_bytes.saturating_sub(budget.input_bytes);
            if remaining == 0 {
                budget.stop("capture", "input_limit", Some("input_bytes"));
                break;
            }
            let capacity = buffer.len().min(remaining.min(usize::MAX as u64) as usize);
            let count = match source.read(&mut buffer[..capacity]) {
                Ok(count) => count,
                Err(_) => {
                    budget.stop("capture", "io_error", None);
                    break;
                }
            };
            if count == 0 {
                complete = true;
                break;
            }
            if file.write_all(&buffer[..count]).is_err() {
                file.as_file().set_len(bytes)?;
                budget.stop("capture", "io_error", None);
                break;
            }
            let sample_count = count.min(SAMPLE_BYTES - sample.len());
            sample.extend_from_slice(&buffer[..sample_count]);
            hash.update(&buffer[..count]);
            validator.push(&buffer[..count]);
            bytes += count as u64;
            budget.input_bytes += count as u64;
        }
        budget.memory_bytes = budget.memory_bytes.saturating_sub(scratch);
        file.as_file_mut().seek(SeekFrom::Start(0))?;
        Ok(Self {
            file,
            data: sample,
            bytes,
            sha256: format!("{:x}", hash.finalize()),
            complete,
            parseable_utf8: !validator.invalid && (!complete || validator.tail.is_empty()),
            utf8: !validator.invalid && validator.tail.is_empty(),
        })
    }
}

struct Utf8 {
    tail: Vec<u8>,
    invalid: bool,
}
impl Default for Utf8 {
    fn default() -> Self {
        Self {
            tail: Vec::with_capacity(CHUNK_BYTES + 4),
            invalid: false,
        }
    }
}
impl Utf8 {
    fn push(&mut self, bytes: &[u8]) {
        if self.invalid {
            return;
        }
        self.tail.extend_from_slice(bytes);
        match std::str::from_utf8(&self.tail) {
            Ok(_) => self.tail.clear(),
            Err(error) if error.error_len().is_none() => {
                self.tail.drain(..error.valid_up_to());
            }
            Err(_) => {
                self.invalid = true;
                self.tail.clear();
            }
        }
    }
}

/// A physical line is bounded before UTF-8 decoding, including newline-free inputs.
/// The one-byte overshoot lets the budgeted parser retain its existing diagnostics.
pub(crate) struct Lines<R> {
    reader: R,
    limit: usize,
    physical_eof: bool,
    budgeted: bool,
    done: bool,
}
impl<R: BufRead> Lines<R> {
    pub fn new(reader: R, limit: usize, physical_eof: bool, budgeted: bool) -> Self {
        Self {
            reader,
            limit,
            physical_eof,
            budgeted,
            done: false,
        }
    }
}
impl<R: BufRead> Iterator for Lines<R> {
    type Item = io::Result<std::borrow::Cow<'static, str>>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let mut bytes = Vec::new();
        let read = self
            .reader
            .by_ref()
            .take(self.limit.saturating_add(3) as u64)
            .read_until(b'\n', &mut bytes);
        if let Err(error) = read {
            self.done = true;
            return Some(Err(error));
        }
        if bytes.is_empty() {
            self.done = true;
            return None;
        }
        let newline = bytes.last() == Some(&b'\n');
        if newline {
            bytes.pop();
        }
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        if bytes.len() > self.limit {
            self.done = true;
            return Some(if self.budgeted {
                Ok(std::borrow::Cow::Owned(
                    String::from_utf8_lossy(&bytes).into_owned(),
                ))
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Physical record exceeds streaming record limit",
                ))
            });
        }
        if !newline && !self.physical_eof {
            self.done = true;
            return None;
        }
        Some(
            String::from_utf8(bytes)
                .map(std::borrow::Cow::Owned)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        )
    }
}

pub(crate) fn verified_snapshot(
    path: &Path,
    limit: u64,
    expected: &str,
) -> Result<(tempfile::NamedTempFile, String), Box<dyn std::error::Error>> {
    let mut source = File::open(path)?;
    if source.metadata()?.len() > limit {
        return Err("Artifact exceeds read limit".into());
    }
    let mut snapshot = tempfile::NamedTempFile::new()?;
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; CHUNK_BYTES];
    loop {
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or("Artifact size overflow")?;
        if total > limit {
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
    Ok((snapshot, hash))
}

/// Stream a UTF-8 JSON string, carrying at most one incomplete code point.
pub(crate) fn write_json_string(mut source: impl Read, writer: &mut impl Write) -> io::Result<()> {
    writer.write_all(b"\"")?;
    let mut buffer = [0u8; CHUNK_BYTES];
    let mut pending = Vec::with_capacity(CHUNK_BYTES + 4);
    loop {
        let count = source.read(&mut buffer)?;
        pending.extend_from_slice(&buffer[..count]);
        let valid = match std::str::from_utf8(&pending) {
            Ok(_) => pending.len(),
            Err(error) if error.error_len().is_none() && count > 0 => error.valid_up_to(),
            Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
        };
        let text = std::str::from_utf8(&pending[..valid]).expect("validated prefix");
        let escaped = serde_json::to_string(text)?;
        writer.write_all(&escaped.as_bytes()[1..escaped.len() - 1])?;
        pending.drain(..valid);
        if count == 0 {
            break;
        }
    }
    writer.write_all(b"\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn string_encoder_carries_split_unicode_and_escapes_controls() {
        let text = "a".repeat(CHUNK_BYTES - 1) + "🐾é\r\n\t\\\"";
        let mut encoded = Vec::new();
        write_json_string(text.as_bytes(), &mut encoded).unwrap();
        assert_eq!(serde_json::from_slice::<String>(&encoded).unwrap(), text);
        assert_eq!(encoded, serde_json::to_vec(&text).unwrap());
    }
    #[test]
    fn incomplete_capture_never_yields_a_trailing_fragment() {
        let lines: Vec<_> = Lines::new(&b"first\r\nsecond\nlast\xf0\x9f"[..], 64, false, true)
            .collect::<io::Result<_>>()
            .unwrap();
        assert_eq!(lines, ["first", "second"]);
    }
    #[test]
    fn utf8_validation_keeps_only_an_incomplete_codepoint() {
        let mut validator = Utf8::default();
        validator.push(&vec![b'a'; CHUNK_BYTES]);
        assert!(validator.tail.is_empty());
        validator.push(&[0xf0, 0x9f]);
        assert_eq!(validator.tail.len(), 2);
        validator.push(&[0x90, 0xbe]);
        assert!(!validator.invalid && validator.tail.is_empty());
        validator.push(&[0xff]);
        assert!(validator.invalid);
    }
    #[test]
    fn captures_keep_only_a_bounded_sample_and_drop_the_temporary_file() {
        let mut source = tempfile::NamedTempFile::new().unwrap();
        let bytes = vec![b'a'; CHUNK_BYTES * 3 + 7];
        source.write_all(&bytes).unwrap();
        let mut budget = crate::processing::test_budget();
        let capture = Capture::read(source.path(), &mut budget).unwrap();
        assert!(capture.complete && capture.utf8);
        assert_eq!(capture.data.len(), SAMPLE_BYTES);
        assert_eq!(capture.bytes, bytes.len() as u64);
        assert_eq!(capture.sha256, crate::evidence::digest(&bytes));
        assert_eq!(std::fs::read(capture.file.path()).unwrap(), bytes);
        let temporary = capture.file.path().to_owned();
        drop(capture);
        assert!(!temporary.exists());
    }
    #[test]
    fn delivery_checks_digest_and_size_before_exposing_a_snapshot() {
        let mut source = tempfile::NamedTempFile::new().unwrap();
        source.write_all(b"immutable evidence").unwrap();
        let hash = crate::evidence::digest(b"immutable evidence");
        assert!(verified_snapshot(source.path(), 2, &hash).is_err());
        assert!(verified_snapshot(source.path(), 100, "wrong").is_err());
        let (snapshot, _) = verified_snapshot(source.path(), 100, &hash).unwrap();
        std::fs::write(source.path(), b"changed").unwrap();
        assert_eq!(
            std::fs::read(snapshot.path()).unwrap(),
            b"immutable evidence"
        );
    }
}

#[cfg(test)]
mod consumer_tests {
    use super::*;
    use crate::{
        config::{AnalyzerConfig, LogFormat},
        normalize::NormalizationRules,
    };
    use std::collections::BTreeMap;

    #[test]
    fn discarded_records_release_state_but_preserve_full_parse_coverage() {
        let mut config = AnalyzerConfig::default();
        config.parser.format = LogFormat::JsonLines;
        for normalized in [false, true] {
            if normalized {
                config.normalization = Some(NormalizationRules {
                    root_path: "/rows".into(),
                    expand_rows: true,
                    fields: BTreeMap::from([
                        ("timestamp".into(), "/timestamp".into()),
                        ("message".into(), "/message".into()),
                    ]),
                    ..Default::default()
                });
            }
            let record =
                serde_json::json!({"timestamp":"2026-01-01T00:00:00Z","message":"excluded"});
            let line = if normalized {
                serde_json::json!({"rows":[record.clone(),record]}).to_string()
            } else {
                record.to_string()
            } + "\n";
            let data = line.repeat(200);
            let mut budget = crate::processing::test_budget();
            budget.limits.memory_bytes = 256 * 1024;
            let mut seen = 0;
            let coverage = crate::parser::visit_capture_reader(
                Path::new("synthetic.jsonl"),
                data.as_bytes(),
                true,
                &config,
                &mut budget,
                |_| {
                    seen += 1;
                    Ok(false)
                },
            )
            .unwrap();
            assert_eq!(seen, if normalized { 400 } else { 200 });
            assert_eq!(coverage.parsed_entries, seen);
            assert_eq!(coverage.rejected_candidates, 0);
            assert!(budget.stop.is_none(), "{:?}", budget.stop);
            assert_eq!(budget.memory_bytes, 0);
        }
    }
}
