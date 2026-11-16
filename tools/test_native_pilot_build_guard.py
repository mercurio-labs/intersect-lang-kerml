"""External engine provenance must be explicit and Cargo-resolved."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import native_pilot_build_guard as guard


class CompilerInputTests(unittest.TestCase):
    def test_build_script_directory_inputs_retain_all_members_and_empty_directories(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source, empty = root / 'src', root / 'empty'
            source.mkdir()
            empty.mkdir()
            first, second = source / 'engine.rs', source / 'retained.json'
            first.write_text('// compiler', encoding='utf-8')
            second.write_text('{"retained":true}', encoding='utf-8')
            files, directories = guard.expand_source_inputs({source, empty, first}, (root,))
            self.assertEqual(files, {first, second})
            self.assertEqual(directories[source], sorted([first, second]))
            self.assertEqual(directories[empty], [])

    @unittest.skipUnless(os.name == 'nt', 'Windows dep-info path syntax')
    def test_extended_windows_source_path_keeps_the_same_canonical_owner(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source = root / 'engine.rs'
            source.write_text('// engine', encoding='utf-8')
            dep = root / 'engine.d'
            dep.write_text('artifact: \\\\?\\' + str(source).replace(' ', '\\ ') + '\n', encoding='utf-8')
            self.assertEqual(list(guard.dependencies(dep, root)), [source])
            self.assertEqual(guard.digest(source), guard.digest(Path('\\\\?\\' + str(source))))

    def test_top_level_and_hashed_library_artifacts_locate_own_dep_info(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            top_level = root / 'libintersect_kpar.rlib'
            cargo_dep = root / 'libintersect_kpar.d'
            cargo_dep.write_text('cargo: inputs', encoding='utf-8')
            self.assertEqual(guard.artifact_dep_info(top_level), cargo_dep)
            hashed = root / 'libintersect_model-abc123.rlib'
            rustc_dep = root / 'intersect_model-abc123.d'
            rustc_dep.write_text('rustc: inputs', encoding='utf-8')
            self.assertEqual(guard.artifact_dep_info(hashed), rustc_dep)
            missing = root / 'libmissing.rlib'
            with self.assertRaises(FileNotFoundError):
                list(guard.dependencies(guard.artifact_dep_info(missing), root))

    def test_dependency_escape_requires_an_explicit_resolved_root(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory).resolve()
            root, external = parent / 'workspace', parent / 'git-source'
            root.mkdir()
            external.mkdir()
            source = external / 'engine.rs'
            source.write_text('// pinned compiler', encoding='utf-8')
            dep = root / 'artifact.d'
            dep.write_text('artifact: ' + str(source).replace(' ', '\\ ') + '\n', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'outside source root'):
                list(guard.dependencies(dep, root))
            self.assertEqual(list(guard.dependencies(dep, root, (external,))), [source])

    def test_external_engine_cannot_bypass_cargo_identity_validation(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory).resolve()
            root, external = parent / 'workspace', parent / 'external'
            root.mkdir()
            external.mkdir()
            source = external / 'lib.rs'
            source.write_text('// engine', encoding='utf-8')
            cargo_json = root / 'build.json'
            rows = [
                {'reason': 'compiler-artifact', 'package_id': 'foreign',
                 'target': {'name': 'intersect_model', 'src_path': str(source), 'kind': ['lib']}},
                {'reason': 'compiler-artifact', 'package_id': 'host', 'executable': str(root / 'host.exe'),
                 'target': {'name': 'probe', 'src_path': str(root / 'host.rs'), 'kind': ['example']}},
                {'reason': 'build-finished', 'success': True},
            ]
            cargo_json.write_text('\n'.join(json.dumps(row) for row in rows), encoding='utf-8')
            for packages, message in [([], 'absent from resolved'),
                                      ([{'id': 'foreign', 'source': 'registry+unreviewed'}], 'canonical immutable')]:
                with self.subTest(packages=packages), mock.patch.object(
                        guard.subprocess, 'check_output', return_value=json.dumps({'packages': packages}).encode()):
                    with self.assertRaisesRegex(ValueError, message):
                        guard.capture(root, cargo_json, 'probe')


if __name__ == '__main__':
    unittest.main()
