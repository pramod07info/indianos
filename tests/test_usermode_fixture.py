"""Host-only checks for the experimental ring-3 ELF preflight."""
import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "tools"))
from check_usermode_fixture import check_program


class UsermodeFixtureTests(unittest.TestCase):
    def test_rejects_existing_loop_only_fixture(self):
        with tempfile.TemporaryDirectory() as d:
            elf = pathlib.Path(d) / "loop.elf"
            elf.write_bytes(b"\x7fELF" + bytes(60) + b"\xf3\x90")
            with self.assertRaisesRegex(ValueError, "lacks an INT 0x80"):
                check_program(elf)

    def test_requires_elf_file(self):
        with tempfile.TemporaryDirectory() as d:
            elf = pathlib.Path(d) / "invalid"
            elf.write_bytes(b"\xcd\x80")
            with self.assertRaisesRegex(ValueError, "not ELF"):
                check_program(elf)

    def test_accepts_opcode_only_as_preflight(self):
        with tempfile.TemporaryDirectory() as d:
            elf = pathlib.Path(d) / "user.elf"
            elf.write_bytes(b"\x7fELF" + bytes(60) + b"\xcd\x80")
            check_program(elf)  # QEMU still needed for actual proof


if __name__ == "__main__":
    unittest.main()
