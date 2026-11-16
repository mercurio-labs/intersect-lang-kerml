use super::*;
use crate::zip::ZipEntry;

fn project(name: &str) -> Project {
    Project {
        name: name.into(),
        version: "1.2.0".into(),
        ..Project::default()
    }
}

fn meta() -> Meta {
    Meta {
        index: [("Vehicles".to_owned(), "model/Vehicles.sysml".to_owned())].into(),
        created: "2026-11-10T00:00:00Z".into(),
        metamodel: Some("https://www.omg.org/spec/SysML/20250201".into()),
        ..Meta::default()
    }
}

fn files() -> Vec<ArchiveFile> {
    vec![
        ArchiveFile {
            path: "model/Vehicles.sysml".into(),
            text: "package Vehicles { part def Car; }\n".into(),
        },
        ArchiveFile {
            path: "Base Types.kerml".into(),
            text: "package BaseTypes;\n".into(),
        },
    ]
}

fn read_default(bytes: &[u8]) -> Result<ProjectArchive, KparError> {
    read("deps/vehicles.kpar", bytes, &ArchiveLimits::default())
}

/// Raw archive with the given entries (metadata included verbatim).
fn raw(entries: &[(&str, &str)]) -> Vec<u8> {
    let entries: Vec<ZipEntry> = entries
        .iter()
        .map(|(path, text)| ZipEntry {
            path: (*path).into(),
            bytes: text.as_bytes().to_vec(),
        })
        .collect();
    crate::zip::write(&entries).unwrap_or_default()
}

const PROJECT: &str = r#"{"name":"P","version":"1.0.0"}"#;
const META: &str = r#"{"index":{},"created":"2026-01-01T00:00:00Z"}"#;

#[test]
fn written_archives_read_back() -> Result<(), KparError> {
    let bytes = write(&project("Vehicles"), &meta(), &files())?;
    let archive = read_default(&bytes)?;
    assert_eq!(archive.project, project("Vehicles"));
    assert_eq!(archive.meta.index, meta().index);
    assert_eq!(archive.meta.includes_derived, Some(false));
    assert_eq!(archive.meta.checksum.len(), 2);
    let uris: Vec<&str> = archive.sources.iter().map(|s| s.uri.as_str()).collect();
    assert_eq!(
        uris,
        [
            "deps/vehicles.kpar!/Base%20Types.kerml",
            "deps/vehicles.kpar!/model/Vehicles.sysml"
        ]
    );
    assert_eq!(archive.sources[0].language, Language::KerML);
    assert_eq!(archive.sources[1].text, files()[0].text);
    assert!(archive.notes.is_empty(), "{:?}", archive.notes);
    assert_eq!(write(&project("Vehicles"), &meta(), &files())?, bytes);
    Ok(())
}

#[test]
fn the_python_fixture_archive_reads() -> Result<(), KparError> {
    let archive = read_default(include_bytes!("../tests/fixtures/python-streamed.zip"))?;
    assert_eq!(archive.project.name, "P");
    assert_eq!(archive.sources.len(), 1);
    assert_eq!(archive.meta.index["P"], "pkg/P.sysml");
    Ok(())
}

#[test]
fn checksums_are_verified() -> Result<(), KparError> {
    let text = "package P;";
    let good = digest("SHA256", text.as_bytes()).unwrap_or_default();
    let meta_with = |value: &str, algorithm: &str| {
        format!(
            r#"{{"index":{{}},"created":"2026-01-01T00:00:00Z","checksum":{{"P.sysml":{{"value":"{value}","algorithm":"{algorithm}"}}}}}}"#
        )
    };
    let upper = good.to_uppercase();
    read_default(&raw(&[
        (".project.json", PROJECT),
        (".meta.json", &meta_with(&upper, "SHA256")),
        ("P.sysml", text),
    ]))?;

    let bad = meta_with(&"0".repeat(64), "SHA256");
    let error = read_default(&raw(&[
        (".project.json", PROJECT),
        (".meta.json", &bad),
        ("P.sysml", text),
    ]));
    assert!(
        matches!(&error, Err(e) if e.kind == ErrorKind::Checksum),
        "{error:?}"
    );

    let unknown = meta_with("abc", "MD5");
    let error = read_default(&raw(&[
        (".project.json", PROJECT),
        (".meta.json", &unknown),
        ("P.sysml", text),
    ]));
    assert!(
        matches!(&error, Err(e) if e.kind == ErrorKind::Unsupported),
        "{error:?}"
    );

    let none = meta_with("", "NONE");
    let archive = read_default(&raw(&[
        (".project.json", PROJECT),
        (".meta.json", &none),
        ("P.sysml", text),
    ]))?;
    assert!(archive.notes.iter().any(|n| n.contains("NONE")));
    Ok(())
}

