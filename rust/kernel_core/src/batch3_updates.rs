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


/// R006 automatic update orchestration controls. The future network service
/// must download over authenticated HTTPS, verify signed metadata and stage
/// durable inactive-slot bytes. This code controls WHEN it may do so.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Channel{Stable,Beta}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct UpdateEnvironment{
 pub internet_authenticated:bool,pub metered:bool,pub allow_metered:bool,
 pub battery_percent:u8,pub external_power:bool,
 pub active_user_session:bool,pub restart_approved:bool,
 pub selected_channel:Channel,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ReleasePolicy{
 pub channel:Channel,pub rollout_percentage:u8,
 pub device_cohort:u8,pub urgent:bool,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum GateError{Offline,Metered,Power,BadRollout,Channel,Busy,Approval,Image(UpdateError)}
impl From<UpdateError> for GateError{
 fn from(x:UpdateError)->Self{Self::Image(x)}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct UpdateDecision{pub eligible:bool,pub restart_now:bool}
pub fn preflight_download(env:UpdateEnvironment,policy:ReleasePolicy)
 ->Result<UpdateDecision,GateError>{
 if policy.rollout_percentage==0||policy.rollout_percentage>100||
   policy.device_cohort>=100{return Err(GateError::BadRollout);}
 if policy.channel!=env.selected_channel{return Err(GateError::Channel);}
 if !env.internet_authenticated{return Err(GateError::Offline);}
 if env.metered&&!env.allow_metered{return Err(GateError::Metered);}
 if !env.external_power&&env.battery_percent<40{return Err(GateError::Power);}
 Ok(UpdateDecision{eligible:policy.device_cohort<policy.rollout_percentage,
   restart_now:env.restart_approved&&!env.active_user_session})
}
pub struct SmartUpdate{pub slot:Update,download_eligible:bool}
impl SmartUpdate{
 pub const fn new(running:u64,floor:u64)->Self{
  Self{slot:Update::new(running,floor),download_eligible:false}
 }
 /// Mark an image for trusted inactive-slot staging only after passing
 /// signature/digest/architecture/downgrade and rollout policy checks.
 pub fn prepare(&mut self,env:UpdateEnvironment,policy:ReleasePolicy,
  candidate:Candidate,host_arch:u16)->Result<(),GateError>{
  let eligible=preflight_download(env,policy)?;
  if !eligible.eligible{return Err(GateError::BadRollout);}
  self.slot.stage(candidate,host_arch)?;
  self.download_eligible=true;Ok(())
 }
 /// Requires genuine inactive slot flush plus an explicit restart choice
 /// while a user session is active. No background forced reboot.
 pub fn activate(&mut self,env:UpdateEnvironment,
  inactive_slot_flushed:bool,retries:u8)->Result<(),GateError>{
  if !self.download_eligible{return Err(GateError::Busy);}
  if env.active_user_session&&!env.restart_approved{
   return Err(GateError::Approval);
  }
  self.slot.prepared(inactive_slot_flushed,retries)?;
  Ok(())
 }
 /// Identify exactly which image was booted and health-tested. A wrong
 /// boot slot never advances the anti-rollback floor.
 pub fn confirm(&mut self,image:u32,healthy:bool)->Result<(),GateError>{
  self.slot.confirm_exact(image,healthy)?;
  self.download_eligible=false;Ok(())
 }
 pub fn cancel(&mut self){self.slot.rollback();self.download_eligible=false;}
}
#[cfg(test)]
mod r006_ota_workflow_tests{
 use super::*;
 fn env()->UpdateEnvironment{
  UpdateEnvironment{internet_authenticated:true,metered:false,
   allow_metered:false,battery_percent:70,external_power:false,
   active_user_session:true,restart_approved:false,selected_channel:Channel::Stable}
 }
 fn policy()->ReleasePolicy{ReleasePolicy{
  channel:Channel::Stable,rollout_percentage:25,device_cohort:10,urgent:false}}
 fn signed()->Candidate{Candidate{image_id:88,version:12,verified_signature:true,
  payload_digest_verified:true,target_arch:62}}
 #[test]fn rollout_power_network_and_metered_constraints_are_enforced(){
  let e=env();let p=policy();
  assert!(preflight_download(e,p).unwrap().eligible);
  assert_eq!(preflight_download(UpdateEnvironment{internet_authenticated:false,..e},p),
   Err(GateError::Offline));
  assert_eq!(preflight_download(UpdateEnvironment{metered:true,..e},p),
   Err(GateError::Metered));
  assert_eq!(preflight_download(UpdateEnvironment{battery_percent:9,..e},p),
   Err(GateError::Power));
  assert!(!preflight_download(e,ReleasePolicy{device_cohort:30,..p}).unwrap().eligible);
 }
 #[test]fn pending_user_approval_and_verified_slot_are_mandatory(){
  let mut agent=SmartUpdate::new(11,11);
  agent.prepare(env(),policy(),signed(),62).unwrap();
  assert_eq!(agent.activate(env(),true,2),Err(GateError::Approval));
  let approved=UpdateEnvironment{restart_approved:true,..env()};
  assert_eq!(agent.activate(approved,false,2),Err(GateError::Image(UpdateError::WrongState)));
  agent.activate(approved,true,2).unwrap();
  assert_eq!(agent.confirm(87,true),Err(GateError::Image(UpdateError::WrongState)));
  agent.confirm(88,true).unwrap();
  assert_eq!(agent.slot.running_floor(),(12,12));
 }
 #[test]fn bad_signatures_never_publish_download_eligibility(){
  let mut agent=SmartUpdate::new(10,10);
  let invalid=Candidate{verified_signature:false,..signed()};
  assert_eq!(agent.prepare(env(),policy(),invalid,62),
   Err(GateError::Image(UpdateError::Untrusted)));
  assert_eq!(agent.activate(env(),true,2),Err(GateError::Busy));
  assert_eq!(agent.slot.state(),Stage::Idle);
 }
}

 
/// R010 deterministic on-device OTA polling scheduler. A trusted clock,
/// signed manifest fetcher, durable A/B slot writer and real HTTPS stack
/// must be connected separately. This type never downloads or reboots.
pub struct UpdatePoller{
 next_check:u64,interval:u64,failures:u8,
}
impl UpdatePoller{
 pub fn new(now:u64,interval:u64)->Result<Self,GateError>{
  if interval==0||interval>86_400{return Err(GateError::Busy);}
  let next=now.checked_add(interval).ok_or(GateError::Busy)?;
  Ok(Self{next_check:next,interval,failures:0})
 }
 pub fn next_check(&self)->u64{self.next_check}
 pub fn eligible(&self,now:u64)->bool{now>=self.next_check}
 pub fn record_check(&mut self,now:u64,success:bool)->Result<u64,GateError>{
  if now<self.next_check{return Err(GateError::Busy);}
  let failures=if success{0}else{self.failures.saturating_add(1).min(5)};
  let factor=1u64<<u32::from(failures);
  let delay=self.interval.checked_mul(factor).ok_or(GateError::Busy)?;
  let next=now.checked_add(delay).ok_or(GateError::Busy)?;
  self.failures=failures;self.next_check=next;Ok(next)
 }
 /// Never force a reboot during an active session without explicit consent.
 pub fn may_restart(&self,environment:UpdateEnvironment)->bool{
  environment.restart_approved || !environment.active_user_session
 }
}
#[cfg(test)]
mod r010_ota_poller_tests{
 use super::*;
 #[test]fn poller_schedules_periodic_success_and_backoff(){
  let mut poller=UpdatePoller::new(100,10).unwrap();
  assert!(!poller.eligible(109));assert!(poller.eligible(110));
  assert_eq!(poller.record_check(110,true),Ok(120));
  assert_eq!(poller.record_check(120,false),Ok(140));
  assert_eq!(poller.record_check(140,false),Ok(180));
  assert_eq!(poller.record_check(180,true),Ok(190));
 }
 #[test]fn impossible_timestamp_cannot_overwrite_valid_schedule(){
  let mut poller=UpdatePoller::new(100,10).unwrap();
  assert_eq!(poller.record_check(109,false),Err(GateError::Busy));
  assert_eq!(poller.next_check(),110);
  assert_eq!(poller.record_check(u64::MAX,false),Err(GateError::Busy));
  assert_eq!(poller.next_check(),110);
 }
 #[test]fn restart_approval_is_explicit_in_active_session(){
  let poller=UpdatePoller::new(0,10).unwrap();
  let environment=UpdateEnvironment{internet_authenticated:true,metered:false,
   allow_metered:false,battery_percent:80,external_power:false,
   active_user_session:true,restart_approved:false,selected_channel:Channel::Stable};
  assert!(!poller.may_restart(environment));
  assert!(poller.may_restart(UpdateEnvironment{restart_approved:true,..environment}));
  assert!(poller.may_restart(UpdateEnvironment{active_user_session:false,..environment}));
 }
}

 
/// R011 signed-update *control plane*. Trusted downloader and storage worker
/// must verify bytes and flush an inactive partition before reporting staged.
/// This does not implement TLS, signature cryptography, or firmware writes.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum BootSlot{A,B}
pub struct SlotTransaction{
 active:BootSlot,pending:Option<(BootSlot,Candidate,u64)>,
 attempts_left:u8,anti_rollback:u64,
 issued:Option<BootHealthChallenge>,
}
impl SlotTransaction{
 pub const fn new(active:BootSlot,running_version:u64)->Self{
  Self{active,pending:None,attempts_left:0,anti_rollback:running_version,
   issued:None}
 }
 pub fn active(&self)->BootSlot{self.active}
 pub fn pending(&self)->bool{self.pending.is_some()}
 pub fn stage(&mut self,slot:BootSlot,candidate:Candidate,
  flushed:bool,deadline:u64,attempts:u8)->Result<(),UpdateError>{
  if self.pending.is_some()||slot==self.active||!flushed||attempts==0||
    candidate.image_id==0||candidate.version<=self.anti_rollback{
   return Err(UpdateError::WrongState);
  }
  if !candidate.verified_signature||!candidate.payload_digest_verified{
   return Err(UpdateError::Untrusted);
  }
  self.pending=Some((slot,candidate,deadline));
  self.attempts_left=attempts;self.issued=None;Ok(())
 }
 /// The firmware boot adapter must select this candidate only after a
 /// durable slot write and can retry at most the verified budget.
 pub fn boot_attempt(&mut self,now:u64)->Result<(BootSlot,u32),UpdateError>{
  let (slot,candidate,deadline)=self.pending.ok_or(UpdateError::WrongState)?;
  if now>deadline||self.attempts_left==0{return Err(UpdateError::NoAttempts);}
  self.attempts_left-=1;Ok((slot,candidate.image_id))
 }
 pub fn confirm_health(&mut self,slot:BootSlot,image:u32,now:u64,
  healthy:bool)->Result<u64,UpdateError>{
  let(expected,candidate,deadline)=self.pending.ok_or(UpdateError::WrongState)?;
  if !healthy||now>deadline||slot!=expected||image!=candidate.image_id{
   return Err(UpdateError::WrongState);
  }
  self.anti_rollback=self.anti_rollback.max(candidate.version);
  self.active=slot;self.pending=None;self.attempts_left=0;
  self.issued=None;
  Ok(self.anti_rollback)
 }
 pub fn fallback(&mut self)->BootSlot{
  self.pending=None;self.attempts_left=0;self.issued=None;self.active
 }
 pub fn floor(&self)->u64{self.anti_rollback}
}
#[cfg(test)]
mod r011_ota_slot_tests{
 use super::*;
 fn candidate(version:u64)->Candidate{
  Candidate{image_id:42,version,verified_signature:true,
   payload_digest_verified:true,target_arch:62}
 }
 #[test]fn verified_slot_transitions_only_after_correct_health(){
  let mut t=SlotTransaction::new(BootSlot::A,10);
  t.stage(BootSlot::B,candidate(11),true,100,2).unwrap();
  assert_eq!(t.boot_attempt(30),Ok((BootSlot::B,42)));
  assert_eq!(t.confirm_health(BootSlot::A,42,40,true),Err(UpdateError::WrongState));
  assert_eq!(t.active(),BootSlot::A);
  assert_eq!(t.confirm_health(BootSlot::B,42,40,true),Ok(11));
  assert_eq!(t.active(),BootSlot::B);
  assert_eq!(t.floor(),11);
 }
 #[test]fn exhausted_boot_attempts_keep_safe_slot_active(){
  let mut t=SlotTransaction::new(BootSlot::A,10);
  t.stage(BootSlot::B,candidate(11),true,100,1).unwrap();
  assert_eq!(t.boot_attempt(10),Ok((BootSlot::B,42)));
  assert_eq!(t.boot_attempt(11),Err(UpdateError::NoAttempts));
  assert_eq!(t.fallback(),BootSlot::A);
  assert_eq!(t.active(),BootSlot::A);
 }
 #[test]fn downgrade_and_invalid_signature_cannot_publish_pending_slot(){
  let mut t=SlotTransaction::new(BootSlot::B,9);
  let mut bad=candidate(10);bad.verified_signature=false;
  assert_eq!(t.stage(BootSlot::A,bad,true,100,1),Err(UpdateError::Untrusted));
  assert!(!t.pending());
  assert_eq!(t.stage(BootSlot::A,candidate(9),true,100,1),
    Err(UpdateError::WrongState));
  assert!(!t.pending());
 }
}


