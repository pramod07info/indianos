//! R114: two-copy persistent boot epoch and recovery-slot binding for a
//! separately authenticated, installer-reserved *two-sector* BlockDevice.
 //! This module does not discover/format a disk or fabricate a boot identity.
 //! A trusted, real boot source must supply the measured boot digest; host
 //! tests use an explicit fixture that is NOT a firmware attestation.
use crate::block::{BlockDevice,SECTOR_BYTES};
use crate::agent_08_boot_slot::Slot;

const MAGIC:&[u8;8]=b"IEPOCH01";
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error { Geometry, Invalid, Io, Exhausted, Stale, Untrusted }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct EpochRecord {
 pub epoch:u64,
 pub journal_generation:u64,
 pub selected_slot:u8,
 pub slot_version:u64,
 pub boot_measurement:[u8;32],
 pub preceding_digest:[u8;32],
}
pub trait TrustedBootMeasurement {
 /// The platform boot loader MUST cryptographically authenticate this digest
 /// against its pinned root before invoking the capability. A bare QEMU RTC
 /// timestamp, userspace PID or unverified command-line is not proof.
 fn read_verified(&self)->Result<[u8;32],Error>;
}
fn encode(r:EpochRecord)->Result<[u8;SECTOR_BYTES],Error>{
 if r.epoch==0||r.journal_generation==0||r.slot_version==0||
    r.selected_slot>1||r.boot_measurement==[0;32] {
  return Err(Error::Invalid)
 }
 let mut sector=[0u8;SECTOR_BYTES];
 sector[..8].copy_from_slice(MAGIC);
 sector[8..16].copy_from_slice(&r.epoch.to_le_bytes());
 sector[16..24].copy_from_slice(&r.journal_generation.to_le_bytes());
 sector[24]=r.selected_slot;
 sector[32..40].copy_from_slice(&r.slot_version.to_le_bytes());
 sector[40..72].copy_from_slice(&r.boot_measurement);
 sector[72..104].copy_from_slice(&r.preceding_digest);
 let checksum=crate::m2_sha256::digest(&sector[..104]);
 sector[104..136].copy_from_slice(&checksum);
 Ok(sector)
}
fn decode(sector:&[u8;SECTOR_BYTES])->Option<EpochRecord>{
 if &sector[..8]!=MAGIC||sector[25..32].iter().any(|&b|b!=0)||
    sector[136..].iter().any(|&b|b!=0)||
    sector[104..136]!=crate::m2_sha256::digest(&sector[..104]){
  return None
 }
 let r=EpochRecord{
  epoch:u64::from_le_bytes(sector[8..16].try_into().ok()?),
  journal_generation:u64::from_le_bytes(sector[16..24].try_into().ok()?),
  selected_slot:sector[24],
  slot_version:u64::from_le_bytes(sector[32..40].try_into().ok()?),
  boot_measurement:sector[40..72].try_into().ok()?,
  preceding_digest:sector[72..104].try_into().ok()?,
 };
 encode(r).ok().filter(|data|data==sector).map(|_|r)
}
fn checked<D:BlockDevice>(disk:&D)->Result<(),Error>{
 if disk.sector_count()!=2 {Err(Error::Geometry)}else{Ok(())}
}
fn recover<D:BlockDevice>(disk:&mut D)->Result<(EpochRecord,u64,[u8;SECTOR_BYTES]),Error>{
 checked(disk)?;
 let mut best:Option<(EpochRecord,u64,[u8;SECTOR_BYTES])>=None;
 let mut seen=[None;2];
 for sector in 0..2u64{
  let mut raw=[0u8;SECTOR_BYTES];
  disk.read_sector(sector,&mut raw).map_err(|_|Error::Io)?;
  if let Some(current)=decode(&raw){
   seen[sector as usize]=Some((current,raw));
   if let Some((prior,_,_))=best.as_ref(){
    if current.epoch==prior.epoch&&current!=*prior{return Err(Error::Invalid);}
    if current.epoch<prior.epoch{continue;}
   }
   best=Some((current,sector,raw));
  }
 }
 if let (Some((a,ab)),Some((b,bb)))=(seen[0],seen[1]){
  if a.epoch!=b.epoch{
   let (newer,older,prior)=if a.epoch>b.epoch{(a,b,&bb)}else{(b,a,&ab)};
   if older.epoch.checked_add(1)!=Some(newer.epoch)||
      newer.preceding_digest!=crate::m2_sha256::digest(prior){
    return Err(Error::Invalid);
   }
  }
 }
 best.ok_or(Error::Invalid)
}
/// Pure read-only observation of the current retained epoch, used by a
/// kernel-only boot coordinator before attempting one fresh boot record.
/// The value is not itself boot authorization; compare the authenticated
/// measurement and slot through recovery_receipt after publication.
pub fn current_epoch<D:BlockDevice>(disk:&mut D)->Result<u64,Error>{
 Ok(recover(disk)?.0.epoch)
}
/// Reads without changing the durable journal. Caller may only use this as a
/// boot recovery authorization after checking an actual trusted boot source.
pub fn recovery_receipt<D:BlockDevice,T:TrustedBootMeasurement>(
 disk:&mut D,trust:&T,expected_epoch:u64,selected:usize,
 slot:Slot,observed_journal_generation:u64
)->Result<EpochRecord,Error>{
 let measured=trust.read_verified()?;
 if measured==[0;32]{return Err(Error::Untrusted);}
 let (r,_,_)=recover(disk)?;
 if r.epoch!=expected_epoch||r.journal_generation!=observed_journal_generation||
    r.selected_slot as usize!=selected||r.slot_version!=slot.version||
    !slot.committed||!slot.valid_metadata||slot.attempts_remaining==0||
    r.boot_measurement!=measured{return Err(Error::Stale);}
 Ok(r)
}
/// Updates the already provisioned TWO-COPY journal for a newly observed
/// boot. Never initializes an unknown disk; does not publish a new boot slot.
/// The caller must be a trusted boot coordinator with an exclusive selected
/// two-sector partition and must call this once per actual boot. Readback and
/// sync prevent silently publishing an unacknowledged epoch.
fn advance_epoch_expected<D:BlockDevice,T:TrustedBootMeasurement>(
 disk:&mut D,trust:&T,current_epoch:u64,
 selected:usize,slot:Slot,journal_generation:u64,
 expected_prior:Option<EpochRecord>,
)->Result<EpochRecord,Error>{
 let measurement=trust.read_verified()?;
 if measurement==[0;32] {return Err(Error::Untrusted);}
 let (old,active,before)=recover(disk)?;
 // The guarded path verified a specific physical journal snapshot. A device
 // swap or concurrent writer changing the same epoch's signed metadata
 // between admission and writing must be denied without publishing anything.
 if expected_prior.is_some_and(|prior|prior!=old){return Err(Error::Stale);}
 if old.epoch!=current_epoch||selected>1||!slot.committed||
    !slot.valid_metadata||slot.attempts_remaining==0||
    journal_generation==0||journal_generation<old.journal_generation {
  return Err(Error::Stale);
 }
 let next=EpochRecord{
  epoch:old.epoch.checked_add(1).ok_or(Error::Exhausted)?,
  journal_generation,selected_slot:selected as u8,
  slot_version:slot.version,boot_measurement:measurement,
  preceding_digest:crate::m2_sha256::digest(&before),
 };
 let raw=encode(next)?;
 let target=1-active;
 disk.write_sector(target,&raw).map_err(|_|Error::Io)?;
 disk.sync().map_err(|_|Error::Io)?;
 let mut observed=[0u8;SECTOR_BYTES];
 disk.read_sector(target,&mut observed).map_err(|_|Error::Io)?;
 if observed!=raw {return Err(Error::Io);}
 // A media swap mid-update is not a valid boot journal.
 disk.read_sector(active,&mut observed).map_err(|_|Error::Io)?;
 if observed!=before{return Err(Error::Stale);}
 Ok(next)
}
/// Preserve the unguarded legacy contract for existing signed recovery
/// callers. Production policy must use advance_epoch_guarded instead.
pub fn advance_epoch<D:BlockDevice,T:TrustedBootMeasurement>(
 disk:&mut D,trust:&T,current_epoch:u64,
 selected:usize,slot:Slot,journal_generation:u64
)->Result<EpochRecord,Error>{
 advance_epoch_expected(disk,trust,current_epoch,selected,slot,journal_generation,None)
}
#[cfg(test)]
mod tests {
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 pub(super) struct Trusted(pub(super) [u8;32]);
 impl TrustedBootMeasurement for Trusted {
  fn read_verified(&self)->Result<[u8;32],Error>{Ok(self.0)}
 }
 pub(super) struct Disk {
  pub(super) sectors:[[u8;SECTOR_BYTES];2],pub(super) writes:usize,pub(super) flushed:bool,
  fail_sync:bool,damage:bool,
 }
 impl BlockDevice for Disk {
  fn sector_count(&self)->u64{2}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
   Ok(())
  }
  fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   let target=self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?;
   *target=*data;self.writes+=1;
   if self.damage{target[40]^=1;}
   Ok(())
  }
  fn sync(&mut self)->Result<(),BlockError>{
   if self.fail_sync{return Err(BlockError::Io);}
   self.flushed=true;Ok(())
  }
 }
 pub(super) fn slot()->Slot{Slot{valid_metadata:true,committed:true,version:9,attempts_remaining:2}}
 pub(super) fn fixture()->Disk{
  let r=EpochRecord{epoch:4,journal_generation:20,selected_slot:0,slot_version:9,
    boot_measurement:[3;32],preceding_digest:[2;32]};
  let bytes=encode(r).unwrap();
  Disk{sectors:[bytes,bytes],writes:0,flushed:false,fail_sync:false,damage:false}
 }
 #[test]fn exactly_one_next_boot_epoch_is_flushed_read_back_and_bound(){
  let mut d=fixture();let source=Trusted([5;32]);
  let next=advance_epoch(&mut d,&source,4,0,slot(),21).unwrap();
  assert_eq!(next.epoch,5);assert_eq!(next.journal_generation,21);
  assert_eq!(d.writes,1);assert!(d.flushed);
  assert_eq!(recovery_receipt(&mut d,&source,5,0,slot(),21),Ok(next));
  assert_eq!(recovery_receipt(&mut d,&source,4,0,slot(),21),Err(Error::Stale));
  assert_eq!(recovery_receipt(&mut d,&Trusted([3;32]),5,0,slot(),21),
      Err(Error::Stale));
  assert_eq!(recovery_receipt(&mut d,&source,5,1,slot(),21),Err(Error::Stale));
 }
 #[test]fn mismatched_or_untrusted_boot_never_mutates_disk(){
  let mut d=fixture();
  for (epoch,measure) in [(3,[5;32]),(4,[0;32])]{
   assert!(advance_epoch(&mut d,&Trusted(measure),epoch,0,slot(),21).is_err());
  }
  assert_eq!(d.writes,0);
  let mut truncated=Disk{sectors:[[0;SECTOR_BYTES];2],writes:0,
     flushed:false,fail_sync:false,damage:false};
  assert_eq!(advance_epoch(&mut truncated,&Trusted([5;32]),4,0,slot(),21),
     Err(Error::Invalid));
  assert_eq!(truncated.writes,0);
 }
 #[test]fn torn_copy_and_write_readback_fault_do_not_hide_last_valid_record(){
  let mut d=fixture();d.damage=true;
  assert_eq!(advance_epoch(&mut d,&Trusted([5;32]),4,0,slot(),21),Err(Error::Io));
  assert_eq!(recover(&mut d).unwrap().0.epoch,4);
  let mut d=fixture();d.fail_sync=true;
  assert_eq!(advance_epoch(&mut d,&Trusted([5;32]),4,0,slot(),21),Err(Error::Io));
  assert_eq!(recover(&mut d).unwrap().0.epoch,5);
  // The on-disk candidate may have reached the device despite a failed sync.
  // Never assume a failed flush means durability was proven.
 }
 #[test]fn forged_predecessor_or_skipped_epoch_is_refused(){
  let mut d=fixture();
  let mut newer=decode(&d.sectors[1]).unwrap();
  newer.epoch=5;newer.preceding_digest=[99;32];
  d.sectors[1]=encode(newer).unwrap();
  assert_eq!(recover(&mut d),Err(Error::Invalid));
  newer.epoch=10;
  newer.preceding_digest=crate::m2_sha256::digest(&d.sectors[0]);
  d.sectors[1]=encode(newer).unwrap();
  assert_eq!(recover(&mut d),Err(Error::Invalid));
  assert_eq!(d.writes,0);
 }
 #[test]fn divergent_same_epoch_is_denied_and_does_not_modify_media(){
  let mut d=fixture();
  let mut second=decode(&d.sectors[1]).unwrap();
  second.boot_measurement=[8;32];
  d.sectors[1]=encode(second).unwrap();
  assert_eq!(recover(&mut d),Err(Error::Invalid));
  assert_eq!(d.writes,0);
 }
}



