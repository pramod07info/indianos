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
 /// Reduce an existing grant's capabilities and lifetime without
 /// extending authority. Exact scheduler-provided PID generation required.
 /// Invalid attempts leave the original grant byte-for-byte unchanged.
 pub fn attenuate(&mut self,owner:Identity,now:u64,
   rights:u32,expires:u64)->Result<(),Error>{
  if owner.pid==0||owner.generation==0||rights==0||
     rights&!ALL!=0||expires<=now{return Err(Error::Invalid);}
  let slot=self.grants.iter_mut().find(|slot|
     slot.is_some_and(|g|g.owner.pid==owner.pid))
     .ok_or(Error::NotFound)?;
  let grant=slot.as_mut().ok_or(Error::NotFound)?;
  if grant.owner!=owner||grant.revoked{return Err(Error::Denied);}
  if now>=grant.expires{return Err(Error::Expired);}
  if rights&grant.rights!=rights||expires>grant.expires{
   return Err(Error::Denied);
  }
  grant.rights=rights;grant.expires=expires;
  Ok(())
 }
 /// Stage a complete set of least-privilege grants atomically. Required
 /// caller approvals and signer verification must occur outside this table.
 pub fn grant_batch(&mut self,requests:&[(Identity,u32,u64)])
  ->Result<usize,Error>{
  if requests.is_empty()||requests.len()>N{return Err(Error::Full);}
  let mut staged=Self{grants:self.grants};
  for &(owner,rights,deadline) in requests{
   staged.grant(owner,rights,deadline)?;
  }
  self.grants=staged.grants;Ok(requests.len())
 }
 /// Apply multiple restricted scopes in one transaction, disallowing
 /// expired calls, cross-incarnation PID reuse and right escalations.
 pub fn attenuate_batch(&mut self,now:u64,
  changes:&[(Identity,u32,u64)])->Result<usize,Error>{
  if changes.is_empty()||changes.len()>N{return Err(Error::Invalid);}
  let mut staged=Self{grants:self.grants};
  for &(owner,rights,deadline) in changes{
   staged.attenuate(owner,now,rights,deadline)?;
  }
  self.grants=staged.grants;Ok(changes.len())
 }
 /// Remove all expired temporary permissions with a deterministic
 /// deadline. This does not renew anything or synthesize implicit grants.
 pub fn reap_expired(&mut self,now:u64)->usize{
  let mut count=0;
  for entry in &mut self.grants{
   if entry.is_some_and(|g|now>=g.expires){
    *entry=None;count+=1;
   }
  }
  count
 }
 /// Read-only preflight for an exact scheduler identity. Returning rights
 /// does not grant the caller any new privilege.
 pub fn active_rights(&self,owner:Identity,now:u64)->Result<u32,Error>{
  self.require(owner,VIEW,now).or_else(|_|{
   // VIEW is not mandatory for a minimal socket-only app, but its
   // exact identity and expiry still must be checked.
   let entry=self.grants.iter().flatten().find(|x|x.owner.pid==owner.pid)
    .ok_or(Error::NotFound)?;
   if entry.owner!=owner||entry.revoked{return Err(Error::Denied);}
   if now>=entry.expires{return Err(Error::Expired);}
   Ok(())
  })?;
  let entry=self.grants.iter().flatten().find(|x|x.owner==owner)
   .ok_or(Error::NotFound)?;
  Ok(entry.rights)
 }
 /// Revocation of multiple exact process incarnations is all-or-nothing.
 pub fn revoke_batch(&mut self,owners:&[Identity])->Result<usize,Error>{
  if owners.is_empty()||owners.len()>N{return Err(Error::Invalid);}
  let mut staged=Self{grants:self.grants};
  for (i,&owner) in owners.iter().enumerate(){
   if owners[..i].contains(&owner){return Err(Error::Duplicate);}
   staged.revoke(owner)?;
  }
  self.grants=staged.grants;Ok(owners.len())
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

 #[test]fn attenuation_can_only_reduce_process_capabilities_and_deadline(){
  let owner=Identity{pid:4,generation:1};
  let mut grants=Access::<1>::new();
  grants.grant(owner,VIEW|FILE_READ|FILE_WRITE,100).unwrap();
  assert_eq!(grants.attenuate(owner,10,VIEW|FILE_READ,90),Ok(()));
  assert_eq!(grants.require(owner,FILE_READ,12),Ok(()));
  assert_eq!(grants.require(owner,FILE_WRITE,12),Err(Error::Denied));
  assert_eq!(grants.attenuate(owner,20,VIEW|FILE_WRITE,80),Err(Error::Denied));
  assert_eq!(grants.attenuate(owner,20,VIEW,120),Err(Error::Denied));
  assert_eq!(grants.require(owner,FILE_READ,80),Ok(()));
  assert_eq!(grants.attenuate(owner,20,VIEW,35),Ok(()));
  assert_eq!(grants.require(owner,VIEW,35),Err(Error::Expired));
 }
 #[test]fn expired_recycled_and_invalid_attenuation_cannot_regrant_rights(){
  let owner=Identity{pid:9,generation:3};
  let mut grants=Access::<1>::new();
  grants.grant(owner,VIEW|SOCKET,50).unwrap();
  assert_eq!(grants.attenuate(Identity{generation:4,..owner},3,VIEW,30),
    Err(Error::Denied));
  assert_eq!(grants.attenuate(owner,3,0,30),Err(Error::Invalid));
  assert_eq!(grants.attenuate(owner,3,VIEW,3),Err(Error::Invalid));
  assert_eq!(grants.require(owner,SOCKET,4),Ok(()));
  assert_eq!(grants.attenuate(owner,50,VIEW,60),Err(Error::Expired));
  assert_eq!(grants.require(owner,SOCKET,50),Err(Error::Expired));
 }

 #[test]fn orbit_permissions_batch_grants_and_revocations_are_atomic(){
  let a=Identity{pid:1,generation:1};let b=Identity{pid:2,generation:1};
  let mut c=Access::<2>::new();
  assert_eq!(c.grant_batch(&[(a,VIEW,50),(a,FILE_READ,50)]),
             Err(Error::Duplicate));
  assert_eq!(c.require(a,VIEW,1),Err(Error::NotFound));
  assert_eq!(c.grant_batch(&[(a,VIEW|FILE_READ,50),(b,SOCKET,60)]),Ok(2));
  assert_eq!(c.active_rights(b,1),Ok(SOCKET));
  assert_eq!(c.revoke_batch(&[a,a]),Err(Error::Duplicate));
  assert_eq!(c.require(a,VIEW,1),Ok(()));
  assert_eq!(c.revoke_batch(&[a,b]),Ok(2));
  assert_eq!(c.require(b,SOCKET,1),Err(Error::NotFound));
 }
 #[test]fn orbit_permissions_bulk_attenuation_never_escalates(){
  let a=Identity{pid:1,generation:1};let b=Identity{pid:2,generation:1};
  let mut c=Access::<2>::new();
  c.grant_batch(&[(a,VIEW|FILE_READ,80),(b,SOCKET,70)]).unwrap();
  assert_eq!(c.attenuate_batch(2,&[(a,VIEW,60),(b,DRIVER,60)]),
             Err(Error::Denied));
  assert_eq!(c.require(a,FILE_READ,4),Ok(()));
  assert_eq!(c.attenuate_batch(2,&[(a,VIEW,60),(b,SOCKET,55)]),Ok(2));
  assert_eq!(c.require(a,FILE_READ,4),Err(Error::Denied));
  assert_eq!(c.active_rights(b,56),Err(Error::Expired));
  assert_eq!(c.reap_expired(60),2);
  assert_eq!(c.require(a,VIEW,60),Err(Error::NotFound));
 }

}

