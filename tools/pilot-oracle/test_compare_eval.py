"""Quick, inline protocol fixtures; no parser, pack, or JVM required."""
from copy import deepcopy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import compare_eval as ce


def values(*items):
    return {"outcome": "values", "values": list(items)}


class EvaluationComparison(unittest.TestCase):
    def test_exported_pairing_reuses_containment_and_preserves_default_report(self):
        resource = "a file.kerml"
        child = "//@ownedRelationship.0"
        pilot = {"resources": [{"resource": resource, "objects": [
            {"fragment": "/", "class": "Namespace", "values": {
                "ownedRelationship": [{"resource": resource, "fragment": child}]}},
            {"fragment": child, "class": "Membership", "values": {}},
        ]}]}
        native = {"resources": {"a%20file.kerml": {"roots": ["root"]}}, "elements": [
            {"id": "root", "resource": "a%20file.kerml", "class": "Namespace",
             "values": {"ownedRelationship": ["member"]}},
            {"id": "member", "resource": "a%20file.kerml", "class": "Membership", "values": {}},
        ]}
        with tempfile.TemporaryDirectory() as directory:
            p, n = Path(directory) / "pilot.json", Path(directory) / "native.json"
            p.write_text(json.dumps(pilot), encoding="utf-8")
            n.write_text(json.dumps(native), encoding="utf-8")
            ordinary = ce.compare_models(p, n)
            pairs, exported = ce.load_pairing(p, n)
        self.assertNotIn("pairs", ordinary)
        self.assertEqual(ordinary, exported)
        self.assertEqual(pairs, {(resource, "/"): "root", (resource, child): "member"})

    def setUp(self):
        self.key = ("a file.kerml", "/@expression.0")
        self.identity = "a%20file.kerml#8"
        self.pairs = {self.key: self.identity}
        self.psite = {"resource": self.key[0], "fragment": self.key[1], "class": "OperatorExpression",
                      "kind": "feature-value", "target": None, "evaluable": True,
                      "source": {"start": 10, "end": 15, "line": 2, "offset_encoding": "utf-16"}}
        for mode in ce.MODES:
            self.psite[mode] = values({"kind": "int", "value": 7})
        self.pilot = {"resources": [self.key[0], "empty.kerml"], "sites": [self.psite], "diagnostics": [],
                      "observation_isolation": ce.ISOLATION}
        self.nrow = {"site": {"expression": self.identity, "resource": "a%20file.kerml", "kind": "feature-value",
                              "target": None, "line": 2, "column": 4, "span": [10, 15]},
                     "evaluable": True, "outcome": values({"kind": "int", "value": 7})}
        self.native = {mode: [deepcopy(self.nrow)] for mode in ce.MODES}

    def compare(self, deviations=None, excluded=None):
        report = ce.compare_reports(self.pilot, self.native, self.pairs, deviations, excluded)
        for key in ce.COUNTS:
            self.assertEqual(report["totals"][key], sum(row[key] for row in report["resources"].values()))
        for mode in ce.MODES:
            for key in ce.COUNTS:
                self.assertEqual(report["modes"][mode][key], sum(row["modes"][mode][key] for row in report["resources"].values()))
        self.assertEqual(report["totals"]["sites"], sum(report["totals"][key] for key in ce.COUNTS if key != "sites"))
        return report

    def outcomes(self, pilot, native):
        for mode in ce.MODES:
            self.psite[mode] = deepcopy(pilot)
            self.native[mode][0]["outcome"] = deepcopy(native)

    def deviation(self, **selector):
        return {"mode": "extended", "reason": "Specific observed test difference", **selector}

    def test_exact_match_and_zero_site_resource(self):
        report = self.compare()
        self.assertEqual(report["totals"]["agree"], 1)
        self.assertEqual(report["resources"]["empty.kerml"]["sites"], 0)
        self.assertTrue(report["comparison_satisfied"])
        self.assertIn("not assessed", report["gate"])

    def test_old_sequential_oracle_reports_cannot_claim_agreement(self):
        for marker in (None, "unknown"):
            self.pilot["observation_isolation"] = marker
            with self.assertRaisesRegex(ValueError, "isolated observations"):
                self.compare()
        del self.pilot["observation_isolation"]
        with self.assertRaisesRegex(ValueError, "isolated observations"):
            self.compare()

    def test_scalars_preserve_types_and_sequence_order(self):
        for p, n in (({"kind": "bool", "value": True}, {"kind": "int", "value": 1}),
                     ({"kind": "int", "value": 1}, {"kind": "real", "bits": "3ff0000000000000"}),
                     ({"kind": "string", "value": "é\n"}, {"kind": "string", "value": "e\n"})):
            with self.subTest(p=p, n=n):
                self.outcomes(values(p), values(n))
                self.assertEqual(self.compare()["totals"]["differ"], 1)
        a, b = {"kind": "int", "value": 1}, {"kind": "infinity"}
        self.outcomes(values(a, b), values(b, a))
        self.assertEqual(self.compare()["totals"]["differ"], 1)

    def test_real_nan_canonicalization_ignores_text(self):
        self.outcomes(values({"kind": "real", "bits": "fff0000000000001", "text": "irrelevant"}),
                      values({"kind": "real", "bits": "7ff8000000001234", "text": "NaN"}))
        self.assertEqual(self.compare()["totals"]["agree"], 1)

    def test_signed_zero_and_one_ulp_difference_remain_different(self):
        for pbits, nbits in (("8000000000000000", "0000000000000000"),
                             ("3ff0000000000000", "3ff0000000000001")):
            self.outcomes(values({"kind": "real", "bits": pbits}), values({"kind": "real", "bits": nbits}))
            self.assertEqual(self.compare()["totals"]["differ"], 1)

    def test_trig_tolerance_is_limited_to_one_finite_ulp_in_extended_mode(self):
        self.psite["function"] = "TrigFunctions::sin"
        tolerance = self.deviation(function="TrigFunctions::sin", max_real_ulp=1)
        for a, b, allowed in (("3ff0000000000000", "3ff0000000000001", True),
                              ("bff0000000000000", "bff0000000000001", True),
                              ("3ff0000000000000", "3ff0000000000002", False),
                              ("8000000000000000", "0000000000000000", False),
                              ("7fefffffffffffff", "7ff0000000000000", False),
                              ("7ff0000000000000", "7ff8000000000000", False)):
            with self.subTest(a=a, b=b):
                self.outcomes(values({"kind": "real", "bits": a}), values({"kind": "real", "bits": b}))
                report = self.compare([tolerance])
                self.assertEqual(report["modes"]["extended"]["allowed"], int(allowed))
                self.assertEqual(report["modes"]["model_level"]["differ"], 1)
                self.assertEqual(report["modes"]["gated"]["differ"], 1)

    def test_trig_tolerance_cannot_hide_other_differences(self):
        self.psite["function"] = "TrigFunctions::cos"
        tolerance = self.deviation(function="TrigFunctions::cos", max_real_ulp=1)
        self.outcomes(values({"kind": "int", "value": 1}), values({"kind": "int", "value": 2}))
        self.assertEqual(self.compare([tolerance])["modes"]["extended"]["differ"], 1)
        self.outcomes(values({"kind": "real", "bits": "3ff0000000000000"}), values({"kind": "real", "bits": "3ff0000000000001"}))
        self.native["extended"][0]["evaluable"] = False
        self.assertEqual(self.compare([tolerance])["modes"]["extended"]["differ"], 1)
        for invalid in (self.deviation(function="DataFunctions::'+'", max_real_ulp=1),
                        self.deviation(function="TrigFunctions::sin", max_real_ulp=2),
                        self.deviation(function="TrigFunctions::sin", max_real_ulp=True),
                        {**tolerance, "mode": "gated"}):
            with self.assertRaises(ValueError):
                self.compare([invalid])

    def test_empty_sequence_is_not_not_evaluable(self):
        self.outcomes(values(), {"outcome": "not-evaluable", "reason": {"reason": "unsupported"}})
        self.assertEqual(self.compare()["totals"]["differ"], 1)

    def test_reasons_do_not_affect_not_evaluable(self):
        self.outcomes({"outcome": "not-evaluable"}, {"outcome": "not-evaluable", "reason": {"reason": "no-result"}})
        self.assertEqual(self.compare()["totals"]["agree"], 1)

    def test_element_and_unevaluated_use_paired_identity(self):
        self.pairs[("library.kerml", "/@feature.1")] = "library#20"
        self.outcomes(values({"kind": "element", "resource": "library.kerml", "fragment": "/@feature.1", "class": "Feature"}),
                      values({"kind": "unevaluated", "id": "library#20", "class": "Feature", "reason": {"reason": "no-value"}}))
        self.assertEqual(self.compare()["totals"]["agree"], 1)
        self.native["extended"][0]["outcome"]["values"][0]["id"] = "library#21"
        self.assertEqual(self.compare()["totals"]["differ"], 1)

    def test_metaclass_is_distinct_from_element(self):
        self.pairs[("model", "/part")] = "model#part"
        p = {"kind": "metaclass", "of": {"resource": "model", "fragment": "/part"}, "class": "PartUsage"}
        self.outcomes(values(p), values({"kind": "metaclass", "of": "model#part", "class": "PartUsage"}))
        self.assertEqual(self.compare()["totals"]["agree"], 1)
        self.outcomes(values(p), values({"kind": "element", "id": "model#part", "class": "PartUsage"}))
        self.assertEqual(self.compare()["totals"]["differ"], 1)

    def test_unpaired_element_value_never_equals_native(self):
        self.outcomes(values({"kind": "element", "resource": "missing", "fragment": "/", "class": "Feature"}),
                      values({"kind": "element", "id": "missing#/", "class": "Feature"}))
        report = self.compare()
        self.assertFalse(report["comparison_satisfied"])
        self.assertEqual(report["differences"][0]["issues"][0]["pilot"][0], "unpaired-value")

    def test_synthetic_requires_exact_class_and_explicit_deviation(self):
        self.outcomes(values({"kind": "synthetic", "class": "Feature"}),
                      values({"kind": "element", "id": "synthetic", "class": "Feature"}))
        self.assertEqual(self.compare()["modes"]["extended"]["differ"], 1)
        entry = self.deviation(sites=[list(self.key)])
        report = self.compare([entry])
        self.assertEqual(report["modes"]["extended"]["allowed"], 1)
        self.assertEqual(report["modes"]["extended"]["agree"], 0)
        self.native["extended"][0]["outcome"]["values"][0]["class"] = "Expression"
        self.assertEqual(self.compare([entry])["modes"]["extended"]["differ"], 1)

    def test_evaluability_difference_with_equal_values_is_reported(self):
        self.native["gated"][0]["evaluable"] = False
        report = self.compare()
        self.assertEqual(report["totals"]["differ"], 1)
        detail = report["differences"][0]
        self.assertEqual(detail["issues"], [{"kind": "evaluable"}])
        self.assertEqual(detail["pilot_location"]["line"], 2)
        self.assertEqual(detail["intersect_location"]["column"], 4)
        self.assertIn("outcome", detail["pilot"])
        self.assertIn("outcome", detail["intersect"])

    def test_unknown_evaluability_is_not_verified_agreement(self):
        self.psite["evaluable"] = None
        self.psite["evaluable_error"] = {"outcome": "exception", "message": "limit"}
        for mode in ce.MODES:
            self.native[mode][0]["evaluable"] = None
            self.native[mode][0]["evaluable_error"] = {"outcome": "exception", "message": "limit"}
        self.assertEqual(self.compare()["totals"]["differ"], 1)

    def test_exception_messages_are_not_all_equivalent(self):
        self.outcomes({"outcome": "exception", "message": "StackOverflowError"},
                      {"outcome": "exception", "message": "DepthLimit(256)"})
        self.assertEqual(self.compare()["totals"]["differ"], 1)
        self.outcomes({"outcome": "exception", "message": "same"}, {"outcome": "exception", "message": "same"})
        self.assertEqual(self.compare()["totals"]["agree"], 1)

    def test_exact_exception_allowance_is_site_and_mode_specific(self):
        self.outcomes({"outcome": "exception", "message": "java.lang.StackOverflowError"},
                      {"outcome": "exception", "message": "evaluation depth limit exceeded: 256"})
        entry = self.deviation(sites=[list(self.key)], exception_messages={
            "pilot": "java.lang.StackOverflowError", "intersect": "evaluation depth limit exceeded: 256"})
        report = self.compare([entry])
        self.assertEqual(report["modes"]["extended"]["allowed"], 1)
        self.assertEqual(report["modes"]["model_level"]["differ"], 1)
        self.assertEqual(report["modes"]["gated"]["differ"], 1)
        entry["sites"] = [[self.key[0], "/other"]]
        self.assertEqual(self.compare([entry])["modes"]["extended"]["differ"], 1)

    def test_exception_allowance_cannot_hide_new_errors_values_or_evaluability(self):
        messages = {"pilot": "java.lang.StackOverflowError", "intersect": "evaluation depth limit exceeded: 256"}
        entry = self.deviation(sites=[list(self.key)], exception_messages=messages)
        self.psite["extended"] = {"outcome": "exception", "message": messages["pilot"]}
        for outcome in ({"outcome": "exception", "message": "different failure"}, values(),
                        {"outcome": "not-evaluable"}):
            self.native["extended"][0]["outcome"] = outcome
            self.assertEqual(self.compare([entry])["modes"]["extended"]["differ"], 1)
        self.native["extended"][0]["outcome"] = {"outcome": "exception", "message": messages["intersect"]}
        self.native["extended"][0]["evaluable"] = False
        self.assertEqual(self.compare([entry])["modes"]["extended"]["differ"], 1)

    def test_exception_allowance_requires_explicit_sites_and_complete_messages(self):
        for selector, messages in (({"function": "ControlFunctions::implies"}, {"pilot": "x", "intersect": "y"}),
                                   ({"class": "OperatorExpression"}, {"pilot": "x", "intersect": "y"}),
                                   ({"sites": [list(self.key)]}, {"pilot": "x"}),
                                   ({"sites": [list(self.key)]}, {"pilot": "x", "intersect": ""})):
            with self.subTest(selector=selector, messages=messages), self.assertRaises(ValueError):
                self.compare([self.deviation(**selector, exception_messages=messages)])

    def test_deviation_selectors_are_exact_and_mode_specific(self):
        self.native["extended"][0]["outcome"] = values()
        self.psite["function"] = "SequenceFunctions::size"
        for selector in ({"function": "SequenceFunctions::size"}, {"class": "OperatorExpression"}, {"sites": [list(self.key)]}):
            report = self.compare([self.deviation(**selector)])
            self.assertEqual(report["totals"]["allowed"], 1)
            self.assertEqual(report["deviations"][0]["hits"], 1)
            self.assertTrue(report["comparison_satisfied"])
        for selector in ({"function": "Other::size"}, {"class": "InvocationExpression"}, {"sites": [[self.key[0], "/other"]]}):
            self.assertEqual(self.compare([self.deviation(**selector)])["totals"]["differ"], 1)
        self.native["gated"][0]["outcome"] = values()
        self.assertEqual(self.compare([self.deviation(sites=[list(self.key)])])["totals"]["differ"], 1)

    def test_missing_mode_row_cannot_be_waived(self):
        self.native["extended"] = []
        report = self.compare([self.deviation(sites=[list(self.key)])])
        self.assertEqual(report["totals"]["differ"], 1)
        self.assertEqual(report["modes"]["extended"]["pilot_only"], 1)
        self.assertEqual(report["deviations"][0]["hits"], 0)

    def test_unpaired_sites_are_counted_on_both_sides(self):
        self.pairs = {}
        report = self.compare()
        self.assertEqual(report["totals"]["sites"], 2)
        self.assertEqual(report["totals"]["pilot_only"], 1)
        self.assertEqual(report["totals"]["intersect_only"], 1)
        self.assertEqual(len(report["differences"]), 6)

    def test_target_pairing_and_site_kind_are_not_hidden_by_value_deviations(self):
        self.psite["target"] = {"resource": "r", "fragment": "/t"}
        self.pairs[("r", "/t")] = "target"
        for mode in ce.MODES:
            self.native[mode][0]["site"]["target"] = "target"
        self.assertEqual(self.compare()["totals"]["agree"], 1)
        self.native["extended"][0]["site"]["target"] = "other"
        self.assertEqual(self.compare([self.deviation(sites=[list(self.key)])])["modes"]["extended"]["differ"], 1)
        self.native["extended"][0]["site"]["target"] = "target"
        self.native["extended"][0]["site"]["kind"] = "result-expression"
        self.assertEqual(self.compare([self.deviation(sites=[list(self.key)])])["modes"]["extended"]["differ"], 1)

    def test_load_diagnostics_prevent_success(self):
        self.pilot["diagnostics"] = [{"resource": self.key[0], "message": "invalid"}]
        report = self.compare()
        self.assertEqual(report["totals"]["agree"], 1)
        self.assertFalse(report["comparison_satisfied"])

    def test_exclusions_are_explicit_and_cannot_hide_native_results(self):
        excluded = [{"resource": self.key[0], "reason": "Native parser rejected this file"}]
        with self.assertRaises(ValueError):
            self.compare(excluded=excluded)
        self.native = {mode: [] for mode in ce.MODES}
        report = self.compare(excluded=excluded)
        self.assertEqual(report["totals"]["sites"], 0)
        self.assertEqual(report["excluded"][0]["pilot_sites"], 1)

    def test_incomplete_modes_duplicate_sites_and_noninjective_pairing_rejected(self):
        original = deepcopy(self.native)
        self.native.pop("extended")
        with self.assertRaises(ValueError): self.compare()
        self.native = original
        self.native["extended"].append(deepcopy(self.nrow))
        with self.assertRaises(ValueError): self.compare()
        self.native = {mode: [deepcopy(self.nrow)] for mode in ce.MODES}
        self.pairs[("r", "/second")] = self.identity
        with self.assertRaises(ValueError): self.compare()

    def test_bad_values_are_rejected_not_counted_as_agreement(self):
        for bad in ({"kind": "int", "value": True}, {"kind": "bool", "value": 1},
                    {"kind": "real", "bits": "NaN"}, {"kind": "element", "id": None}, None):
            self.native["gated"][0]["outcome"] = values(bad)
            with self.subTest(bad=bad), self.assertRaises(ValueError): self.compare()

    def test_missing_pilot_mode_is_rejected(self):
        del self.psite["extended"]
        with self.assertRaises(ValueError): self.compare()

    def test_unpaired_sites_are_still_checked_for_malformed_results(self):
        self.pairs = {}
        self.native["extended"][0]["outcome"] = {"outcome": "error"}
        with self.assertRaises(ValueError): self.compare()

    def test_duplicate_resources_and_non_boolean_evaluability_are_rejected(self):
        self.pilot["resources"].append(self.key[0])
        with self.assertRaises(ValueError): self.compare()
        self.pilot["resources"].pop()
        self.native["extended"][0]["evaluable"] = 1
        with self.assertRaises(ValueError): self.compare()

    def test_multiple_resources_and_partial_native_only_mode_counts(self):
        extra = deepcopy(self.nrow)
        extra["site"].update(expression="other#1", resource="other.kerml")
        self.native["extended"].append(extra)
        report = self.compare()
        self.assertEqual(report["totals"]["sites"], 2)
        self.assertEqual(report["modes"]["model_level"]["sites"], 1)
        self.assertEqual(report["modes"]["extended"]["sites"], 2)
        self.assertEqual(report["resources"]["other.kerml"]["intersect_only"], 1)


