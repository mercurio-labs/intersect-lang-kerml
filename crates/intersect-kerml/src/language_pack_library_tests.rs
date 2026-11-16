//! Handwritten unit tests of the prepared-library part (decision D10) on a
//! small model and a pack with placeholder program and binding sections.
use super::*;
use intersect_model::{
    core::frozen::{ById, FrozenError, ModelBuilder},
    definitions::pack::{PackHeader, canonical_json},
};

const SOURCE: &str = r#"rcore 1;
package library version "1";
metamodel "urn:intersect:library" prefix l {
  class Node { attr name: String [0,1]; ref next: Node [0,1]; }
}"#;

fn metamodel() -> Result<Arc<Metamodel>, String> {
    let package = intersect_model::compile_rcore_at("memory:/library.rcore", SOURCE)
        .map_err(|e| e.to_string())?;
    Metamodel::freeze(package.schema())
        .map(Arc::new)
        .map_err(|e| e.to_string())
}

fn sources(text: &str) -> Vec<LibrarySource> {
    vec![
        LibrarySource {
            uri: "lib/Base.kerml".into(),
            language: KERML_SOURCE.into(),
            text: text.into(),
        },
        LibrarySource {
            uri: "lib/Parts.sysml".into(),
            language: SYSML_SOURCE.into(),
            text: "package Parts;".into(),
        },
    ]
}

/// The "parse" of these tests: one object per source, chained.
fn parse(mm: &Arc<Metamodel>, sources: &[LibrarySource]) -> Result<FrozenModel, String> {
    let e = |error: FrozenError| error.to_string();
    let schema = mm.schema();
    let node = schema
        .declarations
        .iter()
        .find(|(key, d)| d.name == "Node" && schema.classes.contains_key(*key))
        .and_then(|(key, _)| mm.class(key))
        .ok_or("class Node")?;
    let (name, next) = (
        mm.feature(node, "name").ok_or("name")?,
        mm.feature(node, "next").ok_or("next")?,
    );
    let mut builder = ModelBuilder::new(mm.clone());
    let mut previous = None;
    for source in library_parse_order(sources)? {
        let obj = builder
            .create_at(&source.uri, node, &source.uri, 0, source.text.len())
            .map_err(e)?;
        builder.set_str(obj, name, &source.text).map_err(e)?;
        if let Some(previous) = previous {
            builder.add_ref(previous, next, obj).map_err(e)?;
        }
        previous = Some(obj);
    }
    builder
        .link(&ById)
        .map_err(|d| d.to_string())?
        .validate()
        .map_err(|d| d.to_string())
}

/// A pack with this recipe's three parts over placeholder JSON sections.
fn pack() -> Result<LanguagePack, String> {
    let mut parts = PartKeyInputs::current()?.parts()?;
    let payload = canonical_json(&serde_json::json!({})).map_err(|e| e.to_string())?;
    let output = FileDigest::of(&payload).sha256;
    for part in &mut parts {
        part.output_digest = output.clone();
    }
    let mut builder = PackBuilder::new(PackHeader {
        language: LANGUAGE.into(),
        recipe: recipe(),
        compiler: expected_compiler(),
        recipe_constants: super::super::recipe_constants()?,
        inputs: super::super::pinned_inputs(),
        parts: parts.clone(),
        requires_hooks: Vec::new(),
        depends_on: Vec::new(),
    });
    for part in &parts {
        builder = builder.section(
            part.name.clone(),
            "placeholder",
            PackSectionFlags::REQUIRED,
            payload.clone(),
            part.input_digest.clone(),
            output.clone(),
        );
    }
    let bytes = builder.build().map_err(|e| e.to_string())?;
    LanguagePack::read(bytes).map_err(|e| e.to_string())
}

fn context<'a>(mm: &'a Arc<Metamodel>, bindings: &'a str) -> LibraryContext<'a> {
    LibraryContext {
        metamodel: mm,
        bindings,
    }
}

