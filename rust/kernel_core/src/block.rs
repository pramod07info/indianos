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
    /// Explicit persistence barrier. An unknown driver must NOT pretend
    /// it supports durable flushes; a caller may require fsync semantics.
    /// Existing write_sector() contracts still mandate durable completion.
    fn sync(&mut self) -> Result<(), BlockError> { Err(BlockError::Io) }
}

/// A hard read-only capability. Even if a future discovery or diagnostic
/// mistakenly attempts write_sector(), it cannot reach the physical device.
/// This is a borrowed view, not a snapshot or a software-only convention.
pub struct ReadOnlyDevice<'a,D:BlockDevice>{
 device:&'a mut D,
 // A read-only probe is bound to one geometry snapshot. Once a change
 // has been observed, resetting the size cannot resurrect a stale probe.
 parent_capacity:u64,
 invalidated:bool,
}
impl<'a,D:BlockDevice> ReadOnlyDevice<'a,D>{
 pub fn new(device:&'a mut D)->Self{
  let parent_capacity=device.sector_count();
  Self{device,parent_capacity,invalidated:false}
 }
 fn check_capacity(&mut self)->Result<(),BlockError>{
  if self.invalidated{return Err(BlockError::Io);}
  if self.device.sector_count()!=self.parent_capacity{
   self.invalidated=true;
   return Err(BlockError::Io);
  }
  Ok(())
 }
}
impl<D:BlockDevice> BlockDevice for ReadOnlyDevice<'_,D>{
 fn sector_count(&self)->u64{self.parent_capacity}
 fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
  ->Result<(),BlockError>{
  check_range(self.parent_capacity,lba,1)?;
  self.check_capacity()?;
  // Do not expose partial DMA/bounce-buffer output after a failed read.
  let mut staged=[0u8;SECTOR_BYTES];
  let transferred=self.device.read_sector(lba,&mut staged);
  // An uncertain read may represent device removal or replacement.
  // Once a driver faults, do not use this stale read-only view again.
  if transferred.is_err() { self.invalidated=true; }
  self.check_capacity()?;
  transferred?;
  out.copy_from_slice(&staged);
  Ok(())
 }
 fn write_sector(&mut self,_lba:u64,_bytes:&[u8;SECTOR_BYTES])
  ->Result<(),BlockError>{Err(BlockError::ReadOnly)}
 fn sync(&mut self)->Result<(),BlockError>{Err(BlockError::ReadOnly)}
}
/// Restrict access to a validated block-device window; no I/O occurs on construction.
/// Destructive formatting is still an explicit responsibility of the caller.
pub struct BlockWindow<'a, D: BlockDevice> {
    device: &'a mut D,
    start: u64,
    count: u64,
    // Reject removable/media-geometry changes after window selection.
    // This is not a full device identity token: GPT mounts additionally
    // pin and recheck disk and partition GUIDs.
    parent_capacity: u64,
    // Once a capacity change is observed, never reauthorize this window
    // merely because a later capacity reading happens to match again.
    invalidated: bool,
}
impl<'a, D: BlockDevice> BlockWindow<'a, D> {
    pub fn new(device: &'a mut D, start: u64, count: u64) -> Result<Self, BlockError> {
        let parent_capacity = device.sector_count();
        check_range(parent_capacity, start, count)?;
        // Capacity may change while validating the requested geometry.
        // Never authorize a new mount window from two inconsistent reads.
        if device.sector_count() != parent_capacity { return Err(BlockError::Io); }
        Ok(Self { device, start, count, parent_capacity, invalidated: false })
    }
    pub fn sector_start(&self) -> u64 { self.start }