/// R115: read-only, cryptographically sourced epoch observation. A verified
/// measurement must match the newest valid on-disk epoch and its two-sector
/// predecessor chain. This is NOT TPM attestation or bootloader integration.
pub fn observe_verified_epoch<D: BlockDevice, T: TrustedBootMeasurement>(
    disk: &mut D, trusted: &T,
) -> Result<EpochRecord, Error> {
    let measurement = trusted.read_verified()?;
    if measurement == [0; 32] { return Err(Error::Untrusted); }
    let (latest, _, _) = recover(disk)?;
    if latest.boot_measurement != measurement { return Err(Error::Stale); }
    Ok(latest)
}

#[cfg(test)]
mod r115_boot_observation_tests {
    use super::*;

    #[test]
    fn verified_observation_does_not_mutate_media() {
        let mut d = tests::fixture();
        let record = observe_verified_epoch(&mut d, &tests::Trusted([3; 32]))
            .unwrap();
        assert_eq!(record.epoch, 4);
        assert_eq!(record.journal_generation, 20);
        assert_eq!(record.selected_slot, 0);
        assert_eq!(record.slot_version, 9);
        assert_eq!(record.boot_measurement, [3; 32]);
        assert_eq!(d.writes, 0);
        assert!(!d.flushed);
    }

    #[test]
    fn mismatched_and_zero_measurements_never_authorize() {
        let mut d = tests::fixture();
        assert_eq!(observe_verified_epoch(&mut d, &tests::Trusted([4; 32])),
            Err(Error::Stale));
        assert_eq!(observe_verified_epoch(&mut d, &tests::Trusted([0; 32])),
            Err(Error::Untrusted));
        assert_eq!(d.writes, 0);
    }

    #[test]
    fn forged_chain_and_divergent_copies_are_rejected() {
        let mut d = tests::fixture();
        let mut second = decode(&d.sectors[1]).unwrap();
        second.epoch += 1;
        second.preceding_digest = [7; 32];
        d.sectors[1] = encode(second).unwrap();
        assert_eq!(observe_verified_epoch(&mut d, &tests::Trusted([3; 32])),
            Err(Error::Invalid));
        assert_eq!(d.writes, 0);
        let mut d = tests::fixture();
        let mut second = decode(&d.sectors[1]).unwrap();
        second.slot_version += 1;
        d.sectors[1] = encode(second).unwrap();
        assert_eq!(observe_verified_epoch(&mut d, &tests::Trusted([3; 32])),
            Err(Error::Invalid));
        assert_eq!(d.writes, 0);
    }

    #[test]
    fn valid_next_epoch_becomes_observable_without_extra_writes() {
        let mut d = tests::fixture();
        let next = advance_epoch(&mut d, &tests::Trusted([5; 32]),
            4, 0, tests::slot(), 21).unwrap();
        let writes = d.writes;
        assert_eq!(observe_verified_epoch(&mut d, &tests::Trusted([5; 32])),
            Ok(next));
        assert_eq!(d.writes, writes);
        assert_eq!(observe_verified_epoch(&mut d, &tests::Trusted([3; 32])),
            Err(Error::Stale));
    }
}


/// R116: a previously sealed minimum epoch and signed slot version may
/// restrict a new trusted boot observation. The caller must obtain these
/// floors from authenticated rollback-resistant storage, not the journal
/// that is being examined. A host fixture is NOT a production trust root.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct SealedBootFloor {
 pub minimum_epoch:u64,
 pub minimum_journal_generation:u64,
 pub expected_slot:u8,
 pub minimum_slot_version:u64,
 pub expected_measurement:[u8;32],
}
pub fn observe_with_sealed_floor<D:BlockDevice,T:TrustedBootMeasurement>(
 disk:&mut D,trust:&T,floor:SealedBootFloor,
)->Result<EpochRecord,Error>{
 if floor.minimum_epoch==0||floor.minimum_journal_generation==0||
    floor.minimum_slot_version==0||floor.expected_slot>1||
    floor.expected_measurement==[0;32]{return Err(Error::Invalid)}
 let measurement=trust.read_verified()?;
 if measurement==[0;32]{return Err(Error::Untrusted);}
 let (r,_,_)=recover(disk)?;
 if r.boot_measurement!=measurement{return Err(Error::Stale);}
 if r.epoch<floor.minimum_epoch||
    r.journal_generation<floor.minimum_journal_generation||
    r.slot_version<floor.minimum_slot_version||
    r.selected_slot!=floor.expected_slot||
    r.boot_measurement!=floor.expected_measurement{
  return Err(Error::Stale)
 }
 Ok(r)
}
#[cfg(test)]
mod r116_rollback_floor_tests{
 use super::*;
 fn floor()->SealedBootFloor {
  SealedBootFloor{minimum_epoch:4,minimum_journal_generation:20,
   expected_slot:0,minimum_slot_version:9,expected_measurement:[3;32]}
 }
 #[test]fn valid_sealed_floor_does_not_touch_disk(){
  let mut disk=tests::fixture();let before=disk.writes;
  let record=observe_with_sealed_floor(&mut disk,&tests::Trusted([3;32]),
     floor()).unwrap();
  assert_eq!(record.epoch,4);
  assert_eq!(disk.writes,before);
 }
 #[test]fn rollback_floor_mismatch_is_never_authority(){
  let mut disk=tests::fixture();
  let p=floor();
  for bad in [
   SealedBootFloor{minimum_epoch:5,..p},
   SealedBootFloor{minimum_journal_generation:21,..p},
   SealedBootFloor{expected_slot:1,..p},
   SealedBootFloor{minimum_slot_version:10,..p},
   SealedBootFloor{expected_measurement:[4;32],..p},
  ]{
   assert_eq!(observe_with_sealed_floor(
       &mut disk,&tests::Trusted([3;32]),bad),Err(Error::Stale));
  }
  assert_eq!(disk.writes,0);
 }
 #[test]fn zero_floors_and_untrusted_measurement_fail_closed(){
  let mut disk=tests::fixture();let p=floor();
  for bad in [
   SealedBootFloor{minimum_epoch:0,..p},
   SealedBootFloor{minimum_journal_generation:0,..p},
   SealedBootFloor{minimum_slot_version:0,..p},
   SealedBootFloor{expected_slot:2,..p},
   SealedBootFloor{expected_measurement:[0;32],..p},
  ]{
   assert_eq!(observe_with_sealed_floor(
       &mut disk,&tests::Trusted([3;32]),bad),Err(Error::Invalid));
  }
  assert_eq!(observe_with_sealed_floor(
       &mut disk,&tests::Trusted([0;32]),p),Err(Error::Untrusted));
  assert_eq!(disk.writes,0);
 }
 #[test]fn forged_digest_and_divergent_epoch_copies_fail(){
  let mut disk=tests::fixture();
  let mut forged=decode(&disk.sectors[1]).unwrap();
  forged.epoch+=1;forged.preceding_digest=[9;32];
  disk.sectors[1]=encode(forged).unwrap();
  assert_eq!(observe_with_sealed_floor(
      &mut disk,&tests::Trusted([3;32]),floor()),Err(Error::Invalid));
  assert_eq!(disk.writes,0);
  let mut disk=tests::fixture();
  let mut divergent=decode(&disk.sectors[1]).unwrap();
  divergent.slot_version=10;
  disk.sectors[1]=encode(divergent).unwrap();
  assert_eq!(observe_with_sealed_floor(
      &mut disk,&tests::Trusted([3;32]),floor()),Err(Error::Invalid));
 }
}