#[test]
fn a_parsed_library_is_saved_and_then_loaded_from_its_snapshot() -> Result<(), String> {
    let mm = metamodel()?;
    let sources = sources("package Base;");
    let context = context(&mm, "bindings-1");
    let options = LibraryOptions::default();

    // No snapshot: parsed, with a payload to save.
    let first = prepare_library(None, Some(&sources), context, options, |s| parse(&mm, s))?;
    assert_eq!(
        first.origin,
        LibraryOrigin::Parsed("no library snapshot".into())
    );
    assert_eq!(first.key, identity_key(&first.identity)?);
    assert_eq!(
        first.manifest,
        LibraryManifest::of_sources(&sources).map_err(|e| e.to_string())?
    );
    for uri in first
        .model
        .resource_records()
        .map_err(|e| e.to_string())?
        .keys()
    {
        assert_eq!(first.model.resource_package(uri), Some(&first.identity));
    }
    let payload = first.payload.clone().ok_or("payload")?;
    assert_eq!(payload_key(&payload)?, first.key);

    // In a pack: loaded from the snapshot, exporting the same model.
    let base = pack()?;
    let with = LanguagePack::read(with_library_part(&base, payload.clone())?)
        .map_err(|e| e.to_string())?;
    let stored = pack_library_payload(&with).ok_or("library part")??;
    assert_eq!(stored, payload.as_slice());
    let loaded = prepare_library(Some(stored), Some(&sources), context, options, |_| {
        Err("must not parse".into())
    })?;
    assert_eq!(loaded.origin, LibraryOrigin::Snapshot);
    assert_eq!(loaded.key, first.key);
    assert_eq!(loaded.identity, first.identity);
    assert_eq!(loaded.manifest, first.manifest);
    assert!(loaded.payload.is_none());
    assert_eq!(
        loaded.model.to_model().map_err(|e| e.to_string())?,
        first.model.to_model().map_err(|e| e.to_string())?
    );
    // Without host sources, the pack's own manifest selects the snapshot.
    let selected = prepare_library(Some(stored), None, context, options, |_| {
        Err("must not parse".into())
    })?;
    assert_eq!(selected.origin, LibraryOrigin::Snapshot);

    // Removing the part gives back the original pack bytes; adding the same
    // part again gives the same bytes (deterministic).
    assert_eq!(without_library_part(&with)?, base.bytes());
    assert_eq!(with_library_part(&base, payload)?, with.bytes());
    Ok(())
}

#[test]
fn a_stale_or_refused_snapshot_falls_back_to_parsing() -> Result<(), String> {
    let mm = metamodel()?;
    let sources = sources("package Base;");
    let options = LibraryOptions::default();
    let built = prepare_library(None, Some(&sources), context(&mm, "b"), options, |s| {
        parse(&mm, s)
    })?;
    let payload = built.payload.ok_or("payload")?;
    let parsed = |label: &str,
                  payload: &[u8],
                  sources: Option<&[LibrarySource]>,
                  context: LibraryContext<'_>,
                  options: LibraryOptions|
     -> Result<String, String> {
        let result = prepare_library(Some(payload), sources, context, options, |s| parse(&mm, s))?;
        match result.origin {
            LibraryOrigin::Parsed(reason) => Ok(reason),
            LibraryOrigin::Snapshot => Err(format!("{label}: loaded the snapshot")),
        }
    };
    // Other bindings: another key.
    let reason = parsed(
        "bindings",
        &payload,
        Some(&sources),
        context(&mm, "c"),
        options,
    )?;
    assert!(reason.contains("key differs"), "{reason}");
    // Edited library sources on the host.
    let edited = self::sources("package Base; ");
    let reason = parsed(
        "sources",
        &payload,
        Some(&edited),
        context(&mm, "b"),
        options,
    )?;
    assert!(reason.contains("other library sources"), "{reason}");
    // Corrupt bytes.
    let mut corrupt = payload.clone();
    let last = corrupt.len() - 1;
    corrupt[last] ^= 1;
    let reason = parsed(
        "corrupt",
        &corrupt,
        Some(&sources),
        context(&mm, "b"),
        options,
    )?;
    assert!(reason.contains("refused"), "{reason}");
    // The cache disabled.
    let off = LibraryOptions {
        use_snapshot: false,
        ..options
    };
    let reason = parsed("disabled", &payload, Some(&sources), context(&mm, "b"), off)?;
    assert!(reason.contains("disabled"), "{reason}");
    // Nothing to load and nothing to parse.
    let nothing = prepare_library(None, None, context(&mm, "b"), options, |s| parse(&mm, s));
    assert!(nothing.is_err());
    // A stale part still supplies its own sources to parse.
    let rebuilt = prepare_library(Some(&payload), None, context(&mm, "c"), options, |s| {
        parse(&mm, s)
    })?;
    assert!(matches!(rebuilt.origin, LibraryOrigin::Parsed(_)));
    assert_ne!(rebuilt.key, built.key);
    Ok(())
}

