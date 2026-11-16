"""Inline export and diagnostic fixtures for the handwritten Pilot comparison tool."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("pilot_compare", Path(__file__).with_name("compare.py"))
compare = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(compare)


class CompareTests(unittest.TestCase):
    def run_fixture(self, pv=None, fv=None, cls="Namespace", extra_pilot=False, extra_frozen=False, second_resource=False):
        pilot = {"resources": [{"resource": "test.kerml", "objects": [
            {"fragment": "/", "class": cls, "values": pv or {}}]}]}
        frozen = {"resources": {"test.kerml": {"roots": ["root"]}}, "elements": [
            {"id": "root", "resource": "test.kerml", "class": cls, "values": fv or {}}]}
        if second_resource:
            pilot["resources"].append({"resource": "other.kerml", "objects": [
                {"fragment": "/", "class": cls, "values": {"declaredName": "B"}}]})
            frozen["resources"]["other.kerml"] = {"roots": ["other"]}
            frozen["elements"].append({"id": "other", "resource": "other.kerml", "class": cls,
                                       "values": {"declaredName": "C"}})
        if extra_pilot:
            pilot["resources"][0]["objects"].append({"fragment": "//@extra.0", "class": cls, "values": {}})
        if extra_frozen:
            frozen["elements"].append({"id": "extra", "resource": "test.kerml", "class": cls, "values": {}})
        with tempfile.TemporaryDirectory() as directory:
            p, f = Path(directory)/"pilot.json", Path(directory)/"frozen.json"
            p.write_text(json.dumps(pilot), encoding="utf-8")
            f.write_text(json.dumps(frozen), encoding="utf-8")
            result = compare.compare(p, f)
        self.assertEqual(result["paired_objects"], 2 if second_resource else 1)
        for key in ("paired_objects", "fields_compared", "fields_equal", "references_agreeing",
                    "allow_listed_differences", "unexplained_differences"):
            self.assertEqual(result[key], sum(row[key] for row in result["resources"].values()))
        return result

    def test_match(self):
        result = self.run_fixture({"declaredName": "A", "target": {"resource": "test.kerml", "fragment": "/"}},
                                  {"declaredName": "A", "target": "root"})
        self.assertEqual((result["fields_compared"], result["fields_equal"]), (2, 2))
        self.assertEqual(result["references_agreeing"], 1)
        self.assertEqual(result["unexplained_differences"], 0)

    def test_per_resource_counts(self):
        result = self.run_fixture({"declaredName": "A"}, {"declaredName": "A"}, second_resource=True)
        first, second = result["resources"]["test.kerml"], result["resources"]["other.kerml"]
        self.assertEqual((first["fields_equal"], first["unexplained_differences"]), (1, 0))
        self.assertEqual((second["fields_equal"], second["unexplained_differences"]), (0, 1))

    def test_field_difference(self):
        result = self.run_fixture({"declaredName": "A"}, {"declaredName": "B"})
        self.assertEqual(result["fields_equal"], 0)
        self.assertEqual(result["unexplained_differences"], 1)

    def test_allow_list(self):
        for cls, operator in (("FeatureChainExpression", "."), ("IndexExpression", "#")):
            with self.subTest(cls=cls):
                result = self.run_fixture({}, {"operator": operator}, cls)
                self.assertEqual(result["allow_listed_differences"], 1)
                self.assertEqual(result["unexplained_differences"], 0)
                self.assertEqual(result["fields_equal"], 0)
        result = self.run_fixture({}, {"operator": "."}, "OperatorExpression")
        self.assertEqual(result["unexplained_differences"], 1)

    def test_unpaired_object(self):
        for side in ("extra_pilot", "extra_frozen"):
            with self.subTest(side=side):
                result = self.run_fixture(**{side: True})
                self.assertEqual(result["unexplained_differences"], 1)
                self.assertEqual(result["unpaired_pilot" if side == "extra_pilot" else "unpaired_frozen"], 1)

    def test_reference_mismatch(self):
        result = self.run_fixture({"target": {"resource": "test.kerml", "fragment": "/"}}, {"target": "elsewhere"})
        self.assertEqual(result["references_compared"], 1)
        self.assertEqual(result["references_agreeing"], 0)
        self.assertEqual(result["unexplained_differences"], 1)

compare_diagnostics = compare.compare_diagnostics


class DiagnosticPairing(unittest.TestCase):
    def compare(self, pilot, ours, pairs=None):
        return compare_diagnostics(
            {"resources": [{"diagnostics": pilot}]}, {"diagnostics": ours},
            {("r", "/"): "o"} if pairs is None else pairs)

    def pilot(self, **changes):
        return dict(severity="error", code="rule", message="Pilot wording",
                    target={"resource": "r", "fragment": "/"}, start=1, end=5,
                    **changes)

    def ours(self, **changes):
        result = dict(severity="error", code="rule", message="Rust wording",
                      target="o", start=2, end=4)
        result.update(changes)
        return result

    def test_code_pairs_through_object_identity_not_span_or_wording(self):
        result = self.compare([self.pilot()], [self.ours()])
        self.assertEqual((result["matched"], result["pilot_only"], result["intersect_only"]), (1, 0, 0))
        self.assertEqual(result["matches"][0]["pilot"]["start"], 1)

    def test_same_fragment_in_different_resources_does_not_match(self):
        pilot = self.pilot()
        pilot["target"]["resource"] = "other-resource"
        result = self.compare([pilot], [self.ours()],
                              {("r", "/"): "o", ("other-resource", "/"): "other"})
        self.assertEqual(result["matched"], 0)

    def test_duplicates_are_a_multiset(self):
        result = self.compare([self.pilot(), self.pilot()], [self.ours()])
        self.assertEqual((result["matched"], result["pilot_only"]), (1, 1))

    def test_message_fallback_without_code(self):
        self.assertEqual(self.compare([self.pilot()], [self.ours(code=None, message="Pilot wording")])["matched"], 1)

    def test_different_codes_do_not_match_even_with_same_message(self):
        self.assertEqual(self.compare([self.pilot()], [self.ours(code="different", message="Pilot wording")])["matched"], 0)

    def test_code_matches_reserved_before_message_fallback(self):
        codeless = self.pilot()
        codeless["code"] = None
        coded = self.pilot()
        result = self.compare([codeless, coded], [self.ours(message="Pilot wording")])
        self.assertEqual(result["matched"], 1)
        self.assertEqual(result["pilot_only_diagnostics"], [codeless])

    def test_severity_and_target_must_agree(self):
        for other in [self.ours(severity="warning"), self.ours(target="other")]:
            with self.subTest(other=other):
                result = self.compare([self.pilot()], [other])
                self.assertEqual((result["matched"], result["pilot_only"], result["intersect_only"]), (0, 1, 1))

    def test_unpaired_and_absent_targets_stay_unmatched(self):
        self.assertEqual(self.compare([self.pilot()], [self.ours(target=None)], {})["matched"], 0)
        p = self.pilot()
        p.pop("target")
        self.assertEqual(self.compare([p], [self.ours(target=None)])["matched"], 0)

    def test_empty_and_one_sided_exports(self):
        self.assertEqual(self.compare([], [])["matched"], 0)
        self.assertEqual(self.compare([], [self.ours()])["intersect_only"], 1)
        self.assertEqual(self.compare([self.pilot()], [])["pilot_only"], 1)

    def test_missing_diagnostics_are_not_zero(self):
        for pilot, frozen in [({"resources": [{}]}, {"diagnostics": []}),
                              ({"resources": [{"diagnostics": []}]}, {})]:
            with self.assertRaises(ValueError):
                compare_diagnostics(pilot, frozen, {})


if __name__ == "__main__":
    unittest.main()
