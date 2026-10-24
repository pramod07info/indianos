//! Bounded, allocation-free block extent validation (policy helper only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent { pub start: u64, pub blocks: u64 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtentError { Empty, Overflow, OutsideDevice, Overlap, Unsorted }

/// Validate ordered, nonoverlapping [start, start + blocks) ranges.
pub fn validate_extents(items: &[Extent], device_blocks: u64) -> Result<u64, ExtentError> {
    let mut end = 0u64;
    let mut previous_start = 0u64;
    let mut total = 0u64;
    for (index, item) in items.iter().enumerate() {
        if item.blocks == 0 { return Err(ExtentError::Empty); }
        if index != 0 && item.start < previous_start { return Err(ExtentError::Unsorted); }
        if index != 0 && item.start < end { return Err(ExtentError::Overlap); }
        let next_end = item.start.checked_add(item.blocks).ok_or(ExtentError::Overflow)?;
        if next_end > device_blocks { return Err(ExtentError::OutsideDevice); }
        total = total.checked_add(item.blocks).ok_or(ExtentError::Overflow)?;
        previous_start = item.start;
        end = next_end;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_adjacent_and_empty_ranges() {
        assert_eq!(validate_extents(&[], 100), Ok(0));
        assert_eq!(validate_extents(&[Extent { start: 0, blocks: 4 },
            Extent { start: 4, blocks: 6 }], 10), Ok(10));
    }
    #[test]
    fn rejects_overlap_unsorted_and_zero() {
        assert_eq!(validate_extents(&[Extent { start: 1, blocks: 3 },
            Extent { start: 2, blocks: 2 }], 10), Err(ExtentError::Overlap));
        assert_eq!(validate_extents(&[Extent { start: 9, blocks: 1 },
            Extent { start: 2, blocks: 1 }], 10), Err(ExtentError::Unsorted));
        assert_eq!(validate_extents(&[Extent { start: 0, blocks: 0 }], 10), Err(ExtentError::Empty));
    }
    #[test]
    fn rejects_overflow_and_device_boundary() {
        assert_eq!(validate_extents(&[Extent { start: u64::MAX, blocks: 2 }], u64::MAX),
            Err(ExtentError::Overflow));
        assert_eq!(validate_extents(&[Extent { start: 10, blocks: 1 }], 10),
            Err(ExtentError::OutsideDevice));
    }
}
