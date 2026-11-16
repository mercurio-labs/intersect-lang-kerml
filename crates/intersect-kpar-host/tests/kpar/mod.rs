use intersect_kpar::{ArchiveFile, ArchiveLimits, ErrorKind, Meta, Project, zip::ZipEntry};
use intersect_kpar_host::{KparCatalog, KparPayload, KparSource};
use intersect_model::{
    definitions::pack::FileDigest,
    project_usages::{PackageCatalog, ResolutionError, Usage, UsagePath, UsageResolver},
};
use intersect_project_host::{ManifestError, PackageSource, SourcePackage};

const COMMON: &str = "urn:fixture:common";
const CONTROLS: &str = "urn:fixture:controls";
const VEHICLE: &str = "urn:fixture:vehicle";
const LOCATION: &str = "fixtures/source.kpar";

fn acquire<'a>(uri: &'a str, bytes: &'a [u8]) -> SourcePackage<KparPayload<'a>> {
    source(uri, LOCATION, bytes).acquire().unwrap()
}

fn source<'a>(uri: &'a str, location: &'a str, bytes: &'a [u8]) -> KparSource<'a> {
    KparSource {
        package_uri: uri,
        archive_uri: location,
        bytes,
        limits: ArchiveLimits::default(),
    }
}

fn usage(resource_uri: &str, constraint: Option<&str>) -> Usage {
    Usage {
        resource_uri: resource_uri.to_owned(),
        version_constraint: constraint.map(str::to_owned),
    }
}

