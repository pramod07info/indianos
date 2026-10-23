#!/usr/bin/env python3
"""QEMU R029.4: PID1 ring-3 frame scheduling with a distinct PID2 app.

Verifies that two user-drawn command streams do not repaint VGA until the
authorized frame server issues the presentation syscall. A physical mouse
click is also buffered without an automatic repaint. Screenshots are real.
"""
from __future__ import annotations
import argparse
from pathlib import Path
import subprocess
import tempfile
from test_r029_desktop_qemu import (
    QemuSession,open_monitor,check_two_windows,differences,color_at
)
COMPLETE="IndianOS: R029.3 independent PID1 Files and PID2 Terminal GUI PASSED"
def protect(a,b):
    w,h,_=a
    for x,y in ((5,5),(20,20),(w//2,10),(w-20,10),(5,h-10),(w-20,h-10)):
        if color_at(a,x,y)!=color_at(b,x,y):
            raise AssertionError(f"frame server modified chrome {(x,y)}")
def capture(session,folder,name):
    return session.shot(folder/name)
def run(qemu:str,iso:Path,ovmf:Path):
    with tempfile.TemporaryDirectory(prefix="r0294-") as td:
        folder=Path(td);monitor=folder/"monitor.sock"
        args=[qemu,"-machine","pc","-m","256M","-smp","1",
              "-display","none","-vga","std",
              "-monitor",f"unix:{monitor},server=on,wait=off",
              "-serial","stdio","-no-reboot","-no-shutdown","-boot","d",
              "-drive",f"if=pflash,format=raw,readonly=on,file={ovmf}",
              "-cdrom",str(iso)]
        proc=subprocess.Popen(args,stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                              stderr=subprocess.STDOUT,bufsize=0)
        sock=None;session=None
        try:
            sock=open_monitor(monitor,proc);session=QemuSession(proc,sock)
            session.until("R029.4 frame server PID1 ready",35)
            baseline=capture(session,folder,"00-kernel-frame.ppm")
            check_two_windows(baseline)
            session.serial(b"g")
            session.until("R029.4 Files surface queued, not presented",12)
            session.until("R029.4 client PID2 ready",12)
            session.serial(b"g")
            session.until("R029.4 Terminal queued, no framebuffer draw",12)
            session.until("R029.4 frame server ready for first present",12)
            queued=capture(session,folder,"01-both-users-queued.ppm")
            if differences(baseline,queued):
                raise AssertionError("surface commands painted without ring3 frame request")
            session.serial(b"g")
            session.until("R029.4 first user-scheduled frame presented",12)
            first=capture(session,folder,"02-frame-server-presented.ppm")
            if differences(baseline,first)<50:
                raise AssertionError("ring3 frame server did not present both surfaces")
            protect(baseline,first)
            session.until("R029.4 client waiting for real PS2 event",12)
            w,h,_=first
            session.hmp(f"mouse_move {w-120-w//2} 135")
            session.drain_serial(0.15)
            session.hmp("mouse_button 1")
            session.drain_serial(0.15)
            session.hmp("mouse_button 0")
            session.until("R029.4 physical PS2 event queued without kernel present",20)
            delayed=capture(session,folder,"03-physical-input-not-presented.ppm")
            if differences(first,delayed):
                raise AssertionError("kernel repainted before userspace frame server requested it")
            session.until("R029.4 frame server ready for second present",12)
            session.serial(b"g")
            session.until("R029.4 second user-scheduled frame presented",12)
            second=capture(session,folder,"04-compositor-presents-mouse.ppm")
            if differences(first,second)<30:
                raise AssertionError("second userspace frame missing physical mouse visual changes")
            protect(baseline,second)
            session.until("R029.4 client survived independent compositor teardown",20)
            session.until("IndianOS: PID 1 REAPED",20)
            session.until("IndianOS: PID 2 REAPED",20)
            session.until(COMPLETE,20)
            print("R029.4 PASS: two ELF apps, frame-present gate, PS2 input, "
                  "VGA before/after frames, per-PID security and cleanup")
        finally:
            if session:session.selector.close()
            if sock:sock.close()
            if proc.poll() is None:proc.terminate()
            try:proc.communicate(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill();proc.communicate(timeout=3)
def main():
    p=argparse.ArgumentParser()
    p.add_argument("--iso",required=True,type=Path)
    p.add_argument("--qemu",required=True)
    p.add_argument("--ovmf",required=True,type=Path)
    a=p.parse_args()
    if not a.iso.is_file() or not a.ovmf.is_file():
        p.error("build QEMU ISO and OVMF before testing")
    run(a.qemu,a.iso.resolve(),a.ovmf.resolve())
if __name__=="__main__":main()
