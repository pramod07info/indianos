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
  // A live grant monopolizes a numeric PID. A revoked tombstone must
  // never prevent a new scheduler-authenticated generation from starting.
  if self.grants.iter().flatten().any(|g|
      g.owner.pid==owner.pid&&!g.revoked){
   return Err(Error::Duplicate);
  }
  // Reuse the same PID's tombstone first: require() finds by numeric PID,
  // so retaining an old entry beside a new one could deny valid lookups.
  let index=self.grants.iter().position(|slot|
      slot.is_some_and(|g|g.owner.pid==owner.pid&&g.revoked))
    .or_else(||self.grants.iter().position(|s|s.is_none()))
    .or_else(||self.grants.iter().position(|s|s.is_some_and(|g|g.revoked)))
    .ok_or(Error::Full)?;
  self.grants[index]=Some(Grant{owner,rights,expires,revoked:false});Ok(())
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


/// R006 bounded security decision journal. Checksum detects accidental
/// corruption only; persistent tamper-evident audit trails require trusted
/// storage, authentication and an independently reviewed MAC/signature.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum AuditKind{Allowed,Denied,Incident}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct AuditEvent{
 pub owner:Identity,pub requested:u32,pub time:u64,
 pub kind:AuditKind,pub serial:u64,pub checksum:u32,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum AuditError{Full,Invalid,Corrupt,Denied}
