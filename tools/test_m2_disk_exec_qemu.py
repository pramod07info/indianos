#!/usr/bin/env python3
"""Disposable 4-boot M2 native ring3 filesystem acceptance (test-only trust)."""
from __future__ import annotations
import argparse,os,selectors,subprocess,tempfile,time
from pathlib import Path
TOTAL,START,COUNT=128,24,100
STAGES=[
 "IndianOS: M2 disk boot1 installed ELF v1 executed",
 "IndianOS: M2 disk boot2 upgraded ELF v2 executed",
 "IndianOS: M2 disk boot3 rolled-back ELF v1 executed",
 "IndianOS: M2 disk boot4 removed app and rejected exec"]
DONE="IndianOS: M2 persisted ring3 package lifecycle PASSED"
def check_disk(path:Path, phase:int)->None:
 data=path.read_bytes()
 if len(data)!=TOTAL*512 or any(data[:START*512]) or any(data[(START+COUNT)*512:]):
  raise AssertionError("QEMU wrote outside disposable M2 partition")
 if data[START*512:START*512+8]!=b"INDR027!":
  raise AssertionError("M2 durablefs metadata missing")
 if phase<=3 and b"M2P4" not in data[START*512:(START+COUNT)*512]:
  raise AssertionError("large ELF M2P4 manifest was not persisted")
def boot(qemu:str,ovmf:Path,iso:Path,disk:Path,stage:str)->None:
 cmd=[qemu,"-machine","pc","-m","256M","-smp","1",
      "-display","none","-monitor","none","-serial","stdio",
      "-no-reboot","-no-shutdown","-boot","d",
      "-drive",f"if=pflash,format=raw,readonly=on,file={ovmf}",
      "-cdrom",str(iso),
      "-drive",f"if=ide,index=0,media=disk,format=raw,file={disk}"]
 process=subprocess.Popen(cmd,stdin=subprocess.DEVNULL,
  stdout=subprocess.PIPE,stderr=subprocess.STDOUT,bufsize=0)
 output=""
 selector=selectors.DefaultSelector()
 try:
  assert process.stdout is not None
  selector.register(process.stdout,selectors.EVENT_READ)
  end=time.monotonic()+50
  while time.monotonic()<end:
   for key,_ in selector.select(0.25):
    chunk=os.read(key.fileobj.fileno(),8192)
    if chunk:output+=chunk.decode("utf-8","replace")
    else:selector.unregister(key.fileobj)
   if "IndianOS: FATAL" in output:
    raise AssertionError("M2 kernel fatal:\n"+output[-6000:])
   if stage in output and DONE in output:
    if output.index(stage)>output.index(DONE):
     raise AssertionError("M2 serial sequence mismatch")
    if stage!=STAGES[-1]:
     required=["IndianOS: ring-3 user image staged",
       "IndianOS: ring-3 PMM frame count restored",
       "IndianOS: /bin/init exited through int 0x80"]
     if not all(x in output for x in required):
      raise AssertionError("ring3 did not execute and reap:\n"+output[-6000:])
    return
   if process.poll() is not None:break
  raise AssertionError("M2 guest timed out:\n"+output[-7000:])
 finally:
  selector.close()
  if process.poll() is None:process.terminate()
  try:process.communicate(timeout=3)
  except subprocess.TimeoutExpired:
   process.kill();process.communicate(timeout=3)
def main()->None:
 a=argparse.ArgumentParser()
 a.add_argument("--iso",type=Path,required=True)
 a.add_argument("--ovmf",type=Path,required=True)
 a.add_argument("--qemu",required=True)
 args=a.parse_args()
 if not args.iso.is_file() or not args.ovmf.is_file():
  a.error("missing ISO or firmware")
 with tempfile.TemporaryDirectory(prefix="indianos-m2-only-") as directory:
  disk=Path(directory)/"disposable-128-sectors.raw"
  disk.write_bytes(bytes(TOTAL*512))
  for n,stage in enumerate(STAGES,1):
   boot(args.qemu,args.ovmf.resolve(),args.iso.resolve(),disk,stage)
   check_disk(disk,n)
   print(f"M2 native ring3 disk boot {n}/4 PASS")
  print("M2 TEST-ONLY QEMU ring3 stored-image lifecycle passed")
if __name__=="__main__":main()
