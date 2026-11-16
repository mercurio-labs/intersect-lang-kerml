"""Compare a Pilot oracle export with an Intersect frozen-engine export.

Handwritten verification tooling, not production code. Usage:

    python tools/pilot-oracle/compare.py PILOT.json FROZEN.json [REPORT.json]

PILOT.json is written by PilotOracle.java; FROZEN.json is a linked, validated
model exported by `frozen_kerml_load` (FROZEN_KERML_LINKED_DUMP) or another
host. Objects are paired by walking both containment trees from each
resource root in step: the n-th child in a containment feature pairs with the
n-th child in the same feature. Every paired object is then compared field by
field: its class, its attributes and its reference targets (through the
pairing). `elementId` is skipped: the Pilot draws random UUIDs.
Per-resource counts distinguish equal fields, deviations listed by exact class
and feature in deviations.json (optionally restricted by `objects` to the
listed Pilot [resource, fragment] objects), and unexplained differences. Reference counts
are reference-valued fields (an ordered multi-reference counts once).
"""
import json
import struct
import sys
import xml.etree.ElementTree as ET
from collections import Counter
from pathlib import Path
from urllib.parse import unquote

SKIP = {"elementId"}
ECORE = Path(__file__).resolve().parents[2] / "fixtures/native-pilot/v1/sources/org.omg.sysml/model/SysML.ecore"
XSI = "{http://www.w3.org/2001/XMLSchema-instance}type"


def metamodel():
    """Enum literals by classifier index, and attribute defaults by
    (class, feature) with inheritance, from the pinned SysML.ecore."""
    root = ET.parse(ECORE).getroot()
    classifiers = root.findall("eClassifiers")
    enums, literal_default = {}, {}
    by_name = {c.get("name"): c for c in classifiers}
    for c in classifiers:
        if c.get(XSI) == "ecore:EEnum":
            names = [l.get("name") for l in c.findall("eLiterals")]
            enums[c.get("name")] = names
            literal_default[c.get("name")] = names[0] if names else None
    defaults = {}
    types = {}

    def attrs(name, seen=()):
        c = by_name.get(name)
        if c is None or name in seen:
            return {}
        out = {}
        for sup in (c.get("eSuperTypes") or "").split():
            out.update(attrs(sup.split("//")[-1], seen + (name,)))
        for f in c.findall("eStructuralFeatures"):
            if f.get(XSI) != "ecore:EAttribute":
                continue
            etype = f.get("eType") or ""
            types[(name, f.get("name"))] = etype.split("//")[-1]
            literal = f.get("defaultValueLiteral")
            if literal is not None:
                value = {"true": True, "false": False}.get(literal, literal)
            elif etype.endswith("EBoolean"):
                value = False
            elif etype.endswith(("EInt", "EIntegerObject", "EDouble", "ELong")):
                value = 0
            else:
                value = literal_default.get(etype.split("//")[-1])
            out[f.get("name")] = value
        return out

    for name in by_name:
        for feature, value in attrs(name).items():
            defaults[(name, feature)] = value
    # Attribute types with inheritance: the declaring class's entry wins.
    def type_of(name, feature, seen=()):
        if (name, feature) in types:
            return types[(name, feature)]
        c = by_name.get(name)
        if c is None or name in seen:
            return None
        for sup in (c.get("eSuperTypes") or "").split():
            found = type_of(sup.split("//")[-1], feature, seen + (name,))
            if found:
                return found
        return None
    # Container features: references whose opposite is a containment.
    containers = set()
    refs = {}
    for c in classifiers:
        for f in c.findall("eStructuralFeatures"):
            if f.get(XSI) == "ecore:EReference":
                refs[f"#//{c.get('name')}/{f.get('name')}"] = f
    for f in refs.values():
        opposite = refs.get(f.get("eOpposite") or "")
        if opposite is not None and opposite.get("containment") == "true":
            containers.add(f.get("name"))
    return enums, defaults, type_of, containers


ENUMS, DEFAULTS, TYPE_OF, CONTAINERS = metamodel()
# Defaults a Pilot implementation class sets in its constructor, overriding
# the metamodel's. EMF's eIsSet compares against the instance default, so the
# Pilot export omits a value equal to it. EnumerationDefinitionImpl sets
# IS_VARIATION_EDEFAULT and isVariation to true (the metamodel constraint
# `EnumerationDefinition: isVariation`).
DEFAULTS[("EnumerationDefinition", "isVariation")] = True
# FramedConcernMembershipImpl likewise defaults kind to requirement (the
# metamodel constraint `FramedConcernMembership: kind =
# RequirementConstraintKind::requirement`).
DEFAULTS[("FramedConcernMembership", "kind")] = "requirement"


