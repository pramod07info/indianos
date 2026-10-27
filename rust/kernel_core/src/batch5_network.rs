//! Batch 5 task 14: BOOTP/DHCP ACK header identity checks before parsing
//! lease options. Network stack, checksum validation and TLS are separate.
use crate::batch3_network_security::{self,Lease,NetworkError,TlsCertificateVerifier};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Malformed,WrongClient,WrongTransaction,InvalidLease,NoAck,Unverified}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Client{pub xid:u32,pub mac:[u8;6]}
pub fn parse_ack(client:Client,packet:&[u8])->Result<Lease,Error>{
 // BOOTP fixed header through chaddr + server name + boot file + cookie.
 if packet.len()<241||packet.len()>1500||packet[0]!=2||packet[1]!=1||
  packet[2]!=6||packet[3]!=0{return Err(Error::Malformed);}
 if packet[4..8]!=client.xid.to_be_bytes(){return Err(Error::WrongTransaction);}
 if packet[28..34]!=client.mac{return Err(Error::WrongClient);}
 let address:[u8;4]=packet[16..20].try_into().map_err(|_|Error::Malformed)?;
 let options=&packet[236..];
 let lease=batch3_network_security::parse_lease(address,options)
  .map_err(|_|Error::InvalidLease)?;
 // Insist on DHCP message type ACK (53=5), a server identity (54)
 // and reject malformed, duplicate or truncated critical options.
 let mut i=4usize;let mut ack=false;let mut server=false;let mut ended=false;
 while i<options.len(){
  let typ=options[i];i+=1;
  if typ==255{ended=true;break;}
  if typ==0{continue;}
  if i>=options.len(){return Err(Error::Malformed);}
  let len=options[i] as usize;i+=1;
  if len>options.len()-i{return Err(Error::Malformed);}
  let v=&options[i..i+len];i+=len;
  match typ{
   53 if len==1&&!ack =>{if v[0]!=5{return Err(Error::NoAck);}ack=true;}
   54 if len==4&&!server =>{if v==[0u8;4]{return Err(Error::Malformed);}server=true;}
   53|54=>return Err(Error::Malformed),
   _=>{}
  }
 }
 if !ended||!ack||!server{return Err(Error::NoAck);}
 Ok(lease)
}
pub fn tls_authenticated<V:TlsCertificateVerifier>(v:&V,hostname:&[u8],
 cert:&[u8])->Result<(),Error>{
 match batch3_network_security::tls_peer_gate(v,hostname,cert){
  Ok(())=>Ok(()),
  Err(NetworkError::Unverified)=>Err(Error::Unverified),
  Err(_)=>Err(Error::Malformed),
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn packet()->[u8;271]{
  let mut p=[0u8;271];p[0]=2;p[1]=1;p[2]=6;
  p[4..8].copy_from_slice(&42u32.to_be_bytes());
  p[16..20].copy_from_slice(&[10,0,0,9]);
  p[28..34].copy_from_slice(&[1,2,3,4,5,6]);
  let opts=[99,130,83,99,1,4,255,255,255,0,3,4,10,0,0,1,
    51,4,0,0,0,30,53,1,5,54,4,10,0,0,1,255];
  p[236..236+opts.len()].copy_from_slice(&opts);p
 }
 #[test]fn client_and_xid_cannot_be_spoofed(){
  let id=Client{xid:42,mac:[1,2,3,4,5,6]};
  assert_eq!(parse_ack(id,&packet()).unwrap().address,[10,0,0,9]);
  let mut wrong=id;wrong.xid=43;
  assert_eq!(parse_ack(wrong,&packet()),Err(Error::WrongTransaction));
  wrong=id;wrong.mac=[0;6];
  assert_eq!(parse_ack(wrong,&packet()),Err(Error::WrongClient));
 }
 #[test]fn missing_ack_never_installs_lease(){
  let mut p=packet();p[260]=2;
  assert_eq!(parse_ack(Client{xid:42,mac:[1,2,3,4,5,6]},&p),
    Err(Error::NoAck));
 }
}

