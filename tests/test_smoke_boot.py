"""Host-side unit tests for strictly ordered serial milestone recognition."""
import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from smoke_boot import (assert_milestones, assert_expected_exception,
                        assert_expected_pagefault, assert_expected_shadow, MILESTONES)  # noqa: E402


class SmokeBootTests(unittest.TestCase):
    def test_accepts_expected_order_with_additional_messages(self):
        assert_milestones("\nextra\n" + "\r\nother\r\n".join(MILESTONES) + "\r\n")

    def test_rejects_old_m002_only_transcript(self):
        with self.assertRaises(AssertionError):
            assert_milestones("\n".join(MILESTONES[:-2]))

    def test_accepts_page_fault_reporting(self):
        transcript = "\n".join([*MILESTONES,
            "IndianOS: triggering intentional page fault",
            "IndianOS: FATAL CPU exception vector=0x000000000000000e error=0x0",
            "IndianOS: page fault cr2=0x00007fff00000000 present=0 write=0"])
        assert_expected_pagefault(transcript)

    def test_rejects_wrong_cr2(self):
        transcript = "\n".join([*MILESTONES,
            "IndianOS: triggering intentional page fault",
            "IndianOS: FATAL CPU exception vector=0x000000000000000e",
            "IndianOS: page fault cr2=0x0000000000000000"])
        with self.assertRaises(AssertionError):
            assert_expected_pagefault(transcript)

    def test_accepts_shadow_switch_with_restoration(self):
        assert_expected_shadow("\n".join([*MILESTONES[:-1],
            "IndianOS: shadow CR3 switched and restored", MILESTONES[-1]]))

    def test_rejects_missing_shadow_restore(self):
        with self.assertRaises(AssertionError):
            assert_expected_shadow("\n".join(MILESTONES))

    def test_rejects_out_of_order(self):
        with self.assertRaises(AssertionError):
            assert_milestones("\n".join(reversed(MILESTONES)))

    def test_rejects_missing_event(self):
        with self.assertRaises(AssertionError):
            assert_milestones(MILESTONES[0] + "\n" + MILESTONES[2])

    def test_rejects_kernel_fatal(self):
        with self.assertRaises(AssertionError):
            assert_milestones("\n".join(MILESTONES) + "\nIndianOS: FATAL boot information invalid")

    def test_accepts_deliberate_ud2_fault(self):
        output = "\n".join([*MILESTONES[:5],
                           "IndianOS: triggering intentional UD2",
                           "IndianOS: FATAL CPU exception vector=0x0000000000000006 error=0x0"])
        assert_expected_exception(output)

    def test_rejects_unexpected_cpu_exception(self):
        output = "\n".join([*MILESTONES[:5],
                           "IndianOS: triggering intentional UD2",
                           "IndianOS: FATAL CPU exception vector=0x000000000000000e"])
        with self.assertRaises(AssertionError):
            assert_expected_exception(output)

    def test_rejects_exception_handler_that_returns(self):
        output = "\n".join([*MILESTONES[:5],
                           "IndianOS: triggering intentional UD2",
                           "IndianOS: FATAL CPU exception vector=0x0000000000000006",
                           "IndianOS: milestone 002 complete"])
        with self.assertRaises(AssertionError):
            assert_expected_exception(output)


if __name__ == "__main__":
    unittest.main()