    fn check_parent_capacity(&mut self) -> Result<(), BlockError> {
        if self.invalidated { return Err(BlockError::Io); }
        if self.device.sector_count() != self.parent_capacity {
            self.invalidated = true;
            return Err(BlockError::Io);
        }
        Ok(())
    }
    /// Read-only parent inspection while the bounded view retains exclusive
    /// ownership. Used to validate partition identity after the final mount.
    pub(crate) fn validate_parent<E>(&mut self,
        validate:impl FnOnce(&mut ReadOnlyDevice<'_,D>)->Result<(),E>,
        map_failure:impl Fn(BlockError)->E)
        ->Result<(),E>
    {
        // The bounded mount must be healthy both before AND after parent
        // identity inspection. The caller supplies a domain-error mapping;
        // this prevents unchecked physical capacity changes from bypassing
        // a valid GPT identity returned by a read-only callback.
        self.check_parent_capacity().map_err(|e|map_failure(e))?;
        let inspected=validate(&mut ReadOnlyDevice::new(self.device));
        self.check_parent_capacity().map_err(|e|map_failure(e))?;
        if inspected.is_err() {
            // A failed GPT identity validation is a permanent mount
            // revocation even if the new device has the same geometry.
            self.invalidated=true;
        }
        inspected
    }

}
impl<D: BlockDevice> BlockDevice for BlockWindow<'_, D> {
    fn sector_count(&self) -> u64 { self.count }
    fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES])
        -> Result<(), BlockError>
    {
        check_range(self.count, lba, 1)?;
        self.check_parent_capacity()?;
        // A failing physical driver may have already changed its output.
        // Stage until both I/O AND media-geometry postflight succeed.
        let mut sector = [0u8; SECTOR_BYTES];
        if let Err(error)=self.device.read_sector(self.start + lba, &mut sector) {
            // Retire the view after a driver fault; no dirty bytes escape.
            self.invalidated=true;
            return Err(error);
        }
        self.check_parent_capacity()?;
        out.copy_from_slice(&sector);
        Ok(())
    }
    fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_BYTES])
        -> Result<(), BlockError>
    {
        check_range(self.count, lba, 1)?;
        // Never write a selected LBA on a device with changed geometry.
        self.check_parent_capacity()?;
        // A driver can report I/O failure after partially writing a sector.
        // This mounted partition must be retired, not reused for another
        // mutation with a potentially torn filesystem generation.
        if let Err(error)=self.device.write_sector(self.start + lba, data){
            self.invalidated=true;
            return Err(error);
        }
        // A change during a write is uncertain (the write cannot be undone).
        // Make it an error so callers must not trust the mounted session.
        self.check_parent_capacity()
    }
    fn sync(&mut self)->Result<(),BlockError>{
        self.check_parent_capacity()?;
        // A failed cache flush is an uncertain durability boundary. Refuse
        // subsequent traffic even if the driver later reports success.
        let result=self.device.sync();
        if result.is_err(){self.invalidated=true;}
        self.check_parent_capacity()?;
        result
    }
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
    let capacity = device.sector_count();
    check_range(capacity, start, count)?;
    // Reject mid-transfer geometry changes instead of silently combining
    // data from different devices. A failed transfer clears all output.
    let mut sector = [0u8; SECTOR_BYTES];
    for index in 0..(count as usize) {
        if device.sector_count() != capacity {
            out.fill(0);
            return Err(BlockError::Io);
        }
        if let Err(error) = device.read_sector(start + index as u64, &mut sector) {
            out.fill(0);
            return Err(error);
        }
        if device.sector_count() != capacity {
            out.fill(0);
            return Err(BlockError::Io);
        }
        let at = index * SECTOR_BYTES;
        out[at..at + SECTOR_BYTES].copy_from_slice(&sector);
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
    let capacity = device.sector_count();
    check_range(capacity, start, count)?;
    for (index, chunk) in data.chunks_exact(SECTOR_BYTES).enumerate() {
        if device.sector_count() != capacity { return Err(BlockError::Io); }
        let sector: &[u8; SECTOR_BYTES] = chunk.try_into().map_err(|_| BlockError::Io)?;
        device.write_sector(start + index as u64, sector)?;
        // Previously completed sectors cannot be rolled back after a
        // capacity change: stop here and surface uncertain I/O to caller.
        if device.sector_count() != capacity { return Err(BlockError::Io); }
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
    fn r055_failed_multisector_read_wipes_partial_prefix_and_tail() {
        struct Interrupted { disk: RamDisk, reads: usize }
        impl BlockDevice for Interrupted {
            fn sector_count(&self)->u64{self.disk.sector_count()}
            fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
                ->Result<(),BlockError>{
                if self.reads == 1 {
                    out.fill(0x77); // faulting devices may dirty buffers
                    return Err(BlockError::Io);
                }
                self.reads += 1;
                self.disk.read_sector(lba,out)
            }
            fn write_sector(&mut self,lba:u64,bytes:&[u8;SECTOR_BYTES])
                ->Result<(),BlockError>{self.disk.write_sector(lba,bytes)}
        }
        let mut disk=Interrupted{disk:RamDisk{
            bytes:[0x33;SECTOR_BYTES*4],readonly:false},reads:0};
        let mut out=[0xa5;SECTOR_BYTES*3];
        assert_eq!(read_sectors(&mut disk,0,&mut out),Err(BlockError::Io));
        assert_eq!(out,[0;SECTOR_BYTES*3]);
        assert_eq!(disk.reads,1);
    }
    #[test]
    fn r055_invalid_multisector_read_is_preflight_atomic() {
        let mut disk=RamDisk{bytes:[0x33;SECTOR_BYTES*4],readonly:false};
        let mut out=[0xa5;SECTOR_BYTES*2];
        assert_eq!(read_sectors(&mut disk,3,&mut out),Err(BlockError::OutOfRange));
        assert_eq!(out,[0xa5;SECTOR_BYTES*2]);
        assert_eq!(read_sectors(&mut disk,0,&mut out),Ok(()));
        assert_eq!(out,[0x33;SECTOR_BYTES*2]);
    }
    #[test]
    fn bounded_partition_enforces_range_and_preserves_neighbors() {
        let mut disk = RamDisk { bytes: [0; SECTOR_BYTES * 4], readonly: false };
        {
            let mut win = BlockWindow::new(&mut disk, 1, 2).unwrap();
            assert_eq!(win.sector_count(), 2);
            assert_eq!(win.sector_start(), 1);
            win.write_sector(0, &[0x41; SECTOR_BYTES]).unwrap();
            win.write_sector(1, &[0x42; SECTOR_BYTES]).unwrap();
            let mut out = [0; SECTOR_BYTES];
            win.read_sector(1, &mut out).unwrap();
            assert_eq!(out, [0x42; SECTOR_BYTES]);
            assert_eq!(win.read_sector(2, &mut out), Err(BlockError::OutOfRange));
            assert_eq!(win.write_sector(u64::MAX, &[3; SECTOR_BYTES]),
                       Err(BlockError::OutOfRange));
        }
        assert_eq!(&disk.bytes[..SECTOR_BYTES], &[0; SECTOR_BYTES]);
        assert_eq!(&disk.bytes[SECTOR_BYTES..2*SECTOR_BYTES], &[0x41; SECTOR_BYTES]);
        assert_eq!(&disk.bytes[2*SECTOR_BYTES..3*SECTOR_BYTES], &[0x42; SECTOR_BYTES]);
        assert_eq!(&disk.bytes[3*SECTOR_BYTES..], &[0; SECTOR_BYTES]);
    }

    #[test]
    fn invalid_window_never_touches_disk() {
        let mut disk = RamDisk { bytes: [0x33; SECTOR_BYTES * 4], readonly: false };
        assert!(matches!(BlockWindow::new(&mut disk, 3, 2), Err(BlockError::OutOfRange)));
        assert!(matches!(BlockWindow::new(&mut disk, 0, 0), Err(BlockError::OutOfRange)));
        assert!(matches!(BlockWindow::new(&mut disk, u64::MAX, 1), Err(BlockError::OutOfRange)));
        assert!(disk.bytes.iter().all(|&v| v == 0x33));
    }

    #[test]
    fn m1_r045_hard_readonly_borrow_denies_writes_and_preserves_data(){
        let mut disk=RamDisk{bytes:[0x55;SECTOR_BYTES*4],readonly:false};
        {
            let mut readonly=ReadOnlyDevice::new(&mut disk);
            assert_eq!(readonly.sector_count(),4);
            let mut output=[0u8;SECTOR_BYTES];
            assert_eq!(readonly.read_sector(3,&mut output),Ok(()));
            assert_eq!(output,[0x55;SECTOR_BYTES]);
            assert_eq!(readonly.write_sector(2,&[0x44;SECTOR_BYTES]),
                Err(BlockError::ReadOnly));
            assert_eq!(write_sectors(&mut readonly,0,&[0;SECTOR_BYTES]),
                Err(BlockError::ReadOnly));
        }
        assert_eq!(disk.bytes,[0x55;SECTOR_BYTES*4]);
    }
    #[test]
    fn m1_r045_readonly_window_stacks_with_partition_bounds(){
        let mut disk=RamDisk{bytes:[0x11;SECTOR_BYTES*4],readonly:false};
        {
            let mut window=BlockWindow::new(&mut disk,1,2).unwrap();
            let mut readonly=ReadOnlyDevice::new(&mut window);
            let mut output=[0u8;SECTOR_BYTES];
            assert_eq!(readonly.read_sector(1,&mut output),Ok(()));
            assert_eq!(output,[0x11;SECTOR_BYTES]);
            assert_eq!(readonly.read_sector(2,&mut output),Err(BlockError::OutOfRange));
            assert_eq!(readonly.write_sector(0,&[0x44;SECTOR_BYTES]),
                Err(BlockError::ReadOnly));
        }
        assert_eq!(disk.bytes,[0x11;SECTOR_BYTES*4]);
    }
    #[test]
    fn read_only_device_rejects_writes() {
        let mut disk = RamDisk { bytes: [0; SECTOR_BYTES * 4], readonly: true };
        assert_eq!(write_sectors(&mut disk, 0, &[1; SECTOR_BYTES]), Err(BlockError::ReadOnly));
    }

    #[test]
    fn m1_r086_bounded_sector_read_keeps_caller_buffer_on_driver_error() {
        struct FaultingDisk { attempts: usize }
        impl BlockDevice for FaultingDisk {
            fn sector_count(&self) -> u64 { 4 }
            fn read_sector(&mut self, _lba: u64, out: &mut [u8; SECTOR_BYTES])
                -> Result<(), BlockError> {
                self.attempts += 1;
                // Simulate a real driver that partially touches a DMA bounce
                // buffer before reporting device error.
                out[..32].fill(0xef);
                Err(BlockError::Io)
            }
            fn write_sector(&mut self, _lba: u64, _data: &[u8; SECTOR_BYTES])
                -> Result<(), BlockError> { Err(BlockError::ReadOnly) }
        }
        let mut disk = FaultingDisk { attempts: 0 };
        {
            let mut volume = BlockWindow::new(&mut disk, 1, 2).unwrap();
            let mut output = [0x5au8; SECTOR_BYTES];
            assert_eq!(volume.read_sector(0, &mut output), Err(BlockError::Io));
            assert_eq!(output, [0x5a; SECTOR_BYTES]);
            assert_eq!(volume.read_sector(2, &mut output), Err(BlockError::OutOfRange));
            assert_eq!(output, [0x5a; SECTOR_BYTES]);
        }
        assert_eq!(disk.attempts, 1);
    }

    #[test]
    fn m1_r086_bounded_sector_read_publishes_full_successful_sector() {
        let mut disk = RamDisk { bytes: [0; SECTOR_BYTES * 4], readonly: false };
        disk.bytes[SECTOR_BYTES..SECTOR_BYTES * 2].fill(0x35);
        let mut volume = BlockWindow::new(&mut disk, 1, 2).unwrap();
        let mut output = [0x5a; SECTOR_BYTES];
        assert_eq!(volume.read_sector(0, &mut output), Ok(()));
        assert_eq!(output, [0x35; SECTOR_BYTES]);
    }

    /// Emulate removable hardware where geometry changes while the same
    /// borrowed block device remains reachable. The caller must never assume
    /// that a checked partition window remains valid across a media change.
    struct R087GeometryDisk<'a> {
        capacity: &'a core::cell::Cell<u64>,
        reads: usize,
        writes: usize,
        change_on_read: bool,
        change_on_write: bool,
    }
    impl BlockDevice for R087GeometryDisk<'_> {
        fn sector_count(&self) -> u64 { self.capacity.get() }
        fn read_sector(&mut self, _lba: u64, out: &mut [u8; SECTOR_BYTES])
            -> Result<(), BlockError>
        {
            self.reads += 1;
            out.fill(0x4d);
            if self.change_on_read { self.capacity.set(5); }
            Ok(())
        }
        fn write_sector(&mut self, _lba: u64, _data: &[u8; SECTOR_BYTES])
            -> Result<(), BlockError>
        {
            self.writes += 1;
            if self.change_on_write { self.capacity.set(5); }
            Ok(())
        }
    }

    #[test]
    fn m1_r087_changed_parent_geometry_blocks_all_window_io_before_driver() {
        let size = core::cell::Cell::new(4u64);
        let mut disk = R087GeometryDisk {
            capacity: &size, reads: 0, writes: 0,
            change_on_read: false, change_on_write: false,
        };
        let mut window = BlockWindow::new(&mut disk, 1, 2).unwrap();
        size.set(3); // shrinking device; selected LBA still fits, but identity changed
        let mut out = [0x5a; SECTOR_BYTES];
        assert_eq!(window.read_sector(0, &mut out), Err(BlockError::Io));
        assert_eq!(out, [0x5a; SECTOR_BYTES]);
        assert_eq!(window.write_sector(0, &[0x33; SECTOR_BYTES]), Err(BlockError::Io));
        size.set(6); // growing device must also invalidate the existing window
        assert_eq!(window.read_sector(1, &mut out), Err(BlockError::Io));
        assert_eq!(window.write_sector(1, &[0x33; SECTOR_BYTES]), Err(BlockError::Io));
        drop(window);
        assert_eq!(disk.reads, 0);
        assert_eq!(disk.writes, 0);
    }

    #[test]
    fn m1_r087_read_does_not_publish_sector_if_geometry_changes_mid_transfer() {
        let size = core::cell::Cell::new(4u64);
        let mut disk = R087GeometryDisk {
            capacity: &size, reads: 0, writes: 0,
            change_on_read: true, change_on_write: false,
        };
        let mut window = BlockWindow::new(&mut disk, 1, 2).unwrap();
        let mut out = [0xa5; SECTOR_BYTES];
        assert_eq!(window.read_sector(0, &mut out), Err(BlockError::Io));
        assert_eq!(out, [0xa5; SECTOR_BYTES]); // staged device data never leaks
        drop(window);
        assert_eq!(disk.reads, 1);
        assert_eq!(disk.writes, 0);
    }

    #[test]
    fn m1_r087_mid_write_geometry_change_is_reported_as_uncertain_io() {
        let size = core::cell::Cell::new(4u64);
        let mut disk = R087GeometryDisk {
            capacity: &size, reads: 0, writes: 0,
            change_on_read: false, change_on_write: true,
        };
        let mut window = BlockWindow::new(&mut disk, 1, 2).unwrap();
        assert_eq!(window.write_sector(0, &[0x11; SECTOR_BYTES]), Err(BlockError::Io));
        drop(window);
        // Driver may already have written; callers must retire this mount,
        // not assume the operation was rolled back.
        assert_eq!(disk.writes, 1);
    }

    #[test]
    fn m1_r087_media_capacity_restoration_cannot_reactivate_stale_window() {
        let size = core::cell::Cell::new(4u64);
        let mut disk = R087GeometryDisk {
            capacity: &size, reads: 0, writes: 0,
            change_on_read: false, change_on_write: false,
        };
        let mut window = BlockWindow::new(&mut disk, 1, 2).unwrap();
        let mut out = [0xa5; SECTOR_BYTES];
        size.set(5);
        assert_eq!(window.read_sector(0, &mut out), Err(BlockError::Io));
        size.set(4); // A replacement disk can have identical capacity.
        assert_eq!(window.read_sector(0, &mut out), Err(BlockError::Io));
        assert_eq!(window.write_sector(0, &[0x21; SECTOR_BYTES]), Err(BlockError::Io));
        assert_eq!(out, [0xa5; SECTOR_BYTES]);
        drop(window);
        assert_eq!(disk.reads, 0);
        assert_eq!(disk.writes, 0);
    }

    #[test]
    fn m1_r087_mid_transfer_change_retires_window_even_after_restoration() {
        let size = core::cell::Cell::new(4u64);
        let mut disk = R087GeometryDisk {
            capacity: &size, reads: 0, writes: 0,
            change_on_read: true, change_on_write: false,
        };
        let mut window = BlockWindow::new(&mut disk, 1, 2).unwrap();
        let mut out = [0x19; SECTOR_BYTES];
        assert_eq!(window.read_sector(0, &mut out), Err(BlockError::Io));
        size.set(4);
        assert_eq!(window.read_sector(0, &mut out), Err(BlockError::Io));
        assert_eq!(out, [0x19; SECTOR_BYTES]);
        drop(window);
        assert_eq!(disk.reads, 1);
    }

    #[test]
    fn m1_r087_stable_geometry_retains_normal_partition_access() {
        let size = core::cell::Cell::new(4u64);
        let mut disk = R087GeometryDisk {
            capacity: &size, reads: 0, writes: 0,
            change_on_read: false, change_on_write: false,
        };
        let mut window = BlockWindow::new(&mut disk, 1, 2).unwrap();
        let mut out = [0u8; SECTOR_BYTES];
        assert_eq!(window.read_sector(1, &mut out), Ok(()));
        assert_eq!(out, [0x4d; SECTOR_BYTES]);
        assert_eq!(window.write_sector(0, &[0x11; SECTOR_BYTES]), Ok(()));
        drop(window);
        assert_eq!(disk.reads, 1);
        assert_eq!(disk.writes, 1);
    }


    // Real storage drivers can change capacity or dirty their DMA output.
    // Exercise the shared sector abstractions independently of any mock UI.
    struct R087bRemovable<'a> {
        capacity:&'a core::cell::Cell<u64>,
        reads:usize,writes:usize,
        change_on_read:bool,change_on_write:bool,fail_read:bool,
    }
    impl BlockDevice for R087bRemovable<'_>{
        fn sector_count(&self)->u64{self.capacity.get()}
        fn read_sector(&mut self,_lba:u64,out:&mut[u8;SECTOR_BYTES])
          ->Result<(),BlockError>{
            self.reads+=1;
            out.fill(0x6c);
            if self.change_on_read{self.capacity.set(6);}
            if self.fail_read{Err(BlockError::Io)}else{Ok(())}
        }
        fn write_sector(&mut self,_lba:u64,_bytes:&[u8;SECTOR_BYTES])
          ->Result<(),BlockError>{
            self.writes+=1;
            if self.change_on_write{self.capacity.set(6);}
            Ok(())
        }
    }

    #[test]
    fn m1_r087b_readonly_stages_failed_driver_output(){
        let capacity=core::cell::Cell::new(4);
        let mut disk=R087bRemovable{capacity:&capacity,reads:0,writes:0,
          change_on_read:false,change_on_write:false,fail_read:true};
        let mut view=ReadOnlyDevice::new(&mut disk);
        let mut out=[0xa5;SECTOR_BYTES];
        assert_eq!(view.read_sector(1,&mut out),Err(BlockError::Io));
        assert_eq!(out,[0xa5;SECTOR_BYTES]);
        drop(view);
        assert_eq!(disk.reads,1);assert_eq!(disk.writes,0);
    }

    #[test]
    fn m1_r087b_readonly_bounds_and_geometry_prevent_io(){
        let capacity=core::cell::Cell::new(4);
        let mut disk=R087bRemovable{capacity:&capacity,reads:0,writes:0,
          change_on_read:false,change_on_write:false,fail_read:false};
        let mut view=ReadOnlyDevice::new(&mut disk);
        let mut out=[0xa5;SECTOR_BYTES];
        assert_eq!(view.read_sector(4,&mut out),Err(BlockError::OutOfRange));
        capacity.set(3);
        assert_eq!(view.sector_count(),4);
        assert_eq!(view.read_sector(1,&mut out),Err(BlockError::Io));
        assert_eq!(view.write_sector(1,&[0;SECTOR_BYTES]),Err(BlockError::ReadOnly));
        assert_eq!(out,[0xa5;SECTOR_BYTES]);
        drop(view);
        assert_eq!(disk.reads,0);assert_eq!(disk.writes,0);
    }

    #[test]
    fn m1_r087b_readonly_detects_successful_mid_read_swap(){
        let capacity=core::cell::Cell::new(4);
        let mut disk=R087bRemovable{capacity:&capacity,reads:0,writes:0,
          change_on_read:true,change_on_write:false,fail_read:false};
        let mut view=ReadOnlyDevice::new(&mut disk);
        let mut out=[0xa5;SECTOR_BYTES];
        assert_eq!(view.read_sector(1,&mut out),Err(BlockError::Io));
        assert_eq!(out,[0xa5;SECTOR_BYTES]);
        drop(view);
        assert_eq!(disk.reads,1);
    }

    #[test]
    fn m1_r087b_multi_read_zeroes_output_on_mid_read_swap(){
        let capacity=core::cell::Cell::new(4);
        let mut disk=R087bRemovable{capacity:&capacity,reads:0,writes:0,
          change_on_read:true,change_on_write:false,fail_read:false};
        let mut out=[0xa5;SECTOR_BYTES*2];
        assert_eq!(read_sectors(&mut disk,1,&mut out),Err(BlockError::Io));
        assert_eq!(out,[0;SECTOR_BYTES*2]);
        assert_eq!(disk.reads,1);
    }

    #[test]
    fn m1_r087b_multi_write_stops_after_detecting_media_swap(){
        let capacity=core::cell::Cell::new(4);
        let mut disk=R087bRemovable{capacity:&capacity,reads:0,writes:0,
          change_on_read:false,change_on_write:true,fail_read:false};
        assert_eq!(write_sectors(&mut disk,0,&[0x55;SECTOR_BYTES*3]),
                   Err(BlockError::Io));
        assert_eq!(disk.writes,1);
    }


    #[test]
    fn m1_r088_read_only_probe_never_reactivates_after_media_restoration() {
        let capacity=core::cell::Cell::new(4);
        let mut disk=R087bRemovable{capacity:&capacity,reads:0,writes:0,
          change_on_read:false,change_on_write:false,fail_read:false};
        let mut probe=ReadOnlyDevice::new(&mut disk);
        let mut out=[0x5a;SECTOR_BYTES];
        capacity.set(3);
        assert_eq!(probe.read_sector(0,&mut out),Err(BlockError::Io));
        capacity.set(4);
        assert_eq!(probe.read_sector(0,&mut out),Err(BlockError::Io));
        assert_eq!(out,[0x5a;SECTOR_BYTES]);
        drop(probe);
        assert_eq!(disk.reads,0);assert_eq!(disk.writes,0);
    }

    #[test]
    fn m1_r088_read_only_probe_latches_mid_transfer_media_change() {
        let capacity=core::cell::Cell::new(4);
        let mut disk=R087bRemovable{capacity:&capacity,reads:0,writes:0,
          change_on_read:true,change_on_write:false,fail_read:false};
        let mut probe=ReadOnlyDevice::new(&mut disk);
        let mut out=[0xa5;SECTOR_BYTES];
        assert_eq!(probe.read_sector(0,&mut out),Err(BlockError::Io));
        capacity.set(4);
        assert_eq!(probe.read_sector(0,&mut out),Err(BlockError::Io));
        assert_eq!(out,[0xa5;SECTOR_BYTES]);
        drop(probe);
        assert_eq!(disk.reads,1);
    }

    #[test]
    fn m1_r088_sync_requires_explicit_block_driver_support(){
        let mut disk=RamDisk{bytes:[0;SECTOR_BYTES*4],readonly:false};
        assert_eq!(disk.sync(),Err(BlockError::Io));
        let mut window=BlockWindow::new(&mut disk,1,2).unwrap();
        assert_eq!(window.sync(),Err(BlockError::Io));
        drop(window);
        let mut probe=ReadOnlyDevice::new(&mut disk);
        assert_eq!(probe.sync(),Err(BlockError::ReadOnly));
    }

    #[test]
    fn m1_r088_bounded_sync_forwards_only_for_unchanged_media(){
        struct FlushDisk<'a>{capacity:&'a core::cell::Cell<u64>,calls:u32}
        impl BlockDevice for FlushDisk<'_>{
            fn sector_count(&self)->u64{self.capacity.get()}
            fn read_sector(&mut self,_:u64,out:&mut[u8;SECTOR_BYTES])
              ->Result<(),BlockError>{out.fill(0);Ok(())}
            fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])
              ->Result<(),BlockError>{Ok(())}
            fn sync(&mut self)->Result<(),BlockError>{self.calls+=1;Ok(())}
        }
        let capacity=core::cell::Cell::new(4);
        let mut disk=FlushDisk{capacity:&capacity,calls:0};
        let mut window=BlockWindow::new(&mut disk,1,2).unwrap();
        assert_eq!(window.sync(),Ok(()));
        capacity.set(5);
        assert_eq!(window.sync(),Err(BlockError::Io));
        capacity.set(4);
        assert_eq!(window.sync(),Err(BlockError::Io));
        drop(window);
        assert_eq!(disk.calls,1);
    }

    struct R089UncertainDisk {
        sectors:[[u8;SECTOR_BYTES];4],
        writes:u32, reads:u32, syncs:u32,
        fail_write:bool,fail_sync:bool,
    }
    impl R089UncertainDisk{
        fn new()->Self{
            Self{sectors:[[0;SECTOR_BYTES];4],writes:0,reads:0,
                 syncs:0,fail_write:false,fail_sync:false}
        }
    }
    impl BlockDevice for R089UncertainDisk{
        fn sector_count(&self)->u64{4}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
          ->Result<(),BlockError>{
            check_range(4,lba,1)?;
            self.reads+=1;*out=self.sectors[lba as usize];Ok(())
        }
        fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])
          ->Result<(),BlockError>{
            check_range(4,lba,1)?;
            self.writes+=1;
            self.sectors[lba as usize]=*data; // Partial/torn media possible.
            if self.fail_write{Err(BlockError::Io)}else{Ok(())}
        }
        fn sync(&mut self)->Result<(),BlockError>{
            self.syncs+=1;
            if self.fail_sync{Err(BlockError::Io)}else{Ok(())}
        }
    }
    #[test]
    fn m1_r089_partition_window_retires_after_failed_sector_write(){
        let mut disk=R089UncertainDisk::new();
        disk.fail_write=true;
        let mut mounted=BlockWindow::new(&mut disk,1,2).unwrap();
        assert_eq!(mounted.write_sector(0,&[0x77;SECTOR_BYTES]),Err(BlockError::Io));
        assert_eq!(mounted.write_sector(0,&[0x22;SECTOR_BYTES]),Err(BlockError::Io));
        assert_eq!(mounted.read_sector(0,&mut[0;SECTOR_BYTES]),Err(BlockError::Io));
        assert_eq!(mounted.sync(),Err(BlockError::Io));
        drop(mounted);
        assert_eq!(disk.writes,1);
        assert_eq!(disk.reads,0);
        assert_eq!(disk.syncs,0);
        // An explicit fresh mount is the ONLY way to retry on inspected media.
        disk.fail_write=false;
        let mut fresh=BlockWindow::new(&mut disk,1,2).unwrap();
        assert_eq!(fresh.write_sector(0,&[0x55;SECTOR_BYTES]),Ok(()));
        drop(fresh);
        assert_eq!(disk.writes,2);
    }
    #[test]
    fn m1_r089_partition_window_retires_after_uncertain_flush(){
        let mut disk=R089UncertainDisk::new();
        disk.fail_sync=true;
        let mut mounted=BlockWindow::new(&mut disk,1,2).unwrap();
        assert_eq!(mounted.sync(),Err(BlockError::Io));
        assert_eq!(mounted.sync(),Err(BlockError::Io));
        assert_eq!(mounted.write_sector(0,&[0;SECTOR_BYTES]),Err(BlockError::Io));
        assert_eq!(mounted.read_sector(0,&mut[0;SECTOR_BYTES]),Err(BlockError::Io));
        drop(mounted);
        assert_eq!(disk.syncs,1);assert_eq!(disk.writes,0);assert_eq!(disk.reads,0);
        disk.fail_sync=false;
        let mut fresh=BlockWindow::new(&mut disk,1,2).unwrap();
        assert_eq!(fresh.sync(),Ok(()));
        drop(fresh);
        assert_eq!(disk.syncs,2);
    }
    #[test]
    fn m1_r089_partition_window_stable_write_flush_and_read_survive(){
        let mut disk=R089UncertainDisk::new();
        let mut view=BlockWindow::new(&mut disk,1,2).unwrap();
        let bytes=[0x42;SECTOR_BYTES];
        assert_eq!(view.write_sector(1,&bytes),Ok(()));
        assert_eq!(view.sync(),Ok(()));
        let mut out=[0;SECTOR_BYTES];
        assert_eq!(view.read_sector(1,&mut out),Ok(()));
        assert_eq!(out,bytes);
        drop(view);
        assert_eq!(disk.writes,1);assert_eq!(disk.syncs,1);assert_eq!(disk.reads,1);
    }

    #[test]
    fn m1_r089_parent_inspection_rejects_late_capacity_change(){
        let size=core::cell::Cell::new(4u64);
        let mut disk=R087GeometryDisk{
            capacity:&size,reads:0,writes:0,
            change_on_read:false,change_on_write:false,
        };
        let mut mounted=BlockWindow::new(&mut disk,1,2).unwrap();
        let mut seen=[0u8;SECTOR_BYTES];
        assert_eq!(mounted.validate_parent(|device|{
            device.read_sector(0,&mut seen)?;
            size.set(5); // after the last successful read
            Ok::<(),BlockError>(())
        },|e|e),Err(BlockError::Io));
        size.set(4);
        assert_eq!(mounted.read_sector(0,&mut seen),Err(BlockError::Io));
        drop(mounted);
        assert_eq!(disk.reads,1);
    }
    #[test]
    fn m1_r089_parent_inspection_rejects_stale_mount_before_callback(){
        let size=core::cell::Cell::new(4u64);
        let mut disk=R087GeometryDisk{
            capacity:&size,reads:0,writes:0,
            change_on_read:false,change_on_write:false,
        };
        let mut mounted=BlockWindow::new(&mut disk,1,2).unwrap();
        size.set(3);
        let mut called=false;
        assert_eq!(mounted.validate_parent(|_|{
            called=true;Ok::<(),BlockError>(())
        },|e|e),Err(BlockError::Io));
        assert!(!called);
        size.set(4);
        assert_eq!(mounted.validate_parent(|_|Ok::<(),BlockError>(()),|e|e),
                   Err(BlockError::Io));
        drop(mounted);
        assert_eq!(disk.reads,0);
        assert_eq!(disk.writes,0);
    }
    #[test]
    fn m1_r087b_stable_readonly_and_multi_sector_operations(){
        let capacity=core::cell::Cell::new(4);
        let mut disk=R087bRemovable{capacity:&capacity,reads:0,writes:0,
          change_on_read:false,change_on_write:false,fail_read:false};
        {
            let mut view=ReadOnlyDevice::new(&mut disk);
            let mut out=[0u8;SECTOR_BYTES];
            assert_eq!(view.read_sector(3,&mut out),Ok(()));
            assert_eq!(out,[0x6c;SECTOR_BYTES]);
        }
        let mut out=[0u8;SECTOR_BYTES*2];
        assert_eq!(read_sectors(&mut disk,1,&mut out),Ok(()));
        assert_eq!(out,[0x6c;SECTOR_BYTES*2]);
        assert_eq!(write_sectors(&mut disk,2,&[0x42;SECTOR_BYTES*2]),Ok(()));
        assert_eq!(disk.reads,3);assert_eq!(disk.writes,2);
    }

}


