#!/usr/bin/env python3
"""Isolated QEMU legacy virtio-net end-to-end test, no public network access.

QEMU's socket network backend transports *real* guest Ethernet packets to
this deterministic host peer. Peer replies with validated ARP, IPv4 ICMP,
UDP, DNS and a TCP handshake/payload. Protocol violations fail the run.
No host OS network interface, internet DNS or LAN bridge is connected.
"""
from __future__ import annotations
import argparse
import os
import pathlib
import selectors
import socket
import subprocess
import time

GUEST_MAC=bytes.fromhex("525400123456")
PEER_MAC=bytes.fromhex("525400abcdef")
GUEST_IP=bytes([10,7,0,2])
PEER_IP=bytes([10,7,0,1])
DNS_NAME=b"\x08indianos\x04test\x00"
TCP_PEER_SEQ=0x50607080
TCP_CLIENT_SEQ=0x10203040
CLIENT_PORT=41280
UDP_PORT=4040
TCP_PORT=8080
MARKERS=(
    "IndianOS: R028 legacy virtio-net DMA queues initialized",
    "IndianOS: R028 Ethernet ARP peer resolved",
    "IndianOS: R028 IPv4 ICMP echo ping PASSED",
    "IndianOS: R028 IPv4 UDP echo PASSED",
    "IndianOS: R028 UDP DNS A query indianos.test PASSED",
    "IndianOS: R028 TCP three-way handshake SYN-ACK verified",
    "IndianOS: R028 TCP client request-response PASSED",
    "IndianOS: R028 QEMU virtio-net end-to-end network client PASSED",
)
def u16(b:bytes)->int:return int.from_bytes(b,"big")
def u32(b:bytes)->int:return int.from_bytes(b,"big")
def be16(value:int)->bytes:return value.to_bytes(2,"big")
def be32(value:int)->bytes:return value.to_bytes(4,"big")
def checksum(data:bytes)->int:
    if len(data)&1:data+=b"\x00"
    value=sum((data[i]<<8)|data[i+1] for i in range(0,len(data),2))
    while value>>16:value=(value&0xffff)+(value>>16)
    return (~value)&0xffff
def pseudo(src:bytes,dst:bytes,proto:int,payload:bytes)->int:
    return checksum(src+dst+bytes([0,proto])+be16(len(payload))+payload)
def ethernet(payload:bytes,kind:int)->bytes:
    return GUEST_MAC+PEER_MAC+be16(kind)+payload
def ipv4(payload:bytes,proto:int,ident:int)->bytes:
    head=bytearray(b"\x45\x00"+be16(20+len(payload))+
                   be16(ident)+b"\x40\x00\x40"+bytes([proto])+
                   b"\x00\x00"+PEER_IP+GUEST_IP)
    head[10:12]=be16(checksum(head))
    return ethernet(bytes(head)+payload,0x0800)
def udp(payload:bytes,src_port:int)->bytes:
    dat=bytearray(be16(src_port)+be16(CLIENT_PORT)+be16(8+len(payload))+
                  b"\x00\x00"+payload)
    c=pseudo(PEER_IP,GUEST_IP,17,dat)
    dat[6:8]=be16(c or 0xffff)
    return ipv4(bytes(dat),17,0x2901)
def tcp(flags:int,seq:int,ack:int,data:bytes=b"")->bytes:
    segment=bytearray(be16(TCP_PORT)+be16(CLIENT_PORT)+be32(seq)+
                      be32(ack)+b"\x50"+bytes([flags])+be16(4096)+
                      b"\x00\x00\x00\x00"+data)
    segment[16:18]=be16(pseudo(PEER_IP,GUEST_IP,6,segment))
    return ipv4(bytes(segment),6,0x2902)
