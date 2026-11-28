//! M2 Phase 4D: fail-closed boot-root and monotonic publisher-policy journal.
//!
//! This module contains a real Ed25519-verified boot policy selection and
//! crash-safe A/B *algorithm*. The two abstract device traits are TRUST
//! BOUNDARIES: they MUST be implemented by an audited UEFI/TPM driver.
//! An untrusted disk value, caller-supplied bool, kernel test key, or ordinary
//! NVRAM counter is NOT sufficient for Secure Boot or anti-rollback.
//!
//! No production hardware TPM/UEFI implementation is included yet. The
//! QEMU adapter intentionally models these trusted interfaces with TEST data.
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,INSTALL};
use crate::m2_trust::{Registry,Anchor};
use crate::m2_trust_policy::{SignedSnapshot,Error as PolicyError,MAX_PUBLISHERS,SNAPSHOT_BYTES};

/// Trusted platform provider, NOT untrusted filesystem state.
/// The boot public key must be authenticated against firmware/ROM trust,
/// and the monotonic counter must be protected across resets and reinstall.
pub trait HardwareTrust{
 fn read_authenticated_boot_root(&self)->Result<[u8;32],()>;
 fn read_protected_sequence(&self)->Result<u64,()>;
 /// Strict monotonic update, durable before returning Ok. Never decrement.
 fn advance_protected_sequence(&mut self,sequence:u64)->Result<(),()>;
}
/// Two independent, crash-durable policy copies. The contract requires that
/// a torn write to one slot CANNOT damage the other committed slot.
pub trait PolicySlots{
 fn read(&mut self,slot:usize,out:&mut[u8;SNAPSHOT_BYTES])
  ->Result<bool,()>;
 fn replace_durably(&mut self,slot:usize,bytes:&[u8;SNAPSHOT_BYTES])
  ->Result<(),()>;
}
#[derive(Debug,PartialEq,Eq)]
pub enum Error{
 PlatformUnavailable,StorageUnavailable,Unauthenticated,NoValidPolicy,
 Rollback,DuplicateSequence,NotReady,Policy(PolicyError),
}
/// Boot supervisor owns trusted state; no userland accessor can install a
/// policy, provide trust roots, or skip the verified boot transition.
pub struct BootTrust<H:HardwareTrust,S:PolicySlots>{
 platform:H,slots:S,registry:Registry<MAX_PUBLISHERS>,
 active_slot:Option<usize>,active_sequence:u64,active_root:Option<[u8;32]>,
}
impl<H:HardwareTrust,S:PolicySlots> BootTrust<H,S>{
 pub const fn new(platform:H,slots:S)->Self{
  Self{platform,slots,registry:Registry::new(),
   active_slot:None,active_sequence:0,active_root:None}
 }
 /// Check platform continuity before exposing a cached policy. The provider
 /// is still a trusted hardware boundary, not caller-supplied state. A real
 /// platform adapter must serialize trust changes with policy consumers.
 fn active_is_current(&self)->bool{
  self.active_slot.is_some()&&self.active_root.is_some_and(|root|
   self.platform.read_authenticated_boot_root()==Ok(root)&&
   self.platform.read_protected_sequence()==Ok(self.active_sequence))
 }
 fn invalidate_active(&mut self){
  self.active_slot=None;self.active_sequence=0;self.active_root=None;
  self.registry=Registry::new();
 }
 /// Check continuity across storage/verification callbacks as well as method
 /// entry. Hardware adapters still must serialize trust changes with users;
 /// this bounded revalidation does not turn separate reads into an atomic TPM
 /// transaction or establish a firmware root by itself.
 fn require_platform(&mut self,root:[u8;32],sequence:u64)->Result<(),Error>{
  let observed=(||{
   let floor=self.platform.read_protected_sequence().map_err(|_|Error::PlatformUnavailable)?;
   let live_root=self.platform.read_authenticated_boot_root().map_err(|_|Error::PlatformUnavailable)?;
   if live_root!=root{return Err(Error::Unauthenticated);}
   if floor!=sequence{return Err(Error::Rollback);}
   Ok(())
  })();
  if observed.is_err(){self.invalidate_active();}observed
 }
 pub fn registry(&self)->Option<&Registry<MAX_PUBLISHERS>>{
  self.active_is_current().then_some(&self.registry)
 }
 pub fn current_sequence(&self)->Option<u64>{
  self.active_is_current().then_some(self.active_sequence)
 }
 pub fn platform(&self)->&H{&self.platform}
 pub fn slots(&self)->&S{&self.slots}
 pub fn into_parts(self)->(H,S){(self.platform,self.slots)}
 fn verify_snapshot<const N:usize>(
  candidate:&SignedSnapshot,
  root:&[u8;32],
  auth:&Access<N>,administrator:Identity,now:u64
 )->Result<Registry<MAX_PUBLISHERS>,Error>{
  let mut staging=Registry::new();
  // validate layout, complete signature, caller rights and signer entries.
  // The floor is enforced separately with independently verified hardware
  // state; equality is necessary for crash replay AFTER TPM advancement.
  candidate.verify_and_install(&mut staging,root,
      candidate.sequence.saturating_sub(1),auth,administrator,now)
    .map_err(Error::Policy)?;
  Ok(staging)
 }
 /// Crash/reboot recover: never expose a registry until authenticated
 /// publisher state and protected monotonic sequence agree.
 ///
 /// Sequence update occurs only AFTER a valid signed snapshot already lives
 /// durably in one of the two independent slots. If power is lost after
 /// the hardware counter advances, this same slot can recover at equality.
 pub fn boot_recover<const N:usize>(
  &mut self,auth:&Access<N>,administrator:Identity,now:u64
 )->Result<u64,Error>{
  self.invalidate_active(); // fail closed on *any* recovery failure
  auth.require(administrator,INSTALL,now)
   .map_err(|_|Error::Unauthenticated)?;
  let root=self.platform.read_authenticated_boot_root()
    .map_err(|_|Error::PlatformUnavailable)?;
  let floor=self.platform.read_protected_sequence()
    .map_err(|_|Error::PlatformUnavailable)?;
  if root==[0;32]||root==[0xff;32]{
   return Err(Error::Unauthenticated);
  }
  let mut selected:Option<(usize,SignedSnapshot,Registry<MAX_PUBLISHERS>)>=None;
  for slot in 0..2{
   let mut buffer=[0u8;SNAPSHOT_BYTES];
   let exists=self.slots.read(slot,&mut buffer)
    .map_err(|_|Error::StorageUnavailable)?;
   if !exists{continue;}
   let Ok(snapshot)=SignedSnapshot::parse(&buffer)else{continue;};
   if snapshot.sequence<floor{continue;}
   let Ok(registry)=Self::verify_snapshot(
      &snapshot,&root,auth,administrator,now)else{continue;};
   match &selected{
    Some((_,old,_)) if old.sequence>snapshot.sequence=>continue,
    Some((_,old,_)) if old.sequence==snapshot.sequence=>{
     if old.to_bytes()!=snapshot.to_bytes(){
      return Err(Error::DuplicateSequence); // conflicting signed policy
     }
     continue;
    },
    _=>{}
   }
   selected=Some((slot,snapshot,registry));
  }
  let Some((slot,snapshot,registry))=selected else{
   return Err(Error::NoValidPolicy);
  };
  // Selection reads more than one slot. Re-pin the selected durable bytes
  // before making an irreversible protected-counter update: a replaced or
  // removed selected slot must not strand the counter above all valid media.
  let mut pinned=[0u8;SNAPSHOT_BYTES];
  if !self.slots.read(slot,&mut pinned).map_err(|_|Error::StorageUnavailable)?||
     pinned!=snapshot.to_bytes(){return Err(Error::StorageUnavailable);}
  self.require_platform(root,floor)?;
  if snapshot.sequence>floor{
   self.platform.advance_protected_sequence(snapshot.sequence)
    .map_err(|_|Error::PlatformUnavailable)?;
   // Readback catches missing or non-durable hardware acknowledgement.
   let now_floor=self.platform.read_protected_sequence()
    .map_err(|_|Error::PlatformUnavailable)?;
   if now_floor!=snapshot.sequence{return Err(Error::PlatformUnavailable);}
  }
  self.require_platform(root,snapshot.sequence)?;
  self.registry=registry;
  self.active_slot=Some(slot);
  self.active_sequence=snapshot.sequence;
  self.active_root=Some(root);
  Ok(snapshot.sequence)
 }
 /// Stage a higher sequence to the INACTIVE durable slot only. No key
 /// material becomes authorized until a separate boot_recover() verifies
 /// the reread content and advances the hardware counter.
 pub fn stage_signed_update<const N:usize>(
  &mut self,update:&SignedSnapshot,
  auth:&Access<N>,administrator:Identity,now:u64
 )->Result<usize,Error>{
  let old_slot=self.active_slot.ok_or(Error::NotReady)?;
  let floor=self.active_sequence;
  let root=self.active_root.ok_or(Error::NotReady)?;
  self.require_platform(root,floor)?;
  if update.sequence<=floor||update.sequence<=self.active_sequence{
   return Err(Error::Rollback);
  }
  let proposed=Self::verify_snapshot(update,&root,auth,administrator,now)?;
  // Never accept signer-epoch rollback, or implicit reactivation of a
  // locally revoked signer at the same epoch.
  for proposed_anchor in update.publishers.iter()
      .take(update.count as usize){
   if let Some(existing)=self.registry.get(proposed_anchor.signer){
    if proposed_anchor.epoch<existing.epoch||
      (proposed_anchor.epoch==existing.epoch&&
       (!existing.enabled&&proposed_anchor.enabled||
         existing.public_key!=proposed_anchor.public_key)){
     return Err(Error::Rollback);
    }
   }
  }
  drop(proposed);
  let slot=1-old_slot;
  let bytes=update.to_bytes();
  self.require_platform(root,floor)?;
  let stored=(||{
   self.slots.replace_durably(slot,&bytes).map_err(|_|Error::StorageUnavailable)?;
   // Reread must match precisely; torn/lying storage is rejected now.
   let mut reread=[0u8;SNAPSHOT_BYTES];
   if !self.slots.read(slot,&mut reread).map_err(|_|Error::StorageUnavailable)?||
      reread!=bytes{return Err(Error::StorageUnavailable);}
   Ok(())
  })();
  // Also run on I/O failure: a provider failure must not hide simultaneous
  // root/counter change and leave cached authorization able to revive.
  self.require_platform(root,floor)?;
  stored?;
  Ok(slot)
 }
}
#[cfg(test)]
mod m2_tests{
 use super::*;
 use crate::m2_ed25519::Ed25519Backend;
 use crate::m2_trust::SignatureBackend;
 use ed25519_compact::{KeyPair,Seed};
 const BOOT:[u8;32]=[32,18,203,144,202,96,232,229,216,218,246,110,34,114,210,35,62,4,134,213,87,232,198,97,65,237,137,32,23,125,126,183];
 const PUBLISHER:[u8;32]=[72,7,90,89,126,114,26,21,110,46,7,153,222,92,192,197,50,77,198,231,234,241,205,212,98,80,134,142,197,50,21,221];
 #[derive(Clone)]
 struct EmulatedHardware{boot_root:[u8;32],floor:u64,fail_root:bool,
   fail_read:bool,fail_advance:bool,lie_advance:bool}
 impl EmulatedHardware{
  fn new()->Self{Self{boot_root:BOOT,floor:0,fail_root:false,
   fail_read:false,fail_advance:false,lie_advance:false}}
 }
 impl HardwareTrust for EmulatedHardware{
  fn read_authenticated_boot_root(&self)->Result<[u8;32],()>{
   if self.fail_root{Err(())}else{Ok(self.boot_root)}
  }
  fn read_protected_sequence(&self)->Result<u64,()>{
   if self.fail_read{Err(())}else{Ok(self.floor)}
  }
  fn advance_protected_sequence(&mut self,seq:u64)->Result<(),()>{
   if self.fail_advance||seq<self.floor{return Err(());}
   if !self.lie_advance{self.floor=seq;}
   Ok(())
  }
 }
 #[derive(Clone)]
 struct EmulatedSlots{slots:[Option<[u8;SNAPSHOT_BYTES]>;2],
  fail_write:bool,torn_write:bool,fail_read:bool}
 impl EmulatedSlots{
  fn new()->Self{Self{slots:[None;2],fail_write:false,
     torn_write:false,fail_read:false}}
 }
 impl PolicySlots for EmulatedSlots{
  fn read(&mut self,slot:usize,out:&mut[u8;SNAPSHOT_BYTES])->Result<bool,()>{
   if self.fail_read{return Err(());}
   if let Some(bytes)=self.slots[slot]{*out=bytes;Ok(true)}
   else{Ok(false)}
  }
  fn replace_durably(&mut self,slot:usize,bytes:&[u8;SNAPSHOT_BYTES])
   ->Result<(),()>{
   if self.fail_write{return Err(());}
   if self.torn_write{
    let mut b=*bytes;b[20]^=5;self.slots[slot]=Some(b);
    return Err(());
   }
   self.slots[slot]=Some(*bytes);Ok(())
  }
 }
 fn who()->Identity{Identity{pid:8,generation:1}}
 fn access()->Access<1>{let mut a=Access::new();a.grant(who(),INSTALL,30).unwrap();a}
 fn policy(seq:u64,epoch:u64,enabled:bool)->SignedSnapshot{
  let mut s=SignedSnapshot::empty();
  s.sequence=seq;s.count=1;
  s.publishers[0]=Anchor{signer:7,epoch,public_key:PUBLISHER,enabled};
  // Test-only deterministic private seed; NEVER linked outside host tests.
  let kp=KeyPair::from_seed(Seed::new([b'R';32]));
  let digest=crate::m2_sha256::digest(&s.transcript());
  let sig=kp.sk.sign(&digest,None);
  s.signature.copy_from_slice(&sig[..]);
  assert!(Ed25519Backend.verify_ed25519(&BOOT,&digest,&s.signature));
  s
 }
 fn bootstrapped()->BootTrust<EmulatedHardware,EmulatedSlots>{
  let mut slots=EmulatedSlots::new();
  slots.slots[0]=Some(policy(3,2,true).to_bytes());
  let mut b=BootTrust::new(EmulatedHardware::new(),slots);
  assert_eq!(b.boot_recover(&access(),who(),1),Ok(3));
  assert_eq!(b.registry().unwrap().public_key(7,2),Ok(PUBLISHER));
  b
 }
 // Test-only providers model changes at external I/O/firmware boundaries;
 // production must supply audited, serialized hardware and independent slots.
 #[derive(Clone,Copy)]enum Mutation{Root,Floor,Unavailable,RemoveSelected}
 struct ChangingHardware{
  inner:std::rc::Rc<core::cell::RefCell<EmulatedHardware>>,
  advances:std::rc::Rc<core::cell::Cell<u32>>,change_on_advance:bool,
 }
 impl HardwareTrust for ChangingHardware{
  fn read_authenticated_boot_root(&self)->Result<[u8;32],()>{self.inner.borrow().read_authenticated_boot_root()}
  fn read_protected_sequence(&self)->Result<u64,()>{self.inner.borrow().read_protected_sequence()}
  fn advance_protected_sequence(&mut self,sequence:u64)->Result<(),()>{
   self.advances.set(self.advances.get()+1);
   self.inner.borrow_mut().advance_protected_sequence(sequence)?;
   if self.change_on_advance{self.inner.borrow_mut().boot_root=[9;32];}Ok(())
  }
 }
 struct ChangingSlots{
  inner:EmulatedSlots,hardware:std::rc::Rc<core::cell::RefCell<EmulatedHardware>>,
  reads:usize,on_read:Option<(usize,Mutation)>,on_write:Option<Mutation>,
 }
 impl ChangingSlots{
  fn change(&mut self,mutation:Mutation){
   match mutation{
    Mutation::Root=>self.hardware.borrow_mut().boot_root=[9;32],
    Mutation::Floor=>self.hardware.borrow_mut().floor=4,
    Mutation::Unavailable=>self.hardware.borrow_mut().fail_read=true,
    Mutation::RemoveSelected=>self.inner.slots[0]=None,
   }
  }
 }
 impl PolicySlots for ChangingSlots{
  fn read(&mut self,slot:usize,out:&mut[u8;SNAPSHOT_BYTES])->Result<bool,()>{
   self.reads+=1;
   if let Some((read,mutation))=self.on_read{
    if read==self.reads{self.on_read=None;self.change(mutation);}
   }
   self.inner.read(slot,out)
  }
  fn replace_durably(&mut self,slot:usize,bytes:&[u8;SNAPSHOT_BYTES])->Result<(),()>{
   if let Some(mutation)=self.on_write.take(){self.change(mutation);}
   self.inner.replace_durably(slot,bytes)
  }
 }
 fn changing()->BootTrust<ChangingHardware,ChangingSlots>{
  let hardware=std::rc::Rc::new(core::cell::RefCell::new(EmulatedHardware::new()));
  let mut inner=EmulatedSlots::new();inner.slots[0]=Some(policy(3,2,true).to_bytes());
  BootTrust::new(ChangingHardware{inner:hardware.clone(),advances:std::rc::Rc::new(core::cell::Cell::new(0)),change_on_advance:false},
   ChangingSlots{inner,hardware,reads:0,on_read:None,on_write:None})
 }
 #[test]fn m2_boottrust_slot_callback_cannot_change_platform_before_counter_commit(){
  for (mutation,expected) in [(Mutation::Root,Error::Unauthenticated),
    (Mutation::Floor,Error::Rollback),(Mutation::Unavailable,Error::PlatformUnavailable)]{
   let mut boot=changing();boot.slots.on_read=Some((2,mutation));
   assert_eq!(boot.boot_recover(&access(),who(),1),Err(expected));
   assert_eq!(boot.platform.advances.get(),0);assert!(boot.registry().is_none());
  }
 }
 #[test]fn m2_boottrust_removed_selected_slot_never_advances_the_protected_floor(){
  let mut boot=changing();boot.slots.on_read=Some((2,Mutation::RemoveSelected));
  assert_eq!(boot.boot_recover(&access(),who(),1),Err(Error::StorageUnavailable));
  assert_eq!(boot.platform.advances.get(),0);assert_eq!(boot.platform.inner.borrow().floor,0);
  assert!(boot.active_slot.is_none());
 }
 #[test]fn m2_boottrust_root_change_during_hardware_commit_never_publishes_registry(){
  let mut boot=changing();boot.platform.change_on_advance=true;
  assert_eq!(boot.boot_recover(&access(),who(),1),Err(Error::Unauthenticated));
  assert_eq!(boot.platform.advances.get(),1);assert_eq!(boot.platform.inner.borrow().floor,3);
  assert!(boot.active_slot.is_none());assert!(boot.registry().is_none());
 }
 #[test]fn m2_boottrust_staging_io_changes_retire_authority_even_when_io_fails(){
  for readback in [false,true]{for fail_write in [false,true]{
   if readback&&fail_write{continue;} // failing write cannot reach readback
   let mut boot=changing();assert_eq!(boot.boot_recover(&access(),who(),1),Ok(3));
   let original=boot.slots.inner.slots[0];
   if readback{boot.slots.on_read=Some((boot.slots.reads+1,Mutation::Floor));}
   else{boot.slots.on_write=Some(Mutation::Root);}
   boot.slots.inner.fail_write=fail_write;
   assert_eq!(boot.stage_signed_update(&policy(4,3,true),&access(),who(),1),
    Err(if readback{Error::Rollback}else{Error::Unauthenticated}));
   assert_eq!(boot.slots.inner.slots[0],original);assert!(boot.active_slot.is_none());
   boot.platform.inner.borrow_mut().boot_root=BOOT;boot.platform.inner.borrow_mut().floor=3;
   assert!(boot.registry().is_none()); // restored values cannot revive retired authority
   boot.slots.inner.fail_write=false;
   assert!(boot.boot_recover(&access(),who(),1).is_ok());
  }}
 }
 #[test]fn m2_boottrust_counter_change_retires_cached_policy_before_staging(){
  for floor in [2,4]{
   let mut b=bootstrapped();let slots=b.slots.slots;
   b.platform.floor=floor;
   assert!(b.registry().is_none());assert_eq!(b.current_sequence(),None);
   assert_eq!(b.stage_signed_update(&policy(5,4,true),&access(),who(),1),Err(Error::Rollback));
   assert_eq!(b.slots.slots,slots);assert!(b.active_slot.is_none());
   b.platform.floor=3;
   assert!(b.registry().is_none()); // only recovery can grant authority again
   assert_eq!(b.boot_recover(&access(),who(),1),Ok(3));
  }
 }
 #[test]fn m2_boottrust_changed_root_cannot_bridge_old_registry_to_update(){
  for root in [[0;32],[0xff;32],[9;32]]{
   let mut b=bootstrapped();let slots=b.slots.slots;b.platform.boot_root=root;
   assert!(b.registry().is_none());assert_eq!(b.current_sequence(),None);
   assert_eq!(b.stage_signed_update(&policy(4,3,true),&access(),who(),1),
      Err(Error::Unauthenticated));
   assert_eq!(b.slots.slots,slots);assert!(b.active_slot.is_none());
   b.platform.boot_root=BOOT;assert!(b.registry().is_none());
   assert_eq!(b.boot_recover(&access(),who(),1),Ok(3));
  }
 }
 #[test]fn m2_boottrust_platform_read_failure_revokes_authorization_without_slot_write(){
  for root_failure in [false,true]{
   let mut b=bootstrapped();let slots=b.slots.slots;
   b.platform.fail_read=!root_failure;b.platform.fail_root=root_failure;
   assert!(b.registry().is_none());assert_eq!(b.current_sequence(),None);
   assert_eq!(b.stage_signed_update(&policy(4,3,true),&access(),who(),1),
      Err(Error::PlatformUnavailable));
   assert_eq!(b.slots.slots,slots);assert!(b.active_slot.is_none());
   b.platform.fail_read=false;b.platform.fail_root=false;
   assert!(b.registry().is_none());
  }
 }
 #[test]fn m2_boottrust_signed_ab_update_then_reboot_and_same_floor_recovers(){
  let mut b=bootstrapped();
  let next=policy(4,3,true);
  assert_eq!(b.stage_signed_update(&next,&access(),who(),1),Ok(1));
  // Staged policy is not yet active; hardware monotonic floor unchanged.
  assert_eq!(b.current_sequence(),Some(3));
  assert_eq!(b.platform().floor,3);
  assert_eq!(b.boot_recover(&access(),who(),1),Ok(4));
  assert_eq!(b.platform().floor,4);
  assert_eq!(b.registry().unwrap().public_key(7,2),
     Err(crate::m2_trust::Error::StaleEpoch));
  assert_eq!(b.registry().unwrap().public_key(7,3),Ok(PUBLISHER));
  let (h,s)=b.into_parts();
  let mut reboot=BootTrust::new(h,s);
  assert_eq!(reboot.boot_recover(&access(),who(),1),Ok(4));
  assert_eq!(reboot.platform().floor,4);
  assert_eq!(reboot.current_sequence(),Some(4));
  assert_eq!(reboot.stage_signed_update(&policy(4,4,true),
      &access(),who(),1),Err(Error::Rollback));
 }
 #[test]fn m2_boottrust_fails_closed_on_torn_disk_and_lost_protected_counter(){
  let mut b=bootstrapped();
  b.slots.torn_write=true;
  assert_eq!(b.stage_signed_update(&policy(4,3,true),&access(),who(),1),
    Err(Error::StorageUnavailable));
  b.slots.torn_write=false;
  // Old published policy is intact, tampered staging copy rejected.
  assert_eq!(b.boot_recover(&access(),who(),1),Ok(3));
  b.platform.fail_advance=true;
  b.slots.slots[1]=Some(policy(4,3,true).to_bytes());
  assert_eq!(b.boot_recover(&access(),who(),1),
    Err(Error::PlatformUnavailable));
  assert_eq!(b.registry().is_some(),false);
  b.platform.fail_advance=false;b.platform.lie_advance=true;
  assert_eq!(b.boot_recover(&access(),who(),1),
    Err(Error::PlatformUnavailable));
  assert_eq!(b.registry().is_some(),false);
 }
 #[test]fn m2_boottrust_revocation_and_epoch_rollback_denied(){
  let mut b=bootstrapped();
  let revoked=policy(4,3,false);
  assert_eq!(b.stage_signed_update(&revoked,&access(),who(),1),Ok(1));
  assert_eq!(b.boot_recover(&access(),who(),1),Ok(4));
  assert_eq!(b.registry().unwrap().public_key(7,3),
    Err(crate::m2_trust::Error::Revoked));
  assert_eq!(b.stage_signed_update(&policy(5,2,true),
    &access(),who(),1),Err(Error::Rollback));
  assert_eq!(b.stage_signed_update(&policy(5,3,true),
    &access(),who(),1),Err(Error::Rollback));
  assert_eq!(b.stage_signed_update(&policy(5,4,true),
    &access(),Identity{generation:2,..who()},1),
    Err(Error::Policy(PolicyError::Unauthorized)));
 }
 #[test]fn m2_boottrust_missing_firmware_root_and_snapshot_never_grants_registry(){
  let mut empty=BootTrust::new(EmulatedHardware::new(),EmulatedSlots::new());
  assert_eq!(empty.boot_recover(&access(),who(),1),Err(Error::NoValidPolicy));
  assert_eq!(empty.registry().is_some(),false);
  let mut b=bootstrapped();
  b.platform.fail_root=true;
  assert_eq!(b.boot_recover(&access(),who(),1),Err(Error::PlatformUnavailable));
  assert_eq!(b.registry().is_some(),false);
  b.platform.fail_root=false;
  b.platform.boot_root=[9;32];
  assert_eq!(b.boot_recover(&access(),who(),1),Err(Error::NoValidPolicy));
  assert_eq!(b.registry().is_some(),false);
  b.platform.boot_root=BOOT;
  b.platform.floor=5;
  assert_eq!(b.boot_recover(&access(),who(),1),Err(Error::NoValidPolicy));
  assert_eq!(b.registry().is_some(),false);
 }
 #[test]fn m2_boottrust_reboot_after_hardware_commit_recovers_equal_sequence(){
  let mut b=bootstrapped();
  b.slots.slots[1]=Some(policy(4,3,true).to_bytes());
  b.platform.floor=4; // simulate power failure just AFTER hardware commit
  let (h,s)=b.into_parts();
  let mut resumed=BootTrust::new(h,s);
  assert_eq!(resumed.boot_recover(&access(),who(),1),Ok(4));
  assert_eq!(resumed.registry().unwrap().public_key(7,3),Ok(PUBLISHER));
  resumed.slots.slots[1]=None; // attacker removes only current policy
  assert_eq!(resumed.boot_recover(&access(),who(),1),Err(Error::NoValidPolicy));
  assert!(resumed.registry().is_none());
 }
}