/// Orbit R005 kernel policy extensions. The real syscall dispatcher must
/// supply identities from its PCB and invoke these checks at the boundary.
impl<const N:usize> Access<N>{
 /// Issue a tightly bounded temporary grant; no deadline arithmetic wrap,
 /// zero duration, or time window exceeding supervisor policy.
 pub fn grant_temporary(&mut self,owner:Identity,rights:u32,
  now:u64,duration:u64,max_duration:u64)->Result<u64,Error>{
  if duration==0||max_duration==0||duration>max_duration{
   return Err(Error::Invalid);
  }
  let deadline=now.checked_add(duration).ok_or(Error::Invalid)?;
  self.grant(owner,rights,deadline)?;
  Ok(deadline)
 }
 /// Preflight several syscall privileges before making any state change.
 /// Caller must still commit operations transactionally after this check.
 pub fn authorize_many(&self,requirements:&[(Identity,u32)],
  now:u64)->Result<usize,Error>{
  if requirements.is_empty()||requirements.len()>N{return Err(Error::Invalid);}
  for &(owner,rights) in requirements{
   self.require(owner,rights,now)?;
  }
  Ok(requirements.len())
 }
 /// Security incident lockdown: trusted supervisor strips mutation,
 /// installer and hardware-driver rights from all existing grants.
 /// Read-only session access and network grants may remain.
 pub fn lockdown_mutations(&mut self)->usize{
  let blocked=FILE_WRITE|INSTALL|DRIVER;
  let mut affected=0;
  for slot in &mut self.grants{
   if let Some(mut grant)=*slot{
    if grant.rights&blocked!=0{
     affected+=1;
     grant.rights&=!blocked;
     *slot=if grant.rights==0{None}else{Some(grant)};
    }
   }
  }
  affected
 }
 /// Exact caller-identity introspection for a permission review dialog.
 /// Does not enumerate other users' capabilities.
 pub fn has_permission(&self,owner:Identity,required:u32,now:u64)->bool{
  self.require(owner,required,now).is_ok()
 }
}
#[cfg(test)]
mod orbit_r005_kernel_access_tests{
 use super::*;
 fn user()->Identity{Identity{pid:101,generation:3}}
 #[test]fn bounded_grants_never_overflow_or_silently_escalate(){
  let mut a=Access::<1>::new();
  assert_eq!(a.grant_temporary(user(),FILE_READ,10,0,30),Err(Error::Invalid));
  assert_eq!(a.grant_temporary(user(),FILE_READ,u64::MAX,2,30),Err(Error::Invalid));
  assert_eq!(a.grant_temporary(user(),FILE_READ,10,40,30),Err(Error::Invalid));
  assert_eq!(a.grant_temporary(user(),FILE_READ,10,15,30),Ok(25));
  assert!(a.has_permission(user(),FILE_READ,24));
  assert!(!a.has_permission(user(),FILE_READ,25));
 }
 #[test]fn incident_lockdown_revokes_every_mutating_scope(){
  let mut a=Access::<3>::new();
  a.grant(user(),VIEW|FILE_READ|FILE_WRITE|INSTALL,100).unwrap();
  let second=Identity{pid:102,generation:1};
  a.grant(second,DRIVER,100).unwrap();
  assert_eq!(a.lockdown_mutations(),2);
  assert_eq!(a.require(user(),FILE_READ,1),Ok(()));
  assert_eq!(a.require(user(),FILE_WRITE,1),Err(Error::Denied));
  assert_eq!(a.require(second,DRIVER,1),Err(Error::NotFound));
  assert_eq!(a.lockdown_mutations(),0);
 }
 #[test]fn authorizing_multiple_calls_rejects_stale_or_missing_rights(){
  let mut a=Access::<2>::new();
  a.grant(user(),VIEW|FILE_READ,100).unwrap();
  let recycled=Identity{generation:4,..user()};
  assert_eq!(a.authorize_many(&[(user(),VIEW),(recycled,FILE_READ)],1),
   Err(Error::Denied));
  assert_eq!(a.authorize_many(&[(user(),VIEW),(user(),FILE_WRITE)],1),
   Err(Error::Denied));
  assert_eq!(a.authorize_many(&[(user(),VIEW),(user(),FILE_READ)],1),Ok(2));
 }
}
