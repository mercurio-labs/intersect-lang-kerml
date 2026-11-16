"""Installed-wheel acceptance using the real engine and packaged standard library."""
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import json
import tempfile
import unittest
import zipfile
from intersect_kerml import ClosedError, CompatibilityError, ConflictError, Engine, InvalidModelError, UnsupportedError
from intersect_kerml.sysml import PartDefinition


SOURCE = "package Vehicles { part def Vehicle; part vehicle : Vehicle; }"


class NativeSDK(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.engine = Engine()

    @classmethod
    def tearDownClass(cls):
        cls.engine.close()

    def project(self):
        project = self.engine.new_project()
        with project.transaction() as tx:
            tx.put_source("vehicle.sysml", SOURCE)
            tx.commit()
        return project

    def test_create_inspect_save_and_reopen(self):
        project = self.project()
        with project.analyze().require_model() as model:
            parts = list(model.elements(PartDefinition))
            self.assertEqual([part.declared_name for part in parts], ["Vehicle"])
            self.assertTrue(parts[0].is_instance(PartDefinition))
            self.assertEqual(parts[0].source_location.uri, "vehicle.sysml")
            self.assertIsNotNone(parts[0].container)
            with self.assertRaises((AttributeError, UnsupportedError)):
                parts[0].declared_name = "Changed"
        with self.assertRaises(ClosedError):
            _ = parts[0].id
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "vehicle"
            project.save_as(target)
            reopened = self.engine.open(target)
            with reopened.analyze().require_model() as model:
                self.assertEqual(len(list(model.elements(PartDefinition))), 1)

    def test_invalid_candidate_and_stale_transactions(self):
        project = self.project()
        with project.transaction() as bad:
            bad.put_source("vehicle.sysml", "package { certainly invalid ? ? ?")
            self.assertFalse(bad.validate().clean)
            with self.assertRaises(InvalidModelError):
                bad.commit()
        self.assertEqual(project.sources["vehicle.sysml"], SOURCE)
        first, second = project.transaction(), project.transaction()
        first.put_source("other.kerml", "package Other;")
        first.commit()
        with self.assertRaises(ConflictError):
            second.commit()
        second.abort()

    def test_multiple_threads_share_engine_without_moving_session(self):
        def analyze(index):
            project = self.engine.new_project()
            with project.transaction() as tx:
                tx.put_source(f"p{index}.kerml", f"package P{index};")
                with tx.commit() as model:
                    return len(list(model.elements()))
        with ThreadPoolExecutor(max_workers=2) as pool:
            self.assertTrue(all(n > 0 for n in pool.map(analyze, range(2))))

    def test_rejects_untrusted_and_mismatched_packs(self):
        with tempfile.TemporaryDirectory() as directory:
            pack = Path(directory) / "wrong.pack"
            pack.write_bytes(b"not this build's pack")
            with self.assertRaises(CompatibilityError):
                Engine(pack=pack)
            with self.assertRaises(CompatibilityError):
                Engine(pack=pack, trusted=True)

    def test_descriptor_and_offline_archive_dependency(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".project.json").write_text(json.dumps({"name": "Main", "version": "1.0.0",
                "usage": [{"resource": "https://example.test/dependency.kpar", "versionConstraint": "^1.0"}]}), encoding="utf-8")
            (root / "main.sysml").write_text("package Main { part def Use :> Dependency::Imported; }", encoding="utf-8")
            with zipfile.ZipFile(root / "dependency.kpar", "w", compression=zipfile.ZIP_DEFLATED) as archive:
                archive.writestr(".project.json", json.dumps({"name": "Dependency", "version": "1.0.0"}))
                archive.writestr(".meta.json", json.dumps({"index": {"Dependency": "dependency.sysml"},
                    "created": "2026-11-11T00:00:00Z", "metamodel": "https://www.omg.org/spec/SysML/20250201"}))
                archive.writestr("dependency.sysml", "package Dependency { part def Imported; }")
            project = self.engine.open(root)
            with project.analyze().require_model() as model:
                self.assertEqual([p.declared_name for p in model.elements(PartDefinition)], ["Use"])
                imported = [p for p in model.elements(PartDefinition, include_libraries=True) if p.declared_name == "Imported"]
                self.assertEqual(len(imported), 1)
                self.assertIn("dependency.kpar!/", imported[0].source_location.uri)
            with tempfile.TemporaryDirectory() as destination:
                copied = Path(destination) / "copy"
                project.save_as(copied)
                for name in (".project.json", "dependency.kpar", "main.sysml"):
                    self.assertEqual((copied / name).read_bytes(), (root / name).read_bytes())


if __name__ == "__main__":
    unittest.main()