class Protocol:
    def __init__(self):
        self.phase=0
        self.guest_ip=GUEST_IP
        self.counts={"arp":0,"ping":0,"udp":0,"dns":0,"syn":0,"data":0,"tcp_ack":0}
    def inspect(self,frame:bytes)->bytes|None:
        assert len(frame)>=14, "truncated Ethernet frame"
        dst,src,kind=frame[:6],frame[6:12],u16(frame[12:14])
        assert src==GUEST_MAC, f"unexpected guest MAC {src.hex()}"
        assert dst in (PEER_MAC,b"\xff"*6), "invalid Ethernet destination"
        if kind==0x0806:
            assert self.phase==0,"unexpected ARP request order"
            arp=frame[14:]
            assert len(arp)>=28 and arp[:8]==bytes.fromhex("0001080006040001")
            assert arp[8:14]==GUEST_MAC and arp[14:18]==GUEST_IP
            assert arp[24:28]==PEER_IP
            self.counts["arp"]+=1;self.phase=1
            response=(bytes.fromhex("0001080006040002")+PEER_MAC+PEER_IP+
                      GUEST_MAC+GUEST_IP)
            return ethernet(response,0x0806)
        assert kind==0x0800,"unexpected Ethernet ethertype"
        ip=frame[14:]
        assert len(ip)>=20 and ip[0]==0x45 and ip[8]>0
        assert checksum(ip[:20])==0,"bad IPv4 checksum"
        total=u16(ip[2:4])
        assert 20<=total<=len(ip) and u16(ip[6:8])&0x3fff==0
        assert ip[12:16]==GUEST_IP and ip[16:20]==PEER_IP
        protocol=ip[9];data=ip[20:total]
        if protocol==1:
            assert self.phase==1 and len(data)==12 and data[0]==8
            assert checksum(data)==0 and data[4:8]==bytes.fromhex("28280001")
            assert data[8:]==b"PING"
            reply=bytearray(data)
            reply[0]=0;reply[2:4]=b"\x00\x00"
            reply[2:4]=be16(checksum(reply))
            self.phase=2;self.counts["ping"]+=1
            return ipv4(bytes(reply),1,0x2903)
        if protocol==17:
            assert len(data)>=8 and u16(data[0:2])==CLIENT_PORT
            assert u16(data[4:6])==len(data) and u16(data[6:8])!=0
            assert pseudo(GUEST_IP,PEER_IP,17,data)==0
            port=u16(data[2:4]);payload=data[8:]
            if port==UDP_PORT:
                assert self.phase==2 and payload==b"r028-udp-request"
                self.phase=3;self.counts["udp"]+=1
                return udp(b"r028-udp-response",UDP_PORT)
            if port==53:
                assert self.phase==3 and len(payload)>=12+len(DNS_NAME)+4
                assert payload[:2]==b"\x28\x28" and payload[2:4]==b"\x01\x00"
                assert payload[4:12]==b"\x00\x01"+b"\x00"*6
                question=DNS_NAME+b"\x00\x01\x00\x01"
                assert payload[12:]==question
                response=(b"\x28\x28\x81\x80\x00\x01\x00\x01"+
                          b"\x00\x00\x00\x00"+question+
                          b"\xc0\x0c\x00\x01\x00\x01"+
                          b"\x00\x00\x00\x3c\x00\x04"+PEER_IP)
                self.phase=4;self.counts["dns"]+=1
                return udp(response,53)
            raise AssertionError(f"unexpected UDP destination {port}")
        if protocol==6:
            assert len(data)>=20 and pseudo(GUEST_IP,PEER_IP,6,data)==0
            assert u16(data[0:2])==CLIENT_PORT and u16(data[2:4])==TCP_PORT
            assert data[12]>>4==5,"TCP options unexpectedly present"
            seq=u32(data[4:8]);ack=u32(data[8:12])
            flags=data[13];payload=data[20:]
            if flags==2:
                assert self.phase==4 and seq==TCP_CLIENT_SEQ and ack==0 and not payload
                self.phase=5;self.counts["syn"]+=1
                return tcp(0x12,TCP_PEER_SEQ,TCP_CLIENT_SEQ+1)
            if flags==0x10 and self.phase==5:
                assert seq==TCP_CLIENT_SEQ+1 and ack==TCP_PEER_SEQ+1
                assert not payload
                self.phase=6;return None
            if flags==0x18:
                assert self.phase==6 and seq==TCP_CLIENT_SEQ+1
                assert ack==TCP_PEER_SEQ+1 and payload==b"r028-tcp-request"
                self.phase=7;self.counts["data"]+=1
                return tcp(0x18,TCP_PEER_SEQ+1,
                           TCP_CLIENT_SEQ+1+len(payload),b"r028-tcp-response")
            if flags==0x10 and self.phase==7:
                assert seq==TCP_CLIENT_SEQ+1+len(b"r028-tcp-request")
                assert ack==TCP_PEER_SEQ+1+len(b"r028-tcp-response")
                self.phase=8;self.counts["tcp_ack"]+=1
                return None
            raise AssertionError(f"unexpected TCP phase={self.phase} flags={flags}")
        raise AssertionError(f"unexpected IPv4 protocol {protocol}")

