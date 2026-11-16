"""Compare Intersect's Pilot-example load problems with the Pilot's own diagnostics.

Handwritten verification tooling (never production code). Inputs:

- the Pilot report written by ``PilotOracle --examples`` (per folder and file:
  resource errors with the linker's link text and feature, validator issues,
  object counts, timing);
- Intersect's report from ``crates/intersect-kerml/examples/pilot_examples.rs``
  (per folder: files and ``problems`` with phase, message, object/resource);
- the example source root (to quote the source line of a representative).

Writes a JSON report with per-folder/per-file results and the classification:

(a) both report an unresolved name at the same spelling in the same file;
(b) Intersect reports, the Pilot does not (grouped by cause and feature);
(c) the Pilot reports, Intersect does not.

Usage: python -B examples_report.py PILOT.json INTERSECT.json EXAMPLES_ROOT OUT.json
"""

import collections
import json
import re
import sys
import urllib.parse

OBJECT = re.compile(r"^(?P<object>.+?#\d+)\.(?P<feature>\w+): (?P<rest>.*)$", re.S)
FORMS = [
    ("unresolved", re.compile(r"^unresolved reference '(?P<spelling>.*)'$", re.S)),
    ("no element named", re.compile(r"^no element named '(?P<spelling>.*)'$", re.S)),
    ("depends on itself", re.compile(r"^reference '(?P<spelling>.*)' depends on itself$", re.S)),
    ("scope", re.compile(r"^'(?P<spelling>.*)': scope: (?P<reason>.*)$", re.S)),
]
PILOT_LINK = re.compile(r"Couldn't resolve reference to (?P<kind>\w+) '(?P<spelling>.*)'\.?$", re.S)


def norm(spelling):
    """A spelling compared without quotes and whitespace."""
    return re.sub(r"[\s']", "", spelling or "")


def resource_of(name):
    return urllib.parse.unquote(name.split("#", 1)[0])


def parse_intersect(problem):
    """One Intersect problem as (file, kind, object, feature, spelling, reason)."""
    message = problem["message"]
    if problem["phase"] != "link":
        return {
            "file": resource_of(problem.get("resource") or problem.get("object") or ""),
            "phase": problem["phase"],
            "message": message,
        }
    match = OBJECT.match(message)
    if not match:
        return {"file": resource_of(problem.get("object", "")), "phase": "link", "message": message}
    rest = match["rest"]
    for kind, form in FORMS:
        m = form.match(rest)
        if m:
            return {
                "file": resource_of(match["object"]),
                "phase": "link",
                "object": urllib.parse.unquote(match["object"]),
                "feature": match["feature"],
                "spelling": m["spelling"],
                "kind": kind,
                "reason": m.groupdict().get("reason"),
                "message": message,
            }
    return {"file": resource_of(match["object"]), "phase": "link", "message": message}


def intersect_issues(problems):
    """Link problems merged per (object, feature, spelling): one issue each."""
    issues = collections.OrderedDict()
    others = []
    for problem in problems:
        parsed = parse_intersect(problem)
        if parsed["phase"] != "link" or "spelling" not in parsed:
            others.append(parsed)
            continue
        key = (parsed["object"], parsed["feature"], parsed["spelling"])
        issue = issues.setdefault(key, {
            "file": parsed["file"],
            "object": parsed["object"],
            "feature": parsed["feature"],
            "spelling": parsed["spelling"],
            "kinds": [],
            "reason": None,
            "messages": [],
        })
        issue["kinds"].append(parsed["kind"])
        issue["messages"].append(parsed["message"])
        if parsed["reason"]:
            issue["reason"] = parsed["reason"]
    for issue in issues.values():
        if issue["reason"]:
            issue["cause"] = "scope: " + issue["reason"]
        elif "no element named" in issue["kinds"]:
            issue["cause"] = "no element named"
        elif "depends on itself" in issue["kinds"]:
            issue["cause"] = "depends on itself"
        else:
            issue["cause"] = "unresolved reference"
    return list(issues.values()), others


