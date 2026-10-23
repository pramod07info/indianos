#!/usr/bin/env python3
"""R029.3 real-QEMU proof: two separate ELF64 GUI apps, owners, PS2, VGA."""
from __future__ import annotations
import argparse
from pathlib import Path
import subprocess
import tempfile
from test_r029_desktop_qemu import (
    QemuSession,open_monitor,check_two_windows,differences,color_at
)
PASSED="IndianOS: R029.3 independent PID1 Files and PID2 Terminal GUI PASSED"
def protect(a,b):
    w,h,_=a
    for x,y in ((5,5),(20,20),(w//2,10),(w-20,10),(5,h-10),(w-20,h-10)):
        if color_at(a,x,y)!=color_at(b,x,y):
            raise AssertionError(f"ring3 overwrote desktop chrome: {(x,y)}")
def run(qemu:str,iso:Path,ovmf:Path):
    with tempfile.TemporaryDirectory(prefix="r0293-") as temp:
        p=Path(temp);monitor=p/"qemu.sock"
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
            session.until("R029.3 PID1 Files ready",35)
            baseline=session.shot(p/"00-desktop.ppm")
            check_two_windows(baseline)
            session.serial(b"g")
            session.until("R029.3 PID1 Files rendered own surface",12)
            session.until("R029.3 PID2 Terminal ready",12)
            files=session.shot(p/"01-files-pid1.ppm")
            if differences(baseline,files)<30:raise AssertionError("PID1 Files missing pixels")
            protect(baseline,files)
            session.serial(b"g")
            session.until("R029.3 PID2 Terminal rendered own surface",12)
            session.until("R029.3 PID1 ready for focus",12)
            terminal=session.shot(p/"02-terminal-pid2.ppm")
            if differences(files,terminal)<30:raise AssertionError("PID2 Terminal missing pixels")
            protect(baseline,terminal)
            session.serial(b"g")
            session.until("R029.3 PID1 isolated focus event",12)
            session.until("R029.3 PID2 ready for focus",12)
            session.serial(b"g")
            session.until("R029.3 PID2 awaiting actual PS2 input",12)
            w,h,_=terminal
            session.hmp(f"mouse_move {w-120-w//2} 135")
            session.drain_serial(0.15)
            session.hmp("mouse_button 1")
            session.drain_serial(0.15)
            session.hmp("mouse_button 0")
            session.until("R029.3 PID2 received real PS2 mouse event",20)
            physical=session.shot(p/"03-physical-mouse-pid2.ppm")
            # The old sparse 1-in-31 sample can miss a 21x21 cursor.
            # For the small physical mouse repaint, compare EVERY RGB pixel.
            before=terminal[2];after=physical[2]
            moved=sum(before[i:i+3]!=after[i:i+3]
                      for i in range(0,len(before),3))
            if moved<20:
                raise AssertionError(f"real PS2 cursor did not repaint VGA ({moved} pixels)")
            protect(baseline,physical)
            session.until("R029.3 PID1 closed its own GUI service",20)
            session.until("R029.3 PID2 retained its independent GUI surface",20)
            session.until("IndianOS: PID 1 REAPED",20)
            session.until("IndianOS: PID 2 REAPED",20)
            session.until(PASSED,20)
            print("R029.3: two mapped ELFs, separate owners, PS2 and framebuffer PASSED")
        finally:
            if session:session.selector.close()
            if sock:sock.close()
            if proc.poll() is None:proc.terminate()
            try:proc.communicate(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill();proc.communicate(timeout=3)
def main():
    p=argparse.ArgumentParser()
    p.add_argument("--iso",type=Path,required=True)
    p.add_argument("--qemu",required=True)
    p.add_argument("--ovmf",type=Path,required=True)
    x=p.parse_args()
    if not x.iso.is_file() or not x.ovmf.is_file():p.error("QEMU ISO/OVMF missing")
    run(x.qemu,x.iso.resolve(),x.ovmf.resolve())
if __name__=="__main__":main()
