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
    if ip[0]!=0x45 || ip[8]==0 || (be16(&ip[6..8])&0xbfff)!=0 ||
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
    p.proto==1 && p.body.len()==12 && p.body[0]==0 && p.body[1]==0 &&
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
    // The trusted NIC-delivered frame first passes Ethernet/IPv4 peer,
    // destination, length, checksum and fragmentation validation.
    let Ok(ip)=validate_ipv4(frame,mac)else{return false;};
    if ip.proto!=17{return false;}
    let ip_len=be16(&frame[16..18]) as usize;
    // The M2 verifier binds checksum, ports, query ID, exact question,
    // response length and expected DNS A answer. Ignore Ethernet padding.
    crate::m2_network::parse_ipv4_udp_dns_a(
        &frame[14..14+ip_len],PEER_IP,LOCAL_IP,CLIENT_PORT,0x2828,
        b"indianos.test"
    )==Ok(PEER_IP)
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
    if hlen<20 || hlen>tcp.len() || tcp[12]&0x0f!=0 ||
       be16(&tcp[..2])!=PEER_TCP_PORT ||
       be16(&tcp[2..4])!=CLIENT_PORT{return Err(Error::Invalid);}
    if transport_sum(&PEER_IP,&LOCAL_IP,6,tcp)!=0{return Err(Error::Checksum);}
    let flags=tcp[13];
    // This non-ECN demo cannot accept contradictory connection control or
    // malformed options simply because their transport checksum is valid.
    if flags&0x03==0x03 || flags&0x06==0x06 || flags&0x05==0x05 ||
       !valid_tcp_options(&tcp[20..hlen],flags&0x02!=0){
        return Err(Error::Invalid);
    }
    Ok(TcpSegment{seq:be32(&tcp[4..8]),ack:be32(&tcp[8..12]),
        flags:tcp[13],data:&tcp[hlen..]})
}
fn valid_tcp_options(mut options:&[u8],syn:bool)->bool{
    while let Some(&kind)=options.first(){
        match kind{
            0=>return options[1..].iter().all(|&b|b==0), // EOL padding
            1=>{options=&options[1..];continue;}, // NOP
            _=>{},
        }
        let Some(&size)=options.get(1)else{return false;};
        let size=size as usize;
        if size<2||size>options.len(){return false;}
        match kind{
            2 if !syn||size!=4=>return false, // MSS
            3 if !syn||size!=3||options[2]>14=>return false, // window scale
            4 if !syn||size!=2=>return false, // SACK permitted
            5 if syn||size<10||(size-2)%8!=0=>return false, // SACK blocks
            8 if size!=10=>return false, // timestamps
            _=>{}, // Unknown well-framed options are ignored per TCP.
        }
        options=&options[size..];
    }
    true
}
fn wait_for<D:EthernetDevice>(dev:&mut D,
    mut matcher:impl FnMut(&[u8],[u8;6])->bool)->Result<(),Error>
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
    // R034: deliver the actual NIC UDP reply to the PID-owned queue.
    // This QEMU demo uses an explicitly trusted, synthetic PID 1.
    let mut udp_endpoints = UdpEndpoints::<1, 2>::new();
    udp_endpoints.bind(1, CLIENT_PORT).map_err(|_| Error::Unexpected)?;
    transmit(dev,|b,mac|udp_frame(b,mac,PEER_UDP_PORT,UDP_REQ,0x2803))?;
    wait_for(dev,|frame,mac|{
        parse_udp(frame,mac,PEER_UDP_PORT).is_ok_and(|data|data==UDP_RESP)
            && udp_endpoints.ingest_ethernet_frame(frame,mac).is_ok()
    })?;
    let mut udp_reply = [0u8; R033_UDP_BYTES];
    let bytes = udp_endpoints.receive(1, CLIENT_PORT, &mut udp_reply)
        .map_err(|_| Error::Unexpected)?;
    if &udp_reply[..bytes] != UDP_RESP { return Err(Error::Unexpected); }
    udp_endpoints.close_all(1);
    stage(b"IndianOS: R028 IPv4 UDP echo PASSED\n\0");
    transmit(dev,dns_query)?;
    wait_for(dev,valid_dns)?;
    stage(b"IndianOS: R028 UDP DNS A query indianos.test PASSED\n\0");
    stage(b"IndianOS: M2 live NIC IPv4/UDP DNS verifier PASSED\n\0");
    // Strict stop-and-wait TCP handshake and one request-response exchange.
    transmit(dev,|b,m|tcp_frame(b,m,TCP_SEQ,0,0x02,&[],0x2804))?;
    wait_for(dev,|frame,mac|{
        parse_tcp(frame,mac).is_ok_and(|seg|
            seg.flags==0x12 &&
            seg.seq==TCP_PEER_SEQ && seg.ack==TCP_SEQ+1 &&
            seg.data.is_empty())
    })?;
    stage(b"IndianOS: R028 TCP three-way handshake SYN-ACK verified\n\0");
    transmit(dev,|b,m|tcp_frame(b,m,TCP_SEQ+1,TCP_PEER_SEQ+1,0x10,&[],0x2805))?;
    transmit(dev,|b,m|tcp_frame(b,m,TCP_SEQ+1,TCP_PEER_SEQ+1,0x18,TCP_REQ,0x2806))?;
    wait_for(dev,|frame,mac|{
        parse_tcp(frame,mac).is_ok_and(|seg|
            seg.flags==0x18 &&
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
    const TEST_MAC:[u8;6]=[2,0,0,0,0,1];
    fn incoming_tcp(options:&[u8],flags:u8)->([u8;FRAME_CAP],usize){
        assert_eq!(options.len()%4,0);
        let mut frame=[0u8;FRAME_CAP];
        let n=tcp_frame(&mut frame,TEST_MAC,TCP_PEER_SEQ,TCP_SEQ+1,flags,&[],1)
            +options.len();
        frame[..6].copy_from_slice(&TEST_MAC);frame[6..12].copy_from_slice(&PEER_MAC);
        frame[26..30].copy_from_slice(&PEER_IP);frame[30..34].copy_from_slice(&LOCAL_IP);
        put16(&mut frame,16,(n-14) as u16);
        put16(&mut frame,24,0);let sum=checksum(&frame[14..34]);put16(&mut frame,24,sum);
        let tcp=&mut frame[34..n];
        put16(tcp,0,PEER_TCP_PORT);put16(tcp,2,CLIENT_PORT);
        tcp[12]=(((20+options.len())/4) as u8)<<4;
        tcp[20..].copy_from_slice(options);
        put16(tcp,16,0);let sum=transport_sum(&PEER_IP,&LOCAL_IP,6,tcp);put16(tcp,16,sum);
        (frame,n)
    }
    #[test]fn checksummed_tcp_options_and_control_flags_are_validated(){
        for options in [&[2,4,5,180][..],&[1,3,3,7][..],&[4,2,0,0][..],
            &[30,4,1,2][..],&[][..]]{
            let (f,n)=incoming_tcp(options,0x12);
            assert!(parse_tcp(&f[..n],TEST_MAC).is_ok());
        }
        for options in [&[2,0,0,0][..],&[2,5,0,0][..],&[3,3,15,0][..],
            &[4,3,0,0][..],&[8,4,0,0][..],&[0,1,0,0][..],&[1,1,1,2][..]]{
            let (f,n)=incoming_tcp(options,0x12);
            assert!(matches!(parse_tcp(&f[..n],TEST_MAC),Err(Error::Invalid)),"{options:?}");
        }
        for flags in [0x13,0x16,0x15]{
            let (f,n)=incoming_tcp(&[],flags);
            assert!(matches!(parse_tcp(&f[..n],TEST_MAC),Err(Error::Invalid)));
        }
        let (f,n)=incoming_tcp(&[2,4,5,180],0x10);
        assert!(matches!(parse_tcp(&f[..n],TEST_MAC),Err(Error::Invalid)));
    }
    #[test]fn reserved_ipv4_and_tcp_bits_rejected_even_with_valid_checksums(){
        let (mut f,n)=incoming_tcp(&[],0x12);
        put16(&mut f,20,0x8000);put16(&mut f,24,0);
        let sum=checksum(&f[14..34]);put16(&mut f,24,sum);
        assert!(matches!(parse_tcp(&f[..n],TEST_MAC),Err(Error::Invalid)));
        let (mut f,n)=incoming_tcp(&[],0x12);f[46]|=1;
        put16(&mut f,50,0);let sum=transport_sum(&PEER_IP,&LOCAL_IP,6,&f[34..n]);
        put16(&mut f,50,sum);
        assert!(matches!(parse_tcp(&f[..n],TEST_MAC),Err(Error::Invalid)));
        // Every truncation must return an error rather than index out of bounds.
        let (f,n)=incoming_tcp(&[2,4,5,180],0x12);
        for length in 0..n{assert!(parse_tcp(&f[..length],TEST_MAC).is_err());}
    }
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
        // Convert outgoing fixture to an incoming peer frame.
        frame[26..30].copy_from_slice(&PEER_IP);
        frame[30..34].copy_from_slice(&LOCAL_IP);
        frame[24..26].fill(0);
        let header_checksum=checksum(&frame[14..34]);
        put16(&mut frame,24,header_checksum);
        assert!(validate_ipv4(&frame[..n],mac).is_ok());
        frame[24]^=0x40;
        assert!(matches!(validate_ipv4(&frame[..n],mac),Err(Error::Checksum)));
        frame[24]^=0x40;
        put16(&mut frame,16,4000);
        assert!(matches!(validate_ipv4(&frame[..n],mac),Err(Error::Invalid)));
    }
}