class ProtocolAndBridge(unittest.TestCase):
    def test_blanket_malformed_and_duplicate_deviations_are_rejected(self):
        base = {"mode": "gated", "class": "OperatorExpression", "reason": "Specific reason"}
        bad = [dict(base, mode="*"), dict(base, reason=""), dict(base, function="A::f"),
               {"mode": "gated", "reason": "blanket"}, dict(base, **{"class": "*"}),
               {"mode": "gated", "sites": [], "reason": "empty"},
               {"mode": "gated", "sites": [["r", "*"]], "reason": "wildcard"}]
        for entry in bad:
            with self.subTest(entry=entry), self.assertRaises(ValueError): ce.validate_deviations([entry])
        with self.assertRaises(ValueError): ce.validate_deviations([base, base])

    def test_jsonl_accepts_bom_and_blank_lines_but_rejects_truncated_records(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "records.jsonl"
            path.write_text('\ufeff{"site": 1}\n\n{"site": 2}\n', encoding="utf-8")
            self.assertEqual(ce.read_jsonl(path), [{"site": 1}, {"site": 2}])
            path.write_text('{"site":', encoding="utf-8")
            with self.assertRaises(json.JSONDecodeError): ce.read_jsonl(path)
            path.write_text('{"value": NaN}', encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "Nonstandard JSON"): ce.read_jsonl(path)

    def test_bridge_refuses_to_reimplement_private_pairing(self):
        with patch.object(ce, "compare_models", lambda p, n: {}):
            with self.assertRaisesRegex(RuntimeError, "include_pairs API is required"):
                ce.load_pairing("pilot", "native")

    def test_bridge_uses_existing_compare_export_when_available(self):
        calls = []
        def exported(pilot, native, *, include_pairs=False):
            calls.append((pilot, native, include_pairs))
            return {"paired_objects": 1, "pairs": [{"pilot": {"resource": "r", "fragment": "/"}, "intersect": "r#0"}]}
        with patch.object(ce, "compare_models", exported):
            pairs, report = ce.load_pairing("pilot", "native")
        self.assertEqual(calls, [("pilot", "native", True)])
        self.assertEqual(pairs, {("r", "/"): "r#0"})
        self.assertEqual(report, {"paired_objects": 1})


if __name__ == "__main__":
    unittest.main()
