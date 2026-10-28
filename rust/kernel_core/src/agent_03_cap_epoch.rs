//! Immutable check of capability owner, resource, rights, epoch and generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grant {
    pub owner: u32, pub resource: u32, pub rights: u32,
    pub epoch: u64, pub generation: u64, pub revoked: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub caller: u32, pub resource: u32, pub required_rights: u32,
    pub current_epoch: u64, pub current_generation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityError { Revoked, Owner, Resource, Rights, Stale }
pub fn authorize(grant: Grant, request: Request) -> Result<(), CapabilityError> {
    if grant.revoked { return Err(CapabilityError::Revoked); }
    if grant.owner == 0 || grant.owner != request.caller { return Err(CapabilityError::Owner); }
    if grant.resource != request.resource { return Err(CapabilityError::Resource); }
    if grant.epoch != request.current_epoch || grant.generation != request.current_generation {
        return Err(CapabilityError::Stale);
    }
    if request.required_rights == 0
        || (grant.rights & request.required_rights) != request.required_rights {
        return Err(CapabilityError::Rights);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pair() -> (Grant, Request) {
        (Grant { owner: 11, resource: 8, rights: 0b011, epoch: 2,
            generation: 3, revoked: false },
         Request { caller: 11, resource: 8, required_rights: 0b001,
            current_epoch: 2, current_generation: 3 })
    }
    #[test]
    fn grants_only_requested_rights() {
        let (g, r) = pair();
        assert_eq!(authorize(g, r), Ok(()));
    }
    #[test]
    fn denies_spoofed_owner_and_privilege_escalation() {
        let (g, mut r) = pair();
        r.caller = 12;
        assert_eq!(authorize(g, r), Err(CapabilityError::Owner));
        r.caller = 11; r.required_rights = 0b100;
        assert_eq!(authorize(g, r), Err(CapabilityError::Rights));
    }
    #[test]
    fn epoch_change_and_revocation_invalidate_grant() {
        let (mut g, mut r) = pair();
        r.current_epoch += 1;
        assert_eq!(authorize(g, r), Err(CapabilityError::Stale));
        r.current_epoch -= 1; g.revoked = true;
        assert_eq!(authorize(g, r), Err(CapabilityError::Revoked));
        g.revoked = false; r.current_generation += 1;
        assert_eq!(authorize(g, r), Err(CapabilityError::Stale));
    }
}

/// Derive a strictly attenuated grant for the same principal and resource.
/// No caller-supplied identity or epoch can widen rights or revive revocation.
pub fn attenuate_grant(grant: Grant, request: Request, rights: u32)
    -> Result<Grant, CapabilityError>
{
    let mut check = request;
    check.required_rights = rights;
    authorize(grant, check)?;
    Ok(Grant { rights, ..grant })
}
#[cfg(test)]
mod attenuation_tests {
    use super::*;
    fn pair() -> (Grant, Request) {
        (Grant { owner: 5, resource: 20, rights: 0b111, epoch: 9,
            generation: 2, revoked: false },
         Request { caller: 5, resource: 20, required_rights: 1,
            current_epoch: 9, current_generation: 2 })
    }
    #[test]
    fn safely_restricts_rights_without_identity_change() {
        let (g, r) = pair();
        let less = attenuate_grant(g, r, 0b001).unwrap();
        assert_eq!(less.rights, 1);
        assert_eq!((less.owner, less.resource, less.epoch), (5,20,9));
    }
    #[test]
    fn denies_escalation_and_empty_rights() {
        let (g,r) = pair();
        assert_eq!(attenuate_grant(g, r, 0b1000), Err(CapabilityError::Rights));
        assert_eq!(attenuate_grant(g, r, 0), Err(CapabilityError::Rights));
    }
    #[test]
    fn revocation_or_pid_mismatch_blocks_delegation() {
        let (mut g,mut r) = pair();
        r.caller = 6;
        assert_eq!(attenuate_grant(g, r, 1), Err(CapabilityError::Owner));
        r.caller = 5; g.revoked = true;
        assert_eq!(attenuate_grant(g, r, 1), Err(CapabilityError::Revoked));
    }
}


/// Pure owner-checked revocation: verify exact resource, rights and epoch
/// before marking a grant as permanently revoked.
pub fn revoke_owned(grant:Grant,request:Request)->Result<Grant,CapabilityError>{
    authorize(grant,request)?;
    Ok(Grant{revoked:true,..grant})
}
#[cfg(test)]
mod revocation_tests{
    use super::*;
    fn pair()->(Grant,Request){
        (Grant{owner:17,resource:31,rights:0b11,epoch:4,
               generation:7,revoked:false},
         Request{caller:17,resource:31,required_rights:1,
                 current_epoch:4,current_generation:7})
    }
    #[test]fn revocation_invalidates_existing_grant(){
        let (g,r)=pair();
        let gone=revoke_owned(g,r).unwrap();
        assert!(gone.revoked);
        assert_eq!(authorize(gone,r),Err(CapabilityError::Revoked));
        assert_eq!(revoke_owned(gone,r),Err(CapabilityError::Revoked));
    }
    #[test]fn stale_generation_and_foreign_owner_cannot_revoke(){
        let (g,mut r)=pair();
        r.current_generation=8;
        assert_eq!(revoke_owned(g,r),Err(CapabilityError::Stale));
        r.current_generation=7;r.caller=19;
        assert_eq!(revoke_owned(g,r),Err(CapabilityError::Owner));
    }
    #[test]fn rights_escalation_and_resource_spoofing_fail(){
        let (g,mut r)=pair();
        r.required_rights=0b100;
        assert_eq!(revoke_owned(g,r),Err(CapabilityError::Rights));
        r.required_rights=1;r.resource=8;
        assert_eq!(revoke_owned(g,r),Err(CapabilityError::Resource));
    }
}