/// Read-only filesystem extent view over a device. This adapter is suitable
/// for mounting *validated* on-disk extent lists; it never formats or writes.
pub struct ExtentReader<'a, D: BlockDevice> {
    device: &'a mut D,
    extents: &'a [crate::agent_01_fs_extents::Extent],
    logical_sectors: u64,
    parent_capacity:u64,
    invalidated:bool,
}
impl<'a, D: BlockDevice> ExtentReader<'a, D> {
    pub fn new(device: &'a mut D, extents: &'a [crate::agent_01_fs_extents::Extent])
        -> Result<Self, BlockError>
    {
        let parent_capacity=device.sector_count();
        let count = crate::agent_01_fs_extents::validate_extents(
            extents,parent_capacity
        ).map_err(|_| BlockError::OutOfRange)?;
        if count == 0 { return Err(BlockError::OutOfRange); }
        Ok(Self { device, extents, logical_sectors: count,
                  parent_capacity,invalidated:false })
    }
    fn check_parent(&mut self)->Result<(),BlockError>{
        if self.invalidated{return Err(BlockError::Io);}
        if self.device.sector_count()!=self.parent_capacity{
            self.invalidated=true;
            return Err(BlockError::Io);
        }
        Ok(())
    }
    fn physical_lba(&mut self, logical_lba: u64) -> Result<u64, BlockError> {
        self.check_parent()?;
        crate::agent_01_fs_extents::logical_to_physical(
            self.extents, self.parent_capacity, logical_lba
        ).map_err(|_| BlockError::OutOfRange)?
            .ok_or(BlockError::OutOfRange)
    }
}
impl<D: BlockDevice> BlockDevice for ExtentReader<'_, D> {
    fn sector_count(&self) -> u64 { self.logical_sectors }

    fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES])
        -> Result<(), BlockError>
    {
        let mapped = self.physical_lba(lba)?;
        let mut staged=[0u8;SECTOR_BYTES];
        let transferred=self.device.read_sector(mapped,&mut staged);
        self.check_parent()?;
        transferred?;
        out.copy_from_slice(&staged);
        Ok(())
    }

    fn write_sector(&mut self, _lba: u64, _data: &[u8; SECTOR_BYTES])
        -> Result<(), BlockError>
    {
        Err(BlockError::ReadOnly)
    }
    fn sync(&mut self)->Result<(),BlockError>{Err(BlockError::ReadOnly)}
}

