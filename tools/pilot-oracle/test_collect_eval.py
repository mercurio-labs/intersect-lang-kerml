"""Quick collection-protocol tests; no Java, parser, or language pack."""
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import collect_eval


class CollectionProtocol(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        root = Path(self.directory.name)
        self.args = SimpleNamespace(out=root, java=Path("java"), classpath="jar;classes",
                                    library=root / "library", root=root / "inputs")
        self.group = {"id": "example", "files": ["with space/example.sysml"]}
        self.report = {"resources": self.group["files"], "diagnostics": [], "sites": [],
                       "observation_isolation": "isolated"}

    def success(self, command, **kwargs):
        self.assertEqual(command[-1], "with space/example.sysml")
        index = command.index("--model-out")
        Path(command[index + 1]).write_text('{"resources": []}', encoding="utf-8")
        Path(command[index + 2]).write_text(json.dumps(self.report), encoding="utf-8")
        return Mock(wait=Mock(return_value=0), poll=Mock(return_value=0))

    def test_complete_reports_keep_zero_sites_and_check_fresh_equivalence(self):
        previous = self.args.out / "previous.json"
        previous.write_text(json.dumps(self.report), encoding="utf-8")
        self.group["fresh_eval"] = str(previous)
        with patch.object(collect_eval.subprocess, "Popen", side_effect=self.success):
            result = collect_eval.collect(self.args, self.group)
        self.assertEqual(result["exit"], 0)
        self.assertEqual(result["sites"], 0)
        self.assertTrue(result["fresh_load_equivalence"])
        self.assertEqual(len(result["eval_sha256"]), 64)
        self.assertEqual(len(result["model_sha256"]), 64)

    def test_changed_fresh_diagnostics_fail_even_when_the_process_succeeds(self):
        previous = self.args.out / "previous.json"
        previous.write_text(json.dumps(self.report), encoding="utf-8")
        self.group["fresh_eval"] = str(previous)
        self.report["diagnostics"] = [{"message": "changed validation"}]
        with patch.object(collect_eval.subprocess, "Popen", side_effect=self.success):
            with self.assertRaisesRegex(ValueError, "diagnostics"):
                collect_eval.collect(self.args, self.group)
        self.assertTrue((self.args.out / "example.log").exists())

    def test_nonzero_exit_is_retained_without_inventing_site_counts(self):
        process = Mock(wait=Mock(return_value=7), poll=Mock(return_value=7))
        with patch.object(collect_eval.subprocess, "Popen", return_value=process):
            result = collect_eval.collect(self.args, self.group)
        self.assertEqual(result["exit"], 7)
        self.assertNotIn("sites", result)
        self.assertNotIn("eval_sha256", result)

    def test_existing_output_is_never_overwritten(self):
        existing = self.args.out / "example-model.json"
        existing.write_text("retained", encoding="utf-8")
        with patch.object(collect_eval.subprocess, "Popen") as start:
            with self.assertRaisesRegex(ValueError, "overwrite"):
                collect_eval.collect(self.args, self.group)
            start.assert_not_called()
        self.assertEqual(existing.read_text(encoding="utf-8"), "retained")

    def test_interrupted_wait_terminates_and_reaps_the_owned_child(self):
        process = Mock(wait=Mock(side_effect=[KeyboardInterrupt(), 1]),
                       poll=Mock(return_value=None))
        with patch.object(collect_eval.subprocess, "Popen", return_value=process):
            with self.assertRaises(KeyboardInterrupt):
                collect_eval.collect(self.args, self.group)
        process.terminate.assert_called_once()
        self.assertEqual(process.wait.call_count, 2)


if __name__ == "__main__":
    unittest.main()
