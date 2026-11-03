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
fn wait_for_limit<D:EthernetDevice>(dev:&mut D,limit:u32,
    mut matcher:impl FnMut(&[u8],[u8;6])->bool)->Result<(),Error>
{
    let mut buffer=[0u8;FRAME_CAP];
    let mac=dev.mac();
    for _ in 0..limit{
        if let Some(n)=dev.poll(&mut buffer)?{
            if n>buffer.len(){return Err(Error::Invalid);}
            if matcher(&buffer[..n],mac){return Ok(());}
        }
        spin_loop();
    }
    Err(Error::Timeout)
}
/// Poll a genuine NIC frame with the ownership revision sampled BEFORE
/// each poll, not after a possibly delayed DMA completion. The Rust caller
/// exclusively owns NIC, policy and socket tables on the experimental BSP.
/// Physical IRQ-driven drivers need hardware quiescence and a BSP permit.
fn wait_for_limit_fenced<D:EthernetDevice>(
    dev:&mut D,limit:u32,
    mut capture:impl FnMut()->crate::process_retirement_txn::IngressRevision,
    mut matcher:impl FnMut(
        crate::process_retirement_txn::IngressRevision,&[u8],[u8;6]
    )->bool,
)->Result<(),Error>{
    let mut frame=[0u8;FRAME_CAP];
    let mac=dev.mac();
    for _ in 0..limit{
        let pre_poll=capture();
        if let Some(n)=dev.poll(&mut frame)?{
            if n>frame.len(){return Err(Error::Invalid);}
            if matcher(pre_poll,&frame[..n],mac){return Ok(());}
        }
        spin_loop();
    }
    Err(Error::Timeout)
}
fn wait_for<D:EthernetDevice>(dev:&mut D,
    matcher:impl FnMut(&[u8],[u8;6])->bool)->Result<(),Error>{
    wait_for_limit(dev,MAX_POLL,matcher)
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
    // R114: the real QEMU NIC UDP receive route is now generation-bound
    // through the same registry/endpoint grant and validated owned ingress.
    // Synthetic PID1/epoch1 are trusted test fixtures, NOT user-supplied IDs.
    let mut udp_endpoints = EpochUdpEndpoints::<1, 2>::new();
    // R120: this physical QEMU virtio-net POLL path is owned by the same
    // incarnation-aware coordinator that handles atomic process teardown.
    // Do not treat a bare UDP grant as proof that the process remains alive.
    let mut live_owners =
        crate::process_retirement_txn::ProcessRetirement::<1,1,4>::new();
    let network_owner=live_owners.start(1).map_err(|_|Error::Unexpected)?;
    live_owners.bind_live_udp(&mut udp_endpoints,network_owner,CLIENT_PORT)
        .map_err(|_| Error::Unexpected)?;
    let route_owner=crate::batch5_session::Identity{pid:1,generation:1};
    let mut routes=crate::agent_06_ipv4_route::OwnerRoutes::<1>::new();
    routes.bind_kernel(route_owner,1).map_err(|_|Error::Unexpected)?;
    routes.publish(route_owner,1,&[crate::agent_06_ipv4_route::Route{
        network:0x0a07_0000,prefix:24,next_hop:0,
    }]).map_err(|_|Error::Unexpected)?;
    // R114 link-failure acceptance: two separately versioned route
    // candidates. The QEMU socket peer deliberately drops the first UDP
    // response to model an unreachable primary link. The trusted controller
    // withdraws primary epoch 1, verifies backup selection, then retransmits
    // through the actual virtio NIC with the same authenticated socket owner.
    //
    // This tests route/policy failover and real Ethernet traffic, but the
    // backup is a LOGICAL route over the SAME virtual NIC, not a second port.
    let mut alternatives=crate::agent_06_ipv4_route::FailoverRoutes::<2>::new();
    use crate::agent_06_ipv4_route::{RouteCandidate,Route};
    let r=Route{network:0x0a07_0000,prefix:24,next_hop:0};
    // Trusted QEMU logical test clock only. Production must source now
    // from the kernel monotonic timer, never ring-3 or packet headers.
    let initial_clock=100u64;
    alternatives.add_dhcp_lease(RouteCandidate{
        route:r,metric:1,link:1,epoch:1,healthy:true
    },initial_clock,120).map_err(|_|Error::Unexpected)?;
    alternatives.add(RouteCandidate{
        route:r,metric:10,link:2,epoch:1,healthy:true
    }).map_err(|_|Error::Unexpected)?;
    let destination=u32::from_be_bytes(PEER_IP);
    // The first outbound UDP frame uses a time-valid DHCP primary receipt.
    // Carry that receipt across the QEMU peer's forced missing reply; it
    // MUST be invalid exactly at the expiry deadline before retransmission.
    let first_receipt=alternatives.receipt_at(destination,initial_clock+1)
        .ok_or(Error::Unexpected)?;
    if first_receipt.selected.link!=1||
       alternatives.resolve(destination).map(|r|r.link)!=Some(1){
        return Err(Error::Unexpected)
    }
    transmit_leased_ipv4_checked_clock(dev,&routes,&alternatives,route_owner,1,
        PEER_IP,||initial_clock+1,
        |b,mac|udp_frame(b,mac,PEER_UDP_PORT,UDP_REQ,0x2803))?;
    let ingress=|revision:crate::process_retirement_txn::IngressRevision,
                  frame:&[u8],mac:[u8;6]|{
        parse_udp(frame,mac,PEER_UDP_PORT).is_ok_and(|data|data==UDP_RESP)
            && live_owners.ingest_polled_live_udp_fenced(
                &mut udp_endpoints,revision,
                network_owner,CLIENT_PORT,frame,mac).is_ok()
    };
    // Do not treat malformed network replies as successful failover evidence.
    // In the QEMU test the peer confirms packet reception but blackholes
    // this reply. Only a bounded timeout may quarantine the primary path.
    match wait_for_limit_fenced(
        dev,300_000,
        ||live_owners.capture_polled_ingress_revision(),ingress
    ){
        Err(Error::Timeout)=>{
            // R116 physical QEMU packet acceptance: the original time
            // receipt must have expired (not merely have timed out waiting
            // for a reply). The static secondary route must be valid AT the
            // expiry deadline before any new Ethernet frame is submitted.
            let deadline=initial_clock+20;
            if alternatives.receipt_is_current_at(destination,deadline,
                first_receipt) ||
                alternatives.receipt_at(destination,deadline)
                    .is_none_or(|r|r.selected.link!=2){
                return Err(Error::Unexpected);
            }
            alternatives.begin_link_reset(1,1,2)
                .map_err(|_|Error::Unexpected)?;
            let backup=alternatives.resolve_at(destination,deadline)
                .ok_or(Error::Unexpected)?;
            if backup.link!=2||backup.epoch!=1||!backup.healthy{
                return Err(Error::Unexpected);
            }
            stage(b"IndianOS: R114 QEMU primary UDP path timed out and backup route selected\n\0");
            let retry_receipt=transmit_failover_ipv4_checked_clock(
                dev,&routes,&alternatives,route_owner,1,PEER_IP,
                first_receipt,initial_clock+1,||initial_clock+20,
                |b,mac|udp_frame(b,mac,PEER_UDP_PORT,UDP_REQ,0x2808))?;
            if retry_receipt.selected.link!=2||
               retry_receipt.selected.epoch!=backup.epoch {
                return Err(Error::Unexpected);
            }
            wait_for_limit_fenced(
                dev,MAX_POLL,
                ||live_owners.capture_polled_ingress_revision(),
                |revision,frame,mac|{
                    parse_udp(frame,mac,PEER_UDP_PORT)
                        .is_ok_and(|data|data==UDP_RESP)
                        && live_owners.ingest_polled_live_udp_fenced(
                            &mut udp_endpoints,revision,
                            network_owner,CLIENT_PORT,frame,mac).is_ok()
                }
            )?;
            stage(b"IndianOS: R116 QEMU DHCP lease expiry blocked primary UDP resend and backup succeeded\n\0");
        },
        // The failure-injection acceptance requires the primary to become
        // unreachable. A response before failover is not success.
        _=>return Err(Error::Unexpected),
    }
    let mut udp_reply = [0u8; R033_UDP_BYTES];
    let bytes = live_owners.receive_live_udp(&mut udp_endpoints,
        network_owner,CLIENT_PORT,&mut udp_reply)
        .map_err(|_| Error::Unexpected)?;
    if &udp_reply[..bytes] != UDP_RESP { return Err(Error::Unexpected); }
    stage(b"IndianOS: R028 IPv4 UDP echo PASSED\n\0");
    transmit_leased_ipv4_checked_clock(dev,&routes,&alternatives,route_owner,1,
        PEER_IP,||initial_clock+21,dns_query)?;
    wait_for_limit_fenced(
        dev,MAX_POLL,
        ||live_owners.capture_polled_ingress_revision(),
        |revision,frame,mac|{
            valid_dns(frame,mac) &&
                live_owners.ingest_polled_live_udp_fenced(
                    &mut udp_endpoints,revision,
                    network_owner,CLIENT_PORT,frame,mac).is_ok()
        }
    )?;
    let mut dns_reply=[0u8;R033_UDP_BYTES];
    let dns_bytes=live_owners.receive_live_udp(&mut udp_endpoints,
        network_owner,CLIENT_PORT,&mut dns_reply)
        .map_err(|_|Error::Unexpected)?;
    if dns_bytes<12 || dns_reply[..2]!=[0x28,0x28] ||
       dns_reply[2]&0x80==0{return Err(Error::Unexpected);}
    let retired=live_owners.retire_live_exited_batch(
        &mut udp_endpoints,&[network_owner]).map_err(|_|Error::Unexpected)?;
    if retired.udp_ports_released!=1 || retired.notices_enqueued!=1 ||
        live_owners.active_generation(network_owner.pid).is_some(){
        return Err(Error::Unexpected);
    }
    routes.retire(route_owner,1).map_err(|_|Error::Unexpected)?;
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
#[derive(Clone)]
pub struct UdpEndpoints<const PORTS: usize, const QUEUE: usize> {
    ports: [Option<PortOwner>; PORTS],
    queue: [Option<QueuedUdp>; QUEUE],
    len: usize,
    // Default maintains legacy admission; trusted kernel may only narrow.
    per_owner_quota: usize,
}
impl<const PORTS: usize, const QUEUE: usize> UdpEndpoints<PORTS, QUEUE> {
    pub const fn new() -> Self {
        Self { ports: [None; PORTS], queue: [None; QUEUE], len: 0,
            per_owner_quota: QUEUE }
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
        // A UDP datagram can have an empty payload. It still consumes one
        // queue slot and must be observable as Ok(0), not a missing message.
        if data.len() > R033_UDP_BYTES { return Err(SocketError::Invalid); }
        let pid = self.ports.iter().flatten().find(|p| p.port == dest_port)
            .map(|p| p.pid).ok_or(SocketError::Denied)?;
        if self.len == QUEUE || self.queued_for(pid) >= self.per_owner_quota {
            return Err(SocketError::Full);
        }
        let mut bytes = [0; R033_UDP_BYTES];
        bytes[..data.len()].copy_from_slice(data);
        self.queue[self.len] = Some(QueuedUdp {
            destination_pid: pid, destination_port: dest_port, len: data.len(), bytes,
        });
        self.len += 1;
        Ok(())
    }
    /// Consume exactly one datagram. Ok(0) is an empty datagram; Empty means
    /// none is queued. A short destination never consumes the datagram.
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
    /// Close one authenticated binding, discarding only its unread datagrams.
    /// Other bindings of the same owner remain live. Rebinding cannot inherit
    /// data delivered before this close, even if the new owner has another PID.
    pub fn close(&mut self, pid: u32, port: u16) -> Result<(), SocketError> {
        let index = self.ports.iter().position(|p|
            p.is_some_and(|entry| entry.pid == pid && entry.port == port)
        ).ok_or(SocketError::Denied)?;
        self.ports[index] = None;
        self.discard_queued(pid, Some(port));
        Ok(())
    }
    fn discard_queued(&mut self, pid: u32, port: Option<u16>) {
        let original = self.len;
        let mut kept = 0;
        for index in 0..original {
            let item = self.queue[index];
            let remove = item.is_some_and(|entry|
                entry.destination_pid == pid &&
                port.is_none_or(|port| entry.destination_port == port));
            if !remove { self.queue[kept] = item; kept += 1; }
        }
        self.queue[kept..original].fill(None);
        self.len = kept;
    }
    pub fn close_all(&mut self, pid: u32) {
        if pid == 0 { return; }
        for p in &mut self.ports {
            if p.is_some_and(|entry| entry.pid == pid) { *p = None; }
        }
        self.discard_queued(pid, None);
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
        assert_eq!(t.enqueue(6000, &[0;R033_UDP_BYTES+1]), Err(SocketError::Invalid));
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

/// Parse AND validate a NIC-delivered UDP packet before using its destination
/// port to locate a privileged kernel owner. A packet may never select its PID
/// or generation; those come only from the live registry. This parser never
/// touches a socket or mutates the packet queue. No hardware IRQ is enabled.
pub(crate) fn authenticated_udp_frame<'a>(
    frame:&'a [u8],local_mac:[u8;6],
)->Result<(u16,&'a [u8]),IngressError>{
    let ip=validate_ipv4(frame,local_mac).map_err(|e|match e{
        Error::Checksum=>IngressError::BadChecksum,
        _=>IngressError::Malformed,
    })?;
    if ip.proto!=17||ip.body.len()<8{return Err(IngressError::Malformed);}
    let udp=ip.body;
    let source=be16(&udp[..2]);
    let destination=be16(&udp[2..4]);
    let length=be16(&udp[4..6]) as usize;
    if source==0||destination==0||length!=udp.len()||
        be16(&udp[6..8])==0{return Err(IngressError::Malformed);}
    if transport_sum(&PEER_IP,&LOCAL_IP,17,udp)!=0 {
        return Err(IngressError::BadChecksum);
    }
    Ok((destination,&udp[8..]))
}

impl<const PORTS: usize, const QUEUE: usize> UdpEndpoints<PORTS, QUEUE> {
    /// Single-peer QEMU ingress, not a general-purpose UDP socket service.
    /// The queue is not modified if any framing or checksum check fails.
    pub fn ingest_ethernet_frame(
        &mut self, frame: &[u8], local_mac: [u8; 6],
    ) -> Result<(), IngressError> {
        let (destination_port,payload)=authenticated_udp_frame(frame,local_mac)?;
        self.enqueue(destination_port,payload)
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
    fn empty_wire_datagrams_are_real_fifo_messages_not_empty_queue() {
        let mut endpoints = UdpEndpoints::<1, 3>::new();
        endpoints.bind(10,6000).unwrap();
        for data in [&[][..],&b"payload"[..],&[][..]] {
            let (frame,n)=peer_frame(6000,data);
            endpoints.ingest_ethernet_frame(&frame[..n],LOCAL_MAC).unwrap();
        }
        let (frame,n)=peer_frame(6000,&[]);
        assert_eq!(endpoints.ingest_ethernet_frame(&frame[..n],LOCAL_MAC),
            Err(IngressError::Endpoint(SocketError::Full)));
        assert_eq!(endpoints.receive(20,6000,&mut []),Err(SocketError::Denied));
        assert_eq!(endpoints.receive(10,6000,&mut []),Ok(0));
        assert_eq!(endpoints.receive(10,6000,&mut []),Err(SocketError::TooShort));
        let mut out=[0xa5;8];
        assert_eq!(endpoints.receive(10,6000,&mut out),Ok(7));
        assert_eq!(&out[..7],b"payload");assert_eq!(out[7],0xa5);
        assert_eq!(endpoints.receive(10,6000,&mut out),Ok(0));
        assert_eq!(&out[..7],b"payload");
        assert_eq!(endpoints.receive(10,6000,&mut out),Err(SocketError::Empty));
    }

    #[test]
    fn closing_one_port_purges_its_datagrams_and_preserves_other_bindings() {
        let mut endpoints=UdpEndpoints::<3,5>::new();
        endpoints.bind(10,6000).unwrap();endpoints.bind(10,6001).unwrap();
        endpoints.bind(20,7000).unwrap();
        for (port,data) in [(6000,&b"drop"[..]),(6001,&b"first"[..]),
            (7000,&b"peer"[..]),(6000,&[][..]),(6001,&b"last"[..])] {
            let (frame,n)=peer_frame(port,data);
            endpoints.ingest_ethernet_frame(&frame[..n],LOCAL_MAC).unwrap();
        }
        assert_eq!(endpoints.close(20,6000),Err(SocketError::Denied));
        assert_eq!(endpoints.len,5);
        assert_eq!(endpoints.close(10,6000),Ok(()));
        assert_eq!(endpoints.close(10,6000),Err(SocketError::Denied));
        endpoints.bind(30,6000).unwrap();
        let mut out=[0u8;8];
        assert_eq!(endpoints.receive(30,6000,&mut out),Err(SocketError::Empty));
        for (pid,port,data) in [(10,6001,&b"first"[..]),(20,7000,&b"peer"[..]),
            (10,6001,&b"last"[..])] {
            assert_eq!(endpoints.receive(pid,port,&mut out),Ok(data.len()));
            assert_eq!(&out[..data.len()],data);
        }
        assert_eq!(endpoints.len,0);
        assert!(endpoints.queue.iter().all(Option::is_none));
    }

    #[test]
    fn epoch_close_denies_stale_owner_and_releases_only_last_binding_identity() {
        let mut endpoints=EpochUdpEndpoints::<3,3>::new();
        endpoints.bind(10,1,6000).unwrap();endpoints.bind(10,1,6001).unwrap();
        endpoints.bind(20,2,7000).unwrap();
        for (port,data) in [(6000,&[][..]),(6001,&b"live"[..]),(7000,&b"peer"[..])] {
            let (frame,n)=peer_frame(port,data);
            endpoints.ingest_ethernet_frame(&frame[..n],LOCAL_MAC).unwrap();
        }
        assert_eq!(endpoints.close(10,2,6000),Err(SocketError::Denied));
        assert_eq!(endpoints.close(20,2,6000),Err(SocketError::Denied));
        assert_eq!(endpoints.receive(10,1,6000,&mut []),Ok(0));
        assert_eq!(endpoints.close(10,1,6000),Ok(()));
        assert_eq!(endpoints.bind(10,2,6000),Err(SocketError::Denied));
        let mut out=[0u8;8];
        assert_eq!(endpoints.receive(10,1,6001,&mut out),Ok(4));
        assert_eq!(&out[..4],b"live");
        assert_eq!(endpoints.close(10,1,6001),Ok(()));
        endpoints.bind(10,2,6000).unwrap();
        assert_eq!(endpoints.close(10,1,6000),Err(SocketError::Denied));
        assert_eq!(endpoints.receive(10,2,6000,&mut out),Err(SocketError::Empty));
        assert_eq!(endpoints.receive(20,2,7000,&mut out),Ok(4));
        assert_eq!(&out[..4],b"peer");
        endpoints.retire(10,2).unwrap();
        assert_eq!(endpoints.close(10,2,6000),Err(SocketError::Denied));
        assert!(endpoints.endpoints.owns(20,7000));
    }

    #[test]
    fn zero_capacity_still_checks_empty_datagram_ownership_and_capacity() {
        let mut no_queue=UdpEndpoints::<1,0>::new();
        no_queue.bind(10,6000).unwrap();
        let (frame,n)=peer_frame(6000,&[]);
        assert_eq!(no_queue.ingest_ethernet_frame(&frame[..n],LOCAL_MAC),
            Err(IngressError::Endpoint(SocketError::Full)));
        assert_eq!(no_queue.receive(10,6000,&mut []),Err(SocketError::Empty));
        no_queue.close(10,6000).unwrap();
        let mut no_ports=UdpEndpoints::<0,1>::new();
        assert_eq!(no_ports.bind(10,6000),Err(SocketError::Full));
        assert_eq!(no_ports.ingest_ethernet_frame(&frame[..n],LOCAL_MAC),
            Err(IngressError::Endpoint(SocketError::Denied)));
        assert_eq!(no_ports.close(10,6000),Err(SocketError::Denied));
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
#[derive(Clone)]
pub struct EpochUdpEndpoints<const PORTS: usize, const QUEUE: usize> {
    endpoints: UdpEndpoints<PORTS, QUEUE>,
    owners: [Option<(u32,u64)>; PORTS],
}
impl<const PORTS: usize, const QUEUE: usize> EpochUdpEndpoints<PORTS, QUEUE> {
    pub const fn new() -> Self {
        Self { endpoints: UdpEndpoints::new(), owners: [None; PORTS] }
    }

    /// R120: prove that the actual socket/packet queue ownership has exactly
    /// the same live port grants as the kernel generation registry. This is
    /// required before a cross-table transaction may publish mutations.
    /// The surrounding kernel caller must exclusively own both tables.
    pub fn matches_registry<const N:usize>(
        &self,registry:&crate::udp_owner_epoch::Registry<N>
    )->bool{
        let ports=self.endpoints.ports.iter().flatten().count();
        if ports!=registry.active_count(){return false;}
        self.endpoints.ports.iter().flatten().all(|binding|{
            self.owners.iter().flatten().any(|&(pid,generation)|
                pid==binding.pid &&
                registry.owner_of(binding.port)==Some(
                    crate::udp_owner_epoch::Grant{
                        pid,generation,port:binding.port
                    }
                )
            )
        })
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
    /// Retire one port without revoking another port of this generation.
    /// The owner record is released only when its final binding closes.
    pub fn close(&mut self, pid: u32, generation: u64, port: u16)
        -> Result<(),SocketError> {
        if !self.authenticated(pid,generation) { return Err(SocketError::Denied); }
        self.endpoints.close(pid,port)?;
        if !self.endpoints.ports.iter().flatten().any(|entry|entry.pid==pid) {
            for owner in &mut self.owners {
                if *owner==Some((pid,generation)) { *owner=None; }
            }
        }
        Ok(())
    }
    pub fn ingest_ethernet_frame(&mut self, frame: &[u8], mac: [u8;6])
        -> Result<(),IngressError> {
        self.endpoints.ingest_ethernet_frame(frame,mac)
    }
    /// Kernel-authenticated local datagram delivery between ring-3 peers.
    /// This is a bounded in-kernel UDP-like loopback, NOT a physical NIC.
    pub fn send_local(&mut self,pid:u32,generation:u64,source:u16,
        destination:u16,data:&[u8])->Result<(),SocketError>{
        if !self.authenticated(pid,generation)||
           !self.endpoints.owns(pid,source){return Err(SocketError::Denied);}
        if data.is_empty()||data.len()>R033_UDP_BYTES{return Err(SocketError::Invalid);}
        self.endpoints.enqueue(destination,data)
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
mod r114_ring3_local_udp_tests {
 use super::*;
 #[test]fn spoofed_sender_cannot_send_or_receive_a_peer_datagram(){
  let mut bus=EpochUdpEndpoints::<2,4>::new();
  bus.bind(1,3,5501).unwrap();bus.bind(2,9,5502).unwrap();
  assert_eq!(bus.send_local(1,3,5501,5502,b"ping"),Ok(()));
  assert_eq!(bus.send_local(2,9,5501,5502,b"spoof"),Err(SocketError::Denied));
  assert_eq!(bus.send_local(1,4,5501,5502,b"stale"),Err(SocketError::Denied));
  assert_eq!(bus.receive(2,8,5502,&mut [0u8;8]),Err(SocketError::Denied));
  let mut rx=[0u8;8];
  assert_eq!(bus.receive(2,9,5502,&mut rx),Ok(4));
  assert_eq!(&rx[..4],b"ping");
  assert_eq!(bus.receive(1,3,5501,&mut rx),Err(SocketError::Empty));
 }
 #[test]fn retired_pid_cannot_replay_or_inherit_packets(){
  let mut bus=EpochUdpEndpoints::<2,4>::new();
  bus.bind(1,1,5501).unwrap();bus.bind(2,1,5502).unwrap();
  bus.send_local(1,1,5501,5502,b"old").unwrap();
  bus.retire(2,1).unwrap();
  assert_eq!(bus.receive(2,1,5502,&mut [0u8;8]),Err(SocketError::Denied));
  bus.bind(2,2,5502).unwrap();
  assert_eq!(bus.receive(2,2,5502,&mut [0u8;8]),Err(SocketError::Empty));
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


// R114: network ingress and PID-generation policy is wired into the existing
// validated Ethernet/UDP queue. The trusted caller supplies process identity;
// no generic ring-3 syscall or physical NIC is enabled here.
impl<const PORTS: usize, const QUEUE: usize> UdpEndpoints<PORTS, QUEUE> {
    pub fn queued_for(&self, pid:u32)->usize {
        self.queue[..self.len].iter().flatten().filter(|d|d.destination_pid==pid).count()
    }
    /// Fail closed under a flood by one endpoint; cannot raise quotas again.
    pub fn narrow_owner_queue_quota(&mut self,quota:usize)
        ->Result<(),SocketError> {
        if quota==0 || quota>self.per_owner_quota {return Err(SocketError::Invalid);}
        self.per_owner_quota=quota;
        Ok(())
    }
}
impl<const PORTS:usize,const QUEUE:usize> EpochUdpEndpoints<PORTS,QUEUE> {
    /// Bind the same exact incarnation in both the generation registry and
    /// the real network ingress queue; rollback registry if queue admission fails.
    pub fn bind_registered<const N:usize>(
        &mut self,
        registry:&mut crate::udp_owner_epoch::Registry<N>,
        pid:u32,generation:u64,port:u16)->Result<(),SocketError>{
        registry.bind(pid,generation,port).map_err(|_|SocketError::Denied)?;
        if let Err(error)=self.bind(pid,generation,port) {
            let _=registry.unbind(pid,generation,port);
            return Err(error);
        }
        Ok(())
    }
    /// Atomic authorization preflight BEFORE packet parser or queue mutation.
    /// Full IPv4/UDP checksum/length validation remains in ingress.
    pub fn ingest_owned_frame(&mut self,pid:u32,generation:u64,port:u16,
         frame:&[u8],mac:[u8;6])->Result<(),IngressError>{
        if !self.authenticated(pid,generation)||!self.endpoints.owns(pid,port){
            return Err(IngressError::Endpoint(SocketError::Denied));
        }
        let (destination,_payload)=authenticated_udp_frame(frame,mac)?;
        if destination!=port{
            return Err(IngressError::Endpoint(SocketError::Denied));
        }
        self.endpoints.ingest_ethernet_frame(frame,mac)
    }
    /// R115: actual NIC ingress must check BOTH live port tables before
    /// parsing or queueing the peer frame. The registry and endpoints must
    /// be owned by the single-BSP kernel network supervisor.
    pub fn ingest_registered_frame<const N:usize>(
        &mut self,
        registry:&crate::udp_owner_epoch::Registry<N>,
        pid:u32,generation:u64,port:u16,
        frame:&[u8],mac:[u8;6],
    )->Result<(),IngressError>{
        registry.authorize(pid,generation,port)
            .map_err(|_|IngressError::Endpoint(SocketError::Denied))?;
        self.ingest_owned_frame(pid,generation,port,frame,mac)
    }
    pub fn pending_owned(&self,pid:u32,generation:u64)
        ->Result<usize,SocketError>{
        if !self.authenticated(pid,generation){return Err(SocketError::Denied);}
        Ok(self.endpoints.queued_for(pid))
    }
    /// Trusted exact-incarnation shutdown clears both registries and queued
    /// datagrams. A delayed stale generation cannot revoke a replacement.
    pub fn retire_registered<const N:usize>(
        &mut self,registry:&mut crate::udp_owner_epoch::Registry<N>,
        pid:u32,generation:u64)->Result<usize,SocketError>{
        if !self.authenticated(pid,generation){return Err(SocketError::Denied);}
        self.retire(pid,generation)?;
        registry.revoke_owner(pid,generation).map_err(|_|SocketError::Invalid)
    }
    /// A scheduler-authenticated task exit must retire its exact generation
    /// even if its socket-side entry was already removed by a prior close.
    /// This prevents an orphaned grant from denying a new task's port bind.
    /// Never remove a peer's port or a replacement PID incarnation.
    pub fn retire_exact_kernel<const N:usize>(
        &mut self,registry:&mut crate::udp_owner_epoch::Registry<N>,
        pid:u32,generation:u64)->Result<usize,SocketError>{
        if pid==0||generation==0{return Err(SocketError::Invalid);}
        // A newer PID incarnation in the registry must not be disturbed by
        // old, delayed scheduler retirement. Validate before mutating sockets.
        if registry.has_foreign_generation(pid,generation) ||
           self.owners.iter().flatten().any(|&(known,epoch)|
              known==pid&&epoch!=generation){
            return Err(SocketError::Denied);
        }
        if self.authenticated(pid,generation){
            self.retire(pid,generation)?;
        }
        registry.revoke_owner(pid,generation)
            .map_err(|_|SocketError::Invalid)
    }
    /// Serialized scheduler batch: validate every requested process epoch
    /// across both the NIC-facing socket table and the policy registry
    /// before retiring any entry. A stale PID, duplicate or malformed batch
    /// cannot leave a peer half-retired. No NIC I/O or cross-CPU lock is used.
    pub fn retire_exact_batch_kernel<const N:usize>(
        &mut self,registry:&mut crate::udp_owner_epoch::Registry<N>,
        identities:&[crate::batch5_session::Identity]
    )->Result<usize,SocketError>{
        for (index,id) in identities.iter().enumerate(){
            if id.pid==0||id.generation==0{
                return Err(SocketError::Invalid);
            }
            if identities[..index].iter().any(|old|old.pid==id.pid){
                return Err(SocketError::Duplicate);
            }
            if self.owners.iter().flatten().any(|&(pid,generation)|
                pid==id.pid && generation!=id.generation) ||
                registry.has_foreign_generation(id.pid,id.generation){
                return Err(SocketError::Denied);
            }
        }
        for id in identities{
            // All potential errors were preflighted on the same single-BSP
            // exclusive tables. Exact retire tolerates orphaned registry-only
            // entries and never touches a peer's queued datagrams.
            self.retire_exact_kernel(registry,id.pid,id.generation)?;
        }
        Ok(identities.len())
    }
    pub fn narrow_queue_quota_kernel(&mut self,quota:usize)->Result<(),SocketError>{
        self.endpoints.narrow_owner_queue_quota(quota)
    }
}
#[cfg(test)]
mod r114_live_udp_ownership {
    use super::*;
    const MAC:[u8;6]=[0x52,0x54,0,0x12,0x34,0x56];
    fn frame(port:u16)->([u8;FRAME_CAP],usize){
        let mut f=[0u8;FRAME_CAP];
        ethernet_header(&mut f,MAC,PEER_MAC,0x0800);
        ipv4_header(&mut f[14..34],17,12,0x1234);
        f[26..30].copy_from_slice(&PEER_IP);
        f[30..34].copy_from_slice(&LOCAL_IP);
        put16(&mut f,24,0);
        let h=checksum(&f[14..34]);
        put16(&mut f,24,h);
        let u=&mut f[34..46];
        put16(u,0,PEER_UDP_PORT); put16(u,2,port);put16(u,4,12);
        u[8..12].copy_from_slice(b"live");
        let c=transport_sum(&PEER_IP,&LOCAL_IP,17,u);
        put16(u,6,if c==0{0xffff}else{c});
        (f,46)
    }
    #[test]
    fn registry_bind_matches_real_ingress_and_epoch_retirement(){
        let mut endpoints=EpochUdpEndpoints::<3,4>::new();
        let mut grants=crate::udp_owner_epoch::Registry::<3>::new();
        endpoints.bind_registered(&mut grants,7,1,5000).unwrap();
        endpoints.bind_registered(&mut grants,8,1,5001).unwrap();
        let (f,len)=frame(5000);
        assert_eq!(endpoints.ingest_owned_frame(7,2,5000,&f[..len],MAC),
            Err(IngressError::Endpoint(SocketError::Denied)));
        assert_eq!(endpoints.ingest_owned_frame(8,1,5000,&f[..len],MAC),
            Err(IngressError::Endpoint(SocketError::Denied)));
        assert_eq!(endpoints.ingest_owned_frame(7,1,5000,&f[..len],MAC),Ok(()));
        assert_eq!(endpoints.pending_owned(7,1),Ok(1));
        assert_eq!(endpoints.retire_registered(&mut grants,7,2),Err(SocketError::Denied));
        assert_eq!(endpoints.retire_registered(&mut grants,7,1),Ok(1));
        assert_eq!(grants.authorize(7,1,5000),Err(crate::udp_owner_epoch::Error::Denied));
        endpoints.bind_registered(&mut grants,7,2,5000).unwrap();
        assert_eq!(endpoints.pending_owned(7,2),Ok(0));
        assert_eq!(endpoints.ingest_owned_frame(7,1,5000,&f[..len],MAC),
            Err(IngressError::Endpoint(SocketError::Denied)));
    }
    #[test]
    fn input_denial_and_checksum_fault_never_publish_udp(){
        let mut endpoints=EpochUdpEndpoints::<2,3>::new();
        endpoints.bind(7,1,5000).unwrap();
        let (f,len)=frame(5001);
        assert_eq!(endpoints.ingest_owned_frame(7,1,5000,&f[..len],MAC),
            Err(IngressError::Endpoint(SocketError::Denied)));
        let (mut f,len)=frame(5000);
        f[24]^=1;
        assert_eq!(endpoints.ingest_owned_frame(7,1,5000,&f[..len],MAC),
            Err(IngressError::BadChecksum));
        assert_eq!(endpoints.pending_owned(7,1),Ok(0));
    }
    #[test]
    fn owner_specific_quota_prevents_flood_without_blocking_peer(){
        let mut endpoints=EpochUdpEndpoints::<2,4>::new();
        endpoints.bind(7,1,5000).unwrap();
        endpoints.bind(8,1,5001).unwrap();
        endpoints.narrow_queue_quota_kernel(1).unwrap();
        assert_eq!(endpoints.narrow_queue_quota_kernel(4),Err(SocketError::Invalid));
        let (f,n)=frame(5000);
        endpoints.ingest_owned_frame(7,1,5000,&f[..n],MAC).unwrap();
        assert_eq!(endpoints.ingest_owned_frame(7,1,5000,&f[..n],MAC),
            Err(IngressError::Endpoint(SocketError::Full)));
        let (g,m)=frame(5001);
        endpoints.ingest_owned_frame(8,1,5001,&g[..m],MAC).unwrap();
        assert_eq!(endpoints.pending_owned(7,1),Ok(1));
        assert_eq!(endpoints.pending_owned(8,1),Ok(1));
        let mut out=[0u8;8];
        assert_eq!(endpoints.receive(8,1,5001,&mut out),Ok(4));
        assert_eq!(&out[..4],b"live");
    }
    #[test]
    fn registry_rollback_when_live_socket_capacity_exhausted(){
        let mut endpoints=EpochUdpEndpoints::<1,2>::new();
        let mut grants=crate::udp_owner_epoch::Registry::<2>::new();
        endpoints.bind_registered(&mut grants,7,1,5000).unwrap();
        assert_eq!(endpoints.bind_registered(&mut grants,8,1,6000),
            Err(SocketError::Full));
        assert_eq!(grants.owner_of(6000),None);
        assert_eq!(grants.owner_of(5000),Some(crate::udp_owner_epoch::Grant{
            pid:7,generation:1,port:5000,
        }));
    }
}


// R114: network QEMU demo's actual EthernetDevice send can be gated by an
// exact authenticated owner, route snapshot and unchanged packet destination.
// No generic ARP cache, wireless hardware or userspace routing syscall.
fn transmit_owned_ipv4_guarded<D:EthernetDevice,const N:usize>(
    dev:&mut D,routes:&crate::agent_06_ipv4_route::OwnerRoutes<N>,
    owner:crate::batch5_session::Identity,epoch:u64,
    destination:[u8;4],
    dispatch_guard:impl FnOnce()->Result<(),Error>,
    constructor:impl FnOnce(&mut [u8],[u8;6])->usize,
)->Result<(),Error>{
    let route=routes.select(owner,epoch,u32::from_be_bytes(destination))
        .map_err(|_|Error::Unexpected)?.ok_or(Error::Unexpected)?;
    // Only direct/on-link route or the already-known fixed QEMU peer can be
    // transmitted here. A new gateway requires authenticated ARP resolution.
    if route.next_hop!=0&&route.next_hop!=u32::from_be_bytes(PEER_IP){
        return Err(Error::Invalid)
    }
    let mut frame=[0u8;FRAME_CAP];
    let local_mac=dev.mac();
    let len=constructor(&mut frame,local_mac);
    // This is the kernel-owned QEMU peer route: do not allow an otherwise
    // authenticated IPv4 frame to target a different Ethernet recipient,
    // use a forged source MAC, smuggle trailing bytes, or use fragments.
    if len<34 || len>FRAME_CAP || frame[..6]!=PEER_MAC ||
        frame[6..12]!=local_mac || frame[12..14]!=[0x08,0] {
        return Err(Error::Invalid);
    }
    let ip=&frame[14..len];
    if ip[0]!=0x45||ip[8]==0 || ip[12..16]!=LOCAL_IP ||
        ip[16..20]!=destination ||
        be16(&ip[6..8])&0x3fff!=0 ||
        be16(&ip[2..4]) as usize!=ip.len() ||
        checksum(&ip[..20])!=0 {
        return Err(Error::Invalid)
    }
    // The authorization can be rechecked AFTER frame construction and
    // immediately before the NIC send, never merely before serialization.
    dispatch_guard()?;
    dev.send(&frame[..len])
}
pub fn transmit_owned_ipv4<D:EthernetDevice,const N:usize>(
    dev:&mut D,routes:&crate::agent_06_ipv4_route::OwnerRoutes<N>,
    owner:crate::batch5_session::Identity,epoch:u64,
    destination:[u8;4],
    constructor:impl FnOnce(&mut [u8],[u8;6])->usize,
)->Result<(),Error>{
    transmit_owned_ipv4_guarded(dev,routes,owner,epoch,destination,
        ||Ok(()),constructor)
}
#[cfg(test)]
mod r114_owned_route_and_packet_regression {
    use super::*;
    use crate::agent_06_ipv4_route::{OwnerRoutes,Route};
    use crate::batch5_session::Identity;
    struct Fake { sends:u32 }
    impl EthernetDevice for Fake {
        fn mac(&self)->[u8;6]{[0x52,0x54,0,0x12,0x34,0x56]}
        fn send(&mut self,_data:&[u8])->Result<(),Error>{
            self.sends+=1;Ok(())
        }
        fn poll(&mut self,_out:&mut[u8])->Result<Option<usize>,Error>{
            Ok(None)
        }
    }
    fn policy()->(OwnerRoutes<1>,Identity){
        let id=Identity{pid:1,generation:1};
        let mut p=OwnerRoutes::<1>::new();
        p.bind_kernel(id,1).unwrap();
        p.publish(id,1,&[Route{network:0x0a07_0000,prefix:24,next_hop:0}]).unwrap();
        (p,id)
    }
    #[test]
    fn protected_send_requires_live_owner_and_matching_packet_destination(){
        let mut nic=Fake{sends:0};
        let (policy,owner)=policy();
        assert_eq!(transmit_owned_ipv4(&mut nic,&policy,owner,1,PEER_IP,
            |b,mac|udp_frame(b,mac,PEER_UDP_PORT,b"live",10)),Ok(()));
        assert_eq!(nic.sends,1);
        assert_eq!(transmit_owned_ipv4(&mut nic,&policy,
            Identity{pid:owner.pid,generation:2},1,PEER_IP,
            |b,mac|udp_frame(b,mac,PEER_UDP_PORT,b"live",11)),Err(Error::Unexpected));
        assert_eq!(transmit_owned_ipv4(&mut nic,&policy,owner,1,[8,8,8,8],
            |b,mac|udp_frame(b,mac,PEER_UDP_PORT,b"live",12)),Err(Error::Unexpected));
        assert_eq!(nic.sends,1);
    }
    #[test]
    fn valid_route_cannot_launder_mismatched_header_or_bad_checksum(){
        let mut nic=Fake{sends:0};
        let (policy,owner)=policy();
        assert_eq!(transmit_owned_ipv4(&mut nic,&policy,owner,1,PEER_IP,
            |b,mac|{
                let n=udp_frame(b,mac,PEER_UDP_PORT,b"live",10);
                b[30]=8;n
            }),Err(Error::Invalid));
        assert_eq!(transmit_owned_ipv4(&mut nic,&policy,owner,1,PEER_IP,
            |b,mac|{
                let n=udp_frame(b,mac,PEER_UDP_PORT,b"live",10);
                b[24]^=1;n
            }),Err(Error::Invalid));
        assert_eq!(nic.sends,0);
    }
    #[test]
    fn fragment_and_short_frame_fail_before_owned_udp_queue_mutation(){
        let mut owner=EpochUdpEndpoints::<1,3>::new();
        owner.bind(1,1,CLIENT_PORT).unwrap();
        let mut frame=[0u8;FRAME_CAP];
        ethernet_header(&mut frame,[0x52,0x54,0,0x12,0x34,0x56],PEER_MAC,0x0800);
        ipv4_header(&mut frame[14..34],17,12,33);
        frame[26..30].copy_from_slice(&PEER_IP);
        frame[30..34].copy_from_slice(&LOCAL_IP);
        frame[20]=0x20; // more fragments flag
        put16(&mut frame,24,0);
        let checksum=checksum(&frame[14..34]);
        put16(&mut frame,24,checksum);
        assert_eq!(owner.ingest_owned_frame(1,1,CLIENT_PORT,&frame[..46],
            [0x52,0x54,0,0x12,0x34,0x56]),Err(IngressError::Malformed));
        assert_eq!(owner.ingest_owned_frame(1,1,CLIENT_PORT,&frame[..10],
            [0x52,0x54,0,0x12,0x34,0x56]),Err(IngressError::Malformed));
        assert_eq!(owner.pending_owned(1,1),Ok(0));
    }
}


// R114 continued: regression coverage for the real checksum-validated DNS
// reply path after a trusted socket-incarnation restart. These frames use the
// existing test-only QEMU peer format, never a production public DNS resolver.
#[cfg(test)]
mod r114_dns_owner_reconnect_tests {
 use super::*;
 fn response() -> ([u8;FRAME_CAP],usize) {
    let mut frame=[0u8;FRAME_CAP];
    let dns_len=12+DNS_NAME.len()+4+16;
    let udp_len=8+dns_len;
    let total=14+20+udp_len;
    let mac=[0x52,0x54,0,0x12,0x34,0x56];
    ethernet_header(&mut frame,mac,PEER_MAC,0x0800);
    ipv4_header(&mut frame[14..34],17,udp_len,0x3828);
    frame[26..30].copy_from_slice(&PEER_IP);
    frame[30..34].copy_from_slice(&LOCAL_IP);
    put16(&mut frame,24,0);
    let ip_sum=checksum(&frame[14..34]);
    put16(&mut frame,24,ip_sum);
    let udp=&mut frame[34..total];
    put16(udp,0,53);put16(udp,2,CLIENT_PORT);put16(udp,4,udp_len as u16);
    let dns=&mut udp[8..];
    dns[..12].copy_from_slice(b"\x28\x28\x81\x80\x00\x01\x00\x01\x00\x00\x00\x00");
    dns[12..12+DNS_NAME.len()].copy_from_slice(DNS_NAME);
    let a=12+DNS_NAME.len();
    dns[a..a+4].copy_from_slice(&[0,1,0,1]);
    dns[a+4..a+20].copy_from_slice(&[
     0xc0,0x0c,0,1,0,1,0,0,0,60,0,4,PEER_IP[0],PEER_IP[1],PEER_IP[2],PEER_IP[3]]);
    let sum=transport_sum(&PEER_IP,&LOCAL_IP,17,udp);
    put16(udp,6,if sum==0{0xffff}else{sum});
    (frame,total)
 }
 #[test]
 fn restart_discards_old_dns_response_and_requires_new_generation(){
  let mut endpoint=EpochUdpEndpoints::<2,4>::new();
  let mut grants=crate::udp_owner_epoch::Registry::<2>::new();
  let mac=[0x52,0x54,0,0x12,0x34,0x56];
  endpoint.bind_registered(&mut grants,1,1,CLIENT_PORT).unwrap();
  let (frame,n)=response();
  assert!(valid_dns(&frame[..n],mac));
  endpoint.ingest_owned_frame(1,1,CLIENT_PORT,&frame[..n],mac).unwrap();
  assert_eq!(endpoint.pending_owned(1,1),Ok(1));
  endpoint.retire_registered(&mut grants,1,1).unwrap();
  endpoint.bind_registered(&mut grants,1,2,CLIENT_PORT).unwrap();
  assert_eq!(endpoint.receive(1,2,CLIENT_PORT,&mut [0u8;64]),
     Err(SocketError::Empty));
  assert_eq!(endpoint.ingest_owned_frame(1,1,CLIENT_PORT,&frame[..n],mac),
    Err(IngressError::Endpoint(SocketError::Denied)));
  endpoint.ingest_owned_frame(1,2,CLIENT_PORT,&frame[..n],mac).unwrap();
  assert_eq!(endpoint.pending_owned(1,2),Ok(1));
 }
 #[test]
 fn corrupt_dns_transaction_and_checksum_cannot_publish(){
  let mut endpoint=EpochUdpEndpoints::<1,2>::new();
  let mac=[0x52,0x54,0,0x12,0x34,0x56];
  endpoint.bind(1,2,CLIENT_PORT).unwrap();
  let (frame,n)=response();
  let mut bad=frame;
  bad[34+8]^=1; // DNS transaction payload mutation invalidates UDP checksum
  assert!(!valid_dns(&bad[..n],mac));
  assert_eq!(endpoint.ingest_owned_frame(1,2,CLIENT_PORT,&bad[..n],mac),
      Err(IngressError::BadChecksum));
  assert_eq!(endpoint.pending_owned(1,2),Ok(0));
  let mut bad=frame;bad[35]=0; // source port 53 -> 0; breaks UDP checksum and DNS provenance
  assert!(!valid_dns(&bad[..n],mac));
  assert_eq!(endpoint.pending_owned(1,2),Ok(0));
 }
}



// R115: kernel-owned port withdrawal coordinates both policy registry and
// actual live socket/queue state. Validate the whole batch before mutation;
// only one serialized supervisor can call this with scheduler-issued identity.
impl<const PORTS:usize,const QUEUE:usize> EpochUdpEndpoints<PORTS,QUEUE> {
    pub fn close_registered_batch<const N:usize>(
        &mut self,registry:&mut crate::udp_owner_epoch::Registry<N>,
        grants:&[crate::udp_owner_epoch::Grant],
    )->Result<usize,SocketError>{
        use crate::udp_owner_epoch::Error as GrantError;
        // Validate against the live registry and reject duplicate requests
        // before touching either the registry or the socket queue.
        for (i,grant) in grants.iter().enumerate(){
            if grant.pid==0||grant.generation==0||grant.port==0{
                return Err(SocketError::Invalid)
            }
            if grants[..i].iter().any(|g|g.port==grant.port){
                return Err(SocketError::Duplicate)
            }
            if registry.authorize(grant.pid,grant.generation,grant.port).is_err()||
               !self.authenticated(grant.pid,grant.generation)||
               !self.endpoints.owns(grant.pid,grant.port){
                return Err(SocketError::Denied)
            }
        }
        // Kernel scheduler serializes this operation. The entire registry
        // transaction is all-or-nothing; a failed batch cannot drop packets.
        registry.unbind_batch(grants).map_err(|e|match e{
            GrantError::Invalid=>SocketError::Invalid,
            GrantError::Duplicate=>SocketError::Duplicate,
            _=>SocketError::Denied,
        })?;
        for grant in grants {
            // Preflight above proved this exact owner/port in the socket
            // table; there is no fallible device I/O in close().
            self.close(grant.pid,grant.generation,grant.port)?;
        }
        Ok(grants.len())
    }
}
#[cfg(test)]
mod r115_registered_socket_batch_tests{
    use super::*;
    use crate::udp_owner_epoch::{Registry,Grant};
    fn g(pid:u32,generation:u64,port:u16)->Grant{
        Grant{pid,generation,port}
    }
    #[test]
    fn real_socket_close_rejects_stale_and_preserves_peer_traffic(){
        let mut sockets=EpochUdpEndpoints::<4,6>::new();
        let mut owners=Registry::<4>::new();
        sockets.bind_registered(&mut owners,7,1,5001).unwrap();
        sockets.bind_registered(&mut owners,7,1,5002).unwrap();
        sockets.bind_registered(&mut owners,8,3,5003).unwrap();
        sockets.send_local(8,3,5003,5001,b"obsolete").unwrap();
        sockets.send_local(7,1,5002,5003,b"peer").unwrap();
        assert_eq!(sockets.close_registered_batch(&mut owners,
            &[g(7,1,5001),g(8,2,5003)]),Err(SocketError::Denied));
        assert_eq!(owners.active_count(),3);
        assert_eq!(sockets.pending_owned(7,1),Ok(1));
        assert_eq!(sockets.pending_owned(8,3),Ok(1));
        assert_eq!(sockets.close_registered_batch(&mut owners,
            &[g(7,1,5001),g(7,1,5002)]),Ok(2));
        assert_eq!(owners.active_count(),1);
        assert_eq!(sockets.pending_owned(7,1),Err(SocketError::Denied));
        let mut out=[0u8;8];
        assert_eq!(sockets.receive(8,3,5003,&mut out),Ok(4));
        assert_eq!(&out[..4],b"peer");
        sockets.bind_registered(&mut owners,7,2,5001).unwrap();
        assert_eq!(sockets.receive(7,2,5001,&mut out),Err(SocketError::Empty));
    }
    #[test]
    fn malformed_duplicate_batch_is_transactional(){
        let mut sockets=EpochUdpEndpoints::<2,2>::new();
        let mut owners=Registry::<2>::new();
        sockets.bind_registered(&mut owners,7,1,8000).unwrap();
        assert_eq!(sockets.close_registered_batch(&mut owners,
            &[g(7,1,8000),g(7,1,8000)]),Err(SocketError::Duplicate));
        assert_eq!(owners.active_count(),1);
        assert_eq!(sockets.close_registered_batch(&mut owners,
            &[g(7,1,8000),g(0,1,8)]),Err(SocketError::Invalid));
        assert_eq!(owners.authorize(7,1,8000),Ok(()));
        assert_eq!(sockets.close_registered_batch(&mut owners,&[]),Ok(0));
    }
}



/// R115: transmit a packet only while an authenticated route policy AND a
/// time-scoped link receipt are current. The timestamp must be obtained from
/// a trusted monotonic kernel clock; a user-supplied value is not authority.
/// This QEMU-facing API checks the lease before writing to EthernetDevice.
pub fn transmit_leased_ipv4<D:EthernetDevice,const N:usize,const M:usize>(
    dev:&mut D,
    routes:&crate::agent_06_ipv4_route::OwnerRoutes<N>,
    links:&crate::agent_06_ipv4_route::FailoverRoutes<M>,
    owner:crate::batch5_session::Identity,
    owner_epoch:u64,now:u64,destination:[u8;4],
    constructor:impl FnOnce(&mut [u8],[u8;6])->usize,
)->Result<(),Error>{
    let dest=u32::from_be_bytes(destination);
    let receipt=links.receipt_at(dest,now).ok_or(Error::Unexpected)?;
    if !links.receipt_is_current_at(dest,now,receipt){
        return Err(Error::Unexpected)
    }
    let policy=routes.select(owner,owner_epoch,dest)
        .map_err(|_|Error::Unexpected)?.ok_or(Error::Unexpected)?;
    if policy!=receipt.selected.route{return Err(Error::Unexpected)}
    transmit_owned_ipv4(dev,routes,owner,owner_epoch,destination,constructor)
}
#[cfg(test)]
mod r115_transmit_lease_enforcement_tests {
    use super::*;
    use crate::agent_06_ipv4_route::{FailoverRoutes,OwnerRoutes,Route,RouteCandidate};
    use crate::batch5_session::Identity;
    struct Fake{sent:u32}
    impl EthernetDevice for Fake {
        fn mac(&self)->[u8;6]{[0x52,0x54,0,0x12,0x34,0x56]}
        fn send(&mut self,_f:&[u8])->Result<(),Error>{self.sent+=1;Ok(())}
        fn poll(&mut self,_o:&mut[u8])->Result<Option<usize>,Error>{Ok(None)}
    }
    #[test]
    fn expired_link_cannot_send_and_fallback_can_send(){
        let id=Identity{pid:1,generation:1};
        let mut owned=OwnerRoutes::<1>::new();
        owned.bind_kernel(id,1).unwrap();
        let route=Route{network:0x0a07_0000,prefix:24,next_hop:0};
        owned.publish(id,1,&[route]).unwrap();
        let mut links=FailoverRoutes::<2>::new();
        links.add_dhcp_lease(RouteCandidate{
            route,metric:1,link:1,epoch:1,healthy:true},10,20).unwrap();
        links.add(RouteCandidate{route,metric:10,link:2,epoch:1,healthy:true}).unwrap();
        let mut nic=Fake{sent:0};
        let send=|nic:&mut Fake, now, available:&FailoverRoutes<2>|transmit_leased_ipv4(
            nic,&owned,available,id,1,now,PEER_IP,
            |buf,mac|udp_frame(buf,mac,PEER_UDP_PORT,b"proof",1));
        assert_eq!(send(&mut nic,19,&links),Ok(()));
        assert_eq!(send(&mut nic,20,&links),Ok(())); // safe backup route
        assert_eq!(nic.sent,2);
        links.expire_link(2,1);
        assert_eq!(send(&mut nic,20,&links),Err(Error::Unexpected));
        assert_eq!(nic.sent,2);
        assert_eq!(send(&mut nic,19,&links),Ok(())); // primary lease still live
        assert_eq!(nic.sent,3);
        links.expire_link(1,1);
        assert_eq!(send(&mut nic,19,&links),Err(Error::Unexpected));
        assert_eq!(nic.sent,3);
    }
    #[test]
    fn denied_policy_or_wrong_route_cannot_write_to_nic(){
        let id=Identity{pid:1,generation:1};
        let mut owned=OwnerRoutes::<1>::new();
        owned.bind_kernel(id,1).unwrap();
        owned.publish(id,1,&[Route{
            network:0x0a07_0000,prefix:24,next_hop:0
        }]).unwrap();
        let mut links=FailoverRoutes::<1>::new();
        links.add(RouteCandidate{route:Route{
            network:0,prefix:0,next_hop:0
        },metric:1,link:2,epoch:1,healthy:true}).unwrap();
        let mut nic=Fake{sent:0};
        assert_eq!(transmit_leased_ipv4(&mut nic,&owned,&links,id,1,12,PEER_IP,
            |buf,mac|udp_frame(buf,mac,PEER_UDP_PORT,b"proof",1)),
            Err(Error::Unexpected));
        assert_eq!(nic.sent,0);
        assert_eq!(transmit_leased_ipv4(&mut nic,&owned,&links,
            Identity{pid:1,generation:2},1,12,PEER_IP,
            |buf,mac|udp_frame(buf,mac,PEER_UDP_PORT,b"proof",1)),
            Err(Error::Unexpected));
        assert_eq!(nic.sent,0);
    }
}


#[cfg(test)]
mod r115_scheduler_socket_lifecycle_tests {
    use super::*;
    use crate::udp_owner_epoch::{Error as OwnerError, Grant, Registry};

    fn grant(pid:u32,generation:u64,port:u16)->Grant {
        Grant{pid,generation,port}
    }

    #[test]
    fn live_scheduler_exit_revokes_exact_generation_but_not_peer() {
        let mut sockets=EpochUdpEndpoints::<3,6>::new();
        let mut owners=Registry::<3>::new();
        sockets.bind_registered(&mut owners,1,10,5501).unwrap();
        sockets.bind_registered(&mut owners,2,20,5502).unwrap();
        sockets.send_local(1,10,5501,5502,b"old").unwrap();
        sockets.send_local(2,20,5502,5501,b"departing").unwrap();
        assert_eq!(sockets.retire_registered(&mut owners,2,21),
                   Err(SocketError::Denied));
        assert_eq!(owners.authorize(2,20,5502),Ok(()));
        assert_eq!(sockets.retire_registered(&mut owners,2,20),Ok(1));
        assert_eq!(owners.authorize(2,20,5502),Err(OwnerError::Denied));
        assert_eq!(sockets.receive(2,20,5502,&mut [0u8;16]),
                   Err(SocketError::Denied));
        assert_eq!(sockets.receive(1,10,5501,&mut [0u8;16]),Ok(9));
        sockets.bind_registered(&mut owners,2,21,5502).unwrap();
        assert_eq!(sockets.receive(2,21,5502,&mut [0u8;16]),
                   Err(SocketError::Empty));
        assert_eq!(owners.owner_of(5502),Some(grant(2,21,5502)));
        assert_eq!(owners.owner_of(5501),Some(grant(1,10,5501)));
    }

    #[test]
    fn syscall_close_releases_both_tables_and_blocks_replayed_generation() {
        let mut sockets=EpochUdpEndpoints::<2,4>::new();
        let mut owners=Registry::<2>::new();
        sockets.bind_registered(&mut owners,1,4,5501).unwrap();
        sockets.bind_registered(&mut owners,2,5,5502).unwrap();
        sockets.send_local(2,5,5502,5501,b"discard").unwrap();
        assert_eq!(sockets.close_registered_batch(&mut owners,
                  &[grant(1,3,5501)]),Err(SocketError::Denied));
        assert_eq!(owners.active_count(),2);
        assert_eq!(sockets.close_registered_batch(&mut owners,
                  &[grant(1,4,5501)]),Ok(1));
        assert_eq!(owners.owner_of(5501),None);
        assert_eq!(sockets.pending_owned(1,4),Err(SocketError::Denied));
        sockets.bind_registered(&mut owners,1,6,5501).unwrap();
        assert_eq!(sockets.receive(1,6,5501,&mut [0u8;16]),
                   Err(SocketError::Empty));
        assert_eq!(owners.authorize(1,4,5501),Err(OwnerError::Denied));
        assert_eq!(owners.authorize(2,5,5502),Ok(()));
    }
}


#[cfg(test)]
mod r115_registered_ingress_authority_tests {
    use super::*;
    use crate::udp_owner_epoch::Registry;
    const MAC:[u8;6]=[0x52,0x54,0,0x12,0x34,0x56];

    fn packet(port:u16)->([u8;FRAME_CAP],usize){
        let mut frame=[0u8;FRAME_CAP];
        ethernet_header(&mut frame,MAC,PEER_MAC,0x0800);
        ipv4_header(&mut frame[14..34],17,12,0x1234);
        frame[26..30].copy_from_slice(&PEER_IP);
        frame[30..34].copy_from_slice(&LOCAL_IP);
        put16(&mut frame,24,0);
        let cs=checksum(&frame[14..34]);
        put16(&mut frame,24,cs);
        let udp=&mut frame[34..46];
        put16(udp,0,PEER_UDP_PORT);
        put16(udp,2,port);
        put16(udp,4,12);
        udp[8..12].copy_from_slice(b"pass");
        let check=transport_sum(&PEER_IP,&LOCAL_IP,17,udp);
        put16(udp,6,if check==0{0xffff}else{check});
        (frame,46)
    }

    #[test]
    fn parser_never_queues_without_matching_kernel_port_registry() {
        let mut sockets=EpochUdpEndpoints::<2,4>::new();
        let mut registry=Registry::<2>::new();
        sockets.bind(7,2,5000).unwrap();
        let (p,n)=packet(5000);
        assert_eq!(sockets.ingest_registered_frame(&registry,7,2,5000,&p[..n],MAC),
          Err(IngressError::Endpoint(SocketError::Denied)));
        assert_eq!(sockets.pending_owned(7,2),Ok(0));
        registry.bind(7,2,5000).unwrap();
        assert_eq!(sockets.ingest_registered_frame(&registry,7,1,5000,&p[..n],MAC),
          Err(IngressError::Endpoint(SocketError::Denied)));
        assert_eq!(sockets.ingest_registered_frame(&registry,7,2,5000,&p[..n],MAC),Ok(()));
        registry.revoke_owner(7,2).unwrap();
        assert_eq!(sockets.ingest_registered_frame(&registry,7,2,5000,&p[..n],MAC),
          Err(IngressError::Endpoint(SocketError::Denied)));
        assert_eq!(sockets.pending_owned(7,2),Ok(1)); // denied packet was not added
    }

    #[test]
    fn valid_peer_survives_other_owner_epoch_retirement() {
        let mut sockets=EpochUdpEndpoints::<2,4>::new();
        let mut registry=Registry::<2>::new();
        sockets.bind_registered(&mut registry,7,2,5000).unwrap();
        sockets.bind_registered(&mut registry,8,9,5001).unwrap();
        sockets.retire_registered(&mut registry,7,2).unwrap();
        let (p,n)=packet(5001);
        assert_eq!(sockets.ingest_registered_frame(&registry,7,2,5001,&p[..n],MAC),
          Err(IngressError::Endpoint(SocketError::Denied)));
        assert_eq!(sockets.ingest_registered_frame(&registry,8,9,5001,&p[..n],MAC),Ok(()));
        let mut out=[0u8;8];
        assert_eq!(sockets.receive(8,9,5001,&mut out),Ok(4));
        assert_eq!(&out[..4],b"pass");
    }
}


// R116: all real ring-3 UDP actions, not merely bind and close, must require
// authorization from BOTH the exact PID-generation registry and the socket
// table. The serialized single-BSP kernel owns these two tables.
impl<const PORTS:usize,const QUEUE:usize> EpochUdpEndpoints<PORTS,QUEUE> {
 pub fn send_registered<const N:usize>(
  &mut self,registry:&crate::udp_owner_epoch::Registry<N>,
  pid:u32,generation:u64,source:u16,destination:u16,data:&[u8],
 )->Result<(),SocketError>{
  registry.authorize(pid,generation,source).map_err(|_|SocketError::Denied)?;
  self.send_local(pid,generation,source,destination,data)
 }
 pub fn receive_registered<const N:usize>(
  &mut self,registry:&crate::udp_owner_epoch::Registry<N>,
  pid:u32,generation:u64,port:u16,out:&mut[u8],
 )->Result<usize,SocketError>{
  registry.authorize(pid,generation,port).map_err(|_|SocketError::Denied)?;
  self.receive(pid,generation,port,out)
 }
}
#[cfg(test)]
mod r116_registered_ring3_udp_tests{
 use super::*;
 use crate::udp_owner_epoch::Registry;
 #[test]fn every_action_requires_live_kernel_registry_and_socket(){
  let mut s=EpochUdpEndpoints::<2,5>::new();
  let mut r=Registry::<2>::new();
  s.bind_registered(&mut r,1,3,5501).unwrap();
  s.bind_registered(&mut r,2,8,5502).unwrap();
  assert_eq!(s.send_registered(&r,1,3,5501,5502,b"hello"),Ok(()));
  let mut output=[0xcc;12];
  assert_eq!(s.receive_registered(&r,2,7,5502,&mut output),
      Err(SocketError::Denied));
  assert_eq!(s.receive_registered(&r,2,8,5502,&mut output),Ok(5));
  assert_eq!(&output[..5],b"hello");
  assert_eq!(s.send_registered(&r,1,4,5501,5502,b"forge"),
      Err(SocketError::Denied));
  assert_eq!(s.send_registered(&r,2,8,5501,5502,b"forge"),
      Err(SocketError::Denied));
  assert_eq!(s.pending_owned(2,8),Ok(0));
 }
 #[test]fn registry_revocation_blocks_send_receive_without_touching_queued_data(){
  let mut s=EpochUdpEndpoints::<2,4>::new();let mut r=Registry::<2>::new();
  s.bind_registered(&mut r,1,3,5501).unwrap();
  s.bind_registered(&mut r,2,8,5502).unwrap();
  s.send_registered(&r,1,3,5501,5502,b"old").unwrap();
  r.revoke_owner(2,8).unwrap();
  let mut output=[0x44;8];
  assert_eq!(s.receive_registered(&r,2,8,5502,&mut output),
    Err(SocketError::Denied));
  assert_eq!(output,[0x44;8]);
  assert_eq!(s.pending_owned(2,8),Ok(1));
  s.retire(2,8).unwrap(); // single-BSP recovery after fault
  s.bind_registered(&mut r,2,9,5502).unwrap();
  assert_eq!(s.receive_registered(&r,2,9,5502,&mut output),
    Err(SocketError::Empty));
 }
 #[test]fn retired_sender_cannot_inject_into_live_peer(){
  let mut s=EpochUdpEndpoints::<2,4>::new();let mut r=Registry::<2>::new();
  s.bind_registered(&mut r,1,3,5501).unwrap();
  s.bind_registered(&mut r,2,8,5502).unwrap();
  s.retire_registered(&mut r,1,3).unwrap();
  assert_eq!(s.send_registered(&r,1,3,5501,5502,b"bad"),
      Err(SocketError::Denied));
  assert_eq!(s.pending_owned(2,8),Ok(0));
  s.bind_registered(&mut r,1,4,5501).unwrap();
  assert_eq!(s.send_registered(&r,1,4,5501,5502,b"new"),Ok(()));
  let mut out=[0u8;8];
  assert_eq!(s.receive_registered(&r,2,8,5502,&mut out),Ok(3));
  assert_eq!(&out[..3],b"new");
 }
 #[test]fn no_owner_and_wrong_endpoint_are_denied_without_mutation(){
  let mut s=EpochUdpEndpoints::<1,1>::new();let mut r=Registry::<1>::new();
  assert_eq!(s.send_registered(&r,2,9,5502,5502,b"bad"),
     Err(SocketError::Denied));
  assert_eq!(s.receive_registered(&r,2,9,5502,&mut [0u8;0]),
     Err(SocketError::Denied));
  s.bind(2,9,5502).unwrap(); // deliberate dual-table divergence
  assert_eq!(s.send_registered(&r,2,9,5502,5502,b"bad"),
     Err(SocketError::Denied));
  r.bind(2,9,5502).unwrap();
  assert_eq!(s.send_registered(&r,2,9,5502,5502,b"ok"),Ok(()));
 }
}


/// R117: a synthetic-clock QEMU test or a genuine kernel monotonic timer
/// can supply two *trusted* readings. Re-check the same lease receipt at
/// dispatch time so expiry between packet preparation and send cannot
/// authorize the original route. No user-originated timestamp is accepted.
fn transmit_leased_ipv4_checked_try_clock<
 D:EthernetDevice,const N:usize,const M:usize
>(
 dev:&mut D,
 routes:&crate::agent_06_ipv4_route::OwnerRoutes<N>,
 links:&crate::agent_06_ipv4_route::FailoverRoutes<M>,
 owner:crate::batch5_session::Identity,owner_epoch:u64,
 destination:[u8;4],mut clock:impl FnMut()->Result<u64,Error>,
 constructor:impl FnOnce(&mut[u8],[u8;6])->usize,
)->Result<(),Error>{
 let dest=u32::from_be_bytes(destination);
 let before=clock()?;
 let token=links.receipt_at(dest,before).ok_or(Error::Unexpected)?;
 let policy=routes.select(owner,owner_epoch,dest)
    .map_err(|_|Error::Unexpected)?.ok_or(Error::Unexpected)?;
 if policy!=token.selected.route{return Err(Error::Unexpected);}
 // Serialize and validate the IPv4 frame first. Only then take the second
 // trusted clock sample, directly on the NIC dispatch boundary.
 transmit_owned_ipv4_guarded(dev,routes,owner,owner_epoch,destination,
    ||{
        let at_dispatch=clock()?;
        if at_dispatch<before||
            !links.receipt_is_current_at(dest,at_dispatch,token){
            return Err(Error::Unexpected);
        }
        let current=routes.select(owner,owner_epoch,dest)
            .map_err(|_|Error::Unexpected)?.ok_or(Error::Unexpected)?;
        if current!=token.selected.route{return Err(Error::Unexpected);}
        Ok(())
    },constructor)
}

/// QEMU policy clock fixture: caller supplies two trusted samples. The
/// fallible production implementation below never accepts a missing timer.
pub fn transmit_leased_ipv4_checked_clock<
 D:EthernetDevice,const N:usize,const M:usize
>(
 dev:&mut D,
 routes:&crate::agent_06_ipv4_route::OwnerRoutes<N>,
 links:&crate::agent_06_ipv4_route::FailoverRoutes<M>,
 owner:crate::batch5_session::Identity,owner_epoch:u64,
 destination:[u8;4],mut clock:impl FnMut()->u64,
 constructor:impl FnOnce(&mut[u8],[u8;6])->usize,
)->Result<(),Error>{
 transmit_leased_ipv4_checked_try_clock(
    dev,routes,links,owner,owner_epoch,destination,
    ||Ok(clock()),constructor)
}

/// R118: actual kernel timer-backed link admission for callers that have
/// enabled and calibrated the BSP timer. Every NIC transmit re-samples the
/// delivered IRQ counter after frame construction. A stopped, uncalibrated,
/// or zero-progress timer DENIES the frame. No fake test clock is accepted.
/// This does not provide SMP TSC synchronization, NIC IRQ/IOMMU safety or
/// multi-NIC physical qualification.
#[cfg(feature="timer")]
pub fn transmit_leased_ipv4_kernel_clock<
 D:EthernetDevice,const N:usize,const M:usize
>(
 dev:&mut D,
 routes:&crate::agent_06_ipv4_route::OwnerRoutes<N>,
 links:&crate::agent_06_ipv4_route::FailoverRoutes<M>,
 owner:crate::batch5_session::Identity,owner_epoch:u64,
 destination:[u8;4],
 constructor:impl FnOnce(&mut[u8],[u8;6])->usize,
)->Result<(),Error>{
 transmit_leased_ipv4_checked_try_clock(
    dev,routes,links,owner,owner_epoch,destination,
    ||crate::timer::kernel_monotonic_100hz().ok_or(Error::Unexpected),
    constructor)
}
/// R118-079: a RETRY must carry the previous, kernel-issued route receipt
/// and dispatch tick. Every retry reads the trusted clock both before frame
/// serialization and at the NIC dispatch boundary. This denies clock
/// rollback, a nonprogressing retry deadline, same-link replay, a stale link
/// generation and a renewed lease expiring while the frame is assembled.
///
/// The production entry point below derives clock time only from the BSP
/// calibrated timer. The QEMU fixture uses a separately named synthetic
/// source to test the real virtio-net failover network path.
fn transmit_failover_ipv4_checked_try_clock<
 D:EthernetDevice,const N:usize,const M:usize
>(
 dev:&mut D,
 routes:&crate::agent_06_ipv4_route::OwnerRoutes<N>,
 links:&crate::agent_06_ipv4_route::FailoverRoutes<M>,
 owner:crate::batch5_session::Identity,owner_epoch:u64,
 destination:[u8;4],
 previous:crate::agent_06_ipv4_route::RouteReceipt,
 previous_tick:u64,
 mut clock:impl FnMut()->Result<u64,Error>,
 constructor:impl FnOnce(&mut[u8],[u8;6])->usize,
)->Result<crate::agent_06_ipv4_route::RouteReceipt,Error>{
 let dest=u32::from_be_bytes(destination);
 let before=clock()?;
 // A retry without forward time progress cannot prove that a timeout
 // elapsed; a source reset must not reauthorize the original route.
 if before<=previous_tick{return Err(Error::Unexpected);}
 let chosen=links.receipt_at(dest,before).ok_or(Error::Unexpected)?;
 let selected=chosen.selected;
 if selected.link==previous.selected.link ||
    selected.route!=previous.selected.route {
    // This call is specifically a same-destination failover, not ordinary
    // retransmission. Never return to the primary during its reset window.
    return Err(Error::Unexpected);
 }
 let policy=routes.select(owner,owner_epoch,dest)
     .map_err(|_|Error::Unexpected)?.ok_or(Error::Unexpected)?;
 if policy!=selected.route{return Err(Error::Unexpected);}
 transmit_owned_ipv4_guarded(dev,routes,owner,owner_epoch,destination,
     ||{
         let dispatch=clock()?;
         if dispatch<before ||
            !links.receipt_is_current_at(dest,dispatch,chosen) {
             return Err(Error::Unexpected);
         }
         let now_route=routes.select(owner,owner_epoch,dest)
             .map_err(|_|Error::Unexpected)?.ok_or(Error::Unexpected)?;
         if now_route!=selected.route{return Err(Error::Unexpected);}
         Ok(())
     },constructor)?;
 Ok(chosen)
}

/// QEMU-only injectable clock fixture for the real guest's forced primary
/// UDP timeout and live virtio-net backup. Production callers MUST use
/// transmit_failover_ipv4_kernel_clock instead.
pub fn transmit_failover_ipv4_checked_clock<
 D:EthernetDevice,const N:usize,const M:usize
>(
 dev:&mut D,
 routes:&crate::agent_06_ipv4_route::OwnerRoutes<N>,
 links:&crate::agent_06_ipv4_route::FailoverRoutes<M>,
 owner:crate::batch5_session::Identity,owner_epoch:u64,
 destination:[u8;4],
 previous:crate::agent_06_ipv4_route::RouteReceipt,
 previous_tick:u64,
 mut clock:impl FnMut()->u64,
 constructor:impl FnOnce(&mut[u8],[u8;6])->usize,
)->Result<crate::agent_06_ipv4_route::RouteReceipt,Error>{
 transmit_failover_ipv4_checked_try_clock(
     dev,routes,links,owner,owner_epoch,destination,previous,
     previous_tick,||Ok(clock()),constructor)
}

/// Production-only BSP timer-backed failover. A missing/stopped or
/// uncalibrated kernel timer refuses to retry without NIC submission.
/// This is NOT physical multi-NIC, IRQ, IOMMU or SMP qualification.
#[cfg(feature="timer")]
pub fn transmit_failover_ipv4_kernel_clock<
 D:EthernetDevice,const N:usize,const M:usize
>(
 dev:&mut D,
 routes:&crate::agent_06_ipv4_route::OwnerRoutes<N>,
 links:&crate::agent_06_ipv4_route::FailoverRoutes<M>,
 owner:crate::batch5_session::Identity,owner_epoch:u64,
 destination:[u8;4],
 previous:crate::agent_06_ipv4_route::RouteReceipt,
 previous_tick:u64,
 constructor:impl FnOnce(&mut[u8],[u8;6])->usize,
)->Result<crate::agent_06_ipv4_route::RouteReceipt,Error>{
 transmit_failover_ipv4_checked_try_clock(
     dev,routes,links,owner,owner_epoch,destination,previous,
     previous_tick,
     ||crate::timer::kernel_monotonic_100hz().ok_or(Error::Unexpected),
     constructor)
}

#[cfg(test)]
mod r117_monotonic_route_clock_tests {
 use super::*;
 use crate::agent_06_ipv4_route::{OwnerRoutes,FailoverRoutes,Route,RouteCandidate};
 use crate::batch5_session::Identity;
 struct Fake{writes:usize}
 impl EthernetDevice for Fake{
  fn mac(&self)->[u8;6]{[0x52,0x54,0,0x12,0x34,0x56]}
  fn send(&mut self,_frame:&[u8])->Result<(),Error>{self.writes+=1;Ok(())}
  fn poll(&mut self,_out:&mut[u8])->Result<Option<usize>,Error>{Ok(None)}
 }
 fn setup()->(OwnerRoutes<1>,FailoverRoutes<2>,Identity){
  let owner=Identity{pid:1,generation:9};
  let route=Route{network:0x0a07_0000,prefix:24,next_hop:0};
  let mut owned=OwnerRoutes::new();
  owned.bind_kernel(owner,3).unwrap();
  owned.publish(owner,3,&[route]).unwrap();
  let mut links=FailoverRoutes::new();
  links.add_dhcp_lease(RouteCandidate{
   route,metric:1,link:1,epoch:2,healthy:true},10,20).unwrap();
  links.add(RouteCandidate{
   route,metric:8,link:2,epoch:3,healthy:true}).unwrap();
  (owned,links,owner)
 }
 fn send(nic:&mut Fake,owned:&OwnerRoutes<1>,links:&FailoverRoutes<2>,
     owner:Identity,times:[u64;2])->Result<(),Error>{
  let mut i=0;
  transmit_leased_ipv4_checked_clock(nic,owned,links,owner,3,PEER_IP,
    ||{let n=times[i];i+=1;n},
    |buffer,mac|udp_frame(buffer,mac,PEER_UDP_PORT,b"clock",2))
 }
 #[test]fn lease_expiry_during_send_window_denies_frame(){
  let (owned,links,owner)=setup();let mut nic=Fake{writes:0};
  assert_eq!(send(&mut nic,&owned,&links,owner,[19,20]),
      Err(Error::Unexpected));
  assert_eq!(nic.writes,0);
  assert_eq!(send(&mut nic,&owned,&links,owner,[20,20]),Ok(()));
  assert_eq!(nic.writes,1);
 }
 #[test]fn clock_rollback_never_authorizes_route(){
  let (owned,links,owner)=setup();let mut nic=Fake{writes:0};
  assert_eq!(send(&mut nic,&owned,&links,owner,[21,20]),
       Err(Error::Unexpected));
  assert_eq!(nic.writes,0);
  assert_eq!(send(&mut nic,&owned,&links,owner,[19,19]),Ok(()));
  assert_eq!(nic.writes,1);
 }
 #[test]fn foreign_owner_or_expired_routes_never_transmit(){
  let (owned,mut links,owner)=setup();let mut nic=Fake{writes:0};
  assert_eq!(send(&mut nic,&owned,&links,
       Identity{pid:1,generation:10},[18,18]),Err(Error::Unexpected));
  assert_eq!(nic.writes,0);
  links.expire_link(2,3);
  assert_eq!(send(&mut nic,&owned,&links,owner,[20,20]),
      Err(Error::Unexpected));
  assert_eq!(nic.writes,0);
 }
 #[test]fn normal_same_epoch_delivery_reaches_real_device_trait(){
  let (owned,links,owner)=setup();let mut nic=Fake{writes:0};
  assert_eq!(send(&mut nic,&owned,&links,owner,[11,12]),Ok(()));
  assert_eq!(send(&mut nic,&owned,&links,owner,[21,23]),Ok(()));
  assert_eq!(nic.writes,2);
 }
}