def plain(value, cls, feature):
    """Our enum values carry their literal's declaration path; the last
    step is the literal's position in its enum."""
    if isinstance(value, dict) and value.get("kind") == "float64":
        return struct.unpack(">d", bytes.fromhex(value["bits"]))[0]
    if isinstance(value, dict) and value.get("kind") == "enum":
        literals = ENUMS.get(TYPE_OF(cls, feature) or "", [])
        index = value["literal"]["path"][-1]
        return literals[index] if index < len(literals) else value
    if isinstance(value, list):
        return [plain(v, cls, feature) for v in value]
    return value


def pilot_objects(path):
    with open(path, encoding="utf-8") as stream:
        data = json.load(stream)
    objects = {}
    roots = {}
    for resource in data["resources"]:
        name = resource["resource"]
        for o in resource["objects"]:
            objects[(name, o["fragment"])] = o
        roots[name] = [(name, "/")]
    return data, objects, roots


def frozen_objects(path):
    with open(path, encoding="utf-8") as stream:
        data = json.load(stream)
    objects = {e["id"]: e for e in data["elements"]}
    roots = {unquote(uri): list(record.get("roots", [])) for uri, record in data["resources"].items()}
    return objects, roots


def pilot_children(objects, key, pilot):
    """Containment children by feature: refs whose fragment extends this one."""
    name, fragment = key
    base = "/" if fragment == "/" else fragment + "/"
    children = {}
    for feature, value in pilot["values"].items():
        items = value if isinstance(value, list) else [value]
        kids = []
        for item in items:
            if not isinstance(item, dict) or item.get("resource") != name:
                continue
            f = item.get("fragment") or ""
            prefix = ("//@" if fragment == "/" else base + "@") + feature
            if f.startswith(prefix):
                kids.append((name, f))
        if kids:
            children[feature] = kids
    return children


def pair_objects(pilot, pilot_roots, frozen, frozen_roots, note=None):
    """Pair objects by walking both containment trees in step from each
    resource root. Returns {(resource, fragment): frozen id}. `note(resource,
    kind, detail)` receives missing resources, root count and containment
    count differences."""
    note = note or (lambda resource, kind, detail: None)
    pairs = {}

    def walk(pkey, fid):
        if pkey not in pilot or fid not in frozen or pkey in pairs:
            return
        pairs[pkey] = fid
        p, f = pilot[pkey], frozen[fid]
        for feature, kids in pilot_children(pilot, pkey, p).items():
            ours = f["values"].get(feature)
            ours = ours if isinstance(ours, list) else ([ours] if ours else [])
            if len(ours) != len(kids):
                note(pkey[0], "containment count", f"{pkey[0]} {pkey[1]} .{feature}: pilot {len(kids)} ours {len(ours)}")
            for pk, fk in zip(kids, ours):
                if isinstance(fk, str) and fk in frozen:
                    walk(pk, fk)

    for name, proots in pilot_roots.items():
        froots = frozen_roots.get(name, [])
        if not froots:
            note(name, "missing resource", name)
            continue
        if len(froots) != len(proots):
            note(name, "root count", f"{name}: pilot {len(proots)} ours {len(froots)}")
        for pk, fk in zip(proots, froots):
            walk(pk, fk)
    return pairs


def compare_diagnostics(pilot_data, frozen_data, pairs):
    """Pair a multiset by paired target, severity and exact code (or message
    when either producer has no code). Spans are evidence, not identities.
    Missing exports are refused: they are not evidence of zero violations.
    Unpaired or absent targets never match, including two absent targets.
    """
    pilot = []
    for resource in pilot_data["resources"]:
        if "diagnostics" not in resource:
            raise ValueError("Pilot export lacks diagnostics; rerun PilotOracle")
        pilot.extend(resource["diagnostics"])
    if "diagnostics" not in frozen_data:
        raise ValueError("Intersect export lacks diagnostics; rerun frozen_kerml_load")
    frozen = frozen_data["diagnostics"]
    remaining = set(range(len(frozen)))
    pending = list(range(len(pilot)))
    matched = []
    # Reserve exact-code matches before allowing a codeless message match to
    # consume the same diagnostic. Preserve multiplicity and producer order.
    for use_code in (True, False):
        unmatched = []
        for index in pending:
            diagnostic = pilot[index]
            target = diagnostic.get("target") or {}
            mapped = pairs.get((target.get("resource"), target.get("fragment")))
            found = None
            if mapped is not None:
                for i in sorted(remaining):
                    other = frozen[i]
                    if other.get("target") != mapped or diagnostic["severity"] != other["severity"]:
                        continue
                    code, other_code = diagnostic.get("code"), other.get("code")
                    if use_code:
                        same = code and other_code and code == other_code
                    else:
                        same = (not code or not other_code) and diagnostic.get("message") and diagnostic["message"] == other.get("message")
                    if same:
                        found = i
                        break
            if found is None:
                unmatched.append(index)
            else:
                remaining.remove(found)
                matched.append({"pilot": diagnostic, "intersect": frozen[found]})
        pending = unmatched
    pilot_only = [pilot[i] for i in pending]
    intersect_only = [frozen[i] for i in sorted(remaining)]
    return {
        "pilot": len(pilot), "intersect": len(frozen),
        "pilot_targets_paired": sum(
            ((d.get("target") or {}).get("resource"),
             (d.get("target") or {}).get("fragment")) in pairs for d in pilot),
        "matched": len(matched), "pilot_only": len(pilot_only),
        "intersect_only": len(intersect_only),
        "matches": matched, "pilot_only_diagnostics": pilot_only,
        "intersect_only_diagnostics": intersect_only,
    }