pub struct SecurityAudit<const N:usize>{
 entries:[Option<AuditEvent>;N],used:usize,next:u64,chain:u32,
}
fn mix_audit(mut value:u32,bytes:&[u8])->u32{
 for &b in bytes{value=(value^(b as u32)).wrapping_mul(0x01000193);}
 value
}
fn event_hash(seed:u32,event:AuditEvent)->u32{
 let h=mix_audit(seed,&event.owner.pid.to_le_bytes());
 let h=mix_audit(h,&event.owner.generation.to_le_bytes());
 let h=mix_audit(h,&event.requested.to_le_bytes());
 let h=mix_audit(h,&event.time.to_le_bytes());
 let h=mix_audit(h,&event.serial.to_le_bytes());
 mix_audit(h,&[match event.kind{
  AuditKind::Allowed=>1,AuditKind::Denied=>2,AuditKind::Incident=>3}])
}
impl<const N:usize> SecurityAudit<N>{
 pub const fn new()->Self{Self{
  entries:[None;N],used:0,next:1,chain:0x811c9dc5,
 }}
 pub fn remaining(&self)->usize{N-self.used}
 pub fn record(&mut self,owner:Identity,requested:u32,
  now:u64,kind:AuditKind)->Result<u64,AuditError>{
  if owner.pid==0||owner.generation==0||requested&!ALL!=0||
   (requested==0&&kind!=AuditKind::Incident){
   return Err(AuditError::Invalid);
  }
  if self.used>=N||self.next==u64::MAX{return Err(AuditError::Full);}
  let mut event=AuditEvent{owner,requested,time:now,
   kind,serial:self.next,checksum:0};
  event.checksum=event_hash(self.chain,event);
  self.entries[self.used]=Some(event);
  self.used+=1;self.next+=1;self.chain=event.checksum;
  Ok(event.serial)
 }
 pub fn validate(&self)->Result<(),AuditError>{
  let mut hash=0x811c9dc5;
  for item in &self.entries[..self.used]{
   let e=item.ok_or(AuditError::Corrupt)?;
   if e.serial==0||e.checksum!=event_hash(hash,e){
    return Err(AuditError::Corrupt);
   }
   hash=e.checksum;
  }
  if hash!=self.chain{return Err(AuditError::Corrupt);}
  Ok(())
 }
 pub fn read_owner(&self,owner:Identity,out:&mut[AuditEvent])
  ->Result<usize,AuditError>{
  if owner.pid==0||owner.generation==0{return Err(AuditError::Invalid);}
  self.validate()?;
  let count=self.entries[..self.used].iter().flatten()
   .filter(|e|e.owner==owner).count();
  if count>out.len(){return Err(AuditError::Full);}
  let mut n=0;
  for event in self.entries[..self.used].iter().flatten(){
   if event.owner==owner{out[n]=*event;n+=1;}
  }
  Ok(n)
 }
}
impl<const N:usize> Access<N>{
 /// A full audit journal fails closed, including legitimate allow requests.
 /// Record the authorization decision before returning it to syscall ingress.
 pub fn require_audited<const L:usize>(&self,audit:&mut SecurityAudit<L>,
  owner:Identity,right:u32,now:u64)->Result<(),AuditError>{
  if owner.pid==0||owner.generation==0{return Err(AuditError::Invalid);}
  let check=self.require(owner,right,now);
  audit.record(owner,right,now,if check.is_ok(){
   AuditKind::Allowed}else{AuditKind::Denied})?;
  check.map_err(|_|AuditError::Denied)
 }
 pub fn lockdown_audited<const L:usize>(&mut self,
  audit:&mut SecurityAudit<L>,trusted_supervisor:Identity,
  now:u64)->Result<usize,AuditError>{
  // Supervisor identification must come from a trusted PCB, not user bytes.
  audit.record(trusted_supervisor,FILE_WRITE|INSTALL|DRIVER,
   now,AuditKind::Incident)?;
  Ok(self.lockdown_mutations())
 }
}
#[cfg(test)]
mod r006_security_journal_tests{
 use super::*;
 fn a()->Identity{Identity{pid:41,generation:2}}
 fn b()->Identity{Identity{pid:41,generation:3}}
 const EMPTY:AuditEvent=AuditEvent{
  owner:Identity{pid:0,generation:0},requested:0,time:0,
  kind:AuditKind::Denied,serial:0,checksum:0,
 };
 #[test]fn allow_deny_and_lockdown_are_logged_in_order(){
  let mut access=Access::<2>::new();
  access.grant(a(),FILE_READ|FILE_WRITE,50).unwrap();
  let mut audit=SecurityAudit::<4>::new();
  assert_eq!(access.require_audited(&mut audit,a(),FILE_READ,2),Ok(()));
  assert_eq!(access.require_audited(&mut audit,b(),FILE_READ,2),Err(AuditError::Denied));
  assert_eq!(access.lockdown_audited(&mut audit,a(),3),Ok(1));
  assert_eq!(access.require(a(),FILE_WRITE,3),Err(Error::Denied));
  assert_eq!(audit.validate(),Ok(()));
  let mut owner=[EMPTY;3];
  assert_eq!(audit.read_owner(a(),&mut owner),Ok(2));
  assert_eq!(owner[0].kind,AuditKind::Allowed);
  assert_eq!(owner[1].kind,AuditKind::Incident);
 }
 #[test]fn full_journal_refuses_unrecorded_privileged_action(){
  let mut access=Access::<1>::new();access.grant(a(),FILE_WRITE,10).unwrap();
  let mut audit=SecurityAudit::<1>::new();
  assert!(access.require_audited(&mut audit,a(),FILE_WRITE,1).is_ok());
  assert_eq!(access.require_audited(&mut audit,a(),FILE_WRITE,2),Err(AuditError::Full));
  assert_eq!(access.lockdown_audited(&mut audit,a(),3),Err(AuditError::Full));
  assert_eq!(access.require(a(),FILE_WRITE,3),Ok(()));
 }
 #[test]fn stale_owners_cannot_read_other_generation_and_short_output_is_atomic(){
  let mut audit=SecurityAudit::<2>::new();
  audit.record(a(),VIEW,1,AuditKind::Allowed).unwrap();
  audit.record(a(),VIEW,2,AuditKind::Denied).unwrap();
  let mut short=[EMPTY;1];
  assert_eq!(audit.read_owner(a(),&mut short),Err(AuditError::Full));
  assert_eq!(short,[EMPTY]);
  let mut out=[EMPTY;2];
  assert_eq!(audit.read_owner(b(),&mut out),Ok(0));
  assert_eq!(audit.validate(),Ok(()));
 }
}

 
/// R009 privileged capability rotation for process restarts. A restart
/// inherits at most the old process rights and never extends expiration.
/// Caller must provide kernel-authenticated old and new identities.
impl<const N:usize> Access<N>{
 pub fn rotate_process_grant(&mut self,old:Identity,new:Identity,
   rights:u32,expires:u64,now:u64)->Result<(),Error>{
  if old.pid==0||old.generation==0||new.pid==0||new.generation==0||
     old==new||rights==0||rights&!ALL!=0||expires<=now{
    return Err(Error::Invalid);
  }
  let index=self.grants.iter().position(|slot|
    slot.is_some_and(|g|g.owner==old)).ok_or(Error::NotFound)?;
  let grant=self.grants[index].ok_or(Error::NotFound)?;
  if grant.revoked||now>=grant.expires{return Err(Error::Denied);}
  if rights&grant.rights!=rights||expires>grant.expires{return Err(Error::Denied);}
  if self.grants.iter().enumerate().any(|(i,s)|i!=index&&
     s.is_some_and(|g|g.owner.pid==new.pid)){return Err(Error::Duplicate);}
  self.grants[index]=Some(Grant{owner:new,rights,expires,revoked:false});
  Ok(())
 }
}
#[cfg(test)]
mod r009_capability_rotation_tests{
 use super::*;
 fn ident(pid:u32,generation:u64)->Identity{Identity{pid,generation}}
 #[test]fn restart_attenuates_and_rejects_stale_process(){
  let mut a=Access::<2>::new();let old=ident(9,3);let fresh=ident(9,4);
  a.grant(old,FILE_READ|FILE_WRITE,90).unwrap();
  a.rotate_process_grant(old,fresh,FILE_READ,70,10).unwrap();
  assert_eq!(a.require(old,FILE_READ,11),Err(Error::Denied));
  assert_eq!(a.require(fresh,FILE_READ,11),Ok(()));
  assert_eq!(a.require(fresh,FILE_WRITE,11),Err(Error::Denied));
  assert_eq!(a.require(fresh,FILE_READ,70),Err(Error::Expired));
 }
 #[test]fn forbidden_escalation_has_no_side_effect(){
  let mut a=Access::<1>::new();let old=ident(3,1);let fresh=ident(3,2);
  a.grant(old,VIEW,30).unwrap();
  assert_eq!(a.rotate_process_grant(old,fresh,FILE_WRITE,20,1),Err(Error::Denied));
  assert_eq!(a.require(old,VIEW,2),Ok(()));
  assert_eq!(a.rotate_process_grant(old,fresh,VIEW,31,1),Err(Error::Denied));
  assert_eq!(a.require(old,VIEW,2),Ok(()));
 }
 #[test]fn foreign_pid_collision_is_rejected_before_rotation(){
  let mut a=Access::<2>::new();let old=ident(4,1);let foreign=ident(5,1);
  a.grant(old,VIEW,60).unwrap();a.grant(foreign,VIEW,60).unwrap();
  assert_eq!(a.rotate_process_grant(old,ident(5,2),VIEW,30,1),Err(Error::Duplicate));
  assert_eq!(a.require(old,VIEW,2),Ok(()));
 }
}

 
/// R010 atomic privilege handoff. No process receives a partial grant set:
/// every requested generation/rights change must be valid before publication.
/// The trusted scheduler must authenticate every Identity.
impl<const N:usize> Access<N>{
 pub fn rotate_batch_atomic(&mut self,requests:&[(Identity,Identity,u32,u64)],
  now:u64)->Result<usize,Error>{
  if requests.is_empty()||requests.len()>N{return Err(Error::Invalid);}
  let mut staged=Self{grants:self.grants};
  for &(old,new,rights,deadline) in requests{
   staged.rotate_process_grant(old,new,rights,deadline,now)?;
  }
  self.grants=staged.grants;
  Ok(requests.len())
 }
 /// A terminating process cannot transfer rights via its retired PID.
 pub fn revoke_pid(&mut self,pid:u32)->usize{
  if pid==0{return 0;}
  let mut count=0;
  for slot in &mut self.grants{
   if let Some(grant)=slot.as_mut(){
    if grant.owner.pid==pid&&!grant.revoked{grant.revoked=true;count+=1;}
   }
  }
  count
 }
}
#[cfg(test)]
mod r010_atomic_identity_handoff_tests{
 use super::*;
 fn id(pid:u32,generation:u64)->Identity{Identity{pid,generation}}
 #[test]fn batch_rotates_two_grants_only_after_all_preconditions(){
  let mut a=Access::<3>::new();
  a.grant(id(1,1),VIEW|FILE_READ,80).unwrap();
  a.grant(id(2,1),VIEW|SOCKET,90).unwrap();
  assert_eq!(a.rotate_batch_atomic(&[
    (id(1,1),id(1,2),VIEW,60),(id(2,1),id(2,2),SOCKET,65)],10),Ok(2));
  assert_eq!(a.require(id(1,1),VIEW,15),Err(Error::Denied));
  assert_eq!(a.require(id(1,2),VIEW,15),Ok(()));
  assert_eq!(a.require(id(2,2),SOCKET,15),Ok(()));
 }
 #[test]fn failing_second_rotation_keeps_every_previous_grant(){
  let mut a=Access::<2>::new();a.grant(id(1,1),VIEW,80).unwrap();
  a.grant(id(2,1),FILE_READ,80).unwrap();
  assert_eq!(a.rotate_batch_atomic(&[
    (id(1,1),id(1,2),VIEW,60),
    (id(2,1),id(2,2),FILE_WRITE,60)],10),Err(Error::Denied));
  assert_eq!(a.require(id(1,1),VIEW,20),Ok(()));
  assert_eq!(a.require(id(1,2),VIEW,20),Err(Error::Denied));
 }
 #[test]fn retiring_pid_clears_only_its_grant(){
  let mut a=Access::<2>::new();
  a.grant(id(1,2),VIEW,90).unwrap();a.grant(id(2,2),FILE_READ,90).unwrap();
  assert_eq!(a.revoke_pid(1),1);
  assert_eq!(a.revoke_pid(1),0);
  assert_eq!(a.require(id(1,2),VIEW,10),Err(Error::Denied));
  assert_eq!(a.require(id(2,2),FILE_READ,10),Ok(()));
 }
}

 
/// R011 journaled capability mutation. Both side effects commit together:
/// neither a partial grant transfer nor a missing audit entry is accepted.
impl<const N:usize> Access<N>{
 pub fn rotate_audited<const L:usize>(
  &mut self,journal:&mut SecurityAudit<L>,
  requests:&[(Identity,Identity,u32,u64)],now:u64
 )->Result<usize,AuditError>{
  journal.validate()?;
  if requests.is_empty()||journal.remaining()<requests.len(){
   return Err(AuditError::Full);
  }
  let mut grants=Self{grants:self.grants};
  grants.rotate_batch_atomic(requests,now).map_err(|_|AuditError::Denied)?;
  let mut staged=SecurityAudit{
   entries:journal.entries,used:journal.used,next:journal.next,chain:journal.chain,
  };
  for &(_,new,rights,_) in requests{
   staged.record(new,rights,now,AuditKind::Allowed)?;
  }
  self.grants=grants.grants;*journal=staged;Ok(requests.len())
 }
 pub fn revoke_pid_audited<const L:usize>(
  &mut self,journal:&mut SecurityAudit<L>,pid:u32,now:u64
 )->Result<usize,AuditError>{
  if pid==0{return Err(AuditError::Invalid);}
  journal.validate()?;
  let matches=self.grants.iter().flatten().filter(|g|
     g.owner.pid==pid&&!g.revoked).count();
  if matches>journal.remaining(){return Err(AuditError::Full);}
  let mut next=SecurityAudit{
   entries:journal.entries,used:journal.used,next:journal.next,chain:journal.chain,
  };
  for grant in self.grants.iter().flatten(){
   if grant.owner.pid==pid&&!grant.revoked{
    next.record(grant.owner,grant.rights,now,AuditKind::Incident)?;
   }
  }
  let count=self.revoke_pid(pid);
  *journal=next;Ok(count)
 }
}
#[cfg(test)]
mod r011_audited_delegation_tests{
 use super::*;
 fn id(pid:u32,gen:u64)->Identity{Identity{pid,generation:gen}}
 #[test]fn journaled_batch_rotates_all_grants_and_chains_events(){
  let mut a=Access::<2>::new();let mut j=SecurityAudit::<3>::new();
  a.grant(id(1,1),VIEW,40).unwrap();a.grant(id(2,1),SOCKET,40).unwrap();
  assert_eq!(a.rotate_audited(&mut j,&[
    (id(1,1),id(1,2),VIEW,30),(id(2,1),id(2,2),SOCKET,30)],10),Ok(2));
  assert_eq!(j.validate(),Ok(()));
  assert_eq!(j.remaining(),1);
  assert_eq!(a.require(id(1,2),VIEW,11),Ok(()));
 }
 #[test]fn full_journal_rejects_grant_handoff_before_mutation(){
  let mut a=Access::<1>::new();let mut j=SecurityAudit::<0>::new();
  a.grant(id(5,1),VIEW,40).unwrap();
  assert_eq!(a.rotate_audited(&mut j,&[(id(5,1),id(5,2),VIEW,20)],10),
    Err(AuditError::Full));
  assert_eq!(a.require(id(5,1),VIEW,11),Ok(()));
 }
 #[test]fn audited_pid_reap_does_not_revoke_unrelated_owner(){
  let mut a=Access::<3>::new();let mut j=SecurityAudit::<3>::new();
  a.grant(id(1,2),VIEW,40).unwrap();a.grant(id(2,2),FILE_READ,40).unwrap();
  assert_eq!(a.revoke_pid_audited(&mut j,1,15),Ok(1));
  assert_eq!(j.validate(),Ok(()));
  assert_eq!(a.require(id(1,2),VIEW,16),Err(Error::Denied));
  assert_eq!(a.require(id(2,2),FILE_READ,16),Ok(()));
  assert_eq!(a.revoke_pid_audited(&mut j,1,17),Ok(0));
 }
}


