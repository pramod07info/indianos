#!/usr/bin/env python3
"""R126 isolated real two-vCPU Limine AP entry proof in QEMU only.

Includes polled cross-CPU TLB remapping; this is NOT SMP scheduler, IPI,
production frame-reuse, physical CPU or IOMMU proof.
No physical disk is connected. Both CPUs stop before kernel PMM is started.
"""
import argparse
import os
from pathlib import Path
import selectors
import subprocess
import time

MARKERS=(
    "IndianOS: kernel entered",
    "IndianOS: R125 QEMU two-CPU Limine MP inventory validated; AP parked",
    "IndianOS: R126 QEMU secondary AP executed Rust and acknowledged BSP",
    "IndianOS: R127 QEMU real AP denied BSP retirement gate 8192 times",
    "IndianOS: R131 QEMU 64 ordered cross-CPU lifecycle receipts acknowledged",
    "IndianOS: R121 QEMU all 64 AP lifecycle receipt replays denied",
    "IndianOS: R122 QEMU AP proof completed before lifecycle acknowledgement",
    "IndianOS: R132 QEMU AP invalidated 64 remapped pages before acknowledgement",
)
PRIMED="IndianOS: R132 QEMU AP primed shared page translation"
STALE="IndianOS: R132 QEMU stale page refused acknowledgement"

def stale_failure_proved(output):
    return (output.count(PRIMED)==1 and output.count(STALE)==1 and "IndianOS: FATAL" in output and
            output.index(PRIMED)<output.index(STALE)<output.index("IndianOS: FATAL") and
            MARKERS[-1] not in output and MARKERS[4] not in output)

def positive_proved(output):
    required=MARKERS[:2]+(PRIMED,)+MARKERS[2:]
    return (all(output.count(marker)==1 for marker in required) and
            [output.index(marker) for marker in required]==sorted(output.index(marker) for marker in required) and
            STALE not in output and "IndianOS: FATAL" not in output)

def main():
    p=argparse.ArgumentParser()
    p.add_argument("--iso",type=Path,required=True)
    p.add_argument("--ovmf",type=Path,required=True)
    p.add_argument("--qemu",required=True)
    p.add_argument("--expect-stale-tlb",action="store_true")
    p.add_argument("--serial-log",type=Path)
    a=p.parse_args()
    if not a.iso.is_file() or not a.ovmf.is_file():
        p.error("missing exact QEMU ISO/OVMF artifact")
    command=[
        a.qemu,"-machine","pc","-m","256M","-smp","2",
        "-display","none","-monitor","none","-serial","stdio",
        "-no-reboot","-boot","d",
        "-drive",f"if=pflash,format=raw,readonly=on,file={a.ovmf.resolve()}",
        "-cdrom",str(a.iso.resolve())
    ]
    proc=subprocess.Popen(command,stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,stderr=subprocess.STDOUT,bufsize=0)
    transcript=bytearray()
    assert proc.stdout is not None
    sel=selectors.DefaultSelector()
    sel.register(proc.stdout,selectors.EVENT_READ)
    try:
        deadline=time.monotonic()+45
        while time.monotonic()<deadline:
            for key,_ in sel.select(0.15):
                chunk=os.read(key.fileobj.fileno(),4096)
                if chunk:transcript.extend(chunk)
            out=transcript.decode("utf-8","replace")
            if "IndianOS: FATAL" in out:
                if a.expect_stale_tlb and stale_failure_proved(out):
                    print("R132 PASS: real stale AP translation denied acknowledgement and BSP success")
                    return
                raise AssertionError("real AP admission denied: "+out[-6000:])
            if all(marker in out for marker in MARKERS):
                if a.expect_stale_tlb:raise AssertionError("missing INVLPG did not expose a stale translation")
                if not positive_proved(out):raise AssertionError("duplicate or unordered AP/TLB proof")
                print("R126/R132 PASS: real two-vCPU entry and 64 acknowledged remapped-page invalidations")
                return
            if proc.poll() is not None:break
        raise AssertionError("R126 required AP boot proof missing: "+transcript[-6000:].decode("utf-8","replace"))
    finally:
        if a.serial_log is not None:
            a.serial_log.parent.mkdir(parents=True,exist_ok=True)
            a.serial_log.write_bytes(transcript)
        sel.close()
        if proc.poll() is None:proc.kill()
        proc.communicate(timeout=5)

if __name__=="__main__":main()
