//! Fallible page-backed kernel buffers. This is not a global allocator.
//! The normal desktop is single-BSP and calls this only with IRQs disabled.
//! Metadata stays outside the allocation; quota includes rounded physical pages.
const PAGE: usize = 4096;
pub(crate) const FRAME_QUOTA: usize = 1600 * 1600 * 4;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Error { Geometry, Quota, OutOfMemory, Mapping, Release, Owner }

fn allocation_size(bytes: usize, quota: usize) -> Result<(usize, u32), Error> {
    if bytes == 0 { return Err(Error::Geometry); }
    let rounded = bytes.checked_add(PAGE - 1).ok_or(Error::Geometry)? & !(PAGE - 1);
    if rounded > quota { return Err(Error::Quota); }
    let pages = u32::try_from(rounded / PAGE).map_err(|_| Error::Geometry)?;
    Ok((rounded, pages))
}

/// Backend owns the mapping until release succeeds. Allocation must return
/// exclusive, u32-aligned, writable RAM covering every requested page.
unsafe trait Backend {
    fn allocate(&mut self, pages: u32) -> Result<(u64, *mut u8), Error>;
    fn release(&mut self, physical: u64, pages: u32) -> bool;
}

struct Buffer {
    physical: u64,
    pointer: *mut u8,
    pages: u32,
    length: usize,
    owner: u64,
}
impl Buffer {
    const fn new() -> Self {
        Self { physical: 0, pointer: core::ptr::null_mut(), pages: 0, length: 0, owner: 0 }
    }
    fn prepare(&mut self, owner: u64, bytes: usize, quota: usize,
        backend: &mut impl Backend) -> Result<(), Error> {
        if owner == 0 { return Err(Error::Owner); }
        let (_, pages) = allocation_size(bytes, quota)?;
        if self.pages != 0 {
            if self.owner != owner { return Err(Error::Owner); }
            if self.pages == pages { self.length = bytes; return Ok(()); }
            // Staging is unpublished. Release it before resizing so the owner
            // can never exceed quota through simultaneous old/new buffers.
            self.clear(backend)?;
        }
        let (physical, pointer) = backend.allocate(pages)?;
        // Backend guarantees exclusive mapped RAM; zero padding as well.
        unsafe { core::ptr::write_bytes(pointer, 0, pages as usize * PAGE); }
        self.physical = physical; self.pointer = pointer; self.pages = pages;
        self.length = bytes; self.owner = owner;
        Ok(())
    }
    fn clear(&mut self, backend: &mut impl Backend) -> Result<(), Error> {
        if self.pages == 0 { return Ok(()); }
        // Scrub before return to PMM. Retain ownership metadata on failure,
        // preventing a leak, double allocation or silent quota reset.
        unsafe { core::ptr::write_bytes(self.pointer, 0, self.pages as usize * PAGE); }
        if !backend.release(self.physical, self.pages) { return Err(Error::Release); }
        *self = Self::new();
        Ok(())
    }
}

#[cfg(not(test))]
fn memory_log(message: &'static [u8]) {
    unsafe extern "C" { fn serial_write(message: *const core::ffi::c_char); }
    unsafe { serial_write(message.as_ptr().cast()); }
}
// A failed rollback is never forgotten or reused. The normal validated map
// makes this path unreachable; retain its exact PMM ownership until reboot.
#[cfg(not(test))]
static mut MAPPING_QUARANTINE: (u64, u32) = (0, 0);
#[cfg(not(test))]
struct Physical;
#[cfg(not(test))]
unsafe impl Backend for Physical {
    fn allocate(&mut self, pages: u32) -> Result<(u64, *mut u8), Error> {
        if unsafe { MAPPING_QUARANTINE.1 } != 0 { return Err(Error::Mapping); }
        let mut offset = 0;
        if !unsafe { crate::boot::boot_hhdm_offset(&mut offset) } { return Err(Error::Mapping); }
        let Some(physical) = (unsafe { crate::pmm::allocate_buffer_frames(pages) }) else {
            return Err(Error::OutOfMemory);
        };
        // PMM returns only managed physical RAM. The validated boot direct map
        // covers its usable ranges; checked addition prevents alias wrapping.
        let pointer = physical.checked_add(offset).and_then(|start| {
            start.checked_add(u64::from(pages) * PAGE as u64 - 1)?;
            Some(start as *mut u8)
        });
        if let Some(pointer) = pointer {
            memory_log(b"IndianOS: kernel frame buffer allocated\n\0");
            return Ok((physical, pointer));
        }
        // Validated HHDM and managed frames make this unreachable, but undo a
        // failed mapping preflight rather than publishing an unusable buffer.
        if !unsafe { crate::pmm::release_buffer_frames(physical, pages) } {
            unsafe { MAPPING_QUARANTINE = (physical, pages); }
        }
        Err(Error::Mapping)
    }
    fn release(&mut self, physical: u64, pages: u32) -> bool {
        let released = unsafe { crate::pmm::release_buffer_frames(physical, pages) };
        if released { memory_log(b"IndianOS: kernel frame buffer released\n\0"); }
        released
    }
}