#[cfg(test)]
mod extent_reader_tests {
    use super::*;
    use crate::agent_01_fs_extents::Extent;

    struct Device { bytes: [[u8; SECTOR_BYTES]; 8], writes: u32 }
    impl BlockDevice for Device {
        fn sector_count(&self) -> u64 { 8 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES])
            -> Result<(), BlockError>
        {
            check_range(8, lba, 1)?;
            *out = self.bytes[lba as usize];
            Ok(())
        }
        fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_BYTES])
            -> Result<(), BlockError>
        {
            check_range(8, lba, 1)?;
            self.writes += 1;
            self.bytes[lba as usize] = *data;
            Ok(())
        }
    }
    fn device() -> Device {
        let mut disk = Device { bytes: [[0; SECTOR_BYTES]; 8], writes: 0 };
        for (i, sector) in disk.bytes.iter_mut().enumerate() { sector[0] = i as u8; }
        disk
    }
    #[test]
    fn discontiguous_extent_mapping_reads_correct_sectors() {
        let mut disk = device();
        let extents = [Extent { start: 1, blocks: 2 }, Extent { start: 5, blocks: 2 }];
        let mut view = ExtentReader::new(&mut disk, &extents).unwrap();
        assert_eq!(view.sector_count(), 4);
        let mut sector = [0; SECTOR_BYTES];
        for (i, expected) in [1, 2, 5, 6].iter().enumerate() {
            view.read_sector(i as u64, &mut sector).unwrap();
            assert_eq!(sector[0], *expected);
        }
        assert_eq!(view.read_sector(4, &mut sector), Err(BlockError::OutOfRange));
    }
    #[test]
    fn metadata_corruption_is_rejected_before_first_read() {
        let mut disk = device();
        let bad = [Extent { start: 2, blocks: 3 }, Extent { start: 4, blocks: 2 }];
        assert!(matches!(ExtentReader::new(&mut disk, &bad), Err(BlockError::OutOfRange)));
        let overflow = [Extent { start: u64::MAX, blocks: 2 }];
        assert!(matches!(ExtentReader::new(&mut disk, &overflow), Err(BlockError::OutOfRange)));
        assert_eq!(disk.writes, 0);
    }

    #[test]
    fn m1_r088_extent_view_stays_revoked_after_capacity_restored(){
        struct Removable<'a>{size:&'a core::cell::Cell<u64>,reads:u32}
        impl BlockDevice for Removable<'_>{
            fn sector_count(&self)->u64{self.size.get()}
            fn read_sector(&mut self,_lba:u64,out:&mut[u8;SECTOR_BYTES])
              ->Result<(),BlockError>{self.reads+=1;out.fill(0x78);Ok(())}
            fn write_sector(&mut self,_lba:u64,_:&[u8;SECTOR_BYTES])
              ->Result<(),BlockError>{Err(BlockError::ReadOnly)}
        }
        let size=core::cell::Cell::new(8);
        let mut disk=Removable{size:&size,reads:0};
        let extents=[Extent{start:1,blocks:2}];
        let mut view=ExtentReader::new(&mut disk,&extents).unwrap();
        let mut out=[0x39;SECTOR_BYTES];
        size.set(9);
        assert_eq!(view.read_sector(0,&mut out),Err(BlockError::Io));
        size.set(8);
        assert_eq!(view.read_sector(0,&mut out),Err(BlockError::Io));
        assert_eq!(out,[0x39;SECTOR_BYTES]);
        drop(view);
        assert_eq!(disk.reads,0);
    }

    #[test]
    fn m1_r088_extent_read_stages_partial_driver_failure(){
        struct Dirty;
        impl BlockDevice for Dirty{
            fn sector_count(&self)->u64{8}
            fn read_sector(&mut self,_lba:u64,out:&mut[u8;SECTOR_BYTES])
              ->Result<(),BlockError>{out.fill(0xfa);Err(BlockError::Io)}
            fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])
              ->Result<(),BlockError>{Err(BlockError::ReadOnly)}
        }
        let mut disk=Dirty;
        let extents=[Extent{start:1,blocks:2}];
        let mut view=ExtentReader::new(&mut disk,&extents).unwrap();
        let mut out=[0x39;SECTOR_BYTES];
        assert_eq!(view.read_sector(0,&mut out),Err(BlockError::Io));
        assert_eq!(out,[0x39;SECTOR_BYTES]);
    }
    #[test]
    fn extent_view_is_strictly_read_only() {
        let mut disk = device();
        let extents = [Extent { start: 1, blocks: 2 }];
        let mut view = ExtentReader::new(&mut disk, &extents).unwrap();
        assert_eq!(view.write_sector(0, &[1; SECTOR_BYTES]), Err(BlockError::ReadOnly));
        drop(view);
        assert_eq!(disk.writes, 0);
    }
}


