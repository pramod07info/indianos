#!/usr/bin/env python3
"""R126 isolated real two-vCPU Limine AP entry proof in QEMU only.

This is NOT SMP scheduler, IPI, TLB shootdown, physical CPU or IOMMU proof.
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
)

def main():
    p=argparse.ArgumentParser()
    p.add_argument("--iso",type=Path,required=True)
    p.add_argument("--ovmf",type=Path,required=True)
    p.add_argument("--qemu",required=True)
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
                raise AssertionError("real AP admission denied: "+out[-6000:])
            if all(marker in out for marker in MARKERS):
                print("R126 PASS: two QEMU vCPUs, AP entered real Rust, CPUID matched and BSP acknowledged")
                return
            if proc.poll() is not None:break
        raise AssertionError("R126 required AP boot proof missing: "+transcript[-6000:].decode("utf-8","replace"))
    finally:
        sel.close()
        if proc.poll() is None:proc.kill()
        proc.communicate(timeout=5)

if __name__=="__main__":main()
