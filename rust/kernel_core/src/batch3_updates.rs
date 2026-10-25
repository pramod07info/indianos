//! R037/27: monotonic signed-update staging and A/B activation policy.
//! No network download, firmware operation, signature cryptography or disk writes.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Candidate{pub image_id:u32,pub version:u64,pub verified_signature:bool,
    pub payload_digest_verified:bool,pub target_arch:u16}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum UpdateError{Invalid,Untrusted,Downgrade,WrongState,NoAttempts}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Stage{Idle,Staged,ReadyToBoot,Confirmed}
pub struct Update{
    state:Stage, floor:u64, running:u64,
    candidate:Option<Candidate>, attempts:u8,
}
impl Update{
    pub const fn new(running:u64,floor:u64)->Self{
        Self{state:Stage::Idle,floor,running,candidate:None,attempts:0}
    }
    pub fn state(&self)->Stage{self.state}
    pub fn stage(&mut self,candidate:Candidate,host_arch:u16)
        ->Result<(),UpdateError>{
        if self.state!=Stage::Idle||candidate.image_id==0||candidate.version==0||
            host_arch==0||candidate.target_arch!=host_arch{
            return Err(UpdateError::Invalid);
        }
        if !candidate.verified_signature||!candidate.payload_digest_verified{
            return Err(UpdateError::Untrusted);
        }
        if candidate.version<=self.running||candidate.version<self.floor{
            return Err(UpdateError::Downgrade);
        }
        self.candidate=Some(candidate);self.state=Stage::Staged;Ok(())
    }
    /// A trusted disk updater must persist a fully flushed inactive slot first.
    pub fn prepared(&mut self,flushed_inactive_slot:bool,attempts:u8)
        ->Result<(),UpdateError>{
        if self.state!=Stage::Staged||!flushed_inactive_slot||attempts==0{
            return Err(UpdateError::WrongState);
        }
        self.attempts=attempts;self.state=Stage::ReadyToBoot;Ok(())
    }
    pub fn attempt(&mut self)->Result<u32,UpdateError>{
        if self.state!=Stage::ReadyToBoot{return Err(UpdateError::WrongState);}
        if self.attempts==0{return Err(UpdateError::NoAttempts);}
        self.attempts-=1;
        Ok(self.candidate.ok_or(UpdateError::WrongState)?.image_id)
    }
    pub fn confirm(&mut self,boot_health_verified:bool)->Result<(),UpdateError>{
        if self.state!=Stage::ReadyToBoot||!boot_health_verified{
            return Err(UpdateError::WrongState);
        }
        let candidate=self.candidate.ok_or(UpdateError::WrongState)?;
        self.running=candidate.version;self.floor=self.floor.max(candidate.version);
        self.state=Stage::Confirmed;Ok(())
    }
    pub fn rollback(&mut self){
        self.candidate=None;self.attempts=0;self.state=Stage::Idle;
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn candidate()->Candidate{Candidate{image_id:1,version:11,
        verified_signature:true,payload_digest_verified:true,target_arch:62}}
    #[test]fn inactive_slot_must_be_ready_before_boot(){
        let mut u=Update::new(10,10);u.stage(candidate(),62).unwrap();
        assert_eq!(u.prepared(false,2),Err(UpdateError::WrongState));
        u.prepared(true,1).unwrap();
        assert_eq!(u.attempt(),Ok(1));
        assert_eq!(u.attempt(),Err(UpdateError::NoAttempts));
        assert_eq!(u.confirm(false),Err(UpdateError::WrongState));
        u.confirm(true).unwrap();
        assert_eq!(u.state(),Stage::Confirmed);
    }
    #[test]fn unverified_downgrade_denied(){
        let mut u=Update::new(10,10);
        let mut x=candidate();x.verified_signature=false;
        assert_eq!(u.stage(x,62),Err(UpdateError::Untrusted));
        x=candidate();x.version=9;
        assert_eq!(u.stage(x,62),Err(UpdateError::Downgrade));
    }
    #[test]fn rollback_clears_pending_image(){
        let mut u=Update::new(10,10);u.stage(candidate(),62).unwrap();
        u.rollback();
        assert_eq!(u.attempt(),Err(UpdateError::WrongState));
    }
}