/// R012: correct PID/epoch-specific process reaping. The earlier PID-wide
/// method can revoke an unrelated fresh generation of the same numeric PID.
/// Call this after trusted scheduler authentication and audit validation.
impl<const N:usize> Access<N>{
 pub fn revoke_identity_audited<const L:usize>(&mut self,
   audit:&mut SecurityAudit<L>,dead:Identity,now:u64)
   ->Result<usize,AuditError>{
  if dead.pid==0||dead.generation==0{return Err(AuditError::Invalid);}
  audit.validate()?;
  let affected=self.grants.iter().flatten().filter(|g|
    g.owner==dead&&!g.revoked).count();
  if affected>audit.remaining(){return Err(AuditError::Full);}
  let mut staged=SecurityAudit{
    entries:audit.entries,used:audit.used,next:audit.next,chain:audit.chain};
  for grant in self.grants.iter().flatten(){
   if grant.owner==dead&&!grant.revoked{
    staged.record(dead,grant.rights,now,AuditKind::Incident)?;
   }
  }
  let mut removed=0;
  for slot in &mut self.grants{
   if let Some(grant)=slot.as_mut(){
    if grant.owner==dead&&!grant.revoked{grant.revoked=true;removed+=1;}
   }
  }
  *audit=staged;Ok(removed)
 }
 pub fn owned_process_count(&self,pid:u32,now:u64)->usize{
  self.grants.iter().flatten().filter(|g|
    g.owner.pid==pid&&!g.revoked&&g.expires>now).count()
 }
}
#[cfg(test)]
mod r012_epoch_audit_tests{
 use super::*;
 fn id(g:u64)->Identity{Identity{pid:42,generation:g}}
 #[test]fn old_generation_reap_preserves_reused_pid_new_grants(){
  let mut a=Access::<2>::new();let mut j=SecurityAudit::<2>::new();
  a.grant(id(1),FILE_READ,100).unwrap();
  // Numeric PID reuse while its predecessor is still alive is forbidden.
  assert_eq!(a.grant(id(2),VIEW,100),Err(Error::Duplicate));
  assert_eq!(a.revoke_identity_audited(&mut j,id(1),10),Ok(1));
  // Once the old generation has been revoked, the kernel may admit a
  // distinct process incarnation without returning the old authority.
  a.grant(id(2),VIEW,100).unwrap();
  assert_eq!(a.require(id(1),FILE_READ,11),Err(Error::Denied));
  assert_eq!(a.require(id(2),VIEW,11),Ok(()));
  assert_eq!(j.validate(),Ok(()));
  assert_eq!(a.owned_process_count(42,11),1);
 }
 #[test]fn audit_capacity_failure_fails_before_revoke(){
  let mut a=Access::<1>::new();let mut j=SecurityAudit::<0>::new();
  a.grant(id(1),VIEW,100).unwrap();
  assert_eq!(a.revoke_identity_audited(&mut j,id(1),10),Err(AuditError::Full));
  assert_eq!(a.require(id(1),VIEW,11),Ok(()));
 }
 #[test]fn stale_reap_does_not_modify_future_grant(){
  let mut a=Access::<1>::new();let mut j=SecurityAudit::<1>::new();
  a.grant(id(2),VIEW,100).unwrap();
  assert_eq!(a.revoke_identity_audited(&mut j,id(1),10),Ok(0));
  assert_eq!(a.require(id(2),VIEW,11),Ok(()));
  assert_eq!(j.remaining(),1);
 }
}