struct MeasurementSnapshot([u8;32]);
impl TrustedBootMeasurement for MeasurementSnapshot {
 fn read_verified(&self)->Result<[u8;32],Error>{Ok(self.0)}
}
/// R117: before advancing a durable boot epoch, require an independently
/// provided sealed floor for the *current* journal. A trusted measurement
/// mismatch, rollback or invalid signed slot must fail before any mutation.
/// This API does not itself provision the physical trust anchor.
pub fn advance_epoch_guarded<D:BlockDevice,T:TrustedBootMeasurement>(
 disk:&mut D,trust:&T,old_epoch:u64,
 selected:usize,slot:Slot,journal_generation:u64,
 floor:SealedBootFloor,
)->Result<EpochRecord,Error>{
 // Snapshot platform-authenticated authority exactly once; a changing
 // observer cannot authorize one journal and then publish a different one.
 let measurement=trust.read_verified()?;
 if measurement==[0;32]{return Err(Error::Untrusted);}
 let verified=MeasurementSnapshot(measurement);
 let prior=observe_with_sealed_floor(disk,&verified,floor)?;
 if prior.epoch!=old_epoch || prior.selected_slot as usize!=selected ||
    prior.slot_version!=slot.version ||
    journal_generation<floor.minimum_journal_generation {
    return Err(Error::Stale)
 }
 advance_epoch_expected(disk,&verified,old_epoch,selected,slot,
    journal_generation,Some(prior))
}
#[cfg(test)]
mod r117_guarded_advance_tests{
 use super::*;
 fn floor()->SealedBootFloor{
  SealedBootFloor{minimum_epoch:4,minimum_journal_generation:20,
    expected_slot:0,minimum_slot_version:9,
    expected_measurement:[3;32]}
 }
 #[test]fn correct_seal_allows_one_durable_next_epoch(){
  let mut d=tests::fixture();
  let next=advance_epoch_guarded(&mut d,&tests::Trusted([3;32]),
    4,0,tests::slot(),21,floor()).unwrap();
  assert_eq!(next.epoch,5);assert_eq!(d.writes,1);
 }
 #[test]fn independently_sealed_floor_refuses_journal_rollback(){
  let mut d=tests::fixture();
  for f in [
    SealedBootFloor{minimum_epoch:5,..floor()},
    SealedBootFloor{minimum_journal_generation:25,..floor()},
    SealedBootFloor{minimum_slot_version:10,..floor()},
    SealedBootFloor{expected_slot:1,..floor()},
    SealedBootFloor{expected_measurement:[5;32],..floor()},
  ]{
   assert_eq!(advance_epoch_guarded(&mut d,&tests::Trusted([3;32]),
     4,0,tests::slot(),21,f),Err(Error::Stale));
  }
  assert_eq!(d.writes,0);
 }
 #[test]fn forged_epoch_or_wrong_slot_never_changes_disk(){
  let mut d=tests::fixture();
  assert_eq!(advance_epoch_guarded(&mut d,&tests::Trusted([3;32]),
    3,0,tests::slot(),21,floor()),Err(Error::Stale));
  assert_eq!(advance_epoch_guarded(&mut d,&tests::Trusted([3;32]),
    4,1,tests::slot(),21,floor()),Err(Error::Stale));
  assert_eq!(advance_epoch_guarded(&mut d,&tests::Trusted([3;32]),
    4,0,tests::slot(),19,floor()),Err(Error::Stale));
  assert_eq!(d.writes,0);
 }
 #[test]fn absent_trusted_measurement_cannot_advance(){
  let mut d=tests::fixture();
  assert_eq!(advance_epoch_guarded(&mut d,&tests::Trusted([0;32]),
    4,0,tests::slot(),21,floor()),Err(Error::Untrusted));
  assert_eq!(d.writes,0);
 }
}


#[cfg(test)]
mod r118_boot_measurement_snapshot_tests {
 use super::*;
 use core::cell::Cell;
 struct ChangingObserver {reads:Cell<usize>,first:[u8;32]}
 impl TrustedBootMeasurement for ChangingObserver {
  fn read_verified(&self)->Result<[u8;32],Error>{
   let count=self.reads.get();
   self.reads.set(count+1);
   if count==0 {Ok(self.first)} else {Ok([0x77;32])}
  }
 }
 fn floor()->SealedBootFloor{
  SealedBootFloor{minimum_epoch:4,minimum_journal_generation:20,
   expected_slot:0,minimum_slot_version:9,expected_measurement:[3;32]}
 }
 #[test]
 fn changing_platform_source_is_read_exactly_once_for_guarded_commit(){
  let mut disk=tests::fixture();
  let source=ChangingObserver{reads:Cell::new(0),first:[3;32]};
  let next=advance_epoch_guarded(&mut disk,&source,4,0,tests::slot(),21,floor()).unwrap();
  assert_eq!(source.reads.get(),1);
  assert_eq!(next.epoch,5);
  assert_eq!(next.boot_measurement,[3;32]);
  assert_eq!(disk.writes,1);
 }
 #[test]
 fn mismatched_first_platform_snapshot_cannot_mutate_disk(){
  let mut disk=tests::fixture();
  let source=ChangingObserver{reads:Cell::new(0),first:[4;32]};
  assert_eq!(advance_epoch_guarded(&mut disk,&source,4,0,tests::slot(),21,floor()),
   Err(Error::Stale));
  assert_eq!(source.reads.get(),1);
  assert_eq!(disk.writes,0);
 }
}

#[cfg(test)]
mod r118_epoch_media_swap_tests {
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 struct SwapOnSecondObservation{
  inner:tests::Disk,
  reads:usize,
  alternate:[u8;SECTOR_BYTES],
 }
 impl BlockDevice for SwapOnSecondObservation{
  fn sector_count(&self)->u64{2}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
      ->Result<(),BlockError>{
   self.reads+=1;
   if self.reads==3{
    self.inner.sectors=[self.alternate;2];
   }
   self.inner.read_sector(lba,out)
  }
  fn write_sector(&mut self,lba:u64,value:&[u8;SECTOR_BYTES])
      ->Result<(),BlockError>{self.inner.write_sector(lba,value)}
  fn sync(&mut self)->Result<(),BlockError>{self.inner.sync()}
 }
 #[test]
 fn same_epoch_different_signed_journal_after_guard_is_not_written(){
  let initial=tests::fixture();
  let mut changed=decode(&initial.sectors[0]).unwrap();
  changed.journal_generation=22; // same epoch and boot measurement
  let mut disk=SwapOnSecondObservation{
   inner:initial,reads:0,alternate:encode(changed).unwrap(),
  };
  let floor=SealedBootFloor{minimum_epoch:4,minimum_journal_generation:20,
   expected_slot:0,minimum_slot_version:9,expected_measurement:[3;32]};
  assert_eq!(advance_epoch_guarded(&mut disk,&tests::Trusted([3;32]),
   4,0,tests::slot(),23,floor),Err(Error::Stale));
  assert_eq!(disk.inner.writes,0);
  assert_eq!(disk.reads,4);
 }
}

#[cfg(test)]
mod r118_sealed_floor_and_recovery_matrix_tests {
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 fn floor()->SealedBootFloor{
  SealedBootFloor{minimum_epoch:4,minimum_journal_generation:20,
      expected_slot:0,minimum_slot_version:9,expected_measurement:[3;32]}
 }
 #[test]
 fn every_signed_floor_and_slot_gate_denies_before_media_mutation(){
  let mut disk=tests::fixture();
  let valid=tests::slot();
  let baseline=floor();
  for rejected in [
   SealedBootFloor{minimum_epoch:5,..baseline},
   SealedBootFloor{minimum_journal_generation:21,..baseline},
   SealedBootFloor{minimum_slot_version:10,..baseline},
   SealedBootFloor{expected_slot:1,..baseline},
   SealedBootFloor{expected_measurement:[8;32],..baseline}
  ]{
   assert_eq!(advance_epoch_guarded(&mut disk,&tests::Trusted([3;32]),
       4,0,valid,21,rejected),Err(Error::Stale));
  }
  for invalid in [
   Slot{committed:false,..valid},
   Slot{valid_metadata:false,..valid},
   Slot{attempts_remaining:0,..valid}
  ]{
   assert_eq!(advance_epoch_guarded(&mut disk,&tests::Trusted([3;32]),
       4,0,invalid,21,baseline),Err(Error::Stale));
  }
  assert_eq!(advance_epoch_guarded(&mut disk,&tests::Trusted([3;32]),
      4,1,valid,21,baseline),Err(Error::Stale));
  assert_eq!(advance_epoch_guarded(&mut disk,&tests::Trusted([3;32]),
      3,0,valid,21,baseline),Err(Error::Stale));
  assert_eq!(advance_epoch_guarded(&mut disk,&tests::Trusted([3;32]),
      4,0,valid,19,baseline),Err(Error::Stale));
  assert_eq!(disk.writes,0);
 }
 #[test]
 fn forged_predecessor_digest_and_divergent_same_epoch_cannot_advance(){
  let mut disk=tests::fixture();
  let mut forged=decode(&disk.sectors[1]).unwrap();
  forged.epoch=5;
  forged.preceding_digest=[0x99;32];
  disk.sectors[1]=encode(forged).unwrap();
  assert_eq!(advance_epoch_guarded(&mut disk,&tests::Trusted([3;32]),
      4,0,tests::slot(),21,floor()),Err(Error::Invalid));
  assert_eq!(disk.writes,0);
  let mut disk=tests::fixture();
  let mut divergent=decode(&disk.sectors[1]).unwrap();
  divergent.slot_version+=1;
  disk.sectors[1]=encode(divergent).unwrap();
  assert_eq!(advance_epoch_guarded(&mut disk,&tests::Trusted([3;32]),
      4,0,tests::slot(),21,floor()),Err(Error::Invalid));
  assert_eq!(disk.writes,0);
 }
 #[test]
 fn torn_newest_slot_never_bypasses_sealed_minimum_epoch(){
  let mut disk=tests::fixture();
  let new=advance_epoch_guarded(&mut disk,&tests::Trusted([3;32]),
      4,0,tests::slot(),21,floor()).unwrap();
  assert_eq!(new.epoch,5);
  assert_eq!(disk.writes,1);
  // The new commit occupies sector 0, because original sector 1 was the
  // active copy. Simulate an interrupted storage write corrupting its CRC.
  disk.sectors[0][40]^=0x80;
  assert!(matches!(observe_with_sealed_floor(
    &mut disk,&tests::Trusted([3;32]),
    SealedBootFloor{minimum_epoch:5,minimum_journal_generation:21,..floor()}),
    Err(Error::Stale)|Err(Error::Invalid)));
 }
 struct FailFlush{inner:tests::Disk}
 impl BlockDevice for FailFlush {
  fn sector_count(&self)->u64{self.inner.sector_count()}
  fn read_sector(&mut self,lba:u64,output:&mut[u8;SECTOR_BYTES])
       ->Result<(),BlockError>{self.inner.read_sector(lba,output)}
  fn write_sector(&mut self,lba:u64,input:&[u8;SECTOR_BYTES])
       ->Result<(),BlockError>{self.inner.write_sector(lba,input)}
  fn sync(&mut self)->Result<(),BlockError>{Err(BlockError::Io)}
 }
 #[test]
 fn unsuccessful_durable_flush_never_returns_commit_receipt(){
  let mut disk=FailFlush{inner:tests::fixture()};
  assert_eq!(advance_epoch_guarded(&mut disk,&tests::Trusted([3;32]),
      4,0,tests::slot(),21,floor()),Err(Error::Io));
  assert_eq!(disk.inner.writes,1);
  // A write which reached the device but failed flush is not proof of
  // durability, even when one in-memory sector now looks newest.
 }
}

