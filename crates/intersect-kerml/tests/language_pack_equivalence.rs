//! Handwritten language-pack equivalence (language-pack steps 6a, 7 and 8): a
//! Pilot session restored from its host-built pack, in both trust modes, is
//! the same session a fresh preparation from the four original definitions
//! gives: it re-packs to identical bytes, has the same inventory, and parses
//! KerML and SysML documents identically. The pack's KerML Rcore bindings and
//! its combined KerML+SysML bindings load to bindings that save to the same
//! text as the freshly compiled ones, and a
//! SysML document linked through the loaded bindings (frozen engine, fast
//! executor) resolves exactly as through the fresh ones. Forged packs whose
//! container digests are valid are refused by the semantic checks.
//!
//! Program determinism: one session is packed twice (encoding), and the
//! incremental builds below prepare each program owner a second time,
//! independently, next to the other one restored, and must give the same
//! bytes. The whole-pack oracle is two independent full preparations (the
//! step 6b host's recompile check), run at milestone gates. Both binding
//! parts are compiled once here (every job of both recipes at once); their
//! compiler
//! output is re-verified by recompiling only in `sysml_pack check`.
//!
//! Incremental builds (language-pack step 8): previous packs with one part
//! damaged, stale or re-keyed rebuild only that part and give the full
//! pack's bytes; the observer markers name every reused and rebuilt part.
//! The damaged-bindings cases (one per binding part) run the whole bindings
//! rebuild path of `build_language_pack` with a compile closure that returns
//! the full pack's loaded scopes, so they prove the bindings-only rebuild end
//! to end except the compiler itself (see program determinism above).
use intersect_kerml::{
    kerml_bindings,
    language_pack::{
        self, BindingCompile, BindingRecipe, PartDecision, ProgramRole, RcoreBindingsPart,
        RebuildReason, SysmlBindingsPart,
    },
    native_pilot::{NativeKerMLSession, NativePilotSession},
    sysml_bindings,
};
use intersect_model::{
    core::{
        frozen::{FeatureId, FrozenError, FrozenStore, LinkView, ObjId},
        storage,
    },
    definitions::{
        bundle::DefinitionBundle,
        conversion::{InvocationContext, InvocationObserver, ProviderId},
        pack::{
            LanguagePack, PackBuilder, PackHeader, PackHook, PackManifest, PackPart,
            canonical_digest,
        },
        rcore_scope::VerifiedScope,
        setting::ComputedSettingContract,
    },
    frozen_scopes::{Execution, RcoreScopes},
};
use intersect_text::Source;
use intersect_text::{grammar::RuntimeAdmissionLoadMode, preparation::PreparedLanguageDefinition};
use serde_json::{Value, json};
use std::{collections::BTreeMap, rc::Rc, time::Instant};

#[allow(dead_code)]
#[path = "../examples/support/kerml_scopes.rs"]
mod kerml_scopes;

#[allow(dead_code)]
#[path = "../examples/support/sysml_scopes.rs"]
mod sysml_scopes;

#[path = "../examples/support/pilot_languages.rs"]
mod pilot_languages;

const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-pilot/v1/sources/"
);
const DEFINITIONS: [&str; 4] = [
    "org.omg.sysml/model/SysML.ecore",
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext",
    "org.omg.kerml.xtext/src/org/omg/kerml/xtext/KerML.xtext",
    "org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext",
];
const KERML_LIBRARIES: [&str; 2] = [
    "sysml.library/Kernel Libraries/Kernel Data Type Library/ScalarValues.kerml",
    // Deeply nested parenthesized expressions (prediction call depth 512).
    "sysml.library/Kernel Libraries/Kernel Function Library/VectorFunctions.kerml",
];
const KERML_TEXTS: [(&str, &str); 3] = [
    (
        "control/classes.kerml",
        "package P { class A; class B specializes A; feature f : A [0..*]; }\n",
    ),
    (
        "control/expressions.kerml",
        "package E { feature x = (1 + 2) * (3 - (4 / (5 + 6))); }\n",
    ),
    (
        "control/broken.kerml",
        "package Broken { feature x = (1 + ; }\n",
    ),
];
const SYSML_LIBRARIES: [&str; 1] = ["sysml.library/Systems Library/Items.sysml"];
const SYSML_TEXTS: [(&str, &str); 2] = [
    (
        "control/assignment-target.sysml",
        "package P {
	action def A {
		attribute index;
		attribute seq;
		action body;
		private action initialization
			assign index := 1;
		then private action whileLoop
			while index <= size(seq) {
				assign index := seq#(index);
				then perform body;
				then assign index := index + 1;
			}
	}
}
",
    ),
    (
        "control/parts.sysml",
        "package Q { part def Vehicle { attribute mass : Real; part wheels[4]; } part car : Vehicle; }\n",
    ),
];

struct Observer;
impl InvocationObserver for Observer {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}

fn read(path: &str) -> Result<Source, String> {
    Ok(Source {
        // Logical resource URIs may not contain spaces.
        uri: path.replace(' ', "%20"),
        text: std::fs::read_to_string(format!("{CORPUS}{path}"))
            .map_err(|error| format!("{path}: {error}"))?,
    })
}

fn inline(texts: &[(&str, &str)]) -> Vec<Source> {
    texts
        .iter()
        .map(|(uri, text)| Source {
            uri: (*uri).into(),
            text: (*text).into(),
        })
        .collect()
}