def run(qemu:str,iso:pathlib.Path,ovmf:pathlib.Path)->None:
    protocol=Protocol()
    listener=socket.socket(socket.AF_INET,socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
    listener.bind(("127.0.0.1",0))
    listener.listen(1);listener.setblocking(False)
    port=listener.getsockname()[1]
    command=[
        qemu,"-machine","pc","-m","256M","-smp","1",
        "-display","none","-monitor","none","-serial","stdio",
        "-no-reboot","-no-shutdown","-boot","d",
        "-drive",f"if=pflash,format=raw,readonly=on,file={ovmf}",
        "-cdrom",str(iso),
        "-netdev",f"socket,id=r028,connect=127.0.0.1:{port}",
        "-device","virtio-net-pci,netdev=r028,disable-modern=on,mac=52:54:00:12:34:56",
    ]
    qemu_proc=subprocess.Popen(
        command,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,bufsize=0)
    selector=selectors.DefaultSelector()
    input_bytes=bytearray();output=bytearray()
    connection=None
    selector.register(listener,selectors.EVENT_READ,"listen")
    assert qemu_proc.stdout is not None
    selector.register(qemu_proc.stdout,selectors.EVENT_READ,"serial")
    try:
        deadline=time.monotonic()+75
        while time.monotonic()<deadline:
            for key,_ in selector.select(0.25):
                kind=key.data
                if kind=="listen":
                    connection,_=listener.accept()
                    connection.setblocking(False)
                    selector.register(connection,selectors.EVENT_READ,"packet")
                elif kind=="serial":
                    data=os.read(qemu_proc.stdout.fileno(),8192)
                    if data:output.extend(data)
                elif kind=="packet":
                    assert connection is not None
                    packet=connection.recv(8192)
                    if not packet:raise AssertionError("virtio netdev socket closed")
                    input_bytes.extend(packet)
                    while len(input_bytes)>=4:
                        length=u32(input_bytes[:4])
                        if not 14<=length<=1600:
                            raise AssertionError(f"invalid NIC Ethernet length {length}")
                        if len(input_bytes)<4+length:break
                        frame=bytes(input_bytes[4:4+length])
                        del input_bytes[:4+length]
                        reply=protocol.inspect(frame)
                        if reply is not None:
                            connection.sendall(be32(len(reply))+reply)
            text=output.decode("utf-8","replace")
            if len(output)>1_000_000:raise AssertionError("excess serial output")
            if "IndianOS: FATAL" in text:
                raise AssertionError("kernel network fatal:\n"+text[-8000:])
            if all(s in text for s in MARKERS) and protocol.phase>=7:
                position=0
                for marker in MARKERS:
                    at=text.find(marker,position)
                    assert at>=0,f"missing or out of order {marker}"
                    position=at+len(marker)
                for label in ("arp","ping","udp","dns","syn","data"):
                    assert protocol.counts[label]==1,(
                        f"host expected one verified {label} frame; got {protocol.counts[label]}")
                # Socket ACK is sent after the received TCP response. The
                # kernel success marker is printed after that send completes.
                print("R028 QEMU virtio-net ARP ICMP UDP DNS TCP end-to-end PASSED")
                print("Validated real host-side Ethernet frames:",protocol.counts)
                return
            if qemu_proc.poll() is not None and not selector.get_map():
                break
        raise AssertionError("QEMU networking timeout:\n"+output[-8000:].decode("utf-8","replace"))
    finally:
        selector.close()
        if connection is not None:connection.close()
        listener.close()
        if qemu_proc.poll() is None:qemu_proc.terminate()
        try:qemu_proc.communicate(timeout=4)
        except subprocess.TimeoutExpired:
            qemu_proc.kill();qemu_proc.communicate(timeout=4)

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--iso",type=pathlib.Path,required=True)
    parser.add_argument("--qemu",required=True)
    parser.add_argument("--ovmf",type=pathlib.Path,required=True)
    args=parser.parse_args()
    if not args.iso.is_file() or not args.ovmf.is_file():
        parser.error("QEMU ISO or firmware not found")
    run(args.qemu,args.iso.resolve(),args.ovmf.resolve())
if __name__=="__main__":main()
