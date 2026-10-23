#!/usr/bin/env python3
"""R031.1 actual QEMU auto-boot-to-desktop, interactive session, mouse IPC.

Zero serial keystrokes before boot-ready signal. Uses an isolated OVMF/QEMU
process and real framebuffer captures; no physical host disks/network writes.
"""
from __future__ import annotations
import argparse,subprocess,tempfile
from pathlib import Path
from test_r029_desktop_qemu import QemuSession,open_monitor,differences,check_two_windows

READY="R031.1 AUTOMATIC DESKTOP READY: PID1 shell PID2 Files PID3 Terminal"
PASS="IndianOS: R031.1 automatic boot-to-desktop services and shutdown PASSED"
def run(qemu,iso,ovmf):
    with tempfile.TemporaryDirectory(prefix="indianos-r0311-") as tmp:
        folder=Path(tmp);monitor=folder/"monitor.sock"
        argv=[qemu,"-machine","pc","-m","256M","-smp","1",
              "-display","none","-vga","std",
              "-monitor",f"unix:{monitor},server=on,wait=off",
              "-serial","stdio","-no-reboot","-no-shutdown","-boot","d",
              "-drive",f"if=pflash,format=raw,readonly=on,file={ovmf}",
              "-cdrom",str(iso)]
        proc=subprocess.Popen(argv,stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                              stderr=subprocess.STDOUT,bufsize=0)
        sock=None;session=None
        try:
            sock=open_monitor(monitor,proc)
            session=QemuSession(proc,sock)
            session.until("R031.1 Files service registered and running",45)
            session.until("R031.1 Terminal service registered and running",15)
            session.until(READY,15)
            current=session.shot(folder/"01-auto-boot.ppm")
            check_two_windows(current)
            print("R031.1 automatic graphical desktop: 3 independent ELF tasks ready, no prompt")
            for idx,(char,marker) in enumerate([
                (b"a","R031.1 application launcher OPEN"),
                (b"a","R031.1 application launcher CLOSED"),
                (b"2","R031.1 user switched to Terminal app"),
                (b"1","R031.1 user switched to Files app"),
                (b"2","R031.1 user switched to Terminal app"),
                (b"h","R031.1 Files window moved"),
                (b"m","R031.1 Terminal minimized"),
                (b"r","R031.1 Terminal restored"),
            ],1):
                offset=len(session.output)
                session.serial(char)
                session.until(marker,13,since=offset)
                after=session.shot(folder/(f"02-action-{idx:02d}-{char.decode()}.ppm"))
                if differences(current,after)<18:
                    raise AssertionError("keyboard session action did not repaint desktop: "+marker)
                current=after
            # Inject a genuine PS/2 packet over HMP, not a fake userspace
            # pointer event. The kernel must route it to the window owner.
            w,h,_=current
            session.hmp(f"mouse_move {w-120-w//2} 135")
            session.drain_serial(0.15)
            session.hmp("mouse_button 1")
            session.drain_serial(0.15)
            session.hmp("mouse_button 0")
            session.until("R029.2 physical mouse focus event queued",15)
            session.until("IndianOS: R029 PS2 mouse selected Terminal window",15)
            session.until("R031.1 physical PS2 mouse event reached desktop service",15)
            moved=session.shot(folder/"03-real-ps2.ppm")
            # Pointer/crosshair redraws are small; a 1-in-31 sparse sample
            # can miss legitimate changes. Compare every actual RGB byte.
            if current[:2]!=moved[:2] or current[2]==moved[2]:
                raise AssertionError("physical mouse input did not update framebuffer")
            session.serial(b"q")
            session.until("R031.1 shutdown requested: notifying Files and Terminal",15)
            session.until("R031.1 Files service stopped and capabilities revoked",15)
            session.until("R031.1 Terminal received owned physical focus notification",15)
            session.until("R031.1 session manager exiting after app services",15)
            session.until("IndianOS: PID 1 REAPED",15)
            session.until("IndianOS: PID 2 REAPED",15)
            session.until("IndianOS: PID 3 REAPED",15)
            session.until(PASS,15)
            print("R031.1 PASS: automatic GUI boot, IPC readiness, 8 interactive "
                  "window operations, PS2 mouse focus, clean shutdown and PMM reaping")
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
    args=parser.parse_args()
    if not args.iso.is_file() or not args.ovmf.is_file():
        parser.error("QEMU ISO and OVMF firmware required")
    run(args.qemu,args.iso.resolve(),args.ovmf.resolve())
if __name__=="__main__":main()
