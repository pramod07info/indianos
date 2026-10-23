#!/usr/bin/env python3
"""Integrated R029.5-7: three isolated GUI processes, IPC, launcher, PS/2."""
from __future__ import annotations
import argparse
from pathlib import Path
import subprocess,tempfile
from test_r029_desktop_qemu import QemuSession,open_monitor,differences
PASSED="IndianOS: R029.5-7 integrated three-process GUI, IPC and launcher PASSED"
def run(qemu,iso,ovmf):
    with tempfile.TemporaryDirectory(prefix="r029567-") as td:
        root=Path(td); monitor=root/"hmp.sock"
        cmd=[qemu,"-machine","pc","-m","256M","-smp","1",
             "-display","none","-vga","std",
             "-monitor",f"unix:{monitor},server=on,wait=off",
             "-serial","stdio","-no-reboot","-no-shutdown","-boot","d",
             "-drive",f"if=pflash,format=raw,readonly=on,file={ovmf}",
             "-cdrom",str(iso)]
        p=subprocess.Popen(cmd,stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT,bufsize=0)
        sock=None;session=None
        try:
            sock=open_monitor(monitor,p);session=QemuSession(p,sock)
            session.until("R029.5 PID1 desktop shell ready",38)
            blank=session.shot(root/"00-desktop.ppm")
            session.serial(b"g")
            session.until("R029.5 PID2 Files surface staged",15)
            session.until("R029.5 PID3 Terminal surface staged",15)
            session.until("R029.5 IPC joined apps and first frame rendered",15)
            frame=session.shot(root/"01-both-apps.ppm")
            if differences(blank,frame)<30:raise AssertionError("two independent app paints missing")
            session.serial(b"g")
            session.until("R029.7 launcher opened",10)
            launcher=session.shot(root/"02-app-launcher.ppm")
            if differences(frame,launcher)<30:raise AssertionError("launcher not drawn")
            session.serial(b"g")
            session.until("R029.7 launcher closed and Terminal focused",10)
            focus=session.shot(root/"03-terminal-focused.ppm")
            if differences(launcher,focus)<30:raise AssertionError("z-order focus unchanged")
            session.serial(b"g")
            session.until("R029.7 Files moved, Terminal minimized",10)
            hidden=session.shot(root/"04-move-minimize.ppm")
            if differences(focus,hidden)<30:raise AssertionError("move/minimize not rendered")
            session.serial(b"g")
            session.until("R029.7 Terminal restored, fifth frame committed",10)
            shown=session.shot(root/"05-terminal-restored.ppm")
            if differences(hidden,shown)<30:raise AssertionError("restore did not repaint")
            session.until("R029.5 three-process suite awaiting physical pointer",12)
            session.until("R029.5 PID2 Files exited and grants revoked",12)
            session.until("R029.6 PID3 waiting for physical PS2 focus",12)
            w,h,_=shown
            session.hmp(f"mouse_move {w-120-w//2} 135")
            session.drain_serial(0.15)
            session.hmp("mouse_button 1")
            session.drain_serial(0.15)
            session.hmp("mouse_button 0")
            session.until("R029.6 PID3 authenticated physical input delivered",20)
            session.until("IndianOS: PID 1 REAPED",20)
            session.until("IndianOS: PID 2 REAPED",20)
            session.until("IndianOS: PID 3 REAPED",20)
            session.until(PASSED,20)
            print("R029.5-7 PASS: 3 ELF CR3 mappings, IPC, desktop launcher, "
                  "window control, physical input and process cleanup")
        finally:
            if session:session.selector.close()
            if sock:sock.close()
            if p.poll() is None:p.terminate()
            try:p.communicate(timeout=3)
            except subprocess.TimeoutExpired:
                p.kill();p.communicate(timeout=3)
def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--iso",required=True,type=Path)
    parser.add_argument("--qemu",required=True)
    parser.add_argument("--ovmf",required=True,type=Path)
    args=parser.parse_args()
    if not args.iso.is_file() or not args.ovmf.is_file():
        parser.error("ISO/OVMF missing")
    run(args.qemu,args.iso.resolve(),args.ovmf.resolve())
if __name__=="__main__":main()