#[cfg(not(test))]
static mut FRAME: Buffer = Buffer::new();
/// All three entry points are confined to the serialized GUI broker. No raw
/// reference escapes; retirement cannot free a buffer while it is borrowed.
fn frame_bytes(width: u16, height: u16) -> Option<usize> {
    if width == 0 || height == 0 || width > 1600 || height > 1600 { return None; }
    Some(usize::from(width) * usize::from(height) * 4)
}
#[cfg(not(test))]
fn masked() -> bool {
    let flags: u64;
    unsafe { core::arch::asm!("pushfq; pop {}", out(reg) flags, options(preserves_flags)); }
    flags & (1 << 9) == 0
}
#[cfg(not(test))]
pub(crate) fn prepare_frame(owner: u64, width: u16, height: u16) -> bool {
    if !masked() { return false; }
    let Some(bytes) = frame_bytes(width, height) else { return false; };
    unsafe { (&mut *core::ptr::addr_of_mut!(FRAME)).prepare(owner, bytes, FRAME_QUOTA, &mut Physical).is_ok() }
}
#[cfg(not(test))]
pub(crate) fn with_frame<R>(f: impl FnOnce(&mut [u32]) -> R) -> R {
    unsafe {
        let buffer = &mut *core::ptr::addr_of_mut!(FRAME);
        // Only called after a successful prepare_frame, while a transaction
        // owns the buffer. The service clears pending before releasing it.
        f(core::slice::from_raw_parts_mut(buffer.pointer.cast(), buffer.length / 4))
    }
}
#[cfg(not(test))]
pub(crate) fn release_frame() -> bool {
    if !masked() { return false; }
    unsafe { (&mut *core::ptr::addr_of_mut!(FRAME)).clear(&mut Physical).is_ok() }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) struct Memory { pub(super) bytes: std::vec::Vec<u64>, busy: bool, pub(super) oom: bool, fail_release: bool }
    impl Memory {
        pub(super) fn new() -> Self { Self { bytes: std::vec![0; PAGE / 8 * 4], busy: false, oom: false, fail_release: false } }
    }
    unsafe impl Backend for Memory {
        fn allocate(&mut self, pages: u32) -> Result<(u64, *mut u8), Error> {
            if self.oom || self.busy || pages as usize > self.bytes.len() * 8 / PAGE { return Err(Error::OutOfMemory); }
            self.busy = true;
            Ok((PAGE as u64, self.bytes.as_mut_ptr().cast()))
        }
        fn release(&mut self, _: u64, _: u32) -> bool {
            if self.fail_release { return false; }
            self.busy = false; true
        }
    }
    #[test] fn checked_page_quota() {
        assert_eq!(frame_bytes(0, 480), None);
        assert_eq!(frame_bytes(1601, 480), None);
        assert_eq!(frame_bytes(1600, 1600), Some(FRAME_QUOTA));
        assert_eq!(allocation_size(0, PAGE), Err(Error::Geometry));
        assert_eq!(allocation_size(usize::MAX, usize::MAX), Err(Error::Geometry));
        assert_eq!(allocation_size(PAGE + 1, PAGE + 1), Err(Error::Quota));
        assert_eq!(allocation_size(PAGE, PAGE), Ok((PAGE, 1)));
        assert_eq!(allocation_size(FRAME_QUOTA, FRAME_QUOTA), Ok((FRAME_QUOTA, 2500)));
    }
    #[test] fn reuse_scrub_and_owner_isolation() {
        let mut memory = Memory::new(); let mut buffer = Buffer::new();
        assert_eq!(buffer.prepare(0, 4, PAGE, &mut memory), Err(Error::Owner));
        buffer.prepare(7, 4, PAGE, &mut memory).unwrap();
        memory.bytes[0] = 123;
        buffer.prepare(7, 8, PAGE, &mut memory).unwrap();
        assert_eq!(memory.bytes[0], 123);
        assert_eq!(buffer.prepare(8, 4, PAGE, &mut memory), Err(Error::Owner));
        buffer.clear(&mut memory).unwrap();
        assert!(memory.bytes.iter().all(|x| *x == 0));
        buffer.prepare(8, 4, PAGE, &mut memory).unwrap();
        buffer.clear(&mut memory).unwrap(); buffer.clear(&mut memory).unwrap();
    }
    #[test] fn oom_and_quota_do_not_publish_or_lose_ownership() {
        let mut memory = Memory::new(); let mut buffer = Buffer::new();
        memory.oom = true;
        assert_eq!(buffer.prepare(1, PAGE, PAGE, &mut memory), Err(Error::OutOfMemory));
        assert_eq!(buffer.pages, 0); assert!(!memory.busy);
        memory.oom = false; buffer.prepare(1, PAGE, PAGE, &mut memory).unwrap();
        assert_eq!(buffer.prepare(1, PAGE + 1, PAGE, &mut memory), Err(Error::Quota));
        assert_eq!(buffer.pages, 1);
        memory.fail_release = true;
        assert_eq!(buffer.clear(&mut memory), Err(Error::Release));
        assert_eq!(buffer.owner, 1); assert!(memory.busy);
        memory.fail_release = false; buffer.clear(&mut memory).unwrap();
        assert!(!memory.busy);
    }
    #[test] fn failed_resize_leaves_no_live_allocation() {
        let mut memory = Memory::new(); let mut buffer = Buffer::new();
        buffer.prepare(1, PAGE, PAGE * 4, &mut memory).unwrap();
        memory.oom = true;
        assert_eq!(buffer.prepare(1, PAGE * 2, PAGE * 4, &mut memory), Err(Error::OutOfMemory));
        assert_eq!(buffer.pages, 0); assert!(!memory.busy);
    }
}