/// Batch 1: read a byte range through a validated multi-extent block view.
/// All bounds are checked before the first device read or output mutation.
/// Mid-transfer device failure wipes the whole output; range-preflight
/// rejection still preserves the caller's original buffer.
pub fn read_extent_bytes<D: BlockDevice>(
    device: &mut D, extents: &[crate::agent_01_fs_extents::Extent],
    offset: u64, out: &mut [u8],
) -> Result<(), BlockError> {
    let mut reader = ExtentReader::new(device, extents)?;
    let total = reader.sector_count().checked_mul(SECTOR_BYTES as u64)
        .ok_or(BlockError::OutOfRange)?;
    let length = u64::try_from(out.len()).map_err(|_| BlockError::OutOfRange)?;
    if out.is_empty() || offset > total || length > total - offset {
        return Err(BlockError::OutOfRange);
    }
    let mut cursor = offset;
    let mut written=0;
    let mut sector = [0u8; SECTOR_BYTES];
    while written<out.len() {
        if let Err(error)=reader.read_sector(cursor / SECTOR_BYTES as u64,&mut sector){
            out.fill(0);
            return Err(error);
        }
        let position = (cursor % SECTOR_BYTES as u64) as usize;
        let count = (out.len()-written).min(SECTOR_BYTES-position);
        out[written..written+count].copy_from_slice(&sector[position..position+count]);
        cursor += count as u64; // prior range preflight proves no overflow
        written+=count;
    }
    Ok(())
}
#[cfg(test)]
mod batch1_extent_byte_tests {
    use super::*;
    use crate::agent_01_fs_extents::Extent;
    struct Disk([[u8; SECTOR_BYTES]; 6]);
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 { 6 }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_BYTES])
            -> Result<(), BlockError> {
            check_range(6, lba, 1)?;
            *out = self.0[lba as usize];
            Ok(())
        }
        fn write_sector(&mut self, _lba: u64, _data: &[u8; SECTOR_BYTES])
            -> Result<(), BlockError> { Err(BlockError::ReadOnly) }
    }
    #[test]
    fn crosses_disjoint_physical_extents_at_byte_boundary() {
        let mut disk = Disk([[0; SECTOR_BYTES]; 6]);
        disk.0[1].fill(b'a');
        disk.0[4].fill(b'b');
        let mut out = [0; 5];
        read_extent_bytes(&mut disk, &[Extent{start:1,blocks:1},
            Extent{start:4,blocks:1}], 510, &mut out).unwrap();
        assert_eq!(&out, b"aabbb");
    }

    #[test]
    fn m1_r088_extent_byte_read_clears_completed_prefix_on_second_sector_failure(){
        struct DirtySecond{reads:u32}
        impl BlockDevice for DirtySecond{
            fn sector_count(&self)->u64{6}
            fn read_sector(&mut self,_:u64,out:&mut[u8;SECTOR_BYTES])
              ->Result<(),BlockError>{
                self.reads+=1;
                if self.reads==2{out.fill(0xee);return Err(BlockError::Io);}
                out.fill(0x2a);Ok(())
            }
            fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])
              ->Result<(),BlockError>{Err(BlockError::ReadOnly)}
        }
        let mut disk=DirtySecond{reads:0};
        let mut out=[0xa5;SECTOR_BYTES+3];
        assert_eq!(read_extent_bytes(&mut disk,&[Extent{start:1,blocks:2}],
                    0,&mut out),Err(BlockError::Io));
        assert_eq!(out,[0;SECTOR_BYTES+3]);
        assert_eq!(disk.reads,2);
    }
    #[test]
    fn preflight_rejects_overrun_without_mutating_caller_buffer() {
        let mut disk = Disk([[0; SECTOR_BYTES]; 6]);
        let mut out = [0xa5; 4];
        assert_eq!(read_extent_bytes(&mut disk,
            &[Extent{start:4,blocks:1}], 510, &mut out),
            Err(BlockError::OutOfRange));
        assert_eq!(out, [0xa5;4]);
    }
}


// R089: constructor must reject a removable device that changes size while
// its selection is checked, without issuing a read or a write.
#[cfg(test)]
mod r089_window_creation_tests {
    use super::*;
    use core::cell::Cell;
    struct ChangingOnPreflight {
        samples: Cell<usize>,
        transfers: Cell<usize>,
    }
    impl BlockDevice for ChangingOnPreflight {
        fn sector_count(&self)->u64 {
            let count=self.samples.get();
            self.samples.set(count+1);
            if count==0 { 8 } else { 9 }
        }
        fn read_sector(&mut self,_lba:u64,_out:&mut[u8;SECTOR_BYTES])
            ->Result<(),BlockError> {
            self.transfers.set(self.transfers.get()+1);
            Ok(())
        }
        fn write_sector(&mut self,_lba:u64,_data:&[u8;SECTOR_BYTES])
            ->Result<(),BlockError> {
            self.transfers.set(self.transfers.get()+1);
            Ok(())
        }
    }
    #[test]
    fn geometry_change_during_window_creation_is_rejected_before_io() {
        let mut disk=ChangingOnPreflight{
            samples:Cell::new(0),transfers:Cell::new(0)
        };
        assert!(matches!(BlockWindow::new(&mut disk,1,2),Err(BlockError::Io)));
        assert_eq!(disk.samples.get(),2);
        assert_eq!(disk.transfers.get(),0);
    }
}


#[cfg(test)]
mod r090_block_acceptance {
    use super::*;
    struct Disk{
        data:[[u8;SECTOR_BYTES];6],
        reads:usize,writes:usize,flushes:usize,
        fail_read:Option<u64>,fail_sync:bool,
    }
    impl Disk{
        fn new()->Self{
            Self{data:[[0;SECTOR_BYTES];6],reads:0,writes:0,flushes:0,
                 fail_read:None,fail_sync:false}
        }
    }
    impl BlockDevice for Disk{
        fn sector_count(&self)->u64{6}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
            self.reads+=1;
            if self.fail_read==Some(lba){out[0]=0x55;return Err(BlockError::Io);}
            *out=*self.data.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self,lba:u64,input:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
            let slot=self.data.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?;
            *slot=*input;self.writes+=1;Ok(())
        }
        fn sync(&mut self)->Result<(),BlockError>{
            self.flushes+=1;
            if self.fail_sync{Err(BlockError::Io)}else{Ok(())}
        }
    }
    /// R090 B01: reject empty multisector read
    #[test] fn r090_b01_reject_empty_multisector_read(){let mut d=Disk::new();let mut out=[];assert_eq!(read_sectors(&mut d,0,&mut out),Err(BlockError::OutOfRange));assert_eq!(d.reads,0);}
    /// R090 B02: reject partial sector read
    #[test] fn r090_b02_reject_partial_sector_read(){let mut d=Disk::new();let mut out=[7;SECTOR_BYTES+1];assert_eq!(read_sectors(&mut d,0,&mut out),Err(BlockError::OutOfRange));assert_eq!(out,[7;SECTOR_BYTES+1]);}
    /// R090 B03: reject empty multisector write
    #[test] fn r090_b03_reject_empty_multisector_write(){let mut d=Disk::new();assert_eq!(write_sectors(&mut d,0,&[]),Err(BlockError::OutOfRange));assert_eq!(d.writes,0);}
    /// R090 B04: reject partial sector write
    #[test] fn r090_b04_reject_partial_sector_write(){let mut d=Disk::new();assert_eq!(write_sectors(&mut d,0,&[1;SECTOR_BYTES+1]),Err(BlockError::OutOfRange));assert_eq!(d.writes,0);}
    /// R090 B05: read last sector at exact boundary
    #[test] fn r090_b05_read_last_sector_at_exact_boundary(){let mut d=Disk::new();d.data[5]=[0x71;SECTOR_BYTES];let mut out=[0;SECTOR_BYTES];read_sectors(&mut d,5,&mut out).unwrap();assert_eq!(out,[0x71;SECTOR_BYTES]);}
    /// R090 B06: write last sector at exact boundary
    #[test] fn r090_b06_write_last_sector_at_exact_boundary(){let mut d=Disk::new();write_sectors(&mut d,5,&[0x82;SECTOR_BYTES]).unwrap();assert_eq!(d.data[5],[0x82;SECTOR_BYTES]);assert_eq!(d.data[4],[0;SECTOR_BYTES]);}
    /// R090 B07: read overrun fails before publishing
    #[test] fn r090_b07_read_overrun_fails_before_publishing(){let mut d=Disk::new();let mut out=[0xa5;SECTOR_BYTES*2];assert_eq!(read_sectors(&mut d,5,&mut out),Err(BlockError::OutOfRange));assert_eq!(out,[0xa5;SECTOR_BYTES*2]);assert_eq!(d.reads,0);}
    /// R090 B08: write overrun fails without partial sector
    #[test] fn r090_b08_write_overrun_fails_without_partial_sector(){let mut d=Disk::new();assert_eq!(write_sectors(&mut d,5,&[0xff;SECTOR_BYTES*2]),Err(BlockError::OutOfRange));assert_eq!(d.writes,0);}
    /// R090 B09: three sector roundtrip covers middle
    #[test] fn r090_b09_three_sector_roundtrip_covers_middle(){let mut d=Disk::new();let mut data=[0;SECTOR_BYTES*3];for (i,x) in data.iter_mut().enumerate(){*x=(i%251) as u8;}write_sectors(&mut d,2,&data).unwrap();let mut readback=[0;SECTOR_BYTES*3];read_sectors(&mut d,2,&mut readback).unwrap();assert_eq!(readback,data);}
    /// R090 B10: midtransfer failure scrubs all readback
    #[test] fn r090_b10_midtransfer_failure_scrubs_all_readback(){let mut d=Disk::new();d.data[0]=[1;SECTOR_BYTES];d.fail_read=Some(1);let mut out=[0xee;SECTOR_BYTES*2];assert_eq!(read_sectors(&mut d,0,&mut out),Err(BlockError::Io));assert_eq!(out,[0;SECTOR_BYTES*2]);}
    /// R090 B11: first sector failure scrubs all readback
    #[test] fn r090_b11_first_sector_failure_scrubs_all_readback(){let mut d=Disk::new();d.fail_read=Some(0);let mut out=[0xee;SECTOR_BYTES*3];assert_eq!(read_sectors(&mut d,0,&mut out),Err(BlockError::Io));assert_eq!(out,[0;SECTOR_BYTES*3]);}
    /// R090 B12: window exposes bounded sector count
    #[test] fn r090_b12_window_exposes_bounded_sector_count(){let mut d=Disk::new();let win=BlockWindow::new(&mut d,2,3).unwrap();assert_eq!(win.sector_count(),3);assert_eq!(win.sector_start(),2);}
    /// R090 B13: window write does not touch adjacent sector
    #[test] fn r090_b13_window_write_does_not_touch_adjacent_sector(){let mut d=Disk::new();{let mut win=BlockWindow::new(&mut d,2,2).unwrap();win.write_sector(1,&[0x39;SECTOR_BYTES]).unwrap();}assert_eq!(d.data[3],[0x39;SECTOR_BYTES]);assert_eq!(d.data[1],[0;SECTOR_BYTES]);assert_eq!(d.data[4],[0;SECTOR_BYTES]);}
    /// R090 B14: window read uses physical start offset
    #[test] fn r090_b14_window_read_uses_physical_start_offset(){let mut d=Disk::new();d.data[4]=[0x44;SECTOR_BYTES];let mut out=[0;SECTOR_BYTES];{let mut win=BlockWindow::new(&mut d,3,2).unwrap();win.read_sector(1,&mut out).unwrap();}assert_eq!(out,[0x44;SECTOR_BYTES]);}
    /// R090 B15: window rejects outside read without driver access
    #[test] fn r090_b15_window_rejects_outside_read_without_driver_access(){let mut d=Disk::new();{let mut win=BlockWindow::new(&mut d,1,2).unwrap();assert_eq!(win.read_sector(2,&mut[0;SECTOR_BYTES]),Err(BlockError::OutOfRange));}assert_eq!(d.reads,0);}
    /// R090 B16: window rejects outside write without driver access
    #[test] fn r090_b16_window_rejects_outside_write_without_driver_access(){let mut d=Disk::new();{let mut win=BlockWindow::new(&mut d,1,2).unwrap();assert_eq!(win.write_sector(2,&[1;SECTOR_BYTES]),Err(BlockError::OutOfRange));}assert_eq!(d.writes,0);}
    /// R090 B17: readonly wrapper denies device writes
    #[test] fn r090_b17_readonly_wrapper_denies_device_writes(){let mut d=Disk::new();{let mut view=ReadOnlyDevice::new(&mut d);assert_eq!(view.write_sector(0,&[1;SECTOR_BYTES]),Err(BlockError::ReadOnly));}assert_eq!(d.writes,0);}
    /// R090 B18: readonly wrapper denies flush
    #[test] fn r090_b18_readonly_wrapper_denies_flush(){let mut d=Disk::new();{let mut view=ReadOnlyDevice::new(&mut d);assert_eq!(view.sync(),Err(BlockError::ReadOnly));}assert_eq!(d.flushes,0);}
    /// R090 B19: readonly failed driver does not taint caller
    #[test] fn r090_b19_readonly_failed_driver_does_not_taint_caller(){let mut d=Disk::new();d.fail_read=Some(2);let mut out=[0x72;SECTOR_BYTES];{let mut view=ReadOnlyDevice::new(&mut d);assert_eq!(view.read_sector(2,&mut out),Err(BlockError::Io));}assert_eq!(out,[0x72;SECTOR_BYTES]);}
    /// R090 B20: failed flush retires window permanently
    #[test] fn r090_b20_failed_flush_retires_window_permanently(){let mut d=Disk::new();d.fail_sync=true;{let mut win=BlockWindow::new(&mut d,1,2).unwrap();assert_eq!(win.sync(),Err(BlockError::Io));assert_eq!(win.read_sector(0,&mut[0;SECTOR_BYTES]),Err(BlockError::Io));}assert_eq!(d.flushes,1);}
}

