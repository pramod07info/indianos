#!/usr/bin/env python3
"""Actual PID1 writer/PID2 reader lock waits on selected disposable GPT media."""
import argparse
import binascii
import struct
from pathlib import Path
import shutil
import tempfile
from test_normal_desktop_qemu import READY,START,COUNT,spawn,verify_guard

QUEUED=b"IndianOS: PID 1 FILE_LOCK_WAIT_QUEUED"
TIMEOUT=b"IndianOS: PID 1 FILE_LOCK_WAIT_TIMEOUT"
ACQUIRED=b"IndianOS: PID 1 FILE_LOCK_WAIT_ACQUIRED"
HELD=b"IndianOS user: desktop lock probe writer timed out without replacing peer lock"
RELEASED=b"IndianOS user: desktop lock probe reader released original lock"
RESUMED=b"IndianOS user: desktop lock probe writer resumed after peer unlock"

OWNER_MARKERS=tuple(b"IndianOS user: desktop ownership probe "+suffix for suffix in (
    b"root-owned private file denied",b"new owner read exact committed bytes",
    b"retired owner denied",b"new group read exact committed bytes",
    b"revoked group denied",b"named user read exact committed bytes",
    b"explicit named user deny enforced",b"removed named user grant denied",b"original owner group mode restored; stale tokens rejected"))

def verify_owner_proof(output:bytes)->None:
    lines=output.replace(b"\r",b"").splitlines()
    if any(lines.count(marker)!=1 for marker in OWNER_MARKERS):
        raise AssertionError("missing or duplicate live ownership authorization receipt")
    positions=[output.find(marker) for marker in OWNER_MARKERS]+[output.find(QUEUED)]
    if min(positions)<0 or positions!=sorted(positions) or b"IndianOS: FATAL" in output:
        raise AssertionError("ownership grant/revocation did not precede lock continuation")

def metadata_head(image:bytes,slot:int):
    head=bytearray(image[(START+slot)*512:(START+slot+1)*512])
    if len(head)!=512 or head[:8]!=b"INDR027!":raise AssertionError("bad ownership metadata head")
    version,sectors,generation,checksum=struct.unpack_from("<IIQI",head,8)
    head[24:28]=bytes(4)
    if version not in (1,2,3) or sectors!=COUNT or binascii.crc32(head)!=checksum:
        raise AssertionError("unverified ownership metadata head")
    entries=[]
    for index in range(8):
        entry=bytes(head[32+index*40:72+index*40])
        acl=tuple(struct.unpack_from("<HH",head,352+index*8+slot*4) for slot in (0,1)) if version==3 else ((0,0),(0,0))
        active=[uid for uid,rights in acl if uid]
        if any(rights>7 or (uid==0 and rights!=0) for uid,rights in acl) or active!=sorted(set(active)) or (acl[0][0]==0 and acl[1][0]!=0):
            raise AssertionError("invalid named-user metadata")
        if not any(entry):
            if any(uid or rights for uid,rights in acl):raise AssertionError("orphan named-user metadata")
            entries.append(None);continue
        if entry[33]!=1 or entry[32]>=3:raise AssertionError("invalid ownership entry")
        permission=(0,0,0o777) if version==1 else struct.unpack_from("<HHH",entry,34)
        if permission[2]&~0o777:raise AssertionError("invalid persisted mode")
        entries.append((entry[:34],permission,acl))
    if any(head[416 if version==3 else 352:]):raise AssertionError("reserved metadata tail changed")
    return generation,tuple(entries)

def verify_owner_media(before:bytes,after:bytes)->None:
    verify_guard(before,after)
    begin,end=START*512,(START+2)*512
    if before[:begin]!=after[:begin] or before[end:]!=after[end:]:
        raise AssertionError("ownership-only fixture changed a data bank or unrelated sector")
    old=max((metadata_head(before,i) for i in (0,1)),key=lambda item:item[0])
    heads=sorted((metadata_head(after,i) for i in (0,1)),key=lambda item:item[0])
    if [item[0] for item in heads]!=[old[0]+11,old[0]+12]:
        raise AssertionError("missing twelve real ownership/mode/ACL publications")
    if any(struct.unpack_from("<I",after,(START+i)*512+8)[0]!=3 for i in (0,1)):
        raise AssertionError("named-user publication did not persist v3 metadata")
    target=[i for i,e in enumerate(old[1]) if e and e[0].split(b"\0",1)[0]==b"~fshared~readme"]
    if len(target)!=1 or old[1][target[0]][1]!=(0,0,0o777):
        raise AssertionError("ownership fixture requires original legacy root-owned readme")
    expected=list(old[1]);index=target[0]
    expected[index]=(expected[index][0],(0,0,0o640),((0,0),(0,0)))
    if heads[0][1]!=tuple(expected) or heads[1][1]!=old[1]:
        raise AssertionError("ownership checkpoints changed content/catalog or failed to restore permissions")

def verify_lock_proof(output:bytes)->None:
    ready=output.find(READY.encode())
    lines=output.replace(b"\r",b"").splitlines()
    if ready<0 or lines.count(QUEUED)!=2:
        raise AssertionError("missing two genuine contended lock continuations or desktop readiness")
    for marker in (TIMEOUT,ACQUIRED,HELD,RELEASED,RESUMED):
        if lines.count(marker)!=1:raise AssertionError("missing or duplicate lock handoff receipt")
    first=output.find(QUEUED);second=output.find(QUEUED,first+len(QUEUED))
    positions=[first,output.find(TIMEOUT),output.find(HELD),second,output.find(RELEASED),
               output.find(ACQUIRED),output.find(RESUMED),ready]
    if positions!=sorted(positions) or b"IndianOS: FATAL" in output:
        raise AssertionError("lock timeout, peer unlock and actual wakeup are unordered")

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ("iso","disk","ovmf","artifact-dir"):
        parser.add_argument("--"+name,type=Path,required=True)
    parser.add_argument("--qemu",default="qemu-system-x86_64")
    args=parser.parse_args();args.artifact_dir.mkdir(parents=True,exist_ok=True)
    folder=Path(tempfile.mkdtemp(prefix="lock-wait-",dir=args.artifact_dir)).resolve()
    with tempfile.TemporaryDirectory(prefix="lock-wait-gpt-") as temporary:
        disk=Path(temporary)/"selected.raw";shutil.copyfile(args.disk,disk)
        original=disk.read_bytes()
        process,sock,session=spawn(args.qemu,args.iso.resolve(),args.ovmf.resolve(),disk,folder,"lock-wait")
        try:
            session.until(READY,60);verify_owner_proof(bytes(session.output));verify_lock_proof(bytes(session.output))
            session.shot(folder/"lock-wait-ready.ppm")
            verify_owner_media(original,disk.read_bytes())
        finally:
            (folder/"lock-wait.serial.log").write_bytes(session.output)
            if process.poll() is None:process.kill()
            process.wait(timeout=10);sock.close()
    print("PASS: real owner/group grant and revocation; metadata-only twelve-checkpoint ACL restore; contended writer timed out, preserved peer lock and resumed after exact reader unlock")

if __name__=="__main__":main()