// Host-only physical-memory fixture. The same Buffer ownership, quota,
// allocation failure and release code runs under the GUI protocol tests.
#[cfg(test)]
std::thread_local! {
    static HOST: core::cell::RefCell<(Buffer, tests::Memory)> = {
        let mut memory = tests::Memory::new();
        memory.bytes.resize(FRAME_QUOTA / 8, 0);
        core::cell::RefCell::new((Buffer::new(), memory))
    };
}
#[cfg(test)]
pub(crate) fn prepare_frame(owner: u64, width: u16, height: u16) -> bool {
    let Some(bytes) = frame_bytes(width, height) else { return false; };
    HOST.with(|state| {
        let mut state = state.borrow_mut();
        let (buffer, memory) = &mut *state;
        buffer.prepare(owner, bytes, FRAME_QUOTA, memory).is_ok()
    })
}
#[cfg(test)]
pub(crate) fn with_frame<R>(f: impl FnOnce(&mut [u32]) -> R) -> R {
    HOST.with(|state| {
        let mut state = state.borrow_mut();
        let buffer = &mut state.0;
        assert!(buffer.pages != 0);
        unsafe { f(core::slice::from_raw_parts_mut(buffer.pointer.cast(), buffer.length / 4)) }
    })
}
#[cfg(test)]
pub(crate) fn release_frame() -> bool {
    HOST.with(|state| {
        let mut state = state.borrow_mut();
        let (buffer, memory) = &mut *state;
        buffer.clear(memory).is_ok()
    })
}
#[cfg(test)]
pub(crate) fn fail_allocation(fail: bool) {
    HOST.with(|state| state.borrow_mut().1.oom = fail);
}
