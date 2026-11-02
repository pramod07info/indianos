//! Read-only A/B boot-slot selection; does not install or change firmware.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub valid_metadata: bool, pub committed: bool,
    pub version: u64, pub attempts_remaining: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootError { NoBootableSlot }
fn bootable(slot: Slot, min_version: u64) -> bool {
    slot.valid_metadata && slot.committed
        && slot.version >= min_version && slot.attempts_remaining > 0
}
/// Returns the preferred slot index, breaking equal-version ties toward A.
pub fn select_slot(slots: &[Slot; 2], min_version: u64) -> Result<usize, BootError> {
    let a = bootable(slots[0], min_version);
    let b = bootable(slots[1], min_version);
    match (a, b) {
        (false, false) => Err(BootError::NoBootableSlot),
        (true, false) => Ok(0),
        (false, true) => Ok(1),
        (true, true) => if slots[1].version > slots[0].version { Ok(1) } else { Ok(0) },
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn slot(version: u64) -> Slot {
        Slot { valid_metadata: true, committed: true, version, attempts_remaining: 2 }
    }
    #[test]
    fn prefers_newest_committed_slot() {
        assert_eq!(select_slot(&[slot(1),slot(2)], 1), Ok(1));
        assert_eq!(select_slot(&[slot(5),slot(5)], 5), Ok(0));
    }
    #[test]
    fn interrupted_update_falls_back() {
        let mut newer = slot(10);
        newer.committed = false;
        assert_eq!(select_slot(&[slot(8), newer], 8), Ok(0));
        newer.committed = true;
        newer.attempts_remaining = 0;
        assert_eq!(select_slot(&[slot(8), newer], 8), Ok(0));
    }
    #[test]
    fn rollback_floor_cannot_be_bypassed() {
        assert_eq!(select_slot(&[slot(4), slot(3)], 5), Err(BootError::NoBootableSlot));
        let mut corrupt = slot(u64::MAX);
        corrupt.valid_metadata = false;
        assert_eq!(select_slot(&[corrupt, slot(7)], 7), Ok(1));
    }
}

/// Select a slot and consume one permitted boot attempt. The caller must
/// persist the returned metadata transactionally before launching the image.
pub fn consume_boot_attempt(slots: &mut [Slot; 2], min_version: u64)
    -> Result<usize, BootError>
{
    let selected = select_slot(slots, min_version)?;
    slots[selected].attempts_remaining -= 1; // select_slot proves > 0
    Ok(selected)
}
#[cfg(test)]
mod attempt_tests {
    use super::*;
    fn slot(version: u64, tries: u8) -> Slot {
        Slot { valid_metadata: true, committed: true, version, attempts_remaining: tries }
    }
    #[test]
    fn consumes_newer_slot_until_fallback() {
        let mut slots = [slot(3, 2), slot(4, 1)];
        assert_eq!(consume_boot_attempt(&mut slots, 1), Ok(1));
        assert_eq!(slots[1].attempts_remaining, 0);
        assert_eq!(consume_boot_attempt(&mut slots, 1), Ok(0));
    }
    #[test]
    fn no_available_attempts_preserves_metadata() {
        let mut slots = [slot(4, 0), slot(3, 0)];
        let before = slots;
        assert_eq!(consume_boot_attempt(&mut slots, 1), Err(BootError::NoBootableSlot));
        assert_eq!(slots, before);
    }
    #[test]
    fn anti_rollback_floor_is_applied_before_mutation() {
        let mut slots = [slot(5, 2), slot(4, 2)];
        assert_eq!(consume_boot_attempt(&mut slots, 6), Err(BootError::NoBootableSlot));
        assert_eq!(slots[0].attempts_remaining, 2);
    }
}


/// Versioned, corruption-detecting A/B slot metadata record. The caller must
/// write and flush it transactionally on a disposable image; this codec never
/// writes firmware or physical storage. CRC32 detects damage, not forgery.
pub const SLOT_RECORD_BYTES: usize = 32;
fn crc32(data: &[u8]) -> u32 {
    let mut state = !0u32;
    for &byte in data {
        state ^= byte as u32;
        for _ in 0..8 {
            state = (state >> 1) ^ (if state & 1 != 0 { 0xedb8_8320 } else { 0 });
        }
    }
    !state
}
pub fn encode_slot(slot: Slot) -> Option<[u8; SLOT_RECORD_BYTES]> {
    if !slot.valid_metadata || slot.version == 0 { return None; }
    let mut data = [0u8; SLOT_RECORD_BYTES];
    data[..4].copy_from_slice(b"IAB1");
    data[4..12].copy_from_slice(&slot.version.to_le_bytes());
    data[12] = slot.attempts_remaining;
    data[13] = u8::from(slot.committed);
    let checksum = crc32(&data[..28]);
    data[28..32].copy_from_slice(&checksum.to_le_bytes());
    Some(data)
}
pub fn decode_slot(data: &[u8; SLOT_RECORD_BYTES]) -> Option<Slot> {
    if &data[..4] != b"IAB1" || data[13] > 1 ||
        data[14..28].iter().any(|&byte| byte != 0) {
        return None;
    }
    let stored = u32::from_le_bytes(data[28..32].try_into().ok()?);
    if crc32(&data[..28]) != stored { return None; }
    let version = u64::from_le_bytes(data[4..12].try_into().ok()?);
    if version == 0 { return None; }
    Some(Slot {
        valid_metadata: true, committed: data[13] == 1,
        version, attempts_remaining: data[12],
    })
}
#[cfg(test)]
mod slot_codec_tests {
    use super::*;
    fn slot(version: u64, attempts: u8) -> Slot {
        Slot { valid_metadata: true, committed: true, version, attempts_remaining: attempts }
    }
    #[test]
    fn committed_slot_survives_serialization_without_writes() {
        let raw = encode_slot(slot(101, 2)).unwrap();
        assert_eq!(decode_slot(&raw), Some(slot(101, 2)));
    }
    #[test]
    fn torn_metadata_and_reserved_bit_corruption_are_rejected() {
        let mut raw = encode_slot(slot(101, 2)).unwrap();
        raw[12] ^= 1;
        assert_eq!(decode_slot(&raw), None);
        let mut raw = encode_slot(slot(101, 2)).unwrap();
        raw[18] = 1;
        assert_eq!(decode_slot(&raw), None);
    }
    #[test]
    fn interrupted_update_falls_back_to_previous_slot() {
        let old = decode_slot(&encode_slot(slot(10, 2)).unwrap()).unwrap();
        let mut candidate = slot(11, 1);
        candidate.committed = false;
        let new = decode_slot(&encode_slot(candidate).unwrap()).unwrap();
        assert_eq!(select_slot(&[old, new], 10), Ok(0));
        assert_eq!(select_slot(&[old, new], 12), Err(BootError::NoBootableSlot));
    }
}


/// Batch 1: two-copy, checked A/B metadata on an explicitly selected
/// BlockDevice window. This NEVER formats, discovers or selects a physical
/// disk: caller must supply a disposable, preinitialized dedicated volume.
/// The driver must flush each written sector before returning success.
use crate::block::{BlockDevice,SECTOR_BYTES};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum JournalError { Invalid, Io, NoBoot, Exhausted }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Journal { pub generation:u64, pub slots:[Slot;2] }
fn encode_journal(value:Journal)->Option<[u8;SECTOR_BYTES]> {
    if value.generation == 0 { return None; }
    let a=encode_slot(value.slots[0])?;
    let b=encode_slot(value.slots[1])?;
    let mut data=[0u8;SECTOR_BYTES];
    data[..8].copy_from_slice(b"IABJ0001");
    data[8..16].copy_from_slice(&value.generation.to_le_bytes());
    data[16..48].copy_from_slice(&a);
    data[48..80].copy_from_slice(&b);
    let checksum=crc32(&data[..SECTOR_BYTES-4]);
    data[SECTOR_BYTES-4..].copy_from_slice(&checksum.to_le_bytes());
    Some(data)
}
fn decode_journal(data:&[u8;SECTOR_BYTES])->Option<Journal>{
    if &data[..8]!=b"IABJ0001" ||
        data[80..SECTOR_BYTES-4].iter().any(|&b|b!=0) {return None;}
    let recorded=u32::from_le_bytes(data[SECTOR_BYTES-4..].try_into().ok()?);
    if crc32(&data[..SECTOR_BYTES-4])!=recorded {return None;}
    let generation=u64::from_le_bytes(data[8..16].try_into().ok()?);
    if generation==0 {return None;}
    let a=decode_slot(data[16..48].try_into().ok()?)?;
    let b=decode_slot(data[48..80].try_into().ok()?)?;
    Some(Journal{generation,slots:[a,b]})
}
pub fn recover_journal<D:BlockDevice>(disk:&mut D)
    -> Result<(Journal,u64),JournalError> {
    if disk.sector_count()<2{return Err(JournalError::Invalid);}
    let mut latest:Option<(Journal,u64)>=None;
    for lba in 0..2u64 {
        let mut sector=[0u8;SECTOR_BYTES];
        disk.read_sector(lba,&mut sector).map_err(|_|JournalError::Io)?;
        if let Some(record)=decode_journal(&sector) {
            // Two different valid records at the same generation indicate
            // split-brain metadata; never arbitrarily prefer one copy.
            if latest.as_ref().is_some_and(|(old,_)|
                record.generation==old.generation && record!=*old){
                return Err(JournalError::Invalid);
            }
            if latest.as_ref().is_none_or(|(old,_)| record.generation>old.generation){
                latest=Some((record,lba));
            }
        }
    }
    latest.ok_or(JournalError::Invalid)
}
/// Two-phase journal update: first recover the last complete record, then
/// write one NEW copy to the inactive metadata sector. On torn writes, the
/// previous checked generation remains recoverable.
pub fn commit_boot_attempt<D:BlockDevice>(disk:&mut D,min_version:u64)
    -> Result<usize,JournalError>{
    let (mut latest,active)=recover_journal(disk)?;
    let selected=consume_boot_attempt(&mut latest.slots,min_version)
        .map_err(|_|JournalError::NoBoot)?;
    latest.generation=latest.generation.checked_add(1)
        .ok_or(JournalError::Exhausted)?;
    let encoded=encode_journal(latest).ok_or(JournalError::Invalid)?;
    disk.write_sector(1-active,&encoded).map_err(|_|JournalError::Io)?;
    // A successful device acknowledgement alone does not prove that the
    // new boot journal can be read back unchanged.
    let mut readback=[0u8;SECTOR_BYTES];
    disk.read_sector(1-active,&mut readback).map_err(|_|JournalError::Io)?;
    if readback!=encoded{return Err(JournalError::Io);}
    Ok(selected)
}
#[cfg(test)]
mod batch1_disk_journal_tests {
    use super::*;
    use crate::block::{BlockError,check_range};
    struct Disk{sectors:[[u8;SECTOR_BYTES];2],fail:bool}
    impl BlockDevice for Disk{
        fn sector_count(&self)->u64{2}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            check_range(2,lba,1)?;*out=self.sectors[lba as usize];Ok(())
        }
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            check_range(2,lba,1)?;
            if self.fail{return Err(BlockError::Io);}
            self.sectors[lba as usize]=*data;Ok(())
        }
    }
    fn base()->Journal{
        Journal{generation:10,slots:[
            Slot{valid_metadata:true,committed:true,version:2,attempts_remaining:2},
            Slot{valid_metadata:true,committed:true,version:3,attempts_remaining:1},
        ]}
    }
    fn disk()->Disk{
        let encoded=encode_journal(base()).unwrap();
        Disk{sectors:[encoded,encoded],fail:false}
    }
    #[test]
    fn persisted_attempt_and_reboot_fallback() {
        let mut d=disk();
        assert_eq!(commit_boot_attempt(&mut d,1),Ok(1));
        let (new,active)=recover_journal(&mut d).unwrap();
        assert_eq!(new.generation,11);
        assert_eq!(new.slots[1].attempts_remaining,0);
        assert_eq!(active,1);
        assert_eq!(commit_boot_attempt(&mut d,1),Ok(0));
    }
    #[test]
    fn corrupt_inactive_copy_does_not_destroy_last_good_record(){
        let mut d=disk();
        commit_boot_attempt(&mut d,1).unwrap();
        d.sectors[1][8]^=1;
        let (old,active)=recover_journal(&mut d).unwrap();
        assert_eq!(old.generation,10);
        assert_eq!(active,0);
    }
    #[test]
    fn failed_io_and_invalid_journal_never_forge_success(){
        let mut d=disk();d.fail=true;
        assert_eq!(commit_boot_attempt(&mut d,1),Err(JournalError::Io));
        assert_eq!(recover_journal(&mut d).unwrap().0.generation,10);
        let mut blank=Disk{sectors:[[0;SECTOR_BYTES];2],fail:false};
        assert_eq!(commit_boot_attempt(&mut blank,1),Err(JournalError::Invalid));
    }

    #[test]
    fn two_valid_divergent_same_generation_copies_are_rejected(){
        let mut d=disk();
        let mut conflicting=base();
        conflicting.slots[0].version+=1;
        d.sectors[1]=encode_journal(conflicting).unwrap();
        assert_eq!(recover_journal(&mut d),Err(JournalError::Invalid));
    }
    #[test]
    fn identical_generation_copies_still_allow_regular_boot(){
        let mut d=disk();
        assert_eq!(recover_journal(&mut d).unwrap().0.generation,10);
        assert_eq!(commit_boot_attempt(&mut d,1),Ok(1));
        assert_eq!(recover_journal(&mut d).unwrap().0.generation,11);
    }
    struct CorruptingDisk(Disk);
    impl BlockDevice for CorruptingDisk {
        fn sector_count(&self)->u64{self.0.sector_count()}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{self.0.read_sector(lba,out)}
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            self.0.write_sector(lba,data)?;
            self.0.sectors[lba as usize][16]^=1;
            Ok(())
        }
    }
    #[test]
    fn acknowledged_corrupt_write_is_rejected_and_old_record_recovers(){
        let mut d=CorruptingDisk(disk());
        assert_eq!(commit_boot_attempt(&mut d,1),Err(JournalError::Io));
        assert_eq!(recover_journal(&mut d).unwrap().0.generation,10);
    }

}