#[cfg(test)]
mod agent05_reassignment_tests {
    use super::*;
    #[test]
    fn a_zero_rights_request_never_revokes() {
        let g=Grant{owner:3,resource:8,rights:7,epoch:5,
                    generation:19,revoked:false};
        let r=Request{caller:3,resource:8,required_rights:0,
                      current_epoch:5,current_generation:19};
        assert_eq!(revoke_owned(g,r),Err(CapabilityError::Rights));
        assert_eq!(g.revoked,false);
    }
    #[test]
    fn epoch_replay_cannot_revoke_newer_grant() {
        let g=Grant{owner:3,resource:8,rights:7,epoch:5,
                    generation:19,revoked:false};
        let r=Request{caller:3,resource:8,required_rights:1,
                      current_epoch:4,current_generation:19};
        assert_eq!(revoke_owned(g,r),Err(CapabilityError::Stale));
    }
}

/* Restrict two independently authorized grants without widening rights. */
pub fn intersect_grants(a: Grant, b: Grant, request: Request) -> Result<Grant, CapabilityError> {
    authorize(a, request)?;
    authorize(b, request)?;
    if a.owner != b.owner || a.resource != b.resource
        || a.epoch != b.epoch || a.generation != b.generation
    {
        return Err(CapabilityError::Stale);
    }
    let rights = a.rights & b.rights;
    if rights == 0 {
        return Err(CapabilityError::Rights);
    }
    Ok(Grant { rights, ..a })
}

#[cfg(test)]
mod intersection_tests {
    use super::*;

    fn sample(rights: u32) -> Grant {
        Grant { owner: 2, resource: 4, rights, epoch: 3, generation: 1, revoked: false }
    }
    fn request() -> Request {
        Request { caller: 2, resource: 4, required_rights: 1,
            current_epoch: 3, current_generation: 1 }
    }

