#!/usr/bin/env python3
"""R114: actual OVMF cold boot monotonic epoch on an exclusive disposable disk.
This is a QEMU profile fixture, NOT TPM/firmware attestation. No host disks.
"""
from __future__ import annotations
import argparse
import hashlib
import os
from pathlib import Path
import selectors
import struct
import subprocess
import tempfile
import time

SIZE=64
START=40
MAGIC=b"IEPOCH01"
PASS="IndianOS: R114 QEMU boot epoch advanced and recovery receipt verified"
def record(epoch:int,prior:bytes,journal:int=21)->bytes:
    sector=bytearray(512)
    sector[:8]=MAGIC
    struct.pack_into("<QQ",sector,8,epoch,journal)
    sector[24]=0
    struct.pack_into("<Q",sector,32,9)
    sector[40:72]=bytes([0x42])*32
    sector[72:104]=prior
    sector[104:136]=hashlib.sha256(sector[:104]).digest()
    return bytes(sector)
def decode(raw:bytes)->tuple[int,bytes]:
    if len(raw)!=512 or raw[:8]!=MAGIC or any(raw[25:32]) or any(raw[136:]):
        raise AssertionError("bad durable epoch record structure")
    if raw[104:136]!=hashlib.sha256(raw[:104]).digest():
        raise AssertionError("durable epoch record checksum failed")
    if raw[40:72]!=bytes([0x42])*32 or raw[24]!=0 or struct.unpack_from("<Q",raw,32)[0]!=9:
        raise AssertionError("wrong boot profile or slot")
    return struct.unpack_from("<Q",raw,8)[0],raw[72:104]
def boot(qemu:str,iso:Path,ovmf:Path,disk:Path,negative:bool=False):
    command=[qemu,"-machine","pc","-m","256M","-smp","1","-display","none",
             "-monitor","none","-serial","stdio","-no-reboot","-boot","d",
             "-drive",f"if=pflash,format=raw,readonly=on,file={ovmf}",
             "-cdrom",str(iso),
             "-drive",f"if=ide,index=0,media=disk,format=raw,file={disk}"]
    process=subprocess.Popen(command,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT,bufsize=0)
    out=bytearray()
    assert process.stdout is not None
    sel=selectors.DefaultSelector()
    sel.register(process.stdout,selectors.EVENT_READ)
    try:
        deadline=time.monotonic()+45
        while time.monotonic()<deadline:
            for key,_ in sel.select(.15):
                data=os.read(key.fileobj.fileno(),4096)
                if data:out.extend(data)
            text=out.decode("utf8","replace")
            if negative and "IndianOS: FATAL R114 epoch journal not preprovisioned" in text:
                return
            if not negative and PASS in text:
                return
            # Negative-control is SUPPOSED to emit FATAL. Serial stdout
            # arrives in short chunks; do not fail on a partially received
            # prefix before the complete expected denial marker arrives.
            if not negative and "IndianOS: FATAL" in text:
                raise AssertionError("guest epoch failed:\n"+text[-7000:])
            if process.poll() is not None:
                break
        raise AssertionError("missing real QEMU boot evidence:\n"+
                             out[-7000:].decode("utf8","replace"))
    finally:
        sel.close()
        if process.poll() is None:process.kill()
        process.communicate(timeout=5)
def main():
    p=argparse.ArgumentParser()
    p.add_argument("--iso",type=Path,required=True)
    p.add_argument("--ovmf",type=Path,required=True)
    p.add_argument("--qemu",required=True)
    a=p.parse_args()
    with tempfile.TemporaryDirectory(prefix="r114-epoch-") as folder:
        root=Path(folder)
        disk=root/"disposable64.raw"
        before=bytearray(SIZE*512)
        raw=record(1,bytes([0x24])*32,journal=20)
        before[START*512:(START+1)*512]=raw
        before[(START+1)*512:(START+2)*512]=raw
        disk.write_bytes(before)
        for epoch in (2,3,4):
            image_before=disk.read_bytes()
            boot(a.qemu,a.iso.resolve(),a.ovmf.resolve(),disk)
            image_after=disk.read_bytes()
            assert len(image_after)==SIZE*512
            assert image_after[:START*512]==image_before[:START*512]
            assert image_after[(START+2)*512:]==image_before[(START+2)*512:]
            a0=image_after[START*512:(START+1)*512]
            a1=image_after[(START+1)*512:(START+2)*512]
            e0,p0=decode(a0);e1,p1=decode(a1)
            assert max(e0,e1)==epoch and min(e0,e1)==epoch-1,(e0,e1)
            newer,older,prev=(a0,a1,p0) if e0>e1 else (a1,a0,p1)
            assert prev==hashlib.sha256(older).digest()
            print(f"R114 actual OVMF cold boot {epoch-1}: persisted epoch {epoch}")
        corrupt=bytearray(disk.read_bytes())
        corrupt[START*512+104]^=0x3c
        corrupt[(START+1)*512+104]^=0x5a
        disk.write_bytes(corrupt)
        control=disk.read_bytes()
        boot(a.qemu,a.iso.resolve(),a.ovmf.resolve(),disk,negative=True)
        assert disk.read_bytes()==control,"corrupt epoch negative-control mutated disk"
    print("R114 PASS: three actual OVMF epoch advances and corruption denial")
if __name__=="__main__":
    main()