#[test]
fn a_pack_build_reuses_the_library_part_only_for_its_key() -> Result<(), String> {
    let mm = metamodel()?;
    let sources = sources("package Base;");
    let options = LibraryOptions::default();
    let built = prepare_library(None, Some(&sources), context(&mm, "b"), options, |s| {
        parse(&mm, s)
    })?;
    let payload = built.payload.ok_or("payload")?;
    let base = pack()?;
    let with = LanguagePack::read(with_library_part(&base, payload.clone())?)
        .map_err(|e| e.to_string())?;
    let (decision, reused, refusal) = plan_library(Some(&with), &built.key, context(&mm, "b"));
    assert_eq!(decision, PartDecision::Reuse);
    assert_eq!(reused, Some(payload.as_slice()));
    assert!(refusal.is_none());
    let other = library_key(
        &LibraryManifest::of_sources(&sources).map_err(|e| e.to_string())?,
        context(&mm, "c"),
    )?;
    let (decision, reused, _) = plan_library(Some(&with), &other, context(&mm, "c"));
    assert_eq!(decision, PartDecision::Rebuild(RebuildReason::KeyChanged));
    assert!(reused.is_none());
    let (decision, _, _) = plan_library(Some(&base), &built.key, context(&mm, "b"));
    assert_eq!(decision, PartDecision::Rebuild(RebuildReason::KeyChanged));
    let (decision, _, _) = plan_library(None, &built.key, context(&mm, "b"));
    assert_eq!(decision, PartDecision::Rebuild(RebuildReason::NoPrevious));
    // The recipe's own checks still accept a pack with the library part.
    check_layout(with.manifest())?;
    Ok(())
}

#[test]
fn a_host_pack_drops_only_the_kerml_bindings_part() -> Result<(), String> {
    use super::super::{BINDINGS_SECTION, SYSML_BINDINGS_SECTION, without_kerml_bindings};
    let mm = metamodel()?;
    let sources = sources("package Base;");
    let built = prepare_library(
        None,
        Some(&sources),
        context(&mm, "b"),
        LibraryOptions::default(),
        |s| parse(&mm, s),
    )?;
    let full = LanguagePack::read(with_library_part(
        &pack()?,
        built.payload.ok_or("payload")?,
    )?)
    .map_err(|e| e.to_string())?;
    let host = without_kerml_bindings(&full)?;
    assert_eq!(host, without_kerml_bindings(&full)?, "deterministic");
    let host = LanguagePack::read(host).map_err(|e| e.to_string())?;
    // The part and its section are gone together; the layout still passes.
    check_layout(host.manifest())?;
    assert!(host.section_entry(BINDINGS_SECTION).is_none());
    assert!(
        host.manifest()
            .parts
            .iter()
            .all(|part| part.name != BINDINGS_SECTION)
    );
    // Everything else, the library part included, is kept byte for byte
    // (offsets move; kind, digests and bytes do not).
    for kept in &full.manifest().sections {
        if kept.name == BINDINGS_SECTION {
            continue;
        }
        let entry = host.section_entry(&kept.name).ok_or("section dropped")?;
        assert_eq!(entry.kind, kept.kind, "{}", kept.name);
        assert_eq!(entry.digest, kept.digest, "{}", kept.name);
        assert_eq!(entry.part_digest, kept.part_digest, "{}", kept.name);
        assert_eq!(entry.output_digest, kept.output_digest, "{}", kept.name);
        assert_eq!(host.section(&kept.name), full.section(&kept.name));
    }
    assert!(host.section_entry(SYSML_BINDINGS_SECTION).is_some());
    assert!(host.section_entry(LIBRARY_SECTION).is_some());
    // The KerML bindings load names the missing part. Dropping it again
    // changes nothing; a pack missing another recipe part is refused.
    let error = super::super::load_pack_bindings(&host)
        .err()
        .ok_or("a host pack loaded KerML bindings")?;
    assert!(error.contains(BINDINGS_SECTION), "{error}");
    assert_eq!(without_kerml_bindings(&host)?, host.bytes());
    let broken = LanguagePack::read(super::rebuild(&host, SYSML_BINDINGS_SECTION, None)?)
        .map_err(|e| e.to_string())?;
    assert!(check_layout(broken.manifest()).is_err());
    assert!(without_kerml_bindings(&broken).is_err());
    Ok(())
}

