"""R030 signed offline packaging and transaction regression tests."""
import importlib.util
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location(
    "r030_pkg", Path(__file__).resolve().parents[1] / "tools/r030_package_tool.py")
pkg = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pkg)


def elf(tag=0xc3):
    b = bytearray(0x101)
    b[0:9] = b"\x7fELF\x02\x01\x01\x00\x00"
    struct.pack_into("<HHI", b, 16, 2, 62, 1)
    struct.pack_into("<Q", b, 24, 0x400100)
    struct.pack_into("<Q", b, 32, 64)
    struct.pack_into("<HHH", b, 52, 64, 56, 1)
    struct.pack_into("<IIQQQQQQ", b, 64, 1, 5, 0x100, 0x400100, 0, 1, 1, 4096)
    b[0x100] = tag
    return bytes(b)


class R030(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.key_dir = tempfile.TemporaryDirectory()
        d = Path(cls.key_dir.name)
        cls.priv = d / "sign.key"
        cls.pub = d / "verify.pub"
        cls.other = d / "other.pub"
        subprocess.run(["openssl", "genpkey", "-algorithm", "ed25519", "-out", str(cls.priv)],
                       check=True, capture_output=True)
        subprocess.run(["openssl", "pkey", "-in", str(cls.priv), "-pubout", "-out", str(cls.pub)],
                       check=True, capture_output=True)
        other_priv = d / "other.key"
        subprocess.run(["openssl", "genpkey", "-algorithm", "ed25519", "-out", str(other_priv)],
                       check=True, capture_output=True)
        subprocess.run(["openssl", "pkey", "-in", str(other_priv), "-pubout", "-out", str(cls.other)],
                       check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        cls.key_dir.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.b = self.root / "bundle.ipkg"
        self.e = self.root / "program.elf"
        self.e.write_bytes(elf())
        self.meta = pkg.build(self.e, self.b, "native-demo", "1.2.3", self.priv)

    def tearDown(self):
        self.temp.cleanup()

    def test_signature_and_manifest_verified(self):
        meta, output = pkg.parse_archive(self.b, self.pub)
        self.assertEqual(meta, self.meta)
        self.assertEqual(output, elf())
        self.assertEqual(output[0x100], 0xc3)

    def test_wrong_trust_anchor_fails(self):
        with self.assertRaises(pkg.PackageError):
            pkg.parse_archive(self.b, self.other)

    def test_installs_with_unexecutable_payload_and_resolves(self):
        state = pkg.install(self.b, self.pub, self.root)
        self.assertEqual(state["current"]["version"], "1.2.3")
        active = pkg.active(self.root, "native-demo")
        self.assertEqual(active.read_bytes(), elf())
        self.assertEqual(active.stat().st_mode & 0o111, 0)

    def test_update_then_rollback_and_newer_update(self):
        pkg.install(self.b, self.pub, self.root)
        self.e.write_bytes(elf(0x90))
        second = self.root / "v2.ipkg"
        pkg.build(self.e, second, "native-demo", "1.3.0", self.priv)
        state = pkg.install(second, self.pub, self.root)
        self.assertEqual(state["previous"]["version"], "1.2.3")
        self.assertEqual(pkg.active(self.root, "native-demo").read_bytes(), elf(0x90))
        rolled = pkg.rollback(self.root, "native-demo")
        self.assertEqual(rolled["current"]["version"], "1.2.3")
        self.assertEqual(pkg.active(self.root, "native-demo").read_bytes(), elf())
        with self.assertRaises(pkg.PackageError):
            pkg.install(self.b, self.pub, self.root)

    def test_removal_denies_activation(self):
        pkg.install(self.b, self.pub, self.root)
        self.assertEqual(pkg.remove(self.root, "native-demo"), {"removed": "native-demo"})
        with self.assertRaises(pkg.PackageError):
            pkg.active(self.root, "native-demo")
        with self.assertRaises(pkg.PackageError):
            pkg.remove(self.root, "native-demo")

    def test_no_rollback_without_previous_generation(self):
        pkg.install(self.b, self.pub, self.root)
        with self.assertRaises(pkg.PackageError):
            pkg.rollback(self.root, "native-demo")

    def test_downgrade_is_rejected_before_state_mutation(self):
        pkg.install(self.b, self.pub, self.root)
        original = pkg.active(self.root, "native-demo").read_bytes()
        with self.assertRaises(pkg.PackageError):
            pkg.install(self.b, self.pub, self.root)
        self.assertEqual(pkg.active(self.root, "native-demo").read_bytes(), original)

    def test_manifest_tampering_rejected(self):
        with zipfile.ZipFile(self.b) as z:
            members = {x.filename: z.read(x) for x in z.infolist()}
        m = json.loads(members["manifest.json"])
        m["version"] = "9.9.9"
        members["manifest.json"] = pkg.canonical(m)
        with zipfile.ZipFile(self.b, "w", compression=zipfile.ZIP_STORED) as z:
            for k, v in members.items():
                z.writestr(k, v)
        with self.assertRaises(pkg.PackageError):
            pkg.parse_archive(self.b, self.pub)

    def test_payload_digest_tamper_rejected(self):
        with zipfile.ZipFile(self.b) as z:
            members = {x.filename: z.read(x) for x in z.infolist()}
        fake = bytearray(members["payload.elf"])
        fake[-1] ^= 1
        members["payload.elf"] = bytes(fake)
        with zipfile.ZipFile(self.b, "w", compression=zipfile.ZIP_STORED) as z:
            for k, v in members.items():
                z.writestr(k, v)
        with self.assertRaises(pkg.PackageError):
            pkg.parse_archive(self.b, self.pub)

    def test_duplicate_zip_and_path_traversal_rejected(self):
        with zipfile.ZipFile(self.b, "a") as z:
            import warnings
            with warnings.catch_warnings():
                warnings.simplefilter("ignore")
                z.writestr("payload.elf", b"evil")
        with self.assertRaises(pkg.PackageError):
            pkg.parse_archive(self.b, self.pub)
        pkg.build(self.e, self.b, "native-demo", "1.2.3", self.priv)
        with zipfile.ZipFile(self.b, "a") as z:
            z.writestr("../escape", b"bad")
        with self.assertRaises(pkg.PackageError):
            pkg.parse_archive(self.b, self.pub)

    def test_corrupt_existing_payload_is_fail_closed(self):
        pkg.install(self.b, self.pub, self.root)
        f = pkg.active(self.root, "native-demo")
        f.write_bytes(b"hostile")
        with self.assertRaises(pkg.PackageError):
            pkg.active(self.root, "native-demo")
        with self.assertRaises(pkg.PackageError):
            pkg.install(self.b, self.pub, self.root)

    def test_symlink_state_and_root_rejected(self):
        outside = self.root / "outside"
        outside.mkdir()
        link = self.root / "link"
        link.symlink_to(outside, target_is_directory=True)
        with self.assertRaises(pkg.PackageError):
            pkg.install(self.b, self.pub, link)
        state = self.root / ".indianos-apps"
        state.symlink_to(outside, target_is_directory=True)
        with self.assertRaises(pkg.PackageError):
            pkg.install(self.b, self.pub, self.root)

    def test_invalid_elf_wrong_machine_dynamic_wx_and_overlaps(self):
        cases = []
        bad = bytearray(elf()); bad[18] = 3; cases.append(bad)
        bad = bytearray(elf()); bad[68] = 3; cases.append(bad)
        bad = bytearray(elf()); bad[68] = 2; cases.append(bad)
        bad = bytearray(elf()); bad[68] = 7; cases.append(bad)
        for case in cases:
            with self.subTest(value=case[68]):
                with self.assertRaises(pkg.PackageError):
                    pkg.validate_elf(bytes(case))

    def test_filename_and_version_restrictions(self):
        for invalid in ("../bad", "UpperCase", "x", "a/b", "one.with.dot"):
            with self.assertRaises(pkg.PackageError):
                pkg.build(self.e, self.b, invalid, "1.2.3", self.priv)
        for invalid in ("1.2", "01.2.3", "latest", "1.2.3-rc", "-1.2.3"):
            with self.assertRaises(pkg.PackageError):
                pkg.build(self.e, self.b, "native-demo", invalid, self.priv)

    def test_failed_install_preserves_previous_state(self):
        pkg.install(self.b, self.pub, self.root)
        with self.assertRaises(pkg.PackageError):
            pkg.install(self.b, self.other, self.root)
        self.assertEqual(pkg.active(self.root, "native-demo").read_bytes(), elf())


if __name__ == "__main__":
    unittest.main()
