#!/usr/bin/env python3
"""R029: boot visible QEMU VGA, inspect actual framebuffer screenshots,
and exercise independent windows with COM1 + real PS/2 mouse events.

Tests real guest framebuffer pixels via HMP screendump, not a host GUI mock.
Never maps a host desktop, network, disk or physical input device.
"""
from __future__ import annotations
import argparse
import os
import pathlib
import re
import selectors
import socket
import subprocess
import tempfile
import time

READY="IndianOS: R029 PS2 mouse and serial keyboard ready"
SUCCESS="IndianOS: R029 QEMU real framebuffer two interactive windows PASSED"
STEPS=(
    # Mouse already selected Files. First keyboard-focus Terminal so the
    # compositor must actually change visible titlebar pixels on each step.
    (b"2", "IndianOS: R029 keyboard focused Terminal window"),
    (b"1", "IndianOS: R029 keyboard focused Files window"),
    (b"2", "IndianOS: R029 keyboard focused Terminal window"),
    (b"h", "IndianOS: R029 window moved on framebuffer"),
    (b"m", "IndianOS: R029 window minimized on framebuffer"),
    (b"r", "IndianOS: R029 window restored on framebuffer"),
    (b"x", "IndianOS: R029 independent window closed on framebuffer"),
)

def parse_ppm(path:pathlib.Path)->tuple[int,int,bytes]:
    raw=path.read_bytes()
    header=re.match(rb"^P6\s+(\d+)\s+(\d+)\s+255\s",raw)
    if not header:
        raise AssertionError(f"QEMU screendump is not P6 binary PPM: {raw[:90]!r}")
    w,h=map(int,header.groups())
    pixels=raw[header.end():]
    if w<640 or h<480 or len(pixels)!=w*h*3:
        raise AssertionError(f"invalid screenshot geometry: {w}x{h}, {len(pixels)} bytes")
    return w,h,pixels

def color_at(image:tuple[int,int,bytes],x:int,y:int)->bytes:
    w,h,data=image
    x=min(w-1,max(0,x));y=min(h-1,max(0,y))
    i=(y*w+x)*3
    return data[i:i+3]

def differences(a:tuple[int,int,bytes],b:tuple[int,int,bytes])->int:
    assert a[:2]==b[:2],"QEMU display size changed unexpectedly"
    # Sparse whole-frame comparison, excluding no region; proves the actual
    # display changed, not just serial textual acknowledgements.
    return sum(a[2][i:i+3]!=b[2][i:i+3]
               for i in range(0,len(a[2]),3*31))

