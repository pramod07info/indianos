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
}
