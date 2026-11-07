"""Inspect real package trees using a small executable fixture."""
import importlib.util
import os
from pathlib import Path
import platform
import plistlib
import shutil
import struct
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "build.py"
spec = importlib.util.spec_from_file_location("build", SCRIPT)
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.binary = self.root / "aether"
        self.binary.write_text("#!/bin/sh\nexit 0\n")
        self.binary.chmod(0o755)
        self.stage = self.root / "stage"
        self.stage.mkdir()

    def test_linux_install_tree(self):
        build.linux(self.binary, self.stage)
        executable = self.stage / "usr/bin/aether"
        self.assertTrue(os.access(executable, os.X_OK))
        self.assertEqual(subprocess.run([executable]).returncode, 0)
        desktop = self.stage / f"usr/share/applications/{build.APP_ID}.desktop"
        text = desktop.read_text()
        self.assertIn(f"Icon={build.APP_ID}\n", text)
        self.assertIn(f"StartupWMClass={build.APP_ID}\n", text)
        self.assertIn("Exec=aether\n", text)
        self.assertEqual(len(list(self.stage.rglob("*.png"))), 8)
        self.assertFalse(list(self.stage.rglob(".env")))
        self.assertTrue((self.stage / "usr/share/doc/aether/copyright").is_file())
        if shutil.which("desktop-file-validate"):
            subprocess.run(["desktop-file-validate", desktop], check=True)

    def test_icon_dimensions_and_icns_chunks(self):
        for size in (16, 32, 48, 64, 128, 256, 512, 1024):
            data = (build.ICONS / f"{size}.png").read_bytes()
            self.assertEqual(data[:8], b"\x89PNG\r\n\x1a\n")
            self.assertEqual(struct.unpack(">II", data[16:24]), (size, size))
        data = (build.ICONS / "Aether.icns").read_bytes()
        self.assertEqual(data[:4], b"icns")
        self.assertEqual(struct.unpack(">I", data[4:8])[0], len(data))
        offset, chunks = 8, 0
        while offset < len(data):
            size = struct.unpack(">I", data[offset + 4:offset + 8])[0]
            self.assertGreater(size, 8)
            self.assertEqual(data[offset + 8:offset + 16], b"\x89PNG\r\n\x1a\n")
            offset += size
            chunks += 1
        self.assertEqual(offset, len(data))
        self.assertEqual(chunks, 7)

    def test_first_identity_reads_the_first_valid_certificate(self):
        hash = "A" * 40
        listing = f'  1) {hash} "Apple Development: a (B)"\n     1 valid identities found\n'
        self.assertEqual(build.first_identity(listing), hash)
        self.assertIsNone(build.first_identity("     0 valid identities found\n"))

    @unittest.skipUnless(platform.system() == "Darwin", "requires macOS signing tools")
    def test_macos_bundle(self):
        # Sign a real Mach-O binary, then inspect the completed bundle.
        shutil.copy("/usr/bin/true", self.binary)
        app = build.macos(self.binary, self.stage, "0.1.0", None)
        with (app / "Contents/Info.plist").open("rb") as file:
            info = plistlib.load(file)
        self.assertEqual(info["CFBundleIdentifier"], build.APP_ID)
        self.assertEqual(info["CFBundleShortVersionString"], "0.1.0")
        self.assertTrue((app / "Contents/Resources/Aether.icns").is_file())
        self.assertTrue((app / "Contents/Resources/LICENSE").is_file())
        self.assertTrue(os.access(app / "Contents/MacOS/aether", os.X_OK))
        self.assertIn("NSMicrophoneUsageDescription", info)
        self.assertTrue(info["NSAppSleepDisabled"])
        entitlements = subprocess.run(["codesign", "-d", "--entitlements", "-", app],
                                      capture_output=True, text=True).stdout
        self.assertIn("com.apple.security.device.audio-input", entitlements)

    def test_packaged_builds_do_not_target_the_build_machines_cpu(self):
        self.assertIn('os.environ.setdefault("GGML_NATIVE", "OFF")', SCRIPT.read_text())

    def test_invalid_format_fails_before_build(self):
        fmt = "deb" if platform.system() == "Darwin" else "dmg"
        result = subprocess.run(["python3", SCRIPT, "--format", fmt], capture_output=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn(b"unsupported", result.stderr)


if __name__ == "__main__":
    unittest.main()
