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
