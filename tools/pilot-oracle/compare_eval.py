"""Handwritten expression comparison, independent verification only.

The comparison core accepts an existing containment pairing; it never pairs
objects by names, source spans, site order, or a second tree traversal.
The command-line bridge uses compare.py's optional export of that same map.
"""
import argparse
import inspect
import json
import re
import sys
from collections import Counter
from pathlib import Path
from urllib.parse import unquote

from compare import compare as compare_models


MODES = ("model_level", "gated", "extended")
ISOLATION = "EMF ChangeRecorder undo per evaluability query and mode; captured source identities"
COUNTS = ("sites", "agree", "allowed", "differ", "pilot_only", "intersect_only")
TRIG_FUNCTIONS = frozenset("TrigFunctions::" + name for name in
                         ("sin", "cos", "tan", "cot", "arcsin", "arccos", "arctan", "deg", "rad"))


def reference_key(reference):
    if not isinstance(reference, dict) or not all(
            isinstance(reference.get(k), str) and reference[k] for k in ("resource", "fragment")):
        raise ValueError("Pilot identity needs a nonempty resource and fragment")
    return reference["resource"], reference["fragment"]


def canonical_bits(bits):
    if not isinstance(bits, str) or not re.fullmatch(r"[0-9a-fA-F]{16}", bits):
        raise ValueError("A real needs exactly 16 hexadecimal bit digits")
    number = int(bits, 16)
    if number & 0x7ff0000000000000 == 0x7ff0000000000000 and number & 0xfffffffffffff:
        return 0x7ff8000000000000
    return number


def validate_deviations(entries):
    """Only exact, mode-specific selectors; no wildcard or blanket entries."""
    if not isinstance(entries, list):
        raise ValueError("Deviations must be an array")
    selectors = set()
    for entry in entries:
        if not isinstance(entry, dict) or entry.get("mode") not in MODES:
            raise ValueError("Every deviation needs one exact evaluation mode")
        if not isinstance(entry.get("reason"), str) or not entry["reason"].strip():
            raise ValueError("Every deviation needs a reason")
        selected = [key for key in ("function", "class", "sites") if key in entry]
        extra = set(entry) & {"max_real_ulp", "exception_messages"}
        if len(selected) != 1 or len(extra) > 1 or set(entry) != {"mode", "reason", *selected, *extra}:
            raise ValueError("Use exactly one function, class or sites selector")
        if "max_real_ulp" in extra and (entry.get("function") not in TRIG_FUNCTIONS or entry["mode"] != "extended"
                      or type(entry["max_real_ulp"]) is not int or entry["max_real_ulp"] != 1):
            raise ValueError("Only an extended trig function can allow exactly one real ulp")
        if "exception_messages" in extra:
            messages = entry["exception_messages"]
            if ("sites" not in entry or not isinstance(messages, dict)
                    or set(messages) != {"pilot", "intersect"}
                    or not all(isinstance(message, str) and message.strip() for message in messages.values())):
                raise ValueError("Exception allowances need exact sites and both exact messages")
        selector = selected[0]
        value = entry[selector]
        if selector == "sites":
            if not isinstance(value, list) or not value:
                raise ValueError("A sites selector cannot be empty")
            for site in value:
                if not isinstance(site, list) or len(site) != 2 or not all(
                        isinstance(part, str) and part and part != "*" for part in site):
                    raise ValueError("Sites must be exact [resource, fragment] pairs")
            if len({tuple(site) for site in value}) != len(value):
                raise ValueError("Duplicate site in deviation")
            value = tuple(sorted(tuple(site) for site in value))
        elif not isinstance(value, str) or not value.strip() or value == "*":
            raise ValueError("A deviation selector cannot be empty or a wildcard")
        if selector == "class" and not re.fullmatch(r"[A-Za-z_][A-Za-z_0-9]*", value):
            raise ValueError("A class selector must be one metaclass name")
        key = (entry["mode"], selector, value)
        if key in selectors:
            raise ValueError("Duplicate deviation selector")
        selectors.add(key)
    return entries