// R007: upper-layer immutable boot-metadata transitions. The trusted caller
// must perform actual image verification and persist journals through the
// existing readback-verified BlockDevice backend before making them active.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum UpdatePolicyError{Invalid,Stale,Unverified,NoFallback,Exhausted}
pub fn stage_verified_slot(current:Journal,active:usize,new_version:u64,
    verified_by_trusted_loader:bool,attempt_budget:u8)
    ->Result<Journal,UpdatePolicyError>{
    if active>1||current.generation==0||new_version==0||attempt_budget==0{
        return Err(UpdatePolicyError::Invalid);
    }
    if !verified_by_trusted_loader{return Err(UpdatePolicyError::Unverified);}
    let live=current.slots[active];
    if !live.valid_metadata||!live.committed||live.version==0||
       live.attempts_remaining==0{return Err(UpdatePolicyError::NoFallback);}
    if new_version<=live.version{return Err(UpdatePolicyError::Stale);}
    let next_generation=current.generation.checked_add(1)
        .ok_or(UpdatePolicyError::Exhausted)?;
    let mut next=current;
    next.slots[1-active]=Slot{valid_metadata:true,committed:true,
        version:new_version,attempts_remaining:attempt_budget};
    next.generation=next_generation;
    Ok(next)
}
pub fn confirm_boot_healthy(current:Journal,slot:usize,
    observed_version:u64,reset_attempts:u8)
    ->Result<Journal,UpdatePolicyError>{
    if slot>1||current.generation==0||reset_attempts==0{
        return Err(UpdatePolicyError::Invalid);
    }
    let actual=current.slots[slot];
    if !actual.valid_metadata||!actual.committed||
       actual.version!=observed_version||actual.attempts_remaining==0{
        return Err(UpdatePolicyError::Stale);
    }
    let mut updated=current;
    updated.generation=current.generation.checked_add(1)
        .ok_or(UpdatePolicyError::Exhausted)?;
    updated.slots[slot].attempts_remaining=reset_attempts;
    Ok(updated)
}
pub fn rollback_after_failure(current:Journal,failed:usize,min_version:u64)
    ->Result<(Journal,usize),UpdatePolicyError>{
    if failed>1||current.generation==0{return Err(UpdatePolicyError::Invalid);}
    let backup=1-failed;
    let previous=current.slots[backup];
    if !bootable(previous,min_version){return Err(UpdatePolicyError::NoFallback);}
    let mut next=current;
    next.generation=current.generation.checked_add(1)
        .ok_or(UpdatePolicyError::Exhausted)?;
    next.slots[failed].attempts_remaining=0;
    Ok((next,backup))
}
#[cfg(test)]mod r007_boot_policy_tests{
    use super::*;
    fn base()->Journal{Journal{generation:10,slots:[
        Slot{valid_metadata:true,committed:true,version:5,attempts_remaining:3},
        Slot{valid_metadata:true,committed:false,version:0,attempts_remaining:0}]}}
    #[test]fn stages_upgrade_and_validates_observed_health(){
        let staged=stage_verified_slot(base(),0,6,true,2).unwrap();
        assert_eq!(staged.slots[1].version,6);
        assert_eq!(staged.generation,11);
        let confirmed=confirm_boot_healthy(staged,1,6,4).unwrap();
        assert_eq!(confirmed.slots[1].attempts_remaining,4);
        assert_eq!(confirmed.generation,12);
        assert_eq!(confirm_boot_healthy(staged,1,5,4),Err(UpdatePolicyError::Stale));
    }
    #[test]fn rollback_requires_known_good_anti_rollback_compliant_fallback(){
        let staged=stage_verified_slot(base(),0,7,true,2).unwrap();
        assert_eq!(rollback_after_failure(staged,1,5).unwrap().1,0);
        assert_eq!(rollback_after_failure(staged,1,6),Err(UpdatePolicyError::NoFallback));
        assert_eq!(rollback_after_failure(staged,0,5).unwrap().1,1);
    }
    #[test]fn unsigned_downgrade_and_missing_budget_never_stage(){
        assert_eq!(stage_verified_slot(base(),0,7,false,2),Err(UpdatePolicyError::Unverified));
        assert_eq!(stage_verified_slot(base(),0,5,true,2),Err(UpdatePolicyError::Stale));
        assert_eq!(stage_verified_slot(base(),0,7,true,0),Err(UpdatePolicyError::Invalid));
    }
}