/// R117 follow-up: cross-process release proof at the actual privileged
/// syscall boundary. Kernel caller identity is trusted; claim is an untrusted
/// request. Failure leaves both endpoint and registry tables intact.
impl<const PORTS:usize,const QUEUE:usize> EpochUdpEndpoints<PORTS,QUEUE>{
    pub fn close_claimed_registered<const N:usize>(
        &mut self,
        registry:&mut crate::udp_owner_epoch::Registry<N>,
        caller:crate::batch5_session::Identity,
        claim:crate::udp_owner_epoch::Grant,
    )->Result<usize,SocketError>{
        if caller.pid==0 || caller.generation==0 ||
           claim.pid!=caller.pid || claim.generation!=caller.generation ||
           claim.port==0{
            return Err(SocketError::Denied);
        }
        self.close_registered_batch(registry,&[claim])
    }
}

#[cfg(test)]
mod r117_foreign_ring3_close_boundary_tests{
    use super::*;
    use crate::udp_owner_epoch::{Grant,Registry};
    use crate::batch5_session::Identity;
    fn id(pid:u32,generation:u64)->Identity{Identity{pid,generation}}
    fn g(pid:u32,generation:u64,port:u16)->Grant{Grant{pid,generation,port}}
    #[test]
    fn forged_pid_or_generation_cannot_evict_a_registered_peer(){
        let mut sockets=EpochUdpEndpoints::<2,4>::new();
        let mut registry=Registry::<2>::new();
        sockets.bind_registered(&mut registry,1,4,5501).unwrap();
        sockets.bind_registered(&mut registry,2,8,5502).unwrap();
        sockets.send_registered(&registry,1,4,5501,5502,b"peer").unwrap();
        for (caller,claim) in [
            (id(1,4),g(2,8,5502)),
            (id(2,7),g(2,8,5502)),
            (id(2,8),g(1,4,5501)),
            (id(1,0),g(1,4,5501)),
            (id(0,4),g(1,4,5501)),
        ]{
            assert_eq!(sockets.close_claimed_registered(
                &mut registry,caller,claim),Err(SocketError::Denied));
            assert_eq!(registry.active_count(),2);
            assert_eq!(sockets.pending_owned(2,8),Ok(1));
        }
        let mut output=[0u8;8];
        assert_eq!(sockets.receive_registered(&registry,2,8,5502,&mut output),
            Ok(4));
        assert_eq!(&output[..4],b"peer");
    }
    #[test]
    fn successful_exact_close_preserves_other_incarnation(){
        let mut sockets=EpochUdpEndpoints::<2,3>::new();
        let mut registry=Registry::<2>::new();
        sockets.bind_registered(&mut registry,1,1,5501).unwrap();
        sockets.bind_registered(&mut registry,2,2,5502).unwrap();
        assert_eq!(sockets.close_claimed_registered(
            &mut registry,id(2,2),g(2,2,5502)),Ok(1));
        assert_eq!(registry.owner_of(5502),None);
        assert_eq!(registry.owner_of(5501),Some(g(1,1,5501)));
        assert_eq!(sockets.send_registered(&registry,2,2,5502,5501,b"old"),
            Err(SocketError::Denied));
        sockets.bind_registered(&mut registry,2,3,5502).unwrap();
        assert_eq!(sockets.close_claimed_registered(
            &mut registry,id(2,2),g(2,2,5502)),Err(SocketError::Denied));
        assert_eq!(registry.owner_of(5502),Some(g(2,3,5502)));
        assert_eq!(sockets.send_registered(&registry,1,1,5501,5502,b"new"),
            Ok(()));
        let mut out=[0u8;8];
        assert_eq!(sockets.receive_registered(&registry,2,3,5502,&mut out),
            Ok(3));
        assert_eq!(&out[..3],b"new");
    }
    #[test]
    fn zero_port_and_wrong_registry_do_not_mutate_queue(){
        let mut sockets=EpochUdpEndpoints::<1,2>::new();
        let mut registry=Registry::<1>::new();
        sockets.bind_registered(&mut registry,1,1,5501).unwrap();
        assert_eq!(sockets.close_claimed_registered(
            &mut registry,id(1,1),g(1,1,0)),Err(SocketError::Denied));
        assert_eq!(registry.owner_of(5501),Some(g(1,1,5501)));
    }
}