/// Deterministic, fixed-size UDP endpoint and receive queue. A trusted network
/// parser must validate IPv4/UDP lengths and checksums before calling enqueue;
/// this layer adds process and port ownership, not packet authentication.
pub const R033_UDP_BYTES: usize = 64;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketError { Invalid, Duplicate, Full, Denied, Empty, TooShort }
#[derive(Clone, Copy)]
struct PortOwner { pid: u32, port: u16 }
#[derive(Clone, Copy)]
struct QueuedUdp {
    destination_pid: u32, destination_port: u16, len: usize,
    bytes: [u8; R033_UDP_BYTES],
}
pub struct UdpEndpoints<const PORTS: usize, const QUEUE: usize> {
    ports: [Option<PortOwner>; PORTS],
    queue: [Option<QueuedUdp>; QUEUE],
    len: usize,
}
impl<const PORTS: usize, const QUEUE: usize> UdpEndpoints<PORTS, QUEUE> {
    pub const fn new() -> Self {
        Self { ports: [None; PORTS], queue: [None; QUEUE], len: 0 }
    }
    pub fn bind(&mut self, pid: u32, port: u16) -> Result<(), SocketError> {
        if pid == 0 || port == 0 { return Err(SocketError::Invalid); }
        if self.ports.iter().flatten().any(|p| p.port == port) {
            return Err(SocketError::Duplicate);
        }
        let slot = self.ports.iter_mut().find(|x| x.is_none()).ok_or(SocketError::Full)?;
        *slot = Some(PortOwner { pid, port });
        Ok(())
    }
    pub fn owns(&self, pid: u32, port: u16) -> bool {
        self.ports.iter().flatten().any(|p| p.pid == pid && p.port == port)
    }
    /// Called only after successful packet checksum/port validation.
    pub(crate) fn enqueue(&mut self, dest_port: u16, data: &[u8])
        -> Result<(), SocketError>
    {
        if data.is_empty() || data.len() > R033_UDP_BYTES { return Err(SocketError::Invalid); }
        let pid = self.ports.iter().flatten().find(|p| p.port == dest_port)
            .map(|p| p.pid).ok_or(SocketError::Denied)?;
        if self.len == QUEUE { return Err(SocketError::Full); }
        let mut bytes = [0; R033_UDP_BYTES];
        bytes[..data.len()].copy_from_slice(data);
        self.queue[self.len] = Some(QueuedUdp {
            destination_pid: pid, destination_port: dest_port, len: data.len(), bytes,
        });
        self.len += 1;
        Ok(())
    }
    pub fn receive(&mut self, pid: u32, port: u16, out: &mut [u8])
        -> Result<usize, SocketError>
    {
        if !self.owns(pid, port) { return Err(SocketError::Denied); }
        let index = self.queue[..self.len].iter().position(|q|
            q.is_some_and(|d| d.destination_pid == pid && d.destination_port == port)
        ).ok_or(SocketError::Empty)?;
        let item = self.queue[index].ok_or(SocketError::Empty)?;
        if out.len() < item.len { return Err(SocketError::TooShort); }
        out[..item.len].copy_from_slice(&item.bytes[..item.len]);
        for n in index..self.len - 1 { self.queue[n] = self.queue[n + 1]; }
        self.len -= 1;
        self.queue[self.len] = None;
        Ok(item.len)
    }
    pub fn close_all(&mut self, pid: u32) {
        if pid == 0 { return; }
        for p in &mut self.ports {
            if p.is_some_and(|entry| entry.pid == pid) { *p = None; }
        }
        let mut i = 0;
        while i < self.len {
            if self.queue[i].is_some_and(|entry| entry.destination_pid == pid) {
                for n in i..self.len - 1 { self.queue[n] = self.queue[n + 1]; }
                self.len -= 1;
                self.queue[self.len] = None;
            } else { i += 1; }
        }
    }
}
#[cfg(test)]
mod r033_udp_tests {
    use super::*;
    #[test]
    fn delivery_is_isolated_by_authenticated_pid_and_port() {
        let mut t = UdpEndpoints::<2, 4>::new();
        assert_eq!(t.bind(1, 4050), Ok(()));
        assert_eq!(t.bind(2, 4051), Ok(()));
        assert_eq!(t.bind(3, 4050), Err(SocketError::Duplicate));
        assert_eq!(t.enqueue(4050, b"data"), Ok(()));
        let mut buf = [0; 8];
        assert_eq!(t.receive(2, 4050, &mut buf), Err(SocketError::Denied));
        assert_eq!(t.receive(1, 4050, &mut buf), Ok(4));
        assert_eq!(&buf[..4], b"data");
    }
    #[test]
    fn too_short_read_preserves_queued_message() {
        let mut t = UdpEndpoints::<1, 1>::new();
        t.bind(5, 6000).unwrap();
        t.enqueue(6000, b"payload").unwrap();
        assert_eq!(t.receive(5, 6000, &mut [0; 2]), Err(SocketError::TooShort));
        let mut buf = [0; 8];
        assert_eq!(t.receive(5, 6000, &mut buf), Ok(7));
        assert_eq!(t.receive(5, 6000, &mut buf), Err(SocketError::Empty));
    }
    #[test]
    fn full_queue_invalid_lengths_and_fault_teardown() {
        let mut t = UdpEndpoints::<2, 1>::new();
        t.bind(5, 6000).unwrap();
        t.bind(6, 6001).unwrap();
        assert_eq!(t.enqueue(6000, &[]), Err(SocketError::Invalid));
        t.enqueue(6000, b"x").unwrap();
        assert_eq!(t.enqueue(6001, b"y"), Err(SocketError::Full));
        t.close_all(5);
        assert!(!t.owns(5, 6000));
        assert_eq!(t.enqueue(6000, b"y"), Err(SocketError::Denied));
        assert_eq!(t.enqueue(6001, b"y"), Ok(()));
    }
}


