"""Filesystem controls; these tests make no claims about language analysis."""
from pathlib import Path
import tempfile
import unittest
from intersect_kerml import ConflictError, IntersectError, Project


class ProjectIO(unittest.TestCase):
    def test_new_project_save_and_external_conflict(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "project"
            project = Project(None, None)
            project._sources = {"nested/test.sysml": "package Example;"}
            project.save_as(destination)
            source = destination / "nested/test.sysml"
            self.assertEqual(source.read_text(), "package Example;")
            source.write_text("package External;", encoding="utf-8")
            project._sources["nested/test.sysml"] = "package Changed;"
            with self.assertRaises(ConflictError):
                project.save()
            self.assertEqual(source.read_text(), "package External;")

    def test_copy_preserves_sidecars_and_deleted_source(self):
        with tempfile.TemporaryDirectory() as directory:
            origin = Path(directory) / "original"
            origin.mkdir()
            (origin / "test.sysml").write_text("package Original;", encoding="utf-8")
            (origin / "notes.md").write_text("Retain this document", encoding="utf-8")
            project = Project(None, origin)
            project._sources.clear()
            project.save_as(Path(directory) / "copy")
            self.assertFalse((Path(directory) / "copy/test.sysml").exists())
            self.assertEqual((Path(directory) / "copy/notes.md").read_text(), "Retain this document")
            self.assertTrue((origin / "test.sysml").exists())

    def test_traversal_and_reserved_paths(self):
        project = Project(None, None)
        with project.transaction() as transaction:
            for path in ("../outside.sysml", "C:/outside.sysml", ".git/config.sysml", "/outside.sysml"):
                with self.assertRaises(ValueError):
                    transaction.put_source(path, "package P;")

    def test_unchanged_source_retains_original_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "bom.kerml"
            original = b"\xef\xbb\xbfpackage Original;\r\n"
            source.write_bytes(original)
            project = Project(None, Path(directory))
            project.save()
            self.assertEqual(source.read_bytes(), original)

    def test_input_budget_is_checked_before_reading_source(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "large.sysml"
            with source.open("wb") as stream:
                stream.truncate(8 * 1024 * 1024 + 1)
            with self.assertRaises(IntersectError):
                Project(None, Path(directory))


if __name__ == "__main__":
    unittest.main()
