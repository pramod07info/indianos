//! Fallible page-backed kernel buffers. This is not a global allocator.
//! The normal desktop is single-BSP and calls this only with IRQs disabled.
//! Metadata stays outside the allocation; quota includes rounded physical pages.
const PAGE: usize = 4096;
pub(crate) const FRAME_QUOTA: usize = 1600 * 1600 * 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error { Geometry, Quota, OutOfMemory, Mapping, Release, Owner, Metadata, Alignment, Stale, Integrity }

pub(crate) fn allocation_size(bytes: usize, quota: usize) -> Result<(usize, u32), Error> {
    if bytes == 0 { return Err(Error::Geometry); }
    let rounded = bytes.checked_add(PAGE - 1).ok_or(Error::Geometry)? & !(PAGE - 1);
    if rounded > quota { return Err(Error::Quota); }
    let pages = u32::try_from(rounded / PAGE).map_err(|_| Error::Geometry)?;
    Ok((rounded, pages))
}

/// Opaque backing key is distinct from an optional contiguous physical span.
/// A scattered virtual arena must never be treated as one physical DMA run.
#[derive(Debug,PartialEq,Eq)]
pub(crate) struct Backing{pub key:u64,pub physical:Option<u64>,pub pointer:*mut u8}
/// # Safety: allocate returns exclusive mapped writable bytes. A false release
/// keeps mappings valid unless discard is overridden to own partial unmapping.
pub(crate) unsafe trait Backend {
    fn allocate(&mut self,pages:u32)->Result<Backing,Error>;
    fn release(&mut self,key:u64,pages:u32)->bool;
    /// # Safety: tuple is from a still-owned allocation of this backend.
    unsafe fn discard(&mut self,key:u64,pages:u32,pointer:*mut u8)->bool{
        unsafe{core::ptr::write_bytes(pointer,0,pages as usize*PAGE)};
        self.release(key,pages)
    }
    fn has_pending(&self)->bool{false}
    fn pending_bytes(&self)->u64{0}
    fn reclaim_pending(&mut self)->u64{0}
}

pub(crate) struct Buffer {
    retiring:bool,
    backing:u64,
    physical: u64,
    pointer: *mut u8,
    pages: u32,
    length: usize,
    owner: u64,
}
impl Buffer {
    pub(crate) fn charged(&self) -> usize { self.pages as usize * PAGE }
    pub(crate) fn pointer(&self) -> *mut u8 { self.pointer }
    pub(crate) fn physical(&self) -> u64 { self.physical }
    pub(crate) fn backing(&self) -> u64 { self.backing }
    pub(crate) const fn new() -> Self {
        Self { retiring:false,backing:0,physical: 0, pointer: core::ptr::null_mut(), pages: 0, length: 0, owner: 0 }
    }
    pub(crate) fn prepare(&mut self, owner: u64, bytes: usize, quota: usize,
        backend: &mut (impl Backend + ?Sized)) -> Result<(), Error> {
        if owner == 0 { return Err(Error::Owner); }
        let (_, pages) = allocation_size(bytes, quota)?;
        if self.pages != 0 {
            if self.owner != owner { return Err(Error::Owner); }
            if self.pages == pages && !self.retiring { self.length = bytes; return Ok(()); }
            // Staging is unpublished. Release it before resizing so the owner
            // can never exceed quota through simultaneous old/new buffers.
            self.clear(backend)?;
        }
        let backing=backend.allocate(pages)?;let pointer=backing.pointer;
        // Backend guarantees exclusive mapped RAM; zero padding as well.
        unsafe { core::ptr::write_bytes(pointer, 0, pages as usize * PAGE); }
        self.backing=backing.key;self.physical=backing.physical.unwrap_or(0); self.pointer = pointer; self.pages = pages;
        self.length = bytes; self.owner = owner;
        Ok(())
    }
    pub(crate) fn clear(&mut self, backend: &mut (impl Backend + ?Sized)) -> Result<(), Error> {
        if self.pages == 0 { return Ok(()); }
        // Scrub before return to PMM. Retain ownership metadata on failure,
        // preventing a leak, double allocation or silent quota reset.
        // A backend may already have unmapped the span when release fails.
        // Keep ownership for cleanup, but never authorize reuse of that span.
        self.retiring=true;
        if !unsafe{backend.discard(self.backing,self.pages,self.pointer)}{return Err(Error::Release);}
        *self = Self::new();
        Ok(())
    }
}

