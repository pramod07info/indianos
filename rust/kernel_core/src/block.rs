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
        self.device.read_sector(self.start + lba, &mut sector)?;
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