/// R034: validate network input before it can enter a process-owned UDP queue.
/// The caller must supply a NIC-received frame and the actual NIC local MAC.
/// Neither this API nor a PID may be exposed as unverified userspace input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IngressError { Malformed, BadChecksum, Endpoint(SocketError) }

impl<const PORTS: usize, const QUEUE: usize> UdpEndpoints<PORTS, QUEUE> {
    /// Single-peer QEMU ingress, not a general-purpose UDP socket service.
    /// The queue is not modified if any framing or checksum check fails.
    pub fn ingest_ethernet_frame(
        &mut self, frame: &[u8], local_mac: [u8; 6],
    ) -> Result<(), IngressError> {
        let ip = validate_ipv4(frame, local_mac).map_err(|e| match e {
            Error::Checksum => IngressError::BadChecksum,
            _ => IngressError::Malformed,
        })?;
        if ip.proto != 17 || ip.body.len() < 8 {
            return Err(IngressError::Malformed);
        }
        let udp = ip.body;
        let source_port = be16(&udp[0..2]);
        let destination_port = be16(&udp[2..4]);
        let length = be16(&udp[4..6]) as usize;
        if source_port == 0 || destination_port == 0 ||
            length != udp.len() || be16(&udp[6..8]) == 0 {
            return Err(IngressError::Malformed);
        }
        if transport_sum(&PEER_IP, &LOCAL_IP, 17, udp) != 0 {
            return Err(IngressError::BadChecksum);
        }
        self.enqueue(destination_port, &udp[8..])
            .map_err(IngressError::Endpoint)
    }
}

