#!/usr/bin/env python3
"""Four real QEMU boots with *one* disposable disk and installed native ELF.

No arbitrary disk pathname accepted; a new temporary 128-sector raw file is
created here and the tested kernel can only access sectors 24..123.
The trusted demo ELF is signed and verified before guest execution.
"""
from __future__ import annotations
import argparse,os,selectors,subprocess,tempfile,time
from pathlib import Path
from r030_pkg import build,verify
TOTAL=128
START=24
COUNT=100
STAGES=[
 "R030.1 first boot persistent note created; R030.2 app v1 installed",
 "R030.1 reboot persisted user file; R030.2 upgraded installed app to v2",
 "R030.2 previously installed app rollback to v1 persisted",
 "R030.2 native app removed; launch denied; user file deleted"
]
FINAL="IndianOS: R030.1-2 persistent files and native app lifecycle PASSED"
def check_guard(path):
    data=path.read_bytes()
    if len(data)!=TOTAL*512 or any(data[:START*512]) or any(data[(START+COUNT)*512:]):
        raise AssertionError("R030 QEMU wrote outside the disposable bounded partition")
    if data[START*512:START*512+8]!=b"INDR027!":
        raise AssertionError("R030 durable metadata missing on disk")
def boot(qemu,ovmf,iso,disk,marker):
    cmd=[qemu,"-machine","pc","-m","256M","-smp","1",
         "-display","none","-monitor","none","-serial","stdio",
         "-no-reboot","-no-shutdown","-boot","d",
         "-drive",f"if=pflash,format=raw,readonly=on,file={ovmf}",
         "-cdrom",str(iso),
         "-drive",f"if=ide,index=0,media=disk,format=raw,file={disk}"]
    proc=subprocess.Popen(cmd,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT,bufsize=0)
    selector=selectors.DefaultSelector()
    text=""
    try:
        selector.register(proc.stdout,selectors.EVENT_READ)
        end=time.monotonic()+40
        while time.monotonic()<end:
            for key,_ in selector.select(0.25):
                payload=os.read(key.fileobj.fileno(),8192)
                if payload:text+=payload.decode("utf-8","replace")
                else:selector.unregister(key.fileobj)
            if "IndianOS: FATAL" in text:
                raise AssertionError("kernel failed\n"+text[-6000:])
            # Stage 4 intentionally removes the app *after* PID2 exits.
            native="R030.2 installed native ELF ran in isolated PID2 ring3"
            lifecycle="R030.1-2 ring3 lifecycle clean on persistent QEMU volume"
            expected=[native,marker,lifecycle,FINAL] if marker==STAGES[3] else [
                marker,native,lifecycle,FINAL]
            if all(x in text for x in expected):
                if not all(text.index(x)<text.index(y) for x,y in zip(expected,expected[1:])):
                    raise AssertionError("R030 serial milestones in wrong order\n"+text[-6000:])
                return
            if proc.poll() is not None:break
        raise AssertionError("R030 guest reboot/installation timeout\n"+text[-6000:])
    finally:
        selector.close()
        if proc.poll() is None:proc.terminate()
        try:proc.communicate(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill();proc.communicate(timeout=3)
def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--iso",required=True,type=Path)
    parser.add_argument("--qemu",required=True)
    parser.add_argument("--ovmf",required=True,type=Path)
    parser.add_argument("--worker",required=True,type=Path)
    a=parser.parse_args()
    for file in (a.iso,a.ovmf,a.worker):
        if not file.is_file():parser.error(f"missing input {file}")
    with tempfile.TemporaryDirectory(prefix="indianos-r0301-2-disposable-") as tmp:
        p=Path(tmp)
        priv=p/"trusted.key";pub=p/"trusted.pub"
        subprocess.run(["openssl","genpkey","-algorithm","Ed25519","-out",str(priv)],check=True)
        subprocess.run(["openssl","pkey","-in",str(priv),"-pubout","-out",str(pub)],check=True)
        worker=a.worker.read_bytes()
        pkg=build(worker,"indianos-worker","1.0.0",priv)
        meta,verified=verify(pkg,pub)
        if verified!=worker or meta["target"]!="indianos-x86_64-elf-v1":
            raise AssertionError("native app signature mismatch")
        disk=p/"sandbox-only-128.raw"
        disk.write_bytes(bytes(TOTAL*512))
        for index,stage in enumerate(STAGES):
            boot(a.qemu,a.ovmf.resolve(),a.iso.resolve(),disk,stage)
            check_guard(disk)
            print(f"R030.1+30.2 boot {index+1}/4: signed-staged native "
                  f"app + persistent volume OK")
        print("R030.1/R030.2 PASS: four reboot stages, read/create/delete, "
              "install/update/rollback/remove, signed host staging, "
              "ring3 app launch and disk guard integrity")
if __name__=="__main__":main()