#[cfg(test)]
mod r091_failed_read_retires_views {
 use super::*;
 struct Faults { calls:usize }
 impl BlockDevice for Faults {
  fn sector_count(&self)->u64{16}
  fn read_sector(&mut self,_lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   self.calls+=1;out[0]=0xaa;Err(BlockError::Io)
  }
  fn write_sector(&mut self,_lba:u64,_input:&[u8;SECTOR_BYTES])->Result<(),BlockError>{Err(BlockError::Io)}
 }
 #[test]fn failed_bounded_read_poison_is_permanent(){
  let mut disk=Faults{calls:0};let mut out=[0x42;SECTOR_BYTES];
  let mut win=BlockWindow::new(&mut disk,3,4).unwrap();
  assert_eq!(win.read_sector(0,&mut out),Err(BlockError::Io));
  assert_eq!(out,[0x42;SECTOR_BYTES]);
  assert_eq!(win.read_sector(0,&mut out),Err(BlockError::Io));
  drop(win);assert_eq!(disk.calls,1);
 }
 #[test]fn failed_readonly_read_poison_is_permanent(){
  let mut disk=Faults{calls:0};let mut out=[0x42;SECTOR_BYTES];
  let mut read_only=ReadOnlyDevice::new(&mut disk);
  assert_eq!(read_only.read_sector(0,&mut out),Err(BlockError::Io));
  assert_eq!(out,[0x42;SECTOR_BYTES]);
  assert_eq!(read_only.read_sector(0,&mut out),Err(BlockError::Io));
  drop(read_only);assert_eq!(disk.calls,1);
 }
}

#[cfg(test)]
mod r091_parent_identity_retirement {
 use super::*;
 struct Disk {reads:usize}
 impl BlockDevice for Disk {
  fn sector_count(&self)->u64{8}
  fn read_sector(&mut self,_lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   self.reads+=1;out.fill(0);Ok(())
  }
  fn write_sector(&mut self,_lba:u64,_data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   Ok(())
  }
 }
 #[test]fn failed_identity_check_retires_mount_even_if_geometry_unchanged(){
  let mut d=Disk{reads:0};
  let mut w=BlockWindow::new(&mut d,1,2).unwrap();
  let failed=w.validate_parent(|_view|Err::<(),u8>(42),|_|43);
  assert_eq!(failed,Err(42));
  assert_eq!(w.read_sector(0,&mut[0;SECTOR_BYTES]),Err(BlockError::Io));
  drop(w);
  assert_eq!(d.reads,0);
 }
 #[test]fn successful_identity_check_allows_bounded_read(){
  let mut d=Disk{reads:0};
  let mut w=BlockWindow::new(&mut d,1,2).unwrap();
  assert_eq!(w.validate_parent(|_view|Ok::<(),u8>(()),|_|43),Ok(()));
  assert_eq!(w.read_sector(0,&mut[0;SECTOR_BYTES]),Ok(()));
  drop(w);
  assert_eq!(d.reads,1);
 }
}


#[cfg(test)]
mod m1_batch01_w2_block_tests{
 use super::*;
 #[test]fn b01_05_single_last_sector_is_in_bounds(){
  assert_eq!(check_range(8,7,1),Ok(()));
 }
 #[test]fn b01_06_zero_sector_window_is_denied(){
  assert_eq!(check_range(8,4,0),Err(BlockError::OutOfRange));
 }
 #[test]fn b01_07_overflowing_partition_window_is_denied(){
  assert_eq!(check_range(u64::MAX,u64::MAX-1,3),Err(BlockError::OutOfRange));
 }
}
#[cfg(test)]
mod m1_gate20_w4_partition_tests {
 use super::*;
 #[test]fn exact_lba_window_boundary_and_overflow_fail_closed(){
  assert_eq!(check_range(128,127,1),Ok(()));
  assert_eq!(check_range(128,127,2),Err(BlockError::OutOfRange));
  assert_eq!(check_range(128,128,1),Err(BlockError::OutOfRange));
  assert_eq!(check_range(u64::MAX,u64::MAX-2,4),Err(BlockError::OutOfRange));
 }
}


/// Bounded all-or-nothing read for critical disk/partition metadata.
/// A driver can dirty its sector buffer before returning an error: stage all
/// transfers privately and publish none until every capacity check succeeds.
pub const MAX_STAGED_READ_SECTORS:usize=8;
pub fn read_sectors_atomic<D:BlockDevice>(
 device:&mut D,start:u64,out:&mut[u8])->Result<(),BlockError>{
 if out.is_empty()||out.len()%SECTOR_BYTES!=0||
    out.len()/SECTOR_BYTES>MAX_STAGED_READ_SECTORS{
  return Err(BlockError::OutOfRange);
 }
 let count=(out.len()/SECTOR_BYTES) as u64;
 let capacity=device.sector_count();
 check_range(capacity,start,count)?;
 let mut bytes=[0u8;SECTOR_BYTES*MAX_STAGED_READ_SECTORS];
 for i in 0..count as usize{
  if device.sector_count()!=capacity{return Err(BlockError::Io);}
  let mut sector=[0u8;SECTOR_BYTES];
  device.read_sector(start+i as u64,&mut sector)?;
  if device.sector_count()!=capacity{return Err(BlockError::Io);}
  let off=i*SECTOR_BYTES;bytes[off..off+SECTOR_BYTES].copy_from_slice(&sector);
 }
 out.copy_from_slice(&bytes[..out.len()]);
 Ok(())
}
#[cfg(test)]
mod m1_batch04_w2_block_tests{
 use super::*;
 struct Disk{fail:Option<u64>,reads:u32}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{3}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   self.reads+=1;out.fill(lba as u8+1);
   if self.fail==Some(lba){return Err(BlockError::Io);}
   Ok(())
  }
  fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   Err(BlockError::ReadOnly)
  }
 }
 #[test]fn staging_never_exposes_second_sector_driver_failure(){
  let mut d=Disk{fail:Some(1),reads:0};
  let mut output=[55u8;SECTOR_BYTES*2];
  assert_eq!(read_sectors_atomic(&mut d,0,&mut output),Err(BlockError::Io));
  assert_eq!(output,[55u8;SECTOR_BYTES*2]);
  d.fail=None;
  assert_eq!(read_sectors_atomic(&mut d,0,&mut output),Ok(()));
  assert_eq!(&output[..SECTOR_BYTES],[1u8;SECTOR_BYTES]);
  assert_eq!(&output[SECTOR_BYTES..],[2u8;SECTOR_BYTES]);
  let before=d.reads;
  assert_eq!(read_sectors_atomic(&mut d,2,&mut output),Err(BlockError::OutOfRange));
  assert_eq!(d.reads,before);
 }
}


