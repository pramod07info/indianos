//! M2 bounded, per-process outbound socket authorization and DNS A response
//! decoder. Existing R028 QEMU protocol stack is a transport demo, not a
//! general socket API; this service NEVER sends packets or implements TLS.
//! Kernel-derived PID/generation + verified DNS response ingress REQUIRED.
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,SOCKET};
use crate::batch6_network::{Ports,Error as PortError,verified_tls};
use crate::batch3_network_security::TlsCertificateVerifier;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Denied,NotFound,Malformed,Unverified,RetryExhausted}
pub struct Endpoint<const P:usize>{
 ports:Ports<P>,
}
impl<const P:usize> Endpoint<P>{
 pub const fn new()->Self{Self{ports:Ports::new()}}
 pub fn acquire<const N:usize>(&mut self,auth:&Access<N>,
  owner:Identity,local_port:u16,now:u64,deadline:u64)->Result<(),Error>{
  self.ports.bind(auth,owner,local_port,now,deadline)
   .map_err(|_|Error::Denied)
 }
 pub fn allow_query(&self,owner:Identity,local_port:u16,
  now:u64,name:&[u8])->Result<(),Error>{
  self.ports.require(owner,local_port,now).map_err(|_|Error::Denied)?;
  if !valid_domain(name){return Err(Error::Invalid);}
  Ok(())
 }
 /// This only validates a caller-supplied DNS packet. The caller MUST
 /// independently validate the actual network peer, transport checksum
 /// and UDP source/destination endpoints to avoid DNS spoofing.
 pub fn accept_dns_a(&self,owner:Identity,local_port:u16,now:u64,
  transaction_id:u16,name:&[u8],packet:&[u8])->Result<[u8;4],Error>{
  self.allow_query(owner,local_port,now,name)?;
  parse_dns_a(transaction_id,name,packet)
 }
 pub fn allow_tls<V:TlsCertificateVerifier>(&self,owner:Identity,
  local_port:u16,now:u64,name:&[u8],certificate:&[u8],v:&V)
  ->Result<(),Error>{
  self.allow_query(owner,local_port,now,name)?;
  verified_tls(v,name,certificate).map_err(|_|Error::Unverified)
 }
 pub fn retire(&mut self,who:Identity){self.ports.retire(who)}
}
fn valid_domain(name:&[u8])->bool{
 if name.is_empty()||name.len()>253{return false;}
 for label in name.split(|&b|b==b'.'){
  if label.is_empty()||label.len()>63||
     !label[0].is_ascii_alphanumeric()||
     !label[label.len()-1].is_ascii_alphanumeric()||
     !label.iter().all(|b|b.is_ascii_alphanumeric()||*b==b'-'){
   return false;
  }
 }
 true
}
fn be16(b:&[u8])->u16{u16::from_be_bytes([b[0],b[1]])}
pub fn parse_dns_a(id:u16,domain:&[u8],wire:&[u8])->Result<[u8;4],Error>{
 if !valid_domain(domain)||wire.len()<12||wire.len()>512||
  be16(&wire[..2])!=id{return Err(Error::Malformed);}
 let flags=be16(&wire[2..4]);
 if flags&0x8000==0||flags&0x7800!=0||flags&0x0270!=0||
  flags&0x000f!=0||be16(&wire[4..6])!=1||
  be16(&wire[6..8])!=1||be16(&wire[8..10])!=0||
  be16(&wire[10..12])!=0{return Err(Error::Malformed);}
 let mut i=12usize;
 for label in domain.split(|b|*b==b'.'){
  if i>=wire.len()||wire[i] as usize!=label.len()||
   wire.len()-i<1+label.len(){return Err(Error::Malformed);}
  i+=1;
  if wire[i..i+label.len()]!=*label{return Err(Error::Malformed);}
  i+=label.len();
 }
 if i>=wire.len()||wire[i]!=0{return Err(Error::Malformed);}
 i+=1;
 if wire.len()-i<4||be16(&wire[i..i+2])!=1||
   be16(&wire[i+2..i+4])!=1{return Err(Error::Malformed);}
 i+=4;
 // Exactly one bounded answer: owner-name compression to question only.
 if wire.len()-i<16||wire[i]!=0xc0||wire[i+1]!=0x0c||
  be16(&wire[i+2..i+4])!=1||be16(&wire[i+4..i+6])!=1||
  be16(&wire[i+10..i+12])!=4{return Err(Error::Malformed);}
 let ttl=u32::from_be_bytes(wire[i+6..i+10].try_into().unwrap());
 if ttl==0||wire.len()!=i+16{return Err(Error::Malformed);}
 let address:[u8;4]=wire[i+12..i+16].try_into()
  .map_err(|_|Error::Malformed)?;
 // Outbound unicast endpoints cannot be unspecified, multicast, limited
 // broadcast or reserved-class addresses.
 if address[0]==0||address[0]>=224{return Err(Error::Malformed);}
 Ok(address)
}
/// Bounded TCP retransmission *policy*, not a TCP socket or TLS record layer.
/// An authenticated ingress layer must verify source/destination address,
/// ports, checksums and ACK plausibility BEFORE passing an ACK here.
pub const MAX_TCP_RETRIES:u8=3;
#[derive(Debug,PartialEq,Eq)]
pub struct TcpFlight{
 owner:Identity,local_port:u16,expected_ack:u32,
 retry_deadline:u64,timeout:u64,retries:u8,acknowledged:bool,failed:bool,
}
impl TcpFlight{
 pub fn retries(&self)->u8{self.retries}
 pub fn complete(&self)->bool{self.acknowledged}
}
impl<const P:usize> Endpoint<P>{
 /// Sequence wraparound is rejected in this bounded initial implementation.
 /// No transmit occurs; the trusted network driver owns actual packet I/O.
 pub fn track_tcp_segment(&self,owner:Identity,local_port:u16,
  now:u64,sequence:u32,payload_bytes:u16,timeout:u64)->Result<TcpFlight,Error>{
  self.ports.require(owner,local_port,now).map_err(|_|Error::Denied)?;
  if payload_bytes==0||timeout==0{return Err(Error::Invalid);}
  let ack=sequence.checked_add(payload_bytes as u32).ok_or(Error::Invalid)?;
  let deadline=now.checked_add(timeout).ok_or(Error::Invalid)?;
  Ok(TcpFlight{owner,local_port,expected_ack:ack,retry_deadline:deadline,
    timeout,retries:0,acknowledged:false,failed:false})
 }
 /// 'verified_ack' MUST come from a trusted, parsed and peer-authenticated
 /// transport ingress, never directly from untrusted user parameters.
 pub fn accept_tcp_ack(&self,owner:Identity,local_port:u16,now:u64,
  flight:&mut TcpFlight,verified_ack:u32)->Result<(),Error>{
  self.ports.require(owner,local_port,now).map_err(|_|Error::Denied)?;
  if flight.owner!=owner||flight.local_port!=local_port||
     flight.acknowledged||flight.failed||verified_ack!=flight.expected_ack{
   return Err(Error::Denied);
  }
  flight.acknowledged=true;
  Ok(())
 }
 /// Returns true exactly when the caller should retry its buffered payload.
 /// The caller must still enforce source authenticity, flow identity and I/O.
 pub fn retry_tcp_segment(&self,owner:Identity,local_port:u16,
  now:u64,flight:&mut TcpFlight)->Result<bool,Error>{
  self.ports.require(owner,local_port,now).map_err(|_|Error::Denied)?;
  if flight.owner!=owner||flight.local_port!=local_port{return Err(Error::Denied);}
  if flight.failed{return Err(Error::RetryExhausted);}
  if flight.acknowledged||now<flight.retry_deadline{return Ok(false);}
  if flight.retries==MAX_TCP_RETRIES{
   flight.failed=true;
   return Err(Error::RetryExhausted);
  }
  let deadline=now.checked_add(flight.timeout).ok_or(Error::Invalid)?;
  flight.retries+=1;flight.retry_deadline=deadline;
  Ok(true)
 }
}
/// Peer-bound in-flight segment tracking for a real transport ingress.
 /// Caller still MUST validate the received TCP/IPv4 checksum, packet
 /// source/destination pair and TCP header before forwarding acknowledgments.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct TcpPeer{pub address:[u8;4],pub port:u16}