/// R012 persistable A/B slot selection metadata for a trusted boot updater.
/// Metadata bytes must be flushed with the inactive slot's verified image;
/// this encoding is not a substitute for a signed manifest or secure boot.
pub const SLOT_RECORD_BYTES:usize=22;
pub fn encode_slot_record(active:BootSlot,version:u64,
  pending:Option<(BootSlot,u32)>)->[u8;SLOT_RECORD_BYTES]{
 let mut out=[0u8;SLOT_RECORD_BYTES];
 out[..4].copy_from_slice(b"IOS2");
 out[4]=match active{BootSlot::A=>1,BootSlot::B=>2};
 out[5..13].copy_from_slice(&version.to_le_bytes());
 let (code,image)=pending.map_or((0,0),|(s,id)|
   (match s{BootSlot::A=>1,BootSlot::B=>2},id));
 out[13]=code;out[14..18].copy_from_slice(&image.to_le_bytes());
 let mut checksum=0x811c9dc5u32;
 for &value in &out[..18]{
  checksum=(checksum^(value as u32)).wrapping_mul(0x01000193);
 }
 out[18..22].copy_from_slice(&checksum.to_le_bytes());out
}
pub fn decode_slot_record(bytes:&[u8])->Result<(BootSlot,u64,Option<(BootSlot,u32)>),UpdateError>{
 if bytes.len()!=SLOT_RECORD_BYTES||&bytes[..4]!=b"IOS2"{
  return Err(UpdateError::Invalid);
 }
 let mut checksum=0x811c9dc5u32;
 for &v in &bytes[..18]{checksum=(checksum^(v as u32)).wrapping_mul(0x01000193);}
 if bytes[18..22]!=checksum.to_le_bytes(){return Err(UpdateError::Untrusted);}
 let active=match bytes[4]{1=>BootSlot::A,2=>BootSlot::B,_=>return Err(UpdateError::Invalid)};
 let version=u64::from_le_bytes(bytes[5..13].try_into().map_err(|_|UpdateError::Invalid)?);
 let image=u32::from_le_bytes(bytes[14..18].try_into().map_err(|_|UpdateError::Invalid)?);
 let pending=match bytes[13]{
  0 if image==0=>None,
  1 if image!=0&&active!=BootSlot::A=>Some((BootSlot::A,image)),
  2 if image!=0&&active!=BootSlot::B=>Some((BootSlot::B,image)),
  _=>return Err(UpdateError::Invalid),
 };
 if version==0{return Err(UpdateError::Invalid);}
 Ok((active,version,pending))
}
#[cfg(test)]
mod r012_slot_metadata_tests{
 use super::*;
 #[test]fn slot_record_roundtrip_rejects_wrong_active_target(){
  let data=encode_slot_record(BootSlot::A,12,Some((BootSlot::B,44)));
  assert_eq!(decode_slot_record(&data),Ok((BootSlot::A,12,Some((BootSlot::B,44)))));
  assert_eq!(decode_slot_record(&data[..21]),Err(UpdateError::Invalid));
 }
 #[test]fn corrupted_critical_byte_or_invalid_pending_fails_closed(){
  let mut data=encode_slot_record(BootSlot::B,9,None);
  data[4]^=1;
  assert_eq!(decode_slot_record(&data),Err(UpdateError::Untrusted));
  let mut invalid=encode_slot_record(BootSlot::A,10,Some((BootSlot::A,22)));
  assert_eq!(decode_slot_record(&invalid),Err(UpdateError::Invalid));
  invalid[14]^=2;
  assert_eq!(decode_slot_record(&invalid),Err(UpdateError::Untrusted));
 }
 #[test]fn records_keep_version_and_pending_image_identity(){
  let bytes=encode_slot_record(BootSlot::B,1000,Some((BootSlot::A,24)));
  assert_eq!(decode_slot_record(&bytes),Ok((BootSlot::B,1000,Some((BootSlot::A,24)))));
 }
}