DECLARATION = {
    "feature", "part", "attribute", "item", "port", "action", "state", "end", "in", "out", "inout",
    "struct", "class", "package", "step", "expr", "function", "assoc", "behavior", "datatype", "def",
    "ref", "member", "classifier", "type", "predicate", "connector", "binding", "metaclass", "library",
    "standard", "abstract", "private", "public", "protected", "derived", "readonly", "var", "composite",
    "portion", "individual", "snapshot", "timeslice", "occurrence", "requirement", "constraint", "calc",
    "use", "case", "view", "viewpoint", "rendering", "concern", "enum", "allocation", "interface",
    "connection", "flow", "message", "analysis", "verification", "metadata", "namespace", "alias",
    "interaction", "const", "constant", "event", "exhibit", "perform", "then", "entry", "do", "exit",
    "objective", "subject", "actor", "stakeholder", "transition", "first", "succession", "variant",
    "variation", "assert", "comment", "doc", "dependency", "specialization", "subtype", "subclassifier",
}


def source_line(root, file, spelling, cache):
    """The first line of FILE that uses SPELLING as a name (not where it is declared)."""
    if file not in cache:
        try:
            with open(f"{root}/{file}", encoding="utf-8") as handle:
                cache[file] = handle.read().splitlines()
        except OSError:
            cache[file] = []
    lines = cache[file]
    pattern = re.compile(r"(?<![\w'])" + re.escape(spelling) + r"(?![\w'])")
    hits, uses = [], []
    for index, line in enumerate(lines):
        for match in pattern.finditer(line):
            hits.append(index)
            before = line[:match.start()].split()
            if not before or before[-1] not in DECLARATION:
                uses.append(index)
    chosen = uses or hits
    if not chosen:
        return None
    return {"line": chosen[0] + 1, "text": lines[chosen[0]].strip(), "occurrences": len(set(hits)),
            "declaration_only": not uses}


def pilot_links(file_entry):
    links = []
    for d in file_entry["resource_diagnostics"]:
        m = PILOT_LINK.match(d["message"])
        if not m:
            continue
        links.append({
            "spelling": d.get("link_text") or m["spelling"],
            "kind": m["kind"],
            "feature": d.get("feature"),
            "line": d.get("line"),
            "owner_class": d.get("owner_class"),
            "message": d["message"],
        })
    return links