#[cfg(test)]
mod r118_power_interruption_fault_matrix {
    use super::*;
    use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};

    /// Disposable two-sector in-memory media; these are NEVER real power
    /// cuts or claims about storage-controller caches, FUA, or firmware.
    struct InterruptedMedia {
        disk:tests::Disk,
        mode:u8,
        reads:usize,
        writes:usize,
        flushes:usize,
    }
    impl InterruptedMedia {
        fn new(mode:u8)->Self {
            Self{disk:tests::fixture(),mode,reads:0,writes:0,flushes:0}
        }
    }
    impl BlockDevice for InterruptedMedia {
        fn sector_count(&self)->u64{2}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            self.reads+=1;
            // Guarded advance reads 2 sectors twice, then readback of new
            // and old heads; reject completion if readback is lost.
            if self.mode==5 && self.writes>0 && self.reads>=5 {
                return Err(BlockError::Io);
            }
            self.disk.read_sector(lba,out)
        }
        fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            self.writes+=1;
            if self.mode==1 {return Err(BlockError::Io);}
            if self.mode==2 {
                // Simulate a controller reporting failure after only 64 bytes.
                self.disk.sectors[lba as usize][..64]
                    .copy_from_slice(&bytes[..64]);
                return Err(BlockError::Io);
            }
            self.disk.write_sector(lba,bytes)?;
            if self.mode==3{return Err(BlockError::Io);}
            Ok(())
        }
        fn sync(&mut self)->Result<(),BlockError>{
            self.flushes+=1;
            if self.mode==4 {return Err(BlockError::Io);}
            self.disk.sync()
        }
    }
    fn old_floor()->SealedBootFloor {
        SealedBootFloor{
            minimum_epoch:4,minimum_journal_generation:20,
            expected_slot:0,minimum_slot_version:9,
            expected_measurement:[3;32],
        }
    }
    #[test]
    fn every_interrupted_write_flush_or_readback_denies_commit_receipt(){
        for failure in 1u8..=5 {
            let mut media=InterruptedMedia::new(failure);
            assert_eq!(
                advance_epoch_guarded(&mut media,&tests::Trusted([3;32]),
                    4,0,tests::slot(),21,old_floor()),
                Err(Error::Io),"fault phase {failure}"
            );
            assert_eq!(media.writes,1,"fault phase {failure}");
            // The *old* record lives in the untouched second sector.
            // Neither failure may destroy the predecessor recovery copy.
            assert!(decode(&media.disk.sectors[1]).is_some());
            let snapshot=media.disk.sectors;
            // No further writes occurred during rejected publish.
            assert_eq!(media.disk.sectors,snapshot);
        }
    }
    #[test]
    fn torn_recovery_head_cannot_cross_a_sealed_epoch_floor(){
        for damaged_bytes in [1usize,16,64,104,136,512] {
            let mut media=InterruptedMedia::new(0);
            let committed=advance_epoch_guarded(&mut media,&tests::Trusted([3;32]),
                4,0,tests::slot(),21,old_floor()).unwrap();
            assert_eq!(committed.epoch,5);
            let floor5=SealedBootFloor{
                minimum_epoch:5,minimum_journal_generation:21,..old_floor()
            };
            // Model incomplete sector replacement following a new attempt:
            // a corruption of the new head cannot regress into an older
            // (otherwise valid) sector under the sealed hardware floor.
            for byte in 0..damaged_bytes {
                media.disk.sectors[0][byte]^=0xa5;
            }
            let result=observe_with_sealed_floor(
                &mut media.disk,&tests::Trusted([3;32]),floor5);
            assert!(matches!(result,Err(Error::Stale)|Err(Error::Invalid)),
                "torn length {damaged_bytes}");
        }
    }
}

/// R118-075/076: hardware-attested PID1 launch contract. The platform
/// implementor MUST source (1) the measurement from authenticated firmware,
/// (2) the sealed epoch floor from rollback-resistant hardware and
/// (3) the selected signed boot slot from a bootloader-verified policy.
/// None of the three may be supplied by PID1, disk metadata, a QEMU test
/// constant, a kernel command line or an ordinary writable EFI variable.
///
/// A platform that cannot supply all three CANNOT authorize PID1; this API
/// does not pretend the absent UEFI/TPM driver is already implemented.
pub trait AuthenticatedPid1Platform:TrustedBootMeasurement {
    fn read_sealed_floor(&self)->Result<SealedBootFloor,Error>;
    fn read_bootloader_selected_slot(&self)->Result<(usize,Slot),Error>;
}

/// Private fields mean an arbitrary raw epoch record or userland-supplied
/// byte string is not itself a verified launch receipt.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct VerifiedPid1Admission {
    epoch:EpochRecord,
    selected:usize,
}
impl VerifiedPid1Admission {
    pub fn epoch(&self)->u64{self.epoch.epoch}
    pub fn journal_generation(&self)->u64{self.epoch.journal_generation}
    pub fn slot_version(&self)->u64{self.epoch.slot_version}
    pub fn selected_slot(&self)->usize{self.selected}
}

/// Read-only fail-closed gate immediately before launching PID1. Observe
/// both durable journal copies, then recheck all hardware authority and the
/// media. A trust-floor change, signer/slot swap, torn journal or power-cut
/// read error must never return a boot receipt.
///
/// The privileged caller must serialize the verified boot source and the
/// selected block device across this function and the PID1 launch. This
/// remains a software contract until its real UEFI/TPM provider exists.
pub fn authorize_verified_pid1<D:BlockDevice,P:AuthenticatedPid1Platform>(
    disk:&mut D,platform:&P
)->Result<VerifiedPid1Admission,Error>{
    let floor=platform.read_sealed_floor()?;
    let (selected,slot)=platform.read_bootloader_selected_slot()?;
    if selected>1||selected!=floor.expected_slot as usize||
       slot.version==0||!slot.valid_metadata||!slot.committed||
       slot.attempts_remaining==0{
        return Err(Error::Untrusted);
    }
    // Exactly one authenticated measurement is used for the two journal
    // observations; a second hardware read checks that its authority did
    // not change while storage was being accessed.
    let measurement=platform.read_verified()?;
    if measurement==[0;32]||measurement==[0xff;32]{
        return Err(Error::Untrusted);
    }
    let frozen=MeasurementSnapshot(measurement);
    let observed=observe_with_sealed_floor(disk,&frozen,floor)?;
    if observed.selected_slot as usize!=selected ||
       observed.slot_version!=slot.version{
        return Err(Error::Stale);
    }
    if platform.read_sealed_floor()?!=floor ||
       platform.read_bootloader_selected_slot()?!=(selected,slot) ||
       platform.read_verified()?!=measurement{
        return Err(Error::Stale);
    }
    if observe_with_sealed_floor(disk,&frozen,floor)?!=observed{
        return Err(Error::Stale);
    }
    Ok(VerifiedPid1Admission{epoch:observed,selected})
}

#[cfg(test)]
mod r118_authenticated_pid1_handoff_tests {
    use super::*;
    use std::cell::Cell;
    use crate::block::{BlockDevice,BlockError,SECTOR_BYTES};

    fn floor()->SealedBootFloor{
        SealedBootFloor{minimum_epoch:4,minimum_journal_generation:20,
            expected_slot:0,minimum_slot_version:9,
            expected_measurement:[3;32]}
    }
    struct Platform{
        digest:[u8;32],
        floor:SealedBootFloor,
        selected:usize,
        slot:Slot,
        reads:Cell<usize>,
        mutate_on_second:bool,
    }
    impl Platform {
        fn new()->Self{Self{digest:[3;32],floor:floor(),selected:0,
            slot:tests::slot(),reads:Cell::new(0),
            mutate_on_second:false}}
    }
    impl TrustedBootMeasurement for Platform {
        fn read_verified(&self)->Result<[u8;32],Error>{
            Ok(self.digest)
        }
    }
    impl AuthenticatedPid1Platform for Platform {
        fn read_sealed_floor(&self)->Result<SealedBootFloor,Error>{
            let next=self.reads.get()+1;
            self.reads.set(next);
            if self.mutate_on_second&&next==2 {
                return Ok(SealedBootFloor{
                    minimum_epoch:5,..self.floor});
            }
            Ok(self.floor)
        }
        fn read_bootloader_selected_slot(&self)->Result<(usize,Slot),Error>{
            Ok((self.selected,self.slot))
        }
    }
    #[test]
    fn preprovisioned_valid_journal_and_stable_hardware_authorize_once(){
        let mut disk=tests::fixture();
        let p=Platform::new();
        let token=authorize_verified_pid1(&mut disk,&p).unwrap();
        assert_eq!(token.epoch(),4);
        assert_eq!(token.journal_generation(),20);
        assert_eq!(token.slot_version(),9);
        assert_eq!(token.selected_slot(),0);
        assert_eq!(disk.writes,0);
        assert_eq!(p.reads.get(),2);
    }
    #[test]
    fn no_trusted_measurement_or_uncommitted_bootloader_slot_is_denied(){
        let mut disk=tests::fixture();
        let mut p=Platform::new();
        p.digest=[0;32];
        assert_eq!(authorize_verified_pid1(&mut disk,&p),Err(Error::Untrusted));
        p.digest=[0xff;32];
        assert_eq!(authorize_verified_pid1(&mut disk,&p),Err(Error::Untrusted));
        p.digest=[3;32];
        p.slot.committed=false;
        assert_eq!(authorize_verified_pid1(&mut disk,&p),Err(Error::Untrusted));
        p.slot.committed=true;p.slot.attempts_remaining=0;
        assert_eq!(authorize_verified_pid1(&mut disk,&p),Err(Error::Untrusted));
        p.slot.attempts_remaining=1;p.selected=1;
        assert_eq!(authorize_verified_pid1(&mut disk,&p),Err(Error::Untrusted));
        p.selected=0;p.slot.version=10;
        assert_eq!(authorize_verified_pid1(&mut disk,&p),Err(Error::Stale));
        assert_eq!(disk.writes,0);
    }
    #[test]
    fn independently_sealed_floor_change_during_two_reads_is_denied(){
        let mut disk=tests::fixture();
        let mut p=Platform::new();p.mutate_on_second=true;
        assert_eq!(authorize_verified_pid1(&mut disk,&p),Err(Error::Stale));
        assert_eq!(disk.writes,0);
    }