/// R013 fail-closed A/B recovery. A checksum alone cannot reauthenticate
/// the staged OS bytes after reboot: pending images require fresh signature
/// and digest verification by the trusted updater before they may be booted.
impl SlotTransaction{
 pub fn boot_record(&self)->Result<[u8;SLOT_RECORD_BYTES],UpdateError>{
  if self.anti_rollback==0{return Err(UpdateError::Invalid);}
  let pending=self.pending.map(|(slot,candidate,_)|(slot,candidate.image_id));
  Ok(encode_slot_record(self.active,self.anti_rollback,pending))
 }
 pub fn recovered_from_record(bytes:&[u8],minimum_version:u64)
   ->Result<Self,UpdateError>{
  let (active,version,_unverified_pending)=decode_slot_record(bytes)?;
  if version<minimum_version{return Err(UpdateError::Downgrade);}
  // Never revive unverified boot attempts from a checksum-only record.
  Ok(Self::new(active,version))
 }
 pub fn recover_expired(&mut self,now:u64)->bool{
  let expired=self.pending.is_some_and(|(_,_,deadline)|
    now>deadline||self.attempts_left==0);
  if expired{self.fallback();}
  expired
 }
 pub fn trusted_boot_image(&self,now:u64)->Option<u32>{
  self.pending.and_then(|(_,candidate,deadline)|
   if now<=deadline&&self.attempts_left>0&&candidate.verified_signature&&
      candidate.payload_digest_verified{Some(candidate.image_id)}
   else{None})
 }
}
#[cfg(test)]
mod r013_ota_recovery_tests{
 use super::*;
 fn image(version:u64)->Candidate{Candidate{image_id:44,version,
   verified_signature:true,payload_digest_verified:true,target_arch:62}}
 #[test]fn reboot_never_automatically_executes_unverified_pending_slot(){
  let mut tx=SlotTransaction::new(BootSlot::A,10);
  tx.stage(BootSlot::B,image(11),true,100,2).unwrap();
  let record=tx.boot_record().unwrap();
  assert_eq!(decode_slot_record(&record),
    Ok((BootSlot::A,10,Some((BootSlot::B,44)))));
  let recovered=SlotTransaction::recovered_from_record(&record,10).unwrap();
  assert_eq!(recovered.active(),BootSlot::A);
  assert!(!recovered.pending());
  assert_eq!(recovered.trusted_boot_image(10),None);
 }
 #[test]fn recovery_fences_downgrades_and_detects_corrupt_metadata(){
  let tx=SlotTransaction::new(BootSlot::B,20);
  let record=tx.boot_record().unwrap();
  assert!(matches!(SlotTransaction::recovered_from_record(&record,21),
    Err(UpdateError::Downgrade)));
  let mut bad=record;bad[8]^=1;
  assert!(matches!(SlotTransaction::recovered_from_record(&bad,10),
    Err(UpdateError::Untrusted)));
 }
 #[test]fn deadline_and_exhausted_attempts_keep_existing_boot_slot(){
  let mut tx=SlotTransaction::new(BootSlot::A,10);
  tx.stage(BootSlot::B,image(11),true,100,1).unwrap();
  assert_eq!(tx.trusted_boot_image(99),Some(44));
  tx.boot_attempt(90).unwrap();
  assert!(tx.recover_expired(91));
  assert_eq!(tx.active(),BootSlot::A);
  assert!(!tx.pending());
 }
}

