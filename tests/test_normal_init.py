import importlib.util
from pathlib import Path
import sys
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"tools"))
from test_normal_init_qemu import verify_trace,PID1_READY,STATUS,REJECTED
from test_boot_recovery_qemu import READY
class NormalTraceTests(unittest.TestCase):
    def trace(self):
        return "\n".join(["IndianOS: normal boot starting native PID1","PID 1 RUNNING",PID1_READY,
            "bootstrap commands: help status halt",STATUS,REJECTED,STATUS,REJECTED,STATUS,
            "bootstrap: unsupported command","bootstrap stopping PID1","PID 1 EXITED",
            "ring-3 PMM frame count restored","PID 1 REAPED",
            "normal PID1 exited and resources reclaimed","IndianOS recovery: reason=pid1-stopped",READY])
    def test_complete(self):verify_trace(self.trace())
    def test_missing_or_reordered_runtime_evidence_fails(self):
        for marker in [PID1_READY,"PID 1 RUNNING","PID 1 REAPED",REJECTED,STATUS,
                       "ring-3 PMM frame count restored",READY]:
            with self.subTest(marker=marker):
                with self.assertRaises(AssertionError):verify_trace(self.trace().replace(marker,"",1))
    def test_fatal_fails(self):
        with self.assertRaises(AssertionError):verify_trace(self.trace()+"\nFATAL")
