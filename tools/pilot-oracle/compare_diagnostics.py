"""Compare Pilot validation diagnostics with Xpect expectations and with Intersect.

Handwritten verification tooling (never production code). Inputs are written by
the other oracle tools:

- PILOT.json: ``PilotOracle --examples`` over the validation corpus manifest
  (``validation_corpus.py``); per file the resource diagnostics (syntax and
  linking) and the validator issues with their target object.
- EXPECTED.json: ``validation_corpus.py``'s Xpect expectations.
- RULES.json: ``validation-rules.json`` (``validation_rules.py``).
- INTERSECT.json: Intersect's diagnostics in the format below.
- DUMP.json (optional): Intersect's linked model dump of the same files (the
  ``frozen_kerml_load`` / ``library_probe`` export ``compare.py`` reads).

Commands (CORPUS is the corpus directory, for the source text):

    python -B compare_diagnostics.py xpect PILOT.json EXPECTED.json CORPUS [OUT.json]
    python -B compare_diagnostics.py rules PILOT.json RULES.json [OUT.json]
    python -B compare_diagnostics.py intersect PILOT.json INTERSECT.json CORPUS [--dump DUMP.json] [OUT.json]

``xpect`` checks the Pilot against the Xpect annotations: an expectation
matches a Pilot diagnostic of the same file, severity and message whose issue
region starts at the ``at`` text after the annotation (Xpect ignores
whitespace in ``at``); a same-message diagnostic elsewhere in the file counts
as ``moved``. ``rules`` counts the Pilot's diagnostics per rule and batch.

``intersect`` matches Pilot and Intersect diagnostics by resource, severity,
code (message only when a side has no code; ``compare.compare_diagnostics``)
and target element. With ``--dump`` the targets are paired through the
containment pairing of ``compare.py`` (the Pilot object export in PILOT.json
against the dump). Without it they are matched by the line and column of the
target element's start (Pilot UTF-16 offsets and Intersect byte offsets are
both converted through the source text). The report counts, per rule code,
``agree``, ``pilot_only`` and ``intersect_only``.

Intersect diagnostics format (``intersect-diagnostics/1``)::

    {"format": "intersect-diagnostics/1",
     "producer": "...",
     "resources": {
       "units/001/X.kerml": [
         {"severity": "error",            # error | warning | info
          "code": "validateFeatureHasType_",   # the Pilot's issue code, verbatim
          "message": "Features must have at least one type",
          "phase": "validate",            # LoadDiagnostic phase
          "element": {"id": "units/001/X.kerml#12", "class": "Feature"},
          "start": 120, "end": 141}       # byte offsets of the element's node
       ]}}

Resource names are the corpus-relative paths the Pilot report uses.
"""

import argparse
import importlib.util
import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("pilot_compare", Path(__file__).with_name("compare.py"))
compare = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(compare)

LINKING = "org.eclipse.xtext.diagnostics.Diagnostic.Linking"
SYNTAX = "org.eclipse.xtext.diagnostics.Diagnostic.Syntax"


def load(path):
    with open(path, encoding="utf-8") as stream:
        return json.load(stream)


def pilot_diagnostics(pilot):
    """Every Pilot diagnostic as a flat record, resource diagnostics first."""
    out = []
    for folder in pilot["folders"]:
        for entry in folder["files"]:
            resource = entry["resource"]
            for d in entry.get("resource_diagnostics", []):
                out.append({"unit": folder["folder"], "resource": resource, "source": "resource",
                            "severity": d["severity"], "code": d.get("code"), "message": d["message"],
                            "line": d.get("line"), "column": d.get("column"), "offset": d.get("offset"),
                            "length": d.get("length"), "target": None})
            for d in entry.get("diagnostics", []):
                target = None
                if d.get("target_fragment") is not None:
                    target = {"resource": d.get("target_resource", resource), "fragment": d["target_fragment"],
                              "class": d.get("target_class"), "start": d.get("target_start"),
                              "end": d.get("target_end")}
                out.append({"unit": folder["folder"], "resource": resource, "source": "validator",
                            "severity": d["severity"], "code": d.get("code"), "message": d["message"],
                            "line": d.get("line"), "column": d.get("column"), "offset": d.get("offset"),
                            "length": d.get("length"), "target": target})
    return out


def rule_key(d):
    """The rule a diagnostic belongs to: its code, or linking/syntax."""
    if d["code"] == LINKING:
        return "linking"
    if d["code"] == SYNTAX:
        return "syntax"
    return d["code"] or f"(no code) {d['message']}"