#[cfg(not(test))]
fn memory_log(message: &'static [u8]) {
    unsafe extern "C" { fn serial_write(message: *const core::ffi::c_char); }
    unsafe { serial_write(message.as_ptr().cast()); }
}
#[cfg(not(test))]
pub(crate) struct Physical;
#[cfg(not(test))]
unsafe impl Backend for Physical{
 fn allocate(&mut self,pages:u32)->Result<Backing,Error>{
  let backing=crate::kernel_arena::allocate(pages)?;
  memory_log(b"IndianOS: kernel owned buffer pages allocated\n\0");Ok(backing)
 }
 fn release(&mut self,key:u64,pages:u32)->bool{crate::kernel_arena::release(key,pages)}
 unsafe fn discard(&mut self,key:u64,pages:u32,pointer:*mut u8)->bool{
  let released=crate::kernel_arena::discard(key,pages,pointer);
  if released{memory_log(b"IndianOS: kernel owned buffer pages released\n\0");}released
 }
 fn has_pending(&self)->bool{crate::kernel_arena::pending()}
 fn pending_bytes(&self)->u64{crate::kernel_arena::pending_bytes()}
 fn reclaim_pending(&mut self)->u64{crate::kernel_arena::reclaim()}
}

fn frame_bytes(width: u16, height: u16) -> Option<usize> {
    if width == 0 || height == 0 || width > 1600 || height > 1600 { return None; }
    Some(usize::from(width) * usize::from(height) * 4)
}
#[cfg(not(test))]
pub(crate) fn masked() -> bool {
    let flags: u64;
    unsafe { core::arch::asm!("pushfq; pop {}", out(reg) flags, options(preserves_flags)); }
    flags & (1 << 9) == 0
}
#[cfg(test)]
pub(crate) fn masked() -> bool { true }
#[cfg(test)]
fn memory_log(_: &'static [u8]) {}

#[cfg(not(test))]
static mut FRAME: Option<crate::kernel_heap::Allocation> = None;
#[cfg(not(test))]
static mut ROW: Option<crate::kernel_heap::Allocation> = None;
#[cfg(test)]
std::thread_local! {
    // Test service fixtures may intentionally leave a live session. HOST owns
    // all fixture RAM; suppress cross-TLS Drop ordering, not explicit release.
    static FRAME: core::cell::RefCell<core::mem::ManuallyDrop<Option<crate::kernel_heap::Allocation>>> =
        const { core::cell::RefCell::new(core::mem::ManuallyDrop::new(None)) };
    static ROW: core::cell::RefCell<core::mem::ManuallyDrop<Option<crate::kernel_heap::Allocation>>> =
        const { core::cell::RefCell::new(core::mem::ManuallyDrop::new(None)) };
}
// Separate cells permit copying a scratch row into staging without aliasing
// either allocation's exclusive borrow. Neither callback borrows heap metadata.
fn with_slot<R>(row: bool, f: impl FnOnce(&mut Option<crate::kernel_heap::Allocation>) -> R) -> R {
    #[cfg(not(test))]
    unsafe { if row { f(&mut *core::ptr::addr_of_mut!(ROW)) }
        else { f(&mut *core::ptr::addr_of_mut!(FRAME)) } }
    #[cfg(test)]
    { if row { ROW.with(|slot| f(&mut **slot.borrow_mut())) }
        else { FRAME.with(|slot| f(&mut **slot.borrow_mut())) } }
}
pub(crate) fn prepare_frame(owner: u64, width: u16, height: u16) -> bool {
    if !crate::kernel_heap::admission_available() { return false; }
    let Some(bytes) = frame_bytes(width, height) else { return false; };
    with_slot(false, |slot| {
        let owner = crate::kernel_heap::Owner::Frame(owner);
        if let Some(buffer) = slot.as_mut() {
            if buffer.owner() != owner { return false; }
            if buffer.resize_within_pages(bytes) { return true; }
        }
        if let Some(buffer) = slot.take() { if buffer.release().is_err() { return false; } }
        let Ok(layout) = core::alloc::Layout::from_size_align(bytes, 4) else { return false; };
        let Ok(buffer) = crate::kernel_heap::try_allocate(owner, layout) else { return false; };
        *slot = Some(buffer);
        memory_log(b"IndianOS: kernel frame buffer allocated\n\0");
        true
    })
}
pub(crate) fn with_frame<R>(f: impl FnOnce(&mut [u32]) -> R) -> R {
    with_slot(false, |slot| f(slot.as_mut().expect("active frame owns staging").words()))
}
/// Private syscall-copy scratch is a distinct owner and never aliases staging.
/// Reused across rows; both allocations retire with the compositor epoch.
pub(crate) fn with_row<R>(epoch: u64, f: impl FnOnce(&mut [u32]) -> R) -> Option<R> {
    if !crate::kernel_heap::admission_available() { return None; }
    with_slot(true, |slot| {
        let owner = crate::kernel_heap::Owner::Scratch(epoch);
        if slot.as_ref().is_some_and(|buffer| buffer.owner() != owner) { return None; }
        if slot.is_none() {
            *slot = Some(crate::kernel_heap::try_allocate(owner,
                core::alloc::Layout::from_size_align(1600 * 4, 4).ok()?).ok()?);
            memory_log(b"IndianOS: kernel compositor scratch allocated\n\0");
        }
        Some(f(slot.as_mut()?.words()))
    })
}
pub(crate) fn release_frame() -> bool {
    if !crate::kernel_heap::admission_available() { return false; }
    let frame = with_slot(false, |slot| match slot.take() {
        Some(buffer) => {
            let ok = buffer.release().is_ok();
            if ok { memory_log(b"IndianOS: kernel frame buffer released\n\0"); }
            ok
        }, None => true,
    });
    let row = with_slot(true, |slot| match slot.take() {
        Some(buffer) => {
            let ok = buffer.release().is_ok();
            if ok { memory_log(b"IndianOS: kernel compositor scratch released\n\0"); }
            ok
        }, None => true,
    });
    crate::kernel_heap::reclaim_retired() && frame && row
}
#[cfg(test)]
pub(crate) fn fail_allocation(fail: bool) { crate::kernel_heap::fail_allocation(fail); }

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) struct Memory { pub(super) bytes: std::vec::Vec<u64>, busy: bool, pub(super) oom: bool, fail_release: bool }
    impl Memory {
        pub(super) fn new() -> Self { Self { bytes: std::vec![0; PAGE / 8 * 4], busy: false, oom: false, fail_release: false } }
    }
    unsafe impl Backend for Memory {
        fn allocate(&mut self, pages: u32) -> Result<Backing, Error> {
            if self.oom || self.busy || pages as usize > self.bytes.len() * 8 / PAGE { return Err(Error::OutOfMemory); }
            self.busy = true;
            Ok(Backing{key:PAGE as u64,physical:Some(PAGE as u64),pointer:self.bytes.as_mut_ptr().cast()})
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
    #[test] fn pending_release_cannot_be_reused_by_same_size_prepare() {
        let mut memory=Memory::new();let mut buffer=Buffer::new();
        buffer.prepare(7,PAGE,PAGE,&mut memory).unwrap();
        memory.fail_release=true;
        assert_eq!(buffer.clear(&mut memory),Err(Error::Release));
        assert!(buffer.retiring);
        assert_eq!(buffer.prepare(7,PAGE,PAGE,&mut memory),Err(Error::Release));
        assert_eq!(buffer.owner,7);assert!(memory.busy);
        assert_eq!(buffer.prepare(8,PAGE,PAGE,&mut memory),Err(Error::Owner));
        memory.fail_release=false;
        buffer.prepare(7,PAGE,PAGE,&mut memory).unwrap();
        assert!(!buffer.retiring);assert!(memory.bytes.iter().all(|&v|v==0));
        buffer.clear(&mut memory).unwrap();
    }
}