#[cfg(test)]
mod r118_dispatch_boundary_tests {
 use super::*;
 use crate::agent_06_ipv4_route::{OwnerRoutes,FailoverRoutes,Route,RouteCandidate};
 use crate::batch5_session::Identity;
 use core::cell::Cell;
 struct Nic { transmissions:usize }
 impl EthernetDevice for Nic {
  fn mac(&self)->[u8;6]{[0x52,0x54,0,0x12,0x34,0x56]}
  fn send(&mut self,_frame:&[u8])->Result<(),Error>{
   self.transmissions+=1;Ok(())
  }
  fn poll(&mut self,_out:&mut[u8])->Result<Option<usize>,Error>{Ok(None)}
 }
 #[test]
 fn lease_expiry_after_ipv4_serialization_denies_real_send() {
  let owner=Identity{pid:1,generation:8};
  let route=Route{network:0x0a07_0000,prefix:24,next_hop:0};
  let mut routes=OwnerRoutes::<1>::new();
  routes.bind_kernel(owner,7).unwrap();
  routes.publish(owner,7,&[route]).unwrap();
  let mut links=FailoverRoutes::<1>::new();
  links.add_dhcp_lease(RouteCandidate{
   route,metric:1,link:1,epoch:1,healthy:true},10,20).unwrap();
  let constructed=Cell::new(false);
  let reads=Cell::new(0usize);
  let mut nic=Nic{transmissions:0};
  let result=transmit_leased_ipv4_checked_clock(&mut nic,&routes,&links,
   owner,7,PEER_IP,
   ||{
    let index=reads.get();reads.set(index+1);
    if index==0||!constructed.get(){19}else{20}
   },
   |buf,mac|{
    constructed.set(true);
    udp_frame(buf,mac,PEER_UDP_PORT,b"r118",0x118)
   });
  assert_eq!(result,Err(Error::Unexpected));
  assert!(constructed.get());
  assert_eq!(reads.get(),2);
  assert_eq!(nic.transmissions,0);
 }
 #[test]
 fn stable_clock_sends_and_reversed_clock_fails_closed() {
  let owner=Identity{pid:1,generation:8};
  let route=Route{network:0x0a07_0000,prefix:24,next_hop:0};
  let mut routes=OwnerRoutes::<1>::new();
  routes.bind_kernel(owner,7).unwrap();
  routes.publish(owner,7,&[route]).unwrap();
  let mut links=FailoverRoutes::<1>::new();
  links.add_dhcp_lease(RouteCandidate{
   route,metric:1,link:1,epoch:1,healthy:true},10,20).unwrap();
  let mut nic=Nic{transmissions:0};
  let mut readings=[12u64,12].into_iter();
  assert_eq!(transmit_leased_ipv4_checked_clock(&mut nic,&routes,&links,
   owner,7,PEER_IP,||readings.next().unwrap(),
   |buf,mac|udp_frame(buf,mac,PEER_UDP_PORT,b"r118",0x118)),Ok(()));
  assert_eq!(nic.transmissions,1);
  let mut reversed=[13u64,12].into_iter();
  assert_eq!(transmit_leased_ipv4_checked_clock(&mut nic,&routes,&links,
   owner,7,PEER_IP,||reversed.next().unwrap(),
   |buf,mac|udp_frame(buf,mac,PEER_UDP_PORT,b"r118",0x119)),
   Err(Error::Unexpected));
  assert_eq!(nic.transmissions,1);
 }
}

