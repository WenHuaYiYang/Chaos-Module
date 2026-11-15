#!/usr/bin/env python3
"""Integrity, wrong-firmware, candidate/export and build-cache regressions."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile
import contextlib
import io

import build
import firmware_symbols as fw


class ExtractionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.ident = '10p-9.9.9'
        self.ap = b'\xff' * 4 + bytes(12) + bytes.fromhex('14480047') + bytes(range(128))
        self.path = self.root / 'input.bin'
        self.path.write_bytes(self.ap)
        self.folder = self.root / 'symbol' / self.ident

    def seed(self, records=None):
        self.folder.mkdir(parents=True, exist_ok=True)
        fw.write_json(self.folder / 'reviewed.json', {
            'schema': 1, 'ap_sha256': fw.sha(self.ap), 'symbols': records or [{
                'name': 'example', 'kind': 'function', 'address': '0x0c0c0011',
                'instruction_set': 'thumb', 'prototype': 'void(void)',
                'review': 'exact-target-reviewed', 'verification_level': 'STATIC_CONFIRMED',
                'evidence': {'fixture': True, 'approval_state': 'APPROVED', 'policy': 'managed'}}]})

    def test_zip_reads_only_ap_and_ignores_archive_path_traversal(self):
        path = self.root / 'ota.bin'
        with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED) as z:
            z.writestr('vela_ap.bin', self.ap)
            z.writestr('../evil', b'not extracted')
            z.writestr('ota.sh', b'exit 99')
        self.seed()
        out = self.root / 'unpacked'
        result = fw.write_extraction(self.root, path, self.ident, out)
        self.assertEqual(result['exports'], 1)
        self.assertEqual((out / 'vela_ap.bin').read_bytes(), self.ap)
        self.assertFalse((self.root / 'evil').exists())
        target = fw.load_target(self.root, self.ident, require_build=False)
        self.assertEqual(target['ap_sha256'], fw.sha(self.ap))

    def test_unknown_image_never_inherits_reference_exports(self):
        fw.write_extraction(self.root, self.path, self.ident)
        self.assertEqual(fw.read_json(self.folder / 'symbols.json')['symbols'], [])
        with self.assertRaisesRegex(ValueError, '不能构建完整 Chaos'):
            fw.load_target(self.root, self.ident)

    def test_changed_image_is_rejected_without_replacing_existing_profile(self):
        self.seed()
        fw.write_extraction(self.root, self.path, self.ident)
        before = {p.name: p.read_bytes() for p in self.folder.iterdir()}
        self.path.write_bytes(self.ap + b'changed')
        with self.assertRaisesRegex(ValueError, '不一致'):
            fw.write_extraction(self.root, self.path, self.ident)
        self.assertEqual(before, {p.name: p.read_bytes() for p in self.folder.iterdir()})

    def test_tampered_header_is_not_accepted_by_the_build(self):
        self.seed()
        fw.write_extraction(self.root, self.path, self.ident)
        (self.folder / 'firmware.h').write_text('#define WRONG 1')
        with self.assertRaisesRegex(ValueError, '改变或损坏'):
            fw.load_target(self.root, self.ident, require_build=False)

    def test_refreshing_manifest_cannot_hide_wrong_generated_bindings(self):
        self.seed()
        fw.write_extraction(self.root, self.path, self.ident)
        header = self.folder / 'firmware.h'
        header.write_text('#define WRONG 1')
        manifest = fw.read_json(self.folder / 'manifest.json')
        manifest['files']['firmware.h'] = fw.digest(header)
        fw.write_json(self.folder / 'manifest.json', manifest)
        with self.assertRaisesRegex(ValueError, '生成绑定与核验资料不一致'):
            fw.load_target(self.root, self.ident, require_build=False)

    def test_candidates_and_even_function_addresses_cannot_be_exported(self):
        self.seed()
        seed = fw.read_json(self.folder / 'reviewed.json')
        seed['symbols'][0]['review'] = 'candidate'
        with self.assertRaisesRegex(ValueError, '未核验记录'):
            fw.validate_records(seed, fw.sha(self.ap))
        seed['symbols'][0]['review'] = 'exact-target-reviewed'
        seed['symbols'][0]['address'] = '0x0c0c0010'
        with self.assertRaisesRegex(ValueError, 'Thumb'):
            fw.validate_records(seed, fw.sha(self.ap))
        seed['symbols'][0]['address'] = '0x0c0c0011'
        seed['symbols'][0]['verification_level'] = 'CANDIDATE'
        with self.assertRaisesRegex(ValueError, '冲突'):
            fw.validate_records(seed, fw.sha(self.ap))

    def test_raw_ap_output_cannot_replace_input(self):
        self.path = self.root / 'vela_ap.bin'
        self.path.write_bytes(self.ap)
        with self.assertRaisesRegex(ValueError, '不能覆盖输入'):
            fw.write_extraction(self.root, self.path, self.ident, self.root)
        self.assertEqual(self.path.read_bytes(), self.ap)
        self.assertFalse(self.folder.exists())

    def test_target_path_is_validated(self):
        for value in ('../10p-9.9.9', '/tmp/10p-9.9.9', '10p-9.9.9/..'):
            with self.assertRaises(ValueError):
                fw.target_id(value)

    def test_unique_search_result_never_enters_callable_exports(self):
        self.seed()
        fw.write_extraction(self.root, self.path, self.ident)
        pattern = self.ap[16:80]
        target_ap = self.ap[:20] + b'\xaa' * 44 + pattern + b'\xbb' * 80
        target_file = self.root / 'next.bin'
        target_file.write_bytes(target_ap)
        target_ident = '10p-9.9.10'
        result = fw.write_extraction(self.root, target_file, target_ident,
                                     reference=(self.path, self.ident))
        self.assertEqual(result['candidates'], 1)
        folder = self.root / 'symbol' / target_ident
        self.assertEqual(fw.read_json(folder / 'symbols.json')['symbols'], [])
        candidate = fw.read_json(folder / 'candidates.json')['matches'][0]
        self.assertEqual(candidate['status'], 'REVIEW_REQUIRED')
        self.assertNotIn('callable_address', candidate)
        self.assertEqual(candidate['target_file_offset'], '0x40')
        repeated = target_ap + pattern
        self.assertEqual(fw.match_candidates(repeated, self.path, self.root, self.ident), [])

    def test_import_preserves_approval_and_rejects_another_ap(self):
        folder = self.root / 'import'
        folder.mkdir()
        record = {'schema': 1, 'name': 'example', 'kind': 'function',
                  'callable_address': '0x0c0c0011', 'prototype': 'void(void)',
                  'instruction_set': 'thumb', 'symbol_id': 'fixture.example',
                  'approval_state': 'APPROVED', 'status': 'STATIC_CONFIRMED',
                  'policy': 'managed', 'provenance': {'firmware_sha256': fw.sha(self.ap)}}
        fw.write_json(folder / 'one.json', record)
        fw.write_json(folder / 'pending.json', {**record, 'name': 'unreviewed', 'approval_state': 'PENDING'})
        imported = fw.import_reviewed(folder, fw.sha(self.ap))
        self.assertEqual(len(imported['symbols']), 1)
        self.assertEqual(imported['symbols'][0]['verification_level'], 'STATIC_CONFIRMED')
        self.assertEqual(imported['excluded_source_records'], {'PENDING': 1})
        with self.assertRaisesRegex(ValueError, '另一固件'):
            fw.import_reviewed(folder, '0' * 64)

    def test_conflicting_reviewed_import_cannot_replace_an_existing_binding(self):
        self.seed()
        fw.write_extraction(self.root, self.path, self.ident)
        before = (self.folder / 'reviewed.json').read_bytes()
        folder = self.root / 'import'
        folder.mkdir()
        fw.write_json(folder / 'one.json', {
            'schema': 1, 'name': 'example', 'kind': 'function',
            'callable_address': '0x0c0c0041', 'prototype': 'void(void)',
            'instruction_set': 'thumb', 'symbol_id': 'fixture.example',
            'approval_state': 'APPROVED', 'status': 'STATIC_CONFIRMED', 'policy': 'managed',
            'provenance': {'firmware_sha256': fw.sha(self.ap)}})
        with self.assertRaisesRegex(ValueError, '冲突'):
            fw.write_extraction(self.root, self.path, self.ident, reviewed_dir=folder)
        self.assertEqual(before, (self.folder / 'reviewed.json').read_bytes())

    def test_cli_requires_explicit_partial_export_for_an_unadapted_target(self):
        args = ['firmware_symbols.py', '--repo', str(self.root), 'extract', str(self.path), '--target', self.ident]
        with patch.object(fw.sys, 'argv', args), contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(ValueError, '不能构建完整 Chaos'):
                fw.main()
        with patch.object(fw.sys, 'argv', args + ['--exports-only']), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(fw.main(), 0)


class BuildBoundaryTests(unittest.TestCase):
    def test_exact_043_binding_is_consumable_and_036_exports_are_separate(self):
        profile = fw.load_target(fw.ROOT, '10p-3.101.043')
        old = fw.load_target(fw.ROOT, '10p-3.101.036', require_build=False)
        self.assertNotEqual(profile['ap_sha256'], old['ap_sha256'])
        symbols = fw.read_json(profile['symbol_dir'] / 'symbols.json')['symbols']
        values = {r['name']: int(r['address'], 16) for r in symbols}
        self.assertEqual(values[profile['bindings']['FW_REGISTER_DRIVER']], 0x0c1aff61)
        with self.assertRaisesRegex(ValueError, '不能构建完整 Chaos'):
            fw.load_target(fw.ROOT, '10p-3.101.036')

    def test_036_build_fails_before_any_compiler_invocation(self):
        with patch.object(build.sys, 'argv', ['build.py', '--no-icons', '--firmware', '10p-3.101.036']):
            with patch.object(build, 'build_ko') as compiler:
                with self.assertRaisesRegex(ValueError, '不能构建完整 Chaos'):
                    build.main()
                compiler.assert_not_called()

    def test_cache_rejects_wrong_target_manifest_and_changed_module(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            target = {'id': '10p-3.101.043', 'manifest_sha256': 'a'}
            for name in ('chaos_sup.ko', 'fetch-module.ko', 'capatch.elf'):
                (folder / name).write_bytes(b'test payload')
            build.record_artifacts(folder, target)
            build.validate_cached(folder, target, ('chaos_sup.ko',))
            for wrong in ({**target, 'id': '10p-3.101.036'}, {**target, 'manifest_sha256': 'b'}):
                with self.assertRaisesRegex(RuntimeError, '跨固件混包'):
                    build.validate_cached(folder, wrong, ('chaos_sup.ko',))
            (folder / 'fetch-module.ko').write_bytes(b'other firmware')
            with self.assertRaisesRegex(RuntimeError, '内容与固件身份记录不符'):
                build.validate_cached(folder, target, ('fetch-module.ko',))


if __name__ == '__main__':
    unittest.main()
