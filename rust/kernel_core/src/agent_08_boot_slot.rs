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
