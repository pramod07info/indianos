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
        # Existing desktop contracts require real keyboard transitions and
        # physical PS/2 mouse input before the authenticated shutdown.
        for char,marker in [
            (b"a","R031.1 application launcher OPEN"),
            (b"a","R031.1 application launcher CLOSED"),
            (b"2","R031.1 user switched to Terminal app"),
            (b"1","R031.1 user switched to Files app"),
            (b"2","R031.1 user switched to Terminal app"),
            (b"h","R031.1 Files window moved"),
            (b"m","R031.1 Terminal minimized"),
            (b"r","R031.1 Terminal restored"),
        ]:
            offset=len(session.output)
            session.serial(char)
            session.until(marker,15,since=offset)
        session.hmp("mouse_move 100 135")
        session.drain_serial(0.2)
        session.hmp("mouse_button 1")
        session.drain_serial(0.2)
        session.hmp("mouse_button 0")
        session.until("R031.1 physical PS2 mouse event reached desktop service",15)
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
        # Optional visual evidence for real QEMU session, never a mock/render.
        # The normal test remains unchanged unless the capture env is set.
        import os
        import shutil
        target = os.environ.get("R031_GUI_SCREENSHOT_DIR")
        if target:
            output = Path(target)
            output.mkdir(parents=True, exist_ok=True)
            for frame in sorted(folder.glob("*.ppm")):
                shutil.copyfile(frame, output / frame.name)
    print("R031.9 PASS: real GUI file content, IPC refresh, persistent reboot, "
          "strict PID2 directory policy, isolated graphics and bounded disk")
if __name__=="__main__":main()
