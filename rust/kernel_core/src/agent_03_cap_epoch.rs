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



/// R115: revoke multiple time-limited grants as one exact-incarnation
/// operation, without allowing a stale PID to evict its successor's lease.
/// A caller must provide kernel-validated resource identities.
impl<const N: usize> LeaseTable<N> {
    pub fn revoke_batch(
        &mut self, identities: &[(u32, u64, u32)],
    ) -> Result<usize, LeaseFault> {
        let mut candidate = *self;
        for (i, &(pid, generation, resource)) in identities.iter().enumerate() {
            if pid == 0 || generation == 0 {
                return Err(LeaseFault::Invalid);
            }
            if identities[..i].contains(&(pid, generation, resource)) {
                return Err(LeaseFault::Duplicate);
            }
            let at = candidate.entries.iter().position(|entry| entry.is_some_and(
                |lease| lease.grant.owner == pid
                    && lease.grant.generation == generation
                    && lease.grant.resource == resource
            )).ok_or(LeaseFault::Missing)?;
            candidate.entries[at] = None;
        }
        *self = candidate;
        Ok(identities.len())
    }
}

#[cfg(test)]
mod r115_atomic_capability_teardown_tests {
    use super::*;
    fn parent() -> Grant {
        Grant { owner: 1, resource: 6, rights: 7, epoch: 4,
            generation: 3, revoked: false }
    }
    fn request() -> Request {
        Request { caller: 1, resource: 6, required_rights: 1,
            current_epoch: 4, current_generation: 3 }
    }
    fn child(owner: u32, generation: u64) -> TimedGrant {
        TimedGrant { grant: Grant { owner, resource: 6, rights: 1,
            epoch: 4, generation, revoked: false }, expires_at: 100 }
    }
    fn setup() -> LeaseTable<3> {
        let mut t = LeaseTable::<3>::new();
        for (pid, generation) in [(10, 8), (11, 9), (12, 10)] {
            t.delegate(parent(), request(), child(pid, generation), 1).unwrap();
        }
        t
    }

    #[test]
    fn revokes_exact_identities_without_disturbing_a_peer() {
        let mut t = setup();
        assert_eq!(t.revoke_batch(&[(10, 8, 6), (12, 10, 6)]), Ok(2));
        assert_eq!(t.len(), 1);
        let c = |pid, gen| Request { caller: pid, resource: 6, required_rights: 1,
            current_epoch: 4, current_generation: gen };
        assert_eq!(t.check(c(10, 8), 2), Err(LeaseFault::Missing));
        assert_eq!(t.check(c(12, 10), 2), Err(LeaseFault::Missing));
        assert_eq!(t.check(c(11, 9), 2), Ok(()));
    }

    #[test]
    fn stale_identity_and_missing_resource_are_fail_atomic() {
        let mut t = setup();
        for wrong in [(11, 8, 6), (11, 9, 99), (99, 9, 6)] {
            assert_eq!(t.revoke_batch(&[(10, 8, 6), wrong]),
                Err(LeaseFault::Missing));
            assert_eq!(t.len(), 3);
        }
    }

