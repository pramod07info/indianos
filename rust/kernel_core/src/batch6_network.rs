//! R040 task 14: owner-bound outbound port leases with deterministic
//! expiration and verified TLS callback. Does not implement sockets/TLS.
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,SOCKET};
use crate::batch3_network_security::{TlsCertificateVerifier,tls_peer_gate};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Full,Conflict,Denied,Expired,Unverified}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PortLease{pub owner:Identity,pub port:u16,pub deadline:u64}
pub struct Ports<const N:usize>{leases:[Option<PortLease>;N]}
impl<const N:usize> Ports<N>{
 pub const fn new()->Self{Self{leases:[None;N]}}
 pub fn bind<const G:usize>(&mut self,auth:&Access<G>,owner:Identity,
  port:u16,now:u64,deadline:u64)->Result<(),Error>{
  auth.require(owner,SOCKET,now).map_err(|_|Error::Denied)?;
  if port<1024||deadline<=now{return Err(Error::Invalid);}
  if self.leases.iter().flatten().any(|s|s.port==port&&s.deadline>now){
   return Err(Error::Conflict);
  }
  let slot=self.leases.iter_mut().find(|s|s.is_none_or(|x|x.deadline<=now))
   .ok_or(Error::Full)?;
  *slot=Some(PortLease{owner,port,deadline});Ok(())
 }
 pub fn require(&self,owner:Identity,port:u16,now:u64)->Result<(),Error>{
  let lease=self.leases.iter().flatten().find(|x|x.port==port)
   .ok_or(Error::Denied)?;
  if lease.owner!=owner{return Err(Error::Denied);}
  if now>=lease.deadline{return Err(Error::Expired);}
  Ok(())
 }
 pub fn retire(&mut self,owner:Identity){
  for x in &mut self.leases{
   if x.is_some_and(|s|s.owner==owner){*x=None;}
  }
 }
}
pub fn verified_tls<V:TlsCertificateVerifier>(v:&V,
 name:&[u8],cert:&[u8])->Result<(),Error>{
 tls_peer_gate(v,name,cert).map_err(|_|Error::Unverified)
}
#[cfg(test)]mod tests{
 use super::*;use crate::batch6_security::SOCKET;
 #[test]fn ephemeral_socket_lease_cannot_cross_pid_epoch(){
  let a=Identity{pid:3,generation:1};
  let mut acl=Access::<1>::new();acl.grant(a,SOCKET,90).unwrap();
  let mut ports=Ports::<1>::new();ports.bind(&acl,a,8080,1,30).unwrap();
  assert_eq!(ports.bind(&acl,a,8080,2,40),Err(Error::Conflict));
  assert_eq!(ports.require(Identity{generation:2,..a},8080,2),Err(Error::Denied));
  assert_eq!(ports.require(a,8080,30),Err(Error::Expired));
  ports.bind(&acl,a,8080,31,60).unwrap();
  ports.retire(a);assert_eq!(ports.require(a,8080,32),Err(Error::Denied));
 }
}