def matching_deviations(entries, mode, site):
    key = reference_key(site)
    return [i for i, entry in enumerate(entries) if entry["mode"] == mode and (
        ("sites" in entry and list(key) in entry["sites"])
        or ("class" in entry and entry["class"] == site.get("class"))
        or ("function" in entry and entry["function"] == site.get("function")))]


def mapped_identity(reference, pairs):
    key = reference_key(reference)
    return pairs.get(key), key


def deviation_allows(entry, issues):
    """Constrained allowances cannot hide other differences at the same site."""
    if "exception_messages" in entry:
        return issues == [{"kind": "exception", **entry["exception_messages"]}]
    if "max_real_ulp" not in entry:
        return True
    for issue in issues:
        if issue["kind"] != "value":
            return False
        a, b = issue["pilot"], issue["intersect"]
        if a[0] != "real" or b[0] != "real":
            return False
        x, y = a[1], b[1]
        if (x & 0x7ff0000000000000 == 0x7ff0000000000000
                or y & 0x7ff0000000000000 == 0x7ff0000000000000
                or (x ^ y) & 0x8000000000000000 or abs(x - y) > 1):
            return False
    return bool(issues)


def value_key(value, pilot, pairs):
    if not isinstance(value, dict):
        raise ValueError("Each value must be an object")
    kind = value.get("kind")
    if kind in ("bool", "int", "string"):
        expected = {"bool": bool, "int": int, "string": str}[kind]
        if type(value.get("value")) is not expected:
            raise ValueError(f"Invalid {kind} value")
        return (kind, value["value"])
    if kind == "real":
        return (kind, canonical_bits(value.get("bits")))
    if kind == "infinity":
        return (kind,)
    if kind in ("element", "unevaluated", "metaclass"):
        if not isinstance(value.get("class"), str) or not value["class"]:
            raise ValueError("Element and metaclass values need a class")
        if pilot:
            identity, original = mapped_identity(value["of"] if kind == "metaclass" else value, pairs)
            if identity is None:
                return ("unpaired-value", original)
        else:
            identity = value.get("of" if kind == "metaclass" else "id")
            if not isinstance(identity, str) or not identity:
                raise ValueError("Native element value needs an identity")
        return ("metaclass" if kind == "metaclass" else "element", identity)
    if pilot and kind == "synthetic":
        if not isinstance(value.get("class"), str) or not value["class"]:
            raise ValueError("Synthetic value needs a class")
        return (kind, value["class"])
    raise ValueError(f"Unknown {'Pilot' if pilot else 'native'} value kind: {kind!r}")


def outcome_keys(outcome, pilot, pairs):
    if not isinstance(outcome, dict) or outcome.get("outcome") not in (
            "values", "not-evaluable", "exception"):
        raise ValueError("Unknown or missing outcome")
    if outcome["outcome"] == "values":
        if not isinstance(outcome.get("values"), list):
            raise ValueError("A values outcome needs an ordered values array")
        return [value_key(value, pilot, pairs) for value in outcome["values"]]
    if outcome["outcome"] == "exception" and not isinstance(outcome.get("message"), str):
        raise ValueError("An exception outcome needs a message")
    return []


def evaluability_failed(row):
    if type(row.get("evaluable")) is bool:
        return False
    error = row.get("evaluable_error")
    if (row.get("evaluable") is None and isinstance(error, dict)
            and error.get("outcome") == "exception" and isinstance(error.get("message"), str)):
        return True
    raise ValueError("Evaluability needs a Boolean or an explicit exception")


