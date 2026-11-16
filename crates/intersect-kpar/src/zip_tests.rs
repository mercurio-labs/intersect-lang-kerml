use super::*;

fn entry(path: &str, text: &str) -> ZipEntry {
    ZipEntry {
        path: path.into(),
        bytes: text.as_bytes().to_vec(),
    }
}

fn sample() -> Vec<ZipEntry> {
    vec![
        entry("b/Second.sysml", "package Second;"),
        entry("A.kerml", &"package A { }\n".repeat(200)),
        entry("ünïcode/Ä.sysml", "package U;"),
    ]
}

#[test]
fn written_archives_read_back_sorted() -> Result<(), ZipError> {
    let bytes = write(&sample())?;
    let read_back = read(&bytes, &ArchiveLimits::default())?;
    let mut expected = sample();
    expected.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(read_back, expected);
    Ok(())
}

#[test]
fn writing_is_deterministic_and_order_independent() -> Result<(), ZipError> {
    let mut reversed = sample();
    reversed.reverse();
    assert_eq!(write(&sample())?, write(&reversed)?);
    Ok(())
}

#[test]
fn repetitive_entries_are_deflated_and_tiny_ones_stored() -> Result<(), ZipError> {
    let bytes = write(&sample())?;
    // The 2800-byte A.kerml must not appear verbatim.
    let text = "package A { }\n".repeat(200);
    assert!(bytes.len() < text.len());
    assert!(
        bytes
            .windows(b"package Second;".len())
            .any(|w| w == b"package Second;")
    );
    Ok(())
}

/// Archives written by Python's `zipfile` (an independent writer): one
/// seekable (sizes in the local headers) and one streamed (sizes in data
/// descriptors after each entry, as Java's `ZipOutputStream` writes).
#[test]
fn independent_writers_archives_read() -> Result<(), ZipError> {
    for bytes in [
        &include_bytes!("../tests/fixtures/python-deflate.zip")[..],
        &include_bytes!("../tests/fixtures/python-streamed.zip")[..],
    ] {
        let entries = read(bytes, &ArchiveLimits::default())?;
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, [".meta.json", ".project.json", "pkg/P.sysml"]);
        assert_eq!(entries[2].bytes, b"package P;\n");
    }
    Ok(())
}

#[test]
fn unsafe_paths_are_refused() {
    for path in [
        "",
        "/abs.sysml",
        "a\\b.sysml",
        "C:/x.sysml",
        "a//b.sysml",
        "./a.sysml",
        "../a.sysml",
        "a/../../b.sysml",
        "a/\u{7}.sysml",
    ] {
        assert!(
            matches!(check_path(path), Err(ZipError::Path { .. })),
            "{path:?}"
        );
        assert!(write(&[entry(path, "x")]).is_err(), "{path:?}");
    }
    assert!(check_path("Domain Libraries/Quantities.sysml").is_ok());
}

#[test]
fn case_only_duplicates_are_refused() {
    let error = write(&[entry("A.sysml", "x"), entry("a.sysml", "y")]);
    assert!(matches!(error, Err(ZipError::Path { .. })));
}

fn first_name_offset(bytes: &[u8], name: &str) -> usize {
    bytes
        .windows(name.len())
        .position(|w| w == name.as_bytes())
        .unwrap_or(usize::MAX)
}

/// Overwrite both copies (local and central) of an entry name.
fn rename_all(bytes: &mut [u8], from: &str, to: &str) {
    assert_eq!(from.len(), to.len());
    let mut at = 0;
    while let Some(found) = bytes[at..]
        .windows(from.len())
        .position(|w| w == from.as_bytes())
    {
        let start = at + found;
        bytes[start..start + to.len()].copy_from_slice(to.as_bytes());
        at = start + to.len();
    }
}

#[test]
fn traversal_entry_in_a_foreign_archive_is_refused() -> Result<(), ZipError> {
    let mut bytes = write(&[entry("xx/evil.sysml", "x")])?;
    rename_all(&mut bytes, "xx/evil", "../evil");
    let error = read(&bytes, &ArchiveLimits::default());
    assert!(matches!(error, Err(ZipError::Path { .. })), "{error:?}");
    Ok(())
}

#[test]
fn corrupt_content_fails_its_crc() -> Result<(), ZipError> {
    let mut bytes = write(&[entry("a.sysml", "package Abc;")])?;
    let at = first_name_offset(&bytes, "package Abc;");
    bytes[at + 8] = b'X';
    let error = read(&bytes, &ArchiveLimits::default());
    assert!(matches!(error, Err(ZipError::Corrupt(_))), "{error:?}");
    Ok(())
}

#[test]
fn truncated_and_foreign_bytes_are_refused() -> Result<(), ZipError> {
    let bytes = write(&sample())?;
    for broken in [
        &bytes[..bytes.len() - 1],
        &bytes[..10],
        b"not a zip at all, no".as_slice(),
    ] {
        assert!(
            matches!(
                read(broken, &ArchiveLimits::default()),
                Err(ZipError::Corrupt(_))
            ),
            "{} bytes",
            broken.len()
        );
    }
    // The central directory points past the data.
    let mut moved = bytes.clone();
    let end = moved.len() - END_LEN;
    moved[end + 16..end + 20].copy_from_slice(&u32::MAX.wrapping_sub(1).to_le_bytes());
    assert!(read(&moved, &ArchiveLimits::default()).is_err());
    Ok(())
}

#[test]
fn encrypted_entries_are_refused() -> Result<(), ZipError> {
    let mut bytes = write(&[entry("a.sysml", "x")])?;
    let end = end_of_directory(&bytes)?;
    let directory = u32_at(&bytes, end + 16)? as usize;
    bytes[directory + 8] |= 1;
    let error = read(&bytes, &ArchiveLimits::default());
    assert!(matches!(error, Err(ZipError::Unsupported(_))), "{error:?}");
    Ok(())
}

#[test]
fn limits_are_enforced_before_inflating() -> Result<(), ZipError> {
    let bomb = vec![b' '; 4 * 1024 * 1024];
    let bytes = write(&[ZipEntry {
        path: "bomb.sysml".into(),
        bytes: bomb,
    }])?;
    let limits = ArchiveLimits::default();
    assert!(matches!(read(&bytes, &limits), Err(ZipError::Limit(_))));
    let tight = ArchiveLimits {
        max_ratio: u64::MAX,
        max_entry_bytes: 1024,
        ..limits
    };
    assert!(matches!(read(&bytes, &tight), Err(ZipError::Limit(_))));
    let total = ArchiveLimits {
        max_ratio: u64::MAX,
        max_total_bytes: 1024,
        ..limits
    };
    assert!(matches!(read(&bytes, &total), Err(ZipError::Limit(_))));
    let few = ArchiveLimits {
        max_entries: 2,
        ..limits
    };
    assert!(matches!(
        read(&write(&sample())?, &few),
        Err(ZipError::Limit(_))
    ));
    Ok(())
}