#[test]
fn missing_or_bad_metadata_is_refused() {
    for entries in [
        vec![(".meta.json", META)],
        vec![(".project.json", PROJECT)],
        vec![(".project.json", "{}"), (".meta.json", META)],
        vec![("sub/.project.json", PROJECT), (".meta.json", META)],
    ] {
        let error = read_default(&raw(&entries));
        assert!(
            matches!(&error, Err(e) if e.kind == ErrorKind::Metadata),
            "{entries:?}: {error:?}"
        );
    }
}

#[test]
fn content_that_is_not_loaded_is_refused_explicitly() {
    let derived = r#"{"index":{},"created":"2026-01-01T00:00:00Z","includesDerived":true}"#;
    let foreign = r#"{"index":{},"created":"2026-01-01T00:00:00Z","metamodel":"https://www.sysand.com/spec/SysML/20250201"}"#;
    for entries in [
        vec![(".project.json", PROJECT), (".meta.json", derived)],
        vec![(".project.json", PROJECT), (".meta.json", foreign)],
        vec![
            (".project.json", PROJECT),
            (".meta.json", META),
            ("model.json", "{}"),
        ],
        vec![
            (".project.json", PROJECT),
            (".meta.json", META),
            ("Model.XMI", "<x/>"),
        ],
        vec![
            (".project.json", PROJECT),
            (".meta.json", META),
            ("dep.kpar", ""),
        ],
    ] {
        let error = read_default(&raw(&entries));
        assert!(
            matches!(&error, Err(e) if e.kind == ErrorKind::Unsupported),
            "{entries:?}: {error:?}"
        );
    }
}

#[test]
fn other_files_are_noted_not_dropped_silently() -> Result<(), KparError> {
    let archive = read_default(&raw(&[
        (".project.json", PROJECT),
        (".meta.json", META),
        ("README.md", "hi"),
    ]))?;
    assert!(archive.notes.iter().any(|n| n.starts_with("README.md")));
    assert!(archive.notes.iter().any(|n| n.contains("no model files")));
    Ok(())
}

#[test]
fn errors_name_the_archive_and_entry() {
    let error = read_default(&raw(&[
        (".project.json", PROJECT),
        (".meta.json", META),
        ("m.xmi", ""),
    ]))
    .err()
    .map(|e| e.to_string())
    .unwrap_or_default();
    assert!(error.starts_with("deps/vehicles.kpar!/m.xmi: "), "{error}");
}

#[test]
fn entry_paths_are_percent_encoded_in_uris() {
    assert_eq!(
        entry_uri("a.kpar", "Domain Libraries/Ä#1.sysml"),
        "a.kpar!/Domain%20Libraries/%C3%84%231.sysml"
    );
}

#[test]
fn writing_refuses_bad_input() {
    let mut empty = project("");
    assert!(write(&empty, &meta(), &files()).is_err());
    empty.name = "P".into();
    let mut stale = meta();
    stale.created = "now".into();
    assert!(write(&empty, &stale, &files()).is_err());
    let mut dangling = meta();
    dangling.index.insert("Gone".into(), "Gone.sysml".into());
    assert!(write(&empty, &dangling, &files()).is_err());
    let text = vec![ArchiveFile {
        path: "notes.txt".into(),
        text: String::new(),
    }];
    assert!(
        write(
            &empty,
            &Meta {
                index: Default::default(),
                ..meta()
            },
            &text
        )
        .is_err()
    );
}

/// The OMG library archives keep the project in one folder
/// (`Kernel Semantic Library/.project.json`); index and checksum paths are
/// relative to it.
#[test]
fn a_project_folder_inside_the_archive_is_the_root() -> Result<(), KparError> {
    let text = "package P;";
    let sum = digest("SHA256", text.as_bytes()).unwrap_or_default();
    let meta = format!(
        r#"{{"index":{{"P":"P.kerml"}},"created":"2026-01-01T00:00:00Z","checksum":{{"P.kerml":{{"value":"{sum}","algorithm":"SHA256"}}}}}}"#
    );
    let archive = read_default(&raw(&[
        ("Kernel Lib/.project.json", PROJECT),
        ("Kernel Lib/.meta.json", &meta),
        ("Kernel Lib/P.kerml", text),
        ("Other.kerml", "package O;"),
    ]))?;
    assert_eq!(archive.root, "Kernel Lib/");
    assert_eq!(archive.sources.len(), 1);
    assert_eq!(archive.sources[0].path, "P.kerml");
    assert_eq!(archive.sources[0].entry, "Kernel Lib/P.kerml");
    assert_eq!(
        archive.sources[0].uri,
        "deps/vehicles.kpar!/Kernel%20Lib/P.kerml"
    );
    assert!(
        archive
            .notes
            .iter()
            .any(|n| n.starts_with("Other.kerml: outside")),
        "{:?}",
        archive.notes
    );
    Ok(())
}

