#!/usr/bin/env python3
"""R031.9: two real GUI QEMU boots with one persistent sandboxed disk."""
from __future__ import annotations
import argparse
import subprocess
import tempfile
from pathlib import Path
from test_r029_desktop_qemu import QemuSession,open_monitor,differences,check_two_windows

READY="R031.1 AUTOMATIC DESKTOP READY: PID1 shell PID2 Files PID3 Terminal"
PASS="IndianOS: R031.1 automatic boot-to-desktop services and shutdown PASSED"
START=24
TOTAL=128
PHYSICAL="IndianOS: R047 genuine PS2 keyboard byte accepted by kernel input"

def physical_key(session:QemuSession,key:str,marker:str,
                 folder:Path,boot_index:int,step:int,
                 previous:tuple[int,int,bytes],first:bool):
    """Inject a REAL emulated 8042 key transition through QEMU HMP.
    Never use COM1 or a test-only in-memory dispatch for this assertion.
    """
    if key not in ("a","1","2","h","m","r","q"):
        raise AssertionError("unapproved PS2 test keystroke")
    start=len(session.output)
    session.hmp(f"sendkey {key} 80")
    if first:
        session.until(PHYSICAL,20,since=start)
    session.until(marker,20,since=start)
    if key=="q":
        return previous
    frame=session.shot(folder/f"r047-ps2-{boot_index}-{step}-{key}.ppm")
    if differences(previous,frame)<10:
        raise AssertionError("physical PS2 keyboard action did not repaint real QEMU framebuffer: "+key)
    print(f"R047 physical QEMU PS/2 keyboard {key}: guest input and visual action confirmed")
    return frame

def guard(disk:Path):
    raw=disk.read_bytes()
    if len(raw)!=TOTAL*512 or any(raw[:START*512]) or any(raw[124*512:]):
        raise AssertionError("R031.9 mutated disk guard sectors")
    if raw[START*512:START*512+8]!=b"INDR027!":
        raise AssertionError("R031.9 durable filesystem superblock missing")

def boot(qemu:str,iso:Path,ovmf:Path,disk:Path,folder:Path,index:int):
    monitor=folder/f"monitor-{index}.sock"
    cmd=[qemu,"-machine","pc","-m","256M","-smp","1",
         "-display","none","-vga","std",
         "-monitor",f"unix:{monitor},server=on,wait=off",
         "-serial","stdio","-no-reboot","-no-shutdown","-boot","d",
         "-drive",f"if=pflash,format=raw,readonly=on,file={ovmf}",
         "-cdrom",str(iso),
         "-drive",f"if=ide,index=0,media=disk,format=raw,file={disk}"]
    proc=subprocess.Popen(cmd,stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT,bufsize=0)
    sock=None;session=None
    try:
        sock=open_monitor(monitor,proc)
        session=QemuSession(proc,sock)
        if index==0:
            session.until("R031.9 PID1 provisioned persistent /shared/readme for GUI",45)
            message="R031.9 Files displayed HELLO INDIANOS from persistent volume"
        else:
            session.until("R031.9 PID1 mounted existing persistent /shared/readme",45)
            message="R031.9 Files displayed FILE UPDATED from persistent volume"
        session.until(message,15)
        session.until("R031.9 real file and directory contents rendered into Files window",15)
        session.until("R031.1 Terminal service registered and running",15)
        session.until(READY,15)
        before=session.shot(folder/f"r0319-{index}-initial.ppm")
        check_two_windows(before)
        if index==0:
            offset=len(session.output)
            session.serial(b"f")
            session.until("R031.9 Files refreshed on-disk content after desktop IPC",15,since=offset)
            session.until("R031.9 desktop requested Files reload from persistent volume",15,since=offset)
            updated=session.shot(folder/"r0319-updated.ppm")
            if differences(before,updated)<8:
                raise AssertionError("persistent file refresh did not redraw GUI pixels")
        # In both QEMU boots, physical input must reach the guest through
        # the virtual 8042. The existing serial path is still exercised by
        # distinct steps; serial-only shortcuts cannot satisfy this proof.
        if PHYSICAL.encode() in session.output:
            raise AssertionError("PS2 evidence appeared before physical sendkey")
        actions=[
            ("a","R031.1 application launcher OPEN"),
            ("a","R031.1 application launcher CLOSED"),
            ("2","R031.1 user switched to Terminal app"),
            ("1","R031.1 user switched to Files app"),
            ("2","R031.1 user switched to Terminal app"),
            ("h","R031.1 Files window moved"),
            ("m","R031.1 Terminal minimized"),
            ("r","R031.1 Terminal restored"),
        ]
        previous=updated if index==0 else before
        first_physical=True
        for step,(char,marker) in enumerate(actions):
            # Physical launcher + focus during first boot, and focus plus
            # restore during the second boot after the same disk remount.
            use_ps2=(step in (0,2,3,4) if index==0
                     else step in (2,4,7))
            if use_ps2:
                previous=physical_key(
                    session,char,marker,folder,index,step,previous,
                    first_physical)
                first_physical=False
            else:
                offset=len(session.output)
                session.serial(char.encode())
                session.until(marker,15,since=offset)
                previous=session.shot(folder/f"r047-serial-{index}-{step}.ppm")
        if first_physical:
            raise AssertionError("no real 8042 keyboard event was tested")

        session.hmp("mouse_move 100 135")
        session.drain_serial(0.2)
        session.hmp("mouse_button 1")
        session.drain_serial(0.2)
        session.hmp("mouse_button 0")
        session.until("R031.1 physical PS2 mouse event reached desktop service",15)
        if index==0:
            # Shutdown must also be reachable from a physical keyboard,
            # not only the test's serial control channel.
            physical_key(session,"q",
                "R031.1 shutdown requested: notifying Files and Terminal",
                folder,index,9,previous,False)
        else:
            session.serial(b"q")
        session.until("R031.1 Files service stopped and capabilities revoked",15)
        session.until("R031.1 session manager exiting after app services",15)
        session.until("IndianOS: PID 1 REAPED",15)
        session.until("IndianOS: PID 2 REAPED",15)
        session.until("IndianOS: PID 3 REAPED",15)
        session.until(PASS,15)
        print(f"R031.9 QEMU desktop boot {index+1}/2 passed")
    finally:
        if session:session.selector.close()
        if sock:sock.close()
        if proc.poll() is None:proc.terminate()
        try:proc.communicate(timeout=3)
        except subprocess.TimeoutExpired:proc.kill();proc.communicate(timeout=3)

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--iso",type=Path,required=True)
    parser.add_argument("--qemu",required=True)
    parser.add_argument("--ovmf",type=Path,required=True)
    a=parser.parse_args()
    for path in [a.iso,a.ovmf]:
        if not path.is_file():parser.error(f"missing artifact {path}")
    with tempfile.TemporaryDirectory(prefix="indianos-r0319-") as tmp:
        folder=Path(tmp)
        disk=folder/"128-disposable-sectors.raw"
        disk.write_bytes(bytes(TOTAL*512))
        for index in range(2):
            boot(a.qemu,a.iso.resolve(),a.ovmf.resolve(),disk,folder,index)
            guard(disk)
    print("R047 PASS: actual QEMU PS/2 keyboard across two boots, "
          "real framebuffer changes and original R031.9 durable "
          "Files/reboot/IPC and disk-boundary verification")
if __name__=="__main__":main()