// R008: update trial state around checked two-copy boot journal. Network
// transport, firmware writes and installer signature verification are external.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum TrialFault{Invalid,Pending,Missing,Policy(UpdatePolicyError)}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct BootTrial{
 pub journal:Journal,pub active:usize,pub pending:Option<usize>,
 pub minimum_version:u64,pub attempted:bool,
}
impl BootTrial{
 pub fn new(journal:Journal,active:usize,minimum_version:u64)->Result<Self,TrialFault>{
  if active>1||journal.generation==0||
   !bootable(journal.slots[active],minimum_version){
   return Err(TrialFault::Invalid)
  }
  Ok(Self{journal,active,pending:None,minimum_version,attempted:false})
 }
 pub fn stage(&mut self,version:u64,verified:bool,budget:u8,
  power_sufficient:bool)->Result<usize,TrialFault>{
  if self.pending.is_some(){return Err(TrialFault::Pending)}
  if !power_sufficient{return Err(TrialFault::Invalid)}
  let update=stage_verified_slot(self.journal,self.active,version,verified,budget)
   .map_err(TrialFault::Policy)?;
  let target=1-self.active;
  if !bootable(update.slots[target],self.minimum_version){return Err(TrialFault::Invalid)}
  self.journal=update;self.pending=Some(target);self.attempted=false;Ok(target)
 }
 pub fn consume_attempt(&mut self,selected:usize)->Result<u8,TrialFault>{
  if self.pending!=Some(selected){return Err(TrialFault::Missing)}
  if self.attempted{return Err(TrialFault::Invalid)}
  let s=self.journal.slots[selected];
  if !bootable(s,self.minimum_version){return Err(TrialFault::Invalid)}
  let next=self.journal.generation.checked_add(1).ok_or(TrialFault::Invalid)?;
  self.journal.slots[selected].attempts_remaining-=1;
  self.journal.generation=next;self.attempted=true;
  Ok(self.journal.slots[selected].attempts_remaining)
 }
 pub fn healthy(&mut self,selected:usize,observed_version:u64,budget:u8)
  ->Result<(),TrialFault>{
  if self.pending!=Some(selected){return Err(TrialFault::Missing)}
  if !self.attempted||budget==0{return Err(TrialFault::Invalid)}
  let slot=self.journal.slots[selected];
  if !slot.valid_metadata||!slot.committed||slot.version!=observed_version {
   return Err(TrialFault::Policy(UpdatePolicyError::Stale))
  }
  let next=self.journal.generation.checked_add(1).ok_or(TrialFault::Invalid)?;
  self.journal.slots[selected].attempts_remaining=budget;
  self.journal.generation=next;
  self.active=selected;self.pending=None;self.attempted=false;Ok(())
 }
 pub fn fail(&mut self,selected:usize)->Result<usize,TrialFault>{
  if self.pending!=Some(selected){return Err(TrialFault::Missing)}
  let (rollback,backup)=rollback_after_failure(self.journal,selected,self.minimum_version)
   .map_err(TrialFault::Policy)?;
  self.journal=rollback;self.active=backup;self.pending=None;self.attempted=false;Ok(backup)
 }
 pub fn cancel(&mut self)->Result<(),TrialFault>{
  let target=self.pending.ok_or(TrialFault::Missing)?;
  let next=self.journal.generation.checked_add(1).ok_or(TrialFault::Invalid)?;
  self.journal.slots[target].attempts_remaining=0;
  self.journal.generation=next;self.pending=None;self.attempted=false;Ok(())
 }
}
#[cfg(test)]mod r008_ota_trial_tests{
 use super::*;
 fn journal()->Journal{
  Journal{generation:2,slots:[
   Slot{valid_metadata:true,committed:true,version:5,attempts_remaining:3},
   Slot{valid_metadata:false,committed:false,version:0,attempts_remaining:0}]}
 }
 #[test]fn trusted_update_attempt_confirms_and_switches_active(){
  let mut t=BootTrial::new(journal(),0,5).unwrap();
  assert_eq!(t.stage(6,true,3,true),Ok(1));
  assert_eq!(t.consume_attempt(1),Ok(2));
  t.healthy(1,6,3).unwrap();
  assert_eq!(t.active,1);assert_eq!(t.pending,None);
 }
 #[test]fn unhealthy_update_rolls_back_to_previously_working_slot(){
  let mut t=BootTrial::new(journal(),0,5).unwrap();
  t.stage(7,true,1,true).unwrap();
  assert_eq!(t.fail(1),Ok(0));
  assert_eq!(t.journal.slots[1].attempts_remaining,0);
  assert_eq!(t.active,0);
 }
 #[test]fn signature_power_and_identity_gates_are_enforced(){
  let mut t=BootTrial::new(journal(),0,5).unwrap();
  assert_eq!(t.stage(6,false,2,true),Err(TrialFault::Policy(UpdatePolicyError::Unverified)));
  assert_eq!(t.stage(6,true,2,false),Err(TrialFault::Invalid));
  t.stage(6,true,2,true).unwrap();
  assert_eq!(t.healthy(0,6,2),Err(TrialFault::Missing));
  assert_eq!(t.stage(7,true,2,true),Err(TrialFault::Pending));
  t.cancel().unwrap();assert_eq!(t.pending,None);
 }
 #[test]fn last_boot_attempt_may_still_be_confirmed_healthy(){
  let mut t=BootTrial::new(journal(),0,5).unwrap();
  t.stage(8,true,1,true).unwrap();
  assert_eq!(t.consume_attempt(1),Ok(0));
  assert_eq!(t.consume_attempt(1),Err(TrialFault::Invalid));
  t.healthy(1,8,3).unwrap();
  assert_eq!(t.active,1);
  assert_eq!(t.journal.slots[1].attempts_remaining,3);
 }
}


