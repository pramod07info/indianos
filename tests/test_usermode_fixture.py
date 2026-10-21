"""Host-only tests for a strict x86-64 ring-3 demo ELF preflight."""
import pathlib
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "tools"))
from check_usermode_fixture import check_program


def test_elf(*, traps=True, greeting=True):
    payload = (b"\xcd\x80\xcd\x80" if traps else b"\xf3\x90")
    if greeting:
        payload += b"Namaste from IndianOS user mode!"
    binary = bytearray(0x1000 + len(payload))
    binary[0:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<HHI", binary, 16, 2, 62, 1)
    struct.pack_into("<Q", binary, 24, 0x400000)
    struct.pack_into("<Q", binary, 32, 64)
    struct.pack_into("<HHH", binary, 52, 64, 56, 1)
    struct.pack_into("<IIQQQQQQ", binary, 64,
                     1, 5, 0x1000, 0x400000, 0x400000,
                     len(payload), len(payload), 4096)
    binary[0x1000:] = payload
    return binary


class UsermodeFixtureTests(unittest.TestCase):
    def check(self, binary):
        with tempfile.TemporaryDirectory() as folder:
            path = pathlib.Path(folder) / "demo.elf"
            path.write_bytes(binary)
            check_program(path)

    def test_accepts_syscall_enabled_elf_only_as_preflight(self):
        self.check(test_elf())

    def test_rejects_loop_only_elf(self):
        with self.assertRaisesRegex(ValueError, "lacks expected"):
            self.check(test_elf(traps=False))

    def test_rejects_missing_greeting(self):
        with self.assertRaisesRegex(ValueError, "greeting"):
            self.check(test_elf(greeting=False))

    def test_rejects_non_elf_or_wrong_machine(self):
        with self.assertRaisesRegex(ValueError, "ELF64"):
            self.check(b"\xcd\x80")
        blob = test_elf()
        struct.pack_into("<H", blob, 18, 183)  # aarch64, not x86_64
        with self.assertRaisesRegex(ValueError, "ELF64"):
            self.check(blob)

    def test_rejects_bad_program_offset(self):
        blob = test_elf()
        struct.pack_into("<Q", blob, 32, 0xffffffffffffffff)
        with self.assertRaisesRegex(ValueError, "header table"):
            self.check(blob)

    def test_rejects_writable_executable_segment(self):
        blob = test_elf()
        struct.pack_into("<I", blob, 68, 7)
        with self.assertRaisesRegex(ValueError, "invalid"):
            self.check(blob)


if __name__ == "__main__":
    unittest.main()
