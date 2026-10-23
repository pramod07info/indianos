#!/usr/bin/env python3
"""Guard against stale R029 serial markers when GUI commands repeat."""
import importlib.util
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).with_name("test_r029_desktop_qemu.py")
spec = importlib.util.spec_from_file_location("r029_qemu", SCRIPT)
assert spec is not None and spec.loader is not None
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class SerialCheckpointTests(unittest.TestCase):
    def fake_session(self, previous: bytes):
        session = module.QemuSession.__new__(module.QemuSession)
        session.output = bytearray(previous)
        return session

    def test_duplicate_keyboard_focus_waits_for_new_occurrence(self):
        marker = "IndianOS: R029 keyboard focused Terminal window"
        session = self.fake_session((marker + "\nother event\n").encode())
        checkpoint = len(session.output)
        emitted = []
        def receive(_seconds):
            emitted.append(True)
            if len(emitted) == 1:
                session.output.extend(b"mouse moved\n")
            else:
                session.output.extend((marker + "\n").encode())
        session.drain_serial = receive
        session.until(marker, deadline=1, from_offset=checkpoint)
        self.assertEqual(len(emitted), 2)
        self.assertEqual(session.output[checkpoint:].count(marker.encode()), 1)

    def test_old_marker_cannot_satisfy_new_keyboard_event(self):
        marker = "IndianOS: R029 keyboard focused Files window"
        session = self.fake_session((marker + "\n").encode())
        checkpoint = len(session.output)
        session.drain_serial = lambda seconds: None
        with self.assertRaisesRegex(AssertionError, "missing NEW serial marker"):
            session.until(marker, deadline=0, from_offset=checkpoint)

    def test_invalid_checkpoint_is_rejected(self):
        session = self.fake_session(b"some output")
        session.drain_serial = lambda seconds: None
        with self.assertRaises(ValueError):
            session.until("some output", deadline=0, from_offset=100)


if __name__ == "__main__":
    unittest.main()