/// Decode both independently checksummed boot metadata records. Torn or
/// untrusted bytes are never interpreted as a bootable slot. This is only
/// a read-only decision: firmware verification and durable writes are external.
pub fn select_encoded_slots(records:&[[u8;SLOT_RECORD_BYTES];2],floor:u64)
    ->Result<(usize,Slot),BootError>
{
    let invalid=Slot{valid_metadata:false,committed:false,version:0,
        attempts_remaining:0};
    let slots=[decode_slot(&records[0]).unwrap_or(invalid),
               decode_slot(&records[1]).unwrap_or(invalid)];
    let selected=select_slot(&slots,floor)?;
    Ok((selected,slots[selected]))
}

/// Stage a CRC-updated record before mutating the in-memory mirror. The
/// privileged caller must persist + fsync new metadata before booting.
/// Interrupted physical writes are NOT handled by this codec.
pub fn consume_encoded_attempt(records:&mut [[u8;SLOT_RECORD_BYTES];2],
    floor:u64)->Result<usize,BootError>
{
    let (index,slot)=select_encoded_slots(records,floor)?;
    let mut next=slot;
    next.attempts_remaining-=1; // select_slot proves nonzero.
    let staged=encode_slot(next).ok_or(BootError::NoBootableSlot)?;
    records[index]=staged;
    Ok(index)
}