def outcome_differences(pilot, native, pairs):
    """Return detailed differences and whether synthetic classes permit a waiver."""
    issues = []
    waiver_possible = True
    # Validate every value even when outcome kinds/lengths differ.
    pkeys, nkeys = outcome_keys(pilot, True, pairs), outcome_keys(native, False, pairs)
    nvalues = native.get("values", [])
    if pilot["outcome"] != native["outcome"]:
        issues.append({"kind": "outcome"})
    elif pilot["outcome"] == "exception":
        if pilot["message"] != native["message"]:
            issues.append({"kind": "exception", "pilot": pilot["message"], "intersect": native["message"]})
    elif pilot["outcome"] == "values":
        if len(pkeys) != len(nkeys):
            issues.append({"kind": "length", "pilot": len(pkeys), "intersect": len(nkeys)})
        for index, (pkey, nkey) in enumerate(zip(pkeys, nkeys)):
            if pkey[0] == "synthetic":
                compatible = (nvalues[index]["kind"] in ("element", "metaclass", "unevaluated")
                              and pkey[1] == nvalues[index].get("class"))
                issues.append({"kind": "synthetic" if compatible else "synthetic-class", "index": index})
                waiver_possible &= compatible
            elif pkey != nkey:
                issues.append({"kind": "value", "index": index, "pilot": pkey, "intersect": nkey})
    return issues, waiver_possible


def site_differences(pilot, native, mode, pairs):
    issues, waiver_possible = outcome_differences(pilot[mode], native["outcome"], pairs)
    for side, row in (("pilot", pilot), ("intersect", native)):
        if evaluability_failed(row):
            issues.append({"kind": "evaluable-error", "side": side})
    if pilot.get("evaluable") != native.get("evaluable"):
        issues.append({"kind": "evaluable"})
    target = pilot.get("target")
    mapped = mapped_identity(target, pairs)[0] if target is not None else None
    native_target = native["site"].get("target")
    if (target is not None and mapped is None) or mapped != native_target:
        issues.append({"kind": "target"})
        waiver_possible = False
    if pilot.get("kind") != native["site"].get("kind"):
        issues.append({"kind": "site-kind"})
        waiver_possible = False
    return issues, waiver_possible


def counts():
    return Counter({key: 0 for key in COUNTS})