    #[test]
    fn only_shared_rights_survive() {
        assert_eq!(intersect_grants(sample(3), sample(5), request()).unwrap().rights, 1);
    }
    #[test]
    fn stale_epoch_is_denied() {
        let mut b = sample(3);
        b.epoch = 4;
        assert_eq!(intersect_grants(sample(3), b, request()), Err(CapabilityError::Stale));
    }
    #[test]
    fn revoked_grants_are_denied() {
        let mut b = sample(3);
        b.revoked = true;
        assert_eq!(intersect_grants(sample(3), b, request()), Err(CapabilityError::Revoked));
    }
    #[test]
    fn foreign_owner_is_denied() {
        let mut b = sample(3);
        b.owner = 99;
        assert_eq!(intersect_grants(sample(3), b, request()), Err(CapabilityError::Owner));
    }
    #[test]
    fn missing_requested_rights_are_denied() {
        assert_eq!(intersect_grants(sample(3), sample(4), request()), Err(CapabilityError::Rights));
    }
}


/// Authorization for short-lived filesystem/network grants: expiry is
/// checked *after* owner, resource, rights, epoch and generation validation.
/// A recycled PID or replayed grant can never extend its lifetime.
pub fn authorize_until(grant:Grant,request:Request,now:u64,expires_at:u64)
    ->Result<(),CapabilityError>{
    authorize(grant,request)?;
    if expires_at==0 || now>=expires_at {
        return Err(CapabilityError::Stale);
    }
    Ok(())
}
#[cfg(test)]
mod grant_expiration_tests{
    use super::*;
    fn pair()->(Grant,Request){
        (Grant{owner:5,resource:7,rights:3,epoch:2,generation:9,revoked:false},
         Request{caller:5,resource:7,required_rights:1,
                 current_epoch:2,current_generation:9})
    }
    #[test]fn valid_grant_before_expiry_is_authorized(){
        let (g,r)=pair();assert_eq!(authorize_until(g,r,19,20),Ok(()));
    }
    #[test]fn exact_expiry_and_zero_expiry_are_denied(){
        let (g,r)=pair();
        assert_eq!(authorize_until(g,r,20,20),Err(CapabilityError::Stale));
        assert_eq!(authorize_until(g,r,0,0),Err(CapabilityError::Stale));
    }
    #[test]fn revoked_or_recycled_identity_never_reauthorizes(){
        let (mut g,mut r)=pair();r.current_generation=10;
        assert_eq!(authorize_until(g,r,1,20),Err(CapabilityError::Stale));
        r.current_generation=9;g.revoked=true;
        assert_eq!(authorize_until(g,r,1,20),Err(CapabilityError::Revoked));
    }
}