    struct MediaSwap{
        disk:tests::Disk,read_count:usize,
        changed:[u8;SECTOR_BYTES],
    }
    impl BlockDevice for MediaSwap {
        fn sector_count(&self)->u64{2}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            self.read_count+=1;
            if self.read_count==3 {
                self.disk.sectors=[self.changed;2];
            }
            self.disk.read_sector(lba,out)
        }
        fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{self.disk.write_sector(lba,bytes)}
        fn sync(&mut self)->Result<(),BlockError>{self.disk.sync()}
    }
    #[test]
    fn journal_swapped_between_attested_observations_is_denied(){
        let original=tests::fixture();
        let mut media=MediaSwap{changed:original.sectors[0],
            disk:original,read_count:0};
        // Same authenticated boot digest but different on-disk epoch/journal.
        // Construct from a separate trusted fixture, not a shell-provided
        // boot authority. The second observation detects the replacement.
        let mut changed=decode(&media.changed).unwrap();
        changed.epoch=5;
        changed.journal_generation=21;
        media.changed=encode(changed).unwrap();
        assert_eq!(authorize_verified_pid1(&mut media,&Platform::new()),
            Err(Error::Stale));
        assert_eq!(media.disk.writes,0);
    }
}


/// G8 verified UEFI boot extension: the firmware implementation must check
/// enforced Secure Boot, authenticated boot image and protected TPM/ROM roots.
/// No implementation of this trait exists for ordinary production boot;
/// caller-provided flags are NEVER accepted as firmware attestation.
pub trait AuthenticatedSecureUefi:AuthenticatedPid1Platform{
 fn require_firmware_secure_boot_chain(&self)->Result<(),Error>;
}
pub fn authorize_secure_uefi_pid1<D:BlockDevice,P:AuthenticatedSecureUefi>(
 disk:&mut D,platform:&P)->Result<VerifiedPid1Admission,Error>{
 platform.require_firmware_secure_boot_chain()?;
 let admitted=authorize_verified_pid1(disk,platform)?;
 // Firmware and boot source may change during a disk observation.
 platform.require_firmware_secure_boot_chain()?;
 Ok(admitted)
}
#[cfg(test)]
mod m1_batch05_w4_uefi_tests{
 use super::*;
 struct Unavailable{secure:bool}
 impl TrustedBootMeasurement for Unavailable{
  fn read_verified(&self)->Result<[u8;32],Error>{Err(Error::Untrusted)}
 }
 impl AuthenticatedPid1Platform for Unavailable{
  fn read_sealed_floor(&self)->Result<SealedBootFloor,Error>{Err(Error::Untrusted)}
  fn read_bootloader_selected_slot(&self)->Result<(usize,Slot),Error>{
   Err(Error::Untrusted)
  }
 }
 impl AuthenticatedSecureUefi for Unavailable{
  fn require_firmware_secure_boot_chain(&self)->Result<(),Error>{
   if self.secure{Ok(())}else{Err(Error::Untrusted)}
  }
 }
 struct Unreadable;
 impl BlockDevice for Unreadable{
  fn sector_count(&self)->u64{0}
  fn read_sector(&mut self,_:u64,_:&mut[u8;SECTOR_BYTES])->Result<(),crate::block::BlockError>{
   panic!("untrusted boot must fail before device I/O")
  }
  fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])->Result<(),crate::block::BlockError>{
   panic!("authorization never writes")
  }
 }
 #[test]fn no_firmware_secure_boot_and_missing_tpm_never_authorize_pid1(){
  let mut disk=Unreadable;
  assert_eq!(authorize_secure_uefi_pid1(&mut disk,&Unavailable{secure:false}),
   Err(Error::Untrusted));
  assert_eq!(authorize_secure_uefi_pid1(&mut disk,&Unavailable{secure:true}),
   Err(Error::Untrusted));
 }
}


/// P0 W6: independently re-check the full signed-slot, protected floor and
/// persistent two-copy epoch immediately before PID1's final handoff.
/// Returns no new authority. The trusted boot coordinator MUST serialize
/// the disk and hardware token between this check and executable launch.
pub fn revalidate_verified_pid1_at_launch<D:BlockDevice,P:AuthenticatedPid1Platform>(
 disk:&mut D,platform:&P,original:VerifiedPid1Admission
)->Result<(),Error>{
 let fresh=authorize_verified_pid1(disk,platform)?;
 if fresh!=original{return Err(Error::Stale);}
 Ok(())
}
#[cfg(test)]
mod kernel_p0_w6_boot_handoff_tests{
 use super::*;
 struct Platform{measurement:[u8;32],floor:SealedBootFloor,slot:Slot}
 impl TrustedBootMeasurement for Platform{
  fn read_verified(&self)->Result<[u8;32],Error>{Ok(self.measurement)}
 }
 impl AuthenticatedPid1Platform for Platform{
  fn read_sealed_floor(&self)->Result<SealedBootFloor,Error>{Ok(self.floor)}
  fn read_bootloader_selected_slot(&self)->Result<(usize,Slot),Error>{
   Ok((0,self.slot))
  }
 }
 fn platform()->Platform{
  Platform{
   measurement:[3;32],
   floor:SealedBootFloor{minimum_epoch:4,minimum_journal_generation:20,
     expected_slot:0,minimum_slot_version:9,expected_measurement:[3;32]},
   slot:tests::slot(),
  }
 }
 #[test]fn launch_receipt_is_invalidated_by_new_trusted_root_or_slot_revision(){
  let mut disk=tests::fixture();
  let mut p=platform();
  let original=authorize_verified_pid1(&mut disk,&p).unwrap();
  assert_eq!(revalidate_verified_pid1_at_launch(&mut disk,&p,original),Ok(()));
  p.measurement=[7;32];
  assert_eq!(revalidate_verified_pid1_at_launch(&mut disk,&p,original),
    Err(Error::Stale));
  p.measurement=[3;32];
  p.slot.version+=1;
  assert_eq!(revalidate_verified_pid1_at_launch(&mut disk,&p,original),
    Err(Error::Stale));
 }
}


/// P0 follow-up W6: volatile, single-use launch authorization in the
/// privileged boot coordinator. Persistent rollback resistance still comes
/// from the external sealed-floor provider; this does NOT program TPM/UEFI.
pub struct BootLaunchGate{last_epoch:u64,last_journal:u64}
impl BootLaunchGate{
 pub const fn new()->Self{Self{last_epoch:0,last_journal:0}}
 /// Check trusted roots, signed selection and both COW boot journal copies
 /// before issuing one in-memory PID1 launch receipt for a newer boot epoch.
 pub fn authorize_once<D:BlockDevice,P:AuthenticatedPid1Platform>(
  &mut self,disk:&mut D,platform:&P)->Result<VerifiedPid1Admission,Error>{
  let accepted=authorize_verified_pid1(disk,platform)?;
  if accepted.epoch()<=self.last_epoch||
     accepted.journal_generation()<self.last_journal{
   return Err(Error::Stale);
  }
  revalidate_verified_pid1_at_launch(disk,platform,accepted)?;
  self.last_epoch=accepted.epoch();
  self.last_journal=accepted.journal_generation();
  Ok(accepted)
 }
 pub fn last_admitted_epoch(&self)->u64{self.last_epoch}
}
#[cfg(test)]
mod kernel_p0_next_w6_boot_replay_tests{
 use super::*;
 struct Platform;
 impl TrustedBootMeasurement for Platform{
  fn read_verified(&self)->Result<[u8;32],Error>{Ok([3;32])}
 }
 impl AuthenticatedPid1Platform for Platform{
  fn read_sealed_floor(&self)->Result<SealedBootFloor,Error>{
   Ok(SealedBootFloor{minimum_epoch:4,minimum_journal_generation:20,
    expected_slot:0,minimum_slot_version:9,expected_measurement:[3;32]})
  }
  fn read_bootloader_selected_slot(&self)->Result<(usize,Slot),Error>{
   Ok((0,tests::slot()))
  }
 }
 #[test]fn duplicated_pid1_launch_from_same_boot_journal_fails_closed(){
  let mut disk=tests::fixture();let mut gate=BootLaunchGate::new();
  assert_eq!(gate.authorize_once(&mut disk,&Platform).unwrap().epoch(),4);
  assert_eq!(gate.last_admitted_epoch(),4);
  assert_eq!(gate.authorize_once(&mut disk,&Platform),Err(Error::Stale));
  assert_eq!(gate.last_admitted_epoch(),4);
  assert_eq!(disk.writes,0);
 }
 #[test]fn untrusted_boot_does_not_consume_one_shot_gate(){
  let mut disk=tests::fixture();let mut gate=BootLaunchGate::new();
  struct Untrusted;
  impl TrustedBootMeasurement for Untrusted{
   fn read_verified(&self)->Result<[u8;32],Error>{Err(Error::Untrusted)}
  }
  impl AuthenticatedPid1Platform for Untrusted{
   fn read_sealed_floor(&self)->Result<SealedBootFloor,Error>{Err(Error::Untrusted)}
   fn read_bootloader_selected_slot(&self)->Result<(usize,Slot),Error>{Err(Error::Untrusted)}
  }
  assert_eq!(gate.authorize_once(&mut disk,&Untrusted),Err(Error::Untrusted));
  assert_eq!(gate.last_admitted_epoch(),0);
  assert_eq!(gate.authorize_once(&mut disk,&Platform).unwrap().epoch(),4);
 }
}


