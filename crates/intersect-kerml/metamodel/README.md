# Stored metamodel structure

`SysML.ecore.rcore` is generated: it is the Rcore structure that Ecore
migration produces from the pinned Pilot
`fixtures/native-pilot/v1/sources/org.omg.sysml/model/SysML.ecore` (logical
URI `org.omg.sysml/model/SysML.ecore.rcore`). Hosts load it instead of
migrating on every start. Do not edit it by hand: schema declarations name its
exact SHA-256, so any byte change gives a different source identity.

`tests/sysml_structure.rs` regenerates it and fails when the stored bytes
differ. After a migration or Ecore change, rewrite it with:

    UPDATE_SYSML_RCORE=1 cargo test -p intersect-kerml --test sysml_structure