/// R013 audit-backed expiration/reaping. No grant disappears without a
/// journal event, and a full/corrupted audit leaves live state unchanged.
impl<const N:usize> Access<N>{
 pub fn reap_expired_audited<const L:usize>(&mut self,
   audit:&mut SecurityAudit<L>,now:u64)->Result<usize,AuditError>{
  audit.validate()?;
  let count=self.grants.iter().flatten().filter(|g|
    !g.revoked&&now>=g.expires).count();
  if count>audit.remaining(){return Err(AuditError::Full);}
  let mut staged=SecurityAudit{
   entries:audit.entries,used:audit.used,next:audit.next,chain:audit.chain};
  for grant in self.grants.iter().flatten(){
   if !grant.revoked&&now>=grant.expires{
    staged.record(grant.owner,grant.rights,now,AuditKind::Incident)?;
   }
  }
  let mut removed=0;
  for slot in &mut self.grants{
   if slot.is_some_and(|g|now>=g.expires){
    *slot=None;removed+=1;
   }
  }
  *audit=staged;
  Ok(removed)
 }
 /// Exact process-generation security state without exposing raw tokens.
 pub fn owner_rights_checked(&self,owner:Identity,now:u64)
   ->Result<u32,Error>{
  if owner.pid==0||owner.generation==0{return Err(Error::Invalid);}
  let grant=self.grants.iter().flatten().find(|g|g.owner.pid==owner.pid)
    .ok_or(Error::NotFound)?;
  if grant.owner!=owner||grant.revoked{return Err(Error::Denied);}
  if now>=grant.expires{return Err(Error::Expired);}
  Ok(grant.rights)
 }
 pub fn active_grants(&self,now:u64)->usize{
  self.grants.iter().flatten().filter(|g|!g.revoked&&now<g.expires).count()
 }
}
#[cfg(test)]
mod r013_security_reaper_tests{
 use super::*;
 fn id(pid:u32,g:u64)->Identity{Identity{pid,generation:g}}
 #[test]fn expiry_requires_audited_event_and_retains_other_owner(){
  let mut a=Access::<3>::new();let mut journal=SecurityAudit::<3>::new();
  a.grant(id(1,1),FILE_READ,10).unwrap();
  a.grant(id(2,1),VIEW,100).unwrap();
  assert_eq!(a.reap_expired_audited(&mut journal,10),Ok(1));
  assert_eq!(journal.validate(),Ok(()));
  assert_eq!(a.active_grants(10),1);
  assert_eq!(a.require(id(2,1),VIEW,11),Ok(()));
 }
 #[test]fn exhausted_audit_capacity_aborts_cleanup_atomically(){
  let mut a=Access::<1>::new();let mut journal=SecurityAudit::<0>::new();
  a.grant(id(1,1),VIEW,10).unwrap();
  assert_eq!(a.reap_expired_audited(&mut journal,11),Err(AuditError::Full));
  assert_eq!(a.active_grants(1),1);
  assert_eq!(a.owner_rights_checked(id(1,1),1),Ok(VIEW));
 }
 #[test]fn retired_pid_can_reuse_tombstone_without_stale_access(){
  let mut a=Access::<1>::new();let mut journal=SecurityAudit::<1>::new();
  a.grant(id(1,1),FILE_READ,10).unwrap();
  a.reap_expired_audited(&mut journal,10).unwrap();
  a.grant(id(1,2),VIEW,90).unwrap();
  assert_eq!(a.owner_rights_checked(id(1,1),11),Err(Error::Denied));
  assert_eq!(a.owner_rights_checked(id(1,2),11),Ok(VIEW));
 }
}

