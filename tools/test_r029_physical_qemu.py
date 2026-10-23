#!/usr/bin/env python3
"""Verify one genuine ring-3 ELF graphics client can modify two VGA windows.

Captures actual QEMU screendump PPM data after host-stepped INT 0x80 draw
commands. Tests pixel changes and screen-chrome non-interference. No mock GUI.
"""
from __future__ import annotations
import argparse
import os
from pathlib import Path
import subprocess
import tempfile
from test_r029_desktop_qemu import (
    QemuSession,open_monitor,check_two_windows,differences,color_at,
)

READY="IndianOS user: R029 ring3: user compositor client ready"
EVENTS=[
    "IndianOS user: R029 ring3: Files content rendered by usermode syscall",
    "IndianOS user: R029 ring3: Terminal content rendered by usermode syscall",
    "IndianOS user: R029 ring3: isolated drawing bounds and forged pointers rejected",
]
DONE="IndianOS: R029 ring3 command-owned two-window surfaces PASSED"
MOUSE="IndianOS user: R029 ring3: physical PS2 mouse focus event delivered"

def save_frame(s:QemuSession,folder:Path,filename:str):
    path=folder/filename
    frame=s.shot(path)
    artifact_dir=os.environ.get("R029_USER_SCREENSHOT_DIR")
    if artifact_dir:
        import shutil
        d=Path(artifact_dir)
        d.mkdir(parents=True,exist_ok=True)
        shutil.copyfile(path,d/filename)
    return frame

def protected_chrome(a,b):
    w,h,_=a
    for x,y in ((5,5),(20,20),(w//2,10),(w-20,10),(5,h-10),(w-20,h-10)):
        if color_at(a,x,y)!=color_at(b,x,y):
            raise AssertionError(f"ring3 client drew outside window interiors at {(x,y)}")

def run(qemu:str,iso:Path,ovmf:Path)->None:
    with tempfile.TemporaryDirectory(prefix="indianos-r029-real-ring3-") as tmp:
        directory=Path(tmp)
        monitor=directory/"monitor.sock"
        cmd=[qemu,"-machine","pc","-m","256M","-smp","1",
             "-display","none","-vga","std",
             "-monitor",f"unix:{monitor},server=on,wait=off",
             "-serial","stdio","-no-reboot","-no-shutdown","-boot","d",
             "-drive",f"if=pflash,format=raw,readonly=on,file={ovmf}",
             "-cdrom",str(iso)]
        proc=subprocess.Popen(cmd,stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                              stderr=subprocess.STDOUT,bufsize=0)
        sock=None;session=None
        try:
            sock=open_monitor(monitor,proc)
            session=QemuSession(proc,sock)
            session.until(READY,35)
            baseline=save_frame(session,directory,"00-kernel-owned-desktop.ppm")
            check_two_windows(baseline)
            print(f"R029 ring3: real baseline VGA {baseline[0]}x{baseline[1]} PASSED")
            previous=baseline
            for idx,event in enumerate(EVENTS,1):
                session.serial(b"g")
                session.until(event,12)
                frame=save_frame(session,directory,f"0{idx}-user-surface.ppm")
                diff=differences(previous,frame)
                if diff<35:
                    raise AssertionError(
                        f"ring3 drawing/focus did not change real framebuffer: {event} ({diff})")
                protected_chrome(baseline,frame)
                print(f"R029 ring3: stage {idx} display changed at {diff} sampled pixels")
                previous=frame
            # A real HMP pointer event reaches the emulated PS/2 port while
            # ring-3 polls; no model callback or forged user event is allowed.
            w,h,_=previous
            session.hmp(f"mouse_move {w-120-w//2} {135}")
            session.drain_serial(0.15)
            session.hmp("mouse_button 1")
            session.drain_serial(0.15)
            session.hmp("mouse_button 0")
            session.until(MOUSE,20)
            actual=save_frame(session,directory,"04-physical-mouse-focus.ppm")
            if differences(previous,actual)<30:
                raise AssertionError("physical PS2 focus did not repaint real VGA")
            protected_chrome(baseline,actual)
            print("R029.2: physical PS2 input reached ring3 and changed VGA pixels")
            session.serial(b"g")
            session.until("IndianOS user: R029 ring3: user surface client exited cleanly",12)
            session.until(DONE,20)
            session.until("IndianOS: ring-3 PMM frame count restored",20)
            # Original runtime must still reap its PID1 and return all frames.
            session.until("IndianOS: PID 1 REAPED",20)
            print("R029 ring3: authenticated two-window real QEMU pixels,"
                  " malformed command denial and process teardown PASSED")
        finally:
            if session:session.selector.close()
            if sock:sock.close()
            if proc.poll() is None:proc.terminate()
            try:proc.communicate(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill();proc.communicate(timeout=3)

def main()->None:
    parser=argparse.ArgumentParser()
    parser.add_argument("--iso",required=True,type=Path)
    parser.add_argument("--qemu",required=True)
    parser.add_argument("--ovmf",required=True,type=Path)
    args=parser.parse_args()
    if not args.iso.is_file() or not args.ovmf.is_file():
        parser.error("QEMU ISO and OVMF firmware must exist")
    run(args.qemu,args.iso.resolve(),args.ovmf.resolve())

if __name__=="__main__":main()