#[cfg(test)]
mod r118_orphan_udp_and_ipv4_security_tests{
 use super::*;
 use crate::agent_06_ipv4_route::{OwnerRoutes,Route};
 use crate::batch5_session::Identity;
 use crate::udp_owner_epoch::Registry;
 struct Fake{sent:usize}
 impl EthernetDevice for Fake{
  fn mac(&self)->[u8;6]{[0x52,0x54,0,0x12,0x34,0x56]}
  fn send(&mut self,_:&[u8])->Result<(),Error>{self.sent+=1;Ok(())}
  fn poll(&mut self,_:&mut[u8])->Result<Option<usize>,Error>{Ok(None)}
 }
 #[test]
 fn scheduler_retires_orphan_registry_without_killing_peer_or_new_generation(){
  let mut sockets=EpochUdpEndpoints::<4,4>::new();
  let mut registry=Registry::<4>::new();
  sockets.bind_registered(&mut registry,7,1,5401).unwrap();
  sockets.bind_registered(&mut registry,8,2,5402).unwrap();
  // A separate close already released socket state but failed to retire the
  // grant. Kernel scheduler must clean the exact orphaned generation.
  sockets.retire(7,1).unwrap();
  assert!(registry.authorize(7,1,5401).is_ok());
  assert_eq!(sockets.retire_exact_kernel(&mut registry,7,1),Ok(1));
  assert_eq!(registry.authorize(7,1,5401),
   Err(crate::udp_owner_epoch::Error::Denied));
  assert!(registry.authorize(8,2,5402).is_ok());
  sockets.bind_registered(&mut registry,7,2,5401).unwrap();
  // A stale owner must fail closed once a newer PID incarnation exists.
  assert_eq!(sockets.retire_exact_kernel(&mut registry,7,1),
      Err(SocketError::Denied));
  assert!(registry.authorize(7,2,5401).is_ok());
  assert!(registry.authorize(8,2,5402).is_ok());
  assert_eq!(sockets.retire_exact_kernel(&mut registry,0,1),
   Err(SocketError::Invalid));
 }
 #[test]
 fn owned_qemu_packet_must_have_exact_peer_mac_and_no_trailing_bytes_or_fragments(){
  let who=Identity{pid:1,generation:9};
  let mut routes=OwnerRoutes::<1>::new();
  routes.bind_kernel(who,7).unwrap();
  routes.publish(who,7,&[Route{network:0x0a07_0000,prefix:24,next_hop:0}]).unwrap();
  let mut nic=Fake{sent:0};
  for corruption in 0..5{
   assert_eq!(transmit_owned_ipv4(&mut nic,&routes,who,7,PEER_IP,
    |out,mac|{
     let n=udp_frame(out,mac,PEER_UDP_PORT,b"r118",0x118);
     match corruption {
      0=>out[0]^=1, // wrong destination MAC
      1=>out[6]^=1, // forged local MAC
      2=>out[n]=0x55, // illicit trailing byte
      3=>{out[20]=0x20;out[24]=0;out[25]=0;
          let c=checksum(&out[14..34]);put16(out,24,c);}, // IP fragment
      _=>out[14]=0x46, // unsupported header length
     }
     n+usize::from(corruption==2)
    }),Err(Error::Invalid));
  }
  assert_eq!(nic.sent,0);
  assert_eq!(transmit_owned_ipv4(&mut nic,&routes,who,7,PEER_IP,
   |out,mac|udp_frame(out,mac,PEER_UDP_PORT,b"ok",0x119)),Ok(()));
  assert_eq!(nic.sent,1);
 }
}