fn fixture(name: &str, version: &str, usages: &[Usage]) -> Vec<u8> {
    let path = format!("model/{name}.kerml");
    intersect_kpar::write(
        &Project {
            name: name.into(),
            version: version.into(),
            usage: usages
                .iter()
                .map(|usage| intersect_kpar::Usage {
                    resource: usage.resource_uri.clone(),
                    version_constraint: usage.version_constraint.clone(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        },
        &Meta {
            created: "2025-03-13T00:00:00Z".into(),
            index: [(name.into(), path.clone())].into(),
            metamodel: Some("https://www.omg.org/spec/KerML/20250201".into()),
            ..Default::default()
        },
        &[ArchiveFile {
            path,
            text: format!("package {name} {{}}\n"),
        }],
    )
    .unwrap()
}

#[test]
fn acquires_section_10_3_project_and_meta_records() {
    let bytes = fixture(
        "Vehicle",
        "1.0.0",
        &[
            usage(CONTROLS, None),
            usage(COMMON, Some(">=1.0.0, <2.0.0")),
        ],
    );
    let package = acquire(VEHICLE, &bytes);
    assert_eq!(package.uri, VEHICLE);
    assert_eq!(package.version, "1.0.0");
    assert_eq!(package.provenance.uri, LOCATION);
    assert_eq!(package.provenance.sha256, FileDigest::of(&bytes).sha256);
    assert_eq!(package.content_digest, package.provenance.sha256);
    assert_eq!(package.payload.bytes, bytes);
    assert_eq!(
        package.usages,
        [
            usage(CONTROLS, None),
            usage(COMMON, Some(">=1.0.0, <2.0.0"))
        ]
    );
    let archive = &package.payload.archive;
    assert_eq!(archive.meta.index["Vehicle"], "model/Vehicle.kerml");
    assert_eq!(
        archive.sources[0].uri,
        "fixtures/source.kpar!/model/Vehicle.kerml"
    );
    assert_eq!(archive.sources[0].text, "package Vehicle {}\n");
}

#[test]
fn acquires_the_existing_independently_written_deflate_fixture() {
    let bytes = include_bytes!("../../../intersect-kpar/tests/fixtures/python-deflate.zip");
    let package = acquire(COMMON, bytes);
    assert_eq!(package.payload.archive.project.name, "P");
    assert_eq!(package.payload.archive.meta.index["P"], "pkg/P.sysml");
    assert_eq!(
        package.payload.archive.sources[0].uri,
        "fixtures/source.kpar!/pkg/P.sysml"
    );
}

#[test]
fn binds_catalog_entries_with_the_core_resolver() {
    let common1 = fixture("Common", "1.0.0", &[]);
    let common2 = fixture("Common", "2.0.0", &[]);
    let controls = fixture("Controls", "1.0.0", &[usage(COMMON, Some("=1.0.0"))]);
    let vehicle = fixture(
        "Vehicle",
        "1.0.0",
        &[usage(CONTROLS, None), usage(COMMON, Some("^1.0.0"))],
    );
    let controls = acquire(CONTROLS, &controls);
    let vehicle = acquire(VEHICLE, &vehicle);
    let mut catalog = KparCatalog::default();

    assert!(matches!(
        catalog.register(&controls, "engine", "schema"),
        Err(ManifestError::Dependencies(
            ResolutionError::Unsatisfied { .. }
        ))
    ));
    assert!(catalog.candidates(CONTROLS, None).is_empty());
    assert!(matches!(
        catalog.register(&acquire(COMMON, &common1), "", "schema"),
        Err(ManifestError::MissingBuildContext)
    ));
    assert!(catalog.candidates(COMMON, None).is_empty());
    catalog
        .register(&acquire(COMMON, &common2), "engine", "schema")
        .unwrap();
    let common = catalog
        .register(&acquire(COMMON, &common1), "engine", "schema")
        .unwrap();
    let mirror = source(COMMON, "mirror/common.kpar", &common1)
        .acquire()
        .unwrap();
    assert_eq!(
        catalog.register(&mirror, "engine", "schema").unwrap(),
        common
    );
    let controls_identity = catalog.register(&controls, "engine", "schema").unwrap();
    assert_eq!(
        controls_identity.dependency_lock_digest,
        UsageResolver::resolve(&controls.usages, &catalog)
            .unwrap()
            .digest()
            .unwrap()
    );

    let lock = UsageResolver::resolve(&vehicle.usages, &catalog).unwrap();
    let entry = &lock.entries()[COMMON];
    assert_eq!(entry.identity, common);
    assert!(entry.direct);
    assert_eq!(
        entry.requested_by,
        [
            UsagePath(vec![usage(COMMON, Some("^1.0.0"))]),
            UsagePath(vec![usage(CONTROLS, None), usage(COMMON, Some("=1.0.0"))]),
        ]
    );
    assert_eq!(lock.entries()[CONTROLS].identity, controls_identity);
    assert!(catalog.candidates("urn:other:common", None).is_empty());
    assert!(matches!(
        UsageResolver::resolve(&[usage(COMMON, None)], &catalog),
        Err(ResolutionError::Ambiguous { usage, .. }) if usage.resource_uri == COMMON
    ));
    let mut reversed = KparCatalog::default();
    reversed
        .register(&acquire(COMMON, &common1), "engine", "schema")
        .unwrap();
    reversed
        .register(&acquire(COMMON, &common2), "engine", "schema")
        .unwrap();
    reversed.register(&controls, "engine", "schema").unwrap();
    assert_eq!(
        UsageResolver::resolve(&vehicle.usages, &reversed).unwrap(),
        lock
    );
}

#[test]
fn inherits_archive_wrapper_compatibility_and_retains_metadata() {
    let bytes = intersect_kpar::zip::write(&[
        ZipEntry {
            path: "Wrapped/.project.json".into(),
            bytes: br#"{"name":"Common","version":"1.0.0","extra":"original"}"#.to_vec(),
        },
        ZipEntry {
            path: "Wrapped/.meta.json".into(),
            bytes: br#"{"index":{"Common":"Common.kerml"},"created":"2025-03-13T00:00:00Z"}"#
                .to_vec(),
        },
        ZipEntry {
            path: "Wrapped/Common.kerml".into(),
            bytes: b"package Common {}\n".to_vec(),
        },
    ])
    .unwrap();
    let package = acquire(COMMON, &bytes);
    let archive = &package.payload.archive;
    assert_eq!(archive.root, "Wrapped/");
    assert_eq!(archive.project.extra["extra"].as_str(), Some("original"));
    assert_eq!(archive.sources[0].entry, "Wrapped/Common.kerml");
    assert_eq!(package.payload.bytes, bytes);
}

#[test]
fn propagates_reader_errors_and_refuses_unsupported_constraints() {
    let invalid = source(COMMON, LOCATION, b"not a ZIP")
        .acquire()
        .unwrap_err();
    assert_eq!(invalid.kind, ErrorKind::Container);
    let bytes = fixture(
        "Controls",
        "1.0.0",
        &[usage(COMMON, Some("^1.0.0 || ^2.0.0"))],
    );
    let unsupported = source(CONTROLS, LOCATION, &bytes).acquire().unwrap_err();
    assert_eq!(unsupported.kind, ErrorKind::Dependency);
    assert_eq!(unsupported.entry.as_deref(), Some(".project.json"));
}

#[test]
fn registered_unconstrained_archive_keeps_its_recorded_identity() {
    let common1 = fixture("Common", "1.0.0", &[]);
    let common2 = fixture("Common", "2.0.0", &[]);
    let controls = fixture("Controls", "1.0.0", &[usage(COMMON, None)]);
    let mut catalog = KparCatalog::default();
    let identity = catalog
        .register(&acquire(COMMON, &common1), "engine", "schema")
        .unwrap();
    catalog
        .register(&acquire(CONTROLS, &controls), "engine", "schema")
        .unwrap();
    let before = UsageResolver::resolve(&[usage(CONTROLS, None)], &catalog).unwrap();
    catalog
        .register(&acquire(COMMON, &common2), "engine", "schema")
        .unwrap();
    let after = UsageResolver::resolve(&[usage(CONTROLS, None)], &catalog).unwrap();
    assert_eq!(before, after);
    let recorded = catalog.candidates(CONTROLS, None).pop().unwrap();
    assert_eq!(
        recorded.dependency_lock.entries()[COMMON].identity,
        identity
    );
    assert_eq!(
        recorded.dependency_lock.digest().unwrap(),
        recorded.identity.dependency_lock_digest
    );
}

#[test]
fn invalid_root_constraints_have_a_constraint_error() {
    let invalid = usage(COMMON, Some("^1.0.0 || ^2.0.0"));
    assert!(
        matches!(UsageResolver::resolve(std::slice::from_ref(&invalid), &KparCatalog::default()),
        Err(ResolutionError::Constraint { usage, .. }) if usage == invalid)
    );
}

#[test]
fn archive_standard_library_usage_resolves_through_a_combined_catalog() {
    use intersect_kerml::{
        language_pack::{LibraryContext, LibraryOptions, prepare_library},
        standard_library::{CombinedCatalog, STANDARD_LIBRARY_URIS, StdlibCatalog},
    };
    use intersect_model::{
        core::frozen::{ById, Metamodel, ModelBuilder},
        definitions::library_part::LibrarySource,
    };
    use std::sync::Arc;
    let schema = intersect_model::compile_rcore(
        r#"rcore 1; package fixture version "1";
metamodel "urn:fixture:library" prefix f { class Node {} }"#,
    )
    .unwrap();
    let mm = Arc::new(Metamodel::freeze(schema.schema()).unwrap());
    let sources = [LibrarySource {
        uri: "library/Base.kerml".into(),
        language: "kerml".into(),
        text: "package Base;".into(),
    }];
    let library = prepare_library(
        None,
        Some(&sources),
        LibraryContext {
            metamodel: &mm,
            bindings: "fixture-bindings",
        },
        LibraryOptions::default(),
        |_| {
            let class = mm
                .schema()
                .declarations
                .iter()
                .find(|(key, d)| d.name == "Node" && mm.schema().classes.contains_key(*key))
                .and_then(|(key, _)| mm.class(key))
                .ok_or("Node")?;
            let mut builder = ModelBuilder::new(mm.clone());
            builder
                .create("base", class, "library/Base.kerml")
                .map_err(|e| e.to_string())?;
            builder
                .link(&ById)
                .map_err(|e| e.to_string())?
                .validate()
                .map_err(|e| e.to_string())
        },
    )
    .unwrap();
    let stdlib = StdlibCatalog::new(&library).unwrap();
    let bytes = include_bytes!("../fixtures/stdlib-usage.kpar");
    let package = acquire("urn:fixture:stdlib-consumer", bytes);
    assert_eq!(package.usages, [usage(STANDARD_LIBRARY_URIS[0], None)]);
    let mut archives = KparCatalog::default();
    let manifest = {
        let catalogs: [&dyn PackageCatalog; 2] = [&archives, &stdlib];
        package
            .manifest(
                "archive-engine",
                &library.identity.schema,
                &CombinedCatalog(&catalogs),
            )
            .unwrap()
    };
    let archive_identity = archives.register_manifest(manifest);
    let roots = [usage(&package.uri, None)];
    let catalogs: [&dyn PackageCatalog; 2] = [&archives, &stdlib];
    let lock = UsageResolver::resolve(&roots, &CombinedCatalog(&catalogs)).unwrap();
    assert_eq!(
        lock.entries()[STANDARD_LIBRARY_URIS[0]].identity,
        library.identity
    );
    assert!(!lock.entries()[STANDARD_LIBRARY_URIS[0]].direct);
    assert_eq!(lock.entries()[&package.uri].identity, archive_identity);
    let reversed: [&dyn PackageCatalog; 2] = [&stdlib, &archives];
    assert_eq!(
        lock,
        UsageResolver::resolve(&roots, &CombinedCatalog(&reversed)).unwrap()
    );
}