def squash(text):
    return re.sub(r"\s+", "", text or "")


def source_text(path):
    """The file as the Pilot sees it: line ends kept, so offsets agree (CRLF counts 2)."""
    with open(path, encoding="utf-8", newline="") as stream:
        return stream.read()


def compare_xpect(pilot, expected, corpus):
    """Match Xpect expectations with Pilot diagnostics per file (a multiset)."""
    corpus = Path(corpus)
    diagnostics = defaultdict(list)
    for d in pilot_diagnostics(pilot):
        diagnostics[d["resource"]].append(d)
    files, totals, per_rule = [], Counter(), defaultdict(Counter)
    for unit in expected["units"]:
        name = unit["file"]
        text = source_text(corpus / name)
        pending = list(diagnostics.get(name, []))
        rows = []
        for e in unit["expected"]:
            same = [d for d in pending if d["severity"] == e["severity"] and d["message"] == e["message"]]
            here = [d for d in same if d["offset"] is not None and e["at_offset"] is not None and (
                d["offset"] == e["at_offset"] or (e["at_offset"] <= d["offset"] < e["at_end"]) or
                squash(text[d["offset"]:d["offset"] + (d["length"] or 0)]) == squash(e["at"]))]
            found = here[0] if here else (same[0] if same else None)
            status = "matched" if here else ("moved" if same else "missing")
            if found is not None:
                pending.remove(found)
            rows.append({**e, "status": status, "pilot": found})
            totals[status] += 1
            per_rule[rule_key(found) if found else f"(missing) {e['message']}"][status] += 1
        for d in pending:
            per_rule[rule_key(d)]["unannotated"] += 1
        totals["unannotated"] += len(pending)
        support = [d for f in unit.get("support_files", []) for d in diagnostics.get(f, [])]
        totals["support_file_diagnostics"] += len(support)
        files.append({"unit": unit["unit"], "file": name, "source": unit["source"],
                      "valid_control": unit["valid_control"], "expectations": rows,
                      "unannotated": pending, "support_file_diagnostics": support})
    totals["expectations"] = sum(len(u["expected"]) for u in expected["units"])
    return {"totals": dict(totals), "per_rule": {k: dict(v) for k, v in sorted(per_rule.items())}, "files": files}


def rule_summary(pilot, rules):
    """Pilot diagnostics per rule code, rule ids, batch and category."""
    by_code = defaultdict(list)
    for r in rules["rules"]:
        by_code[r["code"]].append(r)
    counts = Counter()
    units = defaultdict(set)
    for d in pilot_diagnostics(pilot):
        key = rule_key(d)
        counts[(key, d["severity"])] += 1
        units[key].add(d["unit"])
    rows = []
    batches = Counter()
    batch_rules = defaultdict(set)
    for (key, severity), n in counts.most_common():
        matching = by_code.get(key, [])
        batch = sorted({r["batch"] for r in matching})
        rows.append({"code": key, "severity": severity, "count": n, "units": len(units[key]),
                     "rules": [r["id"] for r in matching], "batches": batch,
                     "categories": sorted({r["category"] for r in matching})})
        for b in batch:
            batches[b] += n
            batch_rules[b].update(r["id"] for r in matching)
    fired = {rid for row in rows for rid in row["rules"]}
    per_batch = {}
    for b, title in rules["batches"].items():
        inventoried = [r["id"] for r in rules["rules"] if r["batch"] == b]
        per_batch[b] = {"title": title, "rules": len(inventoried),
                        "rules_firing_on_codes": len(set(inventoried) & fired),
                        "diagnostics": batches.get(b, 0)}
    return {"codes": rows, "batches": per_batch,
            "rules_with_a_firing_code": len(fired), "rules": len(rules["rules"]),
            "framework": {k: sum(r["count"] for r in rows if r["code"] == k) for k in ("linking", "syntax")}}


def line_column(text, offset, utf16=False):
    """1-based line and column (in code points) of an offset."""
    if offset is None:
        return None
    if utf16:
        units = text.encode("utf-16-le")
        prefix = units[:offset * 2].decode("utf-16-le", errors="ignore")
    else:
        prefix = text.encode("utf-8")[:offset].decode("utf-8", errors="ignore")
    line = prefix.count("\n") + 1
    return line, len(prefix) - (prefix.rfind("\n") + 1) + 1


