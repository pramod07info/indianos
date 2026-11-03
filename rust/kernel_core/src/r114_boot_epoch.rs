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
