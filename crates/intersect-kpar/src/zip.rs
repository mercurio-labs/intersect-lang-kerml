//! Handwritten (not generated) bounded ZIP reader and deterministic ZIP
//! writer: the container format of a project archive.
//!
//! The reader trusts the central directory, accepts only stored and
//! deflated entries, and refuses ZIP64, multi-disk and encrypted archives,
//! unsafe or duplicate paths, and archives over the limits before inflating
//! anything. The writer sorts entries, uses fixed timestamps and no extra
//! fields, so equal entries give equal bytes (with the same `miniz_oxide`).
use std::collections::BTreeMap;
use std::fmt;

/// Upper bounds a reader enforces before and while extracting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveLimits {
    /// Entries in the central directory, directories included.
    pub max_entries: usize,
    /// Uncompressed bytes of all entries together.
    pub max_total_bytes: u64,
    /// Uncompressed bytes of one entry.
    pub max_entry_bytes: u64,
    /// Uncompressed-to-compressed ratio of an entry over 1 MiB.
    pub max_ratio: u64,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_total_bytes: 512 * 1024 * 1024,
            max_entry_bytes: 64 * 1024 * 1024,
            max_ratio: 200,
        }
    }
}

/// One file of an archive: its path inside the archive (forward slashes,
/// relative) and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZipEntry {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// Why an archive was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZipError {
    /// Not a ZIP archive, or its structure is damaged.
    Corrupt(String),
    /// A ZIP feature this reader does not support.
    Unsupported(String),
    /// Over one of the [`ArchiveLimits`].
    Limit(String),
    /// An entry path that is unsafe or ambiguous.
    Path { path: String, reason: String },
}

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupt(message) => write!(f, "damaged ZIP archive: {message}"),
            Self::Unsupported(message) => write!(f, "unsupported ZIP archive: {message}"),
            Self::Limit(message) => write!(f, "archive over limit: {message}"),
            Self::Path { path, reason } => write!(f, "entry '{path}': {reason}"),
        }
    }
}

impl std::error::Error for ZipError {}

const LOCAL_HEADER: u32 = 0x0403_4b50;
const CENTRAL_HEADER: u32 = 0x0201_4b50;
const END_OF_DIRECTORY: u32 = 0x0605_4b50;
const ZIP64_LOCATOR: u32 = 0x0706_4b50;
const END_LEN: usize = 22;
const CENTRAL_LEN: usize = 46;
const LOCAL_LEN: usize = 30;
const STORED: u16 = 0;
const DEFLATED: u16 = 8;
const UTF8_NAMES: u16 = 0x0800;
const RATIO_FLOOR: u64 = 1024 * 1024;
/// 1980-01-01 in MS-DOS date format: the earliest ZIP date.
const DOS_EPOCH_DATE: u16 = 0x0021;

