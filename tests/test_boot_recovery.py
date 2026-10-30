"""Negative controls for the real QEMU recovery acceptance harness."""
import importlib.util
from pathlib import Path
import struct
import sys
import unittest

TOOLS = Path(__file__).resolve().parents[1] / "tools"
sys.path.insert(0, str(TOOLS))
spec = importlib.util.spec_from_file_location("boot_recovery_qemu", TOOLS / "test_boot_recovery_qemu.py")
recovery = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recovery)


class RecoveryAcceptanceTests(unittest.TestCase):
    def trace(self):
        return "\n".join((recovery.UNAVAILABLE, recovery.READY, recovery.REJECTED,
                         recovery.REJECTED, "IndianOS recovery: unsupported command",
                         recovery.UNAVAILABLE, recovery.STATUS, recovery.HELP, recovery.HALTED))

    def test_complete_input_dependent_trace(self):
        recovery.verify_trace(self.trace(), recovery.UNAVAILABLE)

    def test_banner_alone_and_missing_rejections_fail(self):
        for trace in (recovery.READY, self.trace().replace(recovery.REJECTED, "", 1),
                      self.trace().replace(recovery.STATUS, ""),
                      self.trace().replace(recovery.HELP, "")):
            with self.assertRaises(AssertionError):
                recovery.verify_trace(trace, recovery.UNAVAILABLE)

    def test_bad_command_execution_and_user_launch_fail(self):
        for extra in (recovery.HALTED, recovery.STATUS, "entering ring-3"):
            with self.assertRaises(AssertionError):
                recovery.verify_trace(self.trace() + extra, recovery.UNAVAILABLE)

    def test_wrong_failure_reason_is_not_accepted(self):
        with self.assertRaises(AssertionError):
            recovery.verify_trace(self.trace(), recovery.STAGING)

    def test_module_removal_keeps_kernel_and_protocol(self):
        config = "timeout: 1\n/IndianOS\n protocol: limine\n path: boot():/boot/indianos.elf\n module_path: boot():/boot/initramfs.img\n module_string: init\n"
        changed = recovery.without_module(config)
        self.assertIn("protocol: limine", changed)
        self.assertIn("path: boot():/boot/indianos.elf", changed)
        self.assertNotIn("module_", changed)

    def test_fixture_can_separate_missing_file_and_malformed_elf(self):
        for name in (b"/bin/init", b"/bin/other"):
            image = recovery.archive(name, b"bad ELF")
            self.assertEqual(image[:8], b"INDRAM01")
            self.assertEqual(struct.unpack_from("<II", image, 8), (1, len(image)))
            self.assertEqual(image[24:24 + len(name)], name)


if __name__ == "__main__":
    unittest.main()