/// P0 batch three W6: the verified handoff callback is the only point
/// where a trusted coordinator can launch PID1. The one-shot record is
/// consumed BEFORE invoking the potentially side-effectful callback so a
/// failed/partial launch cannot replay the same boot epoch.
impl BootLaunchGate{
 pub fn launch_once<D:BlockDevice,P:AuthenticatedPid1Platform,R>(
  &mut self,disk:&mut D,platform:&P,
  launch:impl FnOnce(VerifiedPid1Admission)->Result<R,Error>)
   ->Result<R,Error>{
  let receipt=self.authorize_once(disk,platform)?;
  launch(receipt)
 }
}
#[cfg(test)]
mod kernel_p0_batch3_w6_pid1_launch_tests{
 use super::*;
 struct Platform;
 impl TrustedBootMeasurement for Platform{
  fn read_verified(&self)->Result<[u8;32],Error>{Ok([3;32])}
 }
 impl AuthenticatedPid1Platform for Platform{
  fn read_sealed_floor(&self)->Result<SealedBootFloor,Error>{
   Ok(SealedBootFloor{minimum_epoch:4,minimum_journal_generation:20,
     expected_slot:0,minimum_slot_version:9,expected_measurement:[3;32]})
  }
  fn read_bootloader_selected_slot(&self)->Result<(usize,Slot),Error>{
   Ok((0,tests::slot()))
  }
 }
 #[test]fn pid1_callback_runs_only_once_for_one_authenticated_epoch(){
  let mut d=tests::fixture();let mut gate=BootLaunchGate::new();
  let mut calls=0u32;
  assert_eq!(gate.launch_once(&mut d,&Platform,|receipt|{
   calls+=1;Ok(receipt.epoch())
  }),Ok(4));
  assert_eq!(gate.launch_once(&mut d,&Platform,|_|{
   calls+=1;Ok(5)
  }),Err(Error::Stale));
  assert_eq!(calls,1);
  assert_eq!(gate.last_admitted_epoch(),4);
 }
 #[test]fn failed_launch_still_consumes_receipt_to_prevent_double_start(){
  let mut d=tests::fixture();let mut gate=BootLaunchGate::new();
  let mut calls=0;
  assert_eq!(gate.launch_once(&mut d,&Platform,|_|{
   calls+=1;Err::<(),Error>(Error::Io)
  }),Err(Error::Io));
  assert_eq!(gate.launch_once(&mut d,&Platform,|_|{
   calls+=1;Ok(())
  }),Err(Error::Stale));
  assert_eq!(calls,1);
 }
}

/// P0-78..80: after consuming the trusted one-shot PID1 authorization,
/// make a final independent measurement/slot/floor/journal observation.
/// A late trust failure blocks callback and cannot resurrect launch token.
impl BootLaunchGate{
 pub fn launch_with_last_chance_recheck<D:BlockDevice,P:AuthenticatedPid1Platform,R>(
  &mut self,disk:&mut D,platform:&P,
  launch:impl FnOnce(VerifiedPid1Admission)->Result<R,Error>)
  ->Result<R,Error>{
  let receipt=self.authorize_once(disk,platform)?;
  revalidate_verified_pid1_at_launch(disk,platform,receipt)?;
  launch(receipt)
 }
}
#[cfg(test)]
mod kernel_p0_batch4_w6_final_boot_tests{
 use super::*;
 use std::cell::Cell;
 struct Platform{reads:Cell<usize>,change_at:usize}
 impl TrustedBootMeasurement for Platform{
  fn read_verified(&self)->Result<[u8;32],Error>{Ok([3;32])}
 }
 impl AuthenticatedPid1Platform for Platform{
  fn read_sealed_floor(&self)->Result<SealedBootFloor,Error>{
   let count=self.reads.get()+1;self.reads.set(count);
   Ok(SealedBootFloor{minimum_epoch:if count>=self.change_at{5}else{4},
    minimum_journal_generation:20,expected_slot:0,
    minimum_slot_version:9,expected_measurement:[3;32]})
  }
  fn read_bootloader_selected_slot(&self)->Result<(usize,Slot),Error>{
   Ok((0,tests::slot()))
  }
 }
 #[test]fn one_time_accepted_launch_denies_replay(){
  let mut disk=tests::fixture();let mut gate=BootLaunchGate::new();
  let p=Platform{reads:Cell::new(0),change_at:usize::MAX};
  assert_eq!(gate.launch_with_last_chance_recheck(&mut disk,&p,
   |receipt|Ok(receipt.epoch())),Ok(4));
  assert_eq!(gate.launch_with_last_chance_recheck(&mut disk,&p,|_|Ok(5)),
   Err(Error::Stale));
 }
 #[test]fn late_changed_floor_blocks_pid1_and_consumes_launch_receipt(){
  let mut disk=tests::fixture();let mut gate=BootLaunchGate::new();
  let p=Platform{reads:Cell::new(0),change_at:5};
  let mut called=false;
  assert_eq!(gate.launch_with_last_chance_recheck(&mut disk,&p,
   |_|{called=true;Ok(())}),Err(Error::Stale));
  assert!(!called);assert_eq!(gate.last_admitted_epoch(),4);
  assert_eq!(gate.launch_with_last_chance_recheck(&mut disk,&p,
   |_|{called=true;Ok(())}),Err(Error::Stale));
  assert!(!called);
 }
}

/// P0-98..100: enforce trusted Secure Boot state both before and after
/// consuming a single-use PID1 launch authorization, and at final handoff.
/// The platform trait MUST be backed by a real authenticated firmware/TPM
/// provider before this is a production secure-boot guarantee.
impl BootLaunchGate{
 pub fn launch_secure_uefi_checked<D:BlockDevice,P:AuthenticatedSecureUefi,R>(
  &mut self,disk:&mut D,platform:&P,
  launch:impl FnOnce(VerifiedPid1Admission)->Result<R,Error>)
  ->Result<R,Error>{
  platform.require_firmware_secure_boot_chain()?;
  let receipt=self.authorize_once(disk,platform)?;
  platform.require_firmware_secure_boot_chain()?;
  revalidate_verified_pid1_at_launch(disk,platform,receipt)?;
  platform.require_firmware_secure_boot_chain()?;
  launch(receipt)
 }
}
#[cfg(test)]
mod kernel_p0_batch5_w6_secure_boot_handoff_tests{
 use super::*;
 use std::cell::Cell;
 struct Platform{checks:Cell<usize>,reject_on:usize}
 impl TrustedBootMeasurement for Platform{
  fn read_verified(&self)->Result<[u8;32],Error>{Ok([3;32])}
 }
 impl AuthenticatedPid1Platform for Platform{
  fn read_sealed_floor(&self)->Result<SealedBootFloor,Error>{
   Ok(SealedBootFloor{minimum_epoch:4,minimum_journal_generation:20,
    expected_slot:0,minimum_slot_version:9,expected_measurement:[3;32]})
  }
  fn read_bootloader_selected_slot(&self)->Result<(usize,Slot),Error>{
   Ok((0,tests::slot()))
  }
 }
 impl AuthenticatedSecureUefi for Platform{
  fn require_firmware_secure_boot_chain(&self)->Result<(),Error>{
   let n=self.checks.get()+1;self.checks.set(n);
   if n==self.reject_on{Err(Error::Untrusted)}else{Ok(())}
  }
 }
 #[test] fn secure_chain_is_checked_three_times_before_exact_pid1_launch(){
  let mut disk=tests::fixture();let mut gate=BootLaunchGate::new();
  let p=Platform{checks:Cell::new(0),reject_on:usize::MAX};
  assert_eq!(gate.launch_secure_uefi_checked(&mut disk,&p,
   |receipt|Ok(receipt.epoch())),Ok(4));
  assert_eq!(p.checks.get(),3);
  assert_eq!(gate.launch_secure_uefi_checked(&mut disk,&p,|_|Ok(5)),
   Err(Error::Stale));
 }
 #[test] fn late_firmware_downgrade_denies_callback_and_consumes_receipt(){
  let mut disk=tests::fixture();let mut gate=BootLaunchGate::new();
  let p=Platform{checks:Cell::new(0),reject_on:2};
  let mut called=false;
  assert_eq!(gate.launch_secure_uefi_checked(&mut disk,&p,|_|{
   called=true;Ok(())
  }),Err(Error::Untrusted));
  assert!(!called);assert_eq!(gate.last_admitted_epoch(),4);
  assert_eq!(gate.launch_secure_uefi_checked(&mut disk,&p,|_|{
   called=true;Ok(())
  }),Err(Error::Stale));
  assert!(!called);
 }
}
/// P0-118..120: bind the one-shot authenticated firmware handoff to an
/// image hash supplied by an independently verified signed boot manifest.
/// The image verifier MUST hash actual immutable executable bytes under the
/// same trusted boot lock. This is not a TPM provider or live ELF loader.
/// A missing/incorrect image verification consumes the one-shot receipt.
impl BootLaunchGate{
 pub fn launch_secure_uefi_manifest_image<D:BlockDevice,P:AuthenticatedSecureUefi,R>(
  &mut self,disk:&mut D,platform:&P,expected_image:[u8;32],
  verify_image:impl FnOnce(VerifiedPid1Admission)->Result<[u8;32],Error>,
  launch:impl FnOnce(VerifiedPid1Admission)->Result<R,Error>
 )->Result<R,Error>{
  self.launch_secure_uefi_checked(disk,platform,|receipt|{
   let found=verify_image(receipt)?;
   if expected_image==[0;32]||found!=expected_image{
    return Err(Error::Untrusted);
   }
   launch(receipt)
  })
 }
}
#[cfg(test)]
mod kernel_p0_batch6_w6_image_manifest_tests{
 use super::*;
 struct Platform;
 impl TrustedBootMeasurement for Platform{
  fn read_verified(&self)->Result<[u8;32],Error>{Ok([3;32])}
 }
 impl AuthenticatedPid1Platform for Platform{
  fn read_sealed_floor(&self)->Result<SealedBootFloor,Error>{
   Ok(SealedBootFloor{minimum_epoch:4,minimum_journal_generation:20,
    expected_slot:0,minimum_slot_version:9,expected_measurement:[3;32]})
  }
  fn read_bootloader_selected_slot(&self)->Result<(usize,Slot),Error>{
   Ok((0,tests::slot()))
  }
 }
 impl AuthenticatedSecureUefi for Platform{
  fn require_firmware_secure_boot_chain(&self)->Result<(),Error>{Ok(())}
 }
 #[test] fn signed_manifest_digest_is_checked_before_single_pid1_callback(){
  let mut disk=tests::fixture();let mut gate=BootLaunchGate::new();
  let mut starts=0;
  assert_eq!(gate.launch_secure_uefi_manifest_image(&mut disk,&Platform,
   [7;32],|r|{assert_eq!(r.epoch(),4);Ok([7;32])},
   |_|{starts+=1;Ok(10u8)}),Ok(10));
  assert_eq!(starts,1);
  assert_eq!(gate.launch_secure_uefi_manifest_image(&mut disk,&Platform,
   [7;32],|_|Ok([7;32]),|_|{starts+=1;Ok(12u8)}),Err(Error::Stale));
  assert_eq!(starts,1);
 }
 #[test] fn failed_image_verification_consumes_receipt_without_launch(){
  let mut disk=tests::fixture();let mut gate=BootLaunchGate::new();
  let mut starts=0;
  assert_eq!(gate.launch_secure_uefi_manifest_image(&mut disk,&Platform,
   [7;32],|_|Ok([9;32]),|_|{starts+=1;Ok(())}),Err(Error::Untrusted));
  assert_eq!(starts,0);assert_eq!(gate.last_admitted_epoch(),4);
  assert_eq!(gate.launch_secure_uefi_manifest_image(&mut disk,&Platform,
   [7;32],|_|Ok([7;32]),|_|{starts+=1;Ok(())}),Err(Error::Stale));
  assert_eq!(starts,0);
 }
}


