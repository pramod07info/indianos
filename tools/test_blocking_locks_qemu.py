#!/usr/bin/env python3
"""Actual PID1 writer/PID2 reader lock waits on selected disposable GPT media."""
import argparse
from pathlib import Path
import shutil
import tempfile
from test_normal_desktop_qemu import READY,spawn

QUEUED=b"IndianOS: PID 1 FILE_LOCK_WAIT_QUEUED"
TIMEOUT=b"IndianOS: PID 1 FILE_LOCK_WAIT_TIMEOUT"
ACQUIRED=b"IndianOS: PID 1 FILE_LOCK_WAIT_ACQUIRED"
HELD=b"IndianOS user: desktop lock probe writer timed out without replacing peer lock"
RELEASED=b"IndianOS user: desktop lock probe reader released original lock"
RESUMED=b"IndianOS user: desktop lock probe writer resumed after peer unlock"

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
            session.until(READY,60);verify_lock_proof(bytes(session.output))
            session.shot(folder/"lock-wait-ready.ppm")
            if disk.read_bytes()!=original:raise AssertionError("lock-only guest fixture modified persistent media")
        finally:
            (folder/"lock-wait.serial.log").write_bytes(session.output)
            if process.poll() is None:process.kill()
            process.wait(timeout=10);sock.close()
    print("PASS: real contended writer timed out, preserved peer lock and resumed after exact reader unlock")

if __name__=="__main__":main()