def check_two_windows(image:tuple[int,int,bytes])->None:
    w,h,_=image
    # Top bar and file/terminal client pixels must have distinct colors.
    points=[
        color_at(image,10,10),
        color_at(image,80,85),
        color_at(image,80,145),
        color_at(image,w-65,h//2-46),
        color_at(image,w-65,h//2+15),
        color_at(image,w//2,h-10)
    ]
    if len(set(points))<4:
        raise AssertionError(f"expected separately painted desktop and two windows, colors: {points}")
    # Visual image must have several true non-text flat-color regions.
    sampled={color_at(image,x,y) for y in range(0,h,27)
             for x in range(0,w,29)}
    if len(sampled)<6:
        raise AssertionError("no distinct window chrome/panel/titlebar pixels")

class QemuSession:
    def __init__(self,proc:subprocess.Popen,sock:socket.socket):
        self.proc=proc
        self.sock=sock
        self.output=bytearray()
        self.selector=selectors.DefaultSelector()
        assert proc.stdout is not None
        self.selector.register(proc.stdout,selectors.EVENT_READ)
        self.recv_monitor()
    def drain_serial(self,duration:float=0.1)->None:
        end=time.monotonic()+duration
        while time.monotonic()<end:
            for key,_ in self.selector.select(0.08):
                data=os.read(key.fileobj.fileno(),8192)
                if data:self.output.extend(data)
            if b"IndianOS: FATAL" in self.output:
                raise AssertionError("QEMU kernel fatal\n"+self.output[-9000:].decode("utf8","replace"))
            if len(self.output)>2_000_000:
                raise AssertionError("serial output larger than 2MB")
            if self.proc.poll() is not None:
                raise AssertionError("QEMU terminated unexpectedly\n"+
                                     self.output[-5000:].decode("utf8","replace"))
    def until(self,marker:str,deadline:float=22)->None:
        timeout=time.monotonic()+deadline
        while marker.encode() not in self.output:
            if time.monotonic()>timeout:
                raise AssertionError("missing serial marker "+marker+"\n"+
                    self.output[-8000:].decode("utf8","replace"))
            self.drain_serial(0.1)
    def recv_monitor(self,timeout:float=4)->str:
        response=b""
        self.sock.settimeout(timeout)
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            try:part=self.sock.recv(4096)
            except socket.timeout:break
            if not part:raise AssertionError("QEMU monitor socket closed")
            response+=part
            if b"(qemu)" in response:break
        if b"(qemu)" not in response:
            raise AssertionError("QEMU monitor command did not complete: "+repr(response[-800:]))
        return response.decode("utf-8","replace")
    def hmp(self,cmd:str)->str:
        self.sock.sendall((cmd+"\n").encode())
        result=self.recv_monitor()
        if "unknown command" in result.lower() or "error:" in result.lower():
            raise AssertionError(f"HMP {cmd}: {result}")
        return result
    def shot(self,path:pathlib.Path)->tuple[int,int,bytes]:
        self.hmp(f"screendump {path}")
        deadline=time.monotonic()+5
        while not path.exists():
            if time.monotonic()>deadline:
                raise AssertionError("QEMU did not create framebuffer screenshot "+str(path))
            self.drain_serial(0.1)
        return parse_ppm(path)
    def serial(self,bytes_:bytes)->None:
        if self.proc.stdin is None:raise AssertionError("COM1 stdin missing")
        self.proc.stdin.write(bytes_)
        self.proc.stdin.flush()

def open_monitor(path:pathlib.Path,proc:subprocess.Popen)->socket.socket:
    end=time.monotonic()+12
    while time.monotonic()<end:
        if proc.poll() is not None:raise AssertionError("QEMU boot exited before monitor opened")
        if path.exists():
            sock=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
            try:
                sock.connect(str(path))
                return sock
            except OSError:
                sock.close()
        time.sleep(0.07)
    raise AssertionError("QEMU HMP socket unavailable")

def run(qemu:str,iso:pathlib.Path,ovmf:pathlib.Path)->None:
    with tempfile.TemporaryDirectory(prefix="indianos-r029-visual-test-") as tmp:
        directory=pathlib.Path(tmp)
        monitor=directory/"monitor.sock"
        cmd=[
            qemu,"-machine","pc","-m","256M","-smp","1",
            "-display","none","-vga","std",
            "-monitor",f"unix:{monitor},server=on,wait=off",
            "-serial","stdio",
            "-no-reboot","-no-shutdown","-boot","d",
            "-drive",f"if=pflash,format=raw,readonly=on,file={ovmf}",
            "-cdrom",str(iso)
        ]
        proc=subprocess.Popen(cmd,stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                              stderr=subprocess.STDOUT,bufsize=0)
        sock=None;session=None
        try:
            sock=open_monitor(monitor,proc)
            session=QemuSession(proc,sock)
            session.until(READY,30)
            first=session.shot(directory/"00-initial.ppm")
            check_two_windows(first)
            w,h,_=first
            print(f"R029 QEMU actual VGA frame {w}x{h}: two independent windows rendered")
            # HMP mouse_move and mouse_button reach PS/2 auxiliary port
            # through QEMU's emulated 8042 mouse, not a model callback.
            session.hmp(f"mouse_move {120-w//2} {90-h//2}")
            session.drain_serial(0.2)
            session.hmp("mouse_button 1")
            session.drain_serial(0.2)
            session.hmp("mouse_button 0")
            session.until("IndianOS: R029 PS2 mouse selected Files window",8)
            after_mouse=session.shot(directory/"01-mouse.ppm")
            if differences(first,after_mouse)<50:
                raise AssertionError("real PS2 mouse click did not repaint framebuffer titlebar")
            print("R029 PS2 mouse physically selected Files and changed display pixels")
            previous=after_mouse
            for index,(key,marker) in enumerate(STEPS,2):
                session.serial(key)
                session.until(marker,10)
                changed=session.shot(directory/f"{index:02d}-step.ppm")
                if differences(previous,changed)<24:
                    raise AssertionError(
                        f"keyboard input {key!r} did not change real framebuffer")
                previous=changed
                print(f"R029 QEMU keyboard {key.decode()}: distinct visual framebuffer update")
            # PID1's graphical service closes only after user-requested quit;
            # no headless model test is accepted as a substitute.
            session.serial(b"q")
            session.until(SUCCESS,8)
            session.until("IndianOS: milestone 001 complete",15)
            print("R029 real QEMU VGA compositor and two interactive windows PASSED")
        finally:
            if session:session.selector.close()
            if sock:sock.close()
            if proc.poll() is None:proc.terminate()
            try:proc.communicate(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill();proc.communicate(timeout=3)

def main()->None:
    parser=argparse.ArgumentParser()
    parser.add_argument("--iso",type=pathlib.Path,required=True)
    parser.add_argument("--qemu",required=True)
    parser.add_argument("--ovmf",type=pathlib.Path,required=True)
    args=parser.parse_args()
    if not args.iso.is_file() or not args.ovmf.is_file():
        parser.error("ISO or OVMF firmware path does not exist")
    run(args.qemu,args.iso.resolve(),args.ovmf.resolve())

if __name__=="__main__":main()