#[cfg(test)]
mod r118_three_process_retirement_tests {
 use super::*;
 use crate::udp_owner_epoch::{Registry,Error as GrantError};
 use crate::batch5_session::Identity;
 fn id(pid:u32,generation:u64)->Identity{Identity{pid,generation}}
 #[test]
 fn stale_member_rejects_entire_three_owner_batch_before_reclamation(){
  let mut sockets=EpochUdpEndpoints::<4,6>::new();
  let mut owners=Registry::<4>::new();
  for (pid,gen,port) in [(1,1,6101),(2,2,6102),(3,3,6103)] {
   sockets.bind_registered(&mut owners,pid,gen,port).unwrap();
  }
  sockets.send_local(1,1,6101,6103,b"live").unwrap();
  assert_eq!(sockets.retire_exact_batch_kernel(&mut owners,
    &[id(1,1),id(2,99),id(3,3)]),Err(SocketError::Denied));
  assert_eq!(owners.active_count(),3);
  assert!(owners.authorize(1,1,6101).is_ok());
  assert!(owners.authorize(2,2,6102).is_ok());
  assert_eq!(sockets.pending_owned(3,3),Ok(1));
  assert_eq!(sockets.retire_exact_batch_kernel(&mut owners,
    &[id(1,1),id(1,1)]),Err(SocketError::Duplicate));
  assert_eq!(owners.active_count(),3);
 }
 #[test]
 fn retired_owner_batch_keeps_peer_datagram_and_rebind_clean(){
  let mut sockets=EpochUdpEndpoints::<4,6>::new();
  let mut owners=Registry::<4>::new();
  for (pid,gen,port) in [(1,1,6101),(2,2,6102),(3,3,6103)] {
   sockets.bind_registered(&mut owners,pid,gen,port).unwrap();
  }
  sockets.send_local(2,2,6102,6103,b"peer").unwrap();
  assert_eq!(sockets.retire_exact_batch_kernel(&mut owners,
    &[id(1,1),id(2,2)]),Ok(2));
  assert_eq!(owners.active_count(),1);
  assert_eq!(owners.authorize(1,1,6101),Err(GrantError::Denied));
  let mut buf=[0u8;8];
  assert_eq!(sockets.receive(3,3,6103,&mut buf),Ok(4));
  assert_eq!(&buf[..4],b"peer");
  sockets.bind_registered(&mut owners,2,4,6102).unwrap();
  assert_eq!(sockets.receive(2,4,6102,&mut buf),Err(SocketError::Empty));
  assert_eq!(sockets.retire_exact_batch_kernel(&mut owners,
    &[id(2,2)]),Err(SocketError::Denied));
  assert_eq!(sockets.pending_owned(2,4),Ok(0));
  assert_eq!(sockets.retire_exact_batch_kernel(&mut owners,
    &[id(2,4)]),Ok(1));
  assert_eq!(sockets.retire_exact_batch_kernel(&mut owners,
    &[]),Ok(0));
 }
 #[test]
 fn orphan_only_grant_is_retired_without_live_socket(){
  let mut sockets=EpochUdpEndpoints::<2,2>::new();
  let mut owners=Registry::<2>::new();
  owners.bind(8,11,6120).unwrap();
  assert_eq!(sockets.retire_exact_batch_kernel(&mut owners,
   &[id(8,10)]),Err(SocketError::Denied));
  assert_eq!(owners.active_count(),1);
  assert_eq!(sockets.retire_exact_batch_kernel(&mut owners,
   &[id(8,11)]),Ok(1));
  assert_eq!(owners.active_count(),0);
 }
}

