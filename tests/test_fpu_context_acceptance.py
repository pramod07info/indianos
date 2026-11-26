import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from test_fpu_context_qemu import READY, verify_context_proof


class ContextEvidenceTests(unittest.TestCase):
    def proof(self):
        demand = b"\n".join(
            f"IndianOS: PID {pid} DEMAND_STACK_PAGE_READY".encode()
            for pid in range(1, 5) for _ in range(2)
        ) + b"\n"
        cow=b"\n".join(
            f"IndianOS: PID {pid} {marker}".encode()
            for pid in range(1,5)
            for marker in ("COW_STACK_ZERO_SHARED_READY","COW_STACK_PRIVATE_READY","COW_STACK_ZERO_SHARED_READY")
        )+b"\n"
        return demand+cow+b"\n".join(
            f"IndianOS user: desktop PID{pid} x87 SSE context retained across yields".encode()
            for pid in range(1, 5)
        ) + b"\n" + READY.encode()

    def test_all_four_distinct_contexts_precede_readiness(self):
        verify_context_proof(self.proof())

    def test_missing_duplicate_or_post_ready_proof_is_rejected(self):
        lines = self.proof().splitlines()
        for payload in (b"\n".join(lines[1:]), self.proof() + b"\n" + lines[-2],
                        b"\n".join(lines[:-2] + lines[-1:] + lines[-2:-1]),
                        b"\n".join(lines[:4])):
            with self.assertRaises(AssertionError):
                verify_context_proof(payload)

    def test_private_split_requires_preceding_shared_zero_and_remaining_zero_alias(self):
        good=self.proof();shared=b"IndianOS: PID 1 COW_STACK_ZERO_SHARED_READY";
        private=b"IndianOS: PID 1 COW_STACK_PRIVATE_READY"
        for payload in (good.replace(shared+b"\n",b"",1),good.replace(private,b"missing"),
                        good.replace(shared+b"\n"+private,private+b"\n"+shared,1)):
            with self.assertRaises(AssertionError):verify_context_proof(payload)


if __name__ == "__main__":
    unittest.main()
