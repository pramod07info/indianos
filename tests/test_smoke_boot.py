"""Host-side unit tests for strictly ordered serial milestone recognition."""
import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from smoke_boot import assert_milestones, MILESTONES  # noqa: E402


class SmokeBootTests(unittest.TestCase):
    def test_accepts_expected_order_with_additional_messages(self):
        assert_milestones("\nextra\n" + "\r\nother\r\n".join(MILESTONES) + "\r\n")

    def test_rejects_out_of_order(self):
        with self.assertRaises(AssertionError):
            assert_milestones("\n".join(reversed(MILESTONES)))

    def test_rejects_missing_event(self):
        with self.assertRaises(AssertionError):
            assert_milestones(MILESTONES[0] + "\n" + MILESTONES[2])

    def test_rejects_kernel_fatal(self):
        with self.assertRaises(AssertionError):
            assert_milestones("\n".join(MILESTONES) + "\nIndianOS: FATAL boot information invalid")


if __name__ == "__main__":
    unittest.main()