#[cfg(test)]
mod r118_primary_backup_dispatch_expiry_tests {
 use super::*;
 use crate::agent_06_ipv4_route::{FailoverRoutes,OwnerRoutes,Route,RouteCandidate};
 use crate::batch5_session::Identity;
 struct MockNic{send_count:usize}
 impl EthernetDevice for MockNic {
  fn mac(&self)->[u8;6]{[0x52,0x54,0,0x12,0x34,0x56]}
  fn send(&mut self,_frame:&[u8])->Result<(),Error>{self.send_count+=1;Ok(())}
  fn poll(&mut self,_out:&mut[u8])->Result<Option<usize>,Error>{Ok(None)}
 }
 fn setup()->(OwnerRoutes<1>,FailoverRoutes<2>,Identity){
  let owner=Identity{pid:1,generation:4};
  let route=Route{network:0x0a07_0000,prefix:24,next_hop:0};
  let mut owned=OwnerRoutes::<1>::new();
  owned.bind_kernel(owner,3).unwrap();
  owned.publish(owner,3,&[route]).unwrap();
  let mut links=FailoverRoutes::<2>::new();
  links.add_dhcp_lease(RouteCandidate{
    route,metric:1,link:1,epoch:2,healthy:true},10,20).unwrap();
  links.add_dhcp_lease(RouteCandidate{
    route,metric:8,link:2,epoch:3,healthy:true},10,25).unwrap();
  (owned,links,owner)
 }
 fn send(nic:&mut MockNic,owned:&OwnerRoutes<1>,
     links:&FailoverRoutes<2>,owner:Identity,readings:[u64;2])
     ->Result<(),Error>{
  let mut iter=readings.into_iter();
  transmit_leased_ipv4_checked_clock(nic,owned,links,owner,3,PEER_IP,
      ||iter.next().unwrap(),
      |out,mac|udp_frame(out,mac,PEER_UDP_PORT,b"lease",0x141))
 }
 #[test]
 fn primary_expiry_after_packet_preparation_does_not_fallback_to_backup(){
  let (owned,links,owner)=setup();let mut nic=MockNic{send_count:0};
  assert_eq!(send(&mut nic,&owned,&links,owner,[19,20]),Err(Error::Unexpected));
  assert_eq!(nic.send_count,0);
  // The caller must acquire a NEW token selecting backup link 2.
  assert_eq!(send(&mut nic,&owned,&links,owner,[20,21]),Ok(()));
  assert_eq!(nic.send_count,1);
 }
 #[test]
 fn backup_expiry_after_packet_preparation_and_reversal_deny_send(){
  let (owned,links,owner)=setup();let mut nic=MockNic{send_count:0};
  assert_eq!(send(&mut nic,&owned,&links,owner,[24,25]),Err(Error::Unexpected));
  assert_eq!(send(&mut nic,&owned,&links,owner,[26,26]),Err(Error::Unexpected));
  assert_eq!(send(&mut nic,&owned,&links,owner,[23,22]),Err(Error::Unexpected));
  assert_eq!(nic.send_count,0);
 }
 #[test]
 fn route_owner_generation_and_link_epoch_are_bound_to_dispatch_receipt(){
  let (owned,mut links,owner)=setup();let mut nic=MockNic{send_count:0};
  assert_eq!(send(&mut nic,&owned,&links,
     Identity{pid:owner.pid,generation:owner.generation+1},[18,18]),
     Err(Error::Unexpected));
  links.expire_link(1,2);
  assert_eq!(send(&mut nic,&owned,&links,owner,[18,19]),Ok(()));
  links.expire_link(2,3);
  assert_eq!(send(&mut nic,&owned,&links,owner,[18,19]),Err(Error::Unexpected));
  assert_eq!(nic.send_count,1);
 }
}

