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

/// Preflight a complete MMIO aperture set before mapping anything. This
/// numeric-only check rejects cross-device overlapping BAR ranges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetError { Invalid(ApertureError), Overlap, TooMany }
pub const MAX_APERTURES: usize = 32;
pub fn validate_set(regions: &[Aperture], lower: u64, upper: u64, align: u64)
    -> Result<(), SetError>
{
    if regions.len() > MAX_APERTURES { return Err(SetError::TooMany); }
    for (i, region) in regions.iter().enumerate() {
        validate(*region, lower, upper, align).map_err(SetError::Invalid)?;
        let end = region.base + region.length; // validated non-overflow above
        for earlier in &regions[..i] {
            let earlier_end = earlier.base + earlier.length;
            if region.base < earlier_end && earlier.base < end {
                return Err(SetError::Overlap);
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod set_tests {
    use super::*;
    #[test]
    fn adjacent_bar_regions_are_allowed() {
        let items = [Aperture { base: 0x1000, length: 0x1000 },
            Aperture { base: 0x2000, length: 0x1000 }];
        assert_eq!(validate_set(&items, 0x1000, 0x4000, 0x1000), Ok(()));
    }
    #[test]
    fn overlapping_bars_cannot_be_mapped() {
        let items = [Aperture { base: 0x1000, length: 0x2000 },
            Aperture { base: 0x2000, length: 0x1000 }];
        assert_eq!(validate_set(&items, 0x1000, 0x5000, 0x1000), Err(SetError::Overlap));
    }
    #[test]
    fn invalid_or_oversized_sets_fail_closed() {
        let many = [Aperture { base: 0x1000, length: 0x1000 }; MAX_APERTURES + 1];
        assert_eq!(validate_set(&many, 0, 0x10000, 0x1000), Err(SetError::TooMany));
        assert_eq!(validate_set(&[Aperture { base: 0, length: 0 }], 0, 10, 1),
            Err(SetError::Invalid(ApertureError::Empty)));
    }
}
