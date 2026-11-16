"""Deterministic migration source inventory; never a semantic support award."""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DESTINATION = ROOT / "docs/rcore-language-migration.json"
UPSTREAM = ROOT / "fixtures/xtext/imported-assignment-v66/full-source-inputs"


def inventory():
    previous = {}
    if DESTINATION.exists():
        saved = json.loads(DESTINATION.read_text(encoding="utf-8"))
        previous = {row["path"]: row for row in saved["sources"]}
    paths = set((ROOT / "crates/intersect-kerml/src").rglob("*.rs"))
    paths.update((ROOT / "crates/intersect-kerml").rglob("*.rcore"))
    paths.update(path for path in UPSTREAM.rglob("*.xtext") if path.is_file())
    paths.add(ROOT / "fixtures/xtext/pinned-kerml-operands/SysML.ecore")
    rows = []
    for path in sorted(paths, key=lambda value: value.relative_to(ROOT).as_posix()):
        relative = path.relative_to(ROOT).as_posix()
        raw = path.read_bytes()
        # Rust follows the repository's canonical LF checkout policy. Definition
        # artifacts keep their exact bytes, including pinned upstream inputs.
        if path.suffix == ".rs":
            raw = raw.replace(b"\r\n", b"\n")
        digest = hashlib.sha256(raw).hexdigest()
        old = previous.get(relative, {})
        unchanged = old.get("sha256") == digest
        rows.append({
            "path": relative,
            "presence": "present",
            "sha256": digest,
            "bytes": len(raw),
            "hash_basis": "canonical-lf" if path.suffix == ".rs" else "exact-bytes",
            "kind": {".rs": "rust", ".rcore": "rcore", ".xtext": "xtext", ".ecore": "migration-input"}[path.suffix],
            # Changed source invalidates its receipt links. Human review must
            # identify actual replacements and their independent evidence.
            "review": old.get("review", "unassessed") if unchanged else "unassessed",
            "replacement_sources": old.get("replacement_sources", []) if unchanged else [],
            "evidence": old.get("evidence", []) if unchanged else [],
        })
    present = {row["path"] for row in rows}
    for relative, old in previous.items():
        if relative not in present:
            # Removal alone proves no replacement. Keep its last source
            # identity and audit trail after the Rust implementation is gone.
            rows.append({**old, "presence": "absent"})
    rows.sort(key=lambda row: row["path"])
    return {
        "version": 1,
        "purpose": "Source inventory for the active KerML/SysML Rcore migration goal",
        "qualification": "None; source presence and receipt links do not establish semantic support",
        "sources": rows,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    args = parser.parse_args()
    result = inventory()
    if args.write:
        DESTINATION.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8", newline="\n")
    elif not DESTINATION.exists() or json.loads(DESTINATION.read_text(encoding="utf-8")) != result:
        raise SystemExit("Migration source inventory is stale; review changes and run --write.")
    counts = {}
    absent = 0
    for row in result["sources"]:
        if row["presence"] == "absent":
            absent += 1
            continue
        counts[row["kind"]] = counts.get(row["kind"], 0) + 1
    print(json.dumps({"source_counts": counts, "absent_sources_retained": absent,
                      "semantic_support_awarded": False}, sort_keys=True))


if __name__ == "__main__":
    main()
