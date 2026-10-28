//! R022/17: generation-locked rights leases for workstation file/process actions.
//! Enforces expiration, revocation, and least-rights masks before privileged use.
use super::Error;
use crate::batch5_session::Identity;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Lease{pub id:u32,pub owner:Identity,pub rights:u32,
 pub expires:u64,pub uses_remaining:u16,pub revoked:bool}
pub struct Authority<const N:usize>{leases:[Option<Lease>;N]}
impl<const N:usize> Authority<N>{
 pub const fn new()->Self{Self{leases:[None;N]}}
 fn index(&self,id:u32)->Option<usize>{
  self.leases.iter().position(|l|l.is_some_and(|l|l.id==id))
 }
 pub fn issue(&mut self,supervisor:bool,lease:Lease)->Result<(),Error>{
  if !supervisor||lease.id==0||lease.owner.pid==0||
   lease.owner.generation==0||lease.rights==0||lease.expires==0||
   lease.uses_remaining==0{return Err(Error::Denied);}
  if self.index(lease.id).is_some(){return Err(Error::Conflict);}
  let at=self.leases.iter().position(Option::is_none).ok_or(Error::Full)?;
  let mut l=lease;l.revoked=false;self.leases[at]=Some(l);Ok(())
 }
 pub fn use_rights(&mut self,actor:Identity,id:u32,rights:u32,
  now:u64)->Result<u16,Error>{
  let i=self.index(id).ok_or(Error::Missing)?;
  let mut l=self.leases[i].ok_or(Error::Missing)?;
  if actor.pid==0||actor!=l.owner||l.revoked||now>=l.expires||
   rights==0||rights&l.rights!=rights||l.uses_remaining==0{
   return Err(Error::Denied);
  }
  l.uses_remaining-=1;self.leases[i]=Some(l);Ok(l.uses_remaining)
 }
 pub fn revoke(&mut self,supervisor:bool,id:u32)->Result<(),Error>{
  if !supervisor{return Err(Error::Denied);}
  let i=self.index(id).ok_or(Error::Missing)?;
  let mut l=self.leases[i].ok_or(Error::Missing)?;
  l.revoked=true;l.uses_remaining=0;self.leases[i]=Some(l);Ok(())
 }
 pub fn retire_session(&mut self,supervisor:bool,actor:Identity)
  ->Result<usize,Error>{
  if !supervisor{return Err(Error::Denied);}
  let mut n=0;
  for item in &mut self.leases{
   if item.is_some_and(|l|l.owner==actor){*item=None;n+=1;}
  }Ok(n)
 }
 pub fn remaining(&self,actor:Identity,id:u32)->Result<u16,Error>{
  let l=self.index(id).and_then(|i|self.leases[i]).ok_or(Error::Missing)?;
  if actor.pid==0||actor!=l.owner{return Err(Error::Denied);}
  if l.revoked{return Ok(0);}Ok(l.uses_remaining)
 }
}
#[cfg(test)]mod tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:7,generation:g}}
 fn lease()->Lease{Lease{id:1,owner:id(1),rights:3,expires:100,
  uses_remaining:2,revoked:false}}
 #[test]fn file_and_build_capabilities_have_bounded_use_counts(){
  let mut a=Authority::<1>::new();a.issue(true,lease()).unwrap();
  assert_eq!(a.use_rights(id(1),1,1,1),Ok(1));
  assert_eq!(a.use_rights(id(1),1,2,2),Ok(0));
  assert_eq!(a.use_rights(id(1),1,1,3),Err(Error::Denied));
 }
 #[test]fn reused_pid_cannot_reuse_old_generation_capability(){
  let mut a=Authority::<1>::new();a.issue(true,lease()).unwrap();
  assert_eq!(a.use_rights(id(2),1,1,1),Err(Error::Denied));
  assert_eq!(a.use_rights(id(1),1,4,1),Err(Error::Denied));
 }
 #[test]fn revocation_prevents_remaining_privileged_actions(){
  let mut a=Authority::<1>::new();a.issue(true,lease()).unwrap();
  a.revoke(true,1).unwrap();
  assert_eq!(a.remaining(id(1),1),Ok(0));
  assert_eq!(a.use_rights(id(1),1,1,1),Err(Error::Denied));
 }
}
