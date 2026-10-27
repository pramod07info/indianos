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