/// R014 audit-atomic privilege provisioning. The commit is rejected if
/// either the capability table or the append-only journal cannot accept
/// all proposed grants. No half-provisioned desktop session may launch.
impl<const N:usize> Access<N>{
 pub fn grant_batch_audited<const L:usize>(&mut self,
  audit:&mut SecurityAudit<L>,now:u64,grants:&[(Identity,u32,u64)])
  ->Result<usize,AuditError>{
  audit.validate()?;
  if grants.is_empty()||grants.len()>N||grants.len()>audit.remaining(){
   return Err(AuditError::Full);
  }
  let mut staged=Self{grants:self.grants};
  staged.grant_batch(grants).map_err(|_|AuditError::Denied)?;
  let mut log=SecurityAudit{
   entries:audit.entries,used:audit.used,next:audit.next,chain:audit.chain,
  };
  for &(owner,rights,expiration) in grants{
   if expiration<=now{return Err(AuditError::Invalid);}
   log.record(owner,rights,now,AuditKind::Allowed)?;
  }
  self.grants=staged.grants;*audit=log;Ok(grants.len())
 }
 pub fn unrevoked_owners(&self,now:u64)->usize{
  self.grants.iter().flatten().filter(|g|!g.revoked&&now<g.expires).count()
 }
}
#[cfg(test)]
mod r014_audit_atomic_provision_tests{
 use super::*;
 fn id(p:u32)->Identity{Identity{pid:p,generation:1}}
 #[test]fn batch_grant_and_audit_commit_together(){
  let mut a=Access::<3>::new();let mut log=SecurityAudit::<3>::new();
  assert_eq!(a.grant_batch_audited(&mut log,5,&[
   (id(1),VIEW,20),(id(2),FILE_READ,30)]),Ok(2));
  assert_eq!(a.require(id(1),VIEW,6),Ok(()));
  assert_eq!(a.require(id(2),FILE_READ,6),Ok(()));
  assert_eq!(log.remaining(),1);assert_eq!(log.validate(),Ok(()));
 }
 #[test]fn duplicate_pid_or_excess_capacity_does_not_partially_grant(){
  let mut a=Access::<3>::new();let mut log=SecurityAudit::<3>::new();
  assert_eq!(a.grant_batch_audited(&mut log,5,&[
   (id(1),VIEW,20),(id(1),FILE_WRITE,20)]),Err(AuditError::Denied));
  assert_eq!(a.unrevoked_owners(5),0);assert_eq!(log.remaining(),3);
 }
 #[test]fn expired_in_batch_aborts_permissions_and_log(){
  let mut a=Access::<2>::new();let mut log=SecurityAudit::<2>::new();
  assert_eq!(a.grant_batch_audited(&mut log,10,&[
   (id(1),VIEW,20),(id(2),VIEW,10)]),Err(AuditError::Invalid));
  assert_eq!(a.unrevoked_owners(10),0);assert_eq!(log.remaining(),2);
 }
}

