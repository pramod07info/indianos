//! Bounded, fallible kernel page heap. Allocations are zeroed and page aligned;
//! aggregate quotas include physical page rounding. No global allocator, small
//! object slab or SMP promise. Reuses PMM ownership/release through Buffer.
use core::{alloc::Layout, marker::PhantomData};
use crate::kernel_buffer::{allocation_size, Backend, Buffer, Error, FRAME_QUOTA};
const SLOTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owner { Frame(u64), Scratch(u64) }
impl Owner {
    fn epoch(self) -> u64 { match self { Self::Frame(e) | Self::Scratch(e) => e } }
    fn quota(self) -> usize { match self { Self::Frame(_) => FRAME_QUOTA, Self::Scratch(_) => 8192 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Token { slot: usize, generation: u64 }
struct Slot { buffer: Buffer, owner: Option<Owner>, generation: u64, live: bool }
impl Slot {
    const fn new() -> Self { Self { buffer: Buffer::new(), owner: None, generation: 0, live: false } }
}
struct Heap { slots: [Slot; SLOTS], next_generation: u64 }
impl Heap {
    const fn new() -> Self { Self { slots: [const { Slot::new() }; SLOTS], next_generation: 1 } }
    fn allocate(&mut self, owner: Owner, layout: Layout, backend: &mut (impl Backend + ?Sized))
        -> Result<(Token, *mut u8), Error> {
        if owner.epoch() == 0 { return Err(Error::Owner); }
        if layout.align() > 4096 { return Err(Error::Alignment); }
        let (charge, _) = allocation_size(layout.size(), owner.quota())?;
        // Quarantined allocations still consume their owner's quota.
        let used: usize = self.slots.iter().filter(|s| s.owner == Some(owner))
            .map(|s| s.buffer.charged()).sum();
        let available = owner.quota().checked_sub(used).ok_or(Error::Quota)?;
        if charge > available { return Err(Error::Quota); }
        let index = self.slots.iter().position(|s| !s.live && s.buffer.charged() == 0)
            .ok_or(Error::Metadata)?;
        let generation = self.next_generation;
        let next = generation.checked_add(1).ok_or(Error::Metadata)?;
        let slot = &mut self.slots[index];
        slot.buffer.prepare(owner.epoch(), layout.size(), available, backend)?;
        slot.owner = Some(owner);
        if slot.buffer.pointer() as usize % layout.align() != 0 {
            slot.buffer.clear(backend)?; slot.owner = None;
            return Err(Error::Alignment);
        }
        slot.generation = generation; slot.live = true;
        self.next_generation = next;
        Ok((Token { slot: index, generation }, slot.buffer.pointer()))
    }
    fn release(&mut self, token: Token, backend: &mut (impl Backend + ?Sized)) -> Result<(), Error> {
        let slot = self.slots.get_mut(token.slot).ok_or(Error::Stale)?;
        if !slot.live || slot.generation != token.generation { return Err(Error::Stale); }
        // Revoke first. A failed physical release becomes quarantined and may
        // only be retried by reclamation, never through a stale handle.
        slot.live = false;
        slot.buffer.clear(backend)?;
        slot.owner = None;
        Ok(())
    }
    fn reclaim(&mut self, backend: &mut (impl Backend + ?Sized)) -> bool {
        let mut clean = true;
        for slot in &mut self.slots {
            if !slot.live && slot.buffer.charged() != 0 {
                if slot.buffer.clear(backend).is_ok() { slot.owner = None; }
                else { clean = false; }
            }
        }
        clean
    }
}

#[cfg(not(test))]
static mut HEAP: Heap = Heap::new();
#[cfg(test)]
std::thread_local! {
    static HOST: core::cell::RefCell<(Heap, fixture::Memory)> =
        const { core::cell::RefCell::new((Heap::new(), fixture::Memory::new())) };
}
fn with_heap<R>(f: impl FnOnce(&mut Heap, &mut dyn Backend) -> R) -> R {
    #[cfg(not(test))]
    { let mut physical = crate::kernel_buffer::Physical;
      unsafe { f(&mut *core::ptr::addr_of_mut!(HEAP), &mut physical) } }
    #[cfg(test)]
    { HOST.with(|state| {
        let mut state = state.borrow_mut(); let (heap, memory) = &mut *state;
        f(heap, memory)
    }) }
}

/// Unique allocation lease. Raw-pointer marker prevents cross-thread transfer;
/// payload borrows cannot outlive or overlap release of this handle. All heap
/// operations obey the existing IRQ-disabled single-BSP admission contract.
pub(crate) struct Allocation {
    token: Option<Token>, pointer: *mut u8, length: usize, charged: usize,
    owner: Owner, _single_cpu: PhantomData<*mut ()>,
}
impl Allocation {
    pub(crate) fn owner(&self) -> Owner { self.owner }
    pub(crate) fn resize_within_pages(&mut self, bytes: usize) -> bool {
        if allocation_size(bytes, self.owner.quota()).map(|s| s.0) != Ok(self.charged) { return false; }
        self.length = bytes; true
    }
    pub(crate) fn bytes(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.pointer, self.length) }
    }
    pub(crate) fn words(&mut self) -> &mut [u32] {
        assert!(self.length % 4 == 0);
        unsafe { core::slice::from_raw_parts_mut(self.pointer.cast(), self.length / 4) }
    }
    pub(crate) fn release(mut self) -> Result<(), Error> {
        if !crate::kernel_buffer::masked() { return Err(Error::Owner); }
        let token = self.token.take().ok_or(Error::Stale)?;
        with_heap(|heap, backend| heap.release(token, backend))
    }
}
impl Drop for Allocation {
    fn drop(&mut self) {
        // IRQ-enabled misuse cannot touch PMM. Its live slot retains ownership
        // and quota until reboot rather than freeing memory unsafely.
        if !crate::kernel_buffer::masked() { return; }
        if let Some(token) = self.token.take() {
            let _ = with_heap(|heap, backend| heap.release(token, backend));
        }
    }
}
pub(crate) fn try_allocate(owner: Owner, layout: Layout) -> Result<Allocation, Error> {
    if !crate::kernel_buffer::masked() { return Err(Error::Owner); }
    let (token, pointer) = with_heap(|heap, backend| heap.allocate(owner, layout, backend))?;
    let (charged, _) = allocation_size(layout.size(), owner.quota())?;
    Ok(Allocation { token: Some(token), pointer, length: layout.size(), charged,
        owner, _single_cpu: PhantomData })
}
pub(crate) fn reclaim_retired() -> bool {
    crate::kernel_buffer::masked() && with_heap(|heap, backend| heap.reclaim(backend))
}
#[cfg(test)]
pub(crate) fn fail_allocation(fail: bool) { HOST.with(|state| state.borrow_mut().1.oom = fail); }

#[cfg(test)]
mod fixture {
    use super::*;
    #[repr(align(4096))]
    #[derive(Clone)]
    struct Page([u32;1024]);
    pub(super) struct Memory { runs: std::vec::Vec<(u64, std::vec::Vec<Page>)>,
        next: u64, pub oom: bool, pub fail_release: bool }
    impl Memory {
        pub const fn new() -> Self { Self { runs: std::vec::Vec::new(), next: 4096, oom: false, fail_release: false } }
        pub fn count(&self) -> usize { self.runs.len() }
    }
    unsafe impl Backend for Memory {
        fn allocate(&mut self, pages: u32) -> Result<(u64, *mut u8), Error> {
            if self.oom { return Err(Error::OutOfMemory); }
            let physical = self.next; self.next += u64::from(pages) * 4096;
            let mut bytes = std::vec![Page([0xdeadbeef;1024]); pages as usize];
            let pointer = bytes.as_mut_ptr().cast(); self.runs.push((physical, bytes));
            Ok((physical, pointer))
        }
        fn release(&mut self, physical: u64, _: u32) -> bool {
            if self.fail_release { return false; }
            let Some(index) = self.runs.iter().position(|r| r.0 == physical) else { return false; };
            assert!(self.runs[index].1.iter().all(|page| page.0.iter().all(|p| *p == 0)));
            self.runs.remove(index); true
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn layout(bytes: usize) -> Layout { Layout::from_size_align(bytes, 4).unwrap() }
    #[test] fn quota_is_aggregate_and_independent_between_owners() {
        let mut heap = Heap::new(); let mut memory = fixture::Memory::new();
        let owner = Owner::Scratch(1);
        let (a, _) = heap.allocate(owner, layout(1), &mut memory).unwrap();
        let (b, _) = heap.allocate(owner, layout(4096), &mut memory).unwrap();
        assert_eq!(heap.allocate(owner, layout(1), &mut memory), Err(Error::Quota));
        let (c, _) = heap.allocate(Owner::Scratch(2), layout(8192), &mut memory).unwrap();
        assert_eq!(memory.count(), 3);
        for token in [a,b,c] { heap.release(token, &mut memory).unwrap(); }
        assert_eq!(memory.count(), 0);
    }
    #[test] fn generation_blocks_double_free_after_slot_reuse() {
        let mut heap = Heap::new(); let mut memory = fixture::Memory::new();
        let (old, _) = heap.allocate(Owner::Scratch(1), layout(4), &mut memory).unwrap();
        heap.release(old, &mut memory).unwrap();
        let (new, _) = heap.allocate(Owner::Scratch(1), layout(4), &mut memory).unwrap();
        assert_eq!(old.slot, new.slot); assert_ne!(old.generation, new.generation);
        assert_eq!(heap.release(old, &mut memory), Err(Error::Stale));
        assert_eq!(memory.count(), 1); heap.release(new, &mut memory).unwrap();
    }
    #[test] fn refused_release_stays_charged_and_reclamation_is_bounded() {
        let mut heap = Heap::new(); let mut memory = fixture::Memory::new();
        let owner = Owner::Scratch(1);
        let (token, _) = heap.allocate(owner, layout(8192), &mut memory).unwrap();
        memory.fail_release = true;
        assert_eq!(heap.release(token, &mut memory), Err(Error::Release));
        assert_eq!(heap.allocate(owner, layout(4), &mut memory), Err(Error::Quota));
        assert_eq!(heap.release(token, &mut memory), Err(Error::Stale));
        assert!(!heap.reclaim(&mut memory)); assert_eq!(memory.count(), 1);
        memory.fail_release = false; assert!(heap.reclaim(&mut memory));
        assert_eq!(memory.count(), 0);
        assert!(heap.allocate(owner, layout(8192), &mut memory).is_ok());
    }
    #[test] fn invalid_layout_oom_and_metadata_exhaustion_are_atomic() {
        let mut heap = Heap::new(); let mut memory = fixture::Memory::new();
        assert_eq!(heap.allocate(Owner::Frame(0), layout(4), &mut memory), Err(Error::Owner));
        assert_eq!(heap.allocate(Owner::Frame(1), layout(0), &mut memory), Err(Error::Geometry));
        assert_eq!(heap.allocate(Owner::Frame(1), Layout::from_size_align(4,8192).unwrap(), &mut memory), Err(Error::Alignment));
        memory.oom = true;
        assert_eq!(heap.allocate(Owner::Frame(1), layout(4), &mut memory), Err(Error::OutOfMemory));
        assert_eq!(heap.next_generation, 1); assert_eq!(memory.count(), 0);
        memory.oom = false;
        let mut tokens = std::vec::Vec::new();
        for epoch in 1..=SLOTS as u64 { tokens.push(heap.allocate(Owner::Scratch(epoch), layout(4), &mut memory).unwrap().0); }
        assert_eq!(heap.allocate(Owner::Frame(1), layout(4), &mut memory), Err(Error::Metadata));
        for token in tokens { heap.release(token, &mut memory).unwrap(); }
        assert_eq!(memory.count(), 0);
        heap.next_generation = u64::MAX;
        assert_eq!(heap.allocate(Owner::Frame(1), layout(4), &mut memory), Err(Error::Metadata));
        assert_eq!(memory.count(), 0);
    }
    #[test] fn byte_sized_layout_and_page_rounding_are_supported() {
        let mut tiny = try_allocate(Owner::Scratch(76), Layout::from_size_align(1,1).unwrap()).unwrap();
        assert_eq!(tiny.bytes(), &[0]);tiny.bytes()[0]=0xff;
        assert_eq!(tiny.charged,4096);
        assert!(!tiny.resize_within_pages(4097));
        assert!(tiny.resize_within_pages(4096));
        assert_eq!(tiny.bytes()[0],0xff);
        drop(tiny);
        HOST.with(|state| assert_eq!(state.borrow().1.count(),0));
    }
    #[test] fn owned_payloads_are_private_and_drop_returns_all_pages() {
        let mut a = try_allocate(Owner::Scratch(77), Layout::from_size_align(4096,4096).unwrap()).unwrap();
        assert_eq!(a.pointer as usize % 4096, 0);
        let mut b = try_allocate(Owner::Scratch(77), layout(4096)).unwrap();
        a.words().fill(0xabcdef); assert!(b.words().iter().all(|p| *p == 0));
        b.words().fill(0x987654); assert!(a.words().iter().all(|p| *p == 0xabcdef));
        drop(a); drop(b);
        HOST.with(|state| assert_eq!(state.borrow().1.count(), 0));
    }
}