def main(pilot_path, intersect_path, root, out_path):
    with open(pilot_path, encoding="utf-8") as handle:
        pilot = json.load(handle)
    with open(intersect_path, encoding="utf-8") as handle:
        intersect = json.load(handle)
    pilot_folders = {f["folder"]: f for f in pilot["folders"]}
    cache = {}

    folders = []
    class_a, class_b, class_c_links, class_c_validation, parse_b = [], [], [], [], []
    clean_in_pilot = []
    for ifolder in intersect["folders"]:
        name = ifolder["folder"]
        pfolder = pilot_folders.get(name, {"files": []})
        pfiles = {f["resource"]: f for f in pfolder["files"]}
        issues, others = intersect_issues(ifolder["problems"])
        files = []
        for raw in ifolder["files"]:
            file = urllib.parse.unquote(raw)
            pfile = pfiles.get(file)
            phases = collections.Counter(
                parse_intersect(p)["phase"] for p in ifolder["problems"]
                if parse_intersect(p)["file"] == file)
            file_issues = [i for i in issues if i["file"] == file]
            links = pilot_links(pfile) if pfile else []
            # Pair by normalized spelling (a multiset per file).
            pending = list(links)
            for issue in file_issues:
                match = next((l for l in pending if norm(l["spelling"]) == norm(issue["spelling"])), None)
                if match is None:
                    where = source_line(root, file, issue["spelling"], cache)
                    class_b.append(dict(issue, source=where))
                    issue["class"] = "b"
                else:
                    pending.remove(match)
                    issue["class"] = "a"
                    class_a.append({
                        "file": file,
                        "object": issue["object"],
                        "feature": issue["feature"],
                        "pilot_feature": match["feature"],
                        "spelling": issue["spelling"],
                        "pilot_line": match["line"],
                        "intersect_cause": issue["cause"],
                    })
            for link in pending:
                class_c_links.append(dict(link, file=file))
            pilot_parse = [d for d in (pfile or {}).get("resource_diagnostics", [])
                           if not PILOT_LINK.match(d["message"])]
            for other in others:
                if other["file"] == file and other["phase"] == "parse" and not pilot_parse:
                    parse_b.append(dict(other))
            validation = (pfile or {}).get("diagnostics", [])
            for d in validation:
                class_c_validation.append(dict(d, file=file))
            perrors = [d for d in (pfile or {}).get("resource_diagnostics", []) if d["severity"] == "error"]
            perrors += [d for d in validation if d["severity"] == "error"]
            pwarnings = [d for d in (pfile or {}).get("resource_diagnostics", []) if d["severity"] == "warning"]
            pwarnings += [d for d in validation if d["severity"] == "warning"]
            entry = {
                "file": file,
                "pilot": {
                    "loaded": pfile is not None,
                    "objects": (pfile or {}).get("objects"),
                    "errors": len(perrors),
                    "warnings": len(pwarnings),
                    "unresolved_references": len(links),
                    "error_messages": [f"{d.get('line')}: {d['message']}" for d in perrors],
                    "warning_messages": [f"{d.get('line')}: {d['message']}" for d in pwarnings],
                },
                "intersect": {
                    "problems_by_phase": dict(phases),
                    "link_issues": len(file_issues),
                    "class_a": sum(1 for i in file_issues if i["class"] == "a"),
                    "class_b": sum(1 for i in file_issues if i["class"] == "b"),
                },
            }
            if pfile is not None and not perrors:
                clean_in_pilot.append({
                    "file": file,
                    "pilot_warnings": len(pwarnings),
                    "intersect_problems": sum(phases.values()),
                    "intersect_folder_loaded": bool(ifolder.get("loaded")),
                })
            files.append(entry)
        extra = [f["resource"] for f in pfolder["files"] if not f.get("requested")]
        folders.append({
            "folder": name,
            "pilot_timing": pfolder.get("timing"),
            "pilot_exception": pfolder.get("exception"),
            "pilot_errors": pfolder.get("error_count"),
            "pilot_warnings": pfolder.get("warning_count"),
            "pilot_demand_loaded": extra,
            "intersect_loaded": ifolder.get("loaded"),
            "intersect_problems_by_phase": ifolder.get("problems_by_phase"),
            "intersect_references": ifolder.get("references"),
            "intersect_resolved": ifolder.get("resolved"),
            "files": files,
        })

    groups = collections.OrderedDict()
    for issue in sorted(class_b, key=lambda i: (i["cause"], i["feature"])):
        key = (issue["cause"], issue["feature"])
        group = groups.setdefault(key, {"cause": issue["cause"], "feature": issue["feature"],
                                        "count": 0, "files": collections.Counter(), "examples": []})
        group["count"] += 1
        group["files"][issue["file"]] += 1
        if len(group["examples"]) < 3:
            group["examples"].append({k: issue[k] for k in ("file", "object", "spelling", "source")})
    b_groups = sorted(groups.values(), key=lambda g: -g["count"])
    for group in b_groups:
        group["files"] = dict(group["files"].most_common())

    c_validation = collections.Counter((d["severity"], d.get("code"), d["message"]) for d in class_c_validation)
    report = {
        "inputs": {"pilot": pilot_path, "intersect": intersect_path, "examples_root": root},
        "pilot_oracle": pilot.get("oracle"),
        "pilot_java": pilot.get("java"),
        "pilot_timing": pilot.get("timing"),
        "totals": {
            "folders": len(folders),
            "files": sum(len(f["files"]) for f in folders),
            "pilot_errors": sum(f["pilot_errors"] or 0 for f in folders),
            "pilot_warnings": sum(f["pilot_warnings"] or 0 for f in folders),
            "pilot_unresolved_references": sum(e["pilot"]["unresolved_references"]
                                               for f in folders for e in f["files"]),
            "intersect_problems": sum(len(f["problems"]) for f in intersect["folders"]),
            "intersect_link_issues": len(class_a) + len(class_b),
            "class_a": len(class_a),
            "class_b_link": len(class_b),
            "class_b_parse": len(parse_b),
            "class_c_unresolved": len(class_c_links),
            "class_c_validation": len(class_c_validation),
            "clean_in_pilot": len(clean_in_pilot),
            "clean_in_pilot_with_intersect_problems": sum(1 for c in clean_in_pilot if c["intersect_problems"]),
        },
        "class_a": class_a,
        "class_b_groups": b_groups,
        "class_b_parse": parse_b,
        "class_b": class_b,
        "class_c_unresolved": class_c_links,
        "class_c_validation_summary": [
            {"severity": s, "code": c, "message": m, "count": n} for (s, c, m), n in c_validation.most_common()
        ],
        "clean_in_pilot": clean_in_pilot,
        "folders": folders,
    }
    with open(out_path, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=1, ensure_ascii=False)
    print(json.dumps(report["totals"], indent=1))


if __name__ == "__main__":
    if len(sys.argv) != 5:
        sys.exit(__doc__)
    main(*sys.argv[1:])