/// P0 follow-up W4: require two identical bounded disk observations before
/// publishing metadata bytes. Detects geometry changes, torn driver output
/// and reads that change between probes. This is not cryptographic media
/// authentication or a substitute for a COW journal and durable flush.
pub fn read_stable_sectors<D:BlockDevice>(device:&mut D,start:u64,
 out:&mut[u8])->Result<(),BlockError>{
 if out.is_empty()||out.len()%SECTOR_BYTES!=0||
    out.len()>MAX_STAGED_READ_SECTORS*SECTOR_BYTES{
  return Err(BlockError::OutOfRange);
 }
 let mut first=[0u8;MAX_STAGED_READ_SECTORS*SECTOR_BYTES];
 let mut second=[0u8;MAX_STAGED_READ_SECTORS*SECTOR_BYTES];
 let n=out.len();
 read_sectors_atomic(device,start,&mut first[..n])?;
 read_sectors_atomic(device,start,&mut second[..n])?;
 if first[..n]!=second[..n]{return Err(BlockError::Io);}
 out.copy_from_slice(&first[..n]);Ok(())
}
#[cfg(test)]
mod kernel_p0_next_w4_storage_tests{
 use super::*;
 struct Disk{reads:u8,change:bool,fail:bool}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{4}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   self.reads+=1;
   if self.fail&&self.reads==2{return Err(BlockError::Io);}
   out.fill(lba as u8+if self.change&&self.reads>=2{2}else{1});
   Ok(())
  }
  fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   Err(BlockError::ReadOnly)
  }
 }
 #[test]fn unchanging_read_produces_full_sector_and_detects_torn_second_pass(){
  let mut good=Disk{reads:0,change:false,fail:false};
  let mut out=[9u8;SECTOR_BYTES];
  assert_eq!(read_stable_sectors(&mut good,1,&mut out),Ok(()));
  assert_eq!(out,[2u8;SECTOR_BYTES]);
  let mut bad=Disk{reads:0,change:true,fail:false};out.fill(17);
  assert_eq!(read_stable_sectors(&mut bad,1,&mut out),Err(BlockError::Io));
  assert_eq!(out,[17u8;SECTOR_BYTES]);
 }
 #[test]fn second_pass_io_failure_invalid_length_and_bounds_preserve_caller_bytes(){
  let mut d=Disk{reads:0,change:false,fail:true};
  let mut out=[4u8;SECTOR_BYTES];
  assert_eq!(read_stable_sectors(&mut d,0,&mut out),Err(BlockError::Io));
  assert_eq!(out,[4u8;SECTOR_BYTES]);
  assert_eq!(read_stable_sectors(&mut d,4,&mut out),Err(BlockError::OutOfRange));
  assert_eq!(out,[4u8;SECTOR_BYTES]);
  assert_eq!(read_stable_sectors(&mut d,0,&mut out[..7]),Err(BlockError::OutOfRange));
 }
}


/// P0 batch three W4: a permanently retiring, read-only, twice-observed
/// block window for boot and partition metadata inspection. The disk is read
/// through a capacity-pinned immutable logical window; mismatch, transport
/// failure or media resize retires this view even if geometry later returns.
pub struct StableReadOnlyWindow<'a,D:BlockDevice>{
 device:&'a mut D,capacity:u64,invalidated:bool,
}
impl<'a,D:BlockDevice> StableReadOnlyWindow<'a,D>{
 pub fn new(device:&'a mut D)->Result<Self,BlockError>{
  let capacity=device.sector_count();
  if capacity==0||capacity!=device.sector_count(){
   return Err(BlockError::Io);
  }
  Ok(Self{device,capacity,invalidated:false})
 }
 pub fn is_retired(&self)->bool{self.invalidated}
}
impl<D:BlockDevice> BlockDevice for StableReadOnlyWindow<'_,D>{
 fn sector_count(&self)->u64{self.capacity}
 fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
  ->Result<(),BlockError>{
  if self.invalidated{return Err(BlockError::Io);}
  check_range(self.capacity,lba,1)?;
  if self.device.sector_count()!=self.capacity{
   self.invalidated=true;return Err(BlockError::Io);
  }
  let mut first=[0u8;SECTOR_BYTES];
  let mut second=[0u8;SECTOR_BYTES];
  let observed=(||{
   self.device.read_sector(lba,&mut first)?;
   if self.device.sector_count()!=self.capacity{return Err(BlockError::Io);}
   self.device.read_sector(lba,&mut second)?;
   if self.device.sector_count()!=self.capacity{return Err(BlockError::Io);}
   if first!=second{return Err(BlockError::Io);}
   Ok(())
  })();
  if observed.is_err(){self.invalidated=true;return observed;}
  out.copy_from_slice(&first);Ok(())
 }
 fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])
  ->Result<(),BlockError>{Err(BlockError::ReadOnly)}
 fn sync(&mut self)->Result<(),BlockError>{Err(BlockError::ReadOnly)}
}
#[cfg(test)]
mod kernel_p0_batch3_w4_block_window_tests{
 use super::*;
 struct Device{reads:usize,changing:bool,fail_at:usize,capacity:u64}
 impl BlockDevice for Device{
  fn sector_count(&self)->u64{self.capacity}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   self.reads+=1;
   if self.reads==self.fail_at{return Err(BlockError::Io);}
   out.fill(lba as u8+if self.changing&&self.reads%2==0{2}else{1});
   Ok(())
  }
  fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{Err(BlockError::ReadOnly)}
 }
 #[test]fn stable_sector_delivers_data_and_never_permits_mutations(){
  let mut disk=Device{reads:0,changing:false,fail_at:0,capacity:8};
  let mut view=StableReadOnlyWindow::new(&mut disk).unwrap();
  let mut output=[99u8;SECTOR_BYTES];
  assert_eq!(view.read_sector(2,&mut output),Ok(()));
  assert_eq!(output,[3;SECTOR_BYTES]);
  assert_eq!(view.write_sector(2,&output),Err(BlockError::ReadOnly));
  assert_eq!(view.sync(),Err(BlockError::ReadOnly));
 }
 #[test]fn torn_second_read_retires_window_without_exposing_bytes(){
  let mut disk=Device{reads:0,changing:true,fail_at:0,capacity:8};
  let mut view=StableReadOnlyWindow::new(&mut disk).unwrap();
  let mut output=[19u8;SECTOR_BYTES];
  assert_eq!(view.read_sector(1,&mut output),Err(BlockError::Io));
  assert_eq!(output,[19;SECTOR_BYTES]);assert!(view.is_retired());
  assert_eq!(view.read_sector(1,&mut output),Err(BlockError::Io));
 }
 #[test]fn failed_second_read_and_new_media_geometry_both_revoke(){
  let mut disk=Device{reads:0,changing:false,fail_at:2,capacity:8};
  let mut view=StableReadOnlyWindow::new(&mut disk).unwrap();
  let mut output=[33u8;SECTOR_BYTES];
  assert_eq!(view.read_sector(1,&mut output),Err(BlockError::Io));
  assert!(view.is_retired());assert_eq!(output,[33;SECTOR_BYTES]);
  let mut disk=Device{reads:0,changing:false,fail_at:0,capacity:8};
  let mut view=StableReadOnlyWindow::new(&mut disk).unwrap();
  view.device.capacity=9;
  assert_eq!(view.read_sector(0,&mut output),Err(BlockError::Io));
  view.device.capacity=8;
  assert_eq!(view.read_sector(0,&mut output),Err(BlockError::Io));
 }
}

/// P0-73..75: two full-range observations inside the permanently retiring
/// read-only sector view. No torn or failed read exposes partial output.
impl<D:BlockDevice> StableReadOnlyWindow<'_,D>{
 pub fn read_verified_range(&mut self,start:u64,out:&mut[u8])
  ->Result<(),BlockError>{
  let result=read_stable_sectors(self,start,out);
  // A torn whole-range observation is just as fatal as an individual
  // sector fault: never accept the same window again after mismatch.
  if result==Err(BlockError::Io){self.invalidated=true;}
  result
 }
}
#[cfg(test)]
mod kernel_p0_batch4_w4_range_tests{
 use super::*;
 struct Disk{reads:usize,change_after:usize,fail_at:usize}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{8}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   self.reads+=1;
   if self.reads==self.fail_at{return Err(BlockError::Io);}
   out.fill(lba as u8+if self.reads>=self.change_after{2}else{1});
   Ok(())
  }
  fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{Err(BlockError::ReadOnly)}
 }
 #[test]fn double_range_observation_publishes_complete_read(){
  let mut d=Disk{reads:0,change_after:usize::MAX,fail_at:0};
  let mut w=StableReadOnlyWindow::new(&mut d).unwrap();
  let mut bytes=[9u8;SECTOR_BYTES*2];
  assert_eq!(w.read_verified_range(2,&mut bytes),Ok(()));
  assert_eq!(&bytes[..SECTOR_BYTES],&[3;SECTOR_BYTES]);
  assert_eq!(&bytes[SECTOR_BYTES..],&[4;SECTOR_BYTES]);
  assert!(!w.is_retired());
 }
 #[test]fn late_fault_poison_is_sticky_and_does_not_publish(){
  let mut d=Disk{reads:0,change_after:usize::MAX,fail_at:4};
  let mut w=StableReadOnlyWindow::new(&mut d).unwrap();
  let mut bytes=[77u8;SECTOR_BYTES*2];
  assert_eq!(w.read_verified_range(1,&mut bytes),Err(BlockError::Io));
  assert_eq!(bytes,[77;SECTOR_BYTES*2]);assert!(w.is_retired());
  assert_eq!(w.read_verified_range(1,&mut bytes),Err(BlockError::Io));
 }
 #[test]fn changed_second_pass_and_out_of_bounds_fail_closed(){
  let mut d=Disk{reads:0,change_after:3,fail_at:0};
  let mut w=StableReadOnlyWindow::new(&mut d).unwrap();
  let mut bytes=[88u8;SECTOR_BYTES];
  assert_eq!(w.read_verified_range(0,&mut bytes),Err(BlockError::Io));
  assert_eq!(bytes,[88;SECTOR_BYTES]);assert!(w.is_retired());
  let mut d=Disk{reads:0,change_after:usize::MAX,fail_at:0};
  let mut w=StableReadOnlyWindow::new(&mut d).unwrap();
  assert_eq!(w.read_verified_range(8,&mut bytes),Err(BlockError::OutOfRange));
  assert_eq!(bytes,[88;SECTOR_BYTES]);
 }
}