#[cfg(test)]
mod r034_ingress_tests {
    use super::*;
    const LOCAL_MAC: [u8; 6] = [0x52, 0x54, 0, 0x12, 0x34, 0x56];

    fn peer_frame(port: u16, data: &[u8]) -> ([u8; FRAME_CAP], usize) {
        let mut frame = [0u8; FRAME_CAP];
        let udp_length = 8 + data.len();
        ethernet_header(&mut frame, LOCAL_MAC, PEER_MAC, 0x0800);
        ipv4_header(&mut frame[14..34], 17, udp_length, 0x3401);
        frame[26..30].copy_from_slice(&PEER_IP);
        frame[30..34].copy_from_slice(&LOCAL_IP);
        frame[24..26].fill(0);
        let ip_sum = checksum(&frame[14..34]);
        put16(&mut frame, 24, ip_sum);
        let udp = &mut frame[34..34 + udp_length];
        put16(udp, 0, PEER_UDP_PORT);
        put16(udp, 2, port);
        put16(udp, 4, udp_length as u16);
        udp[8..].copy_from_slice(data);
        let sum = transport_sum(&PEER_IP, &LOCAL_IP, 17, udp);
        put16(udp, 6, if sum == 0 { 0xffff } else { sum });
        (frame, 34 + udp_length)
    }