    #[test]
    fn invalid_and_duplicate_requests_do_not_partially_revoke() {
        let mut t = setup();
        for invalid in [(0, 9, 6), (11, 0, 6)] {
            assert_eq!(t.revoke_batch(&[(10, 8, 6), invalid]),
                Err(LeaseFault::Invalid));
            assert_eq!(t.len(), 3);
        }
        assert_eq!(t.revoke_batch(&[(10, 8, 6), (10, 8, 6)]),
            Err(LeaseFault::Duplicate));
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn empty_and_zero_capacity_never_mutate() {
        let mut t = setup();
        assert_eq!(t.revoke_batch(&[]), Ok(0));
        assert_eq!(t.len(), 3);
        let mut empty = LeaseTable::<0>::new();
        assert_eq!(empty.revoke_batch(&[]), Ok(0));
        assert_eq!(empty.revoke_batch(&[(1, 1, 6)]), Err(LeaseFault::Missing));
        assert_eq!(empty.len(), 0);
    }
}


/// R116: read-only filesystem-directory authority must be bound to the
/// actual running PCB incarnation and a kernel-selected resource. No ring3
/// code can mint a TimedGrant or choose a source PID.
pub const SHARED_DIRECTORY_RESOURCE:u32=0x5348_5244;
pub const SHARED_DIRECTORY_READ:u32=1;
impl<const N:usize> LeaseTable<N>{
 pub fn permits_shared_directory(&self,owner:crate::batch5_session::Identity)->bool{
  owner.pid==2&&owner.generation!=0&&
  self.check(Request{caller:owner.pid,resource:SHARED_DIRECTORY_RESOURCE,
    required_rights:SHARED_DIRECTORY_READ,
    current_epoch:1,current_generation:owner.generation},0)==Ok(())
 }
}
#[cfg(test)]
mod r116_process_bound_lease_tests {
 use super::*;
 use crate::batch5_session::Identity;
 fn setup()->LeaseTable<3>{
  let mut table=LeaseTable::new();
  let parent=Grant{owner:1,resource:SHARED_DIRECTORY_RESOURCE,
   rights:SHARED_DIRECTORY_READ,epoch:1,generation:3,revoked:false};
  let issuer=Request{caller:1,resource:SHARED_DIRECTORY_RESOURCE,
   required_rights:SHARED_DIRECTORY_READ,current_epoch:1,current_generation:3};
  let child=TimedGrant{grant:Grant{owner:2,resource:SHARED_DIRECTORY_RESOURCE,
   rights:SHARED_DIRECTORY_READ,epoch:1,generation:4,revoked:false},
   expires_at:u64::MAX};
  table.delegate(parent,issuer,child,0).unwrap();
  table
 }
 #[test]fn exact_pid2_incarnation_can_enumerate_and_peer_cannot(){
  let t=setup();
  assert!(t.permits_shared_directory(Identity{pid:2,generation:4}));
  for id in [Identity{pid:1,generation:3},
             Identity{pid:2,generation:3},Identity{pid:2,generation:5},
             Identity{pid:3,generation:4},Identity{pid:2,generation:0}]{
   assert!(!t.permits_shared_directory(id));
  }
 }
 #[test]fn exact_scheduler_teardown_revokes_old_generation(){
  let mut t=setup();
  assert_eq!(t.revoke_identity(2,5),0);
  assert!(t.permits_shared_directory(Identity{pid:2,generation:4}));
  assert_eq!(t.revoke_identity(2,4),1);
  assert!(!t.permits_shared_directory(Identity{pid:2,generation:4}));
  assert_eq!(t.revoke_identity(2,4),0);
 }
 #[test]fn wrong_resource_or_rights_cannot_be_delegated(){
  let mut t=LeaseTable::<1>::new();
  let p=Grant{owner:1,resource:SHARED_DIRECTORY_RESOURCE,
    rights:SHARED_DIRECTORY_READ,epoch:1,generation:3,revoked:false};
  let r=Request{caller:1,resource:SHARED_DIRECTORY_RESOURCE,
    required_rights:1,current_epoch:1,current_generation:3};
  for g in [
   Grant{resource:8,..p},
   Grant{rights:2,..p},
   Grant{generation:0,..p},
   Grant{revoked:true,..p},
  ]{
   assert!(t.delegate(p,r,TimedGrant{grant:Grant{owner:2,..g},
       expires_at:30},0).is_err());
   assert_eq!(t.len(),0);
  }
 }
 #[test]fn capacity_and_expiry_do_not_revive_old_owner(){
  let mut t=setup();
  let request=Request{caller:2,resource:SHARED_DIRECTORY_RESOURCE,
    required_rights:1,current_epoch:1,current_generation:4};
  assert_eq!(t.check(request,u64::MAX),Err(LeaseFault::Expired));
  t.revoke_identity(2,4);
  assert_eq!(t.check(request,0),Err(LeaseFault::Missing));
 }
}


