import sys
import struct
import binascii
import unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"tools"))
from test_blocking_locks_qemu import QUEUED,TIMEOUT,ACQUIRED,HELD,RELEASED,RESUMED,READY,verify_lock_proof,OWNER_MARKERS,verify_owner_proof,verify_owner_media,START,COUNT

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

class OwnershipEvidenceTests(unittest.TestCase):
    def proof(self):return b"\n".join(OWNER_MARKERS+(QUEUED,))+b"\n"
    def head(self,generation,mode=0o777,owner=0):
        head=bytearray(512);head[:8]=b"INDR027!"
        struct.pack_into("<IIQ",head,8,2,COUNT,generation)
        name=b"~fshared~readme";head[32:32+len(name)]=name
        struct.pack_into("<II",head,56,14,binascii.crc32(b"HELLO INDIANOS"))
        head[65]=1;struct.pack_into("<HHH",head,66,owner,0,mode)
        struct.pack_into("<I",head,24,binascii.crc32(head));return head
    def media(self):
        before=bytearray(256*512)
        before[START*512:(START+1)*512]=self.head(1)
        before[(START+1)*512:(START+2)*512]=self.head(2)
        before[(START+2)*512:(START+2)*512+14]=b"HELLO INDIANOS"
        after=bytearray(before)
        after[START*512:(START+1)*512]=self.head(9,0o600)
        after[(START+1)*512:(START+2)*512]=self.head(10)
        return before,after
    def test_live_ownership_receipts_require_each_grant_and_revocation_in_order(self):
        proof=self.proof();verify_owner_proof(proof)
        for marker in OWNER_MARKERS:
            for bad in (proof.replace(marker+b"\n",b""),proof+marker+b"\n",
                        proof.replace(marker,b"stale "+marker)):
                with self.assertRaises(AssertionError):verify_owner_proof(bad)
        for bad in (b"\n".join(tuple(reversed(OWNER_MARKERS))+(QUEUED,)),
                    proof.replace(QUEUED,b"not queued"),proof+b"IndianOS: FATAL"):
            with self.assertRaises(AssertionError):verify_owner_proof(bad)
    def test_only_eight_metadata_checkpoints_restore_original_catalog_and_permissions(self):
        before,after=self.media();verify_owner_media(before,after)
    def test_modified_data_partition_geometry_or_checksum_cannot_pass(self):
        before,after=self.media()
        for offset in (0,(START+2)*512,START*512+24):
            bad=bytearray(after);bad[offset]^=1
            with self.assertRaises(AssertionError):verify_owner_media(before,bad)
        with self.assertRaises(AssertionError):verify_owner_media(before,after[:-1])
        with self.assertRaises(AssertionError):verify_owner_media(before,before)
    def test_valid_crc_does_not_hide_wrong_generation_or_unrestored_owner_or_mode(self):
        before,after=self.media()
        for head in (self.head(9),self.head(10,0o600),self.head(10,0o777,1000)):
            bad=bytearray(after);bad[(START+1)*512:(START+2)*512]=head
            with self.assertRaises(AssertionError):verify_owner_media(before,bad)
        # A data descriptor edit with a correct metadata CRC still changes the
        # catalog; metadata-only writes cannot launder payload substitutions.
        bad=bytearray(after);head=self.head(10);head[56]=15
        head[24:28]=bytes(4);struct.pack_into("<I",head,24,binascii.crc32(head))
        bad[(START+1)*512:(START+2)*512]=head
        with self.assertRaises(AssertionError):verify_owner_media(before,bad)

if __name__=="__main__":unittest.main()
