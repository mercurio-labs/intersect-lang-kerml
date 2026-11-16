//! `metamodel/SysML.ecore.rcore` is the Rcore structure that Ecore migration
//! produces from the pinned Pilot `SysML.ecore`. Hosts load it directly instead
//! of migrating on every start. This test regenerates it and requires the
//! stored bytes to match exactly; run with `UPDATE_SYSML_RCORE=1` to rewrite it
//! after the migration or the Ecore source changes.
use intersect_model::definitions::{
    bundle::{DefinitionSource, SourceKind},
    ecore::ParseLimits,
};

const ECORE: &str =
    include_str!("../../../fixtures/native-pilot/v1/sources/org.omg.sysml/model/SysML.ecore");
const STORED: &str = "metamodel/SysML.ecore.rcore";

#[test]
fn stored_sysml_structure_is_the_migration_output() -> Result<(), String> {
    let original =
        DefinitionSource::new("org.omg.sysml/model/SysML.ecore", SourceKind::Ecore, ECORE);
    let migrated = intersect_model::migration::migrate_ecore_with_limits(
        original,
        "org.omg.sysml/model/SysML.ecore.rcore",
        ParseLimits {
            source_bytes: 2_000_000,
        },
    )
    .map_err(|e| e.to_string())?;
    let structure = migrated
        .converted
        .ok_or_else(|| format!("{:?}", migrated.issues))?;
    assert_eq!(structure.kind, SourceKind::Rcore);
    assert!(
        structure.dependencies.is_empty(),
        "the stored structure must be self-contained"
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(STORED);
    if std::env::var_os("UPDATE_SYSML_RCORE").is_some() {
        std::fs::write(&path, &structure.text).map_err(|e| e.to_string())?;
    }
    let stored = std::fs::read_to_string(&path).map_err(|e| format!("{STORED}: {e}"))?;
    assert!(
        stored == structure.text,
        "{STORED} is stale; regenerate with UPDATE_SYSML_RCORE=1"
    );
    // Hosts rebuild the source from the stored text; its identity must equal
    // the migration's, which every schema declaration names.
    let reloaded = DefinitionSource::new(&structure.id.uri, SourceKind::Rcore, stored);
    assert_eq!(reloaded.id, structure.id);
    Ok(())
}
