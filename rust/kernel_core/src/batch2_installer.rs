//! R036/18: consent-bound, reversible application install transaction state.
//! No filesystem writes, signature verification or executable launch occur.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Phase{Idle,Staged,Approved,Committed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Proposal{pub app_id:u32,pub manifest_verified:bool,
    pub payload_verified:bool,pub permissions:u32,pub architecture_ok:bool}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum InstallError{Invalid,Untrusted,PermissionDenied,WrongState}
pub struct Transaction{phase:Phase,proposal:Option<Proposal>}
impl Transaction{
    pub const fn new()->Self{Self{phase:Phase::Idle,proposal:None}}
    pub fn phase(&self)->Phase{self.phase}
    pub fn stage(&mut self,p:Proposal)->Result<(),InstallError>{
        if self.phase!=Phase::Idle||p.app_id==0{return Err(InstallError::Invalid);}
        if !p.manifest_verified||!p.payload_verified||!p.architecture_ok{
            return Err(InstallError::Untrusted);
        }
        self.proposal=Some(p);self.phase=Phase::Staged;Ok(())
    }
    pub fn approve(&mut self,granted:u32,explicit_consent:bool)
        ->Result<(),InstallError>{
        if self.phase!=Phase::Staged{return Err(InstallError::WrongState);}
        let p=self.proposal.ok_or(InstallError::WrongState)?;
        if !explicit_consent||(p.permissions&granted)!=p.permissions{
            return Err(InstallError::PermissionDenied);
        }
        self.phase=Phase::Approved;Ok(())
    }
    /// Records policy commitment; a durable transactional installer is separate.
    pub fn commit_policy(&mut self)->Result<u32,InstallError>{
        if self.phase!=Phase::Approved{return Err(InstallError::WrongState);}
        self.phase=Phase::Committed;
        Ok(self.proposal.ok_or(InstallError::WrongState)?.app_id)
    }
    pub fn rollback(&mut self){
        self.proposal=None;self.phase=Phase::Idle;
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn p()->Proposal{Proposal{app_id:42,manifest_verified:true,
        payload_verified:true,permissions:3,architecture_ok:true}}
    #[test]fn consent_and_permissions_required_for_commit(){
        let mut t=Transaction::new();
        assert_eq!(t.commit_policy(),Err(InstallError::WrongState));
        t.stage(p()).unwrap();
        assert_eq!(t.approve(3,false),Err(InstallError::PermissionDenied));
        assert_eq!(t.approve(1,true),Err(InstallError::PermissionDenied));
        t.approve(3,true).unwrap();
        assert_eq!(t.commit_policy(),Ok(42));
        assert_eq!(t.phase(),Phase::Committed);
    }
    #[test]fn unsigned_or_incompatible_payload_is_rejected(){
        let mut t=Transaction::new();let mut a=p();a.payload_verified=false;
        assert_eq!(t.stage(a),Err(InstallError::Untrusted));
        a=p();a.architecture_ok=false;
        assert_eq!(t.stage(a),Err(InstallError::Untrusted));
        assert_eq!(t.phase(),Phase::Idle);
    }
    #[test]fn rollback_drops_uncommitted_approval(){
        let mut t=Transaction::new();
        t.stage(p()).unwrap();t.approve(3,true).unwrap();
        t.rollback();
        assert_eq!(t.commit_policy(),Err(InstallError::WrongState));
        assert_eq!(t.phase(),Phase::Idle);
    }
}
