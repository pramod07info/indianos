//! R040 tasks 4/7/13/22: least-privilege, PID-epoch-scoped syscall rights
//! and capability expiration. Kernel derives Identity from scheduler PCB.
use crate::batch5_session::Identity;
pub const VIEW:u32=1;pub const FILE_READ:u32=2;pub const FILE_WRITE:u32=4;
pub const INSTALL:u32=8;pub const SOCKET:u32=16;pub const DRIVER:u32=32;
pub const ALL:u32=VIEW|FILE_READ|FILE_WRITE|INSTALL|SOCKET|DRIVER;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Full,Duplicate,Denied,Expired,NotFound}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Grant{owner:Identity,rights:u32,expires:u64,revoked:bool}
pub struct Access<const N:usize>{grants:[Option<Grant>;N]}
impl<const N:usize> Access<N>{
 pub const fn new()->Self{Self{grants:[None;N]}}
 pub fn grant(&mut self,owner:Identity,rights:u32,expires:u64)
  ->Result<(),Error>{
  if owner.pid==0||owner.generation==0||rights==0||rights&!ALL!=0||
   expires==0{return Err(Error::Invalid);}
  if self.grants.iter().flatten().any(|g|g.owner.pid==owner.pid){
   return Err(Error::Duplicate);
  }
  let slot=self.grants.iter_mut().find(|x|x.is_none()).ok_or(Error::Full)?;
  *slot=Some(Grant{owner,rights,expires,revoked:false});Ok(())
 }
 pub fn require(&self,owner:Identity,needed:u32,now:u64)->Result<(),Error>{
  if owner.pid==0||owner.generation==0||needed==0||needed&!ALL!=0{
   return Err(Error::Invalid);
  }
  let g=self.grants.iter().flatten().find(|g|g.owner.pid==owner.pid)
   .ok_or(Error::NotFound)?;
  if g.owner!=owner||g.revoked{return Err(Error::Denied);}
  if now>=g.expires{return Err(Error::Expired);}
  if g.rights&needed!=needed{return Err(Error::Denied);}
  Ok(())
 }
 pub fn revoke(&mut self,owner:Identity)->Result<(),Error>{
  let slot=self.grants.iter_mut().find(|s|s.is_some_and(|g|g.owner==owner))
   .ok_or(Error::NotFound)?;
  *slot=None;Ok(())
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn cannot_reuse_pid_credentials_or_escalate(){
  let a=Identity{pid:1,generation:5};let mut c=Access::<1>::new();
  c.grant(a,VIEW|FILE_READ,100).unwrap();
  assert_eq!(c.require(a,FILE_READ,99),Ok(()));
  assert_eq!(c.require(a,FILE_WRITE,99),Err(Error::Denied));
  assert_eq!(c.require(a,VIEW,100),Err(Error::Expired));
  assert_eq!(c.require(Identity{pid:1,generation:6},VIEW,0),Err(Error::Denied));
  c.revoke(a).unwrap();assert_eq!(c.require(a,VIEW,0),Err(Error::NotFound));
 }
}