def intersect_records(data):
    if data.get("format") != "intersect-diagnostics/1":
        raise ValueError("INTERSECT.json is not intersect-diagnostics/1")
    resources = data["resources"]
    if isinstance(resources, list):
        resources = {r["resource"]: r["diagnostics"] for r in resources}
    return resources


def compare_intersect(pilot, intersect, corpus, dump=None):
    """Match Pilot and Intersect validator diagnostics per resource and rule."""
    corpus = Path(corpus)
    ours = intersect_records(intersect)
    texts = {}

    def text(resource):
        if resource not in texts:
            path = corpus / resource
            texts[resource] = source_text(path) if path.exists() else ""
        return texts[resource]

    records = [d for d in pilot_diagnostics(pilot) if d["source"] == "validator"]
    pairs = {}
    if dump is not None:
        objects, roots = {}, {}
        for folder in pilot["folders"]:
            for entry in folder["files"]:
                if "object_export" not in entry:
                    raise ValueError("PILOT.json has no object export; set export_objects in the manifest")
                for o in entry["object_export"]:
                    objects[(entry["resource"], o["fragment"])] = o
                roots[entry["resource"]] = [(entry["resource"], "/")]
        frozen, frozen_roots = compare.frozen_objects(dump)
        pairs = compare.pair_objects(objects, roots, frozen, frozen_roots)
    pilot_side, our_side = [], []
    for d in records:
        target = d["target"] or {}
        key = (target.get("resource"), target.get("fragment"))
        if dump is None and target.get("start") is not None:
            pairs[key] = (target["resource"], line_column(text(target["resource"]), target["start"], utf16=True))
        pilot_side.append({**d, "target": {"resource": key[0], "fragment": key[1]}})
    for resource, items in ours.items():
        for d in items:
            element = d.get("element") or {}
            if dump is not None:
                target = element.get("id")
            else:
                target = (resource, line_column(text(resource), d.get("start"))) if d.get("start") is not None else None
            our_side.append({**d, "resource": resource, "target": target})
    result = compare.compare_diagnostics({"resources": [{"diagnostics": pilot_side}]},
                                         {"diagnostics": our_side}, pairs)
    per_rule = defaultdict(Counter)
    for m in result["matches"]:
        per_rule[rule_key(m["pilot"])]["agree"] += 1
    for d in result["pilot_only_diagnostics"]:
        per_rule[rule_key(d)]["pilot_only"] += 1
    for d in result["intersect_only_diagnostics"]:
        per_rule[d.get("code") or f"(no code) {d.get('message')}"]["intersect_only"] += 1
    result["per_rule"] = {k: dict(v) for k, v in sorted(per_rule.items())}
    result["pairing"] = "containment" if dump is not None else "line and column"
    return result


def write(result, out):
    if out:
        with open(out, "w", encoding="utf-8", newline="\n") as stream:
            json.dump(result, stream, indent=1)
            stream.write("\n")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    x = sub.add_parser("xpect")
    x.add_argument("pilot"), x.add_argument("expected"), x.add_argument("corpus"), x.add_argument("out", nargs="?")
    r = sub.add_parser("rules")
    r.add_argument("pilot"), r.add_argument("rules"), r.add_argument("out", nargs="?")
    i = sub.add_parser("intersect")
    i.add_argument("pilot"), i.add_argument("intersect"), i.add_argument("corpus")
    i.add_argument("--dump"), i.add_argument("out", nargs="?")
    args = parser.parse_args(argv)
    if args.command == "xpect":
        result = compare_xpect(load(args.pilot), load(args.expected), args.corpus)
        print(json.dumps(result["totals"]))
        for key, row in result["per_rule"].items():
            if set(row) - {"matched"}:
                print(f"  {key}: {row}")
    elif args.command == "rules":
        result = rule_summary(load(args.pilot), load(args.rules))
        print(f"{result['rules_with_a_firing_code']} of {result['rules']} rules share a code the Pilot reported; "
              f"framework {result['framework']}")
        for b, row in result["batches"].items():
            print(f"  {b}: {row['diagnostics']} diagnostics, {row['rules_firing_on_codes']} of {row['rules']} rules ({row['title'][:60]})")
    else:
        result = compare_intersect(load(args.pilot), load(args.intersect), args.corpus,
                                   args.dump)
        print(f"matched {result['matched']}, pilot_only {result['pilot_only']}, "
              f"intersect_only {result['intersect_only']} ({result['pairing']})")
    write(result, args.out)
    return result


if __name__ == "__main__":
    main()