#[test]
fn several_project_files_are_ambiguous() {
    let error = read_default(&raw(&[
        ("a/.project.json", PROJECT),
        ("a/.meta.json", META),
        ("b/.project.json", PROJECT),
        ("b/.meta.json", META),
    ]));
    assert!(
        matches!(&error, Err(e) if e.kind == ErrorKind::Metadata && e.message.contains("ambiguous")),
        "{error:?}"
    );
}

/// The ten OMG-published standard library archives (KerML and SysML
/// release 20250201; URLs and SHA-256 in `fixtures/kpar/omg-20250201.json`),
/// when `INTERSECT_KPAR_OMG` names a folder holding them. They are not
/// redistributed in this repository.
#[test]
fn omg_library_archives_read() -> Result<(), KparError> {
    let Some(folder) = std::env::var_os("INTERSECT_KPAR_OMG") else {
        eprintln!("SKIPPED: INTERSECT_KPAR_OMG is unset");
        return Ok(());
    };
    let mut total = 0;
    for (name, files, root) in [
        ("Semantic-Library.kpar", 16, "Kernel Semantic Library/"),
        ("Data-Type-Library.kpar", 3, "Kernel Data Type Library/"),
        ("Function-Library.kpar", 17, "Kernel Function Library/"),
        ("Systems-Library.kpar", 21, "Systems Library/"),
        ("Analysis-Domain-Library.kpar", 4, "Analysis/"),
        (
            "Cause-and-Effect-Domain-Library.kpar",
            2,
            "Cause and Effect/",
        ),
        ("Geometry-Domain-Library.kpar", 2, "Geometry/"),
        ("Metadata-Domain-Library.kpar", 4, "Metadata/"),
        (
            "Quantities-and-Units-Domain-Library.kpar",
            23,
            "Quantities and Units/",
        ),
        (
            "Requirement-Derivation-Domain-Library.kpar",
            2,
            "Requirement Derivation/",
        ),
    ] {
        let path = std::path::Path::new(&folder).join(name);
        let bytes = std::fs::read(&path)
            .map_err(|e| KparError::new(name, None, ErrorKind::Container, e.to_string()))?;
        let archive = read(name, &bytes, &ArchiveLimits::default())?;
        assert_eq!(archive.root, root);
        assert_eq!(archive.sources.len(), files, "{name}");
        assert_eq!(archive.meta.index.len(), files, "{name}");
        // OMG's Systems Library archive also holds macOS metadata
        // (`.DS_Store`, `__MACOSX/...`): noted, not loaded.
        assert!(
            archive.notes.iter().all(|note| note.contains(".DS_Store")),
            "{name}: {:?}",
            archive.notes
        );
        assert!(
            archive
                .project
                .usage
                .iter()
                .all(|u| crate::resolve::is_standard_library(&u.resource)),
            "{name}"
        );
        // Two of OMG's index entries name files the archives do not hold
        // (the files are `AnalysisCases.sysml` and
        // `RequirementDerivation.sysml`): warnings, not refusals.
        let expected: &[&str] = match name {
            "Systems-Library.kpar" => &["index entry 'AnalysisCases' names 'AnalysisCase.sysml'"],
            "Requirement-Derivation-Domain-Library.kpar" => {
                &["index entry 'RequirementsDerivation' names 'RequirementsDerivation.sysml'"]
            }
            _ => &[],
        };
        assert_eq!(
            archive.warnings.len(),
            expected.len(),
            "{name}: {:?}",
            archive.warnings
        );
        for (warning, expected) in archive.warnings.iter().zip(expected) {
            assert!(warning.contains(expected), "{name}: {warning}");
        }
        total += files;
    }
    assert_eq!(total, 94);
    Ok(())
}

#[test]
fn a_dangling_index_entry_is_a_warning() -> Result<(), KparError> {
    let archive = read_default(&raw(&[
        (".project.json", PROJECT),
        (
            ".meta.json",
            r#"{"index":{"X":"X.sysml"},"created":"2026-01-01T00:00:00Z"}"#,
        ),
    ]))?;
    assert_eq!(archive.warnings.len(), 1);
    assert!(archive.warnings[0].contains("index entry 'X' names 'X.sysml'"));
    Ok(())
}
