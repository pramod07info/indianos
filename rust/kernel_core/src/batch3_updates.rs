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


/// Smart Update R005 safety extensions for an eventual HTTPS OTA client.
/// These are side-effect-free boot-slot state transitions, not a downloader,
/// a trusted signature verifier, a cloud service or a persistent bootloader.
impl Update{
 /// The UI can display pending image information without authorizing it.
 pub fn pending(&self)->Option<(u32,u64,u8)>{
  self.candidate.map(|x|(x.image_id,x.version,self.attempts))
 }
 /// Require an exact booted image identity before confirming a new release.
 /// A healthy OLD slot cannot accidentally confirm a DIFFERENT new image.
 pub fn confirm_exact(&mut self,booted_image:u32,
  health_verified:bool)->Result<(),UpdateError>{
  if !health_verified{return Err(UpdateError::WrongState);}
  let expected=self.candidate.ok_or(UpdateError::WrongState)?;
  if self.state!=Stage::ReadyToBoot||expected.image_id!=booted_image{
   return Err(UpdateError::WrongState);
  }
  self.confirm(true)
 }
 /// Exhausted attempts require a return to the known-good running slot.
 /// A real bootloader must atomically write this decision to durable media.
 pub fn recover_exhausted(&mut self)->Result<u64,UpdateError>{
  if self.state!=Stage::ReadyToBoot||self.attempts!=0{
   return Err(UpdateError::WrongState);
  }
  let safe_version=self.running;
  self.rollback();Ok(safe_version)
 }
 /// Stop a staged update when an offline policy rejects a release before
 /// it has been marked bootable; never turn rejection into confirmation.
 pub fn abort_staged(&mut self)->Result<(),UpdateError>{
  if self.state!=Stage::Staged{return Err(UpdateError::WrongState);}
  self.rollback();Ok(())
 }
 /// Monotonic version inspection for the release manifest verifier.
 pub fn running_floor(&self)->(u64,u64){(self.running,self.floor)}
}
#[cfg(test)]
mod orbit_r005_smart_update_tests{
 use super::*;
 fn signed(v:u64,id:u32)->Candidate{
  Candidate{image_id:id,version:v,verified_signature:true,
   payload_digest_verified:true,target_arch:62}
 }
 #[test]fn only_expected_booted_image_can_confirm_a_slot(){
  let mut u=Update::new(4,4);u.stage(signed(5,8),62).unwrap();
  u.prepared(true,2).unwrap();
  assert_eq!(u.confirm_exact(7,true),Err(UpdateError::WrongState));
  assert_eq!(u.confirm_exact(8,false),Err(UpdateError::WrongState));
  assert_eq!(u.running_floor(),(4,4));
  assert_eq!(u.confirm_exact(8,true),Ok(()));
  assert_eq!(u.running_floor(),(5,5));
 }
 #[test]fn exhausted_boot_attempts_roll_back_to_last_good_version(){
  let mut u=Update::new(10,10);u.stage(signed(11,42),62).unwrap();
  u.prepared(true,1).unwrap();
  assert_eq!(u.recover_exhausted(),Err(UpdateError::WrongState));
  assert_eq!(u.attempt(),Ok(42));
  assert_eq!(u.pending(),Some((42,11,0)));
  assert_eq!(u.recover_exhausted(),Ok(10));
  assert_eq!(u.state(),Stage::Idle);
  assert_eq!(u.pending(),None);
 }
 #[test]fn reject_before_activation_and_prevent_downgrade(){
  let mut u=Update::new(8,8);
  u.stage(signed(9,90),62).unwrap();u.abort_staged().unwrap();
  assert_eq!(u.pending(),None);
  assert_eq!(u.stage(signed(7,90),62),Err(UpdateError::Downgrade));
  assert_eq!(u.abort_staged(),Err(UpdateError::WrongState));
 }
}