#[cfg(test)]
mod heap_consumer_tests {
    use super::*;
    #[test] fn maximum_frame_and_scratch_have_separate_quotas_and_storage() {
        assert!(release_frame()); fail_allocation(false);
        assert!(prepare_frame(41,1600,1600));
        with_frame(|frame| { frame[0]=0x123456; frame[1600*1600-1]=0x654321; });
        assert_eq!(with_row(41,|row| {
            assert_eq!(row.len(),1600); assert!(row.iter().all(|p| *p==0));
            row.fill(0xabcdef);
            with_frame(|frame| assert_eq!((frame[0],frame[1600*1600-1]),(0x123456,0x654321)));
            row[0]
        }),Some(0xabcdef));
        assert!(with_row(42,|_|()).is_none());
        assert!(!prepare_frame(42,1600,1600));
        assert!(release_frame());
        assert!(prepare_frame(42,640,480));
        assert_eq!(with_row(42,|row|row[0]),Some(0));
        assert!(release_frame());
    }
    #[test] fn scratch_oom_preserves_committed_staging_and_allows_later_retry() {
        assert!(release_frame()); fail_allocation(false);
        assert!(prepare_frame(5,640,480)); with_frame(|pixels|pixels[0]=0x123456);
        fail_allocation(true);
        assert!(with_row(5,|_|panic!("OOM exposed scratch")).is_none());
        with_frame(|pixels|assert_eq!(pixels[0],0x123456));
        fail_allocation(false);
        assert!(with_row(5,|row|row[0]=0xff).is_some());
        fail_allocation(true); // Subsequent rows reuse the owned allocation.
        assert_eq!(with_row(5,|row|row[0]),Some(0xff));
        fail_allocation(false); assert!(release_frame());
    }
}
