//! Fallible owner-scoped heap. Frame allocations retain page granularity;
//! scratch allocations share bounded arenas with implicit free-hole coalescing.
//! Physical quota includes whole live/quarantined arenas, not requested bytes.
use core::{alloc::Layout, marker::PhantomData};
use crate::kernel_buffer::{allocation_size, Backend, Buffer, Error, FRAME_QUOTA};
const SLOTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owner { Frame(u64), Scratch(u64) }
impl Owner {
    fn epoch(self) -> u64 { match self { Self::Frame(e) | Self::Scratch(e) => e } }
    fn quota(self) -> usize { match self { Self::Frame(_) => FRAME_QUOTA, Self::Scratch(_) => 8192 } }
    fn extent(self, bytes: usize) -> Result<usize, Error> {
        allocation_size(bytes,self.quota())?;
        match self {
            Self::Frame(_) => Ok(allocation_size(bytes,self.quota())?.0),
            Self::Scratch(_) => bytes.checked_add(7).map(|n|n & !7).ok_or(Error::Geometry),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Token { slot: usize, generation: u64 }
struct Region { buffer: Buffer, owner: Option<Owner>, quarantined: bool, sharing_logged: bool }
impl Region {
    const fn new() -> Self { Self { buffer: Buffer::new(), owner: None, quarantined: false, sharing_logged: false } }
}
struct Slot { region: usize, offset: usize, extent: usize, generation: u64, live: bool }
impl Slot {
    const fn new() -> Self { Self { region: 0, offset: 0, extent: 0, generation: 0, live: false } }
}
struct Heap { slots: [Slot; SLOTS], regions: [Region; SLOTS], next_generation: u64 }
impl Heap {
    const fn new() -> Self { Self { slots: [const { Slot::new() }; SLOTS],
        regions: [const { Region::new() }; SLOTS], next_generation: 1 } }
    fn charge(&self, owner: Owner) -> usize {
        self.regions.iter().filter(|r|r.owner==Some(owner)).map(|r|r.buffer.charged()).sum()
    }
    /// Out-of-band live extents are the free-space authority. Removing adjacent
    /// extents coalesces their holes automatically, without mutable free links.
    fn gap(&self, region: usize, extent: usize, align: usize) -> Option<usize> {
        let capacity=self.regions.get(region)?.buffer.charged();let mut at=0usize;
        for _ in 0..=SLOTS {
            at=at.checked_add(align-1)? & !(align-1);
            let end=at.checked_add(extent)?;
            if end>capacity{return None;}
            let collision=self.slots.iter().find(|s|s.live&&s.region==region&&
                at<s.offset+s.extent&&s.offset<end);
            if let Some(slot)=collision {at=slot.offset.checked_add(slot.extent)?;}
            else{return Some(at);}
        }
        None
    }
    fn allocate(&mut self, owner: Owner, layout: Layout, backend: &mut (impl Backend + ?Sized))
        -> Result<(Token, *mut u8), Error> {
        if owner.epoch()==0{return Err(Error::Owner);}
        if layout.align()>4096{return Err(Error::Alignment);}
        let extent=owner.extent(layout.size())?;
        let align=match owner{Owner::Frame(_)=>4096,Owner::Scratch(_)=>layout.align().max(8)};
        let index=self.slots.iter().position(|s|!s.live).ok_or(Error::Metadata)?;
        let generation=self.next_generation;
        let next=generation.checked_add(1).ok_or(Error::Metadata)?;
        let existing=self.regions.iter().enumerate().find_map(|(i,r)|{
            if r.owner!=Some(owner)||r.quarantined{return None;}
            self.gap(i,extent,align).map(|offset|(i,offset))
        });
        let (region,offset)=if let Some(found)=existing {found} else {
            let capacity=match owner{Owner::Frame(_)=>extent,Owner::Scratch(_)=>8192};
            let available=owner.quota().checked_sub(self.charge(owner)).ok_or(Error::Quota)?;
            if capacity>available{return Err(Error::Quota);}
            let region=self.regions.iter().position(|r|r.buffer.charged()==0).ok_or(Error::Metadata)?;
            let entry=&mut self.regions[region];
            entry.buffer.prepare(owner.epoch(),capacity,available,backend)?;
            entry.owner=Some(owner);entry.sharing_logged=false;
            if entry.buffer.pointer() as usize % 4096 != 0 {
                entry.quarantined=true;
                entry.buffer.clear(backend)?;entry.owner=None;entry.quarantined=false;
                return Err(Error::Alignment);
            }
            entry.quarantined=false;(region,0)
        };
        // A whole new region is page aligned; a reused gap checked alignment
        // and every live neighbor before this exclusive pointer is formed.
        let pointer=unsafe{self.regions[region].buffer.pointer().add(offset)};
        unsafe{core::ptr::write_bytes(pointer,0,extent);}
        self.slots[index]=Slot{region,offset,extent,generation,live:true};
        self.next_generation=next;
        Ok((Token{slot:index,generation},pointer))
    }
    fn release(&mut self, token: Token, backend: &mut (impl Backend + ?Sized)) -> Result<(), Error> {
        let slot=self.slots.get_mut(token.slot).ok_or(Error::Stale)?;
        if !slot.live||slot.generation!=token.generation{return Err(Error::Stale);}
        let region=slot.region;
        unsafe{core::ptr::write_bytes(self.regions[region].buffer.pointer().add(slot.offset),0,slot.extent);}
        slot.live=false;
        if self.slots.iter().any(|s|s.live&&s.region==region){return Ok(());}
        let entry=&mut self.regions[region];entry.quarantined=true;
        entry.buffer.clear(backend)?;
        entry.owner=None;entry.quarantined=false;entry.sharing_logged=false;
        Ok(())
    }
    fn reclaim(&mut self, backend: &mut (impl Backend + ?Sized)) -> bool {
        let mut clean=true;
        for region in &mut self.regions {
            if region.quarantined {
                if region.buffer.clear(backend).is_ok(){
                    region.owner=None;region.quarantined=false;region.sharing_logged=false;
                }else{clean=false;}
            }
        }
        clean
    }
    fn note_sharing(&mut self, token: Token) -> bool {
        let Some(slot)=self.slots.get(token.slot)else{return false;};
        if !slot.live||slot.generation!=token.generation{return false;}
        let region=slot.region;
        if self.regions[region].sharing_logged||
           self.slots.iter().filter(|s|s.live&&s.region==region).count()<2{return false;}
        self.regions[region].sharing_logged=true;true
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
    token: Option<Token>, pointer: *mut u8, length: usize, capacity: usize,
    owner: Owner, _single_cpu: PhantomData<*mut ()>,
}
impl Allocation {
    pub(crate) fn owner(&self) -> Owner { self.owner }
    pub(crate) fn resize_within_pages(&mut self, bytes: usize) -> bool {
        if bytes==0 || bytes>self.capacity { return false; }
        self.length = bytes; true
    }
    pub(crate) fn note_shared_backing(&self) {
        if !crate::kernel_buffer::masked(){return;}
        if self.token.is_some_and(|token|with_heap(|heap,_|heap.note_sharing(token))) {
            #[cfg(not(test))]
            unsafe {
                unsafe extern "C" {fn serial_write(message:*const core::ffi::c_char);}
                serial_write(b"IndianOS: kernel heap descriptor shares scratch arena\n\0".as_ptr().cast());
            }
        }
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
    let capacity=owner.extent(layout.size())?;
    let (token, pointer) = with_heap(|heap, backend| heap.allocate(owner, layout, backend))?;
    Ok(Allocation { token: Some(token), pointer, length: layout.size(), capacity,
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
        let (a, _) = heap.allocate(owner, layout(4096), &mut memory).unwrap();
        let (b, _) = heap.allocate(owner, layout(4096), &mut memory).unwrap();
        assert_eq!(heap.charge(owner),8192);
        assert_eq!(heap.allocate(owner, layout(1), &mut memory), Err(Error::Quota));
        let (c, _) = heap.allocate(Owner::Scratch(2), layout(8192), &mut memory).unwrap();
        assert_eq!(memory.count(), 2);
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
    #[test] fn adjacent_holes_coalesce_without_new_physical_pages() {
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();let owner=Owner::Scratch(10);
        let (a,ap)=heap.allocate(owner,layout(128),&mut memory).unwrap();
        let (b,bp)=heap.allocate(owner,layout(1024),&mut memory).unwrap();
        let (c,cp)=heap.allocate(owner,layout(1024),&mut memory).unwrap();
        let (d,dp)=heap.allocate(owner,layout(1024),&mut memory).unwrap();
        unsafe{ap.write(0x12);bp.write(0x34);cp.write(0x56);dp.write(0x78);}
        heap.release(b,&mut memory).unwrap();heap.release(c,&mut memory).unwrap();
        memory.oom=true;
        let (large,pointer)=heap.allocate(owner,layout(2048),&mut memory).unwrap();
        assert_eq!(pointer,bp);assert_eq!(memory.count(),1);assert_eq!(heap.charge(owner),8192);
        assert!(unsafe{core::slice::from_raw_parts(pointer,2048)}.iter().all(|b|*b==0));
        assert_eq!(unsafe{(ap.read(),dp.read())},(0x12,0x78));
        assert_eq!(heap.release(b,&mut memory),Err(Error::Stale));
        for token in [a,large,d]{heap.release(token,&mut memory).unwrap();}
        assert_eq!(memory.count(),0);
    }
    #[test] fn nonadjacent_holes_do_not_authorize_overlap_or_exceed_owner_quota() {
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();let owner=Owner::Scratch(11);
        let mut tokens=std::vec::Vec::new();
        for _ in 0..8{tokens.push(heap.allocate(owner,layout(1024),&mut memory).unwrap().0);}
        heap.release(tokens[1],&mut memory).unwrap();heap.release(tokens[3],&mut memory).unwrap();
        let next=heap.next_generation;
        assert_eq!(heap.allocate(owner,layout(2048),&mut memory),Err(Error::Quota));
        assert_eq!(heap.next_generation,next);assert_eq!(memory.count(),1);
        for (i,token) in tokens.into_iter().enumerate(){if i!=1&&i!=3{heap.release(token,&mut memory).unwrap();}}
        assert_eq!(memory.count(),0);
    }
    #[test] fn alignment_padding_is_reusable_but_never_part_of_another_lease() {
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();let owner=Owner::Scratch(12);
        let (a,ap)=heap.allocate(owner,Layout::from_size_align(13,1).unwrap(),&mut memory).unwrap();
        let (b,bp)=heap.allocate(owner,Layout::from_size_align(17,256).unwrap(),&mut memory).unwrap();
        let (c,cp)=heap.allocate(owner,Layout::from_size_align(32,16).unwrap(),&mut memory).unwrap();
        assert_eq!(bp as usize%256,0);assert_eq!(cp,unsafe{ap.add(16)});
        assert_eq!(memory.count(),1);
        for token in [a,b,c]{heap.release(token,&mut memory).unwrap();}
    }
    #[test] fn shared_neighbor_survives_release_failure_and_quarantine_retry() {
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();let owner=Owner::Scratch(13);
        let (a,_)=heap.allocate(owner,layout(128),&mut memory).unwrap();
        let (b,bp)=heap.allocate(owner,layout(6400),&mut memory).unwrap();
        unsafe{bp.write(0x77);}
        memory.fail_release=true;
        heap.release(a,&mut memory).unwrap(); // No PMM release with a live neighbor.
        assert_eq!(unsafe{bp.read()},0x77);assert_eq!(memory.count(),1);
        assert_eq!(heap.release(b,&mut memory),Err(Error::Release));
        assert_eq!(heap.charge(owner),8192);assert!(!heap.reclaim(&mut memory));
        memory.fail_release=false;assert!(heap.reclaim(&mut memory));
        assert_eq!(heap.charge(owner),0);assert_eq!(memory.count(),0);
    }
    #[test] fn metadata_limit_does_not_allocate_an_extra_arena() {
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();let owner=Owner::Scratch(14);
        let mut tokens=std::vec::Vec::new();
        for _ in 0..SLOTS{tokens.push(heap.allocate(owner,layout(8),&mut memory).unwrap().0);}
        assert_eq!(memory.count(),1);
        assert_eq!(heap.allocate(owner,layout(8),&mut memory),Err(Error::Metadata));
        for token in tokens{heap.release(token,&mut memory).unwrap();}
        assert_eq!(memory.count(),0);
    }
    #[test] fn sharing_receipt_requires_two_live_leases_and_is_once_per_arena() {
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();let owner=Owner::Scratch(15);
        let (a,_)=heap.allocate(owner,layout(128),&mut memory).unwrap();
        assert!(!heap.note_sharing(a));
        let (b,_)=heap.allocate(owner,layout(6400),&mut memory).unwrap();
        assert!(heap.note_sharing(a));assert!(!heap.note_sharing(b));
        for token in [a,b]{heap.release(token,&mut memory).unwrap();}
        assert!(!heap.note_sharing(a));
        let (c,_)=heap.allocate(owner,layout(128),&mut memory).unwrap();
        let (d,_)=heap.allocate(owner,layout(6400),&mut memory).unwrap();
        assert!(heap.note_sharing(c));
        for token in [c,d]{heap.release(token,&mut memory).unwrap();}
    }
    #[test] fn byte_sized_layout_cannot_resize_over_a_shared_neighbor() {
        let mut tiny = try_allocate(Owner::Scratch(76), Layout::from_size_align(1,1).unwrap()).unwrap();
        assert_eq!(tiny.bytes(), &[0]);tiny.bytes()[0]=0xff;
        assert_eq!(tiny.capacity,8);
        assert!(!tiny.resize_within_pages(4097));
        assert!(!tiny.resize_within_pages(4096));
        assert!(tiny.resize_within_pages(8));
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