fn ms(clock: Instant) -> f64 {
    clock.elapsed().as_secs_f64() * 1e3
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

// Parse every KerML source into its own FrozenStore through the original
// KerML capability and its operand settings (bound by `pilot_languages`).
fn kerml_rows(
    definition: &PreparedLanguageDefinition<'_>,
    contracts: &[ComputedSettingContract],
    corpus: &[Source],
) -> Result<Vec<Value>, String> {
    pilot_languages::with_kerml(definition, contracts, |language| {
        let mut rows = Vec::new();
        for source in corpus {
            let mut store =
                FrozenStore::for_schema(definition.schema()).map_err(|e| e.to_string())?;
            let row = match language.parse(source, &mut store) {
                Ok((evaluation, references, consumed)) => json!({
                    "uri": source.uri,
                    "evaluation": to_json(&evaluation)?,
                    "references": references,
                    "consumed": consumed,
                    "model": to_json(&store.into_model().map_err(|e| e.to_string())?)?,
                }),
                Err(failure) => json!({ "uri": source.uri, "failure": failure }),
            };
            rows.push(row);
        }
        Ok(rows)
    })
}

// Parse every SysML source into its own FrozenStore over the loaded
// definition's own frozen metamodel (redefinitions computed at load).
fn sysml_rows(
    definition: &PreparedLanguageDefinition<'_>,
    contracts: &[ComputedSettingContract],
    corpus: &[Source],
) -> Result<Vec<Value>, String> {
    let preparation = InvocationContext::new(Rc::new(Observer));
    let metamodel = language_pack::frozen_metamodel(definition)?;
    pilot_languages::with_sysml(definition, contracts, &preparation, |language| {
        let mut rows = Vec::new();
        for source in corpus {
            let mut store = FrozenStore::new(metamodel.clone());
            let row = match language.parse(source, &mut store) {
                Ok((evaluation, references, consumed)) => {
                    let mut model = storage::export(&store).map_err(|e| e.to_string())?;
                    model.elements.sort_by(|a, b| a.id.cmp(&b.id));
                    json!({
                        "uri": source.uri,
                        "evaluation": to_json(&evaluation)?,
                        "references": references,
                        "consumed": consumed,
                        "model": to_json(&model)?,
                    })
                }
                Err(failure) => json!({ "uri": source.uri, "failure": failure }),
            };
            rows.push(row);
        }
        Ok(rows)
    })
}

struct Expected {
    inventory: Value,
    kerml: Vec<Value>,
    sysml: Vec<Value>,
}

fn observe(
    session: &NativePilotSession,
    kerml_corpus: &[Source],
    sysml_corpus: &[Source],
) -> Result<Expected, String> {
    let (kerml, sysml) =
        session.with_definitions(|kerml, kerml_contracts, sysml, sysml_contracts| {
            // The metamodel's redefinitions, computed at load from the converted
            // structure inside the program, equal those of the stored file.
            for definition in [kerml, sysml] {
                assert_eq!(
                    language_pack::metamodel_redefinitions(definition)?,
                    kerml_scopes::redefinitions(definition.schema())?
                );
            }
            Ok((
                kerml_rows(kerml, kerml_contracts, kerml_corpus)?,
                sysml_rows(sysml, sysml_contracts, sysml_corpus)?,
            ))
        })?;
    Ok(Expected {
        inventory: to_json(&session.inventory()?)?,
        kerml,
        sysml,
    })
}

/// (owner, feature, spelling, candidates) of one reference the link asked
/// the Rcore scopes to resolve.
type Resolution = (String, String, String, Result<Vec<String>, String>);

/// Parse `source` with the session's SysML program into a FrozenStore over
/// its frozen metamodel and link it through `scopes` on the fast executor
/// (`RcoreScopes`, which prepares a `FastProgram` per binding). Returns every
/// resolution and whether the link succeeded: this USES the bindings.
fn link_resolutions(
    session: &NativePilotSession,
    source: &Source,
    bundle: &DefinitionBundle,
    scopes: &[VerifiedScope],
) -> Result<(Vec<Resolution>, bool), String> {
    session.with_definitions(|_, _, sysml, contracts| {
        let preparation = InvocationContext::new(Rc::new(Observer));
        let metamodel = language_pack::frozen_metamodel(sysml)?;
        let resolvers = RcoreScopes::new(&metamodel, bundle, scopes)
            .map_err(|e| e.to_string())?
            .with_execution(Execution::Fast);
        pilot_languages::with_sysml(sysml, contracts, &preparation, |language| {
            let mut store = FrozenStore::new(metamodel.clone());
            let (evaluation, _, _) = language.parse(source, &mut store)?;
            if !evaluation.diagnostics.is_empty() {
                return Err(format!("{}: {:?}", source.uri, evaluation.diagnostics));
            }
            let outcomes = std::cell::RefCell::new(Vec::new());
            let resolve = |view: &LinkView<'_>,
                           owner: ObjId,
                           feature: FeatureId,
                           spelling: &str|
             -> Result<Option<ObjId>, FrozenError> {
                let found = resolvers.candidates(view, owner, feature, spelling);
                let names = found.as_ref().map_err(|e| e.to_string()).map(|found| {
                    found
                        .iter()
                        .filter_map(|object| view.identity(*object))
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                });
                outcomes.borrow_mut().push((
                    view.identity(owner).unwrap_or("").to_owned(),
                    metamodel.feature_name(feature).to_owned(),
                    spelling.to_owned(),
                    names,
                ));
                Ok(match found {
                    Ok(found) if found.len() == 1 => found.first().copied(),
                    _ => None,
                })
            };
            let linked = store
                .into_builder()
                .map_err(|e| e.to_string())?
                .link(&resolve)
                .is_ok();
            Ok((outcomes.into_inner(), linked))
        })
    })
}

fn assert_same(label: &str, actual: &Expected, expected: &Expected) {
    assert_eq!(actual.inventory, expected.inventory, "{label}: inventory");
    assert_eq!(actual.kerml.len(), expected.kerml.len());
    for (actual, expected) in actual.kerml.iter().zip(&expected.kerml) {
        assert_eq!(actual, expected, "{label}: KerML {}", expected["uri"]);
    }
    for (actual, expected) in actual.sysml.iter().zip(&expected.sysml) {
        assert_eq!(actual, expected, "{label}: SysML {}", expected["uri"]);
    }
}

/// Rebuild a pack after editing its manifest and section JSON. Part keys stay
/// as written; each output digest is recomputed from the edited payload, so
/// the container verifies and only the semantic checks can refuse it.
fn forge(
    bytes: &[u8],
    edit: impl FnOnce(&mut PackManifest, &mut BTreeMap<String, Value>),
) -> Result<Vec<u8>, String> {
    let pack = LanguagePack::read(bytes.to_vec()).map_err(|e| e.to_string())?;
    let mut manifest = pack.manifest().clone();
    let mut sections = BTreeMap::new();
    for section in &manifest.sections {
        let value: Value = pack
            .section_json(&section.name)
            .map_err(|e| e.to_string())?;
        sections.insert(section.name.clone(), value);
    }
    edit(&mut manifest, &mut sections);
    for part in &mut manifest.parts {
        let value = sections
            .get(&part.name)
            .ok_or("forged part has no section")?;
        part.output_digest = canonical_digest(value).map_err(|e| e.to_string())?.sha256;
    }
    let mut builder = PackBuilder::new(PackHeader {
        language: manifest.language.clone(),
        recipe: manifest.recipe.clone(),
        compiler: manifest.compiler.clone(),
        recipe_constants: manifest.recipe_constants.clone(),
        inputs: manifest.inputs.clone(),
        parts: manifest.parts.clone(),
        requires_hooks: manifest.requires_hooks.clone(),
        depends_on: manifest.depends_on.clone(),
    });
    for section in &manifest.sections {
        let part = manifest
            .parts
            .iter()
            .find(|part| part.name == section.name)
            .ok_or("forged section has no part")?;
        let value = sections.get(&section.name).ok_or("missing section")?;
        builder = builder.json_section(
            section.name.clone(),
            section.kind.clone(),
            section.flags,
            value,
            part.input_digest.clone(),
            part.output_digest.clone(),
        );
    }
    builder.build().map_err(|e| e.to_string())
}

/// Apply `edit` to every object (at any depth) for which `select` holds.
fn edit_objects(
    value: &mut Value,
    select: &dyn Fn(&serde_json::Map<String, Value>) -> bool,
    edit: &mut dyn FnMut(&mut serde_json::Map<String, Value>),
) -> usize {
    let mut count = 0;
    match value {
        Value::Object(map) => {
            if select(map) {
                edit(map);
                count += 1;
            }
            for item in map.values_mut() {
                count += edit_objects(item, select, edit);
            }
        }
        Value::Array(items) => {
            for item in items {
                count += edit_objects(item, select, edit);
            }
        }
        _ => {}
    }
    count
}

/// Like `refused`, for the `rcore/bindings` part. Whether the program parts
/// of a forgery still load is asserted by the caller (`programs_load`).
fn bindings_refused(label: &str, bytes: Vec<u8>) -> Result<String, String> {
    let pack = LanguagePack::read(bytes).map_err(|e| format!("{label}: container refused: {e}"))?;
    match language_pack::load_pack_bindings(&pack) {
        Ok(_) => Err(format!("{label}: forged bindings were accepted")),
        Err(error) => {
            println!(
                "refused {label}: {}",
                error.chars().take(240).collect::<String>()
            );
            Ok(error)
        }
    }
}

/// Edit the `save_bindings` envelope inside the `rcore/bindings` section.
fn forge_saved(bytes: &[u8], edit: impl FnOnce(&mut Value)) -> Result<Vec<u8>, String> {
    forge_saved_in(language_pack::BINDINGS_SECTION, bytes, edit)
}

/// Edit the `save_bindings` envelope inside binding section `section`.
fn forge_saved_in(
    section: &str,
    bytes: &[u8],
    edit: impl FnOnce(&mut Value),
) -> Result<Vec<u8>, String> {
    let mut failure = None;
    let forged = forge(bytes, |_, sections| {
        // The saved text is nested JSON in its part, not a string.
        match sections
            .get_mut(section)
            .and_then(|part| part.get_mut("saved"))
        {
            Some(envelope @ Value::Object(_)) => edit(envelope),
            _ => failure = Some("no nested saved bindings".to_owned()),
        }
    })?;
    match failure {
        Some(failure) => Err(failure),
        None => Ok(forged),
    }
}

/// The program parts of `bytes` still load (`HostCache`): a bindings refusal
/// of this pack is bindings-specific.
fn programs_load(label: &str, bytes: &[u8]) -> Result<(), String> {
    let pack = LanguagePack::read(bytes.to_vec()).map_err(|e| e.to_string())?;
    NativePilotSession::from_pack(&pack, RuntimeAdmissionLoadMode::HostCache)
        .map(drop)
        .map_err(|error| format!("{label}: programs refused too: {error}"))
}

/// A labelled edit of one manifest part.
type PartEdit = (&'static str, fn(&mut PackPart));

/// The `rcore/bindings` part (manifest entry) of `manifest`.
fn bindings_part(manifest: &mut PackManifest) -> Option<&mut PackPart> {
    manifest
        .parts
        .iter_mut()
        .find(|part| part.name == language_pack::BINDINGS_SECTION)
}

fn bindings_negative_controls(bytes: &[u8]) -> Result<(), String> {
    // Every forgery below keeps the manifest's inputs, compiler and constants;
    // only the last one (labelled) edits the manifest-wide pins.
    // The saved format is tampered, not the saved digest: a pack load does
    // not recompute that digest, because the section digest the pack verified
    // (and `forge` recomputes) already covers the same bytes.
    let forged = forge_saved(bytes, |envelope| envelope["format"] = json!(0))?;
    programs_load("bindings format tampered", &forged)?;
    let error = bindings_refused("bindings format tampered", forged)?;
    assert!(
        error.contains("rcore/bindings load failed") && error.contains("another format"),
        "{error}"
    );

    // One saved binding removed and the envelope digest recomputed: format,
    // identity and digest are consistent, so only the recipe-results check
    // refuses it.
    let mut removed = 0;
    let forged = forge_saved(bytes, |envelope| {
        use sha2::{Digest, Sha256};
        if let Value::Array(mut scopes) = envelope["scopes"].take()
            && scopes.pop().is_some()
            && let Ok(text) = serde_json::to_string(&scopes)
        {
            envelope["digest"] = json!(format!("{:x}", Sha256::digest(text.as_bytes())));
            envelope["scopes"] = Value::Array(scopes);
            removed += 1;
        }
    })?;
    assert_eq!(removed, 1);
    programs_load("bindings truncated", &forged)?;
    let error = bindings_refused("bindings truncated", forged)?;
    assert!(
        error.contains("rcore/bindings does not hold the recipe's bindings")
            && error.contains("the counts differ"),
        "{error}"
    );

    let error = bindings_refused(
        "bindings compiler identity",
        forge_saved(bytes, |envelope| {
            envelope["compiler"] = json!(format!(
                "intersect.kerml.bindings.v2+{}",
                intersect_model::SOURCE_IDENTITY
            ));
        })?,
    )?;
    assert!(
        error.contains("another binding compiler identity"),
        "{error}"
    );

    let mut edited = 0;
    let error = bindings_refused(
        "bindings bundle source changed",
        forge(bytes, |_, sections| {
            if let Some(Value::Array(sources)) = sections
                .get_mut(language_pack::BINDINGS_SECTION)
                .and_then(|part| part.pointer_mut("/bundle/sources"))
                && let Some(Value::String(text)) = sources
                    .iter_mut()
                    .find(|source| source["id"]["uri"] == json!(kerml_bindings::CONSTRUCTION_URI))
                    .and_then(|source| source.get_mut("text"))
            {
                text.push_str("\n// forged\n");
                edited += 1;
            }
        })?,
    )?;
    assert_eq!(edited, 1);
    assert!(
        error.contains("bundle differs from the recipe's pinned sources"),
        "{error}"
    );

    // The bindings part key is recomputed: edits of only the `rcore/bindings`
    // manifest part (its key, its input roles, its components) are refused by
    // the bindings part-key check; the program parts still load.
    let part_edits: [PartEdit; 3] = [
        ("bindings part input digest", |part| {
            part.input_digest = "0".repeat(64)
        }),
        ("bindings part inputs", |part| {
            part.inputs.retain(|input| input != "kerml-construction")
        }),
        ("bindings part components", |part| {
            part.components
                .retain(|component| component != "intersect-model")
        }),
    ];
    for (label, edit) in part_edits {
        let mut edited = 0;
        let forged = forge(bytes, |manifest, _| {
            if let Some(part) = bindings_part(manifest) {
                edit(part);
                edited += 1;
            }
        })?;
        assert_eq!(edited, 1, "{label}");
        programs_load(label, &forged)?;
        let error = bindings_refused(label, forged)?;
        assert!(
            error.contains("rcore/bindings part key differs from this recipe"),
            "{label}: {error}"
        );
    }

    // The bindings section kind is the recipe's.
    let mut edited = 0;
    let forged = forge(bytes, |manifest, _| {
        if let Some(section) = manifest
            .sections
            .iter_mut()
            .find(|section| section.name == language_pack::BINDINGS_SECTION)
        {
            section.kind = language_pack::PROGRAM_KIND.into();
            edited += 1;
        }
    })?;
    assert_eq!(edited, 1);
    programs_load("bindings section kind", &forged)?;
    let error = bindings_refused("bindings section kind", forged)?;
    assert!(
        error.contains("rcore/bindings section kind differs from this recipe"),
        "{error}"
    );

    // Manifest-wide pin check (not bindings-specific): a pinned binding input
    // edited in the manifest is refused by `check_pack`'s inputs equality,
    // for the programs as well.
    let forged = forge(bytes, |manifest, _| {
        if let Some(id) = manifest.inputs.get_mut("kerml-construction") {
            id.sha256 = "0".repeat(64);
        }
    })?;
    assert!(programs_load("manifest binding input pin", &forged).is_err());
    let error = bindings_refused("manifest binding input pin", forged)?;
    assert!(error.contains("inputs differs from this recipe"), "{error}");

    // The SysML bindings part runs the same checks on its own section: a
    // tampered format there is refused by `load_pack_sysml_bindings` only.
    let forged = forge_saved_in(language_pack::SYSML_BINDINGS_SECTION, bytes, |envelope| {
        envelope["format"] = json!(0)
    })?;
    programs_load("SysML bindings format tampered", &forged)?;
    let pack = LanguagePack::read(forged).map_err(|e| e.to_string())?;
    language_pack::load_pack_bindings(&pack)?;
    let error = language_pack::load_pack_sysml_bindings(&pack)
        .err()
        .ok_or("forged SysML bindings were accepted")?;
    println!("refused SysML bindings format tampered: {error}");
    assert!(
        error.contains("rcore/sysml-bindings load failed") && error.contains("another format"),
        "{error}"
    );
    // The KerML part's saved text under the SysML part is another identity.
    let mut kerml_text = None;
    let forged = forge(bytes, |_, sections| {
        kerml_text = sections
            .get(language_pack::BINDINGS_SECTION)
            .and_then(|part| part.get("saved"))
            .cloned();
        if let (Some(text), Some(part)) = (
            &kerml_text,
            sections.get_mut(language_pack::SYSML_BINDINGS_SECTION),
        ) {
            part["saved"] = text.clone();
        }
    })?;
    assert!(kerml_text.is_some());
    let pack = LanguagePack::read(forged).map_err(|e| e.to_string())?;
    let error = language_pack::load_pack_sysml_bindings(&pack)
        .err()
        .ok_or("KerML bindings under rcore/sysml-bindings were accepted")?;
    assert!(
        error.contains("another binding compiler identity"),
        "{error}"
    );
    Ok(())
}

/// `RcoreBindingsPart::new` and `pack_session` refuse bindings that are not
/// the recipe jobs' results: missing, reordered across kinds, reordered
/// within a kind, or an empty deserialized part.
fn bindings_part_controls(
    session: &NativePilotSession,
    scopes: &[VerifiedScope],
) -> Result<(), String> {
    let bundle = kerml_bindings::bundle();
    let jobs = kerml_bindings::binding_jobs(&bundle)?;
    let first = jobs[0].declarations.len();
    assert!(first >= 2 && scopes.len() > first);
    let refuse = |label: &str, scopes: Vec<VerifiedScope>, want: &str| -> Result<(), String> {
        match RcoreBindingsPart::new(&scopes) {
            Ok(_) => Err(format!("{label}: bindings part was built")),
            Err(error) => {
                println!("refused {label}: {error}");
                assert!(
                    error.contains("not the recipe jobs' results") && error.contains(want),
                    "{label}: {error}"
                );
                Ok(())
            }
        }
    };
    let mut missing = scopes.to_vec();
    missing.pop();
    refuse("binding missing", missing, "the counts differ")?;
    let mut reversed = scopes.to_vec();
    reversed.reverse();
    refuse("bindings reversed", reversed, "is a")?;
    // Same count; the last scope and the first default change places.
    let mut kinds = scopes.to_vec();
    kinds.swap(first - 1, first);
    refuse("kinds swapped", kinds, "is a")?;
    // Same count and kinds; two scopes change places.
    let mut within = scopes.to_vec();
    within.swap(0, 1);
    refuse("scopes swapped", within, "another declaration")?;

    let empty: RcoreBindingsPart = serde_json::from_value(json!({
        "bundle": to_json(&bundle)?,
        // Nested JSON, as a part stores it.
        "saved": serde_json::from_str::<Value>(&kerml_bindings::save(&[])?).map_err(|e| e.to_string())?,
    }))
    .map_err(|e| e.to_string())?;
    let empty_sysml: SysmlBindingsPart = serde_json::from_value(json!({
        "bundle": to_json(&sysml_bindings::bundle())?,
        "saved": serde_json::from_str::<Value>(&sysml_bindings::save(&[])?).map_err(|e| e.to_string())?,
    }))
    .map_err(|e| e.to_string())?;
    let error = language_pack::pack_session(session, &empty, &empty_sysml)
        .err()
        .ok_or("pack_session encoded an empty bindings part")?;
    assert!(
        error.contains("does not hold the recipe's bindings"),
        "{error}"
    );
    Ok(())
}

fn refused(label: &str, bytes: Vec<u8>, mode: RuntimeAdmissionLoadMode) -> Result<String, String> {
    let pack = LanguagePack::read(bytes).map_err(|e| format!("{label}: container refused: {e}"))?;
    match NativePilotSession::from_pack(&pack, mode) {
        Ok(_) => Err(format!("{label}: forged pack was accepted")),
        Err(error) => {
            println!(
                "refused {label}: {}",
                error.chars().take(240).collect::<String>()
            );
            Ok(error)
        }
    }
}

const KERML_DECLARATIONS: i64 = 284;
const SYSML_DECLARATIONS: i64 = 551;

/// Every copy of a role's requested source policy inside `section` (the
/// policy objects carry `structure_bytes` and its `xtext_declarations`).
fn is_policy(map: &serde_json::Map<String, Value>, declarations: i64) -> bool {
    map.contains_key("structure_bytes")
        && map.get("xtext_declarations") == Some(&json!(declarations))
}

/// The KerML program with every copy of its requested policy shrunk by one
/// declaration: the container, manifest and part keys stay valid.
fn shrunk_policy(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut policies = 0;
    let forged = forge(bytes, |_, sections| {
        if let Some(kerml) = sections.get_mut(ProgramRole::KerML.section()) {
            policies = edit_objects(
                kerml,
                &|map| is_policy(map, KERML_DECLARATIONS),
                &mut |map| {
                    map.insert("xtext_declarations".into(), json!(KERML_DECLARATIONS - 1));
                },
            );
        }
    })?;
    assert!(policies >= 2, "{policies}");
    Ok(forged)
}

pub(crate) fn negative_controls(bytes: &[u8]) -> Result<(), String> {
    use RuntimeAdmissionLoadMode::HostCache;
    // The forging itself is faithful: an unedited rebuild is byte-identical.
    assert_eq!(forge(bytes, |_, _| {})?, bytes);

    let error = refused(
        "compiler digest",
        forge(bytes, |manifest, _| {
            if let Some(identity) = manifest.compiler.get_mut("intersect-xtext") {
                *identity = "0".repeat(64);
            }
        })?,
        HostCache,
    )?;
    assert!(error.contains("compiler identity"), "{error}");

    let error = refused(
        "unknown hook",
        forge(bytes, |manifest, _| {
            manifest.requires_hooks.push(PackHook {
                provider: ProviderId {
                    name: "intersect.kerml.pilot.unknown".into(),
                    version: "1".into(),
                },
                provenance: "intersect-kerml".into(),
            });
        })?,
        HostCache,
    )?;
    assert!(error.contains("unknown hook"), "{error}");

    let error = refused(
        "missing hook",
        forge(bytes, |manifest, _| {
            manifest
                .requires_hooks
                .retain(|hook| hook.provider.name != intersect_kerml::OPERAND_PROVIDER);
        })?,
        HostCache,
    )?;
    assert!(
        error.contains("does not declare the setting provider"),
        "{error}"
    );

    // The admission and the prediction program agree on a smaller depth;
    // only the recipe expectation can refuse it.
    let mut depths = 0;
    let error = refused(
        "coherent call depth 128",
        forge(bytes, |_, sections| {
            if let Some(kerml) = sections.get_mut(ProgramRole::KerML.section()) {
                depths = edit_objects(
                    kerml,
                    &|map| map.get("call_depth") == Some(&json!(512)),
                    &mut |map| {
                        map.insert("call_depth".into(), json!(128));
                    },
                );
            }
        })?,
        HostCache,
    )?;
    assert!(depths > 0);
    assert!(error.contains("call depth"), "{error}");

    // Both copies of the requested policy shrink, so only the recipe
    // equality (not the restored-versus-retained check) can refuse it.
    let error = refused("requested policy shrunk", shrunk_policy(bytes)?, HostCache)?;
    assert!(
        error.contains("requested source policy differs from its recipe"),
        "{error}"
    );

    for (label, key, replacement) in [
        (
            "selection namespace",
            "namespace",
            json!("https://www.omg.org/spec/SysML/forged"),
        ),
        (
            "selection ecore",
            "ecore",
            json!({"uri": "org.omg.sysml/model/Forged.ecore.rcore", "sha256": "0".repeat(64)}),
        ),
    ] {
        let mut selections = 0;
        let mut model_selections = 0;
        let error = refused(
            label,
            forge(bytes, |_, sections| {
                if let Some(kerml) = sections.get_mut(ProgramRole::KerML.section()) {
                    selections = edit_objects(
                        kerml,
                        &|map| map.contains_key("rcore_behavior") && map.contains_key(key),
                        &mut |map| {
                            map.insert(key.into(), replacement.clone());
                        },
                    );
                    if key == "namespace" {
                        // Keep the new checked model owner coherent with the
                        // text selection so this still reaches the B7 recipe check.
                        model_selections = edit_objects(
                            kerml,
                            &|map| {
                                map.contains_key("metadata")
                                    && map.contains_key("entry")
                                    && map.contains_key("materialized_types")
                                    && map.contains_key("namespace")
                            },
                            &mut |map| {
                                map.insert("namespace".into(), replacement.clone());
                            },
                        );
                    }
                }
            })?,
            HostCache,
        )?;
        assert!(selections > 0, "{label}");
        let expected = if key == "namespace" {
            assert_eq!(
                model_selections, 1,
                "one checked model owner must be forged coherently"
            );
            "KerML converted selection differs from the recipe's remapped selection"
        } else {
            // A foreign structural selection fails the checked owner tie before
            // the language recipe can reinterpret the retained cache.
            "Saved draft selection differs from its model owner"
        };
        assert!(error.contains(expected), "{label}: {error}");
    }

    let mut dropped = 0;
    let error = refused(
        "contract dropped",
        forge(bytes, |_, sections| {
            if let Some(sysml) = sections.get_mut(ProgramRole::SysML.section()) {
                dropped = edit_objects(
                    sysml,
                    &|map| map.get("computed_settings").is_some_and(Value::is_array),
                    &mut |map| {
                        if let Some(Value::Array(contracts)) = map.get_mut("computed_settings") {
                            contracts.pop();
                        }
                    },
                );
            }
        })?,
        HostCache,
    )?;
    assert!(dropped > 0);
    // Refused by the contract re-derivation (A3/B9), not earlier.
    assert!(
        error.contains("SysML computed-setting contracts differ from the recipe derivation"),
        "{error}"
    );

    for edit_role in [false, true] {
        let label = if edit_role {
            "SysML payload under program/kerml, role edited"
        } else {
            "SysML payload under program/kerml"
        };
        let error = refused(
            label,
            forge(bytes, |_, sections| {
                if let Some(mut sysml) = sections.get(ProgramRole::SysML.section()).cloned() {
                    if edit_role {
                        sysml["role"] = json!("kerml");
                    }
                    sections.insert(ProgramRole::KerML.section().into(), sysml);
                }
            })?,
            HostCache,
        )?;
        if edit_role {
            // The SysML request (551 declarations) exceeds the KerML host
            // policy, so host admission refuses it before any recipe check.
            assert!(
                error.contains("program/kerml load failed")
                    && error.contains("Requested source policy exceeds host admission limits"),
                "{error}"
            );
        } else {
            assert!(error.contains("holds the SysML program"), "{error}");
        }
    }

    // The reverse swap fits the larger SysML host policy, so only the recipe
    // checks can refuse it: first the requested-policy equality, and with the
    // requested policy also made the SysML one, the original selection
    // rebuilt from the restored KerML originals for the SysML entry.
    for edit_policy in [false, true] {
        let label = if edit_policy {
            "KerML payload under program/sysml, role and policy edited"
        } else {
            "KerML payload under program/sysml, role edited"
        };
        let mut policies = 0;
        let error = refused(
            label,
            forge(bytes, |_, sections| {
                let sysml_policy =
                    sections
                        .get_mut(ProgramRole::SysML.section())
                        .and_then(|sysml| {
                            let mut found = None;
                            edit_objects(
                                sysml,
                                &|map| is_policy(map, SYSML_DECLARATIONS),
                                &mut |map| {
                                    found.get_or_insert_with(|| map.clone());
                                },
                            );
                            found
                        });
                if let Some(mut kerml) = sections.get(ProgramRole::KerML.section()).cloned() {
                    kerml["role"] = json!("sysml");
                    if edit_policy && let Some(sysml_policy) = &sysml_policy {
                        policies = edit_objects(
                            &mut kerml,
                            &|map| is_policy(map, KERML_DECLARATIONS),
                            &mut |map| *map = sysml_policy.clone(),
                        );
                    }
                    sections.insert(ProgramRole::SysML.section().into(), kerml);
                }
            })?,
            HostCache,
        )?;
        if edit_policy {
            // Host admission and the requested-policy equality pass; the
            // original-selection check (B7) rebuilds the SysML entry from the
            // restored originals, which hold only the KerML grammar.
            assert!(policies >= 2, "{policies}");
            assert!(
                error.contains("Missing original definition: org.omg.sysml.xtext/"),
                "{error}"
            );
        } else {
            assert!(
                error.contains("program/sysml load failed")
                    && error.contains("requested source policy differs from its recipe"),
                "{error}"
            );
        }
    }
    Ok(())
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn pack_loaded_sessions_equal_a_fresh_preparation() -> Result<(), String> {
    std::thread::Builder::new()
        .name("language-pack-equivalence".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(equivalence)
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_| "Language pack equivalence panicked".to_owned())?
}

fn equivalence() -> Result<(), String> {
    let definitions = DEFINITIONS
        .iter()
        .map(|path| read(path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut kerml_corpus = inline(&KERML_TEXTS);
    for path in KERML_LIBRARIES {
        kerml_corpus.push(read(path)?);
    }
    let mut sysml_corpus = inline(&SYSML_TEXTS);
    for path in SYSML_LIBRARIES {
        sysml_corpus.push(read(path)?);
    }
    let preparation = InvocationContext::new(Rc::new(Observer));

    let clock = Instant::now();
    let fresh = NativePilotSession::prepare(&definitions, &preparation)?;
    println!("fresh Pilot prepare: {:.0} ms", ms(clock));
    let clock = Instant::now();
    let fresh_kerml = NativeKerMLSession::prepare(&definitions[..3], &preparation)?;
    println!("fresh KerML-only prepare: {:.0} ms", ms(clock));

    let clock = Instant::now();
    let requests = [
        BindingCompile::new(BindingRecipe::KerML)?,
        BindingCompile::new(BindingRecipe::SysML)?,
    ];
    let (scopes, sysml_compiled) =
        match <[_; 2]>::try_from(sysml_scopes::compile_requests(&requests)?) {
            Ok([kerml, sysml]) => (kerml, sysml),
            Err(results) => return Err(format!("{} binding results", results.len())),
        };
    let bundle = kerml_bindings::bundle();
    println!(
        "fresh KerML and SysML bindings compile: {:.0} ms, {} + {} bindings",
        ms(clock),
        scopes.len(),
        sysml_compiled.len()
    );
    assert!(!scopes.is_empty());
    assert!(sysml_compiled.len() > scopes.len());
    let bindings = RcoreBindingsPart::new(&scopes)?;
    assert_eq!(*bindings.bundle(), bundle);
    let sysml_part = SysmlBindingsPart::new(&sysml_compiled)?;
    assert_eq!(*sysml_part.bundle(), sysml_bindings::bundle());
    bindings_part_controls(&fresh, &scopes)?;

    // Reference behaviour of the fresh bindings on one SysML document: the
    // link asks the scopes, and at least one reference resolves uniquely.
    let link_source = inline(&SYSML_TEXTS)
        .into_iter()
        .find(|source| source.uri == "control/parts.sysml")
        .ok_or("no control/parts.sysml")?;
    let through_fresh = link_resolutions(&fresh, &link_source, &bundle, &scopes)?;
    println!(
        "link through fresh bindings: {} resolutions, linked {}",
        through_fresh.0.len(),
        through_fresh.1
    );
    assert!(
        through_fresh
            .0
            .iter()
            .any(|(.., found)| found.as_ref().is_ok_and(|found| found.len() == 1)),
        "{:?}",
        through_fresh.0
    );

    let clock = Instant::now();
    let bytes = language_pack::pack_session(&fresh, &bindings, &sysml_part)?;
    println!("pack_session: {:.0} ms, {} bytes", ms(clock), bytes.len());
    assert_eq!(
        language_pack::pack_session(&fresh, &bindings, &sysml_part)?,
        bytes,
        "encoding is deterministic"
    );

    let expected = observe(&fresh, &kerml_corpus, &sysml_corpus)?;
    // Real controls: the corpus constructs models, references and decisions,
    // and the broken document reports diagnostics.
    for row in expected.kerml.iter().chain(&expected.sysml) {
        if row["uri"] == "control/broken.kerml" {
            assert!(
                row.get("failure").is_some()
                    || row["evaluation"]["diagnostics"]
                        .as_array()
                        .is_some_and(|d| !d.is_empty()),
                "{row:#}"
            );
        } else {
            assert!(
                row["evaluation"]["diagnostics"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
                    && row["model"].is_object(),
                "{}: {:#}",
                row["uri"],
                row.get("failure")
                    .unwrap_or(&row["evaluation"]["diagnostics"])
            );
        }
    }
    // Runtime decisions are consumed somewhere in each corpus.
    for rows in [&expected.kerml, &expected.sysml] {
        assert!(
            rows.iter()
                .any(|row| row["consumed"].as_u64().is_some_and(|n| n > 0))
        );
    }
    let fresh_kerml_rows = fresh_kerml.with_definition(|definition, contracts| {
        kerml_rows(definition, contracts, &kerml_corpus)
    })?;
    assert_eq!(fresh_kerml_rows, expected.kerml);

    for mode in [
        RuntimeAdmissionLoadMode::HostCache,
        RuntimeAdmissionLoadMode::Revalidate,
    ] {
        let clock = Instant::now();
        let pack = LanguagePack::read(bytes.clone()).map_err(|e| e.to_string())?;
        println!("{mode:?}: LanguagePack::read {:.0} ms", ms(clock));
        let clock = Instant::now();
        let loaded = NativePilotSession::from_pack(&pack, mode)?;
        println!("{mode:?}: Pilot from_pack {:.0} ms", ms(clock));
        let clock = Instant::now();
        let loaded_scopes = language_pack::load_pack_bindings(&pack)?;
        println!("{mode:?}: load_pack_bindings {:.0} ms", ms(clock));
        assert_eq!(
            kerml_bindings::save(&loaded_scopes)?,
            bindings.saved(),
            "{mode:?}: loaded bindings"
        );
        for (loaded, fresh) in loaded_scopes.iter().zip(&scopes) {
            assert_eq!(loaded.kind(), fresh.kind());
            assert_eq!(to_json(loaded.binding())?, to_json(fresh.binding())?);
        }
        let clock = Instant::now();
        let (loaded_bundle, loaded_sysml) = language_pack::load_pack_sysml_bindings(&pack)?;
        println!("{mode:?}: load_pack_sysml_bindings {:.0} ms", ms(clock));
        assert_eq!(loaded_bundle, sysml_bindings::bundle());
        assert_eq!(loaded_sysml.len(), sysml_compiled.len());
        assert_eq!(
            sysml_bindings::save(&loaded_sysml)?,
            sysml_part.saved(),
            "{mode:?}: loaded SysML bindings"
        );
        // The loaded bindings execute as the fresh ones: the same document
        // links to the same resolutions through either.
        let clock = Instant::now();
        let through_loaded = link_resolutions(&loaded, &link_source, &bundle, &loaded_scopes)?;
        println!("{mode:?}: link through loaded bindings {:.0} ms", ms(clock));
        assert_eq!(
            through_loaded, through_fresh,
            "{mode:?}: linked resolutions"
        );
        let loaded_bindings = RcoreBindingsPart::new(&loaded_scopes)?;
        let loaded_sysml_bindings = SysmlBindingsPart::new(&loaded_sysml)?;
        // Fixed point: the loaded session and bindings pack to the same bytes.
        assert_eq!(
            language_pack::pack_session(&loaded, &loaded_bindings, &loaded_sysml_bindings)?,
            bytes,
            "{mode:?}: re-pack"
        );
        let actual = observe(&loaded, &kerml_corpus, &sysml_corpus)?;
        assert_same(&format!("{mode:?}"), &actual, &expected);

        let clock = Instant::now();
        let kerml_only = NativeKerMLSession::from_pack(&pack, mode)?;
        println!("{mode:?}: KerML-only from_pack {:.0} ms", ms(clock));
        let rows = kerml_only.with_definition(|definition, contracts| {
            kerml_rows(definition, contracts, &kerml_corpus)
        })?;
        assert_eq!(rows, expected.kerml, "{mode:?}: KerML-only");
    }

    negative_controls(&bytes)?;
    bindings_negative_controls(&bytes)?;
    // Revalidate refuses what HostCache refuses (spot check).
    let error = refused(
        "Revalidate: SysML payload under program/kerml",
        forge(&bytes, |_, sections| {
            if let Some(sysml) = sections.get(ProgramRole::SysML.section()).cloned() {
                sections.insert(ProgramRole::KerML.section().into(), sysml);
            }
        })?,
        RuntimeAdmissionLoadMode::Revalidate,
    )?;
    assert!(error.contains("holds the SysML program"), "{error}");

    incremental_builds(&definitions, &preparation, &bytes)
}

const REUSED_KERML: &str = "pack_part_reused:program/kerml";
const REUSED_SYSML: &str = "pack_part_reused:program/sysml";
const REUSED_BINDINGS: &str = "pack_part_reused:rcore/bindings";
const REUSED_SYSML_BINDINGS: &str = "pack_part_reused:rcore/sysml-bindings";
const REBUILT_KERML: &str = "pack_part_rebuilt:program/kerml";
const REBUILT_SYSML: &str = "pack_part_rebuilt:program/sysml";
const REBUILT_BINDINGS: &str = "pack_part_rebuilt:rcore/bindings";
const REBUILT_SYSML_BINDINGS: &str = "pack_part_rebuilt:rcore/sysml-bindings";

/// The result list of a binding compile.
type Compiled = Result<Vec<Vec<VerifiedScope>>, String>;

/// A build's result and its observer markers.
type Built = (Result<Vec<u8>, String>, Vec<&'static str>);

/// The binding compile of every build here except the corrupt-bindings one:
/// no test here compiles the bindings a second time.
fn sentinel(_: &[BindingCompile]) -> Compiled {
    Err("sentinel: bindings compile reached".into())
}

/// `build_language_pack` over `previous` with the binding compile `compile`:
/// the result and the observer markers.
fn build_over(
    label: &str,
    definitions: &[Source],
    preparation: &InvocationContext<'_>,
    previous: &[u8],
    compile: impl FnOnce(&[BindingCompile]) -> Compiled,
) -> Result<Built, String> {
    let previous = LanguagePack::read(previous.to_vec()).map_err(|e| e.to_string())?;
    let markers = std::cell::RefCell::new(Vec::new());
    let clock = Instant::now();
    let built = language_pack::build_language_pack(
        definitions,
        Some(&previous),
        preparation,
        compile,
        |marker| markers.borrow_mut().push(marker),
    );
    println!("build_language_pack, {label}: {:.0} ms", ms(clock));
    let markers = markers.into_inner();
    println!("  markers: {markers:?}");
    Ok((built, markers))
}

/// The `explain_previous` decisions of `previous`, by part.
fn decisions(previous: &[u8]) -> Result<Vec<(String, PartDecision, Option<String>)>, String> {
    let previous = LanguagePack::read(previous.to_vec()).map_err(|e| e.to_string())?;
    language_pack::explain_previous(&previous)
}

/// Step 8: per-part reuse. A rebuilt program is prepared again from the
/// definitions. A rebuilt bindings part goes through the whole compile path
/// of `build_language_pack` (`RcoreBindingsPart::new`, encode, self-check)
/// with a compile closure that returns the full pack's verified scopes
/// instead of compiling again, so the binding compile runs once per test,
/// not twice; every other build stops at the sentinel compile. Each
/// incremental build must be byte-identical to the full build.
fn incremental_builds(
    definitions: &[Source],
    preparation: &InvocationContext<'_>,
    bytes: &[u8],
) -> Result<(), String> {
    use PartDecision::{Rebuild, Reuse};
    // A matching previous pack: every part reused, re-encoded and
    // self-checked, the bytes unchanged, nothing prepared or compiled.
    let (built, markers) = build_over(
        "matching previous",
        definitions,
        preparation,
        bytes,
        sentinel,
    )?;
    assert_eq!(built?, bytes);
    assert_eq!(
        markers,
        [
            REUSED_KERML,
            REUSED_SYSML,
            REUSED_BINDINGS,
            REUSED_SYSML_BINDINGS,
            "pack_encode_begin",
            "pack_self_check_begin",
            "pack_self_check_end",
            "pack_reused"
        ]
    );

    // Programs load but the bindings do not: only the bindings are rebuilt,
    // nothing is prepared. The compile closure hands back the full pack's
    // scopes (loaded, not compiled), checked to be asked for this recipe.
    let corrupt = forge_saved(bytes, |envelope| envelope["format"] = json!(0))?;
    programs_load("corrupt bindings", &corrupt)?;
    let explained = decisions(&corrupt)?;
    assert_eq!(explained[0].1, Reuse);
    assert_eq!(explained[1].1, Reuse);
    assert_eq!(explained[2].1, Rebuild(RebuildReason::LoadRefused));
    assert_eq!(explained[3].1, Reuse);
    assert!(
        explained[2]
            .2
            .as_ref()
            .is_some_and(|error| error.contains("another format")),
        "{explained:?}"
    );
    let full = LanguagePack::read(bytes.to_vec()).map_err(|e| e.to_string())?;
    let full_scopes = language_pack::load_pack_bindings(&full)?;
    let (built, markers) = build_over(
        "corrupt bindings",
        definitions,
        preparation,
        &corrupt,
        |requests| {
            if requests != [BindingCompile::new(BindingRecipe::KerML)?] {
                return Err("compile closure called with other requests".into());
            }
            Ok(vec![full_scopes])
        },
    )?;
    assert_eq!(built?, bytes, "bindings-only incremental equals full");
    assert_eq!(
        markers,
        [
            REUSED_KERML,
            REUSED_SYSML,
            REBUILT_BINDINGS,
            REUSED_SYSML_BINDINGS,
            "bindings_compile_begin",
            "bindings_compile_end",
            "pack_encode_begin",
            "pack_self_check_begin",
            "pack_self_check_end"
        ]
    );

    // The same for the SysML bindings part alone.
    let corrupt = forge_saved_in(language_pack::SYSML_BINDINGS_SECTION, bytes, |envelope| {
        envelope["format"] = json!(0)
    })?;
    let explained = decisions(&corrupt)?;
    assert_eq!(
        explained
            .iter()
            .map(|(_, decision, _)| *decision)
            .collect::<Vec<_>>(),
        [Reuse, Reuse, Reuse, Rebuild(RebuildReason::LoadRefused)]
    );
    let (_, full_sysml_scopes) = language_pack::load_pack_sysml_bindings(&full)?;
    drop(full);
    let (built, markers) = build_over(
        "corrupt SysML bindings",
        definitions,
        preparation,
        &corrupt,
        |requests| {
            if requests != [BindingCompile::new(BindingRecipe::SysML)?] {
                return Err("compile closure called with other requests".into());
            }
            Ok(vec![full_sysml_scopes])
        },
    )?;
    assert_eq!(built?, bytes, "SysML-bindings-only incremental equals full");
    assert_eq!(
        markers[..6],
        [
            REUSED_KERML,
            REUSED_SYSML,
            REUSED_BINDINGS,
            REBUILT_SYSML_BINDINGS,
            "bindings_compile_begin",
            "bindings_compile_end",
        ]
    );

    // A KerML program that passes the container, manifest and part-key checks
    // but fails `from_pack`: only program/kerml is prepared again, the SysML
    // program and the bindings are reused, and the pack equals the full one.
    let stale = shrunk_policy(bytes)?;
    language_pack::check_pack(
        &LanguagePack::read(stale.clone()).map_err(|e| e.to_string())?,
        &[ProgramRole::KerML, ProgramRole::SysML],
    )?;
    let explained = decisions(&stale)?;
    assert_eq!(explained[0].1, Rebuild(RebuildReason::LoadRefused));
    assert_eq!(
        (explained[1].1, explained[2].1, explained[3].1),
        (Reuse, Reuse, Reuse)
    );
    let (built, markers) = build_over(
        "stale program/kerml",
        definitions,
        preparation,
        &stale,
        sentinel,
    )?;
    assert_eq!(built?, bytes, "incremental equals full");
    assert_eq!(
        markers[..4],
        [
            REBUILT_KERML,
            REUSED_SYSML,
            REUSED_BINDINGS,
            REUSED_SYSML_BINDINGS
        ]
    );
    assert!(markers.contains(&"kerml_owner_end"), "{markers:?}");
    assert!(!markers.contains(&"sysml_owner_begin"), "{markers:?}");
    assert!(!markers.contains(&"bindings_compile_begin"), "{markers:?}");
    assert_eq!(markers.last(), Some(&"pack_self_check_end"));

    // A previous program/sysml part key edited (as an input change of the
    // SysML grammar would): only program/sysml is prepared again.
    let mut edited = 0;
    let rekeyed = forge(bytes, |manifest, _| {
        if let Some(part) = manifest
            .parts
            .iter_mut()
            .find(|part| part.name == ProgramRole::SysML.section())
        {
            part.input_digest = "0".repeat(64);
            edited += 1;
        }
    })?;
    assert_eq!(edited, 1);
    let explained = decisions(&rekeyed)?;
    assert_eq!(
        explained
            .iter()
            .map(|(_, decision, _)| *decision)
            .collect::<Vec<_>>(),
        [Reuse, Rebuild(RebuildReason::KeyChanged), Reuse, Reuse]
    );
    let (built, markers) = build_over(
        "program/sysml key changed",
        definitions,
        preparation,
        &rekeyed,
        sentinel,
    )?;
    assert_eq!(built?, bytes, "incremental equals full");
    assert_eq!(
        markers[..4],
        [
            REUSED_KERML,
            REBUILT_SYSML,
            REUSED_BINDINGS,
            REUSED_SYSML_BINDINGS
        ]
    );
    assert!(markers.contains(&"sysml_owner_end"), "{markers:?}");
    assert!(!markers.contains(&"kerml_owner_begin"), "{markers:?}");
    assert!(!markers.contains(&"bindings_compile_begin"), "{markers:?}");

    // The manifest-wide compiler summary is not a part key: a previous pack
    // whose summary names another intersect-xtext but whose part entries
    // match reuses every part and is re-encoded with the current summary.
    let summary = forge(bytes, |manifest, _| {
        if let Some(identity) = manifest.compiler.get_mut("intersect-xtext") {
            *identity = "0".repeat(64);
        }
    })?;
    assert_ne!(summary, bytes);
    let (built, markers) = build_over(
        "compiler summary edited",
        definitions,
        preparation,
        &summary,
        sentinel,
    )?;
    assert_eq!(built?, bytes);
    assert_eq!(
        markers[..4],
        [
            REUSED_KERML,
            REUSED_SYSML,
            REUSED_BINDINGS,
            REUSED_SYSML_BINDINGS
        ]
    );
    assert!(!markers.contains(&"pack_reused"), "{markers:?}");

    // A previous pack this recipe cannot lay out (an unknown hook) has no
    // reusable part (not built here: it would prepare both programs).
    let foreign = forge(bytes, |manifest, _| {
        manifest.requires_hooks.push(PackHook {
            provider: ProviderId {
                name: "intersect.kerml.pilot.unknown".into(),
                version: "1".into(),
            },
            provenance: "intersect-kerml".into(),
        });
    })?;
    let explained = decisions(&foreign)?;
    assert!(
        explained.iter().all(|(_, decision, error)| {
            *decision == Rebuild(RebuildReason::LoadRefused)
                && error
                    .as_ref()
                    .is_some_and(|error| error.contains("unknown hook"))
        }),
        "{explained:?}"
    );

    // Definitions that are not the pinned files are refused before any work.
    let mut changed = definitions.to_vec();
    if let Some(sysml) = changed.last_mut() {
        sysml.text.push_str("\n// changed\n");
    }
    let (built, markers) = build_over(
        "changed SysML.xtext",
        &changed,
        preparation,
        bytes,
        sentinel,
    )?;
    let error = built.err().ok_or("changed definitions were accepted")?;
    assert!(error.contains("sysml-grammar"), "{error}");
    assert!(markers.is_empty(), "{markers:?}");
    Ok(())
}
