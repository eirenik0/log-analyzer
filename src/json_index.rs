//! A temporary structural index over an immutable JSON snapshot. Container
//! navigation uses fixed-size records on disk; strings stay in the source file.
use serde_json::Value;
use std::{
    cell::RefCell,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    rc::Rc,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const WIDTH: u64 = 64;
const MAX_DEPTH: usize = 128;
#[derive(Clone, Copy, Debug)]
struct Header {
    start: u64,
    end: u64,
    next: u64,
    children: u64,
    key_start: u64,
    key_end: u64,
    kind: u64,
}
impl Header {
    fn bytes(self) -> [u8; 64] {
        let mut bytes = [0; 64];
        for (slot, value) in bytes.as_chunks_mut::<8>().0.iter_mut().zip([
            self.start,
            self.end,
            self.next,
            self.children,
            self.key_start,
            self.key_end,
            self.kind,
            0,
        ]) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
    fn from_bytes(bytes: [u8; 64]) -> Self {
        let mut values = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|v| u64::from_le_bytes(*v));
        Self {
            start: values.next().unwrap(),
            end: values.next().unwrap(),
            next: values.next().unwrap(),
            children: values.next().unwrap(),
            key_start: values.next().unwrap(),
            key_end: values.next().unwrap(),
            kind: values.next().unwrap(),
        }
    }
}
struct Storage {
    source: RefCell<File>,
    index: RefCell<File>,
    count: u64,
    _temporary: tempfile::TempPath,
}
/// A reference into a disk index. Clones retain no JSON subtrees.
#[derive(Clone)]
pub(crate) struct Node {
    storage: Rc<Storage>,
    ordinal: u64,
    header: Header,
}
impl Node {
    pub fn build(
        source: File,
        index_max_bytes: u64,
        deadline: Option<std::time::Instant>,
        max_nodes: u64,
    ) -> Result<Self> {
        let temporary = tempfile::NamedTempFile::new()?;
        let (mut index, path) = temporary.into_parts();
        let mut reader = BufReader::with_capacity(64 * 1024, source.try_clone()?);
        reader.seek(SeekFrom::Start(0))?;
        let mut parser = Scanner {
            source: reader,
            index: &mut index,
            offset: 0,
            count: 0,
            index_max_bytes,
            deadline,
            max_nodes,
        };
        parser.value(0, 0, 0)?;
        parser.whitespace()?;
        if parser.peek()?.is_some() {
            return Err("trailing JSON data".into());
        }
        let count = parser.count;
        let storage = Rc::new(Storage {
            source: RefCell::new(source),
            index: RefCell::new(index),
            count,
            _temporary: path,
        });
        Self::at(storage, 0)
    }
    fn at(storage: Rc<Storage>, ordinal: u64) -> Result<Self> {
        if ordinal >= storage.count {
            return Err("JSON index address outside document".into());
        }
        let mut bytes = [0; 64];
        {
            let mut file = storage.index.borrow_mut();
            file.seek(SeekFrom::Start(ordinal * WIDTH))?;
            file.read_exact(&mut bytes)?;
        }
        Ok(Self {
            storage,
            ordinal,
            header: Header::from_bytes(bytes),
        })
    }
    pub fn is_string(&self) -> bool {
        self.header.kind == u64::from(b'"')
    }
    pub fn is_array(&self) -> bool {
        self.header.kind == b'[' as u64
    }
    pub fn is_object(&self) -> bool {
        self.header.kind == b'{' as u64
    }
    pub fn len(&self) -> u64 {
        self.header.children
    }
    pub fn byte_len(&self) -> u64 {
        self.header.end - self.header.start
    }
    pub fn read(&self, max_bytes: u64) -> Result<Value> {
        if self.byte_len() > max_bytes {
            return Err("JSON item exceeds materialization byte limit".into());
        }
        let mut file = self.storage.source.borrow_mut();
        file.seek(SeekFrom::Start(self.header.start))?;
        Ok(serde_json::from_reader(BufReader::new(
            (&mut *file).take(self.byte_len()),
        ))?)
    }
    pub fn key(&self) -> Result<Option<String>> {
        if self.header.key_end == self.header.key_start {
            return Ok(None);
        }
        let mut file = self.storage.source.borrow_mut();
        file.seek(SeekFrom::Start(self.header.key_start))?;
        Ok(Some(serde_json::from_reader(
            (&mut *file).take(self.header.key_end - self.header.key_start),
        )?))
    }
    pub fn node_count(&self) -> u64 {
        self.header.next - self.ordinal
    }
    pub fn ordinal(&self) -> u64 {
        self.ordinal
    }
    pub fn at_ordinal(&self, ordinal: u64) -> Result<Self> {
        Self::at(self.storage.clone(), ordinal)
    }
    pub fn read_without(&self, omitted: &[&str], max_bytes: u64) -> Result<Value> {
        if !self.is_object() {
            return Err("expected JSON object".into());
        }
        let mut object = serde_json::Map::new();
        let mut remaining = max_bytes;
        for child in self.children() {
            let child = child?;
            let key = child.key()?.ok_or("object property has no key")?;
            if omitted.contains(&key.as_str()) {
                continue;
            }
            remaining = remaining
                .checked_sub(key.len() as u64 + 64)
                .ok_or("JSON object exceeds materialization byte limit")?;
            let value = child.read(remaining)?;
            remaining = remaining
                .checked_sub(child.byte_len())
                .ok_or("JSON object exceeds materialization byte limit")?;
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
    /// Decode a JSON string into a buffered sink without allocating its payload.
    pub fn copy_string(
        &self,
        output: &mut impl Write,
        mut checkpoint: impl FnMut() -> Result<()>,
    ) -> Result<u64> {
        if self.header.kind != u64::from(b'"') {
            return Err("expected JSON string".into());
        }
        let mut file = self.storage.source.borrow_mut();
        file.seek(SeekFrom::Start(self.header.start + 1))?;
        let mut input = BufReader::with_capacity(64 * 1024, (&mut *file).take(self.byte_len() - 2));
        let mut output = std::io::BufWriter::with_capacity(64 * 1024, output);
        let mut count = 0;
        fn next(input: &mut impl Read) -> Result<u8> {
            let mut b = [0];
            input.read_exact(&mut b)?;
            Ok(b[0])
        }
        loop {
            checkpoint()?;
            let buffer = input.fill_buf()?;
            if buffer.is_empty() {
                break;
            }
            let plain = buffer
                .iter()
                .position(|b| *b == b'\\')
                .unwrap_or(buffer.len());
            if plain > 0 {
                output.write_all(&buffer[..plain])?;
                count += plain as u64;
                input.consume(plain);
                continue;
            }
            input.consume(1);
            match next(&mut input)? {
                b'u' => {
                    let mut spelling = vec![b'"', b'\\', b'u'];
                    let mut hex = [0; 4];
                    input.read_exact(&mut hex)?;
                    spelling.extend(hex);
                    let first = u16::from_str_radix(std::str::from_utf8(&hex)?, 16)?;
                    if (0xd800..=0xdbff).contains(&first) {
                        let mut second = [0; 6];
                        input.read_exact(&mut second)?;
                        spelling.extend(second);
                    }
                    spelling.push(b'"');
                    let decoded: String = serde_json::from_slice(&spelling)?;
                    output.write_all(decoded.as_bytes())?;
                    count += decoded.len() as u64;
                }
                escape => {
                    let byte = match escape {
                        b'"' => b'"',
                        b'\\' => b'\\',
                        b'/' => b'/',
                        b'b' => 8,
                        b'f' => 12,
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        _ => return Err("invalid JSON escape".into()),
                    };
                    output.write_all(&[byte])?;
                    count += 1;
                }
            }
        }
        output.flush()?;
        Ok(count)
    }
    pub fn children(&self) -> Children {
        Children {
            storage: self.storage.clone(),
            next: self.ordinal + 1,
            remaining: self.header.children,
        }
    }
    pub fn field(&self, key: &str) -> Result<Option<Self>> {
        if !self.is_object() {
            return Ok(None);
        }
        let mut found = None;
        for child in self.children() {
            let child = child?;
            if child.key()?.as_deref() == Some(key) {
                found = Some(child);
            }
        }
        Ok(found)
    }
    pub fn pointer(&self, pointer: &str) -> Result<Option<Self>> {
        if pointer.is_empty() {
            return Ok(Some(self.clone()));
        }
        if !pointer.starts_with('/') {
            return Ok(None);
        }
        let mut current = self.clone();
        for token in pointer[1..].split('/') {
            // RFC 6901 decoding; malformed escapes are not alternate paths.
            let mut decoded = String::new();
            let mut chars = token.chars();
            while let Some(c) = chars.next() {
                if c == '~' {
                    match chars.next() {
                        Some('0') => decoded.push('~'),
                        Some('1') => decoded.push('/'),
                        _ => return Ok(None),
                    }
                } else {
                    decoded.push(c);
                }
            }
            let next = if current.is_object() {
                current.field(&decoded)?
            } else if current.is_array() {
                if decoded.len() > 1 && decoded.starts_with('0') {
                    return Ok(None);
                }
                let Ok(ordinal) = decoded.parse::<u64>() else {
                    return Ok(None);
                };
                if ordinal >= current.len() {
                    return Ok(None);
                }
                let mut result = None;
                for (i, child) in current.children().enumerate() {
                    let child = child?;
                    if i as u64 == ordinal {
                        result = Some(child);
                        break;
                    }
                }
                result
            } else {
                None
            };
            let Some(next) = next else { return Ok(None) };
            current = next;
        }
        Ok(Some(current))
    }
}
pub(crate) struct Children {
    storage: Rc<Storage>,
    next: u64,
    remaining: u64,
}
impl Iterator for Children {
    type Item = Result<Node>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        match Node::at(self.storage.clone(), self.next) {
            Ok(node) => {
                self.next = node.header.next;
                self.remaining -= 1;
                Some(Ok(node))
            }
            Err(error) => {
                self.remaining = 0;
                Some(Err(error))
            }
        }
    }
}
struct Scanner<'a> {
    source: BufReader<File>,
    index: &'a mut File,
    offset: u64,
    count: u64,
    index_max_bytes: u64,
    deadline: Option<std::time::Instant>,
    max_nodes: u64,
}
impl Scanner<'_> {
    fn peek(&mut self) -> Result<Option<u8>> {
        Ok(self.source.fill_buf()?.first().copied())
    }
    fn byte(&mut self) -> Result<u8> {
        if self.offset.is_multiple_of(65536)
            && self
                .deadline
                .is_some_and(|deadline| std::time::Instant::now() >= deadline)
        {
            return Err("artifact validation time limit exceeded".into());
        }
        let b = self.peek()?.ok_or("unexpected end of JSON")?;
        self.source.consume(1);
        self.offset += 1;
        Ok(b)
    }
    fn whitespace(&mut self) -> Result<()> {
        while matches!(self.peek()?, Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.byte()?;
        }
        Ok(())
    }
    fn expect(&mut self, b: u8) -> Result<()> {
        if self.byte()? != b {
            return Err("invalid JSON token".into());
        }
        Ok(())
    }
    fn string(&mut self) -> Result<()> {
        self.expect(b'"')?;
        loop {
            match self.byte()? {
                b'"' => return Ok(()),
                b'\\' => match self.byte()? {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                    b'u' => {
                        let first = self.hex4()?;
                        if (0xd800..=0xdbff).contains(&first) {
                            self.expect(b'\\')?;
                            self.expect(b'u')?;
                            if !(0xdc00..=0xdfff).contains(&self.hex4()?) {
                                return Err("invalid JSON surrogate pair".into());
                            }
                        } else if (0xdc00..=0xdfff).contains(&first) {
                            return Err("lone JSON surrogate".into());
                        }
                    }
                    _ => return Err("invalid JSON escape".into()),
                },
                0..=31 => return Err("unescaped control character in JSON string".into()),
                first @ 128..=255 => {
                    let length = match first {
                        0xc2..=0xdf => 2,
                        0xe0..=0xef => 3,
                        0xf0..=0xf4 => 4,
                        _ => return Err("invalid UTF-8 in JSON string".into()),
                    };
                    let mut bytes = [first, 0, 0, 0];
                    for byte in &mut bytes[1..length] {
                        *byte = self.byte()?;
                    }
                    std::str::from_utf8(&bytes[..length])?;
                }
                _ => {}
            }
        }
    }
    fn hex4(&mut self) -> Result<u16> {
        let mut value = 0;
        for _ in 0..4 {
            let b = self.byte()?;
            value = value * 16
                + match b {
                    b'0'..=b'9' => u16::from(b - b'0'),
                    b'a'..=b'f' => u16::from(b - b'a' + 10),
                    b'A'..=b'F' => u16::from(b - b'A' + 10),
                    _ => return Err("invalid Unicode escape".into()),
                };
        }
        Ok(value)
    }
    fn value(&mut self, depth: usize, key_start: u64, key_end: u64) -> Result<()> {
        if depth >= MAX_DEPTH {
            return Err("JSON nesting limit exceeded".into());
        }
        if self.count >= self.max_nodes {
            return Err("artifact validation work limit exceeded".into());
        }
        if self.count >= self.index_max_bytes / WIDTH {
            return Err("JSON index disk limit exceeded".into());
        }
        self.whitespace()?;
        let ordinal = self.count;
        self.count += 1;
        self.index.seek(SeekFrom::Start(ordinal * WIDTH))?;
        self.index.write_all(&[0; 64])?;
        let mut header = Header {
            start: self.offset,
            end: 0,
            next: 0,
            children: 0,
            key_start,
            key_end,
            kind: u64::from(self.peek()?.ok_or("missing JSON value")?),
        };
        match header.kind as u8 {
            b'[' | b'{' => {
                let object = self.byte()? == b'{';
                let close = if object { b'}' } else { b']' };
                self.whitespace()?;
                if self.peek()? == Some(close) {
                    self.byte()?;
                } else {
                    loop {
                        let (key_start, key_end) = if object {
                            self.whitespace()?;
                            let start = self.offset;
                            self.string()?;
                            if self.offset - start > 65536 {
                                return Err("JSON object key exceeds 64 KiB".into());
                            }
                            let end = self.offset;
                            self.whitespace()?;
                            self.expect(b':')?;
                            (start, end)
                        } else {
                            (0, 0)
                        };
                        self.value(depth + 1, key_start, key_end)?;
                        header.children += 1;
                        self.whitespace()?;
                        let delimiter = self.byte()?;
                        if delimiter == close {
                            break;
                        }
                        if delimiter != b',' {
                            return Err("invalid JSON container delimiter".into());
                        }
                    }
                }
            }
            b'"' => self.string()?,
            b't' => {
                for b in b"true" {
                    self.expect(*b)?;
                }
            }
            b'f' => {
                for b in b"false" {
                    self.expect(*b)?;
                }
            }
            b'n' => {
                for b in b"null" {
                    self.expect(*b)?;
                }
            }
            b'-' | b'0'..=b'9' => {
                let mut number = Vec::new();
                while matches!(
                    self.peek()?,
                    Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                ) {
                    if number.len() == 1024 {
                        return Err("JSON number exceeds 1024 bytes".into());
                    }
                    number.push(self.byte()?);
                }
                let value: Value = serde_json::from_slice(&number)?;
                if !value.is_number() {
                    return Err("invalid JSON number".into());
                }
            }
            _ => return Err("invalid JSON value".into()),
        }
        header.end = self.offset;
        header.next = self.count;
        self.index.seek(SeekFrom::Start(ordinal * WIDTH))?;
        self.index.write_all(&header.bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn index(bytes: &[u8]) -> Result<Node> {
        let mut source = tempfile::tempfile()?;
        source.write_all(bytes)?;
        Node::build(source, 1024 * 1024, None, u64::MAX)
    }
    #[test]
    fn pointers_and_iteration_match_json_without_materializing_containers() {
        let bytes=br#"{"a":[{"id":"first","value":"\uD83D\uDE00"},{"id":"last","value":-1.25e3}],"a/b":{"~key":true}}"#;
        let root = index(bytes).unwrap();
        let array = root.pointer("/a").unwrap().unwrap();
        assert!(array.is_array());
        assert_eq!(array.len(), 2);
        assert!(array.read(10).is_err());
        let value: Value = serde_json::from_slice(bytes).unwrap();
        for path in ["/a/0/id", "/a/1/value", "/a~1b/~0key"] {
            assert_eq!(
                root.pointer(path).unwrap().unwrap().read(1024).unwrap(),
                *value.pointer(path).unwrap()
            );
        }
        assert!(root.pointer("/a/01").unwrap().is_none());
        assert!(root.pointer("/a~2b").unwrap().is_none());
        assert_eq!(
            array
                .children()
                .map(|c| c.unwrap().read(1024).unwrap())
                .collect::<Vec<_>>(),
            *value["a"].as_array().unwrap()
        );
    }
    #[test]
    fn malformed_json_is_rejected_even_in_unselected_branches() {
        for bytes in [
            br#"[1,]"#.as_slice(),
            br#"{"x":true,}"#,
            br#"{"x":"\uD800"}"#,
            br#"{"x":"\uDC00"}"#,
            br#"[01]"#,
            br#"[1] null"#,
            br#"[1e9999]"#,
            b"[\"\xff\"]",
            b"[\"\xc0\xaf\"]",
        ] {
            assert!(index(bytes).is_err(), "accepted {bytes:?}");
        }
    }

    #[test]
    fn captured_strings_decode_across_buffers_and_obey_cancellation() {
        let text = format!("{}é\n\"\\😀", "x".repeat(65535));
        let bytes = serde_json::to_vec(&text).unwrap();
        let root = index(&bytes).unwrap();
        let mut decoded = Vec::new();
        root.copy_string(&mut decoded, || Ok(())).unwrap();
        assert_eq!(decoded, text.as_bytes());
        let root = index(br#""\uD83D\uDE00\u00e9""#).unwrap();
        decoded.clear();
        root.copy_string(&mut decoded, || Ok(())).unwrap();
        assert_eq!(decoded, "😀é".as_bytes());
        assert!(
            root.copy_string(&mut std::io::sink(), || Err("cancelled".into()))
                .is_err()
        );
        let root = index(br#"{"x":1,"x":2}"#).unwrap();
        assert_eq!(root.pointer("/x").unwrap().unwrap().read(16).unwrap(), 2);
    }
    #[test]
    fn index_disk_cap_is_explicit() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"[1,2,3]").unwrap();
        assert!(Node::build(file, 3 * WIDTH, None, u64::MAX).is_err());
    }
}
