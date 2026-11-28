import sys
import unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"tools"))
from test_r028_network_qemu import MARKERS,IRQ_ARMED,IRQ_STOPPED,verify_irq_trace

class NicIrqEvidenceTests(unittest.TestCase):
    def test_requires_actual_interrupt_receipt_after_verified_dma_retirement(self):
        ordered=(MARKERS[1],IRQ_ARMED,MARKERS[2],MARKERS[-2],IRQ_STOPPED,MARKERS[-1])
        verify_irq_trace("\n".join(ordered))
        for marker in ordered:
            with self.assertRaises(AssertionError):verify_irq_trace("\n".join(ordered).replace(marker,""))
            with self.assertRaises(AssertionError):verify_irq_trace("\n".join(ordered)+marker)
        with self.assertRaises(AssertionError):verify_irq_trace("\n".join(reversed(ordered)))
        with self.assertRaises(AssertionError):verify_irq_trace("\n".join(ordered)+"IndianOS: FATAL")

if __name__=="__main__":unittest.main()
