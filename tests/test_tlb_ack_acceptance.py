import sys
import unittest
from pathlib import Path

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"tools"))
from test_r118_ap_qemu import PRIMED, STALE, MARKERS, stale_failure_proved, positive_proved

class TlbAckEvidenceTests(unittest.TestCase):
    def test_stale_read_requires_priming_refusal_and_terminal_bsp_failure(self):
        self.assertTrue(stale_failure_proved(PRIMED+"\n"+STALE+"\nIndianOS: FATAL"))

    def test_missing_reordered_or_successful_receipts_cannot_pass_negative_control(self):
        good=PRIMED+"\n"+STALE+"\nIndianOS: FATAL"
        for output in (good.replace(PRIMED,""),good.replace(STALE,""),
                       good.replace("IndianOS: FATAL",""),STALE+"\n"+PRIMED+"\nIndianOS: FATAL",
                       good+MARKERS[-1],good+MARKERS[4]):
            self.assertFalse(stale_failure_proved(output))

    def test_positive_requires_all_ordered_receipts_and_no_refusal(self):
        ordered=MARKERS[:2]+(PRIMED,)+MARKERS[2:]
        good="\n".join(ordered)
        self.assertTrue(positive_proved(good))
        for marker in ordered:
            self.assertFalse(positive_proved(good.replace(marker,"")))
            self.assertFalse(positive_proved(good+marker))
        self.assertFalse(positive_proved("\n".join(reversed(ordered))))
        self.assertFalse(positive_proved(good+STALE))
        self.assertFalse(positive_proved(good+"IndianOS: FATAL"))

if __name__=="__main__":unittest.main()