def compare_reports(pilot, native_modes, pairs, deviations=None, excluded=None):
    """Compare all three modes using a supplied, injective G5 containment map.

    `agree` counts exact agreement; `allowed` stays separate. Resource totals
    count distinct sites, while each mode counts its own site observations.
    Missing sites cannot be waived. This does not attest corpus completeness
    or user review of deviations, so it does not itself declare gate GE passed.
    """
    deviations = validate_deviations([] if deviations is None else deviations)
    if pilot.get("observation_isolation") != ISOLATION:
        raise ValueError("Rerun PilotEvalOracle with isolated observations; sequential reports can be contaminated")
    if set(native_modes) != set(MODES):
        raise ValueError("Supply all three native modes")
    for key, identity in pairs.items():
        if (not isinstance(key, tuple) or len(key) != 2 or not all(isinstance(x, str) and x for x in key)
                or not isinstance(identity, str) or not identity):
            raise ValueError("Invalid containment pairing")
    if len(set(pairs.values())) != len(pairs):
        raise ValueError("Containment pairing is not injective")
    excluded_by_resource = {}
    for entry in excluded or []:
        if not isinstance(entry, dict) or not all(isinstance(entry.get(k), str) and entry[k].strip()
                                                for k in ("resource", "reason")):
            raise ValueError("An exclusion needs a resource and load-failure reason")
        if entry["resource"] in excluded_by_resource:
            raise ValueError("Duplicate resource exclusion")
        excluded_by_resource[entry["resource"]] = {**entry, "pilot_sites": 0}
    if not isinstance(pilot.get("sites"), list) or not isinstance(pilot.get("resources"), list):
        raise ValueError("Pilot report needs sites and selected resources")
    resources = {}

    def resource_counts(name):
        if name not in resources:
            resources[name] = {**counts(), "modes": {mode: counts() for mode in MODES}}
        return resources[name]

    for resource in pilot["resources"]:
        if not isinstance(resource, str) or not resource:
            raise ValueError("Invalid selected resource")
        if resource not in excluded_by_resource:
            resource_counts(resource)
    if len(set(pilot["resources"])) != len(pilot["resources"]):
        raise ValueError("Duplicate selected resource")
    psites = {}
    for site in pilot["sites"]:
        key = reference_key(site)
        if not all(mode in site for mode in MODES):
            raise ValueError("Every Pilot site needs all three outcomes")
        evaluability_failed(site)
        for mode in MODES:
            outcome_keys(site[mode], True, pairs)
        if key in psites:
            raise ValueError("Duplicate Pilot site")
        if key[0] not in pilot["resources"]:
            raise ValueError("Pilot site is outside the selected resources")
        psites[key] = site
    nsites = {}
    native_resources = {}
    for mode in MODES:
        rows = {}
        if not isinstance(native_modes[mode], list):
            raise ValueError("Native mode input must be an array of JSONL rows")
        for row in native_modes[mode]:
            if not isinstance(row, dict) or not isinstance(row.get("site"), dict):
                raise ValueError("Native row needs a site object")
            evaluability_failed(row)
            outcome_keys(row.get("outcome"), False, pairs)
            site = row.get("site", {})
            identity, resource = site.get("expression"), site.get("resource")
            if not isinstance(identity, str) or not identity or not isinstance(resource, str) or not resource:
                raise ValueError("Native site needs an expression identity and resource")
            resource = unquote(resource)
            if resource in excluded_by_resource:
                raise ValueError("Cannot exclude a resource with native evaluation results")
            if identity in rows:
                raise ValueError("Duplicate native site in one mode")
            if identity in native_resources and native_resources[identity] != resource:
                raise ValueError("Native site resource differs across modes")
            rows[identity] = row
            native_resources[identity] = resource
        nsites[mode] = rows
    differences, used, deviation_hits = [], set(), Counter()

    def detail(mode, psite, nsite, issues, status, matching=()):
        differences.append({"mode": mode, "status": status, "issues": issues,
            "pilot_site": ({k: psite.get(k) for k in ("resource", "fragment", "class", "kind", "target", "function")}
                           if psite is not None else None),
            "intersect_site": nsite["site"] if nsite is not None else None,
            "pilot_location": psite.get("source") if psite is not None else None,
            "intersect_location": ({k: nsite["site"].get(k) for k in ("resource", "line", "column", "span")}
                                   if nsite is not None else None),
            "pilot": ({"evaluable": psite.get("evaluable"), "evaluable_error": psite.get("evaluable_error"),
                       "outcome": psite.get(mode)} if psite is not None else None),
            "intersect": ({"evaluable": nsite.get("evaluable"), "evaluable_error": nsite.get("evaluable_error"),
                           "outcome": nsite.get("outcome")} if nsite is not None else None),
            "deviations": list(matching)})

    for key, psite in psites.items():
        if key[0] in excluded_by_resource:
            excluded_by_resource[key[0]]["pilot_sites"] += 1
            continue
        row = resource_counts(key[0])
        row["sites"] += 1
        identity = pairs.get(key)
        present = identity is not None and identity in native_resources
        if present and native_resources[identity] != key[0]:
            raise ValueError("Paired site resources differ")
        if present:
            used.add(identity)
        statuses = []
        for mode in MODES:
            counter = row["modes"][mode]
            counter["sites"] += 1
            nsite = nsites[mode].get(identity)
            if nsite is None:
                status = "pilot_only"
                detail(mode, psite, None, [{"kind": "unpaired-site"}], status)
            else:
                issues, waiver_possible = site_differences(psite, nsite, mode, pairs)
                matching = matching_deviations(deviations, mode, psite) if issues and waiver_possible else []
                matching = [i for i in matching if deviation_allows(deviations[i], issues)]
                status = "allowed" if matching else "differ" if issues else "agree"
                if issues:
                    detail(mode, psite, nsite, issues, status, matching)
                    deviation_hits.update(matching)
            counter[status] += 1
            statuses.append(status)
        status = ("pilot_only" if not present else "differ" if any(s in ("differ", "pilot_only") for s in statuses)
                  else "allowed" if "allowed" in statuses else "agree")
        row[status] += 1
    for identity, resource in native_resources.items():
        if identity in used:
            continue
        row = resource_counts(resource)
        row["sites"] += 1
        row["intersect_only"] += 1
        for mode in MODES:
            if identity in nsites[mode]:
                row["modes"][mode]["sites"] += 1
                row["modes"][mode]["intersect_only"] += 1
                detail(mode, None, nsites[mode][identity], [{"kind": "unpaired-site"}], "intersect_only")
    totals, modes = counts(), {mode: counts() for mode in MODES}
    for row in resources.values():
        totals.update({key: row[key] for key in COUNTS})
        for mode in MODES:
            modes[mode].update(row["modes"][mode])
    if not isinstance(pilot.get("diagnostics"), list) or not all(isinstance(d, dict) for d in pilot["diagnostics"]):
        raise ValueError("Pilot report must record load diagnostics")
    diagnostics = [d for d in pilot["diagnostics"] if d.get("resource") not in excluded_by_resource]
    return {"totals": dict(totals), "modes": {m: dict(v) for m, v in modes.items()},
            "resources": dict(sorted(resources.items())), "differences": differences,
            "excluded": list(excluded_by_resource.values()), "load_diagnostics": diagnostics,
            "deviations": [{**d, "hits": deviation_hits[i]} for i, d in enumerate(deviations)],
            "comparison_satisfied": not diagnostics and not any(totals[k] for k in ("differ", "pilot_only", "intersect_only")),
            "gate": "not assessed: requires corpus coverage and user review of deviations"}