def compare(pilot_path, frozen_path, deviations_path=None, *, include_pairs=False):
    """Compare exports, retaining separate equal, allowed and unexplained counts."""
    with open(deviations_path or Path(__file__).with_name("deviations.json"), encoding="utf-8") as stream:
        deviations = json.load(stream)
    allowed = {}
    for entry in deviations:
        if not all(isinstance(entry.get(k), str) and entry[k].strip()
                   for k in ("class", "feature", "reason")):
            raise ValueError("Each deviation needs a class, feature and reason")
        key = (entry["class"], entry["feature"])
        if key in allowed:
            raise ValueError(f"Duplicate deviation: {key}")
        objects = entry.get("objects")
        if objects is not None and not (isinstance(objects, list) and objects and all(
                isinstance(o, list) and len(o) == 2 and all(isinstance(x, str) for x in o)
                for o in objects)):
            raise ValueError(f"Deviation objects must be [resource, fragment] pairs: {key}")
        # Without objects a deviation covers the feature on every object of
        # the class; with them, only on those Pilot objects.
        allowed[key] = None if objects is None else {tuple(o) for o in objects}
    pilot_data, pilot, pilot_roots = pilot_objects(pilot_path)
    frozen, frozen_roots = frozen_objects(frozen_path)
    with open(frozen_path, encoding="utf-8") as stream:
        frozen_data = json.load(stream)

    pairs = {}
    problems = Counter()
    examples = {}
    per_file = {}
    resource, cls, feature, owner = None, None, None, None

    def counts(name):
        return per_file.setdefault(name, Counter())

    def note(kind, detail):
        problems[kind] += 1
        examples.setdefault(kind, detail)
        listed = (cls, feature) in allowed and (allowed[(cls, feature)] is None
                                                or owner in allowed[(cls, feature)])
        bucket = "allow_listed_differences" if listed else "unexplained_differences"
        counts(resource)[bucket] += 1

    def pairing_note(name, kind, detail):
        nonlocal resource
        resource = name
        note(kind, detail)

    for name in pilot_roots:
        counts(name)
    pairs.update(pair_objects(pilot, pilot_roots, frozen, frozen_roots, pairing_note))

    reverse = {v: k for k, v in pairs.items()}
    parent = {}
    for fid, f in frozen.items():
        for v in f["values"].values():
            for child in v if isinstance(v, list) else [v]:
                if isinstance(child, str) and child in frozen:
                    parent.setdefault(child, fid)
    for pkey in pilot:
        counts(pkey[0])["pilot_objects"] += 1
        if pkey not in pairs:
            resource, cls, feature = pkey[0], None, None
            counts(resource)["unpaired_pilot"] += 1
            note("unpaired pilot object", str(pkey))
    for fid, obj in frozen.items():
        resource = unquote(obj.get("resource", fid.split("#")[0]))
        counts(resource)["frozen_objects"] += 1
        if fid not in reverse:
            cls, feature = None, None
            counts(resource)["unpaired_frozen"] += 1
            note("unpaired frozen object", fid)
    for pkey, fid in pairs.items():
        resource, cls, feature, owner = pkey[0], None, None, pkey
        fields = counts(resource)
        fields["paired_objects"] += 1
        p, f = pilot[pkey], frozen[fid]
        if p["class"] != f["class"]:
            note("class", f"{pkey[1]}: pilot {p['class']} ours {f['class']}")
            continue
        for name in sorted(set(p["values"]) | set(f["values"])):
            cls, feature = p["class"], name
            if name in SKIP:
                continue
            pv, fv = p["values"].get(name), plain(f["values"].get(name), p["class"], name)
            if name not in p["values"] and (name in CONTAINERS or fv == parent.get(fid)):
                continue  # a parent pointer; the Pilot export omits containers
            if name not in p["values"]:
                fields["compared"] += 1
                # Unset in the Pilot: ours is unset too, or holds the default.
                if fv in (None, []) or fv == DEFAULTS.get((p["class"], name)):
                    fields["equal"] += 1
                else:
                    note(f"ours only {p['class']}.{name}", f"{pkey[1]}: ours {fv!r}")
                continue
            fields["compared"] += 1
            if pv is None and fv in (None, [], False, ""):
                fields["equal"] += 1
                continue
            if isinstance(pv, (dict, list)) and (pv == [] or isinstance(pv, dict) or isinstance(pv[0], dict)):
                # References: map Pilot targets through the pairing.
                items = pv if isinstance(pv, list) else [pv]
                mapped = []
                for item in items:
                    if "unresolved" in item:
                        mapped.append("unresolved:" + item["unresolved"])
                    else:
                        mapped.append(pairs.get((item["resource"], item["fragment"]), "unpaired"))
                ours = fv if isinstance(fv, list) else ([fv] if fv is not None else [])
                same = mapped == ours if isinstance(pv, list) else (mapped[0] == fv)
                fields["references_compared"] += 1
                if same:
                    fields["references_agreeing"] += 1
                    fields["equal"] += 1
                else:
                    note(f"reference {p['class']}.{name}", f"{pkey[0]} {pkey[1]}: pilot {mapped[:3]} ours {ours[:3]}")
            else:
                if pv == fv:
                    fields["equal"] += 1
                else:
                    note(f"attribute {p['class']}.{name}", f"{pkey[0]} {pkey[1]}: pilot {pv!r} ours {fv!r}")

    keys = ("pilot_objects", "frozen_objects", "paired_objects", "unpaired_pilot",
            "unpaired_frozen", "fields_compared", "fields_equal", "references_compared",
            "references_agreeing", "allow_listed_differences", "unexplained_differences")
    # Diagnostics are compared only when both exports carry them; an export
    # without them is reported as not compared, never as zero diagnostics.
    has_pilot = all("diagnostics" in r for r in pilot_data["resources"])
    if has_pilot and "diagnostics" in frozen_data:
        diagnostics = compare_diagnostics(pilot_data, frozen_data, pairs)
    else:
        diagnostics = "not compared: an export lacks diagnostics"
    totals = Counter()
    for row in per_file.values():
        row["fields_compared"] = row.pop("compared", 0)
        row["fields_equal"] = row.pop("equal", 0)
        for key in keys:
            row.setdefault(key, 0)
        totals.update(row)
    result = {**{key: totals[key] for key in keys},
            "differences": dict(problems.most_common()), "examples": examples,
            "resources": dict(sorted(per_file.items())),
            "diagnostics": diagnostics,
            "frozen_graph_stage": frozen_data.get("graph_stage", "unspecified")}
    if include_pairs:
        result["pairs"] = [
            {"pilot": {"resource": resource, "fragment": fragment}, "intersect": identity}
            for (resource, fragment), identity in sorted(pairs.items())]
    return result


def main():
    summary = compare(sys.argv[1], sys.argv[2])
    print("| Resource | Objects paired | Fields compared | Fields equal | References agreeing | Allow-listed | Unexplained |")
    print("|---|---:|---:|---:|---:|---:|---:|")
    for name, row in summary["resources"].items():
        print(f"| {name} | {row['paired_objects']} | {row['fields_compared']} | "
              f"{row['fields_equal']} | {row['references_agreeing']}/{row['references_compared']} | "
              f"{row['allow_listed_differences']} | {row['unexplained_differences']} |")
    print(f"allow-listed differences: {summary['allow_listed_differences']}")
    print(f"unexplained differences: {summary['unexplained_differences']}")
    diagnostics = summary["diagnostics"]
    if isinstance(diagnostics, dict):
        print("diagnostics: " + ", ".join(f"{k} {v}" for k, v in diagnostics.items() if isinstance(v, int)))
    else:
        print(f"diagnostics: {diagnostics}")
    if len(sys.argv) > 3:
        with open(sys.argv[3], "w", encoding="utf-8") as stream:
            json.dump(summary, stream, indent=1)


if __name__ == "__main__":
    main()
