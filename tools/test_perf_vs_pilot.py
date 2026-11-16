"""Quick tests: python -B -m unittest discover -s tools -p test_perf_vs_pilot.py."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

import perf_vs_pilot as perf


class PerfTests(unittest.TestCase):
    def test_fake_child_medians_and_table(self):
        with tempfile.TemporaryDirectory() as directory:
            counter = Path(directory) / "counter"
            code = ("import pathlib,json; "
                    f"p=pathlib.Path({str(counter)!r}); "
                    "n=int(p.read_text())+1 if p.exists() else 1; "
                    "p.write_text(str(n)); "
                    "print(json.dumps({'timing':{'parse_ms':[9,1,5][n-1]}}))")
            runs = [perf.run_once([sys.executable, "-c", code], directory, "command") for _ in range(3)]
            summary = perf.summarize(runs)
            self.assertEqual(summary["parse_ms"], dict(median=5, min=1, max=9))
            for metric in ("wall_ms", "peak_working_set_bytes"):
                values = sorted(run[metric] for run in runs)
                self.assertEqual(summary[metric], dict(median=values[1], min=values[0], max=values[2]))
            for run in runs:
                self.assertGreater(run["wall_ms"], 0)
                self.assertGreater(run["peak_working_set_bytes"], 0)
            table = perf.markdown({"profiles": {"fake": {"summary": summary}}})
            self.assertIn("| fake | parse_ms | 5.000 | 1.000 | 9.000 |", table)
            self.assertIn("| Profile | Metric | Median | Min | Max |", table)

    def test_pilot_file_and_cold_warm(self):
        timing = dict(setup_ms=1, cold_parse_ms=2, cold_resolve_ms=3,
                      warm_parse_ms=4, warm_resolve_ms=5)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "pilot.json"
            code = f"import pathlib; pathlib.Path({str(path)!r}).write_text({json.dumps({'timing': timing})!r})"
            run = perf.run_once([sys.executable, "-c", code], directory, "pilot-bsl", path)
            self.assertEqual(run["phases"], timing)
        self.assertEqual(perf.parse_phases("progress\n" + json.dumps({"timing": timing}), "pilot-bsl"), timing)

    def test_intersect_phases(self):
        text = """KerML definition preparation 20 ms
