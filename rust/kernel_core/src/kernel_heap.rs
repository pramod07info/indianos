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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pressure {
    requests:u64, attempts:u64, failures:u64, reclamation_passes:u64,
    retries:u64, recovered:u64, reclaimed_bytes:u64, peak_reserved:u64,
    last_error:Option<Error>,frame_requests:u64,frame_attempts:u64,frame_failures:u64,
    frame_retries:u64,frame_recovered:u64,frame_last_error:Option<Error>,
}
impl Pressure {
    const fn new()->Self{Self{requests:0,attempts:0,failures:0,reclamation_passes:0,
        retries:0,recovered:0,reclaimed_bytes:0,peak_reserved:0,last_error:None,frame_requests:0,frame_attempts:0,frame_failures:0,
        frame_retries:0,frame_recovered:0,frame_last_error:None}}
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Snapshot { pressure:Pressure, reserved:u64, quarantined:u64, live_bytes:u64,
    live_allocations:u64, owner_reserved:u64, owner_kind:u64, owner_epoch:u64,backend_pending:u64 }
struct Heap { slots: [Slot; SLOTS], regions: [Region; SLOTS], next_generation: u64, pressure:Pressure, report_due:bool, reported_classes:u16 }
impl Heap {
    const fn new() -> Self { Self { slots: [const { Slot::new() }; SLOTS],
        regions: [const { Region::new() }; SLOTS], next_generation: 1, pressure:Pressure::new(), report_due:false, reported_classes:0 } }
    fn charge(&self, owner: Owner) -> usize {
        self.regions.iter().filter(|r|r.owner==Some(owner)).map(|r|r.buffer.charged()).sum()
    }
    /// Validate all external extent metadata before pointer arithmetic, scrub
    /// or physical release. Corruption fails closed without guessing ownership.
    fn integrity_ok(&self)->bool{
        if self.next_generation==0{return false;}
        for (index,region) in self.regions.iter().enumerate(){
            let charge=region.buffer.charged();
            if charge==0{
                if region.owner.is_some()||region.quarantined||region.sharing_logged||
                   !region.buffer.pointer().is_null()||region.buffer.physical()!=0||region.buffer.backing()!=0{return false;}
                continue;
            }
            let Some(owner)=region.owner else{return false;};
            if owner.epoch()==0||charge>owner.quota()||charge%4096!=0||
               region.buffer.pointer().is_null()||region.buffer.pointer() as usize%4096!=0||
               region.buffer.backing()==0||region.buffer.physical()%4096!=0||
               self.charge(owner)>owner.quota(){return false;}
            if matches!(owner,Owner::Scratch(_))&&charge!=8192{return false;}
            let Some(physical_end)=region.buffer.physical().checked_add(charge as u64)else{return false;};
            let virtual_start=region.buffer.pointer() as usize;
            let Some(virtual_end)=virtual_start.checked_add(charge)else{return false;};
            for other in self.regions.iter().skip(index+1).filter(|r|r.buffer.charged()!=0){
                let Some(other_physical_end)=other.buffer.physical().checked_add(other.buffer.charged() as u64)else{return false;};
                let other_start=other.buffer.pointer() as usize;
                let Some(other_end)=other_start.checked_add(other.buffer.charged())else{return false;};
                if region.buffer.backing()==other.buffer.backing()||
                   (region.buffer.physical()!=0&&other.buffer.physical()!=0&&
                    region.buffer.physical()<other_physical_end&&other.buffer.physical()<physical_end)||
                   (virtual_start<other_end&&other_start<virtual_end){return false;}
            }
            let live=self.slots.iter().any(|slot|slot.live&&slot.region==index);
            if region.quarantined==live{return false;}
        }
        for (index,slot) in self.slots.iter().enumerate().filter(|(_,slot)|slot.live){
            let Some(region)=self.regions.get(slot.region)else{return false;};
            let Some(end)=slot.offset.checked_add(slot.extent)else{return false;};
            if slot.extent==0||slot.offset%8!=0||slot.extent%8!=0||
               slot.generation==0||slot.generation>=self.next_generation||
               region.owner.is_none()||region.quarantined||end>region.buffer.charged(){return false;}
            if matches!(region.owner,Some(Owner::Frame(_)))&&
               (slot.offset%4096!=0||slot.extent%4096!=0){return false;}
            for other in self.slots.iter().skip(index+1).filter(|s|s.live&&s.region==slot.region){
                let Some(other_end)=other.offset.checked_add(other.extent)else{return false;};
                if slot.offset<other_end&&other.offset<end{return false;}
            }
        }
        true
    }
    fn snapshot(&self,owner:Owner)->Snapshot{
        Snapshot{pressure:self.pressure,
            reserved:self.regions.iter().map(|r|r.buffer.charged() as u64).sum(),
            quarantined:self.regions.iter().filter(|r|r.quarantined).map(|r|r.buffer.charged() as u64).sum(),
            live_bytes:self.slots.iter().filter(|s|s.live).fold(0u64,|sum,s|sum.saturating_add(s.extent as u64)),
            live_allocations:self.slots.iter().filter(|s|s.live).count() as u64,
            owner_reserved:self.charge(owner) as u64,owner_kind:match owner{Owner::Frame(_)=>1,Owner::Scratch(_)=>2},owner_epoch:owner.epoch(),backend_pending:0}
    }
    fn observe_peak(&mut self){
        let reserved:u64=self.regions.iter().map(|r|r.buffer.charged() as u64).sum();
        self.pressure.peak_reserved=self.pressure.peak_reserved.max(reserved);
    }
    /// At most one bounded reclamation pass and one allocation retry. A live
    /// lease is never revoked; retry requires a successful physical release.
    fn allocate_recovering(&mut self,owner:Owner,layout:Layout,backend:&mut(impl Backend+?Sized))
        ->Result<(Token,*mut u8),Error>{
        self.pressure.requests=self.pressure.requests.saturating_add(1);self.report_due=false;
        self.pressure.attempts=self.pressure.attempts.saturating_add(1);
        let mut result=self.allocate(owner,layout,backend);self.observe_peak();
        if matches!(result,Err(Error::Quota|Error::Metadata|Error::OutOfMemory))&&
           (self.regions.iter().any(|r|r.quarantined)||backend.has_pending()){
            self.pressure.reclamation_passes=self.pressure.reclamation_passes.saturating_add(1);
            let (released,_)=self.reclaim_count(backend);
            if released>0{
                self.pressure.retries=self.pressure.retries.saturating_add(1);
                self.pressure.attempts=self.pressure.attempts.saturating_add(1);
                result=self.allocate(owner,layout,backend);self.observe_peak();
                if result.is_ok(){self.pressure.recovered=self.pressure.recovered.saturating_add(1);}
            }
        }
        if let Err(error)=result{
            self.pressure.failures=self.pressure.failures.saturating_add(1);
            // At most one first report per class plus logarithmic repeats,
            // including when the caller alternates failure classes.
            let class=1u16<<error_code(error);
            self.report_due=self.reported_classes&class==0||self.pressure.failures.is_power_of_two();
            self.reported_classes|=class;
            self.pressure.last_error=Some(error);
        }
        result
    }
    /// Image/table frame requests use the same bounded quarantine recovery,
    /// but successful frames transfer directly to the caller's ownership list.
    fn allocate_frame_recovering(&mut self,backend:&mut(impl Backend+?Sized),
        mut allocate:impl FnMut()->Option<u64>)->Result<u64,Error>{
        self.pressure.frame_requests=self.pressure.frame_requests.saturating_add(1);self.report_due=false;
        let mut result=if !self.integrity_ok(){Err(Error::Integrity)}else{
            self.pressure.frame_attempts=self.pressure.frame_attempts.saturating_add(1);
            allocate().ok_or(Error::OutOfMemory)
        };
        if matches!(result,Err(Error::OutOfMemory))&&(self.regions.iter().any(|r|r.quarantined)||backend.has_pending()){
            self.pressure.reclamation_passes=self.pressure.reclamation_passes.saturating_add(1);
            if self.reclaim_count(backend).0>0{
                self.pressure.frame_retries=self.pressure.frame_retries.saturating_add(1);
                self.pressure.frame_attempts=self.pressure.frame_attempts.saturating_add(1);
                result=allocate().ok_or(Error::OutOfMemory);
                if result.is_ok(){self.pressure.frame_recovered=self.pressure.frame_recovered.saturating_add(1);}
            }
        }
        if let Err(error)=result{
            self.pressure.frame_failures=self.pressure.frame_failures.saturating_add(1);
            self.pressure.frame_last_error=Some(error);
            let class=1u16<<error_code(error);
            self.report_due=self.reported_classes&class==0||self.pressure.frame_failures.is_power_of_two();
            self.reported_classes|=class;
        }
        result
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
        if !self.integrity_ok(){return Err(Error::Integrity);}
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
        if !self.integrity_ok(){return Err(Error::Integrity);}
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
    fn reclaim_count(&mut self,backend:&mut(impl Backend+?Sized))->(usize,usize){
        if !self.integrity_ok(){return (0,1);}
        let mut released=0;let mut remaining=0;
        for region in &mut self.regions{
            if region.quarantined{
                let bytes=region.buffer.charged() as u64;
                if region.buffer.clear(backend).is_ok(){
                    region.owner=None;region.quarantined=false;region.sharing_logged=false;
                    self.pressure.reclaimed_bytes=self.pressure.reclaimed_bytes.saturating_add(bytes);
                    released+=1;
                }else{remaining+=1;}
            }
        }
        let orphan_bytes=backend.reclaim_pending();
        if orphan_bytes>0{released+=1;self.pressure.reclaimed_bytes=self.pressure.reclaimed_bytes.saturating_add(orphan_bytes);}
        if backend.has_pending(){remaining+=1;}
        (released,remaining)
    }
    fn reclaim(&mut self, backend: &mut (impl Backend + ?Sized)) -> bool {
        self.reclaim_count(backend).1==0
    }
    fn note_sharing(&mut self, token: Token) -> bool {
        if !self.integrity_ok(){return false;}
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
#[cfg(not(test))]
static HEAP_GATE:crate::retirement_gate::RetirementGate=crate::retirement_gate::RetirementGate::new();
#[cfg(test)]
std::thread_local!{static HEAP_GATE:crate::retirement_gate::RetirementGate=
    const{crate::retirement_gate::RetirementGate::new()};}
fn guarded<R>(gate:&crate::retirement_gate::RetirementGate,cpu:u32,masked:bool,
    f:impl FnOnce()->R)->Result<R,Error>{
    let _permit=gate.try_enter(cpu,masked).map_err(|_|Error::Owner)?;
    Ok(f())
}
/// Bind once during normal boot, before any GUI or user image can allocate.
/// A later caller cannot rebind the allocator to an application processor.
pub(crate) fn initialize()->bool{
    #[cfg(not(test))]
    {HEAP_GATE.register_bsp(crate::user_runtime::r120_cpu_id(),crate::kernel_buffer::masked()).is_ok()&&
     guarded(&HEAP_GATE,crate::user_runtime::r120_cpu_id(),crate::kernel_buffer::masked(),crate::kernel_arena::initialize).unwrap_or(false)}
    #[cfg(test)]
    {HEAP_GATE.with(|gate|gate.register_bsp(0,true).is_ok())}
}
pub(crate) fn admission_available()->bool{
    #[cfg(not(test))]
    {guarded(&HEAP_GATE,crate::user_runtime::r120_cpu_id(),crate::kernel_buffer::masked(),||()).is_ok()}
    #[cfg(test)]
    {initialize()&&HEAP_GATE.with(|gate|guarded(gate,0,true,||()).is_ok())}
}
fn with_heap<R>(f: impl FnOnce(&mut Heap, &mut dyn Backend) -> R) -> Result<R,Error> {
    #[cfg(not(test))]
    {guarded(&HEAP_GATE,crate::user_runtime::r120_cpu_id(),crate::kernel_buffer::masked(),||{
        let mut physical=crate::kernel_buffer::Physical;
        // The exact live heap gate permit precedes creation of this reference.
        unsafe{f(&mut *core::ptr::addr_of_mut!(HEAP),&mut physical)}
    })}
    #[cfg(test)]
    {initialize();HEAP_GATE.with(|gate|guarded(gate,0,true,||HOST.with(|state|{
        let mut state=state.borrow_mut();let(heap,memory)=&mut *state;f(heap,memory)
    })))}
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
        if self.token.is_some_and(|token|with_heap(|heap,_|heap.note_sharing(token)).unwrap_or(false)) {
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
        with_heap(|heap, backend| heap.release(token, backend))?
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
    let (result,snapshot,report,recovered)=with_heap(|heap,backend|{
        let before=heap.pressure.recovered;
        let result=heap.allocate_recovering(owner,layout,backend)
            .map(|(token,pointer)|(token,pointer,heap.slots[token.slot].extent));
        let mut snapshot=heap.snapshot(owner);snapshot.backend_pending=backend.pending_bytes();
        (result,snapshot,heap.report_due,heap.pressure.recovered>before)
    })?;
    if report{report_pressure(snapshot);}
    if recovered{
        #[cfg(not(test))]
        unsafe{
            unsafe extern "C"{fn serial_write(message:*const core::ffi::c_char);}
            serial_write(b"IndianOS: heap allocation recovered after quarantine reclaim\n\0".as_ptr().cast());
        }
    }
    let (token,pointer,capacity)=result?;
    Ok(Allocation { token: Some(token), pointer, length: layout.size(), capacity,
        owner, _single_cpu: PhantomData })
}
fn error_code(error:Error)->u8{
    match error{Error::Geometry=>1,Error::Quota=>2,Error::OutOfMemory=>3,
        Error::Mapping=>4,Error::Release=>5,Error::Owner=>6,Error::Metadata=>7,
        Error::Alignment=>8,Error::Stale=>9,Error::Integrity=>10}
}
#[cfg(not(test))]
fn report_pressure(snapshot:Snapshot){
    // Reuse the runtime's bounded, allocation-free hexadecimal diagnostic writer.
    let reason=snapshot.pressure.last_error.map_or(0,|error|u64::from(error_code(error)));
    for (label,value) in [
        (&b"IndianOS: heap failure reason=0x\0"[..],reason),
        (&b"IndianOS: heap reserved bytes=0x\0"[..],snapshot.reserved),
        (&b"IndianOS: heap quarantined bytes=0x\0"[..],snapshot.quarantined),
        (&b"IndianOS: heap backend pending bytes=0x\0"[..],snapshot.backend_pending),
        (&b"IndianOS: heap owner reserved bytes=0x\0"[..],snapshot.owner_reserved),
        (&b"IndianOS: heap live extents bytes=0x\0"[..],snapshot.live_bytes),
        (&b"IndianOS: heap live allocations=0x\0"[..],snapshot.live_allocations),
        (&b"IndianOS: heap peak reserved bytes=0x\0"[..],snapshot.pressure.peak_reserved),
        (&b"IndianOS: heap failed requests=0x\0"[..],snapshot.pressure.failures),
        (&b"IndianOS: heap owner kind=0x\0"[..],snapshot.owner_kind),
        (&b"IndianOS: heap owner epoch=0x\0"[..],snapshot.owner_epoch),
        (&b"IndianOS: heap allocation attempts=0x\0"[..],snapshot.pressure.attempts),
        (&b"IndianOS: heap pressure retries=0x\0"[..],snapshot.pressure.retries),
        (&b"IndianOS: heap recovered requests=0x\0"[..],snapshot.pressure.recovered),
        (&b"IndianOS: heap reclaimed bytes=0x\0"[..],snapshot.pressure.reclaimed_bytes),
    ]{crate::user_runtime::fault_hex(label,value);}
}
#[cfg(test)]
fn report_pressure(_:Snapshot){}

/// Normal-desktop PMM admission for image and page-table backends. Failure
/// leaves out unchanged. No caller state is discarded and no task is killed.
/// # Safety: out is exclusive kernel storage for one u64 physical frame.
pub(crate) unsafe fn allocate_process_frame(out:*mut u64)->bool{
    if out.is_null(){return false;}
    let Ok((result,snapshot,report))=with_heap(|heap,backend|{
        let result=heap.allocate_frame_recovering(backend,||{
            let mut frame=0;
            unsafe{crate::pmm::pmm_allocate_frame(&mut frame)}.then_some(frame)
        });
        let mut snapshot=heap.snapshot(Owner::Frame(0));snapshot.backend_pending=backend.pending_bytes();
        (result,snapshot,heap.report_due)
    })else{return false;};
    #[cfg(not(test))]
    if report{
        crate::user_runtime::fault_hex(b"IndianOS: physical request failure reason=0x\0",
            snapshot.pressure.frame_last_error.map_or(0,|error|u64::from(error_code(error))));
        crate::user_runtime::fault_hex(b"IndianOS: physical failed requests=0x\0",snapshot.pressure.frame_failures);
        crate::user_runtime::fault_hex(b"IndianOS: physical pressure retries=0x\0",snapshot.pressure.frame_retries);
        crate::user_runtime::fault_hex(b"IndianOS: physical recovered requests=0x\0",snapshot.pressure.frame_recovered);
        crate::user_runtime::fault_hex(b"IndianOS: physical heap quarantine bytes=0x\0",snapshot.quarantined);
        crate::user_runtime::fault_hex(b"IndianOS: physical backend pending bytes=0x\0",snapshot.backend_pending);
    }
    #[cfg(test)]let _=(snapshot,report);
    if let Ok(frame)=result{unsafe{out.write(frame)};true}else{false}
}
/// # Safety: frame is exclusively owned by the inactive image/table caller.
pub(crate) unsafe fn release_process_frame(frame:u64)->bool{
    with_heap(|_,_|unsafe{crate::pmm::pmm_release_frame(frame)}).unwrap_or(false)
}
pub(crate) fn reclaim_retired() -> bool {
    with_heap(|heap, backend| heap.reclaim(backend)).unwrap_or(false)
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
        next: u64, pub oom: bool, pub fail_release: bool, pub block_until_release:bool, pub allocations:usize, pub releases:usize }
    impl Memory {
        pub const fn new() -> Self { Self { runs: std::vec::Vec::new(), next: 4096, oom: false, fail_release: false, block_until_release:false, allocations:0, releases:0 } }
        pub fn count(&self) -> usize { self.runs.len() }
    }
    unsafe impl Backend for Memory {
        fn allocate(&mut self, pages: u32) -> Result<crate::kernel_buffer::Backing, Error> {
            self.allocations+=1;
            if self.oom||self.block_until_release { return Err(Error::OutOfMemory); }
            let physical = self.next; self.next += u64::from(pages) * 4096;
            let mut bytes = std::vec![Page([0xdeadbeef;1024]); pages as usize];
            let pointer = bytes.as_mut_ptr().cast(); self.runs.push((physical, bytes));
            Ok(crate::kernel_buffer::Backing{key:physical,physical:Some(physical),pointer})
        }
        fn release(&mut self, physical: u64, _: u32) -> bool {
            self.releases+=1;
            if self.fail_release { return false; }
            let Some(index) = self.runs.iter().position(|r| r.0 == physical) else { return false; };
            assert!(self.runs[index].1.iter().all(|page| page.0.iter().all(|p| *p == 0)));
            self.runs.remove(index);self.block_until_release=false; true
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
    #[test] fn pressure_reclaims_quarantined_owner_quota_then_retries_once() {
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();let owner=Owner::Scratch(20);
        let (old,_)=heap.allocate(owner,layout(8192),&mut memory).unwrap();
        memory.fail_release=true;assert_eq!(heap.release(old,&mut memory),Err(Error::Release));
        memory.fail_release=false;
        let (new,_)=heap.allocate_recovering(owner,layout(6400),&mut memory).unwrap();
        let snapshot=heap.snapshot(owner);
        assert_eq!((snapshot.reserved,snapshot.quarantined,snapshot.owner_reserved),(8192,0,8192));
        assert_eq!((snapshot.live_bytes,snapshot.live_allocations),(6400,1));
        assert_eq!((snapshot.pressure.requests,snapshot.pressure.attempts,snapshot.pressure.failures),(1,2,0));
        assert_eq!((snapshot.pressure.retries,snapshot.pressure.recovered,snapshot.pressure.reclaimed_bytes),(1,1,8192));
        assert_eq!(heap.release(old,&mut memory),Err(Error::Stale));
        heap.release(new,&mut memory).unwrap();assert_eq!(memory.count(),0);
    }
    #[test] fn global_oom_can_recover_from_another_owners_retired_region() {
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();
        let (old,_)=heap.allocate(Owner::Scratch(21),layout(128),&mut memory).unwrap();
        memory.fail_release=true;assert_eq!(heap.release(old,&mut memory),Err(Error::Release));
        memory.fail_release=false;memory.block_until_release=true;
        let attempts=memory.allocations;
        let (new,_)=heap.allocate_recovering(Owner::Scratch(22),layout(128),&mut memory).unwrap();
        assert_eq!(memory.allocations-attempts,2);assert_eq!(heap.charge(Owner::Scratch(21)),0);
        assert_eq!(heap.pressure.recovered,1);heap.release(new,&mut memory).unwrap();
    }
    #[test] fn persistent_oom_has_one_retry_and_preserves_a_live_neighbors_bytes() {
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();
        let (live,pointer)=heap.allocate(Owner::Scratch(23),layout(128),&mut memory).unwrap();
        unsafe{pointer.write(0xab);}
        let (retired,_)=heap.allocate(Owner::Scratch(24),layout(128),&mut memory).unwrap();
        memory.fail_release=true;assert_eq!(heap.release(retired,&mut memory),Err(Error::Release));
        memory.fail_release=false;memory.oom=true;
        let attempts=memory.allocations;
        assert_eq!(heap.allocate_recovering(Owner::Scratch(25),layout(128),&mut memory),Err(Error::OutOfMemory));
        assert_eq!(memory.allocations-attempts,2);assert_eq!(heap.pressure.retries,1);
        assert_eq!(heap.pressure.failures,1);assert_eq!(heap.pressure.recovered,0);
        assert_eq!(unsafe{pointer.read()},0xab);assert_eq!(memory.count(),1);
        heap.release(live,&mut memory).unwrap();
    }
    #[test] fn failed_cleanup_is_one_pass_and_cannot_reset_quota_or_tokens() {
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();
        for epoch in 1..=SLOTS as u64{
            memory.fail_release=false;
            let (token,_)=heap.allocate(Owner::Scratch(epoch),layout(128),&mut memory).unwrap();
            memory.fail_release=true;assert_eq!(heap.release(token,&mut memory),Err(Error::Release));
        }
        let releases=memory.releases;let next=heap.next_generation;
        assert_eq!(heap.allocate_recovering(Owner::Frame(99),layout(128),&mut memory),Err(Error::Metadata));
        assert_eq!(memory.releases-releases,SLOTS);assert_eq!(heap.next_generation,next);
        assert_eq!(heap.pressure.retries,0);assert_eq!(heap.pressure.reclamation_passes,1);
        assert_eq!(heap.snapshot(Owner::Scratch(1)).quarantined,8192*SLOTS as u64);
        memory.fail_release=false;assert!(heap.reclaim(&mut memory));assert_eq!(memory.count(),0);
    }
    #[test] fn validation_errors_and_oom_without_quarantine_do_not_retry() {
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();memory.oom=true;
        assert_eq!(heap.allocate_recovering(Owner::Scratch(30),layout(128),&mut memory),Err(Error::OutOfMemory));
        assert_eq!(memory.allocations,1);assert_eq!(memory.releases,0);
        assert_eq!(heap.allocate_recovering(Owner::Scratch(30),layout(0),&mut memory),Err(Error::Geometry));
        assert_eq!(memory.allocations,1);assert_eq!(heap.pressure.reclamation_passes,0);
        assert_eq!(heap.pressure.attempts,2);assert_eq!(heap.pressure.failures,2);
    }
    #[test] fn diagnostic_reports_are_bounded_across_alternating_failure_classes() {
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();memory.oom=true;let mut reports=0;
        for i in 0..100{
            let size=if i%2==0{0}else{128};
            assert!(heap.allocate_recovering(Owner::Scratch(31),layout(size),&mut memory).is_err());
            if heap.report_due{reports+=1;}
        }
        assert!(reports<=9);assert_eq!(heap.pressure.failures,100);
        heap.pressure.requests=u64::MAX;heap.pressure.attempts=u64::MAX;heap.pressure.failures=u64::MAX;
        assert!(heap.allocate_recovering(Owner::Scratch(31),layout(0),&mut memory).is_err());
        assert_eq!((heap.pressure.requests,heap.pressure.attempts,heap.pressure.failures),(u64::MAX,u64::MAX,u64::MAX));
    }
    #[test] fn admission_denial_never_executes_heap_or_backend_access() {
        use crate::retirement_gate::RetirementGate;
        let gate=RetirementGate::new();let called=core::cell::Cell::new(false);
        assert_eq!(guarded(&gate,5,true,||called.set(true)),Err(Error::Owner));
        gate.register_bsp(5,true).unwrap();
        for (cpu,masked) in [(6,true),(5,false)]{
            assert_eq!(guarded(&gate,cpu,masked,||called.set(true)),Err(Error::Owner));
        }
        let held=gate.try_enter(5,true).unwrap();
        assert_eq!(guarded(&gate,5,true,||called.set(true)),Err(Error::Owner));
        assert!(!called.get());drop(held);
        assert_eq!(guarded(&gate,5,true,||called.set(true)),Ok(()));assert!(called.get());
    }
    #[test] fn nested_production_heap_entry_is_rejected_before_refcell_borrow() {
        let nested=with_heap(|heap,_|{
            let before=heap.pressure;
            let denied=with_heap(|_,_|panic!("reentrant heap access"));
            assert_eq!(heap.pressure,before);denied
        });
        assert_eq!(nested,Ok(Err::<(),_>(Error::Owner)));
        assert!(admission_available());
    }
    #[test] fn corrupt_extent_refuses_scrub_allocation_and_reclamation_without_backend_calls(){
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();let owner=Owner::Scratch(40);
        let (token,pointer)=heap.allocate(owner,layout(128),&mut memory).unwrap();
        unsafe{pointer.write(0x5a);}let offset=heap.slots[token.slot].offset;
        heap.slots[token.slot].offset=usize::MAX;
        let calls=(memory.allocations,memory.releases);
        assert_eq!(heap.release(token,&mut memory),Err(Error::Integrity));
        assert_eq!(heap.allocate_recovering(owner,layout(16),&mut memory),Err(Error::Integrity));
        assert!(!heap.reclaim(&mut memory));assert!(!heap.note_sharing(token));
        assert_eq!((memory.allocations,memory.releases),calls);assert_eq!(unsafe{pointer.read()},0x5a);
        heap.slots[token.slot].offset=offset;
        assert!(heap.integrity_ok());heap.release(token,&mut memory).unwrap();
    }
    #[test] fn overlapping_leases_and_live_quarantine_are_rejected_before_free(){
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();let owner=Owner::Scratch(41);
        let (a,_)=heap.allocate(owner,layout(128),&mut memory).unwrap();
        let (b,pointer)=heap.allocate(owner,layout(128),&mut memory).unwrap();
        unsafe{pointer.write(0x33);}let offset=heap.slots[b.slot].offset;
        heap.slots[b.slot].offset=heap.slots[a.slot].offset;
        assert_eq!(heap.release(a,&mut memory),Err(Error::Integrity));
        heap.slots[b.slot].offset=offset;
        let region=heap.slots[a.slot].region;heap.regions[region].quarantined=true;
        assert!(!heap.reclaim(&mut memory));assert_eq!(unsafe{pointer.read()},0x33);
        heap.regions[region].quarantined=false;
        assert!(heap.integrity_ok());for token in [a,b]{heap.release(token,&mut memory).unwrap();}
    }
    #[test] fn invalid_region_and_generation_metadata_cannot_free_another_allocation(){
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();
        let (token,_)=heap.allocate(Owner::Scratch(42),layout(128),&mut memory).unwrap();
        let region=heap.slots[token.slot].region;heap.slots[token.slot].region=SLOTS;
        assert_eq!(heap.release(token,&mut memory),Err(Error::Integrity));
        heap.slots[token.slot].region=region;let generation=heap.slots[token.slot].generation;
        heap.slots[token.slot].generation=heap.next_generation;
        assert_eq!(heap.release(token,&mut memory),Err(Error::Integrity));
        heap.slots[token.slot].generation=generation;heap.release(token,&mut memory).unwrap();
        assert!(heap.integrity_ok());assert_eq!(memory.count(),0);
    }
    #[test] fn overlapping_backing_regions_cannot_authorize_double_free(){
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();
        let (a,_)=heap.allocate(Owner::Scratch(43),layout(128),&mut memory).unwrap();
        let (b,_)=heap.allocate(Owner::Scratch(44),layout(128),&mut memory).unwrap();
        let ar=heap.slots[a.slot].region;let br=heap.slots[b.slot].region;
        // Deliberately corrupt external metadata; Buffer has no automatic free.
        let duplicate=unsafe{core::ptr::read(&heap.regions[ar].buffer)};
        let original=core::mem::replace(&mut heap.regions[br].buffer,duplicate);
        let releases=memory.releases;
        assert_eq!(heap.release(a,&mut memory),Err(Error::Integrity));
        assert_eq!(heap.release(b,&mut memory),Err(Error::Integrity));
        assert_eq!(memory.releases,releases);
        heap.regions[br].buffer=original;
        assert!(heap.integrity_ok());for token in [a,b]{heap.release(token,&mut memory).unwrap();}
    }
    #[test]fn image_frame_oom_reclaims_only_retired_heap_backing_and_retries_once(){
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();let owner=Owner::Scratch(50);
        let (live,pointer)=heap.allocate(owner,layout(128),&mut memory).unwrap();unsafe{pointer.write(0x55);}
        let (retired,_)=heap.allocate(Owner::Scratch(51),layout(128),&mut memory).unwrap();
        memory.fail_release=true;assert_eq!(heap.release(retired,&mut memory),Err(Error::Release));
        memory.fail_release=false;let mut calls=0;
        let result=heap.allocate_frame_recovering(&mut memory,||{calls+=1;if calls==2{Some(0x10000)}else{None}});
        assert_eq!(result,Ok(0x10000));assert_eq!(calls,2);assert_eq!(unsafe{pointer.read()},0x55);
        assert_eq!(heap.pressure.frame_recovered,1);assert_eq!(heap.pressure.frame_failures,0);
        heap.release(live,&mut memory).unwrap();assert_eq!(memory.count(),0);
    }
    #[test]fn image_frame_pressure_without_reclaimable_memory_is_one_attempt(){
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();let mut calls=0;
        assert_eq!(heap.allocate_frame_recovering(&mut memory,||{calls+=1;None}),Err(Error::OutOfMemory));
        assert_eq!(calls,1);assert_eq!(heap.pressure.frame_retries,0);
        assert_eq!(heap.pressure.frame_failures,1);assert_eq!(memory.releases,0);
    }
    #[test]fn image_frame_requests_refuse_corrupt_heap_before_pmm_access(){
        let mut heap=Heap::new();let mut memory=fixture::Memory::new();let mut calls=0;
        heap.next_generation=0;
        assert_eq!(heap.allocate_frame_recovering(&mut memory,||{calls+=1;Some(0x10000)}),Err(Error::Integrity));
        assert_eq!(calls,0);assert_eq!(heap.pressure.frame_attempts,0);
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
