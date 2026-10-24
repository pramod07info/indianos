#!/usr/bin/env python3
"""R031.3 four isolated QEMU boots, hierarchical disk and corrupt CRC recovery."""
import argparse
import os
import selectors
import subprocess
import tempfile
import time
from pathlib import Path
from r030_pkg import build,verify
MARKERS=[
 "R031.3 boot1 created directories, VFS files and persistent phase",
 "R031.3 boot2 persisted atomic rename and overwritten file",
 "R031.3 boot3 committed redundant data for tear-recovery injection",
 "R031.3 boot4 recovered torn metadata and deleted empty directory",
]
PASSED="IndianOS: R031.3 HIERARCHICAL VFS, ATOMIC RENAME AND REBOOT RECOVERY PASSED"
START=24
TOTAL=128
def boot(qemu,iso,ovmf,disk,marker):
    cmd=[qemu,"-machine","pc","-m","256M","-smp","1",
         "-display","none","-monitor","none","-serial","stdio",
         "-no-reboot","-no-shutdown","-boot","d",
         "-drive",f"if=pflash,format=raw,readonly=on,file={ovmf}",
         "-cdrom",str(iso),
         "-drive",f"if=ide,index=0,media=disk,format=raw,file={disk}"]
    proc=subprocess.Popen(cmd,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT,bufsize=0)
    selector=selectors.DefaultSelector()
    output=""
    try:
        selector.register(proc.stdout,selectors.EVENT_READ)
        deadline=time.monotonic()+45
        while time.monotonic()<deadline:
            for key,_ in selector.select(0.25):
                buf=os.read(key.fileobj.fileno(),8192)
                if buf:output+=buf.decode("utf-8","replace")
                else:selector.unregister(key.fileobj)
            if "IndianOS: FATAL" in output:
                raise AssertionError("guest failure\n"+output[-7000:])
            requirements=[marker,
              "R030.2 installed native ELF ran in isolated PID2 ring3",
              "R031.3 verified persistent hierarchical storage and ring3 isolation",
              PASSED]
            if all(x in output for x in requirements):
                if not all(output.index(x)<output.index(y) for x,y in zip(requirements,requirements[1:])):
                    raise AssertionError("invalid guest milestones\n"+output[-7000:])
                return
            if proc.poll() is not None:break
        raise AssertionError("QEMU boot timed out\n"+output[-8000:])
    finally:
        selector.close()
        if proc.poll() is None:proc.terminate()
        try:proc.communicate(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill();proc.communicate(timeout=3)
def check_disk(disk):
    raw=disk.read_bytes()
    if len(raw)!=TOTAL*512 or any(raw[:START*512]) or any(raw[124*512:]):
        raise AssertionError("kernel modified QEMU guard sectors")
    if raw[START*512:START*512+8]!=b"INDR027!":
        raise AssertionError("missing durable filesystem superblock")
def corrupt_latest(disk):
    buf=bytearray(disk.read_bytes())
    g0=int.from_bytes(buf[START*512+16:START*512+24],"little")
    g1=int.from_bytes(buf[(START+1)*512+16:(START+1)*512+24],"little")
    if max(g0,g1)<2:raise AssertionError("metadata generations absent")
    latest=START if g0>=g1 else START+1
    buf[latest*512+24]^=0x5a
    disk.write_bytes(buf)
    check_disk(disk)
    print("R031.3 latest metadata checksum deliberately corrupted")
def main():
    p=argparse.ArgumentParser()
    p.add_argument("--iso",required=True,type=Path)
    p.add_argument("--qemu",required=True)
    p.add_argument("--ovmf",required=True,type=Path)
    p.add_argument("--worker",required=True,type=Path)
    a=p.parse_args()
    for path in [a.iso,a.ovmf,a.worker]:
        if not path.is_file():p.error(f"missing file: {path}")
    with tempfile.TemporaryDirectory(prefix="indianos-r0313-vfs-") as d:
        root=Path(d);priv=root/"key";pub=root/"pub"
        subprocess.run(["openssl","genpkey","-algorithm","Ed25519","-out",str(priv)],check=True)
        subprocess.run(["openssl","pkey","-in",str(priv),"-pubout","-out",str(pub)],check=True)
        worker=a.worker.read_bytes()
        meta,verified=verify(build(worker,"indianos-worker","1.0.0",priv),pub)
        if verified!=worker or meta["target"]!="indianos-x86_64-elf-v1":
            raise AssertionError("staged worker signature invalid")
        disk=root/"128-sectors-disposable.raw"
        disk.write_bytes(bytes(TOTAL*512))
        for idx,marker in enumerate(MARKERS):
            boot(a.qemu,a.iso.resolve(),a.ovmf.resolve(),disk,marker)
            check_disk(disk)
            print(f"R031.3 stage {idx+1}/4 passed")
            if idx==2:corrupt_latest(disk)
    print("R031.3 PASS: hierarchical tree, atomic rename, reboot persistence, "
          "corrupt metadata fallback, sector guard and ring3 worker")
if __name__=="__main__":main()