fn corrupt(what: &str) -> ZipError {
    ZipError::Corrupt(what.to_owned())
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, ZipError> {
    bytes
        .get(at..at.saturating_add(2))
        .and_then(|slice| <[u8; 2]>::try_from(slice).ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| corrupt("truncated header"))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, ZipError> {
    bytes
        .get(at..at.saturating_add(4))
        .and_then(|slice| <[u8; 4]>::try_from(slice).ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| corrupt("truncated header"))
}

fn slice(bytes: &[u8], start: usize, len: usize) -> Result<&[u8], ZipError> {
    start
        .checked_add(len)
        .and_then(|end| bytes.get(start..end))
        .ok_or_else(|| corrupt("an entry or header runs past the end of the archive"))
}

/// Whether `path` is a safe relative entry path: non-empty, forward
/// slashes, no absolute or drive prefix, no empty, `.` or `..` segment, no
/// control character.
pub fn check_path(path: &str) -> Result<(), ZipError> {
    let refuse = |reason: &str| {
        Err(ZipError::Path {
            path: path.to_owned(),
            reason: reason.to_owned(),
        })
    };
    if path.is_empty() {
        return refuse("empty path");
    }
    if path.starts_with('/') {
        return refuse("absolute path");
    }
    if path.contains('\\') {
        return refuse("backslash in path");
    }
    if path.contains(':') {
        return refuse("':' in path (drive or scheme prefix)");
    }
    if path.chars().any(char::is_control) {
        return refuse("control character in path");
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return refuse("empty, '.' or '..' path segment");
    }
    Ok(())
}

/// The end-of-central-directory record's offset.
fn end_of_directory(bytes: &[u8]) -> Result<usize, ZipError> {
    if bytes.len() < END_LEN {
        return Err(corrupt("too short to be a ZIP archive"));
    }
    let last = bytes.len() - END_LEN;
    let first = last.saturating_sub(usize::from(u16::MAX));
    for at in (first..=last).rev() {
        if u32_at(bytes, at)? == END_OF_DIRECTORY
            && at + END_LEN + usize::from(u16_at(bytes, at + 20)?) == bytes.len()
        {
            return Ok(at);
        }
    }
    Err(corrupt(
        "no end-of-central-directory record (not a ZIP archive)",
    ))
}

/// Read every file entry of `bytes`, sorted by path. Directory entries are
/// checked and dropped.
pub fn read(bytes: &[u8], limits: &ArchiveLimits) -> Result<Vec<ZipEntry>, ZipError> {
    let end = end_of_directory(bytes)?;
    if end >= 20 && u32_at(bytes, end - 20)? == ZIP64_LOCATOR {
        return Err(ZipError::Unsupported("ZIP64".into()));
    }
    let disk = u16_at(bytes, end + 4)?;
    let directory_disk = u16_at(bytes, end + 6)?;
    let on_disk = u16_at(bytes, end + 8)?;
    let count = u16_at(bytes, end + 10)?;
    let size = u32_at(bytes, end + 12)?;
    let offset = u32_at(bytes, end + 16)?;
    if count == u16::MAX || size == u32::MAX || offset == u32::MAX {
        return Err(ZipError::Unsupported("ZIP64".into()));
    }
    if disk != 0 || directory_disk != 0 || on_disk != count {
        return Err(ZipError::Unsupported("multi-disk archive".into()));
    }
    let count = usize::from(count);
    if count > limits.max_entries {
        return Err(ZipError::Limit(format!(
            "{count} entries (limit {})",
            limits.max_entries
        )));
    }
    let directory = slice(bytes, offset as usize, size as usize)?;
    if offset as usize + size as usize > end {
        return Err(corrupt("central directory overlaps its end record"));
    }

    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut total: u64 = 0;
    let mut entries = Vec::new();
    let mut at = 0usize;
    for _ in 0..count {
        if u32_at(directory, at)? != CENTRAL_HEADER {
            return Err(corrupt("bad central directory header"));
        }
        let flags = u16_at(directory, at + 8)?;
        let method = u16_at(directory, at + 10)?;
        let crc = u32_at(directory, at + 16)?;
        let compressed = u64::from(u32_at(directory, at + 20)?);
        let uncompressed = u64::from(u32_at(directory, at + 24)?);
        let name_len = usize::from(u16_at(directory, at + 28)?);
        let extra_len = usize::from(u16_at(directory, at + 30)?);
        let comment_len = usize::from(u16_at(directory, at + 32)?);
        let local = u32_at(directory, at + 42)? as usize;
        let name = slice(directory, at + CENTRAL_LEN, name_len)?;
        at += CENTRAL_LEN + name_len + extra_len + comment_len;

        let path = std::str::from_utf8(name)
            .map_err(|_| ZipError::Path {
                path: String::from_utf8_lossy(name).into_owned(),
                reason: "entry name is not UTF-8".into(),
            })?
            .to_owned();
        if flags & 0x0041 != 0 {
            return Err(ZipError::Unsupported(format!("encrypted entry '{path}'")));
        }
        let (path, directory_entry) = match path.strip_suffix('/') {
            Some(dir) => (dir.to_owned(), true),
            None => (path, false),
        };
        check_path(&path)?;
        let folded = path.to_lowercase();
        if let Some(previous) = seen.insert(folded, path.clone()) {
            return Err(ZipError::Path {
                path,
                reason: format!("duplicates entry '{previous}' (paths compare case-insensitively)"),
            });
        }
        if directory_entry {
            if uncompressed != 0 {
                return Err(corrupt("directory entry with content"));
            }
            continue;
        }
        if uncompressed > limits.max_entry_bytes {
            return Err(ZipError::Limit(format!(
                "entry '{path}' is {uncompressed} bytes (limit {})",
                limits.max_entry_bytes
            )));
        }
        total = total.saturating_add(uncompressed);
        if total > limits.max_total_bytes {
            return Err(ZipError::Limit(format!(
                "more than {} uncompressed bytes",
                limits.max_total_bytes
            )));
        }
        if uncompressed > RATIO_FLOOR && uncompressed > compressed.saturating_mul(limits.max_ratio)
        {
            return Err(ZipError::Limit(format!(
                "entry '{path}' compresses more than {}:1",
                limits.max_ratio
            )));
        }

        if u32_at(bytes, local)? != LOCAL_HEADER {
            return Err(corrupt("bad local header"));
        }
        let local_name = usize::from(u16_at(bytes, local + 26)?);
        let local_extra = usize::from(u16_at(bytes, local + 28)?);
        let data = slice(
            bytes,
            local + LOCAL_LEN + local_name + local_extra,
            compressed as usize,
        )?;
        let content = match method {
            STORED => data.to_vec(),
            DEFLATED => {
                miniz_oxide::inflate::decompress_to_vec_with_limit(data, uncompressed as usize)
                    .map_err(|error| {
                        ZipError::Corrupt(format!("entry '{path}' does not inflate: {error}"))
                    })?
            }
            other => {
                return Err(ZipError::Unsupported(format!(
                    "entry '{path}' uses compression method {other} (only stored and deflate)"
                )));
            }
        };
        if content.len() as u64 != uncompressed {
            return Err(ZipError::Corrupt(format!(
                "entry '{path}' is {} bytes, its header says {uncompressed}",
                content.len()
            )));
        }
        if crc32fast::hash(&content) != crc {
            return Err(ZipError::Corrupt(format!(
                "entry '{path}' fails its CRC-32"
            )));
        }
        entries.push(ZipEntry {
            path,
            bytes: content,
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn to_u32(value: usize, what: &str) -> Result<u32, ZipError> {
    u32::try_from(value).map_err(|_| ZipError::Limit(format!("{what} needs ZIP64")))
}

/// Write `entries` as a ZIP archive: sorted by path, deflated when that is
/// smaller, every timestamp 1980-01-01 00:00, no extra fields or comments.
pub fn write(entries: &[ZipEntry]) -> Result<Vec<u8>, ZipError> {
    let mut sorted: Vec<&ZipEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut seen = BTreeMap::new();
    for entry in &sorted {
        check_path(&entry.path)?;
        if let Some(previous) = seen.insert(entry.path.to_lowercase(), &entry.path) {
            return Err(ZipError::Path {
                path: entry.path.clone(),
                reason: format!("duplicates entry '{previous}' (paths compare case-insensitively)"),
            });
        }
    }
    let count = u16::try_from(sorted.len())
        .ok()
        .filter(|count| *count != u16::MAX)
        .ok_or_else(|| ZipError::Limit("too many entries (needs ZIP64)".into()))?;

    let mut out = Vec::new();
    let mut directory = Vec::new();
    for entry in sorted {
        let deflated = miniz_oxide::deflate::compress_to_vec(&entry.bytes, 6);
        let (method, data) = if deflated.len() < entry.bytes.len() {
            (DEFLATED, deflated.as_slice())
        } else {
            (STORED, entry.bytes.as_slice())
        };
        let flags = if entry.path.is_ascii() { 0 } else { UTF8_NAMES };
        let crc = crc32fast::hash(&entry.bytes);
        let name = entry.path.as_bytes();
        let name_len = u16::try_from(name.len())
            .map_err(|_| ZipError::Limit(format!("entry name '{}' too long", entry.path)))?;
        let compressed = to_u32(data.len(), "an entry")?;
        let uncompressed = to_u32(entry.bytes.len(), "an entry")?;
        let offset = to_u32(out.len(), "the archive")?;

        push_u32(&mut out, LOCAL_HEADER);
        for value in [20, flags, method, 0, DOS_EPOCH_DATE] {
            push_u16(&mut out, value);
        }
        for value in [crc, compressed, uncompressed] {
            push_u32(&mut out, value);
        }
        push_u16(&mut out, name_len);
        push_u16(&mut out, 0);
        out.extend_from_slice(name);
        out.extend_from_slice(data);

        push_u32(&mut directory, CENTRAL_HEADER);
        for value in [20, 20, flags, method, 0, DOS_EPOCH_DATE] {
            push_u16(&mut directory, value);
        }
        for value in [crc, compressed, uncompressed] {
            push_u32(&mut directory, value);
        }
        for value in [name_len, 0, 0, 0, 0] {
            push_u16(&mut directory, value);
        }
        push_u32(&mut directory, 0);
        push_u32(&mut directory, offset);
        directory.extend_from_slice(name);
    }
    let directory_offset = to_u32(out.len(), "the archive")?;
    let directory_size = to_u32(directory.len(), "the central directory")?;
    out.extend_from_slice(&directory);
    push_u32(&mut out, END_OF_DIRECTORY);
    for value in [0, 0, count, count] {
        push_u16(&mut out, value);
    }
    push_u32(&mut out, directory_size);
    push_u32(&mut out, directory_offset);
    push_u16(&mut out, 0);
    Ok(out)
}

#[cfg(test)]
#[path = "zip_tests.rs"]
mod tests;