    #[test]
    fn validated_wire_payload_is_delivered_to_right_pid_in_fifo_order() {
        let mut endpoints = UdpEndpoints::<2, 3>::new();
        endpoints.bind(10, 6000).unwrap();
        endpoints.bind(20, 7000).unwrap();
        let (a, an) = peer_frame(6000, b"first");
        let (b, bn) = peer_frame(7000, b"peer");
        let (c, cn) = peer_frame(6000, b"last");
        assert_eq!(endpoints.ingest_ethernet_frame(&a[..an], LOCAL_MAC), Ok(()));
        assert_eq!(endpoints.ingest_ethernet_frame(&b[..bn], LOCAL_MAC), Ok(()));
        assert_eq!(endpoints.ingest_ethernet_frame(&c[..cn], LOCAL_MAC), Ok(()));
        let mut out = [0u8; R033_UDP_BYTES];
        assert_eq!(endpoints.receive(20, 6000, &mut out), Err(SocketError::Denied));
        assert_eq!(endpoints.receive(10, 6000, &mut out), Ok(5));
        assert_eq!(&out[..5], b"first");
        assert_eq!(endpoints.receive(20, 7000, &mut out), Ok(4));
        assert_eq!(&out[..4], b"peer");
        assert_eq!(endpoints.receive(10, 6000, &mut out), Ok(4));
        assert_eq!(&out[..4], b"last");
    }

    #[test]
    fn corrupted_mac_ip_udp_and_short_frames_are_rejected_without_delivery() {
        let mut endpoints = UdpEndpoints::<1, 1>::new();
        endpoints.bind(7, 6000).unwrap();
        let (valid, len) = peer_frame(6000, b"payload");
        let mut invalid = valid;
        invalid[0] ^= 1;
        assert_eq!(endpoints.ingest_ethernet_frame(&invalid[..len], LOCAL_MAC),
            Err(IngressError::Malformed));
        invalid = valid;
        invalid[24] ^= 1;
        assert_eq!(endpoints.ingest_ethernet_frame(&invalid[..len], LOCAL_MAC),
            Err(IngressError::BadChecksum));
        invalid = valid;
        invalid[42] ^= 1;
        assert_eq!(endpoints.ingest_ethernet_frame(&invalid[..len], LOCAL_MAC),
            Err(IngressError::BadChecksum));
        invalid = valid;
        put16(&mut invalid, 38, (len - 35) as u16);
        assert_eq!(endpoints.ingest_ethernet_frame(&invalid[..len], LOCAL_MAC),
            Err(IngressError::Malformed));
        assert_eq!(endpoints.ingest_ethernet_frame(&valid[..30], LOCAL_MAC),
            Err(IngressError::Malformed));
        assert_eq!(endpoints.receive(7, 6000, &mut [0u8; 16]),
            Err(SocketError::Empty));
    }