#[test]
fn library_sources_parse_kerml_first() -> Result<(), String> {
    let mut sources = sources("package Base;");
    sources.reverse();
    sources.push(LibrarySource {
        uri: "lib/A.sysml".into(),
        language: SYSML_SOURCE.into(),
        text: String::new(),
    });
    let order: Vec<_> = library_parse_order(&sources)?
        .into_iter()
        .map(|s| s.uri.as_str())
        .collect();
    assert_eq!(order, ["lib/Base.kerml", "lib/A.sysml", "lib/Parts.sysml"]);
    sources.push(LibrarySource {
        uri: "lib/x.txt".into(),
        language: "text".into(),
        text: String::new(),
    });
    assert!(library_parse_order(&sources).is_err());
    Ok(())
}

#[test]
fn package_identity_partitions_every_cache_input() -> Result<(), String> {
    use intersect_model::definitions::pack::PackRecipe;
    let source = sources("package Base;");
    let manifest = LibraryManifest::of_sources(&source).map_err(|e| e.to_string())?;
    let mut changed_sources = source.clone();
    changed_sources[0].text.push(' ');
    let changed_manifest =
        LibraryManifest::of_sources(&changed_sources).map_err(|e| e.to_string())?;
    let recipe = PackRecipe {
        name: "recipe".into(),
        version: "1".into(),
    };
    let other_recipe = PackRecipe {
        version: "2".into(),
        ..recipe.clone()
    };
    let programs = BTreeMap::from([("program/kerml".into(), "p1".into())]);
    let other_programs = BTreeMap::from([("program/kerml".into(), "p2".into())]);
    let compiler = BTreeMap::from([("compiler".into(), "c1".into())]);
    let other_compiler = BTreeMap::from([("compiler".into(), "c2".into())]);
    let input = LibraryKeyInput {
        part: LIBRARY_SECTION,
        part_version: LIBRARY_PART_VERSION,
        recipe: &recipe,
        manifest: &manifest,
        programs: &programs,
        bindings: "bindings",
        metamodel: "schema",
        compiler: &compiler,
        snapshot_format: SNAPSHOT_FORMAT,
        part_format: LIBRARY_PART_FORMAT,
    };
    let base = identity_for_inputs(input)?;
    assert_eq!(base, identity_for_inputs(input)?);
    assert_eq!(
        base.content_digest,
        manifest.digest().map_err(|e| e.to_string())?
    );
    assert_eq!(base.schema, "schema");
    assert_eq!(
        base.dependency_lock_digest,
        DependencyLock::empty().digest()?
    );
    let content = identity_for_inputs(LibraryKeyInput {
        manifest: &changed_manifest,
        ..input
    })?;
    assert_ne!(content.content_digest, base.content_digest);
    assert_eq!(content.engine, base.engine);
    assert_eq!(content.schema, base.schema);
    let schema = identity_for_inputs(LibraryKeyInput {
        metamodel: "other-schema",
        ..input
    })?;
    assert_ne!(schema.schema, base.schema);
    assert_eq!(schema.engine, base.engine);
    for changed in [
        LibraryKeyInput {
            part: "library/other",
            ..input
        },
        LibraryKeyInput {
            part_version: input.part_version + 1,
            ..input
        },
        LibraryKeyInput {
            recipe: &other_recipe,
            ..input
        },
        LibraryKeyInput {
            programs: &other_programs,
            ..input
        },
        LibraryKeyInput {
            bindings: "other-bindings",
            ..input
        },
        LibraryKeyInput {
            compiler: &other_compiler,
            ..input
        },
        LibraryKeyInput {
            snapshot_format: input.snapshot_format + 1,
            ..input
        },
        LibraryKeyInput {
            part_format: input.part_format + 1,
            ..input
        },
    ] {
        let changed = identity_for_inputs(changed)?;
        assert_ne!(changed.engine, base.engine);
        assert_eq!(changed.content_digest, base.content_digest);
        assert_eq!(changed.schema, base.schema);
        assert_ne!(identity_key(&changed)?, identity_key(&base)?);
    }
    let mut changed_lock = base.clone();
    changed_lock.dependency_lock_digest = "another-lock".into();
    assert_ne!(identity_key(&changed_lock)?, identity_key(&base)?);
    let mm = metamodel()?;
    let context = context(&mm, "bindings");
    let actual = library_package_identity(&manifest, context)?;
    assert_eq!(identity_key(&actual)?, library_key(&manifest, context)?);
    assert!(
        identity_for_inputs(LibraryKeyInput {
            bindings: "",
            ..input
        })
        .is_err()
    );
    Ok(())
}