/// P0-93..95: safely extract an unaligned sub-sector metadata field while
/// verifying the full one- or two-sector span in private staging. Never
/// mutate the caller's destination after a fault, torn read or invalid span.
impl<D:BlockDevice> StableReadOnlyWindow<'_,D>{
 pub fn read_verified_bytes(&mut self,lba:u64,offset:usize,
  out:&mut[u8])->Result<(),BlockError>{
  if offset>=SECTOR_BYTES||out.is_empty(){return Err(BlockError::OutOfRange);}
  let end=offset.checked_add(out.len()).ok_or(BlockError::OutOfRange)?;
  if end>SECTOR_BYTES*2{return Err(BlockError::OutOfRange);}
  let sectors=(end+SECTOR_BYTES-1)/SECTOR_BYTES;
  let mut staged=[0u8;SECTOR_BYTES*2];
  self.read_verified_range(lba,&mut staged[..sectors*SECTOR_BYTES])?;
  out.copy_from_slice(&staged[offset..end]);
  Ok(())
 }
}
#[cfg(test)]
mod kernel_p0_batch5_w4_unaligned_metadata_tests{
 use super::*;
 struct Disk{reads:usize,fail_at:usize}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{4}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   self.reads+=1;
   if self.reads==self.fail_at{return Err(BlockError::Io);}
   out.fill(lba as u8);Ok(())
  }
  fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{Err(BlockError::ReadOnly)}
 }
 #[test] fn cross_sector_metadata_is_verified_before_publishing(){
  let mut disk=Disk{reads:0,fail_at:0};
  let mut view=StableReadOnlyWindow::new(&mut disk).unwrap();
  let mut out=[99u8;4];
  assert_eq!(view.read_verified_bytes(1,SECTOR_BYTES-2,&mut out),Ok(()));
  assert_eq!(out,[1,1,2,2]);
  assert!(!view.is_retired());
 }
 #[test] fn late_fault_keeps_destination_untouched_and_permanently_retires(){
  let mut disk=Disk{reads:0,fail_at:6};
  let mut view=StableReadOnlyWindow::new(&mut disk).unwrap();
  let mut out=[77u8;4];
  assert_eq!(view.read_verified_bytes(1,SECTOR_BYTES-2,&mut out),
   Err(BlockError::Io));
  assert_eq!(out,[77u8;4]);assert!(view.is_retired());
  assert_eq!(view.read_verified_bytes(1,SECTOR_BYTES-2,&mut out),
   Err(BlockError::Io));
 }
 #[test] fn invalid_offset_or_oversized_span_never_reads_media(){
  let mut disk=Disk{reads:0,fail_at:0};
  let mut view=StableReadOnlyWindow::new(&mut disk).unwrap();
  let mut out=[10u8;SECTOR_BYTES*2];
  assert_eq!(view.read_verified_bytes(0,1,&mut out),Err(BlockError::OutOfRange));
  assert_eq!(view.read_verified_bytes(0,SECTOR_BYTES,&mut out[..1]),
   Err(BlockError::OutOfRange));
  assert_eq!(view.device.reads,0);
  assert_eq!(out,[10u8;SECTOR_BYTES*2]);
 }
}
/// P0-113..115: authenticated partition bounds are checked BEFORE reading
/// any unaligned boot metadata. The two-pass permanently retiring media view
/// still performs the real read and prevents partial destination publication.
impl<D:BlockDevice> StableReadOnlyWindow<'_,D>{
 pub fn read_partition_metadata_bytes(&mut self,
  partition_start:u64,partition_sectors:u64,relative_lba:u64,
  byte_offset:usize,out:&mut[u8])->Result<(),BlockError>{
  check_range(self.capacity,partition_start,partition_sectors)?;
  if byte_offset>=SECTOR_BYTES||out.is_empty(){return Err(BlockError::OutOfRange);}
  let end=byte_offset.checked_add(out.len()).ok_or(BlockError::OutOfRange)?;
  if end>2*SECTOR_BYTES{return Err(BlockError::OutOfRange);}
  let sectors=((end+SECTOR_BYTES-1)/SECTOR_BYTES) as u64;
  check_range(partition_sectors,relative_lba,sectors)?;
  let actual_lba=partition_start.checked_add(relative_lba)
   .ok_or(BlockError::OutOfRange)?;
  self.read_verified_bytes(actual_lba,byte_offset,out)
 }
}
#[cfg(test)]
mod kernel_p0_batch6_w4_partition_metadata_tests{
 use super::*;
 struct Disk{reads:usize,fail_on:usize}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{8}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   self.reads+=1;
   if self.reads==self.fail_on{return Err(BlockError::Io);}
   out.fill(lba as u8);Ok(())
  }
  fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{Err(BlockError::ReadOnly)}
 }
 #[test] fn cross_sector_metadata_stays_within_authenticated_partition(){
  let mut d=Disk{reads:0,fail_on:0};
  let mut w=StableReadOnlyWindow::new(&mut d).unwrap();
  let mut value=[88u8;4];
  assert_eq!(w.read_partition_metadata_bytes(2,2,0,SECTOR_BYTES-2,&mut value),Ok(()));
  assert_eq!(value,[2,2,3,3]);
  let prior=w.device.reads;
  value.fill(55);
  assert_eq!(w.read_partition_metadata_bytes(2,2,1,SECTOR_BYTES-2,&mut value),
   Err(BlockError::OutOfRange));
  assert_eq!(w.read_partition_metadata_bytes(u64::MAX-1,3,0,0,&mut value),
   Err(BlockError::OutOfRange));
  assert_eq!(value,[55u8;4]);assert_eq!(w.device.reads,prior);
 }
 #[test] fn late_media_failure_poison_is_sticky_and_does_not_publish(){
  let mut d=Disk{reads:0,fail_on:4};
  let mut w=StableReadOnlyWindow::new(&mut d).unwrap();
  let mut value=[77u8;4];
  assert_eq!(w.read_partition_metadata_bytes(2,2,0,SECTOR_BYTES-2,&mut value),
   Err(BlockError::Io));
  assert_eq!(value,[77u8;4]);assert!(w.is_retired());
  assert_eq!(w.read_partition_metadata_bytes(2,2,0,SECTOR_BYTES-2,&mut value),
   Err(BlockError::Io));
 }
}


/// P0-133..135: read two independent authenticated partition metadata fields
/// with FOUR verified observations per field (two per full paired pass).
/// Publish both outputs together only if every read succeeds and both paired
/// passes agree. Device faults/torn snapshots retire the read-only view.
impl<D:BlockDevice> StableReadOnlyWindow<'_,D>{
 pub fn read_partition_metadata_pair(&mut self,
  partition_start:u64,partition_sectors:u64,
  first:(u64,usize),first_out:&mut[u8],
  second:(u64,usize),second_out:&mut[u8])->Result<(),BlockError>{
  if first_out.is_empty()||second_out.is_empty()||
     first_out.len()>2*SECTOR_BYTES||second_out.len()>2*SECTOR_BYTES{
   return Err(BlockError::OutOfRange);
  }
  let mut a=[0u8;2*SECTOR_BYTES];let mut b=[0u8;2*SECTOR_BYTES];
  let mut later_a=[0u8;2*SECTOR_BYTES];
  let mut later_b=[0u8;2*SECTOR_BYTES];
  for (left,right) in [
   (&mut a[..first_out.len()],&mut b[..second_out.len()]),
   (&mut later_a[..first_out.len()],&mut later_b[..second_out.len()])
  ]{
   self.read_partition_metadata_bytes(partition_start,partition_sectors,
    first.0,first.1,left)?;
   self.read_partition_metadata_bytes(partition_start,partition_sectors,
    second.0,second.1,right)?;
  }
  if a[..first_out.len()]!=later_a[..first_out.len()]||
     b[..second_out.len()]!=later_b[..second_out.len()]{
   self.invalidated=true;
   return Err(BlockError::Io);
  }
  first_out.copy_from_slice(&a[..first_out.len()]);
  second_out.copy_from_slice(&b[..second_out.len()]);
  Ok(())
 }
}
#[cfg(test)]
mod kernel_p0_batch7_w4_atomic_pair_tests{
 use super::*;
 struct Disk{reads:usize,fail_at:usize,change_after:usize}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{8}
  fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{
   self.reads+=1;
   if self.reads==self.fail_at{return Err(BlockError::Io);}
   let added=if self.reads>=self.change_after{5}else{0};
   out.fill(lba as u8+added);Ok(())
  }
  fn write_sector(&mut self,_:u64,_:&[u8;SECTOR_BYTES])
   ->Result<(),BlockError>{Err(BlockError::ReadOnly)}
 }
 #[test]fn successful_pair_copies_both_fields_together(){
  let mut disk=Disk{reads:0,fail_at:0,change_after:usize::MAX};
  let mut view=StableReadOnlyWindow::new(&mut disk).unwrap();
  let mut a=[9u8;4];let mut b=[9u8;4];
  assert_eq!(view.read_partition_metadata_pair(2,3,(0,SECTOR_BYTES-2),
   &mut a,(1,0),&mut b),Ok(()));
  assert_eq!(a,[2,2,3,3]);assert_eq!(b,[3,3,3,3]);
  assert!(!view.is_retired());
 }
 #[test]fn late_io_fault_and_changed_snapshot_never_partially_publish(){
  for (fault,change) in [(7,usize::MAX),(0,7)]{
   let mut disk=Disk{reads:0,fail_at:fault,change_after:change};
   let mut view=StableReadOnlyWindow::new(&mut disk).unwrap();
   let mut a=[7u8;4];let mut b=[8u8;4];
   assert_eq!(view.read_partition_metadata_pair(2,3,(0,SECTOR_BYTES-2),
    &mut a,(1,0),&mut b),Err(BlockError::Io));
   assert_eq!(a,[7;4]);assert_eq!(b,[8;4]);assert!(view.is_retired());
  }
 }
 #[test]fn invalid_partition_span_never_writes_caller_buffers(){
  let mut disk=Disk{reads:0,fail_at:0,change_after:usize::MAX};
  let mut view=StableReadOnlyWindow::new(&mut disk).unwrap();
  let mut a=[7u8;4];let mut b=[8u8;4];
  assert_eq!(view.read_partition_metadata_pair(2,1,(0,SECTOR_BYTES-2),
   &mut a,(1,0),&mut b),Err(BlockError::OutOfRange));
  assert_eq!(a,[7;4]);assert_eq!(b,[8;4]);
 }
}