#[cfg(test)]
mod r119_scheduler_udp_ingress_regression_tests {
 use super::*;
 use crate::udp_owner_epoch::{Error as OwnerError,Registry,Grant};
 const MAC:[u8;6]=[0x52,0x54,0,0x12,0x34,0x56];
 fn frame(port:u16)->([u8;FRAME_CAP],usize){
  let mut f=[0u8;FRAME_CAP];
  ethernet_header(&mut f,MAC,PEER_MAC,0x0800);
  ipv4_header(&mut f[14..34],17,12,0x3000);
  f[26..30].copy_from_slice(&PEER_IP);
  f[30..34].copy_from_slice(&LOCAL_IP);
  put16(&mut f,24,0);
  let ip=checksum(&f[14..34]);put16(&mut f,24,ip);
  let udp=&mut f[34..46];
  put16(udp,0,PEER_UDP_PORT);put16(udp,2,port);put16(udp,4,12);
  udp[8..12].copy_from_slice(b"data");
  let sum=transport_sum(&PEER_IP,&LOCAL_IP,17,udp);
  put16(udp,6,if sum==0{0xffff}else{sum});
  (f,46)
 }
 #[test]
 fn foreign_registered_port_close_never_discards_owner_queue(){
  let mut sockets=EpochUdpEndpoints::<3,4>::new();
  let mut registry=Registry::<3>::new();
  sockets.bind_registered(&mut registry,1,3,5111).unwrap();
  sockets.bind_registered(&mut registry,2,4,5112).unwrap();
  sockets.send_local(1,3,5111,5112,b"peer").unwrap();
  let foreign=Grant{pid:1,generation:3,port:5112};
  assert_eq!(sockets.close_registered_batch(&mut registry,&[foreign]),
      Err(SocketError::Denied));
  assert_eq!(registry.active_count(),2);
  assert_eq!(sockets.pending_owned(2,4),Ok(1));
  let mut out=[0u8;8];
  assert_eq!(sockets.receive(2,4,5112,&mut out),Ok(4));
  assert_eq!(&out[..4],b"peer");
 }
 #[test]
 fn network_ingress_checks_port_generation_before_parsing(){
  let mut sockets=EpochUdpEndpoints::<2,3>::new();
  let mut registry=Registry::<2>::new();
  sockets.bind_registered(&mut registry,7,9,5301).unwrap();
  let (packet,n)=frame(5301);
  assert_eq!(sockets.ingest_registered_frame(&registry,7,10,5301,
      &packet[..n],MAC),Err(IngressError::Endpoint(SocketError::Denied)));
  assert_eq!(sockets.pending_owned(7,9),Ok(0));
  assert_eq!(sockets.ingest_registered_frame(&registry,7,9,5301,
      &packet[..n],MAC),Ok(()));
  assert_eq!(sockets.pending_owned(7,9),Ok(1));
  assert_eq!(sockets.retire_exact_kernel(&mut registry,7,9),Ok(1));
  sockets.bind_registered(&mut registry,7,10,5301).unwrap();
  assert_eq!(sockets.ingest_registered_frame(&registry,7,9,5301,
      &packet[..n],MAC),Err(IngressError::Endpoint(SocketError::Denied)));
  assert_eq!(sockets.pending_owned(7,10),Ok(0));
 }
 #[test]
 fn delayed_old_generation_cleanup_denies_new_registry_and_socket(){
  let mut sockets=EpochUdpEndpoints::<2,2>::new();
  let mut registry=Registry::<2>::new();
  sockets.bind_registered(&mut registry,5,3,5101).unwrap();
  assert_eq!(sockets.retire_exact_kernel(&mut registry,5,3),Ok(1));
  sockets.bind_registered(&mut registry,5,4,5101).unwrap();
  assert_eq!(sockets.retire_exact_kernel(&mut registry,5,3),
      Err(SocketError::Denied));
  assert_eq!(registry.authorize(5,4,5101),Ok(()));
  assert_eq!(sockets.pending_owned(5,4),Ok(0));
  assert_eq!(registry.authorize(5,3,5101),Err(OwnerError::Denied));
 }
}

#[cfg(all(test,feature="test-r031-vfs"))]
mod r120_three_process_mixed_resource_retirement {
 use super::*;
 use crate::agent_03_cap_epoch::{
    LeaseTable,Grant as CapGrant,Request,TimedGrant,
    LeaseFault,CapabilityError
 };
 use crate::batch5_session::Identity;
 use crate::fs_handles::{FileHandles,READ};
 use crate::udp_owner_epoch::{Registry,Error as OwnerError};

 fn identity(pid:u32,generation:u64)->Identity{
  Identity{pid,generation}
 }
 fn setup_leases()->LeaseTable<3>{
  let mut table=LeaseTable::<3>::new();
  let parent=CapGrant{owner:1,resource:8,rights:3,
      epoch:4,generation:1,revoked:false};
  let issuer=Request{caller:1,resource:8,required_rights:1,
      current_epoch:4,current_generation:1};
  for pid in 1..=3{
   let child=TimedGrant{grant:CapGrant{
      owner:pid,resource:8,rights:1,epoch:4,
      generation:pid as u64,revoked:false},expires_at:100};
   table.delegate(parent,issuer,child,1).unwrap();
  }
  table
 }
 fn cap_request(pid:u32)->Request{
  Request{caller:pid,resource:8,required_rights:1,
     current_epoch:4,current_generation:pid as u64}
 }
 #[test]
 fn stale_three_process_network_batch_does_not_mutate_capabilities_or_files(){
  for _round in 0..32{
   let mut sockets=EpochUdpEndpoints::<3,4>::new();
   let mut registry=Registry::<3>::new();
   let mut leases=setup_leases();
   let mut files=FileHandles::new();
   let init=files.bind_process(1).unwrap();
   let worker=files.bind_process(2).unwrap();
   let init_alias=files.mount_alias_for_current_root_kernel(init).unwrap();
   for pid in 1..=3{
    sockets.bind_registered(&mut registry,pid,pid as u64,6100+pid as u16)
      .unwrap();
   }
   sockets.send_local(1,1,6101,6103,b"old").unwrap();
   let stale=[identity(1,1),identity(2,99),identity(3,3)];
   assert_eq!(sockets.retire_exact_batch_kernel(&mut registry,&stale),
       Err(SocketError::Denied));
   assert_eq!(leases.revoke_identity_batch_kernel(&[(1,1),(2,99),(3,3)]),
       Err(LeaseFault::Denied(CapabilityError::Stale)));
   assert_eq!(files.retire_process_batch_kernel(&[
       init,Identity{pid:2,generation:worker.generation+1}]),
       Err(crate::durablefs::FsError::InvalidName));
   assert!(files.mount_alias_allows(init_alias,init,"/boot/init"));
   assert!(files.owns(worker));
   assert_eq!(registry.active_count(),3);
   assert_eq!(sockets.pending_owned(3,3),Ok(1));
   assert_eq!(leases.check(cap_request(2),2),Ok(()));
   assert_eq!(leases.check(cap_request(3),2),Ok(()));
   assert_eq!(sockets.retire_exact_batch_kernel(&mut registry,&[
       identity(1,1),identity(2,2)]),Ok(2));
   assert_eq!(leases.revoke_identity_batch_kernel(&[(1,1),(2,2)]),Ok(2));
   assert_eq!(files.retire_process_batch_kernel(&[init,worker]),Ok(2));
   assert!(!files.owns(init)&&!files.owns(worker));
   assert_eq!(registry.authorize(1,1,6101),Err(OwnerError::Denied));
   assert_eq!(registry.authorize(2,2,6102),Err(OwnerError::Denied));
   assert_eq!(registry.authorize(3,3,6103),Ok(()));
   assert_eq!(leases.check(cap_request(3),2),Ok(()));
   let mut out=[0u8;8];
   assert_eq!(sockets.receive(3,3,6103,&mut out),Ok(3));
   assert_eq!(&out[..3],b"old");
   let new_worker=files.bind_process(2).unwrap();
   assert_ne!(worker,new_worker);
   assert!(!files.mount_alias_allows(init_alias,new_worker,"/boot/init"));
   sockets.bind_registered(&mut registry,2,4,6102).unwrap();
   assert_eq!(sockets.retire_exact_kernel(&mut registry,2,2),
       Err(SocketError::Denied));
   assert_eq!(registry.authorize(2,4,6102),Ok(()));
  }
 }
}

#[cfg(test)]
mod r118_kernel_clock_dispatch_fail_closed_tests{
    use super::*;
    use crate::agent_06_ipv4_route::{OwnerRoutes,FailoverRoutes,Route,RouteCandidate};
    use crate::batch5_session::Identity;
    struct Fake{writes:usize}
    impl EthernetDevice for Fake{
        fn mac(&self)->[u8;6]{[0x52,0x54,0,0x12,0x34,0x56]}
        fn send(&mut self,_frame:&[u8])->Result<(),Error>{self.writes+=1;Ok(())}
        fn poll(&mut self,_out:&mut[u8])->Result<Option<usize>,Error>{Ok(None)}
    }
    #[test]
    fn missing_hardware_tick_at_dispatch_never_writes_a_nic_frame(){
        let owner=Identity{pid:1,generation:8};
        let route=Route{network:0x0a07_0000,prefix:24,next_hop:0};
        let mut routes=OwnerRoutes::<1>::new();
        routes.bind_kernel(owner,3).unwrap();
        routes.publish(owner,3,&[route]).unwrap();
        let mut links=FailoverRoutes::<1>::new();
        links.add(RouteCandidate{
            route,metric:1,link:1,epoch:7,healthy:true
        }).unwrap();
        let mut nic=Fake{writes:0};
        let mut samples=0;
        let result=transmit_leased_ipv4_checked_try_clock(
            &mut nic,&routes,&links,owner,3,PEER_IP,
            ||{
                samples+=1;
                if samples==1{Ok(10)}else{Err(Error::Unexpected)}
            },
            |buf,mac|udp_frame(buf,mac,PEER_UDP_PORT,b"safe",6));
        assert_eq!(result,Err(Error::Unexpected));
        assert_eq!(samples,2);
        assert_eq!(nic.writes,0);
        assert_eq!(transmit_leased_ipv4_checked_try_clock(
            &mut nic,&routes,&links,owner,3,PEER_IP,
            ||Err(Error::Unexpected),
            |buf,mac|udp_frame(buf,mac,PEER_UDP_PORT,b"safe",7)),
            Err(Error::Unexpected));
        assert_eq!(nic.writes,0);
    }
}