/// R016 kernel syscall gate for multi-capability transactions. All decisions
/// are journalled together; a full or corrupt journal denies authorization.
impl<const N:usize> Access<N>{
 pub fn require_batch_audited<const L:usize>(&self,
   audit:&mut SecurityAudit<L>,owner:Identity,now:u64,rights:&[u32])
   ->Result<usize,AuditError>{
  if owner.pid==0||owner.generation==0||rights.is_empty()||
    rights.len()>L||audit.remaining()<rights.len(){
   return Err(AuditError::Full);
  }
  audit.validate()?;
  let mut scratch=SecurityAudit{entries:audit.entries,
    used:audit.used,next:audit.next,chain:audit.chain};
  let mut permitted=true;
  for &right in rights{
   if right==0||right&!ALL!=0{return Err(AuditError::Invalid);}
   let decision=self.require(owner,right,now).is_ok();
   scratch.record(owner,right,now,if decision{
     AuditKind::Allowed
   }else{AuditKind::Denied})?;
   if !decision{permitted=false;}
  }
  *audit=scratch;
  if permitted{Ok(rights.len())}else{Err(AuditError::Denied)}
 }
 pub fn permitted_mask_at(&self,owner:Identity,now:u64)->u32{
  if owner.pid==0||owner.generation==0{return 0;}
  let mut mask=0;
  for bit in [VIEW,FILE_READ,FILE_WRITE,INSTALL,SOCKET,DRIVER]{
   if self.require(owner,bit,now).is_ok(){mask|=bit;}
  }
  mask
 }
}
#[cfg(test)]mod r016_audited_multi_gate_tests{
 use super::*;fn id(g:u64)->Identity{Identity{pid:5,generation:g}}
 #[test]fn two_rights_publish_audit_only_when_capacity_is_sufficient(){
  let mut access=Access::<1>::new();let mut log=SecurityAudit::<3>::new();
  access.grant(id(1),VIEW|FILE_READ,100).unwrap();
  assert_eq!(access.require_batch_audited(&mut log,id(1),2,
   &[VIEW,FILE_READ]),Ok(2));
  assert_eq!(log.remaining(),1);
  assert_eq!(access.permitted_mask_at(id(1),2),VIEW|FILE_READ);
 }
 #[test]fn denied_second_permission_is_audited_but_not_granted(){
  let mut access=Access::<1>::new();let mut log=SecurityAudit::<2>::new();
  access.grant(id(1),VIEW,100).unwrap();
  assert_eq!(access.require_batch_audited(&mut log,id(1),2,
    &[VIEW,FILE_WRITE]),Err(AuditError::Denied));
  assert_eq!(access.require(id(1),FILE_WRITE,2),Err(Error::Denied));
  assert_eq!(log.validate(),Ok(()));assert_eq!(log.remaining(),0);
 }
 #[test]fn journal_capacity_invalid_rights_and_recycled_pid_fail_closed(){
  let mut access=Access::<1>::new();let mut log=SecurityAudit::<1>::new();
  access.grant(id(1),VIEW,100).unwrap();
  assert_eq!(access.require_batch_audited(&mut log,id(1),2,
    &[VIEW,VIEW]),Err(AuditError::Full));
  assert_eq!(log.remaining(),1);
  assert_eq!(access.require_batch_audited(&mut log,id(1),2,
    &[0]),Err(AuditError::Invalid));
  assert_eq!(log.remaining(),1);
  assert_eq!(access.permitted_mask_at(id(2),2),0);
 }
}