/// Count valid committed records at a specified anti-rollback floor.
/// Does not confer trust on an image or change firmware.
pub fn bootable_record_count(records:&[[u8;SLOT_RECORD_BYTES];2],
    floor:u64)->usize
{
    records.iter().filter_map(decode_slot)
        .filter(|slot|bootable(*slot,floor)).count()
}

#[cfg(test)]
mod r114_mirrored_boot_tests{
    use super::*;
    fn slot(v:u64,n:u8)->Slot{
        Slot{valid_metadata:true,committed:true,version:v,attempts_remaining:n}
    }
    #[test]
    fn checksum_damage_fails_over_to_last_valid_record(){
        let mut r=[encode_slot(slot(5,2)).unwrap(),encode_slot(slot(9,2)).unwrap()];
        r[1][18]=1;
        assert_eq!(select_encoded_slots(&r,5),Ok((0,slot(5,2))));
        assert_eq!(bootable_record_count(&r,5),1);
    }
    #[test]
    fn consumption_never_modifies_another_slot(){
        let mut r=[encode_slot(slot(5,2)).unwrap(),encode_slot(slot(9,1)).unwrap()];
        let old=r[0];
        assert_eq!(consume_encoded_attempt(&mut r,5),Ok(1));
        assert_eq!(r[0],old);
        assert_eq!(decode_slot(&r[1]),Some(slot(9,0)));
        assert_eq!(consume_encoded_attempt(&mut r,5),Ok(0));
        assert_eq!(decode_slot(&r[0]),Some(slot(5,1)));
    }
    #[test]
    fn anti_rollback_and_corrupt_pair_leave_bytes_untouched(){
        let mut r=[encode_slot(slot(1,2)).unwrap(),encode_slot(slot(3,2)).unwrap()];
        let before=r;
        assert_eq!(consume_encoded_attempt(&mut r,4),Err(BootError::NoBootableSlot));
        assert_eq!(r,before);
        r[0][0]=0;r[1][0]=0;
        let before=r;
        assert_eq!(consume_encoded_attempt(&mut r,0),Err(BootError::NoBootableSlot));
        assert_eq!(r,before);
    }
    #[test]
    fn uncommitted_or_exhausted_cannot_be_selected(){
        let mut pending=slot(11,2);pending.committed=false;
        let r=[encode_slot(pending).unwrap(),encode_slot(slot(10,0)).unwrap()];
        assert_eq!(bootable_record_count(&r,0),0);
        assert_eq!(select_encoded_slots(&r,0),Err(BootError::NoBootableSlot));
    }
}