/// R006: transactional DHCP client lease lifecycle built around the real
/// bounded ACK parser. The NIC, UDP transport, timers and packet checksum
/// verification must be supplied by the network driver.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum LeasePhase{Unbound,AwaitingAck,Bound,Renewing}
pub struct LeaseClient{
 phase:LeasePhase,client:Client,lease:Option<Lease>,expires:u64,
}
impl LeaseClient{
 pub const fn new()->Self{
  Self{phase:LeasePhase::Unbound,client:Client{xid:0,mac:[0;6]},
   lease:None,expires:0}
 }
 pub fn phase(&self)->LeasePhase{self.phase}
 pub fn discover(&mut self,client:Client)->Result<(),Error>{
  if client.xid==0||client.mac==[0;6]||self.phase!=LeasePhase::Unbound{
   return Err(Error::WrongTransaction);
  }
  self.client=client;self.phase=LeasePhase::AwaitingAck;Ok(())
 }
 /// Does not discard an unexpired lease while a renewal is pending.
 /// Only the authenticated peer ACK for the NEW transaction can replace it.
 pub fn renew(&mut self,new_xid:u32,now:u64)->Result<(),Error>{
  if self.phase!=LeasePhase::Bound||now>=self.expires||
   new_xid==0||new_xid==self.client.xid{return Err(Error::InvalidLease);}
  self.client.xid=new_xid;self.phase=LeasePhase::Renewing;Ok(())
 }
 pub fn receive_ack(&mut self,packet:&[u8],now:u64)->Result<Lease,Error>{
  if !matches!(self.phase,LeasePhase::AwaitingAck|LeasePhase::Renewing){
   return Err(Error::NoAck);
  }
  // Full parser must pass before changing state or publishing IP/gateway.
  let next=parse_ack(self.client,packet)?;
  let deadline=now.checked_add(next.lease_seconds as u64)
   .ok_or(Error::InvalidLease)?;
  self.lease=Some(next);self.expires=deadline;
  self.phase=LeasePhase::Bound;Ok(next)
 }
 pub fn valid_lease(&self,now:u64)->Result<Lease,Error>{
  if !matches!(self.phase,LeasePhase::Bound|LeasePhase::Renewing)||
   now>=self.expires{return Err(Error::InvalidLease);}
  self.lease.ok_or(Error::InvalidLease)
 }
 /// Network link teardown or absolute TTL expiry fully clears identifiers
 /// so an old packet cannot silently reactivate the prior lease.
 pub fn expire(&mut self,now:u64)->bool{
  if self.lease.is_none()||now<self.expires{return false;}
  self.reset();true
 }
 pub fn reset(&mut self){
  self.phase=LeasePhase::Unbound;self.client=Client{xid:0,mac:[0;6]};
  self.lease=None;self.expires=0;
 }
}
#[cfg(test)]
mod r006_dhcp_session_tests{
 use super::*;
 fn client(xid:u32)->Client{Client{xid,mac:[1,2,3,4,5,6]}}
 fn packet(xid:u32)->[u8;271]{
  let mut p=[0u8;271];p[0]=2;p[1]=1;p[2]=6;
  p[4..8].copy_from_slice(&xid.to_be_bytes());
  p[16..20].copy_from_slice(&[10,0,0,9]);
  p[28..34].copy_from_slice(&[1,2,3,4,5,6]);
  let opts=[99,130,83,99,1,4,255,255,255,0,3,4,10,0,0,1,
    51,4,0,0,0,30,53,1,5,54,4,10,0,0,1,255];
  p[236..236+opts.len()].copy_from_slice(&opts);p
 }
 #[test]fn discover_ack_renew_expire_obeys_real_transaction_ids(){
  let mut x=LeaseClient::new();x.discover(client(42)).unwrap();
  assert_eq!(x.receive_ack(&packet(43),5),Err(Error::WrongTransaction));
  assert_eq!(x.phase(),LeasePhase::AwaitingAck);
  assert_eq!(x.receive_ack(&packet(42),5).unwrap().address,[10,0,0,9]);
  assert!(x.valid_lease(34).is_ok());
  x.renew(50,10).unwrap();
  assert!(x.valid_lease(11).is_ok());
  assert_eq!(x.receive_ack(&packet(42),11),Err(Error::WrongTransaction));
  assert_eq!(x.receive_ack(&packet(50),11).unwrap().lease_seconds,30);
  assert_eq!(x.expire(40),false);
  assert_eq!(x.expire(41),true);
  assert_eq!(x.phase(),LeasePhase::Unbound);
 }
 #[test]fn invalid_ack_never_replaces_prior_working_lease(){
  let mut x=LeaseClient::new();x.discover(client(1)).unwrap();
  x.receive_ack(&packet(1),0).unwrap();x.renew(2,2).unwrap();
  let mut forged=packet(2);forged[28]=0;
  assert_eq!(x.receive_ack(&forged,3),Err(Error::WrongClient));
  assert_eq!(x.valid_lease(3).unwrap().address,[10,0,0,9]);
  assert_eq!(x.phase(),LeasePhase::Renewing);
  x.reset();assert_eq!(x.valid_lease(3),Err(Error::InvalidLease));
 }
}
