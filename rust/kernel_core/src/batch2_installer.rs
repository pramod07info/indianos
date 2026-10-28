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

 
/// R009 installer handoff: invoke a trusted durable package backend only
/// after signed payload, architecture and explicit consent were accepted.
/// The callback must itself be atomic/durable and perform actual signature
/// verification; this wrapper does not make filesystem writes safe.
#[derive(Debug,PartialEq,Eq)]
pub enum CommitError<E>{Policy(InstallError),Backend(E)}
impl Transaction{
 pub fn commit_with<T,E,F>(&mut self,backend:F)->Result<T,CommitError<E>>
   where F:FnOnce(Proposal)->Result<T,E>{
  if self.phase!=Phase::Approved{return Err(CommitError::Policy(InstallError::WrongState));}
  let proposal=self.proposal.ok_or(CommitError::Policy(InstallError::WrongState))?;
  match backend(proposal){
   Ok(receipt)=>{self.phase=Phase::Committed;Ok(receipt)},
   Err(error)=>{
    // A failed backend cannot reuse an old consent token. The backend
    // remains responsible for rollback of any staged disk transaction.
    self.rollback();Err(CommitError::Backend(error))
   }
  }
 }
 pub fn proposal(&self)->Option<Proposal>{self.proposal}
}
#[cfg(test)]
mod r009_install_backend_tests{
 use super::*;
 fn signed()->Proposal{Proposal{app_id:7,manifest_verified:true,
   payload_verified:true,permissions:3,architecture_ok:true}}
 #[test]fn approved_install_commits_backend_receipt(){
  let mut t=Transaction::new();t.stage(signed()).unwrap();
  t.approve(3,true).unwrap();
  let receipt=t.commit_with(|p|->Result<u64,&'static str>{
   assert_eq!(p.app_id,7);Ok(0x5555)
  });
  assert_eq!(receipt,Ok(0x5555));
  assert_eq!(t.phase(),Phase::Committed);
 }
 #[test]fn backend_failure_invalidates_consent_and_requires_reapproval(){
  let mut t=Transaction::new();t.stage(signed()).unwrap();
  t.approve(3,true).unwrap();
  assert_eq!(t.commit_with(|_|->Result<(),u8>{Err(8)}),Err(CommitError::Backend(8)));
  assert_eq!(t.phase(),Phase::Idle);
  assert_eq!(t.commit_with(|_|->Result<(),u8>{Ok(())}),
    Err(CommitError::Policy(InstallError::WrongState)));
 }
 #[test]fn unsigned_or_unapproved_install_never_calls_backend(){
  let mut t=Transaction::new();
  let mut invoked=false;
  assert_eq!(t.commit_with(|_|->Result<(),u8>{invoked=true;Ok(())}),
    Err(CommitError::Policy(InstallError::WrongState)));
  assert!(!invoked);
  let mut p=signed();p.manifest_verified=false;
  assert_eq!(t.stage(p),Err(InstallError::Untrusted));
  assert!(!invoked);
 }
}
