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
 let mut label=0usize;
 for &b in name{
  if b==b'.'{
   if label==0||label>63{return false;}
   label=0;
  }else if b.is_ascii_alphanumeric()||b==b'-'{
   label+=1;if label>63{return false;}
  }else{return false;}
 }
 label!=0
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
 Ok(wire[i+12..i+16].try_into().unwrap())
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

}