#[test]
fn prepared_library_catalog_aliases_share_one_identity_and_check_constraints() -> Result<(), String>
{
    use crate::standard_library::{STANDARD_LIBRARY_URIS, StdlibCatalog};
    use intersect_model::project_usages::{PackageCatalog, Usage, UsageResolver};
    let mm = metamodel()?;
    let source = sources("package Base;");
    let mut prepared = prepare_library(
        None,
        Some(&source),
        context(&mm, "b"),
        LibraryOptions::default(),
        |s| parse(&mm, s),
    )?;
    let catalog = StdlibCatalog::new(&prepared)?;
    let roots: Vec<_> = STANDARD_LIBRARY_URIS
        .iter()
        .map(|uri| Usage {
            resource_uri: (*uri).into(),
            version_constraint: None,
        })
        .collect();
    let lock = UsageResolver::resolve(&roots, &catalog).map_err(|e| e.to_string())?;
    assert_eq!(lock.entries().len(), STANDARD_LIBRARY_URIS.len());
    assert!(
        lock.entries()
            .values()
            .all(|entry| entry.identity == prepared.identity)
    );
    // The synthetic source set cannot claim a published or pinned release.
    assert_eq!(
        catalog.candidates(STANDARD_LIBRARY_URIS[0], None)[0].version,
        "unversioned"
    );
    assert!(
        catalog
            .candidates(STANDARD_LIBRARY_URIS[0], Some("1.0.0"))
            .is_empty()
    );
    assert!(
        catalog
            .validate_usage(&Usage {
                resource_uri: STANDARD_LIBRARY_URIS[0].into(),
                version_constraint: Some("latest please".into())
            })
            .is_err()
    );
    assert!(
        catalog
            .candidates(
                "https://www.omg.org/spec/KerML/20990101/Semantic-Library.kpar",
                None
            )
            .is_empty()
    );
    prepared.key.push('x');
    assert!(StdlibCatalog::new(&prepared).is_err());
    Ok(())
}

#[test]
fn legacy_untagged_library_is_adapted_to_the_same_package_load_contract() -> Result<(), String> {
    use crate::standard_library::package_for_model;
    use intersect_model::package_load::{PackagePayload, load_against_lock};
    use intersect_model::project_usages::UsageResolver;
    let mm = metamodel()?;
    let source = sources("package Base;");
    let original = parse(&mm, &source)?;
    let (adapted, catalog) = package_for_model(&original, context(&mm, "b"))?;
    assert!(matches!(adapted, std::borrow::Cow::Owned(_)));
    assert!(
        adapted
            .resource_records()
            .map_err(|e| e.to_string())?
            .values()
            .all(|record| record.package.as_ref() == Some(catalog.identity()))
    );
    let lock = UsageResolver::resolve(
        &[crate::standard_library::implicit_stdlib_usage()],
        &catalog,
    )
    .map_err(|e| e.to_string())?;
    let project = intersect_model::model::Model {
        schema_revision: mm.revision().into(),
        elements: Vec::new(),
        resources: BTreeMap::new(),
    };
    let loaded = load_against_lock(
        mm.clone(),
        &lock,
        &[PackagePayload {
            identity: catalog.identity(),
            model: &adapted,
        }],
        &project,
    )
    .map_err(|e| e.to_string())?;
    let mut expected = original.to_model().map_err(|e| e.to_string())?;
    for record in expected.resources.values_mut() {
        record.package = Some(catalog.identity().clone());
    }
    assert_eq!(intersect_model::storage::export(&loaded.store)?, expected);
    let (borrowed, again) = package_for_model(&adapted, context(&mm, "b"))?;
    assert!(matches!(borrowed, std::borrow::Cow::Borrowed(_)));
    assert_eq!(again.identity(), catalog.identity());
    Ok(())
}