def read_jsonl(path):
    """Reject a truncated/malformed row instead of silently losing a site."""
    with open(path, encoding="utf-8-sig") as stream:
        return [json.loads(line, parse_constant=reject_constant) for line in stream if line.strip()]


def reject_constant(value):
    raise ValueError(f"Nonstandard JSON constant {value}; real values must use bits")


def load_pairing(pilot_model, native_model):
    """Use compare.py's traversal; do not substitute another pairing algorithm."""
    if "include_pairs" not in inspect.signature(compare_models).parameters:
        raise RuntimeError("compare.py does not expose its pairing map; the approved include_pairs API is required")
    report = compare_models(pilot_model, native_model, include_pairs=True)
    pairs = {}
    for row in report["pairs"]:
        key = reference_key(row["pilot"])
        if key in pairs:
            raise ValueError("Duplicate exported containment pair")
        pairs[key] = row["intersect"]
    return pairs, {key: value for key, value in report.items() if key != "pairs"}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pilot-eval", required=True)
    parser.add_argument("--pilot-model", required=True)
    parser.add_argument("--native-model", required=True)
    for mode in MODES:
        parser.add_argument("--" + mode.replace("_", "-"), required=True)
    parser.add_argument("--deviations", default=str(Path(__file__).with_name("eval-deviations.json")))
    parser.add_argument("--excluded", help="JSON array of explicit resource/load-failure reasons")
    parser.add_argument("--out", required=True)
    args = parser.parse_args(argv)

    def read(path):
        with open(path, encoding="utf-8-sig") as stream:
            return json.load(stream, parse_constant=reject_constant)

    try:
        pairs, model_report = load_pairing(args.pilot_model, args.native_model)
        report = compare_reports(read(args.pilot_eval),
            {mode: read_jsonl(getattr(args, mode)) for mode in MODES}, pairs,
            read(args.deviations), read(args.excluded) if args.excluded else [])
        report["model_pairing"] = model_report
        report["inputs"] = vars(args)
        with open(args.out, "w", encoding="utf-8") as stream:
            json.dump(report, stream, indent=2, allow_nan=False)
        print(json.dumps(report["totals"]))
        return 0 if report["comparison_satisfied"] else 1
    except (OSError, ValueError, KeyError, RuntimeError) as error:
        print(f"compare_eval: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
