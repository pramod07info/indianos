"""R030 end-to-end host tests of signed native ELF package lifecycle."""
from __future__ import annotations
import io
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from r030_pkg import (PackageError, build, verify, run_action, TARGET,
                       openssl_signature, check_elf, signed_payload, canonical)


def fixture_elf(marker: bytes = b"R030") -> bytes:
    # Minimal well-formed static ET_EXEC for signature/manifest unit tests;
    # the QEMU acceptance uses actual compiled freestanding Rust binaries.
    b = bytearray(256)
    b[:16] = b"\x7fELF\x02\x01\x01" + bytes(9)
    struct.pack_into("<HHI", b, 16, 2, 62, 1)
    struct.pack_into("<Q", b, 24, 0x400080)
    struct.pack_into("<Q", b, 32, 64)
    struct.pack_into("<I", b, 48, 0)
    struct.pack_into("<HHHHHH", b, 52, 64, 56, 1, 0, 0, 0)
    struct.pack_into("<IIQQQQQQ", b, 64, 1, 5, 128, 0x400080, 0,
                     len(marker), len(marker), 4096)
    b[128:128 + len(marker)] = marker
    return bytes(b)


class SignedPackageTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        base = Path(self.tmp.name)
        self.private = base / "test-only-key.pem"
        self.public = base / "trusted-key.pem"
        self.root = base / "staging"
        subprocess.run(["openssl", "genpkey", "-algorithm", "Ed25519",
                        "-out", str(self.private)], check=True, capture_output=True)
        subprocess.run(["openssl", "pkey", "-in", str(self.private),
                        "-pubout", "-out", str(self.public)], check=True, capture_output=True)
        self.elf1 = fixture_elf(b"R030 native v1")
        self.elf2 = fixture_elf(b"R030 native v2")
        self.pkg1 = build(self.elf1, "hello", "1.0.0", self.private)
        self.pkg2 = build(self.elf2, "hello", "1.1.0", self.private)
        self.first = base / "first.pkg"
        self.second = base / "second.pkg"
        self.first.write_bytes(self.pkg1)
        self.second.write_bytes(self.pkg2)

    def test_verified_signing_is_deterministic(self):
        self.assertEqual(self.pkg1, build(self.elf1, "hello", "1.0.0", self.private))
        metadata, elf = verify(self.pkg1, self.public)
        self.assertEqual(elf, self.elf1)
        self.assertEqual(metadata["target"], TARGET)
        self.assertEqual(metadata["version"], "1.0.0")

    def test_install_update_rollback_remove_and_export(self):
        out = Path(self.tmp.name) / "verified.elf"
        run_action("install", self.root, "hello", self.first, self.public, None)
        run_action("status", self.root, "hello", None, self.public, None)
        run_action("export", self.root, "hello", None, self.public, out)
        self.assertEqual(out.read_bytes(), self.elf1)
        with self.assertRaises(PackageError):
            run_action("install", self.root, "hello", self.first, self.public, None)
        run_action("update", self.root, "hello", self.second, self.public, None)
        run_action("export", self.root, "hello", None, self.public, out)
        self.assertEqual(out.read_bytes(), self.elf2)
        run_action("rollback", self.root, "hello", None, self.public, None)
        run_action("export", self.root, "hello", None, self.public, out)
        self.assertEqual(out.read_bytes(), self.elf1)
        run_action("rollback", self.root, "hello", None, self.public, None)
        run_action("export", self.root, "hello", None, self.public, out)
        self.assertEqual(out.read_bytes(), self.elf2)
        run_action("remove", self.root, "hello", None, self.public, None)
        self.assertFalse((self.root / "apps" / "hello").exists())

    def test_tampered_payload_wrong_trust_key_and_mismatched_identity(self):
        other = Path(self.tmp.name) / "other-key.pem"
        pub = Path(self.tmp.name) / "other-public.pem"
        subprocess.run(["openssl", "genpkey", "-algorithm", "Ed25519",
                        "-out", str(other)], check=True, capture_output=True)
        subprocess.run(["openssl", "pkey", "-in", str(other), "-pubout",
                        "-out", str(pub)], check=True, capture_output=True)
        with self.assertRaises(PackageError):
            verify(self.pkg1, pub)
        with zipfile.ZipFile(io.BytesIO(self.pkg1)) as original:
            files = {n: original.read(n) for n in original.namelist()}
        files["app.elf"] = self.elf2
        output = io.BytesIO()
        with zipfile.ZipFile(output, "w") as archive:
            for n, data in files.items():
                archive.writestr(n, data)
        with self.assertRaises(PackageError):
            verify(output.getvalue(), self.public)
        with self.assertRaises(PackageError):
            run_action("install", self.root, "wrong-name", self.first, self.public, None)

    def test_non_upgrade_rejected_without_mutating_installed_version(self):
        run_action("install", self.root, "hello", self.second, self.public, None)
        with self.assertRaises(PackageError):
            run_action("update", self.root, "hello", self.first, self.public, None)
        out = Path(self.tmp.name) / "export.elf"
        run_action("export", self.root, "hello", None, self.public, out)
        self.assertEqual(out.read_bytes(), self.elf2)
        with self.assertRaises(PackageError):
            run_action("rollback", self.root, "hello", None, self.public, None)

    def test_manifest_validation_rejects_unsupported_target_and_unsafe_name(self):
        with self.assertRaises(PackageError):
            build(self.elf1, "../escape", "1.0.0", self.private)
        with self.assertRaises(PackageError):
            build(self.elf1, "hello", "1.0.0-alpha", self.private)
        with self.assertRaises(PackageError):
            check_elf(b"not an ELF")
        with self.assertRaises(PackageError):
            check_elf(self.elf1[:80])
        bad = bytearray(self.elf1)
        bad[18] = 0x28  # ARM e_machine
        with self.assertRaises(PackageError):
            check_elf(bytes(bad))

    def test_packages_do_not_accept_symlinks_duplicates_or_zip_compression(self):
        zip_buf = io.BytesIO()
        with zipfile.ZipFile(zip_buf, "w") as archive:
            archive.writestr("manifest.json", b"{}")
            archive.writestr("manifest.json", b"{}")
            archive.writestr("app.elf", self.elf1)
            archive.writestr("signature.ed25519", bytes(64))
        with self.assertRaises(PackageError):
            verify(zip_buf.getvalue(), self.public)
        sym = Path(self.tmp.name) / "sym"
        sym.symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(PackageError):
            run_action("install", sym, "hello", self.first, self.public, None)
        compressed = io.BytesIO()
        with zipfile.ZipFile(compressed, "w", compression=zipfile.ZIP_DEFLATED) as z:
            z.writestr("app.elf", self.elf1)
            z.writestr("manifest.json", b"{}")
            z.writestr("signature.ed25519", bytes(64))
        with self.assertRaises(PackageError):
            verify(compressed.getvalue(), self.public)


if __name__ == "__main__":
    unittest.main()
