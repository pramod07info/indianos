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
