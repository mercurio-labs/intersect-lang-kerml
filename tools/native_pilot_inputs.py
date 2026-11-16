"""Verify pinned native Pilot assets; this does not execute either language."""
import argparse
import hashlib
import json
from pathlib import Path
from urllib.parse import quote

ROOT = Path(__file__).resolve().parent.parent
DEFAULT = ROOT / "fixtures/native-pilot/v1/manifest.json"


def checked_file(root, row):
    relative = Path(row["fixture_path"])
    if relative.is_absolute() or ".." in relative.parts:
        raise ValueError("Pilot fixture path escapes its corpus")
    path = (root / relative).resolve()
    if not path.is_relative_to(root.resolve()):
        raise ValueError("Pilot fixture resolves outside its corpus")
    data = path.read_bytes()
    if len(data) != row["bytes"] or hashlib.sha256(data).hexdigest() != row["sha256"]:
        raise ValueError(f"Pilot asset identity mismatch: {relative}")
    return data


def verify(manifest=DEFAULT):
    manifest = Path(manifest)
    data = json.loads(manifest.read_text(encoding="utf-8"))
    if data["schema"] != "intersect.native-pilot.corpus.v1":
        raise ValueError("Unsupported native Pilot manifest")
    original = json.loads((ROOT / "fixtures/basic-systems/v1/contract.json").read_text())
    libraries = json.loads((ROOT / "fixtures/basic-systems/v1/upstream-standard-library-manifest.json").read_text())
    grounding = json.loads((ROOT / "fixtures/basic-systems/v1/grounding.json").read_text())
    if data["pilot_commit"] != original["pilot"]["commit"]:
        raise ValueError("Pilot commit differs from the pinned corpus")
    rows = [row for group in ("definitions", "libraries", "examples") for row in data[group]]
    sources = set()
    resources = set()
    for row in rows:
        checked_file(manifest.parent, row)
        if row["source_uri"] in sources or row["resource_uri"] in resources:
            raise ValueError("Duplicate source or resource identity")
        sources.add(row["source_uri"])
        resources.add(row["resource_uri"])
        if row["resource_uri"] != "pilot:/" + quote(row["source_uri"], safe="/"):
            raise ValueError("Resource URI differs from its explicit source mapping")
    definitions = {row["id"]: row for row in data["definitions"]}
    if set(definitions) != set(original["pilot"]["sources"]):
        raise ValueError("Original definition closure differs")
    for key, expected in original["pilot"]["sources"].items():
        row = definitions[key]
        if (row["source_uri"], row["bytes"], row["sha256"]) != (expected["uri"], expected["bytes"], expected["sha256"]):
            raise ValueError(f"Original definition changed: {key}")
    by_source = {row["source_uri"]: row for row in data["libraries"]}
    if set(by_source) != set(libraries["files"]):
        raise ValueError("Full 94-file library inventory differs")
    for uri, expected in libraries["files"].items():
        row = by_source[uri]
        if (row["bytes"], row["sha256"]) != (expected["bytes"], expected["sha256"]):
            raise ValueError(f"Original library changed: {uri}")
        owner = "kerml" if uri.endswith(".kerml") else "sysml"
        if row["language"] != owner:
            raise ValueError(f"Library language owner differs: {uri}")
    examples = {row["source_uri"]: row for row in data["examples"]}
    expected_examples = {row["relative_path"]: row for row in grounding["inspected_upstream_examples"]}
    if set(examples) != set(expected_examples):
        raise ValueError("Actual Pilot example inventory differs")
    for uri, expected in expected_examples.items():
        row = examples[uri]
        if (row["bytes"], row["sha256"]) != (expected["bytes"], expected["sha256"]):
            raise ValueError(f"Actual Pilot example changed: {uri}")
        owner = "kerml" if uri.endswith(".kerml") else "sysml"
        if row["language"] != owner:
            raise ValueError(f"Example language owner differs: {uri}")
        if row["expected_ingestion"] != "construct_and_link":
            raise ValueError(f"Example ingestion is not required: {uri}")
        if row["whole_language_semantic_expectation"] != "unclassified_requires_independent_baseline":
            raise ValueError("Pilot semantic expectations need explicit independent classification")
    checked_file(manifest.parent, data["license"])
    counts = {"definitions": len(definitions), "libraries": len(by_source),
              "kerml_libraries": sum(row["language"] == "kerml" for row in data["libraries"]),
              "sysml_libraries": sum(row["language"] == "sysml" for row in data["libraries"]),
              "examples": len(examples)}
    if counts != data["expected_counts"]:
        raise ValueError("Native Pilot counts differ from the manifest")
    return {"state": "input_integrity_verified", **counts,
            "source_bytes": sum(row["bytes"] for row in rows),
            "native_construction_verified": False, "semantic_qualification": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT)
    args = parser.parse_args()
    print(json.dumps(verify(args.manifest), sort_keys=True))


if __name__ == "__main__":
    main()