    #[test]
    fn unknown_port_overflow_full_queue_and_short_read_are_fail_closed() {
        let mut endpoints = UdpEndpoints::<1, 1>::new();
        endpoints.bind(3, 6000).unwrap();
        let (other, on) = peer_frame(9000, b"x");
        assert_eq!(endpoints.ingest_ethernet_frame(&other[..on], LOCAL_MAC),
            Err(IngressError::Endpoint(SocketError::Denied)));
        let (huge, hn) = peer_frame(6000, &[42u8; R033_UDP_BYTES + 1]);
        assert_eq!(endpoints.ingest_ethernet_frame(&huge[..hn], LOCAL_MAC),
            Err(IngressError::Endpoint(SocketError::Invalid)));
        let (valid, vn) = peer_frame(6000, b"ok");
        assert_eq!(endpoints.ingest_ethernet_frame(&valid[..vn], LOCAL_MAC), Ok(()));
        assert_eq!(endpoints.ingest_ethernet_frame(&valid[..vn], LOCAL_MAC),
            Err(IngressError::Endpoint(SocketError::Full)));
        assert_eq!(endpoints.receive(3, 6000, &mut [0u8; 1]), Err(SocketError::TooShort));
        let mut out = [0u8; 2];
        assert_eq!(endpoints.receive(3, 6000, &mut out), Ok(2));
        assert_eq!(out, *b"ok");
    }

    #[test]
    fn fault_reaps_queued_messages_without_disturbing_other_processes() {
        let mut endpoints = UdpEndpoints::<2, 2>::new();
        endpoints.bind(4, 6000).unwrap();
        endpoints.bind(5, 7000).unwrap();
        let (old, old_len) = peer_frame(6000, b"old");
        let (peer, peer_len) = peer_frame(7000, b"live");
        endpoints.ingest_ethernet_frame(&old[..old_len], LOCAL_MAC).unwrap();
        endpoints.ingest_ethernet_frame(&peer[..peer_len], LOCAL_MAC).unwrap();
        endpoints.close_all(4);
        endpoints.bind(6, 6000).unwrap();
        let mut out = [0u8; 8];
        assert_eq!(endpoints.receive(6, 6000, &mut out), Err(SocketError::Empty));
        assert_eq!(endpoints.receive(5, 7000, &mut out), Ok(4));
        assert_eq!(&out[..4], b"live");
    }
}


/// Batch 1: generation-bound process endpoint adapter around the actual R034
/// validated-ingress queue. Scheduler supplies trusted (PID,generation).
/// A recycled PID must retire its old bindings before registering new ones.
pub struct EpochUdpEndpoints<const PORTS: usize, const QUEUE: usize> {
    endpoints: UdpEndpoints<PORTS, QUEUE>,
    owners: [Option<(u32,u64)>; PORTS],
}
impl<const PORTS: usize, const QUEUE: usize> EpochUdpEndpoints<PORTS, QUEUE> {
    pub const fn new() -> Self {
        Self { endpoints: UdpEndpoints::new(), owners: [None; PORTS] }
    }
    fn authenticated(&self, pid: u32, generation: u64) -> bool {
        pid != 0 && generation != 0 && self.owners.iter().flatten()
            .any(|&(owner, epoch)| owner == pid && epoch == generation)
    }
    pub fn bind(&mut self, pid: u32, generation: u64, port: u16)
        -> Result<(), SocketError> {
        if pid == 0 || generation == 0 || port == 0 {
            return Err(SocketError::Invalid);
        }
        if self.owners.iter().flatten().any(|&(owner,epoch)|
            owner == pid && epoch != generation) {
            return Err(SocketError::Denied);
        }
        let known = self.authenticated(pid,generation);
        if !known && self.owners.iter().all(Option::is_some) {
            return Err(SocketError::Full);
        }
        self.endpoints.bind(pid,port)?;
        if !known {
            let free = self.owners.iter_mut().find(|o|o.is_none())
                .ok_or(SocketError::Full)?;
            *free = Some((pid,generation));
        }
        Ok(())
    }
    pub fn receive(&mut self, pid: u32, generation: u64,
        port: u16, out: &mut [u8]) -> Result<usize,SocketError> {
        if !self.authenticated(pid,generation) { return Err(SocketError::Denied); }
        self.endpoints.receive(pid,port,out)
    }
    pub fn ingest_ethernet_frame(&mut self, frame: &[u8], mac: [u8;6])
        -> Result<(),IngressError> {
        self.endpoints.ingest_ethernet_frame(frame,mac)
    }
    /// Only a matching live generation may retire an endpoint.
    pub fn retire(&mut self, pid: u32, generation: u64)
        -> Result<(),SocketError> {
        if !self.authenticated(pid,generation) { return Err(SocketError::Denied); }
        self.endpoints.close_all(pid);
        for slot in &mut self.owners {
            if slot.is_some_and(|pair|pair == (pid,generation)){*slot=None;}
        }
        Ok(())
    }
}
#[cfg(test)]
mod batch1_epoch_udp_tests {
    use super::*;
    #[test]
    fn pid_reuse_never_inherits_queued_udp() {
        let mut e=EpochUdpEndpoints::<2,3>::new();
        e.bind(11,1,6000).unwrap();
        e.endpoints.enqueue(6000,b"old").unwrap();
        assert_eq!(e.receive(11,2,6000,&mut[0u8;8]),Err(SocketError::Denied));
        assert_eq!(e.bind(11,2,6001),Err(SocketError::Denied));
        assert_eq!(e.retire(11,2),Err(SocketError::Denied));
        e.retire(11,1).unwrap();
        e.bind(11,2,6000).unwrap();
        assert_eq!(e.receive(11,2,6000,&mut[0u8;8]),Err(SocketError::Empty));
    }
    #[test]
    fn retirement_preserves_peer_messages() {
        let mut e=EpochUdpEndpoints::<2,3>::new();
        e.bind(11,1,6000).unwrap();
        e.bind(12,1,6001).unwrap();
        e.endpoints.enqueue(6000,b"retired").unwrap();
        e.endpoints.enqueue(6001,b"live").unwrap();
        e.retire(11,1).unwrap();
        let mut out=[0u8;8];
        assert_eq!(e.receive(12,1,6001,&mut out),Ok(4));
        assert_eq!(&out[..4],b"live");
    }
}