// R007: supervised least-authority delegation table. The authenticated
// parent request and child PID/generation MUST come from kernel scheduler data.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum DelegationError{Invalid,Full,Duplicate,Unauthorized(CapabilityError),Missing}
pub struct DelegationTable<const N:usize>{entries:[Option<Grant>;N]}
impl<const N:usize> DelegationTable<N>{
    pub const fn new()->Self{Self{entries:[None;N]}}
    pub fn issue(&mut self,parent:Grant,parent_request:Request,
        child:Grant)->Result<(),DelegationError>{
        if child.owner==0||child.generation==0||child.epoch==0||
            child.rights==0||child.revoked||child.resource!=parent.resource||
            child.epoch!=parent.epoch {
            return Err(DelegationError::Invalid);
        }
        let mut required=parent_request;
        required.required_rights=child.rights;
        authorize(parent,required).map_err(DelegationError::Unauthorized)?;
        if self.entries.iter().flatten().any(|old|
            old.owner==child.owner&&old.resource==child.resource){
            return Err(DelegationError::Duplicate);
        }
        let slot=self.entries.iter_mut().find(|v|v.is_none())
            .ok_or(DelegationError::Full)?;
        *slot=Some(child);Ok(())
    }
    pub fn check(&self,request:Request)->Result<(),DelegationError>{
        let grant=self.entries.iter().flatten().find(|g|
            g.owner==request.caller&&g.resource==request.resource)
            .ok_or(DelegationError::Missing)?;
        authorize(*grant,request).map_err(DelegationError::Unauthorized)
    }
    pub fn revoke_process(&mut self,pid:u32,generation:u64)->usize{
        if pid==0||generation==0{return 0;}
        let mut n=0;
        for slot in &mut self.entries{
            if slot.is_some_and(|g|g.owner==pid&&g.generation==generation){
                *slot=None;n+=1;
            }
        }
        n
    }
    pub fn active(&self)->usize{self.entries.iter().flatten().count()}
}
#[cfg(test)]mod r007_delegation_tests{
    use super::*;
    fn parent()->Grant{Grant{owner:2,resource:6,rights:7,epoch:4,generation:8,revoked:false}}
    fn request()->Request{Request{caller:2,resource:6,required_rights:1,
        current_epoch:4,current_generation:8}}
    fn child()->Grant{Grant{owner:3,resource:6,rights:3,epoch:4,generation:1,revoked:false}}
    #[test]fn delegates_restricted_rights_and_revokes_exact_incarnation(){
        let mut d=DelegationTable::<2>::new();
        assert_eq!(d.issue(parent(),request(),child()),Ok(()));
        let check=Request{caller:3,resource:6,required_rights:2,
            current_epoch:4,current_generation:1};
        assert_eq!(d.check(check),Ok(()));
        assert_eq!(d.revoke_process(3,2),0);
        assert_eq!(d.check(check),Ok(()));
        assert_eq!(d.revoke_process(3,1),1);
        assert_eq!(d.check(check),Err(DelegationError::Missing));
    }
    #[test]fn rejects_escalation_replay_and_duplicate_delegation(){
        let mut d=DelegationTable::<1>::new();
        let mut over=child();over.rights=8;
        assert_eq!(d.issue(parent(),request(),over),
            Err(DelegationError::Unauthorized(CapabilityError::Rights)));
        d.issue(parent(),request(),child()).unwrap();
        assert_eq!(d.issue(parent(),request(),child()),Err(DelegationError::Duplicate));
        let mut other=child();other.owner=4;
        assert_eq!(d.issue(parent(),request(),other),Err(DelegationError::Full));
        let stale=Request{current_generation:2,..Request{caller:3,resource:6,
            required_rights:1,current_epoch:4,current_generation:1}};
        assert_eq!(d.check(stale),
            Err(DelegationError::Unauthorized(CapabilityError::Stale)));
    }
    #[test]fn revoked_parent_never_issues_new_authority(){
        let mut d=DelegationTable::<1>::new();
        let mut p=parent();p.revoked=true;
        assert_eq!(d.issue(p,request(),child()),
            Err(DelegationError::Unauthorized(CapabilityError::Revoked)));
        assert_eq!(d.active(),0);
    }
}