impl TcpPeer{
 fn valid(self)->bool{
  self.port!=0&&self.address!=[0;4]&&self.address!=[255;4]&&
  self.address[0]!=0&&self.address[0]<224
 }
}
#[derive(Debug,PartialEq,Eq)]
pub struct PeerFlight{peer:TcpPeer,flight:TcpFlight}
impl PeerFlight{
 pub fn complete(&self)->bool{self.flight.complete()}
 pub fn retries(&self)->u8{self.flight.retries()}
}
impl<const P:usize> Endpoint<P>{
 pub fn track_peer_segment(&self,owner:Identity,local_port:u16,
  peer:TcpPeer,now:u64,sequence:u32,bytes:u16,timeout:u64)
  ->Result<PeerFlight,Error>{
  if !peer.valid(){return Err(Error::Invalid);}
  let flight=self.track_tcp_segment(owner,local_port,now,sequence,bytes,timeout)?;
  Ok(PeerFlight{peer,flight})
 }
 /// Only call after the transport authenticated the sender peer and ACK.
 pub fn accept_peer_ack(&self,owner:Identity,local_port:u16,
  peer:TcpPeer,now:u64,flight:&mut PeerFlight,ack:u32)
  ->Result<(),Error>{
  if flight.peer!=peer{return Err(Error::Denied);}
  self.accept_tcp_ack(owner,local_port,now,&mut flight.flight,ack)
 }
 pub fn retry_peer_segment(&self,owner:Identity,local_port:u16,
  peer:TcpPeer,now:u64,flight:&mut PeerFlight)->Result<bool,Error>{
  if flight.peer!=peer{return Err(Error::Denied);}
  self.retry_tcp_segment(owner,local_port,now,&mut flight.flight)
 }
}
/// Transport-independent TCP client SYN handshake control state.
/// The network driver must validate IPv4/TCP checksums, source addresses and
/// peer identity before invoking accept_syn_ack. No packets are sent here.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum TcpState{SynSent,Established,Closing,Closed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct TcpHandshake{
 owner:Identity,local_port:u16,remote:TcpPeer,
 state:TcpState,local_next:u32,remote_next:u32,
 retry_at:u64,interval:u64,retries:u8,
}
impl TcpHandshake{
 pub fn state(&self)->TcpState{self.state}
 pub fn retry_count(&self)->u8{self.retries}
 pub fn next_local_sequence(&self)->u32{self.local_next}
 pub fn next_remote_sequence(&self)->u32{self.remote_next}
}
impl<const P:usize> Endpoint<P>{
 pub fn begin_tcp_connect(&self,actor:Identity,port:u16,peer:TcpPeer,
   now:u64,initial_sequence:u32,timeout:u64)->Result<TcpHandshake,Error>{
  self.ports.require(actor,port,now).map_err(|_|Error::Denied)?;
  if !peer.valid()||timeout==0{return Err(Error::Invalid);}
  let next=initial_sequence.checked_add(1).ok_or(Error::Invalid)?;
  let retry_at=now.checked_add(timeout).ok_or(Error::Invalid)?;
  Ok(TcpHandshake{owner:actor,local_port:port,remote:peer,
    state:TcpState::SynSent,local_next:next,remote_next:0,
    retry_at,interval:timeout,retries:0})
 }
 /// Peer+sequence proof is mandatory even if the claimed ACK looks valid.
 /// A SYN|ACK TCP frame must have passed checksum and network address checks.
 pub fn accept_tcp_syn_ack(&self,actor:Identity,port:u16,peer:TcpPeer,
   now:u64,connection:&mut TcpHandshake,remote_sequence:u32,ack:u32)
   ->Result<(),Error>{
  self.ports.require(actor,port,now).map_err(|_|Error::Denied)?;
  if actor!=connection.owner||port!=connection.local_port||
     peer!=connection.remote||connection.state!=TcpState::SynSent||
     ack!=connection.local_next{return Err(Error::Denied);}
  let next=remote_sequence.checked_add(1).ok_or(Error::Invalid)?;
  connection.remote_next=next;
  connection.state=TcpState::Established;
  Ok(())
 }
 /// True means the driver should retransmit the original SYN packet.
 pub fn retry_tcp_syn(&self,actor:Identity,port:u16,peer:TcpPeer,
   now:u64,connection:&mut TcpHandshake)->Result<bool,Error>{
  self.ports.require(actor,port,now).map_err(|_|Error::Denied)?;
  if actor!=connection.owner||port!=connection.local_port||
    peer!=connection.remote{return Err(Error::Denied);}
  if connection.state==TcpState::Established{return Ok(false);}
  if connection.state!=TcpState::SynSent{return Err(Error::Denied);}
  if now<connection.retry_at{return Ok(false);}
  if connection.retries>=MAX_TCP_RETRIES{
   connection.state=TcpState::Closed;
   return Err(Error::RetryExhausted);
  }
  let next=now.checked_add(connection.interval).ok_or(Error::Invalid)?;
  connection.retries+=1;connection.retry_at=next;
  Ok(true)
 }
 /// Explicit local closure is owner scoped and invalidates the connection.
 pub fn close_tcp_connection(&self,actor:Identity,port:u16,
   connection:&mut TcpHandshake)->Result<(),Error>{
  if actor!=connection.owner||port!=connection.local_port||
    connection.state==TcpState::Closed{return Err(Error::Denied);}
  connection.state=TcpState::Closed;
  Ok(())
 }
}

/// Internet one's-complement checksum, including odd trailing bytes.
impl<const P:usize> Endpoint<P>{
 /// Complete a connection only after parsing and checksumming the actual
 /// IPv4/TCP SYN|ACK bytes. Peer address and port, leased local port,
 /// authenticated process generation and expected TCP ACK must all match.
 pub fn accept_verified_syn_ack(&self,actor:Identity,local_port:u16,
   peer:TcpPeer,local_ip:[u8;4],now:u64,
   connection:&mut TcpHandshake,wire:&[u8])->Result<(),Error>{
  let (remote_seq,acked)=parse_ipv4_syn_ack(wire,peer,local_ip,local_port)?;
  self.accept_tcp_syn_ack(actor,local_port,peer,now,connection,remote_seq,acked)
 }
}
fn internet_sum(data:&[u8])->u32{
 let mut sum=0u32;
 for pair in data.chunks(2){
  let word=(u16::from(pair[0])<<8)|u16::from(*pair.get(1).unwrap_or(&0));
  sum+=u32::from(word);
 }
 sum
}
fn checked_checksum(mut sum:u32)->u16{
 while sum>>16!=0{sum=(sum&0xffff)+(sum>>16);}
 !(sum as u16)
}
/// Validate an exact IPv4 TCP SYN|ACK datagram from a specific peer.
/// No IPv4 options/fragments or TCP options in this narrow first wire slice.
/// A caller must still authenticate NIC receive ownership and routing; this
/// decoder does not open a socket or emit ACK packets.
pub fn parse_ipv4_syn_ack(
 packet:&[u8],peer:TcpPeer,local_ip:[u8;4],local_port:u16,
)->Result<(u32,u32),Error>{
 if packet.len()!=40||!peer.valid()||local_ip==[0;4]||local_port==0||
   packet[0]!=0x45||packet[9]!=6||packet[8]==0||
   u16::from_be_bytes([packet[2],packet[3]]) as usize!=packet.len()||
   u16::from_be_bytes([packet[6],packet[7]])&0x3fff!=0||
   packet[12..16]!=peer.address||packet[16..20]!=local_ip{
  return Err(Error::Malformed);
 }
 if checked_checksum(internet_sum(&packet[..20]))!=0{
  return Err(Error::Malformed);
 }
 let tcp=&packet[20..];
 let source=u16::from_be_bytes([tcp[0],tcp[1]]);
 let dest=u16::from_be_bytes([tcp[2],tcp[3]]);
 if source!=peer.port||dest!=local_port||tcp[12]!=0x50||
    tcp[13]!=0x12||tcp[18]!=0||tcp[19]!=0{return Err(Error::Malformed);}
 let sum=internet_sum(&packet[12..20])+6+tcp.len() as u32+
   internet_sum(tcp);
 if checked_checksum(sum)!=0{return Err(Error::Malformed);}
 let seq=u32::from_be_bytes(tcp[4..8].try_into().map_err(|_|Error::Malformed)?);
 let ack=u32::from_be_bytes(tcp[8..12].try_into().map_err(|_|Error::Malformed)?);
 Ok((seq,ack))
}

