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
 active_slot:Option<usize>,active_sequence:u64,
}
impl<H:HardwareTrust,S:PolicySlots> BootTrust<H,S>{
 pub const fn new(platform:H,slots:S)->Self{
  Self{platform,slots,registry:Registry::new(),
   active_slot:None,active_sequence:0}
 }
 pub fn registry(&self)->Option<&Registry<MAX_PUBLISHERS>>{
  self.active_slot.map(|_|&self.registry)
 }
 pub fn current_sequence(&self)->Option<u64>{
  self.active_slot.map(|_|self.active_sequence)
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
  self.active_slot=None;self.active_sequence=0;
  self.registry=Registry::new(); // fail closed on *any* recovery failure
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
  if snapshot.sequence>floor{
   self.platform.advance_protected_sequence(snapshot.sequence)
    .map_err(|_|Error::PlatformUnavailable)?;
   // Readback catches missing or non-durable hardware acknowledgement.
   let now_floor=self.platform.read_protected_sequence()
    .map_err(|_|Error::PlatformUnavailable)?;
   if now_floor<snapshot.sequence{return Err(Error::PlatformUnavailable);}
  }
  self.registry=registry;
  self.active_slot=Some(slot);
  self.active_sequence=snapshot.sequence;
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
  let floor=self.platform.read_protected_sequence()
    .map_err(|_|Error::PlatformUnavailable)?;
  if update.sequence<=floor||update.sequence<=self.active_sequence{
   return Err(Error::Rollback);
  }
  let root=self.platform.read_authenticated_boot_root()
    .map_err(|_|Error::PlatformUnavailable)?;
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
  self.slots.replace_durably(slot,&bytes)
    .map_err(|_|Error::StorageUnavailable)?;
  // Reread must match precisely; torn/lying storage is rejected now.
  let mut reread=[0u8;SNAPSHOT_BYTES];
  if !self.slots.read(slot,&mut reread)
      .map_err(|_|Error::StorageUnavailable)?||
      reread!=bytes{return Err(Error::StorageUnavailable);}
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
