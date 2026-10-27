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

/// Orbit R005 outbound lease lifecycle. This is not a TCP/IP or TLS
/// implementation: actual device transport and certificate verification
/// remain mandatory at the real network boundary.
impl<const N:usize> Ports<N>{
 /// Bind a set of ports as a transaction, preserving existing leases
 /// when any port is privileged, already held or exceeds capacity.
 pub fn bind_batch<const G:usize>(&mut self,auth:&Access<G>,
  owner:Identity,ports:&[u16],now:u64,deadline:u64)->Result<usize,Error>{
  if ports.is_empty()||ports.len()>N{return Err(Error::Invalid);}
  let mut staged=Self{leases:self.leases};
  for &port in ports{
   staged.bind(auth,owner,port,now,deadline)?;
  }
  self.leases=staged.leases;Ok(ports.len())
 }
 /// Release only an exact process incarnation's port; stale recycled
 /// PIDs cannot free another process's active network binding.
 pub fn release(&mut self,owner:Identity,port:u16)->Result<(),Error>{
  if owner.pid==0||owner.generation==0||port==0{return Err(Error::Invalid);}
  let item=self.leases.iter_mut().find(|item|
   item.is_some_and(|x|x.port==port)).ok_or(Error::Denied)?;
  if item.is_none_or(|x|x.owner!=owner){return Err(Error::Denied);}
  *item=None;Ok(())
 }
 /// Reclaim expired numeric leases; caller may retry a denied binding
 /// once the trusted monotonic clock has advanced.
 pub fn reap(&mut self,now:u64)->usize{
  let mut reclaimed=0;
  for item in &mut self.leases{
   if item.is_some_and(|x|now>=x.deadline){
    *item=None;reclaimed+=1;
   }
  }
  reclaimed
 }
 /// Copy only leases belonging to the authenticated process incarnation.
 /// Short buffers do not expose partial state.
 pub fn list_owner(&self,owner:Identity,now:u64,
  out:&mut[PortLease])->Result<usize,Error>{
  if owner.pid==0||owner.generation==0{return Err(Error::Invalid);}
  let count=self.leases.iter().flatten()
   .filter(|x|x.owner==owner&&x.deadline>now).count();
  if out.len()<count{return Err(Error::Full);}
  let mut pos=0;
  for entry in self.leases.iter().flatten(){
   if entry.owner==owner&&entry.deadline>now{
    out[pos]=*entry;pos+=1;
   }
  }
  Ok(pos)
 }
}
#[cfg(test)]
mod orbit_r005_port_tests{
 use super::*;
 fn owner()->Identity{Identity{pid:22,generation:9}}
 fn acl()->Access<1>{
  let mut a=Access::new();a.grant(owner(),SOCKET,100).unwrap();a
 }
 #[test]fn atomic_port_group_enforces_capacity_conflicts_and_scope(){
  let mut ports=Ports::<2>::new();
  assert_eq!(ports.bind_batch(&acl(),owner(),&[8080,8081],1,30),Ok(2));
  assert_eq!(ports.bind_batch(&acl(),owner(),&[8082],1,30),Err(Error::Full));
  assert_eq!(ports.release(Identity{generation:10,..owner()},8080),Err(Error::Denied));
  assert_eq!(ports.release(owner(),8080),Ok(()));
  assert_eq!(ports.require(owner(),8081,1),Ok(()));
 }
 #[test]fn failed_batch_has_no_published_side_effects(){
  let mut ports=Ports::<2>::new();
  assert_eq!(ports.bind_batch(&acl(),owner(),&[9000,9000],1,40),Err(Error::Conflict));
  let mut out=[PortLease{owner:owner(),port:0,deadline:0};2];
  assert_eq!(ports.list_owner(owner(),1,&mut out),Ok(0));
  assert_eq!(ports.bind_batch(&acl(),owner(),&[1000,9000],1,40),Err(Error::Invalid));
  assert_eq!(ports.list_owner(owner(),1,&mut out),Ok(0));
 }
 #[test]fn expiry_releases_ports_and_preserves_owner_privacy(){
  let mut ports=Ports::<2>::new();
  ports.bind(&acl(),owner(),8000,1,5).unwrap();
  let mut out=[PortLease{owner:owner(),port:0,deadline:0};1];
  assert_eq!(ports.list_owner(Identity{pid:7,generation:1},1,&mut out),Ok(0));
  assert_eq!(ports.reap(5),1);
  assert_eq!(ports.list_owner(owner(),5,&mut out),Ok(0));
 }
}