/// Parse a complete IPv4/UDP DNS A response from actual received packet bytes.
/// This validates packet structure, exact endpoints, IP and UDP checksums,
/// DNS transaction/question and bounded answer. A UDP checksum is not
/// cryptographic source authentication: the NIC/route ownership and the
/// legitimacy of the configured DNS server remain transport responsibilities.
pub fn parse_ipv4_udp_dns_a(
 wire:&[u8],server_ip:[u8;4],client_ip:[u8;4],client_port:u16,
 transaction_id:u16,domain:&[u8],
)->Result<[u8;4],Error>{
 if wire.len()<40||wire.len()>540||server_ip==[0;4]||
    client_ip==[0;4]||client_port==0||wire[0]!=0x45||
    wire[8]==0||wire[9]!=17||
    u16::from_be_bytes([wire[2],wire[3]]) as usize!=wire.len()||
    u16::from_be_bytes([wire[6],wire[7]])&0x3fff!=0||
    wire[12..16]!=server_ip||wire[16..20]!=client_ip||
    checked_checksum(internet_sum(&wire[..20]))!=0{
  return Err(Error::Malformed);
 }
 let udp=&wire[20..];
 if u16::from_be_bytes([udp[0],udp[1]])!=53||
    u16::from_be_bytes([udp[2],udp[3]])!=client_port||
    u16::from_be_bytes([udp[4],udp[5]]) as usize!=udp.len()||
    (udp[6]==0&&udp[7]==0){
  return Err(Error::Malformed);
 }
 // IPv4 UDP pseudoheader = source + destination + zero/protocol + length.
 let sum=internet_sum(&wire[12..20])+17+udp.len() as u32+
   internet_sum(udp);
 if checked_checksum(sum)!=0{return Err(Error::Malformed);}
 parse_dns_a(transaction_id,domain,&udp[8..])
}
impl<const P:usize> Endpoint<P>{
 /// The caller supplies a NIC-received datagram, not a caller-fabricated DNS
 /// payload. Process/port lease must still be valid at the receipt time.
 pub fn accept_verified_dns_ipv4(&self,owner:Identity,local_port:u16,
   now:u64,server_ip:[u8;4],client_ip:[u8;4],
   transaction_id:u16,name:&[u8],wire:&[u8])->Result<[u8;4],Error>{
  self.allow_query(owner,local_port,now,name)?;
  parse_ipv4_udp_dns_a(wire,server_ip,client_ip,local_port,transaction_id,name)
 }
}

