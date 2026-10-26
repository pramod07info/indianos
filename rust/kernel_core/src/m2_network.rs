//! M2 bounded, per-process outbound socket authorization and DNS A response
//! decoder. Existing R028 QEMU protocol stack is a transport demo, not a
//! general socket API; this service NEVER sends packets or implements TLS.
//! Kernel-derived PID/generation + verified DNS response ingress REQUIRED.
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,SOCKET};
use crate::batch6_network::{Ports,Error as PortError,verified_tls};
use crate::batch3_network_security::TlsCertificateVerifier;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Denied,NotFound,Malformed,Unverified}
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
 if flags&0x8000==0||flags&0x7800!=0||flags&0x0200!=0||
  flags&0x000f!=0||be16(&wire[4..6])!=1||
  be16(&wire[6..8])==0{return Err(Error::Malformed);}
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
 if ttl==0{return Err(Error::Malformed);}
 Ok(wire[i+12..i+16].try_into().unwrap())
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
