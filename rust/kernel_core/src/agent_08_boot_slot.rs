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