#[cfg(test)]
mod m2_tests{
 use super::*;
 struct Deny;impl TlsCertificateVerifier for Deny{
  fn verify_chain_and_name(&self,_:&[u8],_:&[u8])->bool{false}
 }
 fn packet()->[u8;45]{
  let mut p=[0u8;45];
  p[..12].copy_from_slice(&[0x12,0x34,0x81,0x80,0,1,0,1,0,0,0,0]);
  p[12..25].copy_from_slice(b"\x07example\x03com\x00");
  p[25..29].copy_from_slice(&[0,1,0,1]);
  p[29..].copy_from_slice(&[0xc0,0x0c,0,1,0,1,0,0,0,60,0,4,203,0,113,7]);
  p
 }
 #[test]fn m2_valid_dns_and_wrong_txid_rejected(){
  let p=packet();
  assert_eq!(parse_dns_a(0x1234,b"example.com",&p),Ok([203,0,113,7]));
  assert_eq!(parse_dns_a(9,b"example.com",&p),Err(Error::Malformed));
  let mut altered=p;altered[29]=0xff;
  assert_eq!(parse_dns_a(0x1234,b"example.com",&altered),Err(Error::Malformed));
  let mut changed=p;changed[40]=0;
  assert_eq!(parse_dns_a(0x1234,b"example.com",&changed),Err(Error::Malformed));
 }
 #[test]fn m2_dns_rejects_extra_answers_trailing_bytes_and_reserved_flags(){
  let valid=packet();
  let mut extra=valid;extra[7]=2;
  assert_eq!(parse_dns_a(0x1234,b"example.com",&extra),Err(Error::Malformed));
  let mut reserved=valid;reserved[3]|=0x40;
  assert_eq!(parse_dns_a(0x1234,b"example.com",&reserved),Err(Error::Malformed));
  let mut trailing=[0u8;46];trailing[..45].copy_from_slice(&valid);
  assert_eq!(parse_dns_a(0x1234,b"example.com",&trailing),Err(Error::Malformed));
 }
 #[test]fn r055_dns_rejects_unicast_incompatible_a_records(){
  for addr in [[0,0,0,0],[224,0,0,1],[239,1,2,3],
    [240,0,0,1],[255,255,255,255]]{
   let mut p=packet();p[41..45].copy_from_slice(&addr);
   assert_eq!(parse_dns_a(0x1234,b"example.com",&p),
     Err(Error::Malformed));
  }
  let mut p=packet();p[41..45].copy_from_slice(&[1,1,1,1]);
  assert_eq!(parse_dns_a(0x1234,b"example.com",&p),Ok([1,1,1,1]));
 }
 #[test]fn m2_tcp_retry_budget_and_exact_ack_are_identity_bound(){
  let owner=Identity{pid:7,generation:2};
  let stale=Identity{generation:3,..owner};
  let mut auth=Access::<1>::new();auth.grant(owner,SOCKET,100).unwrap();
  let mut sock=Endpoint::<1>::new();
  sock.acquire(&auth,owner,6040,1,90).unwrap();
  let mut flight=sock.track_tcp_segment(owner,6040,1,100,8,5).unwrap();
  assert_eq!(sock.accept_tcp_ack(stale,6040,2,&mut flight,108),Err(Error::Denied));
  assert_eq!(sock.accept_tcp_ack(owner,6040,2,&mut flight,107),Err(Error::Denied));
  assert_eq!(sock.retry_tcp_segment(owner,6040,5,&mut flight),Ok(false));
  for (time,tries) in [(6,1),(11,2),(16,3)]{
   assert_eq!(sock.retry_tcp_segment(owner,6040,time,&mut flight),Ok(true));
   assert_eq!(flight.retries(),tries);
  }
  assert_eq!(sock.retry_tcp_segment(owner,6040,21,&mut flight),
    Err(Error::RetryExhausted));
  assert_eq!(sock.accept_tcp_ack(owner,6040,21,&mut flight,108),Err(Error::Denied));
  let mut received=sock.track_tcp_segment(owner,6040,22,200,4,5).unwrap();
  assert_eq!(sock.accept_tcp_ack(owner,6040,23,&mut received,204),Ok(()));
  assert!(received.complete());
  assert_eq!(sock.retry_tcp_segment(owner,6040,28,&mut received),Ok(false));
 }
 #[test]fn m2_tcp_rejects_invalid_range_and_expired_socket(){
  let owner=Identity{pid:7,generation:2};
  let mut auth=Access::<1>::new();auth.grant(owner,SOCKET,100).unwrap();
  let mut sock=Endpoint::<1>::new();
  sock.acquire(&auth,owner,6040,1,30).unwrap();
  assert_eq!(sock.track_tcp_segment(owner,6040,1,u32::MAX,2,5),
    Err(Error::Invalid));
  assert_eq!(sock.track_tcp_segment(owner,6040,1,100,0,5),
    Err(Error::Invalid));
  assert_eq!(sock.track_tcp_segment(owner,6040,u64::MAX,1,1,3),
    Err(Error::Denied));
  let mut flight=sock.track_tcp_segment(owner,6040,2,100,3,3).unwrap();
  assert_eq!(sock.retry_tcp_segment(owner,6040,31,&mut flight),Err(Error::Denied));
  assert_eq!(sock.accept_tcp_ack(owner,6040,31,&mut flight,103),Err(Error::Denied));
 }
 #[test]fn m2_peer_flight_requires_exact_remote_address_port_and_owner(){
  let owner=Identity{pid:7,generation:2};
  let stale=Identity{generation:3,..owner};
  let mut auth=Access::<1>::new();auth.grant(owner,SOCKET,100).unwrap();
  let mut endpoint=Endpoint::<1>::new();
  endpoint.acquire(&auth,owner,6040,1,50).unwrap();
  let peer=TcpPeer{address:[203,0,113,10],port:443};
  let imposter=TcpPeer{address:[203,0,113,11],port:443};
  let mut flight=endpoint.track_peer_segment(owner,6040,peer,
    1,500,5,5).unwrap();
  assert_eq!(endpoint.accept_peer_ack(owner,6040,imposter,2,
    &mut flight,505),Err(Error::Denied));
  assert_eq!(endpoint.accept_peer_ack(stale,6040,peer,2,
    &mut flight,505),Err(Error::Denied));
  assert_eq!(endpoint.retry_peer_segment(owner,6040,imposter,6,
    &mut flight),Err(Error::Denied));
  assert_eq!(endpoint.retry_peer_segment(owner,6040,peer,6,
    &mut flight),Ok(true));
  assert_eq!(flight.retries(),1);
  assert_eq!(endpoint.accept_peer_ack(owner,6040,peer,7,
    &mut flight,505),Ok(()));
  assert!(flight.complete());
  assert_eq!(endpoint.retry_peer_segment(owner,6040,peer,11,
    &mut flight),Ok(false));
  endpoint.retire(owner);
  assert_eq!(endpoint.accept_peer_ack(owner,6040,peer,12,
    &mut flight,505),Err(Error::Denied));
 }
 #[test]fn m2_peer_flight_rejects_nonunicast_destination_and_overflow(){
  let owner=Identity{pid:7,generation:2};
  let mut auth=Access::<1>::new();auth.grant(owner,SOCKET,100).unwrap();
  let mut endpoint=Endpoint::<1>::new();
  endpoint.acquire(&auth,owner,6040,1,50).unwrap();
  for addr in [[0;4],[255;4],[224,1,2,3],[0,1,2,3]]{
   assert_eq!(endpoint.track_peer_segment(owner,6040,TcpPeer{
      address:addr,port:443},1,5,1,1),Err(Error::Invalid));
  }
  let peer=TcpPeer{address:[203,0,113,10],port:443};
  assert_eq!(endpoint.track_peer_segment(owner,6040,TcpPeer{
   port:0,..peer},1,5,1,1),Err(Error::Invalid));
  assert_eq!(endpoint.track_peer_segment(owner,6040,peer,
   1,u32::MAX,2,1),Err(Error::Invalid));
 }
 #[test]fn m2_socket_lease_stale_owner_and_tls_fail_closed(){
  let a=Identity{pid:7,generation:1};
  let mut auth=Access::<1>::new();auth.grant(a,SOCKET,100).unwrap();
  let mut sock=Endpoint::<1>::new();
  sock.acquire(&auth,a,6000,1,40).unwrap();
  assert_eq!(sock.accept_dns_a(a,6000,1,0x1234,b"example.com",&packet()),
   Ok([203,0,113,7]));
  assert_eq!(sock.accept_dns_a(Identity{generation:2,..a},6000,1,
   0x1234,b"example.com",&packet()),Err(Error::Denied));
  assert_eq!(sock.allow_tls(a,6000,1,b"example.com",b"cert",&Deny),
   Err(Error::Unverified));
  sock.retire(a);assert_eq!(sock.allow_query(a,6000,2,b"example.com"),Err(Error::Denied));
 }

 #[test]fn tcp_connect_requires_verified_peer_and_exact_ack(){
  let owner=Identity{pid:11,generation:4};
  let mut acl=Access::<1>::new();acl.grant(owner,SOCKET,200).unwrap();
  let mut endpoint=Endpoint::<1>::new();
  endpoint.acquire(&acl,owner,51000,1,150).unwrap();
  let peer=TcpPeer{address:[192,0,2,10],port:443};
  let wrong=TcpPeer{address:[192,0,2,11],port:443};
  let mut conn=endpoint.begin_tcp_connect(owner,51000,peer,2,400,5).unwrap();
  assert_eq!(conn.state(),TcpState::SynSent);
  assert_eq!(endpoint.accept_tcp_syn_ack(owner,51000,wrong,3,&mut conn,900,401),
    Err(Error::Denied));
  assert_eq!(endpoint.accept_tcp_syn_ack(owner,51000,peer,3,&mut conn,900,400),
    Err(Error::Denied));
  let stale=Identity{generation:5,..owner};
  assert_eq!(endpoint.accept_tcp_syn_ack(stale,51000,peer,3,&mut conn,900,401),
    Err(Error::Denied));
  assert_eq!(conn.state(),TcpState::SynSent);
  endpoint.accept_tcp_syn_ack(owner,51000,peer,3,&mut conn,900,401).unwrap();
  assert_eq!(conn.state(),TcpState::Established);
  assert_eq!(conn.next_remote_sequence(),901);
  assert_eq!(endpoint.retry_tcp_syn(owner,51000,peer,10,&mut conn),Ok(false));
  endpoint.close_tcp_connection(owner,51000,&mut conn).unwrap();
  assert_eq!(conn.state(),TcpState::Closed);
  assert!(endpoint.accept_tcp_syn_ack(owner,51000,peer,11,&mut conn,901,401).is_err());
 }
 #[test]fn tcp_syn_retries_stop_and_socket_expiry_prevents_reuse(){
  let owner=Identity{pid:11,generation:4};
  let mut acl=Access::<1>::new();acl.grant(owner,SOCKET,200).unwrap();
  let mut endpoint=Endpoint::<1>::new();
  endpoint.acquire(&acl,owner,51000,1,21).unwrap();
  let peer=TcpPeer{address:[192,0,2,10],port:443};
  let mut conn=endpoint.begin_tcp_connect(owner,51000,peer,1,10,4).unwrap();
  assert_eq!(endpoint.retry_tcp_syn(owner,51000,peer,4,&mut conn),Ok(false));
  for (tick,n) in [(5,1),(9,2),(13,3)]{
   assert_eq!(endpoint.retry_tcp_syn(owner,51000,peer,tick,&mut conn),Ok(true));
   assert_eq!(conn.retry_count(),n);
  }
  assert_eq!(endpoint.retry_tcp_syn(owner,51000,peer,17,&mut conn),
    Err(Error::RetryExhausted));
  assert_eq!(conn.state(),TcpState::Closed);
  let mut later=endpoint.begin_tcp_connect(owner,51000,peer,18,25,5).unwrap();
  assert_eq!(endpoint.accept_tcp_syn_ack(owner,51000,peer,21,&mut later,700,26),
    Err(Error::Denied));
  endpoint.retire(owner);
  assert_eq!(endpoint.retry_tcp_syn(owner,51000,peer,20,&mut later),
    Err(Error::Denied));
 }


 fn tcp_handshake_packet()->[u8;40]{
  let mut p=[0u8;40];
  p[0]=0x45;p[2..4].copy_from_slice(&40u16.to_be_bytes());
  p[8]=64;p[9]=6;
  p[12..16].copy_from_slice(&[192,0,2,10]);
  p[16..20].copy_from_slice(&[192,0,2,20]);
  p[20..22].copy_from_slice(&443u16.to_be_bytes());
  p[22..24].copy_from_slice(&51000u16.to_be_bytes());
  p[24..28].copy_from_slice(&900u32.to_be_bytes());
  p[28..32].copy_from_slice(&401u32.to_be_bytes());
  p[32]=0x50;p[33]=0x12;p[34..36].copy_from_slice(&4096u16.to_be_bytes());
  let ip=checked_checksum(internet_sum(&p[..20]));
  p[10..12].copy_from_slice(&ip.to_be_bytes());
  let tcp=checked_checksum(internet_sum(&p[12..20])+6+20+
    internet_sum(&p[20..]));
  p[36..38].copy_from_slice(&tcp.to_be_bytes());
  p
 }
 #[test]fn verified_ipv4_tcp_synack_decodes_sequence_and_ack(){
  let p=tcp_handshake_packet();
  let peer=TcpPeer{address:[192,0,2,10],port:443};
  assert_eq!(parse_ipv4_syn_ack(&p,peer,[192,0,2,20],51000),Ok((900,401)));
 }
 #[test]fn spoofed_peer_corrupt_checksum_and_fragmented_ipv4_are_rejected(){
  let p=tcp_handshake_packet();
  let peer=TcpPeer{address:[192,0,2,10],port:443};
  let spoof=TcpPeer{address:[192,0,2,11],port:443};
  assert_eq!(parse_ipv4_syn_ack(&p,spoof,[192,0,2,20],51000),
    Err(Error::Malformed));
  let mut bad=p;bad[28]^=1;
  assert_eq!(parse_ipv4_syn_ack(&bad,peer,[192,0,2,20],51000),
    Err(Error::Malformed));
  let mut bad=p;bad[6]=0x20;
  assert_eq!(parse_ipv4_syn_ack(&bad,peer,[192,0,2,20],51000),
    Err(Error::Malformed));
  let mut bad=p;bad[33]=0x10;
  assert_eq!(parse_ipv4_syn_ack(&bad,peer,[192,0,2,20],51000),
    Err(Error::Malformed));
 }


 #[test]fn verified_tcp_packet_must_match_handshake_sequence_owner_and_port(){
  let actor=Identity{pid:8,generation:4};
  let mut rights=Access::<1>::new();rights.grant(actor,SOCKET,100).unwrap();
  let mut ep=Endpoint::<1>::new();
  ep.acquire(&rights,actor,51000,1,90).unwrap();
  let peer=TcpPeer{address:[192,0,2,10],port:443};
  let mut conn=ep.begin_tcp_connect(actor,51000,peer,1,400,5).unwrap();
  let wire=tcp_handshake_packet();
  assert_eq!(ep.accept_verified_syn_ack(actor,51000,peer,[192,0,2,20],
     2,&mut conn,&wire),Ok(()));
  assert_eq!(conn.state(),TcpState::Established);
  assert_eq!(conn.next_remote_sequence(),901);
 }
 #[test]fn stale_identity_and_checksum_corruption_cannot_open_connection(){
  let actor=Identity{pid:8,generation:4};
  let mut rights=Access::<1>::new();rights.grant(actor,SOCKET,100).unwrap();
  let mut ep=Endpoint::<1>::new();
  ep.acquire(&rights,actor,51000,1,90).unwrap();
  let peer=TcpPeer{address:[192,0,2,10],port:443};
  let mut conn=ep.begin_tcp_connect(actor,51000,peer,1,400,5).unwrap();
  let mut wire=tcp_handshake_packet();wire[33]^=1;
  assert_eq!(ep.accept_verified_syn_ack(actor,51000,peer,
     [192,0,2,20],2,&mut conn,&wire),Err(Error::Malformed));
  assert_eq!(conn.state(),TcpState::SynSent);
  assert_eq!(ep.accept_verified_syn_ack(Identity{generation:5,..actor},
     51000,peer,[192,0,2,20],2,&mut conn,&tcp_handshake_packet()),
     Err(Error::Denied));
  assert_eq!(conn.state(),TcpState::SynSent);
 }

}

 
/// R009 fixed-capacity, generation-scoped cache for already authenticated DNS
/// A replies. A hit rechecks the live socket lease; stale processes cannot
/// keep network results after their capability is revoked.
#[derive(Clone,Copy)]
struct CachedA{
 owner:Identity,local_port:u16,name:[u8;253],len:usize,
 ip:[u8;4],expires:u64,
}
pub struct DnsLeaseCache<const N:usize>{
 entries:[Option<CachedA>;N],next:usize,
}
impl<const N:usize> DnsLeaseCache<N>{
 pub const fn new()->Self{Self{entries:[None;N],next:0}}
 pub fn resolve_verified<const P:usize>(
  &mut self,endpoint:&Endpoint<P>,owner:Identity,local_port:u16,
  now:u64,id:u16,name:&[u8],packet:&[u8])->Result<[u8;4],Error>{
  let ip=endpoint.accept_dns_a(owner,local_port,now,id,name,packet)?;
  if N==0{return Ok(ip);}
  // The strict DNS parser has already validated the entire answer.
  let mut off=12usize;
  for label in name.split(|b|*b==b'.'){off+=1+label.len();}
  off+=1+4;
  let ttl=u32::from_be_bytes(packet[off+6..off+10].try_into()
    .map_err(|_|Error::Malformed)?).min(120);
  let expires=now.checked_add(ttl as u64).ok_or(Error::Invalid)?;
  let mut fixed=[0u8;253];fixed[..name.len()].copy_from_slice(name);
  let entry=CachedA{owner,local_port,name:fixed,len:name.len(),ip,expires};
  let index=self.entries.iter().position(|saved|saved.is_some_and(|x|
    x.owner==owner&&x.local_port==local_port&&
    x.len==name.len()&&x.name[..x.len]==*name))
    .unwrap_or(self.next);
  self.entries[index]=Some(entry);self.next=(index+1)%N;
  Ok(ip)
 }
 pub fn cached<const P:usize>(&self,endpoint:&Endpoint<P>,
  owner:Identity,local_port:u16,now:u64,name:&[u8])
  ->Result<Option<[u8;4]>,Error>{
  endpoint.allow_query(owner,local_port,now,name)?;
  Ok(self.entries.iter().flatten().find(|x|
    x.owner==owner&&x.local_port==local_port&&now<x.expires&&
    x.len==name.len()&&x.name[..x.len]==*name).map(|x|x.ip))
 }
 pub fn retire(&mut self,owner:Identity){
  for e in &mut self.entries{
   if e.is_some_and(|x|x.owner==owner){*e=None;}
  }
 }
}
#[cfg(test)]
mod r009_dns_lease_cache_tests{
 use super::*;
 fn owner()->Identity{Identity{pid:75,generation:2}}
 fn answer()->[u8;38]{
  [0x12,0x34,0x81,0x80,0,1,0,1,0,0,0,0,
   1,b'a',2,b'i',b'o',0,0,1,0,1,
   0xc0,0x0c,0,1,0,1,0,0,0,30,0,4,1,2,3,4]
 }
 fn socket()->Endpoint<1>{
  let mut endpoint=Endpoint::new();
  let mut a=Access::<1>::new();a.grant(owner(),SOCKET,100).unwrap();
  endpoint.acquire(&a,owner(),1100,1,90).unwrap();endpoint
 }
 #[test]fn valid_answer_caches_then_expires(){
  let endpoint=socket();let mut cache=DnsLeaseCache::<2>::new();
  assert_eq!(cache.resolve_verified(&endpoint,owner(),1100,2,0x1234,
    b"a.io",&answer()),Ok([1,2,3,4]));
  assert_eq!(cache.cached(&endpoint,owner(),1100,20,b"a.io"),Ok(Some([1,2,3,4])));
  assert_eq!(cache.cached(&endpoint,owner(),1100,32,b"a.io"),Ok(None));
 }
 #[test]fn foreign_generation_and_revocation_do_not_reuse_cache(){
  let mut endpoint=socket();let mut cache=DnsLeaseCache::<1>::new();
  cache.resolve_verified(&endpoint,owner(),1100,2,0x1234,b"a.io",&answer()).unwrap();
  assert_eq!(cache.cached(&endpoint,Identity{generation:3,..owner()},1100,3,b"a.io"),
    Err(Error::Denied));
  endpoint.retire(owner());
  assert_eq!(cache.cached(&endpoint,owner(),1100,3,b"a.io"),Err(Error::Denied));
  cache.retire(owner());
 }
 #[test]fn malformed_answer_never_replaces_cached_valid_address(){
  let endpoint=socket();let mut cache=DnsLeaseCache::<1>::new();
  cache.resolve_verified(&endpoint,owner(),1100,2,0x1234,b"a.io",&answer()).unwrap();
  let mut corrupt=answer();corrupt[0]=0;
  assert_eq!(cache.resolve_verified(&endpoint,owner(),1100,3,0x1234,
     b"a.io",&corrupt),Err(Error::Malformed));
  assert_eq!(cache.cached(&endpoint,owner(),1100,3,b"a.io"),Ok(Some([1,2,3,4])));
 }
}

 
/// R010 cache lifecycle control: purge stale TTL entries and invalidate
/// response records when a trusted resolver reconnects or its lease changes.
impl<const N:usize> DnsLeaseCache<N>{
 pub fn sweep_expired(&mut self,now:u64)->usize{
  let mut removed=0;
  for entry in &mut self.entries{
   if entry.is_some_and(|x|now>=x.expires){*entry=None;removed+=1;}
  }
  removed
 }
 pub fn invalidate_name(&mut self,owner:Identity,
  local_port:u16,name:&[u8])->usize{
  if owner.pid==0||owner.generation==0||local_port==0||
     !valid_domain(name){return 0;}
  let mut removed=0;
  for entry in &mut self.entries{
   if entry.is_some_and(|x|x.owner==owner&&x.local_port==local_port&&
       x.len==name.len()&&x.name[..x.len]==*name){
    *entry=None;removed+=1;
   }
  }
  removed
 }
 pub fn entries_active(&self,now:u64)->usize{
  self.entries.iter().flatten().filter(|entry|entry.expires>now).count()
 }
}
#[cfg(test)]
mod r010_dns_cache_hardening_tests{
 use super::*;
 #[test]fn unsafe_label_boundaries_never_enter_dns_lookup(){
  assert!(!valid_domain(b"-host.example"));
  assert!(!valid_domain(b"host-.example"));
  assert!(!valid_domain(b"host..example"));
  assert!(!valid_domain(b"host.example."));
  assert!(valid_domain(b"safe-host.example"));
 }
 #[test]fn expiry_and_exact_name_invalidation_preserve_peers(){
  let a=Identity{pid:7,generation:2};
  let b=Identity{pid:8,generation:2};
  let mut cache=DnsLeaseCache::<3>::new();
  let mut name=[0u8;253];name[..4].copy_from_slice(b"a.io");
  cache.entries[0]=Some(CachedA{owner:a,local_port:5000,
    name,len:4,ip:[1,2,3,4],expires:40});
  cache.entries[1]=Some(CachedA{owner:b,local_port:5000,
    name,len:4,ip:[8,8,8,8],expires:80});
  assert_eq!(cache.invalidate_name(a,5000,b"a.io"),1);
  assert_eq!(cache.entries_active(25),1);
  assert_eq!(cache.sweep_expired(80),1);
  assert_eq!(cache.entries_active(80),0);
 }
 #[test]fn invalid_query_is_fail_closed_and_does_not_invalidate(){
  let owner=Identity{pid:5,generation:2};
  let mut cache=DnsLeaseCache::<1>::new();
  let mut name=[0u8;253];name[..4].copy_from_slice(b"a.io");
  cache.entries[0]=Some(CachedA{owner,local_port:1234,
    name,len:4,ip:[1,1,1,1],expires:50});
  assert_eq!(cache.invalidate_name(owner,1234,b"-bad"),0);
  assert_eq!(cache.entries_active(2),1);
 }
}

 
/// R011 pre-transport TCP connection admission. A trusted network worker
/// supplies only authenticated socket owners and verified peer addresses.
/// Real TCP packet IO, congestion control and certificate trust remain to wire.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ConnectionLease{
 pub actor:Identity,pub local_port:u16,pub peer:TcpPeer,
 pub deadline:u64,pub attempts:u8,pub established:bool,
}
pub struct ConnectionTable<const N:usize>{leases:[Option<ConnectionLease>;N]}
impl<const N:usize> ConnectionTable<N>{
 pub const fn new()->Self{Self{leases:[None;N]}}
 pub fn begin(&mut self,actor:Identity,local_port:u16,peer:TcpPeer,
  now:u64,timeout:u64)->Result<(),Error>{
  if actor.pid==0||actor.generation==0||local_port==0||
     !peer.valid()||timeout==0{return Err(Error::Invalid);}
  let deadline=now.checked_add(timeout).ok_or(Error::Invalid)?;
  if self.leases.iter().flatten().any(|s|
     s.actor==actor&&s.local_port==local_port){return Err(Error::Denied);}
  let slot=self.leases.iter_mut().find(|s|s.is_none())
   .ok_or(Error::RetryExhausted)?;
  *slot=Some(ConnectionLease{actor,local_port,peer,deadline,
    attempts:0,established:false});Ok(())
 }
 pub fn retry(&mut self,actor:Identity,local_port:u16,peer:TcpPeer,
  now:u64,timeout:u64)->Result<u8,Error>{
  if timeout==0{return Err(Error::Invalid);}
  let slot=self.leases.iter_mut().flatten().find(|s|
    s.actor==actor&&s.local_port==local_port&&s.peer==peer)
    .ok_or(Error::NotFound)?;
  if slot.established||now<slot.deadline{return Err(Error::Denied);}
  if slot.attempts>=MAX_TCP_RETRIES{return Err(Error::RetryExhausted);}
  let next_deadline=now.checked_add(timeout).ok_or(Error::Invalid)?;
  slot.attempts+=1;slot.deadline=next_deadline;Ok(slot.attempts)
 }
 pub fn established(&mut self,actor:Identity,local_port:u16,
  peer:TcpPeer,now:u64)->Result<(),Error>{
  let slot=self.leases.iter_mut().flatten().find(|s|
    s.actor==actor&&s.local_port==local_port&&s.peer==peer)
    .ok_or(Error::NotFound)?;
  if now>slot.deadline||slot.established{return Err(Error::Denied);}
  slot.established=true;Ok(())
 }
 pub fn verified(&self,actor:Identity,local_port:u16,
  peer:TcpPeer)->bool{
  self.leases.iter().flatten().any(|s|s.actor==actor&&
   s.local_port==local_port&&s.peer==peer&&s.established)
 }
 pub fn retire(&mut self,actor:Identity)->usize{
  let mut count=0;
  for slot in &mut self.leases{
   if slot.is_some_and(|s|s.actor==actor){*slot=None;count+=1;}
  }
  count
 }
}
#[cfg(test)]
mod r011_connection_lease_tests{
 use super::*;
 fn actor(gen:u64)->Identity{Identity{pid:7,generation:gen}}
 fn peer()->TcpPeer{TcpPeer{address:[1,1,1,1],port:443}}
 #[test]fn establish_only_within_authenticated_peer_deadline(){
  let mut t=ConnectionTable::<2>::new();
  t.begin(actor(1),6000,peer(),10,10).unwrap();
  assert_eq!(t.established(actor(2),6000,peer(),19),Err(Error::NotFound));
  t.established(actor(1),6000,peer(),20).unwrap();
  assert!(t.verified(actor(1),6000,peer()));
  assert!(!t.verified(actor(2),6000,peer()));
 }
 #[test]fn retries_are_bounded_and_time_gated(){
  let mut t=ConnectionTable::<1>::new();
  t.begin(actor(1),6000,peer(),0,10).unwrap();
  assert_eq!(t.retry(actor(1),6000,peer(),9,10),Err(Error::Denied));
  assert_eq!(t.retry(actor(1),6000,peer(),10,10),Ok(1));
  assert_eq!(t.retry(actor(1),6000,peer(),20,10),Ok(2));
  assert_eq!(t.retry(actor(1),6000,peer(),30,10),Ok(3));
  assert_eq!(t.retry(actor(1),6000,peer(),40,10),Err(Error::RetryExhausted));
 }
 #[test]fn process_reap_preserves_other_generation_connection(){
  let mut t=ConnectionTable::<2>::new();
  t.begin(actor(1),6000,peer(),0,10).unwrap();
  t.begin(actor(2),6001,peer(),0,10).unwrap();
  assert_eq!(t.retire(actor(1)),1);
  assert_eq!(t.established(actor(1),6000,peer(),5),Err(Error::NotFound));
  assert_eq!(t.established(actor(2),6001,peer(),5),Ok(()));
 }
}