// R118-072 source regression: real Ethernet/UDP validation must route via
// trusted registry ownership without an untrusted packet-supplied PID.
#[cfg(test)]
mod r118_privileged_nic_ingress_tests{
    use super::*;
    use crate::process_retirement_txn::{ProcessRetirement,Owner,RetirementError};
    use crate::retirement_gate::{RetirementGate,GateError};
    use crate::udp_owner_epoch::Error as OwnerError;
    const LOCAL:[u8;6]=[0x52,0x54,0,0x12,0x34,0x56];

    fn reply(port:u16,payload:&[u8])->([u8;FRAME_CAP],usize){
        let mut frame=[0u8;FRAME_CAP];
        let len=8+payload.len();
        ethernet_header(&mut frame,LOCAL,PEER_MAC,0x0800);
        ipv4_header(&mut frame[14..34],17,len,0x2201);
        frame[26..30].copy_from_slice(&PEER_IP);
        frame[30..34].copy_from_slice(&LOCAL_IP);
        put16(&mut frame,24,0);
        let ip_sum=checksum(&frame[14..34]);
        put16(&mut frame,24,ip_sum);
        let udp=&mut frame[34..34+len];
        put16(udp,0,PEER_UDP_PORT);
        put16(udp,2,port);
        put16(udp,4,len as u16);
        udp[8..].copy_from_slice(payload);
        let sum=transport_sum(&PEER_IP,&LOCAL_IP,17,udp);
        put16(udp,6,if sum==0{0xffff}else{sum});
        (frame,34+len)
    }

    #[test]
    fn actual_wire_destination_selects_only_live_registered_process(){
        let gate=RetirementGate::new();
        gate.register_bsp(0,true).unwrap();
        let mut sockets=EpochUdpEndpoints::<2,4>::new();
        let mut k=ProcessRetirement::<2,2,4>::new();
        let a=k.start(1).unwrap();
        let b=k.start(2).unwrap();
        k.bind_live_udp(&mut sockets,a,5001).unwrap();
        k.bind_live_udp(&mut sockets,b,5002).unwrap();
        let (f,n)=reply(5002,b"peer");
        let permit=gate.try_enter(0,true).unwrap();
        assert_eq!(k.ingest_kernel_nic_frame(
            &permit,&mut sockets,k.capture_ingress_revision(&permit),&f[..n],LOCAL),Ok(b));
        let mut out=[0u8;8];
        assert_eq!(k.receive_live_udp(&mut sockets,a,5001,&mut out),
            Err(RetirementError::Sockets(SocketError::Empty)));
        assert_eq!(k.receive_live_udp(&mut sockets,b,5002,&mut out),Ok(4));
        assert_eq!(&out[..4],b"peer");
        drop(permit);
        k.retire_live_exited_batch(&mut sockets,&[b]).unwrap();
        let permit=gate.try_enter(0,true).unwrap();
        assert_eq!(k.ingest_kernel_nic_frame(
            &permit,&mut sockets,k.capture_ingress_revision(&permit),&f[..n],LOCAL),
            Err(RetirementError::Udp(OwnerError::Denied)));
        assert_eq!(sockets.pending_owned(a.pid,a.generation),Ok(0));
        assert_eq!(k.active_generation(b.pid),None);
    }

    #[test]
    fn malformed_checksum_unknown_port_and_wrong_cpu_never_enqueue(){
        let gate=RetirementGate::new();
        gate.register_bsp(0,true).unwrap();
        let mut sockets=EpochUdpEndpoints::<1,2>::new();
        let mut k=ProcessRetirement::<1,1,2>::new();
        let a=k.start(9).unwrap();
        k.bind_live_udp(&mut sockets,a,6000).unwrap();
        assert!(matches!(gate.try_enter(1,true),Err(GateError::WrongCpu)));
        assert!(matches!(gate.try_enter(0,false),Err(GateError::InterruptsEnabled)));
        let permit=gate.try_enter(0,true).unwrap();
        let (f,n)=reply(6000,b"safe");
        let mut damaged=f;damaged[40]^=1;
        assert_eq!(k.ingest_kernel_nic_frame(
            &permit,&mut sockets,k.capture_ingress_revision(&permit),&damaged[..n],LOCAL),
            Err(RetirementError::Ingress(IngressError::BadChecksum)));
        let (wrong,wn)=reply(6001,b"bad");
        assert_eq!(k.ingest_kernel_nic_frame(
            &permit,&mut sockets,k.capture_ingress_revision(&permit),&wrong[..wn],LOCAL),
            Err(RetirementError::Udp(OwnerError::Denied)));
        let mut out=[0xa5u8;8];
        assert_eq!(k.receive_live_udp(&mut sockets,a,6000,&mut out),
            Err(RetirementError::Sockets(SocketError::Empty)));
        assert_eq!(out,[0xa5;8]);
    }


    #[test]
    fn stale_polled_packet_cannot_cross_port_rebind_to_recycled_pid(){
        let gate=RetirementGate::new();gate.register_bsp(0,true).unwrap();
        let mut sockets=EpochUdpEndpoints::<2,4>::new();
        let mut k=ProcessRetirement::<2,2,4>::new();
        let old=k.start(1).unwrap();
        let peer=k.start(2).unwrap();
        k.bind_live_udp(&mut sockets,old,5001).unwrap();
        k.bind_live_udp(&mut sockets,peer,5002).unwrap();
        // The NIC begins polling while PID 1 still owns port 5001.
        let permit=gate.try_enter(0,true).unwrap();
        let old_rx=k.capture_ingress_revision(&permit);
        drop(permit);
        let (frame,len)=reply(5001,b"stale");
        k.retire_live_exited_batch(&mut sockets,&[old]).unwrap();
        let recycled=k.start(1).unwrap();
        k.bind_live_udp(&mut sockets,recycled,5001).unwrap();
        assert_ne!(old.generation,recycled.generation);
        let permit=gate.try_enter(0,true).unwrap();
        assert_eq!(k.ingest_kernel_nic_frame(
            &permit,&mut sockets,old_rx,&frame[..len],LOCAL),
            Err(RetirementError::Sockets(SocketError::Denied)));
        assert_eq!(sockets.pending_owned(recycled.pid,recycled.generation),Ok(0));
        assert_eq!(sockets.pending_owned(peer.pid,peer.generation),Ok(0));
        // A NEW receive descriptor captured after the rebind may deliver
        // fresh traffic. Physical NIC RX descriptors must be drained and
        // retagged before such a descriptor is considered authoritative.
        let (fresh,fresh_len)=reply(5001,b"fresh");
        let current_rx=k.capture_ingress_revision(&permit);
        assert_eq!(k.ingest_kernel_nic_frame(
            &permit,&mut sockets,current_rx,&fresh[..fresh_len],LOCAL),Ok(recycled));
        let mut out=[0u8;16];
        assert_eq!(k.receive_live_udp(&mut sockets,recycled,5001,&mut out),Ok(5));
        assert_eq!(&out[..5],b"fresh");
    }
    #[test]
    fn stale_poll_token_is_rejected_even_for_unrelated_peer_port(){
        let gate=RetirementGate::new();gate.register_bsp(0,true).unwrap();
        let mut sockets=EpochUdpEndpoints::<2,4>::new();
        let mut k=ProcessRetirement::<2,2,4>::new();
        let a=k.start(1).unwrap();let peer=k.start(2).unwrap();
        k.bind_live_udp(&mut sockets,a,5001).unwrap();
        k.bind_live_udp(&mut sockets,peer,5002).unwrap();
        let permit=gate.try_enter(0,true).unwrap();
        let old_rx=k.capture_ingress_revision(&permit);
        drop(permit);
        k.close_live_udp(&mut sockets,a,5001).unwrap();
        let (frame,n)=reply(5002,b"peer");
        let permit=gate.try_enter(0,true).unwrap();
        assert_eq!(k.ingest_kernel_nic_frame(
            &permit,&mut sockets,old_rx,&frame[..n],LOCAL),
            Err(RetirementError::Sockets(SocketError::Denied)));
        assert_eq!(sockets.pending_owned(peer.pid,peer.generation),Ok(0));
        let new_rx=k.capture_ingress_revision(&permit);
        assert_eq!(k.ingest_kernel_nic_frame(
            &permit,&mut sockets,new_rx,&frame[..n],LOCAL),Ok(peer));
    }
    #[test]
    fn staged_teardown_revokes_old_generation_and_preserves_peer_traffic(){
        let gate=RetirementGate::new();
        gate.register_bsp(0,true).unwrap();
        let mut sockets=EpochUdpEndpoints::<2,4>::new();
        let mut k=ProcessRetirement::<2,2,4>::new();
        let old=k.start(1).unwrap();
        let peer=k.start(2).unwrap();
        k.bind_live_udp(&mut sockets,old,5001).unwrap();
        k.bind_live_udp(&mut sockets,peer,5002).unwrap();
        let (frame,n)=reply(5002,b"live");
        let permit=gate.try_enter(0,true).unwrap();
        assert_eq!(k.ingest_kernel_nic_frame(
            &permit,&mut sockets,k.capture_ingress_revision(&permit),&frame[..n],LOCAL),Ok(peer));
        drop(permit);
        k.retire_live_exited_batch(&mut sockets,&[old]).unwrap();
        let newer=k.start(1).unwrap();
        k.bind_live_udp(&mut sockets,newer,5001).unwrap();
        assert_ne!(newer.generation,old.generation);
        assert!(k.ingest_live_udp(&mut sockets,old,5001,
            &frame[..n],LOCAL).is_err());
        let mut out=[0u8;8];
        assert_eq!(k.receive_live_udp(&mut sockets,peer,5002,&mut out),Ok(4));
        assert_eq!(&out[..4],b"live");
    }
}

#[cfg(test)]
mod r118_monotonic_failover_retry_tests {
    use super::*;
    use crate::agent_06_ipv4_route::{
        OwnerRoutes,FailoverRoutes,Route,RouteCandidate,RouteReceipt
    };
    use crate::batch5_session::Identity;

    struct Nic { sent:usize }
    impl EthernetDevice for Nic {
        fn mac(&self)->[u8;6]{[0x52,0x54,0,0x12,0x34,0x56]}
        fn send(&mut self,_frame:&[u8])->Result<(),Error>{
            self.sent+=1;Ok(())
        }
        fn poll(&mut self,_out:&mut[u8])->Result<Option<usize>,Error>{
            Ok(None)
        }
    }
    fn setup()->(OwnerRoutes<1>,FailoverRoutes<2>,Identity,RouteReceipt) {
        let owner=Identity{pid:1,generation:7};
        let route=Route{network:0x0a07_0000,prefix:24,next_hop:0};
        let mut policy=OwnerRoutes::<1>::new();
        policy.bind_kernel(owner,4).unwrap();
        policy.publish(owner,4,&[route]).unwrap();
        let mut links=FailoverRoutes::<2>::new();
        links.add_dhcp_lease(RouteCandidate{
            route,metric:1,link:1,epoch:1,healthy:true
        },100,120).unwrap();
        links.add(RouteCandidate{
            route,metric:9,link:2,epoch:1,healthy:true
        }).unwrap();
        let initial=links.receipt_at(u32::from_be_bytes(PEER_IP),101).unwrap();
        assert_eq!(initial.selected.link,1);
        (policy,links,owner,initial)
    }
    fn retry(nic:&mut Nic,policy:&OwnerRoutes<1>,links:&FailoverRoutes<2>,
             owner:Identity,prior:RouteReceipt,first_tick:u64,
             times:[u64;2])->Result<RouteReceipt,Error>{
        let mut i=0;
        transmit_failover_ipv4_checked_clock(nic,policy,links,
            owner,4,PEER_IP,prior,first_tick,
            ||{let value=times[i];i+=1;value},
            |bytes,mac|udp_frame(bytes,mac,PEER_UDP_PORT,b"retry",0x2911))
    }
    #[test]
    fn real_failover_requires_new_link_and_forward_kernel_time() {
        let (policy,mut links,owner,prior)=setup();
        let mut nic=Nic{sent:0};
        assert_eq!(retry(&mut nic,&policy,&links,owner,prior,101,[110,110]),
                   Err(Error::Unexpected));
        assert_eq!(nic.sent,0);
        links.begin_link_reset(1,1,2).unwrap();
        assert_eq!(retry(&mut nic,&policy,&links,owner,prior,120,[101,121]),
                   Err(Error::Unexpected));
        assert_eq!(retry(&mut nic,&policy,&links,owner,prior,121,[120,122]),
                   Err(Error::Unexpected));
        assert_eq!(nic.sent,0);
        let new=retry(&mut nic,&policy,&links,owner,prior,101,[120,121]).unwrap();
        assert_eq!(new.selected.link,2);
        assert_eq!(nic.sent,1);
    }
    #[test]
    fn reversed_clock_between_serialization_and_dispatch_denies_frame() {
        let (policy,mut links,owner,prior)=setup();
        links.begin_link_reset(1,1,2).unwrap();
        let mut nic=Nic{sent:0};
        assert_eq!(retry(&mut nic,&policy,&links,owner,prior,101,[121,120]),
                   Err(Error::Unexpected));
        assert_eq!(nic.sent,0);
    }
    #[test]
    fn missing_kernel_time_and_wrong_process_cannot_retry() {
        let (policy,mut links,owner,prior)=setup();
        links.begin_link_reset(1,1,2).unwrap();
        let mut nic=Nic{sent:0};
        assert_eq!(transmit_failover_ipv4_checked_try_clock(
            &mut nic,&policy,&links,owner,4,PEER_IP,prior,101,
            ||Err(Error::Unexpected),
            |bytes,mac|udp_frame(bytes,mac,PEER_UDP_PORT,b"retry",0x2911)),
            Err(Error::Unexpected));
        assert_eq!(retry(&mut nic,&policy,&links,
            Identity{pid:owner.pid,generation:owner.generation+1},
            prior,101,[120,121]),Err(Error::Unexpected));
        assert_eq!(nic.sent,0);
    }
    #[test]
    fn failover_packet_is_rechecked_after_live_link_expiry() {
        let (policy,mut links,owner,prior)=setup();
        links.begin_link_reset(1,1,2).unwrap();
        // Release the quarantined primary candidate to free one route
        // slot while retaining the healthy secondary as fallback.
        assert_eq!(links.expire_link(1,2),1);
        // Expire a newly leased best candidate between receive and send.
        // Its expired receipt must not grant authority to fallback link 2.
        let backup=links.resolve_at(u32::from_be_bytes(PEER_IP),120).unwrap();
        links.add_dhcp_lease(RouteCandidate{
            route:backup.route,metric:8,link:3,epoch:4,healthy:true
        },120,121).unwrap();
        let mut nic=Nic{sent:0};
        // Link 3 wins at tick 120 and expires at tick 121, causing
        // a route change to link 2 before frame dispatch.
        assert_eq!(retry(&mut nic,&policy,&links,owner,prior,101,[120,121]),
                   Err(Error::Unexpected));
        assert_eq!(nic.sent,0);
    }
}