/// P0-138..140: hash the actual caller-pinned PID1 image bytes inside the
/// consumed trusted admission, rather than taking an unrelated hash callback.
/// Validate bounded image and initial stack geometry before launch. The
/// trusted loader still must pin immutable executable pages and prove ELF
/// segment permissions; this is NOT itself a production TPM/UEFI provider.
impl BootLaunchGate{
 pub fn launch_secure_uefi_pinned_bytes<D:BlockDevice,P:AuthenticatedSecureUefi,R>(
  &mut self,disk:&mut D,platform:&P,manifest_digest:[u8;32],
  image:&[u8],entry_offset:usize,stack_pages:usize,
  launch:impl FnOnce(VerifiedPid1Admission)->Result<R,Error>
 )->Result<R,Error>{
  self.launch_secure_uefi_manifest_image(disk,platform,manifest_digest,
   |_|{
    if image.is_empty()||image.len()>16*1024*1024||
       entry_offset>=image.len()||!(2..=256).contains(&stack_pages)||
       stack_pages.checked_mul(4096).is_none(){
      return Err(Error::Invalid);
    }
    Ok(crate::m2_sha256::digest(image))
   },launch)
 }
}
#[cfg(test)]
mod kernel_p0_batch7_w6_pinned_image_tests{
 use super::*;
 struct Platform;
 impl TrustedBootMeasurement for Platform{
  fn read_verified(&self)->Result<[u8;32],Error>{Ok([3;32])}
 }
 impl AuthenticatedPid1Platform for Platform{
  fn read_sealed_floor(&self)->Result<SealedBootFloor,Error>{
   Ok(SealedBootFloor{minimum_epoch:4,minimum_journal_generation:20,
    expected_slot:0,minimum_slot_version:9,expected_measurement:[3;32]})
  }
  fn read_bootloader_selected_slot(&self)->Result<(usize,Slot),Error>{
   Ok((0,tests::slot()))
  }
 }
 impl AuthenticatedSecureUefi for Platform{
  fn require_firmware_secure_boot_chain(&self)->Result<(),Error>{Ok(())}
 }
 #[test]fn real_image_bytes_hash_is_bound_to_one_use_admission(){
  let mut disk=tests::fixture();let mut gate=BootLaunchGate::new();
  let image=b"pinned-test-image";
  let expected=crate::m2_sha256::digest(image);
  let mut launched=0;
  assert_eq!(gate.launch_secure_uefi_pinned_bytes(&mut disk,&Platform,
   expected,image,0,2,|receipt|{
    launched+=1;Ok(receipt.epoch())
   }),Ok(4));
  assert_eq!(launched,1);
  assert_eq!(gate.launch_secure_uefi_pinned_bytes(&mut disk,&Platform,
   expected,image,0,2,|_|{launched+=1;Ok(5)}),Err(Error::Stale));
  assert_eq!(launched,1);
 }
 #[test]fn mismatch_and_invalid_image_geometry_fail_closed_and_burn_receipt(){
  let bytes=b"pinned-test-image";let correct=crate::m2_sha256::digest(bytes);
  for (digest,entry,stack,error) in [
   ([9;32],0,2,Error::Untrusted),
   (correct,bytes.len(),2,Error::Invalid),
   (correct,0,1,Error::Invalid)
  ]{
   let mut disk=tests::fixture();let mut gate=BootLaunchGate::new();
   let mut called=false;
   assert_eq!(gate.launch_secure_uefi_pinned_bytes(&mut disk,&Platform,
    digest,bytes,entry,stack,|_|{called=true;Ok(())}),Err(error));
   assert!(!called);assert_eq!(gate.last_admitted_epoch(),4);
   assert_eq!(gate.launch_secure_uefi_pinned_bytes(&mut disk,&Platform,
    correct,bytes,0,2,|_|{called=true;Ok(())}),Err(Error::Stale));
   assert!(!called);
  }
 }
}


/// P0-158..160: bounded, no-heap ELF64 x86-64 ET_EXEC preflight for pinned
/// PID1 image bytes. Reject ELF interpreters/dynamic loaders, W+X mappings,
/// executable stack, overlaps, bad offsets, and entry outside file-backed
/// executable bytes. This does NOT create page tables or map ELF segments.
pub fn verify_pid1_elf64(image:&[u8])->Result<u64,Error>{
 fn u16le(bytes:&[u8],at:usize)->Result<u16,Error>{
  let s=bytes.get(at..at.checked_add(2).ok_or(Error::Invalid)?)
   .ok_or(Error::Invalid)?;
  Ok(u16::from_le_bytes([s[0],s[1]]))
 }
 fn u32le(bytes:&[u8],at:usize)->Result<u32,Error>{
  let s=bytes.get(at..at.checked_add(4).ok_or(Error::Invalid)?)
   .ok_or(Error::Invalid)?;
  Ok(u32::from_le_bytes([s[0],s[1],s[2],s[3]]))
 }
 fn u64le(bytes:&[u8],at:usize)->Result<u64,Error>{
  let s=bytes.get(at..at.checked_add(8).ok_or(Error::Invalid)?)
   .ok_or(Error::Invalid)?;
  Ok(u64::from_le_bytes([s[0],s[1],s[2],s[3],s[4],s[5],s[6],s[7]]))
 }
 if image.len()<64||image.len()>16*1024*1024||
    image.get(..4)!=Some(&b"\x7fELF"[..])||
    image[4]!=2||image[5]!=1||image[6]!=1||
    u16le(image,16)?!=2||u16le(image,18)?!=62||
    u32le(image,20)?!=1||u16le(image,52)?!=64||
    u16le(image,54)?!=56{
  return Err(Error::Invalid);
 }
 let entry=u64le(image,24)?;
 let phoff=usize::try_from(u64le(image,32)?).map_err(|_|Error::Invalid)?;
 let phnum=u16le(image,56)? as usize;
 if phnum==0||phnum>64||phoff<64||
    phoff.checked_add(phnum*56).is_none_or(|end|end>image.len()){
  return Err(Error::Invalid);
 }
 const USER_END:u64=0x0000_8000_0000_0000;
 if !(0x10000..USER_END).contains(&entry){return Err(Error::Invalid);}
 let mut executable_entry_count=0usize;
 let mut load_count=0usize;
 for index in 0..phnum{
  let at=phoff+index*56;
  let kind=u32le(image,at)?;
  let flags=u32le(image,at+4)?;
  if kind==2||kind==3{return Err(Error::Invalid);} // dynamic / interpreter
  if kind==0x6474e551 && flags&1!=0{return Err(Error::Invalid);} // exec stack
  if kind!=1{continue;}
  load_count+=1;
  if flags&!7!=0||flags&3==3{return Err(Error::Invalid);}
  let file_offset=u64le(image,at+8)?;
  let virtual_address=u64le(image,at+16)?;
  let file_size=u64le(image,at+32)?;
  let memory_size=u64le(image,at+40)?;
  let align=u64le(image,at+48)?;
  if memory_size==0||file_size>memory_size||
     virtual_address<0x10000||virtual_address>=USER_END||
     virtual_address.checked_add(memory_size).is_none_or(|end|end>USER_END)||
     file_offset.checked_add(file_size).is_none_or(|end|end>image.len() as u64)||
     (align>1&&(!align.is_power_of_two()||
         virtual_address%align!=file_offset%align)){
   return Err(Error::Invalid);
  }
  // Reject every overlap in the fully validated list of earlier PT_LOADs.
  let end=virtual_address+memory_size;
  for earlier in 0..index{
   let prior=phoff+earlier*56;
   if u32le(image,prior)?!=1{continue;}
   let prev_start=u64le(image,prior+16)?;
   let prev_len=u64le(image,prior+40)?;
   let prev_end=prev_start.checked_add(prev_len).ok_or(Error::Invalid)?;
   if virtual_address<prev_end&&prev_start<end{return Err(Error::Invalid);}
  }
  if flags&1!=0&&entry>=virtual_address&&
     entry<virtual_address+file_size{
   executable_entry_count+=1;
  }
 }
 if load_count==0||executable_entry_count!=1{return Err(Error::Invalid);}
 Ok(entry)
}
impl BootLaunchGate{
 /// Consume one authenticated admission before accepting or rejecting this
 /// pinned image; a bad ELF or digest cannot be retried under the same epoch.
 pub fn launch_secure_uefi_elf64<D:BlockDevice,P:AuthenticatedSecureUefi,R>(
  &mut self,disk:&mut D,platform:&P,manifest_digest:[u8;32],
  pinned_image:&[u8],
  launch:impl FnOnce(VerifiedPid1Admission,u64)->Result<R,Error>
 )->Result<R,Error>{
  let checked_entry=verify_pid1_elf64(pinned_image);
  self.launch_secure_uefi_manifest_image(disk,platform,manifest_digest,
   |_|{
    checked_entry?;
    Ok(crate::m2_sha256::digest(pinned_image))
   },|receipt|launch(receipt,checked_entry?))
 }
}
#[cfg(test)]
mod kernel_p0_batch8_w6_elf64_tests{
 use super::*;
 struct Platform;
 impl TrustedBootMeasurement for Platform{
  fn read_verified(&self)->Result<[u8;32],Error>{Ok([3;32])}
 }
 impl AuthenticatedPid1Platform for Platform{
  fn read_sealed_floor(&self)->Result<SealedBootFloor,Error>{
   Ok(SealedBootFloor{minimum_epoch:4,minimum_journal_generation:20,
    expected_slot:0,minimum_slot_version:9,expected_measurement:[3;32]})
  }
  fn read_bootloader_selected_slot(&self)->Result<(usize,Slot),Error>{
   Ok((0,tests::slot()))
  }
 }
 impl AuthenticatedSecureUefi for Platform{
  fn require_firmware_secure_boot_chain(&self)->Result<(),Error>{Ok(())}
 }
 fn executable()->[u8;256]{
  let mut image=[0u8;256];
  image[..4].copy_from_slice(b"\x7fELF");
  image[4]=2;image[5]=1;image[6]=1;
  image[16..18].copy_from_slice(&2u16.to_le_bytes());
  image[18..20].copy_from_slice(&62u16.to_le_bytes());
  image[20..24].copy_from_slice(&1u32.to_le_bytes());
  image[24..32].copy_from_slice(&0x400000u64.to_le_bytes());
  image[32..40].copy_from_slice(&64u64.to_le_bytes());
  image[52..54].copy_from_slice(&64u16.to_le_bytes());
  image[54..56].copy_from_slice(&56u16.to_le_bytes());
  image[56..58].copy_from_slice(&1u16.to_le_bytes());
  image[64..68].copy_from_slice(&1u32.to_le_bytes());
  image[68..72].copy_from_slice(&5u32.to_le_bytes()); // R|X, not W
  image[72..80].copy_from_slice(&128u64.to_le_bytes());
  image[80..88].copy_from_slice(&0x400000u64.to_le_bytes());
  image[96..104].copy_from_slice(&4u64.to_le_bytes());
  image[104..112].copy_from_slice(&4096u64.to_le_bytes());
  image[112..120].copy_from_slice(&1u64.to_le_bytes());
  image[128..132].copy_from_slice(&[0x90,0x90,0x90,0xc3]);
  image
 }
 #[test]fn hash_verified_elf_file_backed_entry_is_handed_to_loader_once(){
  let mut disk=tests::fixture();let mut gate=BootLaunchGate::new();
  let image=executable();let expected=crate::m2_sha256::digest(&image);
  assert_eq!(verify_pid1_elf64(&image),Ok(0x400000));
  let mut count=0;
  assert_eq!(gate.launch_secure_uefi_elf64(&mut disk,&Platform,
   expected,&image,|receipt,entry|{
    count+=1;Ok((receipt.epoch(),entry))
   }),Ok((4,0x400000)));
  assert_eq!(count,1);
  assert_eq!(gate.launch_secure_uefi_elf64(&mut disk,&Platform,
   expected,&image,|_,_|{count+=1;Ok((9,0))}),Err(Error::Stale));
  assert_eq!(count,1);
 }
 #[test]fn write_execute_entry_bss_and_truncated_headers_are_not_runnable(){
  let mut elf=executable();
  elf[68..72].copy_from_slice(&7u32.to_le_bytes());
  assert_eq!(verify_pid1_elf64(&elf),Err(Error::Invalid));
  elf=executable();elf[24..32].copy_from_slice(&0x400100u64.to_le_bytes());
  assert_eq!(verify_pid1_elf64(&elf),Err(Error::Invalid));
  elf=executable();elf[32..40].copy_from_slice(&250u64.to_le_bytes());
  assert_eq!(verify_pid1_elf64(&elf),Err(Error::Invalid));
  elf=executable();elf[72..80].copy_from_slice(&u64::MAX.to_le_bytes());
  assert_eq!(verify_pid1_elf64(&elf),Err(Error::Invalid));
 }
 #[test]fn invalid_elf_or_wrong_image_hash_burns_authenticated_admission(){
  let mut disk=tests::fixture();let mut gate=BootLaunchGate::new();
  let mut elf=executable();let correct=crate::m2_sha256::digest(&elf);
  elf[68..72].copy_from_slice(&7u32.to_le_bytes());
  let mut starts=0;
  assert_eq!(gate.launch_secure_uefi_elf64(&mut disk,&Platform,correct,
   &elf,|_,_|{starts+=1;Ok(())}),Err(Error::Invalid));
  assert_eq!(starts,0);assert_eq!(gate.last_admitted_epoch(),4);
  assert_eq!(gate.launch_secure_uefi_elf64(&mut disk,&Platform,correct,
   &executable(),|_,_|{starts+=1;Ok(())}),Err(Error::Stale));
  assert_eq!(starts,0);
  let mut disk=tests::fixture();let mut gate=BootLaunchGate::new();
  assert_eq!(gate.launch_secure_uefi_elf64(&mut disk,&Platform,[9;32],
   &executable(),|_,_|{starts+=1;Ok(())}),Err(Error::Untrusted));
  assert_eq!(starts,0);
 }
}


