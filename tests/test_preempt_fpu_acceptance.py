import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from smoke_boot import MILESTONES
from test_preempt_fpu_qemu import CONFIGURED, BEGIN, END, symbol_bounds, verify_preempt_fpu


class PreemptFpuEvidenceTests(unittest.TestCase):
    bounds = {1: (0x401000, 0x401010), 2: (0x402000, 0x402010)}

    def good(self):
        return "\n".join([*MILESTONES,
            "IndianOS: R024 APIC preemptive scheduler experiment begin", CONFIGURED,
            "IndianOS: PID 1 READY", "IndianOS: PID 2 READY",
            "IndianOS: R024 xAPIC supervisor UC mapping installed",
            "IndianOS: M009 two simultaneous user address spaces ready",
            "IndianOS: R024 calibrated periodic xAPIC IRQ at 100 Hz",
            "IndianOS: PID 1 DISPATCHED", "IndianOS user: M009 timer task A started",
            "IndianOS: PID 1 APIC_TIMER_PREEMPTED",
            "IndianOS: R024 timer interrupted user RIP=0x0000000000401002",
            "IndianOS: PID 1 YIELDED", "IndianOS: PID 2 DISPATCHED",
            "IndianOS user: M009 timer task B started", "IndianOS: PID 2 APIC_TIMER_PREEMPTED",
            "IndianOS: R024 timer interrupted user RIP=0x0000000000402002",
            "IndianOS: PID 2 YIELDED", "IndianOS: PID 1 DISPATCHED",
            "IndianOS user: R024 timer task A x87 SSE control and registers retained",
            "IndianOS user: M009 timer task A finished", "IndianOS: PID 1 EXITED", "IndianOS: PID 1 REAPED",
            "IndianOS: PID 2 DISPATCHED",
            "IndianOS user: R024 timer task B x87 SSE control and registers retained",
            "IndianOS user: M009 timer task B finished", "IndianOS: PID 2 EXITED", "IndianOS: PID 2 REAPED",
            "IndianOS: R024 APIC timer-preemptive round-robin PASSED",
            "IndianOS: M009 scheduler experiment complete"])

    def test_actual_seeded_loop_interrupts_and_checked_receipts_are_required(self):
        verify_preempt_fpu(self.good(), self.bounds)

    def test_outside_wrong_process_or_missing_rip_samples_cannot_pass(self):
        good = self.good()
        for value in ("0000000000400fff", "0000000000401010", "0000000000402002", "ffffffff80401002"):
            with self.assertRaises(AssertionError):
                verify_preempt_fpu(good.replace("0000000000401002", value), self.bounds)
        with self.assertRaises(AssertionError):
            verify_preempt_fpu(good.replace("IndianOS: R024 timer interrupted user RIP=0x", "stale RIP=0x"), self.bounds)

    def test_missing_duplicate_stale_or_late_context_receipts_cannot_pass(self):
        good = self.good()
        proof = "IndianOS user: R024 timer task A x87 SSE control and registers retained"
        for bad in (good.replace(proof, ""), good + "\n" + proof,
                    good.replace(proof, "stale " + proof), good.replace(CONFIGURED, ""),
                    good + "\nIndianOS: FATAL"):
            with self.assertRaises(AssertionError): verify_preempt_fpu(bad, self.bounds)
        late = good.replace(proof + "\n", "") + "\n" + proof
        with self.assertRaises(AssertionError): verify_preempt_fpu(late, self.bounds)

    def test_symbols_require_one_defined_ordered_user_loop(self):
        symbols = f" 1: 0000000000401000 0 NOTYPE GLOBAL DEFAULT 1 {BEGIN}\n 2: 0000000000401010 0 NOTYPE GLOBAL DEFAULT 1 {END}\n"
        self.assertEqual(symbol_bounds(symbols), (0x401000, 0x401010))
        for bad in ("", symbols + symbols, symbols.replace(END, "other"),
                    symbols.replace("401010", "401000"), symbols.replace("DEFAULT 1", "DEFAULT UND")):
            with self.assertRaises(AssertionError): symbol_bounds(bad)


if __name__ == "__main__": unittest.main()
