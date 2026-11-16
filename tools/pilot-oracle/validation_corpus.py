"""Build the Pilot validation negative corpus from the Pilot's Xpect tests.

Handwritten verification tooling (never production code). Reads a sparse
checkout of the Pilot repository at the pinned commit (see README.md,
"Validation diagnostics") and writes, under OUT:

- ``units/NNN/``: one folder per Xpect test file, holding the test file
  (``X.kerml.xt`` copied as ``X.kerml``, byte for byte, so offsets agree with
  the annotations) and every non-library file its ``XPECT_SETUP`` loads with it;
- ``manifest.json``: the ``PilotOracle --examples`` manifest (the whole pinned
  library, then each unit as a folder);
- ``expected.json``: per unit and file, every ``XPECT errors`` / ``XPECT
  warnings`` expectation (severity, message, the ``at`` text, the annotation's
  line and the offset of the ``at`` text after it).

A unit is every ``.xt`` file with an ``errors`` or ``warnings`` expectation,
plus every SysML ``validation/valid`` file (expected clean). The library files
an Xpect setup names are not copied: units load on top of the whole pinned
library instead (``fixtures/native-pilot/v1/sources/sysml.library``), so
inherited-member diagnostics may differ from the Xpect run, which loads only a
few library files.

Usage: python -B validation_corpus.py UPSTREAM_TESTS LIBRARY_ROOT OUT
"""

import json
import re
import shutil
import sys
from pathlib import Path

SINGLE = re.compile(r'XPECT\s+(?P<kind>errors|warnings)\s*-+>\s*(?P<body>"(?:[^"\\]|\\.)*"\s+at\s+"(?:[^"\\]|\\.)*")')
BLOCK = re.compile(r'XPECT\s+(?P<kind>errors|warnings)\s*-{3,}\s*$')
ITEM = re.compile(r'"(?P<message>(?:[^"\\]|\\.)*)"\s+at\s+"(?P<at>(?:[^"\\]|\\.)*)"')
SETUP_FILE = re.compile(r'File\s*(?:\{\s*from\s*=\s*)?"(?P<path>[^"]+)"')
TEST_ROOTS = ("org.omg.kerml.xpect.tests", "org.omg.sysml.xpect.tests")


def unescape(text):
    return re.sub(r"\\(.)", r"\1", text)


def source_text(path):
    """The file as the Pilot sees it: line ends kept, so offsets agree (CRLF counts 2)."""
    with open(path, encoding="utf-8", newline="") as stream:
        return stream.read()


def expectations(text):
    """Every errors/warnings expectation in an Xpect file, in file order."""
    out = []
    lines = text.splitlines(keepends=True)
    offset = 0
    block = None
    for number, line in enumerate(lines, 1):
        end = offset + len(line)
        if block is not None:
            if "---" in line and not ITEM.search(line):
                block = None
            else:
                for item in ITEM.finditer(line):
                    out.append(dict(block, message=unescape(item["message"]), at=unescape(item["at"])))
            offset = end
            continue
        single = SINGLE.search(line)
        if single:
            item = ITEM.search(single["body"])
            out.append({"severity": single["kind"][:-1], "line": number, "after": end,
                        "message": unescape(item["message"]), "at": unescape(item["at"])})
        elif BLOCK.search(line):
            block = {"severity": BLOCK.search(line)["kind"][:-1], "line": number}
        offset = end
    if block is not None:
        raise ValueError(f"unterminated XPECT block at line {block['line']}")
    # A block's expectations apply after its closing line.
    for e in out:
        if "after" not in e:
            e["after"] = _block_end(lines, e["line"])
        found = at_pattern(e["at"]).search(text, e["after"])
        e["at_offset"] = found.start() if found else None
        e["at_end"] = found.end() if found else None
    return out


def at_pattern(at):
    """Xpect matches the `at` text ignoring whitespace."""
    return re.compile(r"\s*".join(re.escape(c) for c in at if not c.isspace()))


