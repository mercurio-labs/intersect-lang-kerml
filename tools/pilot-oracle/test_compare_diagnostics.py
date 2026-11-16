"""Inline fixtures for the validation corpus builder and diagnostic comparison."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


def module(name, file):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(file))
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


corpus = module("validation_corpus", "validation_corpus.py")
diagnostics = module("compare_diagnostics", "compare_diagnostics.py")

SOURCE = (
    "package P {\r\n"
    "\t// XPECT errors --> \"Bad thing\" at \"feature  x;\"\r\n"
    "\tfeature x;\r\n"
    "\t//* XPECT warnings ---\r\n"
    "\t\t\"Name clash\" at \"y\"\r\n"
    "\t--- */\r\n"
    "\tfeature y;\r\n"
    "}\r\n"
)


def pilot_issue(offset, length, severity="error", code="rule", message="Bad thing", fragment="//@ownedRelationship.0"):
    return {"severity": severity, "code": code, "message": message, "line": 3, "column": 2,
            "offset": offset, "length": length, "target_class": "Feature",
            "target_resource": "u/A.kerml", "target_fragment": fragment,
            "target_start": offset, "target_end": offset + length}


def pilot_report(issues, objects=None, resource_diagnostics=()):
    entry = {"resource": "u/A.kerml", "requested": True, "objects": 1,
             "resource_diagnostics": list(resource_diagnostics), "diagnostics": issues}
    if objects is not None:
        entry["object_export"] = objects
    return {"folders": [{"folder": "001", "files": [entry]}]}


class Expectations(unittest.TestCase):
    def test_single_and_block_annotations_with_crlf_offsets(self):
        found = corpus.expectations(SOURCE)
        self.assertEqual([(e["severity"], e["message"], e["at"]) for e in found],
                         [("error", "Bad thing", "feature  x;"), ("warning", "Name clash", "y")])
        # `at` ignores whitespace; offsets count CRLF as two characters.
        self.assertEqual(SOURCE[found[0]["at_offset"]:found[0]["at_end"]], "feature x;")
        self.assertEqual(found[1]["at_offset"], SOURCE.index("feature y;") + len("feature "))
        self.assertEqual(found[1]["line"], 4)

    def test_setup_files_skip_the_library(self):
        text = '//* XPECT_SETUP X ResourceSet { ThisFile {} File {from ="/library/Base.kerml"} ' \
               'File {from ="/src/Dep.kerml"} File "Other.kerml" } END_SETUP */'
        self.assertEqual(corpus.setup_files(text), ["/src/Dep.kerml", "Other.kerml"])


class XpectComparison(unittest.TestCase):
    def run_xpect(self, issues):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "u").mkdir()
            (root / "u" / "A.kerml").write_bytes(SOURCE.encode("utf-8"))
            expected = {"units": [{"unit": "001", "source": "x/A.kerml.xt", "file": "u/A.kerml",
                                   "valid_control": False, "support_files": [],
                                   "expected": corpus.expectations(SOURCE)}]}
            return diagnostics.compare_xpect(pilot_report(issues), expected, root)

    def test_match_moved_missing_unannotated(self):
        x = SOURCE.index("feature x;")
        y = SOURCE.index("feature y;") + len("feature ")
        result = self.run_xpect([pilot_issue(x, 10), pilot_issue(y, 1, "warning", "clash", "Name clash"),
                                 pilot_issue(0, 7, code="other", message="Unexpected")])
        self.assertEqual(result["totals"]["matched"], 2)
        self.assertEqual(result["totals"]["unannotated"], 1)
        result = self.run_xpect([pilot_issue(0, 7)])
        self.assertEqual((result["totals"].get("moved"), result["totals"].get("missing")), (1, 1))

    def test_severity_must_agree(self):
        x = SOURCE.index("feature x;")
        result = self.run_xpect([pilot_issue(x, 10, severity="warning")])
        self.assertEqual(result["totals"].get("matched", 0), 0)


class RuleSummary(unittest.TestCase):
    def test_counts_per_code_and_batch(self):
        rules = {"batches": {"B1": "one", "B2": "two"}, "rules": [
            {"id": "K.a.A", "code": "rule", "batch": "B1", "category": "purely structural"},
            {"id": "K.b.B", "code": "never", "batch": "B2", "category": "derived properties"}]}
        linking = {"severity": "error", "message": "Couldn't resolve reference to Type 'X'.",
                   "code": diagnostics.LINKING, "line": 1, "column": 1, "offset": 0, "length": 1}
        result = diagnostics.rule_summary(pilot_report([pilot_issue(1, 2), pilot_issue(3, 2)],
                                                       resource_diagnostics=[linking]), rules)
        self.assertEqual(result["batches"]["B1"]["diagnostics"], 2)
        self.assertEqual(result["batches"]["B2"]["rules_firing_on_codes"], 0)
        self.assertEqual(result["framework"]["linking"], 1)


class IntersectComparison(unittest.TestCase):
    def run_intersect(self, ours, dump=None, issues=None):
        x = SOURCE.index("feature x;")
        objects = [{"fragment": "/", "class": "Namespace", "values": {
                       "ownedRelationship": [{"resource": "u/A.kerml", "fragment": "//@ownedRelationship.0"}]}},
                   {"fragment": "//@ownedRelationship.0", "class": "Feature", "values": {}}]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "u").mkdir()
            (root / "u" / "A.kerml").write_bytes(SOURCE.encode("utf-8"))
            dump_path = None
            if dump is not None:
                dump_path = root / "dump.json"
                dump_path.write_text(json.dumps(dump), encoding="utf-8")
            return diagnostics.compare_intersect(
                pilot_report(issues if issues is not None else [pilot_issue(x, 10)], objects),
                {"format": "intersect-diagnostics/1", "resources": {"u/A.kerml": ours}}, root, dump_path)

    def ours(self, **changes):
        x = len(SOURCE[:SOURCE.index("feature x;")].encode("utf-8"))
        d = {"severity": "error", "code": "rule", "message": "Other wording",
             "element": {"id": "f1", "class": "Feature"}, "start": x, "end": x + 10}
        d.update(changes)
        return d

    def test_location_pairing(self):
        result = self.run_intersect([self.ours()])
        self.assertEqual((result["matched"], result["pilot_only"], result["intersect_only"]), (1, 0, 0))
        self.assertEqual(result["per_rule"]["rule"], {"agree": 1})
        result = self.run_intersect([self.ours(start=0)])
        self.assertEqual(result["per_rule"]["rule"], {"pilot_only": 1, "intersect_only": 1})

    def test_containment_pairing(self):
        dump = {"resources": {"u/A.kerml": {"roots": ["root"]}}, "elements": [
            {"id": "root", "resource": "u/A.kerml", "class": "Namespace", "values": {"ownedRelationship": ["f1"]}},
            {"id": "f1", "resource": "u/A.kerml", "class": "Feature", "values": {}}]}
        result = self.run_intersect([self.ours(start=None)], dump)
        self.assertEqual(result["matched"], 1)
        self.assertEqual(result["pairing"], "containment")
        result = self.run_intersect([self.ours(element={"id": "root"})], dump)
        self.assertEqual(result["matched"], 0)

    def test_code_must_agree_and_format_is_checked(self):
        result = self.run_intersect([self.ours(code="validateSomethingElse")])
        self.assertEqual(result["matched"], 0)
        with self.assertRaises(ValueError):
            diagnostics.intersect_records({"resources": {}})

    def test_line_column_units(self):
        text = "a\r\n\U0001F600b"
        self.assertEqual(diagnostics.line_column(text, 5, utf16=True), (2, 2))
        self.assertEqual(diagnostics.line_column(text, 7), (2, 2))


if __name__ == "__main__":
    unittest.main()