// R114 follow-up: two-phase inactive-slot storage updates over the existing
// BlockDevice capability. Not wired to firmware/normal boot: the trusted boot
// coordinator must provide two reserved, distinct, non-user LBAs and authenticate
// any replacement image before publishing metadata.
pub const BOOT_SECTOR_BYTES:usize=crate::block::SECTOR_BYTES;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum SlotIoError { Device, Geometry, NoFallback, Downgrade, Verification }
fn sector_record(slot:Slot)->Result<[u8;BOOT_SECTOR_BYTES],SlotIoError> {
    let encoded=encode_slot(slot).ok_or(SlotIoError::Verification)?;
    let mut full=[0u8;BOOT_SECTOR_BYTES];
    full[..SLOT_RECORD_BYTES].copy_from_slice(&encoded);
    Ok(full)
}
fn checked_sector_pair<D:crate::block::BlockDevice>(device:&D,
    lbas:[u64;2])->Result<(),SlotIoError>{
    let capacity=device.sector_count();
    if lbas[0]==lbas[1]||lbas.iter().any(|&n|n>=capacity){
        return Err(SlotIoError::Geometry)
    }
    Ok(())
}
fn read_pair<D:crate::block::BlockDevice>(
    device:&mut D,lbas:[u64;2]
)->Result<[Option<Slot>;2],SlotIoError>{
    checked_sector_pair(device,lbas)?;
    let mut selected=[None;2];
    for i in 0..2 {
        let mut bytes=[0u8;BOOT_SECTOR_BYTES];
        device.read_sector(lbas[i],&mut bytes).map_err(|_|SlotIoError::Device)?;
        if !bytes[SLOT_RECORD_BYTES..].iter().all(|&b|b==0){continue;}
        let mut encoded=[0u8;SLOT_RECORD_BYTES];
        encoded.copy_from_slice(&bytes[..SLOT_RECORD_BYTES]);
        selected[i]=decode_slot(&encoded);
    }
    if checked_sector_pair(device,lbas).is_err(){return Err(SlotIoError::Device);}
    Ok(selected)
}
/// Read both records, rejecting any I/O fault instead of treating it as
/// corrupt-but-recoverable metadata. CRC damage to one record may fall back.
pub fn load_slot_pair<D:crate::block::BlockDevice>(
    device:&mut D,lbas:[u64;2],floor:u64
)->Result<(usize,Slot),SlotIoError>{
    let pair=read_pair(device,lbas)?;
    let invalid=Slot{valid_metadata:false,committed:false,version:0,
        attempts_remaining:0};
    let slots=[pair[0].unwrap_or(invalid),pair[1].unwrap_or(invalid)];
    let selected=select_slot(&slots,floor).map_err(|_|SlotIoError::NoFallback)?;
    Ok((selected,slots[selected]))
}
fn write_barrier_verify<D:crate::block::BlockDevice>(
    device:&mut D,lba:u64,record:&[u8;BOOT_SECTOR_BYTES])
    ->Result<(),SlotIoError>{
    device.write_sector(lba,record).map_err(|_|SlotIoError::Device)?;
    device.sync().map_err(|_|SlotIoError::Device)?;
    let mut observed=[0u8;BOOT_SECTOR_BYTES];
    device.read_sector(lba,&mut observed).map_err(|_|SlotIoError::Device)?;
    if observed!=*record{return Err(SlotIoError::Verification);}
    Ok(())
}
/// Preserve the selected old slot while staging the new slot as uncommitted.
/// Only after a successful flush+readback publish committed=true and flush
/// again. Failures leave the old slot untouched, though the damaged inactive
/// slot remains uncertain and requires readback/recovery on a new boot.
pub fn publish_inactive_slot<D:crate::block::BlockDevice>(
    device:&mut D,lbas:[u64;2],floor:u64,new:Slot
)->Result<usize,SlotIoError>{
    let (active,old)=load_slot_pair(device,lbas,floor)?;
    if !new.valid_metadata||!new.committed||new.attempts_remaining==0||
       new.version<=old.version||new.version<floor {
        return Err(SlotIoError::Downgrade)
    }
    let inactive=1-active;
    let mut staged=new;
    staged.committed=false;
    write_barrier_verify(device,lbas[inactive],&sector_record(staged)?)?;
    write_barrier_verify(device,lbas[inactive],&sector_record(new)?)?;
    Ok(inactive)
}
#[cfg(test)]
mod r114_two_phase_sector_recovery_tests {
    use super::*;
    use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
    struct Fake {
        sectors:[[u8;SECTOR_BYTES];4], writes:usize,flushes:usize,
        fail_write:usize,fail_flush:usize,
    }
    impl Fake {
        fn new()->Self{
            let mut disk=Self{sectors:[[0;SECTOR_BYTES];4],writes:0,
                flushes:0,fail_write:0,fail_flush:0};
            disk.sectors[1]=sector_record(Slot{valid_metadata:true,committed:true,
                version:7,attempts_remaining:3}).unwrap();
            disk
        }
    }
    impl BlockDevice for Fake{
        fn sector_count(&self)->u64{4}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self,lba:u64,input:&[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            self.writes+=1;
            let slot=self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?;
            if self.writes==self.fail_write {
                slot[..17].copy_from_slice(&input[..17]); // torn write
                return Err(BlockError::Io);
            }
            *slot=*input;Ok(())
        }
        fn sync(&mut self)->Result<(),BlockError>{
            self.flushes+=1;
            if self.flushes==self.fail_flush {Err(BlockError::Io)}
            else {Ok(())}
        }
    }
    fn next()->Slot {
        Slot{valid_metadata:true,committed:true,version:8,attempts_remaining:2}
    }
    #[test]
    fn successful_two_phase_update_selects_newer_after_remount(){
        let mut d=Fake::new();
        assert_eq!(publish_inactive_slot(&mut d,[1,2],7,next()),Ok(1));
        assert_eq!(d.writes,2);
        assert_eq!(d.flushes,2);
        assert_eq!(load_slot_pair(&mut d,[1,2],7),Ok((1,next())));
        assert_eq!(decode_slot(&d.sectors[1][..SLOT_RECORD_BYTES].try_into().unwrap()).unwrap().version,7);
    }
    #[test]
    fn torn_first_and_second_writes_preserve_previous_committed_slot(){
        for interrupted in [1,2] {
            let mut d=Fake::new();d.fail_write=interrupted;
            assert_eq!(publish_inactive_slot(&mut d,[1,2],7,next()),Err(SlotIoError::Device));
            assert_eq!(load_slot_pair(&mut d,[1,2],7).unwrap().1.version,7);
        }
    }
    #[test]
    fn flush_failure_and_downgrade_are_fail_closed(){
        for failed_flush in [1,2]{
            let mut d=Fake::new();d.fail_flush=failed_flush;
            assert_eq!(publish_inactive_slot(&mut d,[1,2],7,next()),Err(SlotIoError::Device));
            // Do not claim the inactive sector is durable after a failed flush.
            assert_eq!(d.sectors[1][..4],*b"IAB1");
        }
        let mut d=Fake::new();
        let before=d.sectors;
        assert_eq!(publish_inactive_slot(&mut d,[1,2],7,
             Slot{version:7,..next()}),Err(SlotIoError::Downgrade));
        assert_eq!(d.sectors,before);
    }
    #[test]
    fn corrupt_records_bad_geometry_and_read_only_device_reject(){
        let mut d=Fake::new();
        assert_eq!(load_slot_pair(&mut d,[1,1],7),Err(SlotIoError::Geometry));
        assert_eq!(publish_inactive_slot(&mut d,[1,5],7,next()),Err(SlotIoError::Geometry));
        d.sectors[1][6]^=0x40;
        assert_eq!(load_slot_pair(&mut d,[1,2],7),Err(SlotIoError::NoFallback));
    }
}


