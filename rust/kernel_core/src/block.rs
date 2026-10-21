//! R015 storage foundation: bounded sector I/O independent of hardware.
//! This is an interface, not a persistent QEMU disk driver.
pub const SECTOR_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockError { OutOfRange, ReadOnly, Io }

/// The driver must perform exactly one sector transfer per successful call.
/// Device implementations must not retain the caller's borrowed buffers.
pub trait BlockDevice {
    fn sector_count(&self) -> u64;
    fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES]) -> Result<(), BlockError>;
    fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_BYTES]) -> Result<(), BlockError>;
}

/// Preflight the full transfer before any sector is touched. This prevents
/// partial writes caused solely by a request crossing the device boundary.
/// Device I/O failures may still cause partial transfers.
pub fn check_range(capacity: u64, start: u64, count: u64) -> Result<(), BlockError> {
    if count == 0 || start > capacity || count > capacity - start {
        return Err(BlockError::OutOfRange);
    }
    Ok(())
}

pub fn read_sectors<D: BlockDevice>(
    device: &mut D, start: u64, out: &mut [u8],
) -> Result<(), BlockError> {
    if out.is_empty() || out.len() % SECTOR_BYTES != 0 {
        return Err(BlockError::OutOfRange);
    }
    let count = (out.len() / SECTOR_BYTES) as u64;
    check_range(device.sector_count(), start, count)?;
    for (index, chunk) in out.chunks_exact_mut(SECTOR_BYTES).enumerate() {
        let sector: &mut [u8; SECTOR_BYTES] = chunk.try_into().map_err(|_| BlockError::Io)?;
        device.read_sector(start + index as u64, sector)?;
    }
    Ok(())
}

pub fn write_sectors<D: BlockDevice>(
    device: &mut D, start: u64, data: &[u8],
) -> Result<(), BlockError> {
    if data.is_empty() || data.len() % SECTOR_BYTES != 0 {
        return Err(BlockError::OutOfRange);
    }
    let count = (data.len() / SECTOR_BYTES) as u64;
    check_range(device.sector_count(), start, count)?;
    for (index, chunk) in data.chunks_exact(SECTOR_BYTES).enumerate() {
        let sector: &[u8; SECTOR_BYTES] = chunk.try_into().map_err(|_| BlockError::Io)?;
        device.write_sector(start + index as u64, sector)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct RamDisk { bytes: [u8; SECTOR_BYTES * 4], readonly: bool }
    impl BlockDevice for RamDisk {
        fn sector_count(&self) -> u64 { 4 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            check_range(4, lba, 1)?;
            let offset = lba as usize * SECTOR_BYTES;
            out.copy_from_slice(&self.bytes[offset..offset + SECTOR_BYTES]);
            Ok(())
        }
        fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_BYTES]) -> Result<(), BlockError> {
            check_range(4, lba, 1)?;
            if self.readonly { return Err(BlockError::ReadOnly); }
            let offset = lba as usize * SECTOR_BYTES;
            self.bytes[offset..offset + SECTOR_BYTES].copy_from_slice(data);
            Ok(())
        }
    }
    #[test]
    fn round_trip_multiple_sectors() {
        let mut disk = RamDisk { bytes: [0; SECTOR_BYTES * 4], readonly: false };
        let mut data = [0u8; SECTOR_BYTES * 2];
        for (i, byte) in data.iter_mut().enumerate() { *byte = (i % 251) as u8; }
        write_sectors(&mut disk, 1, &data).unwrap();
        let mut result = [0u8; SECTOR_BYTES * 2];
        read_sectors(&mut disk, 1, &mut result).unwrap();
        assert_eq!(result, data);
        assert_eq!(&disk.bytes[..SECTOR_BYTES], &[0; SECTOR_BYTES]);
    }
    #[test]
    fn bounds_checked_before_mutation() {
        let mut disk = RamDisk { bytes: [0; SECTOR_BYTES * 4], readonly: false };
        let data = [42u8; SECTOR_BYTES * 2];
        assert_eq!(write_sectors(&mut disk, 3, &data), Err(BlockError::OutOfRange));
        assert!(disk.bytes.iter().all(|&byte| byte == 0));
        assert_eq!(check_range(4, u64::MAX, 2), Err(BlockError::OutOfRange));
        assert_eq!(check_range(4, 0, 0), Err(BlockError::OutOfRange));
        assert_eq!(write_sectors(&mut disk, 0, &[0; 1]), Err(BlockError::OutOfRange));
    }
    #[test]
    fn read_only_device_rejects_writes() {
        let mut disk = RamDisk { bytes: [0; SECTOR_BYTES * 4], readonly: true };
        assert_eq!(write_sectors(&mut disk, 0, &[1; SECTOR_BYTES]), Err(BlockError::ReadOnly));
    }
}
