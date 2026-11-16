"""Capture exact local compiler inputs from a completed Cargo JSON example build.

This verifies build provenance only; it does not award runtime qualification.
Source hashes are observed after the build. Copied inputs with preserved old
timestamps require a forced affected-package rebuild; inspect cargo_fresh before
claiming changed bytes were compiled. Cargo success alone is insufficient.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


def readable_path(path):
    readable = path
    value = str(path)
    if os.name == 'nt' and path.is_absolute() and not value.startswith('\\\\?\\'):
        readable = Path('\\\\?\\UNC\\' + value[2:] if value.startswith('\\\\') else '\\\\?\\' + value)
    return readable


def digest(path):
    return hashlib.sha256(readable_path(path).read_bytes()).hexdigest()


def resolved_source(path):
    resolved = str(path.resolve(strict=True))
    if resolved.startswith('\\\\?\\UNC\\'):
        resolved = '\\\\' + resolved[8:]
    elif resolved.startswith('\\\\?\\'):
        resolved = resolved[4:]
    return Path(resolved)


def artifact_dep_info(binary):
    # Cargo's top-level library dep-info retains `lib`; rustc's hashed deps
    # sibling drops it. Keep both supported artifact layouts explicit.
    adjacent = binary.with_suffix('.d')
    if adjacent.is_file():
        return adjacent
    return binary.with_name(binary.stem.removeprefix('lib') + '.d')


def expand_source_inputs(sources, allowed_roots):
    # Cargo also records build-script rerun-if-changed directories. Preserve
    # their complete member list and bytes, including files outside rustc's
    # module graph, rather than dropping them or trying to hash a directory.
    files, directories = set(), {}
    for source in sorted(sources):
        if readable_path(source).is_dir():
            members = []
            for path in readable_path(source).rglob('*'):
                resolved = resolved_source(path)
                if not any(resolved.is_relative_to(root) for root in allowed_roots):
                    raise ValueError(f'Compiler directory input outside source roots: {resolved}')
                if readable_path(resolved).is_file():
                    members.append(resolved)
            directories[source] = sorted(set(members))
            files.update(members)
        else:
            files.add(source)
    return files, directories


def dependencies(path, root, allowed_roots=()):
    # rustc dep-info uses Make escaping for spaces, including Windows paths.
    rule = path.read_text(encoding="utf-8").splitlines()[0]
    tail = rule.split(": ", 1)[1]
    pending = ""
    for token in tail.split(" "):
        pending += token
        if pending.endswith("\\"):
            pending = pending[:-1] + " "
            continue
        if pending:
            source = Path(pending)
            if not source.is_absolute():
                source = root / source
            source = resolved_source(source)
            if not any(source.is_relative_to(candidate) for candidate in (root, *allowed_roots)):
                raise ValueError(f"Compiler input outside source root: {source}")
            yield source
        pending = ""
    if pending:
        raise ValueError("Incomplete compiler dep-info escape")


def capture(root, cargo_json, example, target_kind="example", required_engines=None):
    root = root.resolve(strict=True)
    required = set(required_engines or ("intersect_model", "intersect_text", "intersect_kerml"))
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--format-version', '1', '--locked', '--offline'], cwd=root))
    packages = {package['id']: package for package in metadata['packages']}
    origins = {root: 'workspace'}
    artifacts = []
    finished = False
    for line in cargo_json.read_text(encoding="utf-8-sig").splitlines():
        if not line.strip():
            continue
        message = json.loads(line)
        if message.get("reason") == "build-finished":
            finished = message.get("success") is True
        if message.get("reason") == "compiler-artifact":
            artifacts.append(message)
    if not finished:
        raise ValueError("Cargo build has no successful terminal result")
    hosts = [a for a in artifacts if a["target"]["name"] == example
             and target_kind in a["target"]["kind"] and a.get("executable")]
    if len(hosts) != 1:
        raise ValueError("Expected exactly one selected target executable")
    host = hosts[0]
    sources = {root / "Cargo.toml", root / "Cargo.lock"}
    selected = []
    names = set()
    for artifact in artifacts:
        source = Path(artifact["target"]["src_path"]).resolve()
        if not source.is_relative_to(root):
            if artifact['target']['name'] not in required:
                continue
            package = packages.get(artifact['package_id'])
            if not package:
                raise ValueError('Required external engine is absent from resolved Cargo metadata')
            identity = package.get('source') or ''
            if not re.fullmatch(r'git\+https://github\.com/mercurio-labs/intersect-(?:model|text)\.git\?rev=[0-9a-f]{40}#[0-9a-f]{40}', identity):
                raise ValueError('Required external engine must use a canonical immutable Intersect Git source')
            manifest_root = Path(package['manifest_path']).resolve().parent
            if not source.is_relative_to(manifest_root):
                raise ValueError('Engine source differs from its resolved package')
            checkout = Path(subprocess.check_output(
                ['git', 'rev-parse', '--show-toplevel'], cwd=manifest_root, text=True).strip()).resolve()
            origins[checkout] = identity
        paths = [Path(p) for p in artifact["filenames"] if p.endswith(".rlib")]
        if artifact is host:
            paths = [Path(host["executable"])]
        if not paths:
            continue
        if len(paths) != 1:
            raise ValueError("Ambiguous local compiler artifact")
        binary = paths[0]
        dep = artifact_dep_info(binary)
        sources.update(dependencies(dep, root, tuple(origins)))
        manifest = source.parent
        while not (manifest / "Cargo.toml").is_file():
            if manifest == manifest.parent:
                raise ValueError('Compiler artifact has no owning manifest')
            manifest = manifest.parent
        sources.add(manifest / "Cargo.toml")
        if (manifest / 'build.rs').is_file():
            sources.add(manifest / 'build.rs')
        names.add(artifact["target"]["name"])
        selected.append({"target": artifact["target"]["name"],
                         "features": artifact["features"], "profile": artifact["profile"],
                         "package_id": artifact["package_id"],
                         "cargo_fresh": artifact.get("fresh"),
                         "artifact": str(binary), "sha256": digest(binary)})
    if not required <= names:
        raise ValueError("Build does not identify the explicitly required engine artifacts")
    for checkout in origins:
        sources.update(checkout / name for name in ('Cargo.toml', 'Cargo.lock') if (checkout / name).is_file())
    def source_label(path):
        checkout = max((candidate for candidate in origins if path.is_relative_to(candidate)), key=lambda p: len(p.parts))
        prefix = '' if checkout == root else origins[checkout] + '/'
        return prefix + path.relative_to(checkout).as_posix()
    sources, directories = expand_source_inputs(sources, tuple(origins))
    files = {source_label(p): digest(p) for p in sorted(sources)}
    directory_members = {source_label(path): [source_label(member) for member in members]
                         for path, members in directories.items()}
    return {"schema": "intersect.native-pilot.build-inputs.v3",
            "target": example, "target_kind": target_kind, "required_engine_targets": sorted(required), "files": files, "file_count": len(files),
            "directory_members": directory_members,
            "artifacts": selected, "binary_sha256": digest(Path(host["executable"])),
            "cargo_log_sha256": digest(cargo_json), "qualification_awarded": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cargo_json", type=Path)
    parser.add_argument("example")
    parser.add_argument("output", type=Path)
    parser.add_argument("--engine", action="append", choices=["intersect_model", "intersect_text", "intersect_kerml"], help="Explicit required engine; defaults to all three")
    parser.add_argument("--target-kind", choices=["example", "test"], default="example")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    args = parser.parse_args()
    result = capture(args.root, args.cargo_json, args.example, args.target_kind, args.engine)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"file_count": result["file_count"],
                      "binary_sha256": result["binary_sha256"], "guard": str(args.output)}))


if __name__ == "__main__":
    main()