/// R012 require both a valid established owner+peer socket and independent
/// certificate trust before authorizing a high-privilege OTA TLS channel.
/// Still not a TLS record-layer or packet-sending implementation.
pub fn permit_https<const N:usize,const P:usize,V:TlsCertificateVerifier>(
 connections:&ConnectionTable<N>,endpoint:&Endpoint<P>,
 actor:Identity,local_port:u16,peer:TcpPeer,
 now:u64,hostname:&[u8],certificate:&[u8],verifier:&V
)->Result<(),Error>{
 if !peer.valid()||peer.port!=443||
   !connections.verified_at(actor,local_port,peer,now){
  return Err(Error::Denied);
 }
 endpoint.allow_tls(actor,local_port,now,hostname,certificate,verifier)
}
/// Avoid retaining completed or dead process sockets after a trust epoch
/// changes; only a trusted scheduler may nominate the retiring identity.
impl<const N:usize> ConnectionTable<N>{
 pub fn purge_deadline(&mut self,now:u64)->usize{
  let mut count=0;
  for slot in &mut self.leases{
   if slot.is_some_and(|s|!s.established&&s.deadline<now){
     *slot=None;count+=1;
   }
  }
  count
 }
 pub fn active(&self)->usize{self.leases.iter().flatten().count()}
 pub fn established_count(&self,actor:Identity)->usize{
  self.leases.iter().flatten().filter(|s|s.actor==actor&&s.established).count()
 }
}
#[cfg(test)]
mod r012_secure_network_tests{
 use super::*;
 fn owner()->Identity{Identity{pid:1,generation:2}}
 fn peer()->TcpPeer{TcpPeer{address:[1,2,3,4],port:443}}
 #[test]fn tcp_lease_expires_before_establishment_and_is_reclaimed(){
  let mut connections=ConnectionTable::<2>::new();
  connections.begin(owner(),6000,peer(),5,8).unwrap();
  assert_eq!(connections.established_count(owner()),0);
  assert_eq!(connections.purge_deadline(14),1);
  assert_eq!(connections.active(),0);
  assert_eq!(connections.established(owner(),6000,peer(),10),Err(Error::NotFound));
 }
 #[test]fn trusted_peer_only_marked_established_in_valid_window(){
  let mut connections=ConnectionTable::<2>::new();
  connections.begin(owner(),6000,peer(),5,8).unwrap();
  connections.established(owner(),6000,peer(),13).unwrap();
  assert_eq!(connections.established_count(owner()),1);
  assert_eq!(connections.purge_deadline(30),0);
  assert!(connections.verified(owner(),6000,peer()));
 }
 #[test]fn stale_pid_epoch_does_not_observe_current_tcp_session(){
  let mut connections=ConnectionTable::<1>::new();
  connections.begin(owner(),6000,peer(),5,8).unwrap();
  connections.established(owner(),6000,peer(),10).unwrap();
  let stale=Identity{generation:3,..owner()};
  assert!(!connections.verified(stale,6000,peer()));
  assert_eq!(connections.established_count(stale),0);
 }
}