/// R117: the trusted capability supervisor can extend a still-live grant
/// without widening its rights, changing its principal, or reviving expiry.
/// The entire mutation is fail-atomic, including generation and epoch checks.
impl<const N:usize> LeaseTable<N> {
 pub fn renew_exact_kernel(
    &mut self,request:Request,now:u64,next_deadline:u64
 )->Result<(),LeaseFault>{
    if request.caller==0||request.current_generation==0||
       request.required_rights==0||next_deadline<=now{
        return Err(LeaseFault::Invalid)
    }
    let index=self.entries.iter().position(|entry|entry.is_some_and(|lease|
       lease.grant.owner==request.caller&&
       lease.grant.resource==request.resource))
       .ok_or(LeaseFault::Missing)?;
    let prior=self.entries[index].ok_or(LeaseFault::Missing)?;
    if now>=prior.expires_at{return Err(LeaseFault::Expired);}
    authorize(prior.grant,request).map_err(LeaseFault::Denied)?;
    if next_deadline<=prior.expires_at{return Err(LeaseFault::Invalid);}
    self.entries[index]=Some(TimedGrant{
        grant:prior.grant,expires_at:next_deadline,
    });
    Ok(())
 }
}
#[cfg(test)]
mod r117_capability_renewal_tests {
 use super::*;
 fn pair()->(Grant,Request,TimedGrant){
  let p=Grant{owner:1,resource:7,rights:3,epoch:4,
       generation:9,revoked:false};
  let issuer=Request{caller:1,resource:7,required_rights:1,
       current_epoch:4,current_generation:9};
  let child=TimedGrant{grant:Grant{owner:2,resource:7,rights:1,
       epoch:4,generation:10,revoked:false},expires_at:30};
  (p,issuer,child)
 }
 fn setup()->LeaseTable<2>{
  let (p,issuer,child)=pair();let mut table=LeaseTable::new();
  table.delegate(p,issuer,child,10).unwrap();table
 }
 fn req()->Request{
  Request{caller:2,resource:7,required_rights:1,current_epoch:4,
    current_generation:10}
 }
 #[test]fn only_current_live_generation_can_extend_exact_lease(){
  let mut t=setup();assert_eq!(t.renew_exact_kernel(req(),20,40),Ok(()));
  assert_eq!(t.check(req(),35),Ok(()));
  assert_eq!(t.check(req(),40),Err(LeaseFault::Expired));
  assert_eq!(t.len(),1);
 }
 #[test]fn stale_epoch_generation_and_foreign_rights_never_extend(){
  let mut t=setup();
  for bad in [
   Request{caller:3,..req()},
   Request{current_epoch:5,..req()},
   Request{current_generation:11,..req()},
   Request{resource:8,..req()},
   Request{required_rights:2,..req()},
  ]{assert!(t.renew_exact_kernel(bad,20,60).is_err());}
  assert_eq!(t.check(req(),29),Ok(()));
  assert_eq!(t.check(req(),30),Err(LeaseFault::Expired));
 }
 #[test]fn expired_grants_cannot_be_revived_or_shortened(){
  let mut t=setup();
  assert_eq!(t.renew_exact_kernel(req(),30,60),Err(LeaseFault::Expired));
  assert_eq!(t.renew_exact_kernel(req(),20,30),Err(LeaseFault::Invalid));
  assert_eq!(t.renew_exact_kernel(req(),20,29),Err(LeaseFault::Invalid));
  assert_eq!(t.renew_exact_kernel(req(),20,20),Err(LeaseFault::Invalid));
  assert_eq!(t.check(req(),30),Err(LeaseFault::Expired));
 }
 #[test]fn missing_revoked_and_zero_identity_are_rejected(){
  let mut t=LeaseTable::<1>::new();
  assert_eq!(t.renew_exact_kernel(req(),0,40),Err(LeaseFault::Missing));
  t=setup_into_one();
  for invalid in [
   Request{caller:0,..req()},
   Request{current_generation:0,..req()},
   Request{required_rights:0,..req()},
  ]{assert_eq!(t.renew_exact_kernel(invalid,20,40),Err(LeaseFault::Invalid));}
  t.revoke_identity(2,10);
  assert_eq!(t.renew_exact_kernel(req(),20,40),Err(LeaseFault::Missing));
 }
 fn setup_into_one()->LeaseTable<1>{
  let (p,issuer,child)=pair();let mut t=LeaseTable::new();
  t.delegate(p,issuer,child,10).unwrap();t
 }
 #[test]fn peer_lease_is_unchanged_by_renewal(){
  let mut t=LeaseTable::<2>::new();let (p,issuer,child)=pair();
  t.delegate(p,issuer,child,10).unwrap();
  let second=TimedGrant{grant:Grant{owner:3,generation:11,..child.grant},
    expires_at:50};
  t.delegate(p,issuer,second,10).unwrap();
  assert_eq!(t.renew_exact_kernel(req(),20,45),Ok(()));
  let other=Request{caller:3,current_generation:11,..req()};
  assert_eq!(t.check(other,49),Ok(()));
  assert_eq!(t.check(other,50),Err(LeaseFault::Expired));
  assert_eq!(t.check(req(),44),Ok(()));
 }
}

 
/// R118 single-BSP supervisor helper: fail-atomic retirement of all leases
/// for several authenticated process incarnations. A stale generation or
/// duplicate PID rejects the entire batch, leaving peer leases untouched.
/// This does not create an inter-CPU lock or authenticate caller identity.
impl<const N:usize> LeaseTable<N> {
 pub fn revoke_identity_batch_kernel(
   &mut self,identities:&[(u32,u64)]
 )->Result<usize,LeaseFault>{
   let mut staged=*self;
   let mut count=0usize;
   for (i,&(pid,generation)) in identities.iter().enumerate(){
     if pid==0 || generation==0 {return Err(LeaseFault::Invalid);}
     if identities[..i].iter().any(|&(existing,_)|existing==pid){
       return Err(LeaseFault::Duplicate);
     }
     if staged.entries.iter().flatten().any(|v|
         v.grant.owner==pid&&v.grant.generation!=generation){
       return Err(LeaseFault::Denied(CapabilityError::Stale));
     }
     count+=staged.revoke_identity(pid,generation);
   }
   *self=staged;
   Ok(count)
 }
}
#[cfg(test)]
mod r118_capability_supervisor_batch_tests {
 use super::*;
 fn setup()->LeaseTable<4>{
   let parent=Grant{owner:1,resource:6,rights:7,epoch:4,
     generation:3,revoked:false};
   let issuer=Request{caller:1,resource:6,required_rights:1,
     current_epoch:4,current_generation:3};
   let mut leases=LeaseTable::new();
   for (pid,generation) in [(2,8),(3,9),(4,10)]{
     let child=TimedGrant{grant:Grant{
         owner:pid,resource:6,rights:1,epoch:4,
         generation,revoked:false},expires_at:100};
     leases.delegate(parent,issuer,child,1).unwrap();
   }
   leases
 }
 fn read(pid:u32,generation:u64)->Request{
   Request{caller:pid,resource:6,required_rights:1,
     current_epoch:4,current_generation:generation}
 }
 #[test]
 fn stale_mid_batch_cannot_revoke_other_process_capabilities(){
   let mut leases=setup();
   assert_eq!(leases.revoke_identity_batch_kernel(&[(2,8),(3,10)]),
     Err(LeaseFault::Denied(CapabilityError::Stale)));
   assert_eq!(leases.len(),3);
   for (pid,generation) in [(2,8),(3,9),(4,10)]{
     assert_eq!(leases.check(read(pid,generation),4),Ok(()));
   }
   assert_eq!(leases.revoke_identity_batch_kernel(&[(2,8),(2,8)]),
     Err(LeaseFault::Duplicate));
   assert_eq!(leases.revoke_identity_batch_kernel(&[(0,8)]),
     Err(LeaseFault::Invalid));
   assert_eq!(leases.len(),3);
 }
 #[test]
 fn exact_batch_retires_only_requested_owners_and_preserves_peer(){
   let mut leases=setup();
   assert_eq!(leases.revoke_identity_batch_kernel(&[(2,8),(4,10)]),Ok(2));
   assert_eq!(leases.check(read(2,8),4),Err(LeaseFault::Missing));
   assert_eq!(leases.check(read(3,9),4),Ok(()));
   assert_eq!(leases.check(read(4,10),4),Err(LeaseFault::Missing));
   assert_eq!(leases.revoke_identity_batch_kernel(&[(2,8)]),Ok(0));
   assert_eq!(leases.revoke_identity_batch_kernel(&[(3,9)]),Ok(1));
   assert_eq!(leases.len(),0);
 }
}