/// P0-178..180: page-granular ELF64 mapping preflight with bounded user
/// stack and a completely unmapped guard page. Two byte-disjoint PT_LOAD
/// segments may share one PTE; reject any such permissions ambiguity.
/// This verifies a mapping plan but does not actually create page tables.
pub fn verify_pid1_elf64_page_isolation(image:&[u8],
 stack_top:u64,stack_pages:usize)->Result<u64,Error>{
 let entry=verify_pid1_elf64(image)?;
 const PAGE:u64=4096;
 const USER_END:u64=0x0000_8000_0000_0000;
 if !(2..=256).contains(&stack_pages)||stack_top%PAGE!=0||
    stack_top>USER_END||stack_top<0x10000+PAGE{
  return Err(Error::Invalid);
 }
 let guarded_bytes=(stack_pages as u64+1)*PAGE;
 let guard_start=stack_top.checked_sub(guarded_bytes).ok_or(Error::Invalid)?;
 if guard_start<0x10000{return Err(Error::Invalid);}
 let phoff=u64::from_le_bytes(image[32..40].try_into().map_err(|_|Error::Invalid)?) as usize;
 let phnum=u16::from_le_bytes(image[56..58].try_into().map_err(|_|Error::Invalid)?) as usize;
 for i in 0..phnum{
  let base=phoff+i*56;
  if u32::from_le_bytes(image[base..base+4].try_into().map_err(|_|Error::Invalid)?)!=1{
   continue;
  }
  let start=u64::from_le_bytes(image[base+16..base+24].try_into().map_err(|_|Error::Invalid)?);
  let len=u64::from_le_bytes(image[base+40..base+48].try_into().map_err(|_|Error::Invalid)?);
  let page_start=start&!(PAGE-1);
  let page_end=start.checked_add(len).and_then(|v|v.checked_add(PAGE-1))
   .ok_or(Error::Invalid)?&!(PAGE-1);
  if page_start<stack_top&&guard_start<page_end{return Err(Error::Invalid);}
  for j in 0..i{
   let other=phoff+j*56;
   if u32::from_le_bytes(image[other..other+4].try_into().map_err(|_|Error::Invalid)?)!=1{
    continue;
   }
   let before=u64::from_le_bytes(image[other+16..other+24].try_into().map_err(|_|Error::Invalid)?);
   let size=u64::from_le_bytes(image[other+40..other+48].try_into().map_err(|_|Error::Invalid)?);
   let before_end=before.checked_add(size).and_then(|v|v.checked_add(PAGE-1))
    .ok_or(Error::Invalid)?&!(PAGE-1);
   if page_start<before_end&&(before&!(PAGE-1))<page_end{
    return Err(Error::Invalid);
   }
  }
 }
 Ok(entry)
}
impl BootLaunchGate{
 pub fn launch_secure_uefi_elf64_isolated<D:BlockDevice,P:AuthenticatedSecureUefi,R>(
  &mut self,disk:&mut D,platform:&P,manifest_digest:[u8;32],
  image:&[u8],stack_top:u64,stack_pages:usize,
  launch:impl FnOnce(VerifiedPid1Admission,u64)->Result<R,Error>
 )->Result<R,Error>{
  let isolated=verify_pid1_elf64_page_isolation(image,stack_top,stack_pages);
  self.launch_secure_uefi_manifest_image(disk,platform,manifest_digest,
   |_|{
    isolated?;
    Ok(crate::m2_sha256::digest(image))
   },|receipt|launch(receipt,isolated?))
 }
}
#[cfg(test)]
mod kernel_p0_batch9_w6_page_isolation_tests{
 use super::*;
 fn elf()->[u8;256]{
  let mut image=[0u8;256];image[..4].copy_from_slice(b"\x7fELF");
  image[4]=2;image[5]=1;image[6]=1;
  image[16..18].copy_from_slice(&2u16.to_le_bytes());
  image[18..20].copy_from_slice(&62u16.to_le_bytes());
  image[20..24].copy_from_slice(&1u32.to_le_bytes());
  image[24..32].copy_from_slice(&0x400000u64.to_le_bytes());
  image[32..40].copy_from_slice(&64u64.to_le_bytes());
  image[52..54].copy_from_slice(&64u16.to_le_bytes());
  image[54..56].copy_from_slice(&56u16.to_le_bytes());
  image[56..58].copy_from_slice(&1u16.to_le_bytes());
  image[64..68].copy_from_slice(&1u32.to_le_bytes());
  image[68..72].copy_from_slice(&5u32.to_le_bytes());
  image[72..80].copy_from_slice(&128u64.to_le_bytes());
  image[80..88].copy_from_slice(&0x400000u64.to_le_bytes());
  image[96..104].copy_from_slice(&4u64.to_le_bytes());
  image[104..112].copy_from_slice(&4096u64.to_le_bytes());
  image[112..120].copy_from_slice(&1u64.to_le_bytes());
  image[128..132].copy_from_slice(&[0x90,0x90,0x90,0xc3]);
  image
 }
 #[test]fn isolated_stack_and_guard_validate_before_mapping(){
  let image=elf();
  assert_eq!(verify_pid1_elf64_page_isolation(&image,0x800000,8),
   Ok(0x400000));
  assert_eq!(verify_pid1_elf64_page_isolation(&image,0x401000,2),
   Err(Error::Invalid));
  assert_eq!(verify_pid1_elf64_page_isolation(&image,0x800000,0),
   Err(Error::Invalid));
 }
 #[test]fn byte_disjoint_loads_sharing_one_pte_must_fail(){
  let mut image=elf();image[56..58].copy_from_slice(&2u16.to_le_bytes());
  // Small first segment (not a full-page BSS): byte ranges are disjoint,
  // but both load permissions still target the same 4 KiB page.
  image[104..112].copy_from_slice(&4u64.to_le_bytes());
  image[120..124].copy_from_slice(&1u32.to_le_bytes());
  image[124..128].copy_from_slice(&6u32.to_le_bytes());
  image[128..136].copy_from_slice(&0u64.to_le_bytes());
  image[136..144].copy_from_slice(&0x400100u64.to_le_bytes());
  image[152..160].copy_from_slice(&1u64.to_le_bytes());
  image[160..168].copy_from_slice(&1u64.to_le_bytes());
  assert_eq!(verify_pid1_elf64(&image),Ok(0x400000));
  assert_eq!(verify_pid1_elf64_page_isolation(&image,0x800000,4),
   Err(Error::Invalid));
 }
}
