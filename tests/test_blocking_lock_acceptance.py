import sys
import unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"tools"))
from test_blocking_locks_qemu import QUEUED,TIMEOUT,ACQUIRED,HELD,RELEASED,RESUMED,READY,verify_lock_proof

class BlockingLockEvidenceTests(unittest.TestCase):
    def good(self):return b"\n".join((QUEUED,TIMEOUT,HELD,QUEUED,RELEASED,ACQUIRED,RESUMED,READY.encode()))+b"\n"
    def test_requires_two_actual_queues_timeout_and_success_after_peer_release(self):
        verify_lock_proof(self.good())
    def test_missing_duplicate_reordered_and_stale_receipts_are_rejected(self):
        good=self.good()
        for marker in (QUEUED,TIMEOUT,ACQUIRED,HELD,RELEASED,RESUMED):
            for bad in (good.replace(marker+b"\n",b"",1),good+marker+b"\n",good.replace(marker,b"stale "+marker)):
                with self.assertRaises(AssertionError):verify_lock_proof(bad)
        wrong_order=b"\n".join((QUEUED,TIMEOUT,HELD,QUEUED,ACQUIRED,RELEASED,RESUMED,READY.encode()))
        for bad in (wrong_order,
                    good.replace(READY.encode(),b"not ready"),good+b"IndianOS: FATAL"):
            with self.assertRaises(AssertionError):verify_lock_proof(bad)

if __name__=="__main__":unittest.main()