def _block_end(lines, start_line):
    offset = sum(len(l) for l in lines[:start_line])
    for line in lines[start_line:]:
        offset += len(line)
        if "---" in line and not ITEM.search(line):
            return offset
    return offset


def setup_files(text):
    """Non-library files the XPECT_SETUP loads besides ThisFile."""
    match = re.search(r"XPECT_SETUP(.*?)END_SETUP", text, re.S)
    if not match:
        return []
    found = []
    for m in SETUP_FILE.finditer(match[1]):
        path = m["path"]
        if path.startswith("/library") and "/src/" not in path:
            continue
        if path not in found:
            found.append(path)
    return found


def resolve(test_root, xt, path):
    """An Xpect setup path: '/src/X' from the test project, else beside the test."""
    if path.startswith("/"):
        return test_root / path.lstrip("/")
    candidate = xt.parent / path
    if candidate.exists():
        return candidate
    return xt.parent / (path + ".xt")


def units(upstream):
    """(test project root, .xt path) for every corpus unit, sorted."""
    found = []
    for root_name in TEST_ROOTS:
        root = upstream / root_name
        for xt in sorted(root.rglob("*.xt")):
            text = source_text(xt)
            valid = xt.parent.name == "valid" and xt.parent.parent.name == "validation"
            if re.search(r"XPECT\s+(errors|warnings)", text) or valid:
                found.append((root, xt))
    return found


def library_files(library_root):
    base = library_root / "sysml.library"
    files = [p.relative_to(library_root).as_posix() for p in base.rglob("*")
             if p.suffix in (".kerml", ".sysml")]
    return sorted(files)


def build(upstream, library_root, out):
    upstream, library_root, out = Path(upstream), Path(library_root), Path(out)
    if (out / "units").exists():
        shutil.rmtree(out / "units")
    folders, expected = [], []
    for index, (root, xt) in enumerate(units(upstream), 1):
        name = xt.name[:-3]  # strip .xt
        unit = f"{index:03d}"  # short: Windows paths are limited to 260 characters
        directory = out / "units" / unit
        directory.mkdir(parents=True)
        files = []
        text = source_text(xt)
        shutil.copyfile(xt, directory / name)
        files.append(name)
        missing = []
        for path in setup_files(text):
            source = resolve(root, xt, path)
            if not source.exists():
                missing.append(path)
                continue
            target = source.name[:-3] if source.name.endswith(".xt") else source.name
            if target in files:
                continue
            shutil.copyfile(source, directory / target)
            files.append(target)
        folders.append({"folder": unit, "files": [f"units/{unit}/{f}" for f in files]})
        expected.append({
            "unit": unit,
            "source": xt.relative_to(upstream).as_posix(),
            "file": f"units/{unit}/{name}",
            "language": "sysml" if name.endswith(".sysml") else "kerml",
            "valid_control": xt.parent.name == "valid",
            "support_files": [f"units/{unit}/{f}" for f in files[1:]],
            "missing_setup_files": missing,
            "expected": expectations(text),
        })
    manifest = {
        "library_root": str(library_root.resolve()),
        "library_files": library_files(library_root),
        "examples_root": str(out.resolve()),
        "export_objects": True,
        "folders": folders,
    }
    (out / "manifest.json").write_text(json.dumps(manifest, indent=1), encoding="utf-8")
    (out / "expected.json").write_text(json.dumps({"units": expected}, indent=1), encoding="utf-8")
    return manifest, expected


def main():
    if len(sys.argv) != 4:
        print(__doc__)
        sys.exit(2)
    manifest, expected = build(*sys.argv[1:4])
    count = sum(len(u["expected"]) for u in expected)
    errors = sum(e["severity"] == "error" for u in expected for e in u["expected"])
    print(f"{len(expected)} units, {sum(len(f['files']) for f in manifest['folders'])} files, "
          f"{count} expectations ({errors} errors, {count - errors} warnings), "
          f"{sum(1 for u in expected if u['missing_setup_files'])} units with missing setup files")


if __name__ == "__main__":
    main()