/// R013 expiry-aware TCP lease management. A connected session must be
/// renewed by the trusted network service before its deadline; an old
/// 'established' bit alone cannot authorize an OTA HTTPS request forever.
impl<const N:usize> ConnectionTable<N>{
 pub fn verified_at(&self,actor:Identity,port:u16,peer:TcpPeer,now:u64)
   ->bool{
  self.leases.iter().flatten().any(|s|
   s.actor==actor&&s.local_port==port&&s.peer==peer&&
   s.established&&now<=s.deadline)
 }
 pub fn keepalive_authenticated(&mut self,actor:Identity,port:u16,
   peer:TcpPeer,now:u64,extension:u64)->Result<u64,Error>{
  if actor.pid==0||actor.generation==0||extension==0{
   return Err(Error::Invalid);
  }
  let lease=self.leases.iter_mut().flatten().find(|s|
    s.actor==actor&&s.local_port==port&&s.peer==peer)
    .ok_or(Error::NotFound)?;
  if !lease.established||now>lease.deadline{return Err(Error::Denied);}
  let new_deadline=now.checked_add(extension).ok_or(Error::Invalid)?;
  if new_deadline<=lease.deadline{return Err(Error::Invalid);}
  lease.deadline=new_deadline;Ok(new_deadline)
 }
 pub fn purge_stale(&mut self,now:u64)->usize{
  let mut count=0;
  for slot in &mut self.leases{
   if slot.is_some_and(|s|now>s.deadline){*slot=None;count+=1;}
  }
  count
 }
}
#[cfg(test)]
mod r013_network_expiration_tests{
 use super::*;
 fn id(g:u64)->Identity{Identity{pid:70,generation:g}}
 fn peer()->TcpPeer{TcpPeer{address:[4,4,4,4],port:443}}
 #[test]fn idle_tls_connection_expires_without_authenticated_renewal(){
  let mut t=ConnectionTable::<1>::new();
  t.begin(id(1),5555,peer(),5,10).unwrap();
  t.established(id(1),5555,peer(),10).unwrap();
  assert!(t.verified_at(id(1),5555,peer(),15));
  assert!(!t.verified_at(id(1),5555,peer(),16));
  assert_eq!(t.purge_stale(16),1);
  assert!(!t.verified_at(id(1),5555,peer(),16));
 }
 #[test]fn authenticated_keepalive_fences_wrong_epoch_and_timestamps(){
  let mut t=ConnectionTable::<1>::new();
  t.begin(id(1),5555,peer(),5,10).unwrap();
  t.established(id(1),5555,peer(),10).unwrap();
  assert_eq!(t.keepalive_authenticated(id(2),5555,peer(),12,10),
    Err(Error::NotFound));
  assert_eq!(t.keepalive_authenticated(id(1),5555,peer(),12,20),Ok(32));
  assert!(t.verified_at(id(1),5555,peer(),30));
  assert_eq!(t.keepalive_authenticated(id(1),5555,peer(),33,10),
    Err(Error::Denied));
 }
 #[test]fn bounded_reuse_of_expired_socket_slot(){
  let mut t=ConnectionTable::<1>::new();
  t.begin(id(1),5555,peer(),0,3).unwrap();
  assert_eq!(t.begin(id(2),5555,peer(),0,3),Err(Error::RetryExhausted));
  assert_eq!(t.purge_stale(4),1);
  t.begin(id(2),5555,peer(),4,3).unwrap();
  assert_eq!(t.active(),1);
 }
}

