#!/usr/bin/env python3
"""Disposable 4-boot M2 native ring3 filesystem acceptance (test-only trust)."""
from __future__ import annotations
import argparse,hashlib,os,selectors,struct,subprocess,tempfile,time
from pathlib import Path
TOTAL,START,COUNT=512,24,100
BLOB_START,BLOB_COUNT=144,258
V1_BYTES,V2_BYTES=32768,65536
STAGES=[
 "IndianOS: M2 disk boot1 installed ELF v1 executed",
 "IndianOS: M2 disk boot2 upgraded ELF v2 executed",
 "IndianOS: M2 disk boot3 rolled-back ELF v1 executed",
 "IndianOS: M2 disk boot4 removed app and rejected exec"]
DONE="IndianOS: M2 persisted ring3 package lifecycle PASSED"
def check_disk(path:Path, phase:int)->None:
 data=path.read_bytes()
 if len(data)!=TOTAL*512:
  raise AssertionError("Wrong disposable QEMU disk size")
 if (any(data[:START*512]) or
     any(data[(START+COUNT)*512:BLOB_START*512]) or
     any(data[(BLOB_START+BLOB_COUNT)*512:])):
  raise AssertionError("QEMU wrote outside BOTH bounded package partitions")
 if data[START*512:START*512+8]!=b"INDR027!":
  raise AssertionError("M2 durablefs metadata missing")
 if phase<=3 and b"M2P6" not in data[START*512:(START+COUNT)*512]:
  raise AssertionError("fully signed M2P6 manifest was not persisted")
 # Parse and independently SHA-256 validate the REAL second 258-sector
 # on-disk A/B store, including the exact expected active generation.
 expected_version=[1,2,1,1][phase-1]
 expected_len=[V1_BYTES,V2_BYTES,V1_BYTES,V1_BYTES][phase-1]
 expected_gen=[1,2,3,3][phase-1]
 expected_bank=[0,1,0,0][phase-1]
 entries=[]
 for bank in (0,1):
  head=data[(BLOB_START+bank)*512:(BLOB_START+bank+1)*512]
  if head[:8]!=b"M2BLOB1!":
   if bank==0 or phase>=2:
    raise AssertionError(f"Missing expected M2 large A/B header: {bank}")
   continue
  version=struct.unpack_from("<H",head,8)[0]
  recorded_bank=head[10]
  generation=struct.unpack_from("<Q",head,12)[0]
  app=struct.unpack_from("<I",head,20)[0]
  app_ver=struct.unpack_from("<Q",head,24)[0]
  size=struct.unpack_from("<I",head,32)[0]
  digest=head[36:68]
  if (version!=1 or recorded_bank!=bank or app!=9 or
      not 0<size<=65536 or hashlib.sha256(head[:68]).digest()!=head[68:100]):
   raise AssertionError(f"Corrupt Phase 5C bank {bank} metadata")
  first=(BLOB_START+2+bank*128)*512
  elf=data[first:first+size]
  if hashlib.sha256(elf).digest()!=digest:
   raise AssertionError(f"Corrupt Phase 5C bank {bank} payload")
  entries.append((generation,bank,app_ver,size))
 if max(entries)!=(expected_gen,expected_bank,expected_version,expected_len):
  raise AssertionError(f"Phase 5C A/B recovery state mismatch: {entries}")
 if phase==1 and len(entries)!=1:
  raise AssertionError("Phase 5C first boot published unexpected backup")
 if phase>=2 and len(entries)!=2:
  raise AssertionError("Phase 5C update lost the required rollback bank")
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
    if "IndianOS: M2 real Ed25519 boot policy and package signature VERIFIED" not in output:
     raise AssertionError("M2 Phase 4C Ed25519 boot or package verification missing")
    if "IndianOS: M2 Phase 4D QEMU simulated monotonic boot gate VERIFIED" not in output:
     raise AssertionError("M2 Phase 4D emulated protected-counter policy gate missing")
    if ("IndianOS: M2 Phase 5C real 64KiB signed ELF executed in ring3" not in output or
        "IndianOS: M2 Phase 5C dedicated A/B blob partition verified" not in output):
     raise AssertionError("Large M2 32/64KiB native ELF did not execute in real ring3")
    if output.index(stage)>output.index(DONE):
     raise AssertionError("M2 serial sequence mismatch")
    if stage!=STAGES[-1]:
     required=["IndianOS: M2 installed app privileged syscall DENIED",
       "IndianOS: ring-3 user image staged",
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
  disk=Path(directory)/"disposable-512-sectors.raw"
  disk.write_bytes(bytes(TOTAL*512))
  for n,stage in enumerate(STAGES,1):
   boot(args.qemu,args.ovmf.resolve(),args.iso.resolve(),disk,stage)
   check_disk(disk,n)
   print(f"M2 native ring3 disk boot {n}/4 PASS — large 32/64KiB ELF verified")
  print("M2 Phase 5C TEST-ONLY QEMU 32/64KiB ring3 A/B lifecycle passed")
if __name__=="__main__":main()
