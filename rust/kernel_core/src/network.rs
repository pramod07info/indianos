//! R028: strict, allocator-free Ethernet/ARP/IPv4/ICMP/UDP/DNS/TCP client.
//!
//! QEMU test-only. One fixed socket endpoint and one TCP exchange; no DHCP,
//! retransmission, fragmentation, routing, general sockets or internet use.
use core::hint::spin_loop;
pub const LOCAL_IP:[u8;4]=[10,7,0,2];
pub const PEER_IP:[u8;4]=[10,7,0,1];
pub const PEER_MAC:[u8;6]=[0x52,0x54,0x00,0xab,0xcd,0xef];
const CLIENT_PORT:u16=41280;
const PEER_TCP_PORT:u16=8080;
const PEER_UDP_PORT:u16=4040;
const TCP_SEQ:u32=0x1020_3040;
const TCP_PEER_SEQ:u32=0x5060_7080;
const UDP_REQ:&[u8]=b"r028-udp-request";
const UDP_RESP:&[u8]=b"r028-udp-response";
const TCP_REQ:&[u8]=b"r028-tcp-request";
const TCP_RESP:&[u8]=b"r028-tcp-response";
const DNS_NAME:&[u8]=b"\x08indianos\x04test\x00";
const FRAME_CAP:usize=1600;
const MAX_POLL:u32=15_000_000;

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum Error { Device, Invalid, Timeout, Checksum, Unexpected }
pub trait EthernetDevice {
    fn mac(&self)->[u8;6];
    fn send(&mut self, frame:&[u8])->Result<(),Error>;
    fn poll(&mut self,out:&mut [u8])->Result<Option<usize>,Error>;
}
fn be16(b:&[u8])->u16{u16::from_be_bytes([b[0],b[1]])}
fn be32(b:&[u8])->u32{u32::from_be_bytes([b[0],b[1],b[2],b[3]])}
fn put16(buf:&mut [u8],off:usize,value:u16){buf[off..off+2].copy_from_slice(&value.to_be_bytes());}
fn put32(buf:&mut [u8],off:usize,value:u32){buf[off..off+4].copy_from_slice(&value.to_be_bytes());}
fn sum_words(mut sum:u32,data:&[u8])->u32{
    for pair in data.chunks(2) {
        sum+=((pair[0] as u32)<<8)|(pair.get(1).copied().unwrap_or(0) as u32);
    }
    sum
}
fn fold(mut value:u32)->u16{
    while value>>16!=0{value=(value&0xffff)+(value>>16);}
    !(value as u16)
}
fn checksum(data:&[u8])->u16{fold(sum_words(0,data))}
fn transport_sum(ip:&[u8;4],other:&[u8;4],protocol:u8,data:&[u8])->u16{
    let mut sum=sum_words(0,ip);
    sum=sum_words(sum,other);
    sum+=protocol as u32 +data.len() as u32;
    fold(sum_words(sum,data))
}
fn ethernet_header(out:&mut [u8],dst:[u8;6],src:[u8;6],kind:u16){
    out[..6].copy_from_slice(&dst);
    out[6..12].copy_from_slice(&src);
    put16(out,12,kind);
}
fn ipv4_header(out:&mut [u8],kind:u8,payload_len:usize,sequence:u16){
    out[..20].fill(0);
    out[0]=0x45;
    put16(out,2,(20+payload_len) as u16);
    put16(out,4,sequence);
    put16(out,6,0x4000); // Don't fragment.
    out[8]=64;out[9]=kind;
    out[12..16].copy_from_slice(&LOCAL_IP);
    out[16..20].copy_from_slice(&PEER_IP);
    let sum=checksum(&out[..20]);
    put16(out,10,sum);
}
#[derive(Clone,Copy)]
pub struct Ipv4<'a>{pub proto:u8,pub body:&'a [u8]}
fn validate_ipv4<'a>(frame:&'a [u8],local_mac:[u8;6])->Result<Ipv4<'a>,Error>{
    if frame.len()<14+20 || frame[..6]!=local_mac ||
       frame[6..12]!=PEER_MAC || be16(&frame[12..14])!=0x0800 {
        return Err(Error::Invalid);
    }
    let ip=&frame[14..];
    if ip[0]!=0x45 || ip[8]==0 || (be16(&ip[6..8])&0x3fff)!=0 ||
       ip[12..16]!=PEER_IP || ip[16..20]!=LOCAL_IP{return Err(Error::Invalid);}
    let len=be16(&ip[2..4]) as usize;
    if len<20 || len>ip.len(){return Err(Error::Invalid);}
    if checksum(&ip[..20])!=0{return Err(Error::Checksum);}
    Ok(Ipv4{proto:ip[9],body:&ip[20..len]})
}
fn arp_request(out:&mut [u8],local_mac:[u8;6])->usize{
    ethernet_header(out,[0xff;6],local_mac,0x0806);
    out[14..42].fill(0);
    put16(out,14,1);put16(out,16,0x0800);
    out[18]=6;out[19]=4;put16(out,20,1);
    out[22..28].copy_from_slice(&local_mac);
    out[28..32].copy_from_slice(&LOCAL_IP);
    out[38..42].copy_from_slice(&PEER_IP);
    42
}
fn valid_arp(frame:&[u8],mac:[u8;6])->bool{
    frame.len()>=42 && frame[..6]==mac && frame[6..12]==PEER_MAC &&
    be16(&frame[12..14])==0x0806 &&
    be16(&frame[14..16])==1 && be16(&frame[16..18])==0x800 &&
    frame[18]==6 && frame[19]==4 && be16(&frame[20..22])==2 &&
    frame[22..28]==PEER_MAC && frame[28..32]==PEER_IP &&
    frame[32..38]==mac && frame[38..42]==LOCAL_IP
}
fn icmp_request(out:&mut [u8],mac:[u8;6])->usize{
    ethernet_header(out,PEER_MAC,mac,0x0800);
    let len=12;
    ipv4_header(&mut out[14..34],1,len,0x2801);
    let icmp=&mut out[34..34+len];icmp.fill(0);
    icmp[0]=8;put16(icmp,4,0x2828);put16(icmp,6,1);
    icmp[8..12].copy_from_slice(b"PING");
    let sum=checksum(icmp);put16(icmp,2,sum);
    14+20+len
}
fn valid_icmp(frame:&[u8],mac:[u8;6])->bool{
    let Ok(p)=validate_ipv4(frame,mac)else{return false;};
    p.proto==1 && p.body.len()==12 && p.body[0]==0 &&
    checksum(p.body)==0 && be16(&p.body[4..6])==0x2828 &&
    be16(&p.body[6..8])==1 && &p.body[8..12]==b"PING"
}
fn udp_frame(out:&mut [u8],mac:[u8;6],dst_port:u16,request:&[u8],sequence:u16)->usize{
    ethernet_header(out,PEER_MAC,mac,0x0800);
    let len=8+request.len();
    ipv4_header(&mut out[14..34],17,len,sequence);
    let udp=&mut out[34..34+len];udp.fill(0);
    put16(udp,0,CLIENT_PORT);put16(udp,2,dst_port);put16(udp,4,len as u16);
    udp[8..].copy_from_slice(request);
    let sum=transport_sum(&LOCAL_IP,&PEER_IP,17,udp);
    put16(udp,6,if sum==0{0xffff}else{sum});
    34+len
}
fn parse_udp<'a>(frame:&'a [u8],mac:[u8;6],src_port:u16)
    ->Result<&'a [u8],Error>
{
    let ip=validate_ipv4(frame,mac)?;
    if ip.proto!=17 || ip.body.len()<8{return Err(Error::Invalid);}
    let body=ip.body;
    if be16(&body[0..2])!=src_port || be16(&body[2..4])!=CLIENT_PORT ||
       be16(&body[4..6]) as usize!=body.len() ||
       be16(&body[6..8])==0{return Err(Error::Invalid);}
    if transport_sum(&PEER_IP,&LOCAL_IP,17,body)!=0{
        return Err(Error::Checksum);
    }
    Ok(&body[8..])
}
fn dns_query(out:&mut [u8],mac:[u8;6])->usize{
    // DNS: deterministic transaction id, exactly one A query, no recursion to internet.
    let mut request=[0u8;64];
    put16(&mut request,0,0x2828);
    put16(&mut request,2,0x0100);
    put16(&mut request,4,1);
    request[12..12+DNS_NAME.len()].copy_from_slice(DNS_NAME);
    let end=12+DNS_NAME.len();
    put16(&mut request,end,1);
    put16(&mut request,end+2,1);
    udp_frame(out,mac,53,&request[..end+4],0x2802)
}
fn valid_dns(frame:&[u8],mac:[u8;6])->bool{
    let Ok(payload)=parse_udp(frame,mac,53)else{return false;};
    if payload.len()<12+DNS_NAME.len()+4+16 ||
       be16(&payload[..2])!=0x2828 ||
       be16(&payload[2..4])!=0x8180 ||
       be16(&payload[4..6])!=1 || be16(&payload[6..8])!=1 ||
       &payload[12..12+DNS_NAME.len()]!=DNS_NAME{return false;}
    let answer=12+DNS_NAME.len()+4;
    payload[answer..answer+2]==[0xc0,0x0c] &&
    be16(&payload[answer+2..answer+4])==1 &&
    be16(&payload[answer+4..answer+6])==1 &&
    be16(&payload[answer+10..answer+12])==4 &&
    payload[answer+12..answer+16]==PEER_IP
}
#[derive(Clone,Copy,Debug)]
pub struct TcpSegment<'a>{pub seq:u32,pub ack:u32,pub flags:u8,pub data:&'a [u8]}
fn tcp_frame(out:&mut [u8],mac:[u8;6],seq:u32,ack:u32,flags:u8,
    data:&[u8],id:u16)->usize
{
    ethernet_header(out,PEER_MAC,mac,0x0800);
    let len=20+data.len();
    ipv4_header(&mut out[14..34],6,len,id);
    let tcp=&mut out[34..34+len];tcp.fill(0);
    put16(tcp,0,CLIENT_PORT);put16(tcp,2,PEER_TCP_PORT);
    put32(tcp,4,seq);put32(tcp,8,ack);
    tcp[12]=0x50;tcp[13]=flags;
    put16(tcp,14,4096);
    tcp[20..].copy_from_slice(data);
    let sum=transport_sum(&LOCAL_IP,&PEER_IP,6,tcp);
    put16(tcp,16,sum);
    34+len
}
fn parse_tcp<'a>(frame:&'a [u8],mac:[u8;6])->Result<TcpSegment<'a>,Error>{
    let ip=validate_ipv4(frame,mac)?;
    if ip.proto!=6 || ip.body.len()<20{return Err(Error::Invalid);}
    let tcp=ip.body;
    let hlen=(tcp[12]>>4) as usize*4;
    if hlen<20 || hlen>tcp.len() ||
       be16(&tcp[..2])!=PEER_TCP_PORT ||
       be16(&tcp[2..4])!=CLIENT_PORT{return Err(Error::Invalid);}
    if transport_sum(&PEER_IP,&LOCAL_IP,6,tcp)!=0{return Err(Error::Checksum);}
    Ok(TcpSegment{seq:be32(&tcp[4..8]),ack:be32(&tcp[8..12]),
        flags:tcp[13],data:&tcp[hlen..]})
}
fn wait_for<D:EthernetDevice>(dev:&mut D,
    matcher:impl Fn(&[u8],[u8;6])->bool)->Result<(),Error>
{
    let mut buffer=[0u8;FRAME_CAP];
    let mac=dev.mac();
    for _ in 0..MAX_POLL{
        if let Some(n)=dev.poll(&mut buffer)?{
            if n>buffer.len(){return Err(Error::Invalid);}
            if matcher(&buffer[..n],mac){return Ok(());}
        }
        spin_loop();
    }
    Err(Error::Timeout)
}
fn transmit<D:EthernetDevice>(dev:&mut D,
    constructor:impl FnOnce(&mut [u8],[u8;6])->usize)->Result<(),Error>
{
    let mut buffer=[0u8;FRAME_CAP];
    let length=constructor(&mut buffer,dev.mac());
    if length<42 || length>buffer.len(){return Err(Error::Invalid);}
    dev.send(&buffer[..length])
}
/// Exercised only by QEMU's dedicated virtio-net test and a deterministic
/// TCP-framed Ethernet peer on the host; no arbitrary remote traffic.
pub fn run_demo<D:EthernetDevice>(
    dev:&mut D,mut stage:impl FnMut(&'static [u8])
)->Result<(),Error>
{
    transmit(dev,arp_request)?;
    wait_for(dev,valid_arp)?;
    stage(b"IndianOS: R028 Ethernet ARP peer resolved\n\0");
    transmit(dev,icmp_request)?;
    wait_for(dev,valid_icmp)?;
    stage(b"IndianOS: R028 IPv4 ICMP echo ping PASSED\n\0");
    transmit(dev,|b,mac|udp_frame(b,mac,PEER_UDP_PORT,UDP_REQ,0x2803))?;
    wait_for(dev,|frame,mac|{
        parse_udp(frame,mac,PEER_UDP_PORT)
            .is_ok_and(|data|data==UDP_RESP)
    })?;
    stage(b"IndianOS: R028 IPv4 UDP echo PASSED\n\0");
    transmit(dev,dns_query)?;
    wait_for(dev,valid_dns)?;
    stage(b"IndianOS: R028 UDP DNS A query indianos.test PASSED\n\0");
    // Strict stop-and-wait TCP handshake and one request-response exchange.
    transmit(dev,|b,m|tcp_frame(b,m,TCP_SEQ,0,0x02,&[],0x2804))?;
    wait_for(dev,|frame,mac|{
        parse_tcp(frame,mac).is_ok_and(|seg|
            seg.flags&0x12==0x12 && seg.flags&0x04==0 &&
            seg.seq==TCP_PEER_SEQ && seg.ack==TCP_SEQ+1 &&
            seg.data.is_empty())
    })?;
    stage(b"IndianOS: R028 TCP three-way handshake SYN-ACK verified\n\0");
    transmit(dev,|b,m|tcp_frame(b,m,TCP_SEQ+1,TCP_PEER_SEQ+1,0x10,&[],0x2805))?;
    transmit(dev,|b,m|tcp_frame(b,m,TCP_SEQ+1,TCP_PEER_SEQ+1,0x18,TCP_REQ,0x2806))?;
    wait_for(dev,|frame,mac|{
        parse_tcp(frame,mac).is_ok_and(|seg|
            seg.flags&0x18==0x18 && seg.flags&0x04==0 &&
            seg.seq==TCP_PEER_SEQ+1 &&
            seg.ack==TCP_SEQ+1+TCP_REQ.len() as u32 &&
            seg.data==TCP_RESP)
    })?;
    transmit(dev,|b,m|tcp_frame(b,m,TCP_SEQ+1+TCP_REQ.len() as u32,
        TCP_PEER_SEQ+1+TCP_RESP.len() as u32,0x10,&[],0x2807))?;
    stage(b"IndianOS: R028 TCP client request-response PASSED\n\0");
    Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn rfc_checksums_and_packet_bounds(){
        assert_eq!(checksum(b"\x00\x01\xf2\x03\xf4"),0x19fa);
        let mut frame=[0u8;FRAME_CAP];
        let mac=[0x52,0x54,0,0x12,0x34,0x56];
        let n=icmp_request(&mut frame,mac);
        let ip=&frame[14..34];
        assert_eq!(checksum(ip),0);
        assert_eq!(checksum(&frame[34..n]),0);
        assert_eq!(frame[34],8);
        frame[20]|=0x20; // IP packet frag flags invalid, do not accept.
        let mut echoed=[0u8;FRAME_CAP];
        echoed[..n].copy_from_slice(&frame[..n]);
        assert!(validate_ipv4(&echoed[..n],mac).is_err());
    }
    #[test]
    fn dns_request_and_udp_checksum(){
        let mut frame=[0u8;FRAME_CAP];let mac=[0x52,0x54,0,0x12,0x34,0x56];
        let n=dns_query(&mut frame,mac);
        let ip=&frame[14..34];let udp=&frame[34..n];
        assert_eq!(checksum(ip),0);
        assert_eq!(transport_sum(&LOCAL_IP,&PEER_IP,17,udp),0);
        assert!(udp.ends_with(b"\x00\x01\x00\x01"));
        assert!(udp.windows(DNS_NAME.len()).any(|w|w==DNS_NAME));
    }
    #[test]
    fn tcp_syn_has_valid_wire_checksum(){
        let mut frame=[0u8;FRAME_CAP];let mac=[0x52,0x54,0,0x12,0x34,0x56];
        let n=tcp_frame(&mut frame,mac,TCP_SEQ,0,0x02,&[],0x2855);
        assert_eq!(n,54);
        assert_eq!(transport_sum(&LOCAL_IP,&PEER_IP,6,&frame[34..n]),0);
        assert_eq!(be32(&frame[38..42]),TCP_SEQ);
        assert_eq!(frame[47],0x02);
    }
    #[test]
    fn bounds_refuse_invalid_ethernet_and_ipv4(){
        let mac=[1,2,3,4,5,6];
        assert!(matches!(validate_ipv4(&[0u8;5],mac),Err(Error::Invalid)));
        let mut frame=[0u8;FRAME_CAP];
        let n=icmp_request(&mut frame,mac);
        frame[..6].copy_from_slice(&mac);
        frame[6..12].copy_from_slice(&PEER_MAC);
        assert!(validate_ipv4(&frame[..n],mac).is_ok());
        frame[24]^=0x40;
        assert!(matches!(validate_ipv4(&frame[..n],mac),Err(Error::Checksum)));
        frame[24]^=0x40;
        put16(&mut frame,16,4000);
        assert!(matches!(validate_ipv4(&frame[..n],mac),Err(Error::Invalid)));
    }
}
