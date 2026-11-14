#!/usr/bin/env python3
"""Build dependency/preflight regressions; no cross compiler required."""

from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import build
import p67_pack


class IconPythonTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.current = self.repo / "current-python"
        self.venv = self.repo / ".venv" / ("Scripts/python.exe" if build.os.name == "nt" else "bin/python")
        self.current.touch()
        self.venv.parent.mkdir(parents=True)
        self.venv.touch()
        self.start_patch = self.enterContext(patch.object(build.sys, "executable", str(self.current)))
        self.probe = self.enterContext(patch.object(build.subprocess, "run"))
        self.available = subprocess.CompletedProcess([], 0, "", "")
        self.missing = subprocess.CompletedProcess([], 1, "", "ModuleNotFoundError: No module named 'PIL'")

    def test_prefers_current_python_when_pillow_is_available(self):
        self.probe.return_value = self.available
        self.assertEqual(build.find_icon_python(self.repo), str(self.current))
        self.probe.assert_called_once_with(
            [str(self.current), "-c", "from PIL import Image, ImageDraw"],
            cwd=self.repo, capture_output=True, text=True,
        )

    def test_falls_back_to_project_venv(self):
        self.probe.side_effect = [self.missing, self.available]
        self.assertEqual(build.find_icon_python(self.repo), str(self.venv))
        self.assertEqual(self.probe.call_args.args[0][0], str(self.venv))

    def test_missing_dependency_has_setup_instructions(self):
        self.probe.return_value = self.missing
        with self.assertRaisesRegex(RuntimeError, "pip install -r tools/requirements-build.txt"):
            build.find_icon_python(self.repo)
        self.assertEqual(self.probe.call_count, 2)

    def test_nonexistent_venv_is_not_invoked(self):
        self.venv.unlink()
        self.probe.return_value = self.missing
        with self.assertRaises(RuntimeError):
            build.find_icon_python(self.repo)
        self.probe.assert_called_once()

    def test_broken_interpreter_reports_actionable_error(self):
        self.probe.side_effect = [self.missing, OSError("not executable")]
        with self.assertRaisesRegex(RuntimeError, "not executable"):
            build.find_icon_python(self.repo)

    def test_same_interpreter_is_not_probed_twice(self):
        self.probe.return_value = self.missing
        with patch.object(build.sys, "executable", str(self.venv)):
            with self.assertRaises(RuntimeError):
                build.find_icon_python(self.repo)
        self.probe.assert_called_once()


class BuildPreflightTests(unittest.TestCase):
    def test_missing_pillow_fails_before_compilation_or_outputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(build.sys, "argv", ["build.py", "--repo", tmp]):
                with patch.object(build, "find_icon_python", side_effect=RuntimeError("missing Pillow")):
                    with patch.object(build, "build_ko") as compile_ko:
                        with self.assertRaisesRegex(RuntimeError, "missing Pillow"):
                            build.main()
                        compile_ko.assert_not_called()
            self.assertFalse((Path(tmp) / "dist").exists())

    def test_no_icons_does_not_require_pillow(self):
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(build.sys, "argv", ["build.py", "--repo", tmp, "--no-icons"]):
                with patch.object(build, "find_icon_python") as probe:
                    with patch.object(build, "build_ko", side_effect=RuntimeError("compile reached")):
                        with self.assertRaisesRegex(RuntimeError, "compile reached"):
                            build.main()
                    probe.assert_not_called()

    def test_icon_generation_uses_selected_interpreter(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            output = repo / "icon.bin"
            output.write_bytes(bytes((0x19, 0x10, 0, 0)) + bytes(8 + 64 * 64 * 4))
            with patch.object(build, "run") as run:
                build.build_icon(repo, output, 64, "project-python")
                command, cwd, env = run.call_args.args
                self.assertEqual(command, [
                    "project-python", str(repo / "tools" / "gen_chaos_icon.py"),
                    str(repo / "tools" / "chaos.png"), str(output),
                ])
                self.assertEqual(cwd, repo)
                self.assertEqual(env["CHAOS_ICON_SIZE"], "64")

    def test_preview_generation_uses_pillow_interpreter(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            output = repo / "dist/preview.bin"
            with patch.object(build, "run") as run:
                build.build_preview(repo, output, "project-python")
                run.assert_called_once_with([
                    "project-python", str(repo / "tools/gen_preview.py"), str(output),
                ], repo)


class ContainerIntegrityTests(unittest.TestCase):
    def test_container_preserves_every_payload_byte_including_fetch(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory); assets = repo / 'dist/assets'; stage = repo / 'dist/stage'
            assets.mkdir(parents=True); stage.mkdir(); (repo / 'installer').mkdir()
            (repo / 'installer/chaos_installer.lua').write_text('return true')
            names = ('chaos_sup.bin', 'fetch-module.bin', 'capatch.bin', 'ca.pem',
                     'chaos_icon.bin', 'chaos_icon_settings.bin', 'lxgw.ttf')
            for i, name in enumerate(names): (assets / name).write_bytes(bytes([i]) * (10 + i))
            (assets.parent / 'THIRD-PARTY-NETWORK.txt').write_text('license')
            preview = build.DEFAULT_ROOT / 'tools/p67-preview.block'
            out = repo / 'installer.bin'
            build.build_container(repo, out, preview, stage, assets, '979820260926', 'Test', 'Test')
            self.assertEqual(len(p67_pack.inspect(out)['files']), 9)
            (assets / 'fetch-module.bin').unlink()
            with self.assertRaisesRegex(RuntimeError, 'fetch-module'):
                build.build_container(repo, out, preview, stage, assets, '979820260926', 'Test', 'Test')


if __name__ == "__main__":
    unittest.main()
