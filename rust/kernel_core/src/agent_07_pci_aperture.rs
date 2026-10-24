//! Bounded numeric PCI BAR aperture check. Never maps or accesses MMIO.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aperture { pub base: u64, pub length: u64 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApertureError { Alignment, Empty, Overflow, OutsideAllowed }

/// The upper bound is exclusive, as is the aperture endpoint.
pub fn validate(aperture: Aperture, lower: u64, upper: u64, align: u64)
    -> Result<(), ApertureError>
{
    if align == 0 || !align.is_power_of_two() { return Err(ApertureError::Alignment); }
    if aperture.length == 0 { return Err(ApertureError::Empty); }
    if aperture.base % align != 0 || aperture.length % align != 0 {
        return Err(ApertureError::Alignment);
    }
    let end = aperture.base.checked_add(aperture.length).ok_or(ApertureError::Overflow)?;
    if lower >= upper || aperture.base < lower || end > upper {
        return Err(ApertureError::OutsideAllowed);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allows_page_aligned_aperture_in_reserved_range() {
        assert_eq!(validate(Aperture { base: 0x2000, length: 0x1000 },
            0x1000, 0x4000, 0x1000), Ok(()));
    }
    #[test]
    fn rejects_invalid_alignment_and_zero_length() {
        assert_eq!(validate(Aperture { base: 0x2001, length: 0x1000 },
            0x1000, 0x4000, 0x1000), Err(ApertureError::Alignment));
        assert_eq!(validate(Aperture { base: 0x2000, length: 0 },
            0x1000, 0x4000, 0x1000), Err(ApertureError::Empty));
        assert_eq!(validate(Aperture { base: 0x2000, length: 0x1000 },
            0x1000, 0x4000, 3), Err(ApertureError::Alignment));
    }
    #[test]
    fn handles_overflow_and_exclusive_boundary() {
        assert_eq!(validate(Aperture { base: u64::MAX - 0x0fff, length: 0x2000 },
            0, u64::MAX, 0x1000), Err(ApertureError::Overflow));
        assert_eq!(validate(Aperture { base: 0x3000, length: 0x2000 },
            0x1000, 0x4000, 0x1000), Err(ApertureError::OutsideAllowed));
    }
}