/// Read-only recovery selection against an exact checked physical-partition
/// receipt. Boot-slot metadata is separately stored in two reserved sectors
/// OUTSIDE the selected filesystem. The trusted installer must allocate them
/// and authenticate the image; this helper never writes, formats or loads it.
#[cfg(feature="test-r031-vfs")]
pub fn select_recovery_slot_readonly<D:crate::block::BlockDevice>(
    disk:&mut D,
    partition:crate::m1_partition::PartitionReceipt,
    slot_lbas:[u64;2],
    min_version:u64
)->Result<(usize,Slot),SlotIoError>{
    let start=partition.partition.start;
    let end=start.checked_add(partition.partition.sectors)
        .ok_or(SlotIoError::Geometry)?;
    let capacity=partition.device_sectors;
    // Exclude MBR/GPT primary and backup headers. Slot allocation remains
    // explicitly reserved by installer; this is not a sector allocator.
    if capacity<70 || slot_lbas[0]==slot_lbas[1] ||
       slot_lbas.iter().any(|&sector|sector<34||
          sector>=capacity.saturating_sub(34)||
          (sector>=start&&sector<end)) {
        return Err(SlotIoError::Geometry)
    }
    let mut readonly=crate::block::ReadOnlyDevice::new(disk);
    // Before reading either slot, verify exact partition identity, geometry,
    // metadata heads and data-bank health without granting any write.
    let health=crate::m1_partition::probe_selected_receipt_readonly(
        &mut readonly,partition).map_err(|_|SlotIoError::Verification)?;
    if health.partition!=partition.partition{return Err(SlotIoError::Verification)}
    let selected=load_slot_pair(&mut readonly,slot_lbas,min_version)?;
    // Check receipt again after reading metadata so an observed hot-swap or
    // changed GPT/MBR table cannot issue an apparently valid boot decision.
    crate::m1_partition::probe_selected_receipt_readonly(
        &mut readonly,partition).map_err(|_|SlotIoError::Verification)?;
    Ok(selected)
}


// R114: restore an authenticated prior image from verified reserved backup
// sectors into a distinct failed/inactive image region. The manifest is
// domain-separated and binds the exact source, destination, sector count,
// image version and SHA256 of the FULL source image. A real Ed25519 trust
// backend must be supplied by the caller. Bootloader handoff, slot activation,
// NVRAM, OVMF/physical firmware and reboot are NOT done here.
pub const RECOVERY_MAX_SECTORS:u16=64;
const RECOVERY_DOMAIN:&[u8;16]=b"IndianOS/RECV1\0\0";
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct SignedImageRecovery {
 pub version:u64,pub backup_lba:u64,pub restore_lba:u64,
 pub sectors:u16,pub digest:[u8;32],
}
impl SignedImageRecovery {
 pub fn manifest(&self)->[u8;74] {
  let mut data=[0u8;74];
  data[..16].copy_from_slice(RECOVERY_DOMAIN);
  data[16..24].copy_from_slice(&self.version.to_le_bytes());
  data[24..32].copy_from_slice(&self.backup_lba.to_le_bytes());
  data[32..40].copy_from_slice(&self.restore_lba.to_le_bytes());
  data[40..42].copy_from_slice(&self.sectors.to_le_bytes());
  data[42..74].copy_from_slice(&self.digest);
  data
 }
 fn valid_geometry(&self,total:u64,floor:u64)->bool{
  if self.version<floor||self.version==0||
     self.sectors==0||self.sectors>RECOVERY_MAX_SECTORS||
     self.digest==[0;32]||total<=68{return false;}
  let Some(a)=self.backup_lba.checked_add(u64::from(self.sectors)) else{return false;};
  let Some(b)=self.restore_lba.checked_add(u64::from(self.sectors)) else{return false;};
  self.backup_lba>=34&&self.restore_lba>=34&&
   a<=total-34&&b<=total-34&&
   (a<=self.restore_lba||b<=self.backup_lba)
 }
}
/// Actually copy authenticated backup sectors to an isolated restore region,
/// flushing and verifying each destination sector via SHA256 readback.
/// Reject all authentication/hash/geometry failures BEFORE ANY writes. A
/// failed write/flush or readback never publishes a bootable slot record.
/// The old backup region stays untouched even if restoring fails midway.
pub fn restore_prior_signed_image<D:crate::block::BlockDevice,
 V:crate::batch4_signature::Verify>(
 device:&mut D,image:SignedImageRecovery,floor:u64,
 roots:&[crate::batch4_signature::Root],verifier:&V,
 envelope:&crate::batch4_signature::Envelope,
)->Result<(),SlotIoError>{
 if !image.valid_geometry(device.sector_count(),floor){
  return Err(SlotIoError::Geometry);
 }
 let manifest=image.manifest();
 if envelope.digest!=crate::m2_sha256::digest(&manifest) ||
    crate::batch4_signature::check(roots,verifier,envelope,&manifest).is_err() {
  return Err(SlotIoError::Verification);
 }
 let mut hash=crate::m2_sha256::Stream::new();
 let mut buf=[0u8;BOOT_SECTOR_BYTES];
 for i in 0..u64::from(image.sectors){
  device.read_sector(image.backup_lba+i,&mut buf)
    .map_err(|_|SlotIoError::Device)?;
  hash.update(&buf).map_err(|_|SlotIoError::Verification)?;
 }
 if hash.finish()!=image.digest{return Err(SlotIoError::Verification);}
 // Only after verifying every backup byte and signature may any write occur.
 for i in 0..u64::from(image.sectors) {
  device.read_sector(image.backup_lba+i,&mut buf)
    .map_err(|_|SlotIoError::Device)?;
  device.write_sector(image.restore_lba+i,&buf)
    .map_err(|_|SlotIoError::Device)?;
 }
 device.sync().map_err(|_|SlotIoError::Device)?;
 // Verify that the backup source did not change between authentication
 // and the final restore write. A swapped/removable source may pass the
 // initial signature check, then change during the copy (TOCTOU).
 // In that case the inactive restore region is NEVER authorized for boot.
 let mut rechecked=crate::m2_sha256::Stream::new();
 for i in 0..u64::from(image.sectors){
  device.read_sector(image.backup_lba+i,&mut buf)
    .map_err(|_|SlotIoError::Device)?;
  rechecked.update(&buf).map_err(|_|SlotIoError::Verification)?;
 }
 if rechecked.finish()!=image.digest{return Err(SlotIoError::Verification);}
 let mut restored=crate::m2_sha256::Stream::new();
 for i in 0..u64::from(image.sectors){
  device.read_sector(image.restore_lba+i,&mut buf)
    .map_err(|_|SlotIoError::Device)?;
  restored.update(&buf).map_err(|_|SlotIoError::Verification)?;
 }
 if restored.finish()!=image.digest{return Err(SlotIoError::Verification);}
 Ok(())
}