// R008 bounded time-limited capability delegation and lifecycle cleanup.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct TimedGrant{pub grant:Grant,pub expires_at:u64}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum LeaseFault{Invalid,Full,Duplicate,Missing,Expired,Denied(CapabilityError)}
#[derive(Clone,Copy)]
pub struct LeaseTable<const N:usize>{entries:[Option<TimedGrant>;N]}
impl<const N:usize> LeaseTable<N>{
 pub const fn new()->Self{Self{entries:[None;N]}}
 pub fn len(&self)->usize{self.entries.iter().flatten().count()}
 pub fn delegate(&mut self,parent:Grant,issuer:Request,child:TimedGrant,now:u64)
  ->Result<(),LeaseFault>{
  if now>=child.expires_at||child.grant.owner==0||child.grant.revoked||
   child.grant.rights==0||child.grant.generation==0||
   child.grant.resource!=parent.resource||child.grant.epoch!=parent.epoch||
   (child.grant.rights&!parent.rights)!=0{return Err(LeaseFault::Invalid)}
  let mut proof=issuer;proof.required_rights=child.grant.rights;
  authorize(parent,proof).map_err(LeaseFault::Denied)?;
  if self.entries.iter().flatten().any(|v|
   v.grant.owner==child.grant.owner&&v.grant.resource==child.grant.resource){
   return Err(LeaseFault::Duplicate);
  }
  let slot=self.entries.iter_mut().find(|v|v.is_none()).ok_or(LeaseFault::Full)?;
  *slot=Some(child);Ok(())
 }
 pub fn delegate_batch(&mut self,parent:Grant,issuer:Request,
  children:&[TimedGrant],now:u64)->Result<usize,LeaseFault>{
  if children.is_empty()||children.len()>N{return Err(LeaseFault::Full)}
  let mut candidate=*self;
  for child in children{candidate.delegate(parent,issuer,*child,now)?}
  *self=candidate;Ok(children.len())
 }
 pub fn check(&self,request:Request,now:u64)->Result<(),LeaseFault>{
  let lease=self.entries.iter().flatten().find(|v|
   v.grant.owner==request.caller&&v.grant.resource==request.resource)
   .ok_or(LeaseFault::Missing)?;
  if now>=lease.expires_at{return Err(LeaseFault::Expired)}
  authorize(lease.grant,request).map_err(LeaseFault::Denied)
 }
 pub fn revoke_identity(&mut self,owner:u32,generation:u64)->usize{
  if owner==0||generation==0{return 0}
  let mut removed=0;
  for slot in &mut self.entries{
   if slot.is_some_and(|v|v.grant.owner==owner&&v.grant.generation==generation){
    *slot=None;removed+=1
   }
  }
  removed
 }
 pub fn sweep(&mut self,now:u64)->usize{
  let mut removed=0;
  for slot in &mut self.entries{
   if slot.is_some_and(|v|now>=v.expires_at){*slot=None;removed+=1}
  }
  removed
 }
}
#[cfg(test)]mod r008_timed_capabilities_tests{
 use super::*;
 fn g(owner:u32,rights:u32)->Grant{
  Grant{owner,resource:5,rights,epoch:2,generation:7,revoked:false}
 }
 fn r(caller:u32,rights:u32)->Request{
  Request{caller,resource:5,required_rights:rights,current_epoch:2,current_generation:7}
 }
 #[test]fn expiry_and_generation_scoped_cleanup(){
  let mut t=LeaseTable::<2>::new();
  t.delegate(g(1,7),r(1,7),TimedGrant{grant:g(2,3),expires_at:20},10).unwrap();
  assert_eq!(t.check(r(2,1),19),Ok(()));
  assert_eq!(t.check(r(2,1),20),Err(LeaseFault::Expired));
  assert_eq!(t.sweep(20),1);
  assert_eq!(t.check(r(2,1),19),Err(LeaseFault::Missing));
 }
 #[test]fn atomic_batch_denies_escalation(){
  let mut t=LeaseTable::<2>::new();
  assert_eq!(t.delegate_batch(g(1,7),r(1,7),
   &[TimedGrant{grant:g(2,1),expires_at:20},
     TimedGrant{grant:g(3,8),expires_at:20}],5),Err(LeaseFault::Invalid));
  assert_eq!(t.len(),0);
  assert_eq!(t.delegate_batch(g(1,7),r(1,7),
   &[TimedGrant{grant:g(2,1),expires_at:20},
     TimedGrant{grant:g(3,2),expires_at:20}],5),Ok(2));
  assert_eq!(t.revoke_identity(2,7),1);
  assert_eq!(t.check(r(3,2),10),Ok(()));
 }
 #[test]fn forged_issuer_and_stale_epoch_fail(){
  let mut t=LeaseTable::<1>::new();
  assert_eq!(t.delegate(g(1,7),r(99,1),
   TimedGrant{grant:g(2,1),expires_at:20},5),
   Err(LeaseFault::Denied(CapabilityError::Owner)));
  let mut stale=g(2,1);stale.epoch=1;
  assert_eq!(t.delegate(g(1,7),r(1,1),
   TimedGrant{grant:stale,expires_at:20},5),Err(LeaseFault::Invalid));
 }
}
