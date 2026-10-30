"""Fail-closed host controls for the real R051 QEMU acceptance harness."""
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
try:
    import test_m1_terminal_persistence_qemu as harness
finally:
    sys.path.pop(0)


class Session:
    def __init__(self, acknowledge=True):
        self.output = bytearray((harness.ACK + "\n").encode())
        self.acknowledge = acknowledge
        self.commands = []
        self.response_offsets = []

    def hmp(self, command):
        self.commands.append(command)
        if self.acknowledge:
            self.output.extend((harness.ACK + "\n").encode())

    def until(self, marker, deadline, since=0):
        self.response_offsets.append(since)
        if marker.encode() not in self.output[since:]:
            raise AssertionError("fresh response missing")

    def serial(self, data):
        raise AssertionError("serial input cannot prove physical keyboard acceptance")


class TerminalPersistenceControls(unittest.TestCase):
    def test_stale_acknowledgement_cannot_satisfy_a_new_physical_key(self):
        session = Session(acknowledge=False)
        with self.assertRaisesRegex(AssertionError, "fresh response"):
            harness.acknowledged_key(session, "q")
        self.assertEqual(session.commands, ["sendkey q 80"])

    def test_every_editor_character_requires_a_physical_roundtrip_including_q_and_space(self):
        session = Session()
        harness.type_command(session, "write " + harness.PAYLOAD)
        self.assertEqual(len(session.commands), len("write " + harness.PAYLOAD))
        self.assertIn("sendkey q 80", session.commands)
        self.assertIn("sendkey spc 80", session.commands)
        self.assertEqual(len(set(session.response_offsets)), len(session.commands))

    def test_unapproved_hmp_keys_and_commands_are_rejected_before_injection(self):
        session = Session()
        for key in ("q;quit", "ctrl-alt-delete", "\n", "spc\nquit"):
            with self.assertRaises(AssertionError):
                harness.acknowledged_key(session, key)
        for command in ("", "write /etc", "quit;system_reset", "CAT"):
            with self.assertRaises(AssertionError):
                harness.type_command(session, command)
        self.assertEqual(session.commands, [])

    def test_readback_must_be_exact_complete_and_new(self):
        valid = (harness.READBACK + "\r\n").encode()
        harness.exact_readback(valid, 0)
        for invalid, since in ((valid, len(valid)),
                               (valid.rstrip(b"\r\n"), 0),
                               (valid.rstrip(b"\r\n") + b" EXTRA\n", 0),
                               (valid.replace(b"quiet note", b"wrong note"), 0),
                               (b"echo " + valid, 0),
                               (valid.rstrip(b"\r\n") + b"trailing", 0)):
            with self.assertRaises(AssertionError):
                harness.exact_readback(invalid, since)

    def test_unchanged_framebuffer_cannot_prove_visual_acceptance(self):
        frame = (640, 480, bytes(640 * 480 * 3))
        with self.assertRaisesRegex(AssertionError, "framebuffer"):
            harness.changed(frame, frame, "editor activation")
        with self.assertRaisesRegex(AssertionError, "Files pixels"):
            harness.files_repainted(frame, frame)

    def test_terminal_only_repaint_cannot_prove_files_refresh(self):
        frame = (640, 480, bytes(640 * 480 * 3))
        pixels = bytearray(frame[2])
        for y in range(100, 120):
            for x in range(400, 500):
                i = (y * 640 + x) * 3
                pixels[i:i + 3] = b"\xff\xff\xff"
        terminal_only = (640, 480, bytes(pixels))
        harness.changed(frame, terminal_only, "Terminal status")
        with self.assertRaisesRegex(AssertionError, "Files pixels"):
            harness.files_repainted(frame, terminal_only)
        pixels[:8 * 3] = bytes([255] * (8 * 3))
        harness.files_repainted(frame, (640, 480, bytes(pixels)))

    def test_either_disk_guard_boundary_mutation_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            disk = Path(tmp) / "scratch.raw"
            raw = bytearray(harness.TOTAL * 512)
            raw[24 * 512:24 * 512 + 8] = b"INDR027!"
            disk.write_bytes(raw)
            harness.guard(disk)
            for offset in (0, 24 * 512 - 1, 124 * 512, len(raw) - 1):
                changed = raw.copy()
                changed[offset] = 1
                disk.write_bytes(changed)
                with self.assertRaisesRegex(AssertionError, "guard sectors"):
                    harness.guard(disk)


if __name__ == "__main__":
    unittest.main()