#[cfg(test)]
mod r038_live_dns_tests {
    use super::*;
    const MAC:[u8;6]=[0x52,0x54,0,0x12,0x34,0x56];
    fn response()->([u8;FRAME_CAP],usize) {
        let mut frame=[0u8;FRAME_CAP];
        let dns_length=12+DNS_NAME.len()+4+16;
        let udp_length=8+dns_length;
        let len=14+20+udp_length;
        ethernet_header(&mut frame,MAC,PEER_MAC,0x0800);
        ipv4_header(&mut frame[14..34],17,udp_length,0x3828);
        frame[26..30].copy_from_slice(&PEER_IP);
        frame[30..34].copy_from_slice(&LOCAL_IP);
        put16(&mut frame,24,0);
        let ip_sum=checksum(&frame[14..34]);
        put16(&mut frame,24,ip_sum);
        let udp=&mut frame[34..len];
        put16(udp,0,53);
        put16(udp,2,CLIENT_PORT);
        put16(udp,4,udp_length as u16);
        let dns=&mut udp[8..];
        dns[..12].copy_from_slice(b"\x28\x28\x81\x80\x00\x01\x00\x01\x00\x00\x00\x00");
        dns[12..12+DNS_NAME.len()].copy_from_slice(DNS_NAME);
        let answer=12+DNS_NAME.len();
        dns[answer..answer+4].copy_from_slice(&[0,1,0,1]);
        dns[answer+4..answer+20].copy_from_slice(&[
            0xc0,0x0c,0,1,0,1,0,0,0,60,0,4,
            PEER_IP[0],PEER_IP[1],PEER_IP[2],PEER_IP[3]
        ]);
        let udp_sum=transport_sum(&PEER_IP,&LOCAL_IP,17,udp);
        put16(udp,6,if udp_sum==0{0xffff}else{udp_sum});
        (frame,len)
    }
    #[test]
    fn r038_live_dns_accepts_full_checksums_and_ethernet_padding(){
        let (frame,n)=response();
        assert!(valid_dns(&frame[..n],MAC));
        assert!(valid_dns(&frame[..n+4],MAC));
    }
    #[test]
    fn r038_live_dns_rejects_mac_port_answer_and_transaction_tamper(){
        let (frame,n)=response();
        let mut bad=frame;bad[0]^=1;
        assert!(!valid_dns(&bad[..n],MAC));
        bad=frame;bad[34]^=1;
        assert!(!valid_dns(&bad[..n],MAC));
        bad=frame;bad[n-1]^=1;
        assert!(!valid_dns(&bad[..n],MAC));
        bad=frame;bad[42]^=1;
        assert!(!valid_dns(&bad[..n],MAC));
    }
    #[test]
    fn r038_live_dns_rejects_truncation_and_fragmentation(){
        let (frame,n)=response();
        assert!(!valid_dns(&frame[..n-1],MAC));
        let mut bad=frame;bad[20]=0x20;
        assert!(!valid_dns(&bad[..n],MAC));
    }
}