/// R014 handoff to a trusted boot manager. A boot attempt is authorized
/// only for the exact verified slot/image/version tuple and a bounded
/// deadline. The signature/digest checks still belong to the real updater.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct BootHealthChallenge{
 pub slot:BootSlot,pub image_id:u32,pub version:u64,pub deadline:u64,
}
impl SlotTransaction{
 pub fn issue_boot_challenge(&mut self,now:u64)
   ->Result<BootHealthChallenge,UpdateError>{
  let (slot,candidate,deadline)=self.pending.ok_or(UpdateError::WrongState)?;
  if !candidate.verified_signature||!candidate.payload_digest_verified{
   return Err(UpdateError::Untrusted);
  }
  if self.issued.is_some(){
   // An exhausted retry budget remains NoAttempts even while the most
   // recent challenge is outstanding (backward-compatible ABI semantics).
   return Err(if self.attempts_left==0{
    UpdateError::NoAttempts
   }else{UpdateError::WrongState});
  }
  if candidate.version<=self.anti_rollback||now>deadline{
   return Err(UpdateError::WrongState);
  }
  let (verified_slot,image_id)=self.boot_attempt(now)?;
  let challenge=BootHealthChallenge{slot:verified_slot,image_id,
    version:candidate.version,deadline};
  self.issued=Some(challenge);
  Ok(challenge)
 }
 pub fn confirm_boot_challenge(&mut self,
  challenge:BootHealthChallenge,now:u64,measured_image:u32,
  running_version:u64,healthy:bool)->Result<u64,UpdateError>{
  let (slot,candidate,deadline)=self.pending.ok_or(UpdateError::WrongState)?;
  // Refuse fabricated or stale challenges, including attempts to confirm
  // without actually issuing a trusted boot handoff for this update.
  if self.issued!=Some(challenge)||!healthy||
    now>challenge.deadline||deadline!=challenge.deadline||
    challenge.slot!=slot||challenge.image_id!=candidate.image_id||
    measured_image!=candidate.image_id||
    challenge.version!=candidate.version||running_version!=candidate.version{
   return Err(UpdateError::WrongState);
  }
  self.confirm_health(slot,candidate.image_id,now,true)
 }
 /// A failed boot may explicitly abandon the exact outstanding challenge.
 /// The remaining attempt budget is not replenished.
 pub fn abandon_boot_challenge(&mut self,
   challenge:BootHealthChallenge)->Result<u8,UpdateError>{
  if self.issued!=Some(challenge){return Err(UpdateError::WrongState);}
  self.issued=None;
  if self.attempts_left==0{self.fallback();}
  Ok(self.attempts_left)
 }
 pub fn challenge_pending(&self)->bool{self.issued.is_some()}
 pub fn remaining_boot_attempts(&self)->u8{self.attempts_left}
}
#[cfg(test)]
mod r014_boot_measurement_tests{
 use super::*;
 fn candidate(v:u64)->Candidate{Candidate{image_id:71,version:v,
   verified_signature:true,payload_digest_verified:true,target_arch:62}}
 #[test]fn good_measured_boot_promotes_only_verified_inactive_slot(){
  let mut t=SlotTransaction::new(BootSlot::A,10);
  t.stage(BootSlot::B,candidate(11),true,80,2).unwrap();
  let challenge=t.issue_boot_challenge(20).unwrap();
  assert_eq!(challenge.image_id,71);assert_eq!(t.remaining_boot_attempts(),1);
  assert_eq!(t.confirm_boot_challenge(challenge,30,71,11,true),Ok(11));
  assert_eq!(t.active(),BootSlot::B);assert_eq!(t.floor(),11);
 }
 #[test]fn wrong_measured_version_or_image_does_not_promote_slot(){
  let mut t=SlotTransaction::new(BootSlot::A,10);
  t.stage(BootSlot::B,candidate(11),true,80,2).unwrap();
  let c=t.issue_boot_challenge(20).unwrap();
  assert_eq!(t.confirm_boot_challenge(c,30,72,11,true),
     Err(UpdateError::WrongState));
  assert_eq!(t.confirm_boot_challenge(c,30,71,10,true),
     Err(UpdateError::WrongState));
  assert_eq!(t.active(),BootSlot::A);assert!(t.pending());
 }
 #[test]fn expired_boot_challenge_keeps_last_good_slot(){
  let mut t=SlotTransaction::new(BootSlot::A,10);
  t.stage(BootSlot::B,candidate(11),true,80,1).unwrap();
  let c=t.issue_boot_challenge(20).unwrap();
  assert_eq!(t.issue_boot_challenge(21),Err(UpdateError::NoAttempts));
  assert_eq!(t.confirm_boot_challenge(c,81,71,11,true),
    Err(UpdateError::WrongState));
  assert_eq!(t.fallback(),BootSlot::A);assert!(!t.pending());
 }
}

