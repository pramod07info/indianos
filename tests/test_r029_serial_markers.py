"""Regression tests for R029 cumulative serial-marker handling."""
import unittest
from tools.test_r029_desktop_qemu import QemuSession


class SerialMarkerTests(unittest.TestCase):
    def test_repeated_focus_ack_waits_for_new_guest_output(self):
        marker = "IndianOS: R029 keyboard focused Terminal window"
        session = object.__new__(QemuSession)
        session.output = bytearray(marker.encode())
        since = len(session.output)
        calls = []

        def drain(_duration):
            calls.append(True)
            if len(calls) == 2:
                session.output.extend(b"\\n" + marker.encode())

        session.drain_serial = drain
        session.until(marker, deadline=1, since=since)
        self.assertEqual(len(calls), 2)

    def test_stale_ack_alone_does_not_satisfy_new_event(self):
        session = object.__new__(QemuSession)
        session.output = bytearray(b"ready")
        session.drain_serial = lambda _duration: None
        with self.assertRaisesRegex(AssertionError, "missing serial marker"):
            session.until("ready", deadline=0.001, since=len(session.output))

    def test_initial_boot_marker_can_use_existing_output(self):
        session = object.__new__(QemuSession)
        session.output = bytearray(b"ready")
        session.drain_serial = lambda _duration: self.fail("unexpected wait")
        session.until("ready", deadline=0.1)


if __name__ == "__main__":
    unittest.main()