/// R014 secure connection admission joins kernel-authenticated socket rights
/// with an active bound port and validated remote peer. This is the policy
/// ingress required BEFORE packet transport and TLS record verification.
impl<const N:usize> ConnectionTable<N>{
 pub fn begin_authorized<const P:usize,const G:usize>(&mut self,
  endpoint:&Endpoint<P>,access:&Access<G>,owner:Identity,local_port:u16,
  peer:TcpPeer,now:u64,timeout:u64)->Result<(),Error>{
  access.require(owner,SOCKET,now).map_err(|_|Error::Denied)?;
  endpoint.ports.require(owner,local_port,now).map_err(|_|Error::Denied)?;
  self.begin(owner,local_port,peer,now,timeout)
 }
 pub fn terminate_peer(&mut self,actor:Identity,peer:TcpPeer)->usize{
  let mut removed=0;
  for slot in &mut self.leases{
   if slot.is_some_and(|s|s.actor==actor&&s.peer==peer){
    *slot=None;removed+=1;
   }
  }
  removed
 }
 pub fn active_for(&self,actor:Identity)->usize{
  self.leases.iter().flatten().filter(|s|s.actor==actor).count()
 }
}
#[cfg(test)]
mod r014_authorized_network_tests{
 use super::*;use crate::batch6_security::SOCKET;
 fn id(g:u64)->Identity{Identity{pid:50,generation:g}}
 fn peer()->TcpPeer{TcpPeer{address:[8,8,8,8],port:443}}
 #[test]fn established_socket_requires_capability_and_bound_port(){
  let mut c=ConnectionTable::<2>::new();
  let mut endpoint=Endpoint::<2>::new();let mut access=Access::<2>::new();
  assert_eq!(c.begin_authorized(&endpoint,&access,id(1),5000,peer(),1,20),
    Err(Error::Denied));
  access.grant(id(1),SOCKET,100).unwrap();
  endpoint.acquire(&access,id(1),5000,1,100).unwrap();
  c.begin_authorized(&endpoint,&access,id(1),5000,peer(),1,20).unwrap();
  assert_eq!(c.active_for(id(1)),1);
 }
 #[test]fn stale_pid_epoch_is_denied_and_cannot_close_current_peer(){
  let mut c=ConnectionTable::<2>::new();
  let mut endpoint=Endpoint::<2>::new();let mut access=Access::<2>::new();
  access.grant(id(1),SOCKET,100).unwrap();
  endpoint.acquire(&access,id(1),5000,1,100).unwrap();
  c.begin_authorized(&endpoint,&access,id(1),5000,peer(),1,20).unwrap();
  assert_eq!(c.begin_authorized(&endpoint,&access,id(2),5001,peer(),1,20),
    Err(Error::Denied));
  assert_eq!(c.terminate_peer(id(2),peer()),0);assert_eq!(c.active_for(id(1)),1);
 }
 #[test]fn invalid_multicast_peer_is_rejected_before_allocation(){
  let mut c=ConnectionTable::<1>::new();
  assert_eq!(c.begin(id(1),5000,TcpPeer{address:[239,0,0,1],port:443},
     1,20),Err(Error::Invalid));
  assert_eq!(c.active(),0);
 }
}


/// R016: one scheduler transaction may provision several outbound sockets.
/// All identity, capacity, destination and timeout checks happen on a
/// private table; one bad request cannot leave half-open leases behind.
impl<const N:usize> ConnectionTable<N>{
 pub fn begin_batch(&mut self,
  requests:&[(Identity,u16,TcpPeer,u64,u64)])->Result<usize,Error>{
  if requests.is_empty()||requests.len()>N{return Err(Error::Invalid);}
  let mut shadow=Self{leases:self.leases};
  for &(owner,port,peer,now,timeout) in requests{
   shadow.begin(owner,port,peer,now,timeout)?;
  }
  self.leases=shadow.leases;Ok(requests.len())
 }
 pub fn snapshot_owner(&self,owner:Identity,now:u64,
   out:&mut[ConnectionLease])->Result<usize,Error>{
  if owner.pid==0||owner.generation==0{return Err(Error::Invalid);}
  let count=self.leases.iter().flatten()
    .filter(|l|l.actor==owner&&now<=l.deadline).count();
  if count>out.len(){return Err(Error::RetryExhausted);}
  let mut n=0;
  for lease in self.leases.iter().flatten(){
   if lease.actor==owner&&now<=lease.deadline{
    out[n]=*lease;n+=1;
   }
  }
  Ok(n)
 }
}
#[cfg(test)]mod r016_socket_batch_tests{
 use super::*;
 fn id(g:u64)->Identity{Identity{pid:9,generation:g}}
 fn peer(n:u8)->TcpPeer{TcpPeer{address:[10,0,0,n],port:443}}
 #[test]fn two_connections_are_added_to_a_single_owner(){
  let mut c=ConnectionTable::<3>::new();
  assert_eq!(c.begin_batch(&[
   (id(1),5000,peer(1),1,20),(id(1),5001,peer(2),1,30)]),Ok(2));
  assert_eq!(c.active_for(id(1)),2);
  let placeholder=ConnectionLease{actor:id(1),local_port:0,peer:peer(1),
    deadline:0,attempts:0,established:false};
  let mut out=[placeholder;2];
  assert_eq!(c.snapshot_owner(id(1),10,&mut out),Ok(2));
  assert_eq!(out[0].local_port,5000);assert_eq!(out[1].local_port,5001);
 }
 #[test]fn second_bad_peer_causes_no_partial_lease(){
  let mut c=ConnectionTable::<3>::new();
  assert_eq!(c.begin_batch(&[
   (id(1),5000,peer(1),1,20),
   (id(1),5001,TcpPeer{address:[239,0,0,1],port:80},1,20)]),
   Err(Error::Invalid));
  assert_eq!(c.active_for(id(1)),0);
 }
 #[test]fn short_snapshot_and_recycled_pid_cannot_observe_sockets(){
  let mut c=ConnectionTable::<2>::new();
  c.begin_batch(&[(id(1),5000,peer(1),1,20),(id(1),5001,peer(2),1,20)])
   .unwrap();
  let placeholder=ConnectionLease{actor:id(1),local_port:0,peer:peer(1),
    deadline:0,attempts:0,established:false};
  let mut short=[placeholder;1];
  assert_eq!(c.snapshot_owner(id(1),2,&mut short),
    Err(Error::RetryExhausted));assert_eq!(short,[placeholder]);
  assert_eq!(c.snapshot_owner(id(2),2,&mut short),Ok(0));
 }
}