/// R015 challenge lifecycle tests for no-replay A/B update handoffs.
#[cfg(test)]
mod r015_ota_challenge_lifecycle_tests {
 use super::*;
 fn image()->Candidate{Candidate{image_id:80,version:11,
  verified_signature:true,payload_digest_verified:true,target_arch:62}}
 #[test]
 fn cannot_confirm_without_issue_even_with_guessed_matching_tuple(){
  let mut tx=SlotTransaction::new(BootSlot::A,10);
  tx.stage(BootSlot::B,image(),true,100,2).unwrap();
  let forged=BootHealthChallenge{slot:BootSlot::B,image_id:80,
    version:11,deadline:100};
  assert_eq!(tx.confirm_boot_challenge(forged,20,80,11,true),
    Err(UpdateError::WrongState));
  assert_eq!(tx.floor(),10);assert_eq!(tx.active(),BootSlot::A);
 }
 #[test]
 fn duplicate_handoff_is_rejected_without_spending_second_attempt(){
  let mut tx=SlotTransaction::new(BootSlot::A,10);
  tx.stage(BootSlot::B,image(),true,100,2).unwrap();
  let challenge=tx.issue_boot_challenge(20).unwrap();
  assert_eq!(tx.remaining_boot_attempts(),1);
  assert_eq!(tx.issue_boot_challenge(21),Err(UpdateError::WrongState));
  assert_eq!(tx.remaining_boot_attempts(),1);
  assert!(tx.challenge_pending());
  assert_eq!(tx.confirm_boot_challenge(challenge,22,80,11,true),Ok(11));
  assert!(!tx.challenge_pending());
 }
 #[test]
 fn failed_handoff_requires_explicit_reissue_and_preserves_safe_slot(){
  let mut tx=SlotTransaction::new(BootSlot::A,10);
  tx.stage(BootSlot::B,image(),true,100,2).unwrap();
  let old=tx.issue_boot_challenge(20).unwrap();
  assert_eq!(tx.abandon_boot_challenge(old),Ok(1));
  assert!(!tx.challenge_pending());
  assert_eq!(tx.confirm_boot_challenge(old,21,80,11,true),
    Err(UpdateError::WrongState));
  assert_eq!(tx.active(),BootSlot::A);
  let next=tx.issue_boot_challenge(22).unwrap();
  assert_eq!(tx.remaining_boot_attempts(),0);
  assert_eq!(tx.abandon_boot_challenge(next),Ok(0));
  assert_eq!(tx.active(),BootSlot::A);
  assert!(!tx.pending());assert!(!tx.challenge_pending());
 }
}