#[cfg(test)]
mod r114_signed_system_image_restore_tests {
 use super::*;
 use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};
 use crate::batch4_signature::{Envelope,Root,Verify};
 struct TrustedTest;
 impl Verify for TrustedTest {
  fn verify_digest_and_signature(&self,e:&Envelope,payload:&[u8])->bool {
   // This is a host-test fixture, NOT a real publisher signature verifier.
   e.digest==crate::m2_sha256::digest(payload)&&e.signature==[7;64]
  }
 }
 struct Fake{
  sectors:[[u8;SECTOR_BYTES];96],writes:usize,fail_write:bool,
  corrupt_readback:bool,flushed:bool,tamper_backup_on_restore:bool,
 }
 impl Fake{
  fn new()->Self {
   let mut x=Self{sectors:[[0;SECTOR_BYTES];96],writes:0,
    fail_write:false,corrupt_readback:false,flushed:false,
    tamper_backup_on_restore:false};
   x.sectors[35].fill(0x33);x.sectors[36].fill(0x59);
   x
  }
  fn image(&self)->SignedImageRecovery {
   let mut h=crate::m2_sha256::Stream::new();
   h.update(&self.sectors[35]).unwrap();
   h.update(&self.sectors[36]).unwrap();
   SignedImageRecovery{version:8,backup_lba:35,restore_lba:45,
    sectors:2,digest:h.finish()}
  }
 }
 impl BlockDevice for Fake {
  fn sector_count(&self)->u64{self.sectors.len() as u64}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
      ->Result<(),BlockError>{
   *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
   if self.corrupt_readback&&self.flushed&&lba==45 {out[0]^=1;}
   Ok(())
  }
  fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
      ->Result<(),BlockError>{
   self.writes+=1;
   if self.fail_write{return Err(BlockError::Io);}
   let target=self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?;
   *target=*data;
   if self.tamper_backup_on_restore&&lba==45{
    self.sectors[36][0]^=0x5a;
   }
   Ok(())
  }
  fn sync(&mut self)->Result<(),BlockError>{self.flushed=true;Ok(())}
 }
 fn valid_envelope(job:SignedImageRecovery)->Envelope {
  Envelope{signer:3,epoch:4,
   digest:crate::m2_sha256::digest(&job.manifest()),signature:[7;64]}
 }
 fn roots()->[Root;1]{[Root{signer:3,epoch:4,enabled:true}]}
 #[test]
 fn authenticated_prior_image_restores_bytes_and_flushes() {
  let mut disk=Fake::new();let image=disk.image();
  let backup=[disk.sectors[35],disk.sectors[36]];
  assert_eq!(restore_prior_signed_image(&mut disk,image,7,&roots(),
      &TrustedTest,&valid_envelope(image)),Ok(()));
  assert_eq!(disk.writes,2);assert!(disk.flushed);
  assert_eq!(disk.sectors[45],backup[0]);assert_eq!(disk.sectors[46],backup[1]);
 }
 #[test]
 fn unsigned_corrupt_downgrade_and_overlap_cannot_write() {
  let mut d=Fake::new();let job=d.image();let mut signature=valid_envelope(job);
  signature.signature=[0;64];
  assert_eq!(restore_prior_signed_image(&mut d,job,7,&roots(),&TrustedTest,
      &signature),Err(SlotIoError::Verification));
  assert_eq!(restore_prior_signed_image(&mut d,job,9,&roots(),&TrustedTest,
      &valid_envelope(job)),Err(SlotIoError::Geometry));
  let overlap=SignedImageRecovery{restore_lba:35,..job};
  assert_eq!(restore_prior_signed_image(&mut d,overlap,7,&roots(),&TrustedTest,
      &valid_envelope(overlap)),Err(SlotIoError::Geometry));
  d.sectors[35][0]^=1;
  assert_eq!(restore_prior_signed_image(&mut d,job,7,&roots(),&TrustedTest,
      &valid_envelope(job)),Err(SlotIoError::Verification));
  assert_eq!(d.writes,0);
 }
 #[test]
 fn backup_media_swap_between_signature_check_and_copy_is_denied() {
  let mut d=Fake::new();let job=d.image();
  d.tamper_backup_on_restore=true;
  assert_eq!(restore_prior_signed_image(&mut d,job,7,&roots(),&TrustedTest,
      &valid_envelope(job)),Err(SlotIoError::Verification));
  assert_eq!(d.sectors[45][0],0x33);
  assert!(d.flushed);
  // No verified boot record or boot-slot publication takes place.
 }
 #[test]
 fn failed_write_or_corrupt_readback_denies_success_and_preserves_backup() {
  let mut d=Fake::new();let job=d.image();let original=d.sectors[35];
  d.fail_write=true;
  assert_eq!(restore_prior_signed_image(&mut d,job,7,&roots(),&TrustedTest,
      &valid_envelope(job)),Err(SlotIoError::Device));
  assert_eq!(d.sectors[35],original);
  d.fail_write=false;d.corrupt_readback=true;
  assert_eq!(restore_prior_signed_image(&mut d,job,7,&roots(),&TrustedTest,
      &valid_envelope(job)),Err(SlotIoError::Verification));
  assert_eq!(d.sectors[35],original);
 }
}