/// R017 network development sessions require both live socket rights and
/// an already authenticated local port for every outbound peer.
impl<const N:usize> ConnectionTable<N>{
 pub fn begin_group_authorized<const P:usize,const G:usize>(
  &mut self,endpoint:&Endpoint<P>,auth:&Access<G>,
  requests:&[(Identity,u16,TcpPeer,u64,u64)])->Result<usize,Error>{
  if requests.is_empty()||requests.len()>N{return Err(Error::Invalid);}
  let mut staged=Self{leases:self.leases};
  for &(owner,port,peer,now,timeout) in requests{
   staged.begin_authorized(endpoint,auth,owner,port,peer,now,timeout)?;
  }
  self.leases=staged.leases;Ok(requests.len())
 }
 pub fn session_deadlines(&self,owner:Identity,out:&mut[u64])
  ->Result<usize,Error>{
  if owner.pid==0||owner.generation==0{return Err(Error::Invalid);}
  let count=self.leases.iter().flatten().filter(|l|l.actor==owner).count();
  if count>out.len(){return Err(Error::RetryExhausted);}
  let mut at=0;
  for lease in self.leases.iter().flatten(){
   if lease.actor==owner{out[at]=lease.deadline;at+=1;}
  }
  Ok(at)
 }
}
#[cfg(test)]mod r017_workbench_sockets_tests{
 use super::*;use crate::batch6_security::SOCKET;
 fn id(g:u64)->Identity{Identity{pid:40,generation:g}}
 fn peer(n:u8)->TcpPeer{TcpPeer{address:[8,8,8,n],port:443}}
 #[test]fn two_allowed_dev_connections_commit_together(){
  let mut c=ConnectionTable::<2>::new();let mut e=Endpoint::<2>::new();
  let mut acl=Access::<1>::new();acl.grant(id(1),SOCKET,100).unwrap();
  e.acquire(&acl,id(1),5000,1,100).unwrap();
  e.acquire(&acl,id(1),5001,1,100).unwrap();
  assert_eq!(c.begin_group_authorized(&e,&acl,&[
   (id(1),5000,peer(1),2,20),(id(1),5001,peer(2),2,30)]),Ok(2));
  let mut deadlines=[0;2];assert_eq!(c.session_deadlines(id(1),&mut deadlines),Ok(2));
  assert_eq!(deadlines,[22,32]);
 }
 #[test]fn missing_second_port_fails_without_partial_connection(){
  let mut c=ConnectionTable::<2>::new();let mut e=Endpoint::<2>::new();
  let mut acl=Access::<1>::new();acl.grant(id(1),SOCKET,100).unwrap();
  e.acquire(&acl,id(1),5000,1,100).unwrap();
  assert!(c.begin_group_authorized(&e,&acl,&[
   (id(1),5000,peer(1),2,20),(id(1),5001,peer(2),2,20)]).is_err());
  assert_eq!(c.active_for(id(1)),0);
 }
 #[test]fn stale_pid_cannot_inspect_active_connection(){
  let mut c=ConnectionTable::<1>::new();
  c.begin(id(1),5000,peer(1),2,20).unwrap();
  let mut dates=[77u64;1];
  assert_eq!(c.session_deadlines(id(2),&mut dates),Ok(0));
  assert_eq!(dates,[77]);
 }
}


#[cfg(test)]
mod m2_dns_ipv4_wire_tests {
 use super::*;
 const DNS_SERVER:[u8;4]=[192,0,2,53];
 const CLIENT:[u8;4]=[192,0,2,10];
 fn datagram()->[u8;73]{
  let mut b=[0u8;73];
  b[0]=0x45; b[2..4].copy_from_slice(&73u16.to_be_bytes());
  b[8]=64; b[9]=17;
  b[12..16].copy_from_slice(&DNS_SERVER);
  b[16..20].copy_from_slice(&CLIENT);
  b[20..22].copy_from_slice(&53u16.to_be_bytes());
  b[22..24].copy_from_slice(&6040u16.to_be_bytes());
  b[24..26].copy_from_slice(&53u16.to_be_bytes());
  b[28..40].copy_from_slice(b"\x12\x34\x81\x80\x00\x01\x00\x01\x00\x00\x00\x00");
  b[40..53].copy_from_slice(b"\x07example\x03com\x00");
  b[53..57].copy_from_slice(&[0,1,0,1]);
  b[57..73].copy_from_slice(&[0xc0,0x0c,0,1,0,1,0,0,0,60,0,4,203,0,113,7]);
  let udp_sum=internet_sum(&b[12..20])+17+53+
    internet_sum(&b[20..]);
  b[26..28].copy_from_slice(&checked_checksum(udp_sum).to_be_bytes());
  let ip_sum=checked_checksum(internet_sum(&b[..20]));
  b[10..12].copy_from_slice(&ip_sum.to_be_bytes());
  b
 }
 fn decode(b:&[u8])->Result<[u8;4],Error>{
  parse_ipv4_udp_dns_a(b,DNS_SERVER,CLIENT,6040,0x1234,b"example.com")
 }
 #[test]
 fn m2_dns_ipv4_full_wire_validates_checksum_and_dns_answer(){
  assert_eq!(decode(&datagram()),Ok([203,0,113,7]));
 }
 #[test]
 fn m2_dns_ipv4_rejects_source_destination_port_and_txid_substitution(){
  let b=datagram();
  assert_eq!(parse_ipv4_udp_dns_a(&b,[192,0,2,54],CLIENT,6040,0x1234,b"example.com"),Err(Error::Malformed));
  assert_eq!(parse_ipv4_udp_dns_a(&b,DNS_SERVER,[192,0,2,11],6040,0x1234,b"example.com"),Err(Error::Malformed));
  assert_eq!(parse_ipv4_udp_dns_a(&b,DNS_SERVER,CLIENT,6041,0x1234,b"example.com"),Err(Error::Malformed));
  assert_eq!(parse_ipv4_udp_dns_a(&b,DNS_SERVER,CLIENT,6040,0x1235,b"example.com"),Err(Error::Malformed));
  assert_eq!(parse_ipv4_udp_dns_a(&b,DNS_SERVER,CLIENT,6040,0x1234,b"wrong.com"),Err(Error::Malformed));
 }
 #[test]
 fn m2_dns_ipv4_rejects_corruption_fragments_zero_checksums_and_truncation(){
  let b=datagram();
  let mut corrupt=b;corrupt[72]^=1;
  assert_eq!(decode(&corrupt),Err(Error::Malformed));
  let mut fragment=b;fragment[6]=0x20;
  assert_eq!(decode(&fragment),Err(Error::Malformed));
  let mut zero_udp=b;zero_udp[26]=0;zero_udp[27]=0;
  assert_eq!(decode(&zero_udp),Err(Error::Malformed));
  let mut bad_length=b;bad_length[25]=54;
  assert_eq!(decode(&bad_length),Err(Error::Malformed));
  let mut wrong_protocol=b;wrong_protocol[9]=6;
  assert_eq!(decode(&wrong_protocol),Err(Error::Malformed));
  assert_eq!(decode(&b[..72]),Err(Error::Malformed));
  assert_eq!(decode(&b[..39]),Err(Error::Malformed));
 }
 #[test]
 fn m2_dns_ipv4_owner_generation_and_port_expiry_are_enforced(){
  let owner=Identity{pid:7,generation:2};
  let stale=Identity{pid:7,generation:3};
  let mut access=Access::<1>::new();
  access.grant(owner,SOCKET,100).unwrap();
  let mut endpoint=Endpoint::<1>::new();
  endpoint.acquire(&access,owner,6040,1,50).unwrap();
  let wire=datagram();
  assert_eq!(endpoint.accept_verified_dns_ipv4(owner,6040,2,DNS_SERVER,CLIENT,
    0x1234,b"example.com",&wire),Ok([203,0,113,7]));
  assert_eq!(endpoint.accept_verified_dns_ipv4(stale,6040,2,DNS_SERVER,CLIENT,
    0x1234,b"example.com",&wire),Err(Error::Denied));
  assert_eq!(endpoint.accept_verified_dns_ipv4(owner,6041,2,DNS_SERVER,CLIENT,
    0x1234,b"example.com",&wire),Err(Error::Denied));
  assert_eq!(endpoint.accept_verified_dns_ipv4(owner,6040,51,DNS_SERVER,CLIENT,
    0x1234,b"example.com",&wire),Err(Error::Denied));
 }
}