read 12 redefinitions 3 ms
freeze metamodel 1.5 ms
parse into MemoryStore: 40.1 ms, diagnostics 0, reference proxies 57
parse into FrozenStore: 30.2 ms, diagnostics 0, reference proxies 57
load saved 10 Rcore scopes and 3 defaults 14 ms
link 2.1 ms: 57 references, 57 resolved uniquely
defaults 0.2 ms
validate 0.1 ms: valid, 266 objects
"""
        result = perf.parse_phases(text, "intersect-bsl")
        self.assertEqual(len(result), 9)
        self.assertEqual(result["frozen_parse_ms"], 30.2)
        self.assertEqual(perf.parse_phases(text.replace("load saved", "compile"), "intersect-bsl"), result)
        with self.assertRaisesRegex(ValueError, "missing phase"):
            perf.parse_phases("link 1 ms", "intersect-bsl")

    def test_prefers_psutil_when_available(self):
        process = Mock(pid=123)
        psutil = Mock()
        psutil.Process.return_value.memory_info.return_value = Mock(peak_wset=42, rss=21)
        with patch.dict(sys.modules, {"psutil": psutil}):
            meter = perf.MemoryMeter(process)
            self.assertEqual(meter.read(), 42 if perf.os.name == "nt" else 21)
            self.assertTrue(meter.backend.startswith("psutil"))
            psutil.Process.assert_called_once_with(123)
            meter.close()

    def test_invalid_and_failed_child(self):
        for value in (True, -1, float("nan"), "2"):
            with self.assertRaises(ValueError):
                perf.parse_phases(json.dumps({"timing": {"parse_ms": value}}), "command")
        with self.assertRaises(ValueError):
            perf.parse_phases('{"timing":{}}', "command")
        with self.assertRaises(ValueError):
            perf.parse_phases('{"timing":{"setup_ms":1}}', "pilot-bsl")
        with self.assertRaisesRegex(RuntimeError, "child exited 7"):
            perf.run_once([sys.executable, "-c", "raise SystemExit(7)"], ".", "command")

    def test_cli_outputs_and_default_runs(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(perf.subprocess, "check_output", return_value="revision\n"):
                report = perf.main(["--output", directory, "--", sys.executable, "-c",
                                    "print('{\"timing\":{\"parse_ms\":2}}')"])
            self.assertEqual(report["runs"], 5)
            self.assertEqual(len(report["profiles"]["command"]["runs"]), 5)
            saved = json.loads((Path(directory) / "report.json").read_text())
            self.assertEqual(saved, report)
            self.assertEqual((Path(directory) / "report.md").read_text(), perf.markdown(report))
            self.assertEqual(report["profiles"]["command"]["command"][0], sys.executable)

    def test_library_profiles_commands(self):
        library = ["sysml.library/b/Z.kerml", "sysml.library/a b/A.sysml", "sysml.library/a b/B.kerml"]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            base = root / perf.SOURCES
            for file in library + ["sysml.library/a b/notes.txt", "other/C.kerml"]:
                (base / file).parent.mkdir(parents=True, exist_ok=True)
                (base / file).write_text("x")
            self.assertEqual(perf.library_files(root), sorted(library))
            jar = root / "pilot.jar"
            jar.write_bytes(b"jar")
            classes = root / "classes"
            classes.mkdir()
            (classes / "PilotOracle.class").write_bytes(b"class")
            binary = root / "frozen_library_load"
            binary.write_bytes(b"binary")
            pilot = dict(setup_ms=1, cold_parse_ms=2, cold_resolve_ms=3, warm_parse_ms=4, warm_resolve_ms=5)
            ours = {name: 1 for name in perf.INTERSECT_LIBRARY_PHASES}
            calls = []

            def fake_run(command, cwd, profile, timing_file=None):
                calls.append((command, cwd, profile, timing_file))
                phases = pilot if profile == "pilot-library" else ours
                return dict(wall_ms=1, peak_working_set_bytes=2, phases=phases)
            with patch.object(perf.subprocess, "check_output", return_value="revision\n"), \
                    patch.object(perf, "run_once", side_effect=fake_run):
                report = perf.main(["--runs", "2", "--root", directory, "--output", str(root / "out"),
                                    "--profile", "pilot-library", "--profile", "intersect-library",
                                    "--pilot-jar", str(jar), "--pilot-classes", str(classes),
                                    "--intersect-library-binary", str(binary), "--java", "java",
                                    "--java-option=-Xmx12g", "--java-option=-Xss64m"])
            self.assertEqual(len(calls), 4)
            command, cwd, profile, timing_file = calls[0]
            self.assertEqual(profile, "pilot-library")
            self.assertEqual(command[:4], ["java", "-Xmx12g", "-Xss64m", "-cp"])
            self.assertEqual(command[5:8], ["PilotOracle", str(timing_file), str(base.resolve())])
            self.assertEqual(command[8:], sorted(library))
            self.assertEqual(timing_file.name, "pilot-library-timing.json")
            self.assertEqual(calls[2][0], [str(binary.resolve()), str(root.resolve())])
            self.assertEqual(calls[2][2], "intersect-library")
            self.assertEqual(report["pilot_jar_sha256"], perf.sha256(jar))
            self.assertEqual(report["intersect_library_binary_sha256"], perf.sha256(binary))
            self.assertEqual(report["profiles"]["pilot-library"]["summary"]["cold_resolve_ms"],
                             dict(median=3, min=3, max=3))

    def test_library_phases_required(self):
        ours = {name: 1.5 for name in perf.INTERSECT_LIBRARY_PHASES}
        text = "progress\n" + json.dumps({"timing": ours, "references": 3})
        self.assertEqual(perf.parse_phases(text, "intersect-library"), ours)
        del ours["link_ms"]
        with self.assertRaisesRegex(ValueError, "Intersect library"):
            perf.parse_phases(json.dumps({"timing": ours}), "intersect-library")
        with self.assertRaisesRegex(ValueError, "cold/warm"):
            perf.parse_phases('{"timing":{"setup_ms":1}}', "pilot-library")


if __name__ == "__main__":
    unittest.main()
