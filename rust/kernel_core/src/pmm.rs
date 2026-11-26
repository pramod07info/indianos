//! Fixed-size physical frame allocator, IndianOS Rust migration R003.
//!
//! Owns ONLY frames from non-overlapping usable boot memory-map regions,
//! capped at 8 GiB worth of frames. The bitmap describes frame ownership;
//! it never dereferences arbitrary physical addresses. A tiny unsafe
//! single-CPU cell is necessary for C ABI statics: no SMP or interrupt-side
//! allocator access is allowed in this boot-stage kernel.

use core::{cell::UnsafeCell, ffi::c_void, ptr};

const PAGE: u64 = 4096;
const MAX_SPANS: usize = 128;
const MAX_FRAMES: usize = 8 * 1024 * 1024 * 1024 / PAGE as usize;
const MAX_ENTRIES: u64 = 4096;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Region { pub base: u64, pub length: u64, pub kind: u64 }
pub type RegionReader = Option<unsafe extern "C" fn(u64, *mut Region, *mut c_void) -> bool>;
#[derive(Copy, Clone)]
struct Span { base: u64, count: u64, bit_offset: u64 }
impl Span {
    const fn empty() -> Self { Self { base: 0, count: 0, bit_offset: 0 } }
}
struct State {
    spans: [Span; MAX_SPANS],
    bitmap: [u8; MAX_FRAMES / 8],
    count: usize,
    total: u64,
    used: u64,
    cursor: u64,
    initialized: bool,
}
impl State {
    const fn empty() -> Self {
        Self {
            spans: [Span::empty(); MAX_SPANS],
            bitmap: [0; MAX_FRAMES / 8],
            count: 0, total: 0, used: 0, cursor: 0, initialized: false,
        }
    }
    fn reset(&mut self) {
        self.initialized = false;
        self.count = 0;
        self.total = 0;
        self.used = 0;
        self.cursor = 0;
    }
    fn allocated(&self, bit: u64) -> bool {
        self.bitmap[(bit / 8) as usize] & (1 << (bit % 8)) != 0
    }
    fn set_bit(&mut self, bit: u64, value: bool) {
        let slot = &mut self.bitmap[(bit / 8) as usize];
        let mask = 1u8 << (bit % 8);
        if value { *slot |= mask; } else { *slot &= !mask; }
    }
    /// Corrupt allocator accounting must never report fabricated free frames.
    fn checked_free(&self)->u64 {
        if !self.initialized {return 0;}
        self.total.checked_sub(self.used).unwrap_or(0)
    }
    /// Diagnostic count must agree with the owned bitmap and span geometry.
    /// This slower verification is for status/acceptance, never the hot
    /// single-page allocation path.
    fn trusted_free_count(&self)->u64 {
        if self.verify_frame_accounting(){self.total-self.used}else{0}
    }
    fn physical_at(&self, bit: u64) -> Option<u64> {
        // Corrupt span metadata must never panic or wrap a physical frame
        // address into another owner's page, even in recovery paths.
        for span in self.spans.get(..self.count)? {
            let Some(index) = bit.checked_sub(span.bit_offset) else { continue; };
            if index < span.count {
                return index.checked_mul(PAGE)
                    .and_then(|distance| span.base.checked_add(distance));
            }
        }
        None
    }
}
struct SingleCpuCell(UnsafeCell<State>);
// SAFETY: The boot kernel uses one CPU, with interrupts disabled on all
// calls into PMM. No concurrent/reentrant calls; migration CI enforces this
// invariant only operationally, not with a locking primitive.
unsafe impl Sync for SingleCpuCell {}
static PMM: SingleCpuCell = SingleCpuCell(UnsafeCell::new(State::empty()));
unsafe fn state() -> &'static mut State {
    // SAFETY: Single CPU, caller has exclusive use of allocator until return.
    unsafe { &mut *PMM.0.get() }
}
fn overlap(a0: u64, a1: u64, b0: u64, b1: u64) -> bool {
    a0 < b1 && b0 < a1
}
unsafe fn read_region(
    callback: unsafe extern "C" fn(u64, *mut Region, *mut c_void) -> bool,
    index: u64, context: *mut c_void,
) -> Option<Region> {
    let mut region = Region { base: 0, length: 0, kind: 0 };
    if unsafe { callback(index, &mut region, context) } &&
        region.length != 0 && region.base.checked_add(region.length).is_some() {
        Some(region)
    } else {
        None
    }
}

/// # Safety
/// \`reader\` is a trusted, non-reentrant kernel callback providing valid
/// boot memory-map regions. PMM must not be called from another CPU or IRQ.
#[no_mangle]
pub unsafe extern "C" fn pmm_initialize(
    region_count: u64, reader: RegionReader, context: *mut c_void,
) -> bool {
    let s = unsafe { state() };
    unsafe { initialize_once(s, region_count, reader, context) }
}

/// Never reset a live PMM while callers may own allocated frames.
/// # Safety: reader/context satisfy pmm_initialize's kernel-only contract.
unsafe fn initialize_once(s:&mut State,region_count:u64,reader:RegionReader,
    context:*mut c_void)->bool {
    if s.initialized {return false;}
    unsafe {initialize(s,region_count,reader,context)}
}

// Separate state permits adversarial map tests without racing the ABI global.
unsafe fn initialize(s: &mut State, region_count: u64, reader: RegionReader,
    context: *mut c_void) -> bool {
    s.reset();
    let Some(callback) = reader else { return false; };
    if !(1..=MAX_ENTRIES).contains(&region_count) { return false; }
    // Validate the WHOLE firmware map before applying the bitmap/span caps.
    // A large usable region must not hide malformed or overlapping tail
    // entries, including reserved ranges outside the frames we can manage.
    for index in 0..region_count {
        let Some(region) = (unsafe { read_region(callback, index, context) })
            else { return false; };
        if region.kind != 0 { continue; }
        for other_index in 0..region_count {
            if other_index == index { continue; }
            let Some(other) = (unsafe { read_region(callback, other_index, context) })
                else { return false; };
            if overlap(region.base, region.base + region.length,
                other.base, other.base + other.length) { return false; }
        }
    }
    for index in 0..region_count {
        let Some(region) = (unsafe { read_region(callback, index, context) }) else {
            s.reset();
            return false;
        };
        if region.kind != 0 { continue; }
        let Some(rounded) = region.base.checked_add(PAGE - 1) else {
            s.reset();
            return false;
        };
        let mut begin = rounded & !(PAGE - 1);
        if begin < PAGE { begin = PAGE; }
        let end = (region.base + region.length) & !(PAGE - 1);
        if end <= begin { continue; }
        // The reader is a trusted immutable boot-map adapter; preflight
        // above validated even sub-page overlaps before inward alignment.
        if s.total == MAX_FRAMES as u64 { break; }
        if s.count == MAX_SPANS { s.reset(); return false; }
        let mut count = (end - begin) / PAGE;
        count = count.min(MAX_FRAMES as u64 - s.total);
        s.spans[s.count] = Span {
            base: begin, count, bit_offset: s.total,
        };
        s.count += 1;
        s.total += count;
    }
    if s.total == 0 { s.reset(); return false; }
    for slot in s.bitmap.iter_mut().take(s.total.div_ceil(8) as usize) {
        *slot = 0;
    }
    s.initialized = true;
    true
}

/// # Safety
/// \`physical\` is a valid writable pointer or null; single-CPU caller.
// The C ABI and tests use one allocator path. Corrupt cursor/counters
// fail before bitmap mutation. Global usage remains BSP+IRQs-disabled.
impl State {
    // Reject a corrupt span table before indexing the fixed-size bitmap.
    fn single_frame_geometry_valid(&self)->bool{
        if !self.initialized || self.total==0 ||
           self.total>MAX_FRAMES as u64 || self.used>self.total ||
           self.cursor>=self.total || self.count==0 || self.count>MAX_SPANS {
            return false;
        }
        let mut offset=0u64;
        for (index,span) in self.spans[..self.count].iter().enumerate() {
            if span.count==0 || span.bit_offset!=offset ||
               span.base<PAGE || span.base%PAGE!=0 {return false;}
            let Some(physical_end)=span.count.checked_mul(PAGE)
                .and_then(|len|span.base.checked_add(len))else{return false;};
            // Separate bitmap bits must never alias the same physical frame.
            for previous in &self.spans[..index]{
                let Some(previous_end)=previous.count.checked_mul(PAGE)
                    .and_then(|len|previous.base.checked_add(len))else{return false;};
                if span.base<previous_end && previous.base<physical_end{return false;}
            }
            let Some(end)=offset.checked_add(span.count)else{return false;};
            if end>self.total{return false;}
            offset=end;
        }
        offset==self.total
    }
    fn claim_single_frame(&mut self)->Option<u64>{
        if !self.single_frame_geometry_valid() ||
           self.used>=self.total {return None;}
        for scanned in 0..self.total{
            let mut bit=self.cursor+scanned;
            if bit>=self.total{bit-=self.total;}
            if !self.allocated(bit){
                let address=self.physical_at(bit)?;
                self.set_bit(bit,true);
                self.used+=1;
                self.cursor=if bit+1==self.total{0}else{bit+1};
                return Some(address);
            }
        }
        None
    }
}
#[no_mangle]
pub unsafe extern "C" fn pmm_allocate_frame(physical: *mut u64) -> bool {
    if physical.is_null() { return false; }
    let s = unsafe { state() };
    let Some(address)=s.claim_single_frame()else{return false;};
    unsafe{ptr::write(physical,address);}
    true
}

/// Acquire N physically contiguous 4 KiB frames from ONE usable RAM span.
/// Required for legacy virtio vrings which do not support scattered ring pages.
/// # Safety
/// Single BSP with interrupts disabled; out is exclusive, aligned and writable.
impl State {
    /// Only a verified span table may authorize multi-frame DMA runs.
    fn claim_contiguous_run(&mut self,count:u32)->Option<u64>{
        self.claim_contiguous_run_bounded(count,16)
    }
    /// Kernel-buffer allocation shares the bitmap and first-fit policy, but
    /// has its own hard ceiling. Legacy DMA callers retain the 16-page ABI.
    fn claim_contiguous_run_bounded(&mut self,count:u32,limit:u32)->Option<u64>{
        if count==0||count>limit||limit>2500||!self.verify_frame_accounting(){return None;}
        let want=u64::from(count);
        if want>self.total-self.used{return None;}
        for span_index in 0..self.count {
            let span=self.spans[span_index];let mut free=0;
            for offset in 0..span.count {
                if self.allocated(span.bit_offset+offset){free=0;continue;}
                free+=1;
                if free!=want{continue;}
                let start=offset+1-want;let first_bit=span.bit_offset+start;
                let first=span.base.checked_add(start.checked_mul(PAGE)?)?;
                for i in 0..want{self.set_bit(first_bit+i,true);}
                self.used+=want;self.cursor=(first_bit+want)%self.total;
                return Some(first);
            }
        }
        None
    }
}
#[no_mangle]
pub unsafe extern "C" fn pmm_allocate_contiguous_frames(count:u32,out:*mut u64)->bool{
    if out.is_null(){return false;}
    let s=unsafe{state()};
    let Some(first)=s.claim_contiguous_run(count)else{return false;};
    unsafe{ptr::write(out,first);}
    true
}

/// # Safety
/// Caller passes a previously allocated physical frame and uses PMM only on
/// one CPU. Duplicate frees, unaligned frames and reserved RAM are rejected.
#[no_mangle]
pub unsafe extern "C" fn pmm_release_frame(physical: u64) -> bool {
    let s = unsafe { state() };
    if !s.single_frame_geometry_valid() || physical & (PAGE - 1) != 0 { return false; }
    for i in 0..s.count {
        let span = s.spans[i];
        if physical < span.base { continue; }
        let diff = physical - span.base;
        if diff / PAGE >= span.count { continue; }
        let bit = span.bit_offset + diff / PAGE;
        if !s.allocated(bit) || s.used == 0 { return false; }
        s.set_bit(bit, false);
        s.used -= 1;
        s.cursor = bit;
        return true;
    }
    false
}
#[no_mangle]
pub extern "C" fn pmm_total_frames() -> u64 {
    let s = unsafe { state() };
    if s.initialized { s.total } else { 0 }
}
#[no_mangle]
pub extern "C" fn pmm_free_frames() -> u64 {
    let s = unsafe { state() };
    s.trusted_free_count()
}
/// Allocate two distinct frames, recycle one and always attempt cleanup
/// of live owners after an intermediate failure. The caller's allocation
/// backend owns the generation and must not return a borrowed frame.
fn pmm_self_test_roundtrip<T>(
    context:&mut T,
    mut allocate:impl FnMut(&mut T)->Option<u64>,
    mut release:impl FnMut(&mut T,u64)->bool,
)->bool{
    let Some(first)=allocate(context)else{return false;};
    if first==0 {
        let _=release(context,first);
        return false;
    }
    let Some(second)=allocate(context)else{
        let _=release(context,first);
        return false;
    };
    if second==0 || second==first{
        if second!=first && second!=0 {let _=release(context,second);}
        let _=release(context,first);
        return false;
    }
    if !release(context,second){
        let _=release(context,first);
        return false;
    }
    let recycled=allocate(context);
    let correct=recycled==Some(second);
    let recycled_released=if let Some(frame)=recycled{
        if frame!=first && frame!=0 {release(context,frame)}else{false}
    }else{false};
    let first_released=release(context,first);
    correct && recycled_released && first_released
}
#[no_mangle]
pub extern "C" fn pmm_self_test() -> bool {
    if pmm_free_frames() < 2 { return false; }
    let before=pmm_free_frames();
    let success=pmm_self_test_roundtrip(&mut (),
        |_|{let mut frame=0; if unsafe{pmm_allocate_frame(&mut frame)}{Some(frame)}else{None}},
        |_,frame|unsafe{pmm_release_frame(frame)});
    success && pmm_free_frames()==before
}
#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" fn map_reader(index:u64,out:*mut Region,ctx:*mut c_void)->bool {
        let map=unsafe { &*(ctx.cast::<Vec<Region>>()) };
        let Some(region)=map.get(index as usize) else { return false; };
        unsafe { out.write(*region); }
        true
    }
    fn init_map(state:&mut State,mut map:Vec<Region>)->bool {
        unsafe { initialize(state,map.len() as u64,Some(map_reader),
            (&mut map as *mut Vec<Region>).cast()) }
    }
    #[test]
    fn capacity_limit_does_not_hide_invalid_firmware_tail() {
        let mut s=State::empty();
        let large=Region{base:PAGE,length:MAX_FRAMES as u64*PAGE,kind:0};
        let tail=Region{base:0x10_0000_0000,length:PAGE,kind:0};
        let bad=Region{base:tail.base+1,length:1,kind:2};
        // A tail usable region conflicts with reserved bytes, even
        // if the allocator has already filled its bitmap from `large`.
        assert!(!init_map(&mut s,vec![large,tail,bad]));
        assert!(!s.initialized); assert_eq!(s.total,0);
        for bad in [Region{base:u64::MAX,length:2,kind:1},
            Region{base:0x10_0000_0000,length:0,kind:1}] {
            assert!(!init_map(&mut s,vec![large,
                Region{base:0x10_0000_0000,length:PAGE,kind:0},bad]));
            assert!(!s.initialized); assert_eq!(s.total,0);
        }
        assert!(init_map(&mut s,vec![large,
            Region{base:0x10_0000_0000,length:PAGE,kind:0}]));
        assert_eq!(s.total,MAX_FRAMES as u64);
    }
    #[test]
    fn subpage_conflicts_reject_and_failed_reinitialize_discards_old_state() {
        let mut s=State::empty();
        assert!(init_map(&mut s,vec![Region{base:PAGE,length:PAGE,kind:0}]));
        assert!(!init_map(&mut s,vec![
            Region{base:PAGE+1,length:PAGE*2-1,kind:0},
            Region{base:PAGE+2,length:1,kind:1}]));
        assert!(!s.initialized); assert_eq!(s.total,0);
        assert!(!init_map(&mut s,vec![Region{base:1,length:1,kind:0}]));
        assert!(init_map(&mut s,vec![
            Region{base:PAGE,length:PAGE,kind:0},
            Region{base:PAGE*2,length:PAGE,kind:1}]));
        assert_eq!(s.total,1); assert_eq!(s.physical_at(0),Some(PAGE));
    }
    #[test]
    fn contiguous_allocator_contract_rejects_invalid_requests(){
        let mut frame=0u64;
        assert!(!unsafe{pmm_allocate_contiguous_frames(0,&mut frame)});
        assert!(!unsafe{pmm_allocate_contiguous_frames(17,&mut frame)});
        assert!(!unsafe{pmm_allocate_contiguous_frames(2,ptr::null_mut())});
    }
    #[test]
    fn bounds_and_layout() {
        assert_eq!(core::mem::size_of::<Region>(), 24);
        assert_eq!(core::mem::size_of::<Span>(), 24);
        assert_eq!(MAX_FRAMES, 2_097_152);
        assert!(overlap(0x2000, 0x5000, 0x3000, 0x4000));
        assert!(!overlap(0x2000, 0x3000, 0x3000, 0x4000));
    }
}


// R084 / kernel workstation: a checked physical-run allocator and diagnostics.
// These methods inspect the current real PMM span bitmap, not synthetic
// reservations. All entrypoints use the existing BSP-only exclusivity rule.
impl State {
    /// Reverse-map an aligned physical frame; holes/reserved map to None.
    fn bit_for_physical(&self,physical:u64)->Option<u64>{
        if !self.initialized||physical%PAGE!=0{return None;}
        for span in self.spans[..self.count].iter(){
            if physical<span.base{continue;}
            let offset=(physical-span.base)/PAGE;
            if offset<span.count{return Some(span.bit_offset+offset);}
        }
        None
    }
    fn is_managed_frame(&self,physical:u64)->bool{
        self.bit_for_physical(physical).is_some()
    }
    fn is_frame_busy(&self,physical:u64)->Option<bool>{
        Some(self.allocated(self.bit_for_physical(physical)?))
    }
    /// Count free frames in one span without touching allocation state.
    fn free_in_span(&self,index:usize)->Option<u64>{
        let span=*self.spans.get(index)?;
        if !self.initialized||index>=self.count{return None;}
        Some((0..span.count).filter(|i|!self.allocated(span.bit_offset+i)).count() as u64)
    }
    /// Longest possible contiguous physical run respecting span boundaries.
    fn longest_free_run(&self)->u64{
        if !self.initialized{return 0;}
        let mut longest=0;
        for span in &self.spans[..self.count]{
            let mut run=0;
            for i in 0..span.count{
                if self.allocated(span.bit_offset+i){run=0;}else{
                    run+=1;longest=longest.max(run);
                }
            }
        }
        longest
    }
    /// Number of nonempty free runs, useful for fragmentation diagnosis.
    fn count_free_runs(&self)->u64{
        if !self.initialized{return 0;}
        let mut count=0;
        for span in &self.spans[..self.count]{
            let mut previous_busy=true;
            for i in 0..span.count{
                let free=!self.allocated(span.bit_offset+i);
                if free&&previous_busy{count+=1;}
                previous_busy=!free;
            }
        }
        count
    }
    /// Find first allocated managed frame in physical span order.
    fn first_busy_physical(&self)->Option<u64>{
        if !self.initialized{return None;}
        for bit in 0..self.total{
            if self.allocated(bit){return self.physical_at(bit);}
        }
        None
    }
    /// Validate a run of exact contiguous frames in one physical RAM span.
    fn validate_run(&self,physical:u64,count:u32,expect_busy:bool)->Option<u64>{
        self.validate_run_bounded(physical,count,expect_busy,16)
    }
    fn validate_run_bounded(&self,physical:u64,count:u32,expect_busy:bool,limit:u32)->Option<u64>{
        if !self.single_frame_geometry_valid() || count==0||count>limit||limit>2500{return None;}
        let first=self.bit_for_physical(physical)?;
        let end_physical=physical.checked_add((count as u64-1)*PAGE)?;
        let last=self.bit_for_physical(end_physical)?;
        if last!=first+count as u64-1{return None;}
        for span in &self.spans[..self.count]{
            if first>=span.bit_offset&&first<span.bit_offset+span.count{
                if last>=span.bit_offset+span.count{return None;}
                break;
            }
        }
        if (0..count as u64).any(|i|self.allocated(first+i)!=expect_busy){
            return None;
        }
        Some(first)
    }
    /// Reserve a specific frame run only when ALL frames are free.
    fn claim_run(&mut self,physical:u64,count:u32)->bool{
        if !self.verify_frame_accounting()||
            count as u64>self.total-self.used{return false;}
        let Some(first)=self.validate_run(physical,count,false)else{return false;};
        for i in 0..count as u64{self.set_bit(first+i,true);}
        self.used+=count as u64;
        self.cursor=(first+count as u64)%self.total;
        true
    }
    /// Release a complete frame run atomically or none of its frames.
    fn release_run(&mut self,physical:u64,count:u32)->bool{
        self.release_run_bounded(physical,count,16)
    }
    fn release_run_bounded(&mut self,physical:u64,count:u32,limit:u32)->bool{
        if !self.verify_frame_accounting()||
            self.used<count as u64{return false;}
        let Some(first)=self.validate_run_bounded(physical,count,true,limit)else{return false;};
        for i in 0..count as u64{self.set_bit(first+i,false);}
        self.used-=count as u64;self.cursor=first;
        true
    }
}
/// Reserve an exact contiguous managed frame range, rather than searching
/// for any range. Unsafe because PMM is single-CPU and non-reentrant.
/// # Safety: BSP-only, IRQ-disabled kernel caller; no other allocator user.
#[no_mangle]
pub unsafe extern "C" fn pmm_claim_specific_frames(physical:u64,count:u32)->bool{
    let s=unsafe{state()};s.claim_run(physical,count)
}
/// Undo exactly one kernel-owned contiguous frame range atomically.
/// # Safety: Caller has stopped all DMA/CPU users of the frames; BSP-only.
#[no_mangle]
pub unsafe extern "C" fn pmm_release_contiguous_frames(physical:u64,count:u32)->bool{
    let s=unsafe{state()};s.release_run(physical,count)
}
/// Read-only fragmentation metrics of the live physical frame bitmap.
#[no_mangle]
pub extern "C" fn pmm_largest_contiguous_free_run()->u64{
    let s=unsafe{state()};s.longest_free_run()
}
#[no_mangle]
pub extern "C" fn pmm_number_of_free_runs()->u64{
    let s=unsafe{state()};s.count_free_runs()
}
#[cfg(test)]
mod r084_checked_physical_runs_tests{
    use super::*;
    unsafe extern "C" fn read(i:u64,out:*mut Region,ctx:*mut c_void)->bool{
        let map=unsafe{&*(ctx.cast::<Vec<Region>>())};
        if let Some(r)=map.get(i as usize){unsafe{out.write(*r)};true}else{false}
    }
    fn setup()->State{
        let mut s=State::empty();
        let mut regions=vec![
            Region{base:PAGE,length:PAGE*8,kind:0},
            Region{base:PAGE*20,length:PAGE*4,kind:0}
        ];
        let good=unsafe{initialize(&mut s,2,Some(read),
            (&mut regions as *mut Vec<Region>).cast())};
        assert!(good);s
    }
    #[test]
    fn exact_claim_release_and_fragmentation_never_cross_holes(){
        let mut s=setup();assert_eq!(s.total,12);
        assert_eq!(s.longest_free_run(),8);
        assert_eq!(s.count_free_runs(),2);
        assert_eq!(s.bit_for_physical(PAGE),Some(0));
        assert_eq!(s.bit_for_physical(PAGE*9),None);
        assert!(s.is_managed_frame(PAGE*21));
        assert!(!s.is_managed_frame(PAGE*19));
        assert_eq!(s.free_in_span(0),Some(8));
        assert_eq!(s.free_in_span(1),Some(4));
        assert!(s.claim_run(PAGE*2,3));
        assert_eq!(s.is_frame_busy(PAGE*2),Some(true));
        assert_eq!(s.first_busy_physical(),Some(PAGE*2));
        assert_eq!(s.count_free_runs(),3);
        assert_eq!(s.longest_free_run(),4);
        assert!(!s.claim_run(PAGE*3,2));
        assert!(!s.release_run(PAGE,3));
        assert_eq!(s.used,3);
        assert!(s.release_run(PAGE*2,3));
        assert_eq!(s.used,0);
        assert_eq!(s.longest_free_run(),8);
    }
    #[test]
    fn unaligned_invalid_cross_span_and_duplicate_frees_are_atomic(){
        let mut s=setup();
        assert!(!s.claim_run(PAGE+1,1));
        assert!(!s.claim_run(PAGE*7,3));
        assert!(!s.claim_run(PAGE*8,16));
        assert!(!s.claim_run(PAGE*20,0));
        assert!(!s.release_run(PAGE*20,1));
        assert_eq!(s.used,0);
        assert!(s.claim_run(PAGE*21,2));
        assert!(!s.release_run(PAGE*20,3));
        assert_eq!(s.used,2);
        assert!(s.release_run(PAGE*21,2));
        assert!(!s.release_run(PAGE*21,2));
        assert_eq!(s.used,0);
    }
}


// R084 extension: bitmap-backed frame placement and fault-domain diagnostics.
// These operations use the SAME real PMM state as the boot allocator.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct FrameSpanUsage{
    pub base:u64,pub frames:u64,pub busy:u64,pub longest_free:u64,
}
impl State{
    /// 1. Physical frame → owning usable firmware span.
    fn frame_span_index(&self,physical:u64)->Option<usize>{
        if !self.single_frame_geometry_valid(){return None;}
        self.bit_for_physical(physical)?;
        self.spans[..self.count].iter().position(|s|
            physical>=s.base&&physical-s.base<s.count*PAGE)
    }
    /// 2. Count only active firmware spans.
    fn active_span_count(&self)->usize{if self.initialized{self.count}else{0}}
    /// 3. Report usable frames in a selected firmware span.
    fn span_frames(&self,index:usize)->Option<u64>{
        if !self.initialized{return None;}
        self.spans.get(index).filter(|_|index<self.count).map(|s|s.count)
    }
    /// 4. Count allocated frames for exactly one firmware span.
    fn busy_in_span(&self,index:usize)->Option<u64>{
        Some(self.span_frames(index)?-self.free_in_span(index)?)
    }
    /// 5. Find first free physical frame within a span.
    fn first_free_in_span(&self,index:usize)->Option<u64>{
        self.span_frames(index)?;
        let span=self.spans[index];
        (0..span.count).find(|i|!self.allocated(span.bit_offset+i))
            .and_then(|i|span.base.checked_add(i*PAGE))
    }
    /// 6. Find last free physical frame in that same usable region.
    fn last_free_in_span(&self,index:usize)->Option<u64>{
        self.span_frames(index)?;let s=self.spans[index];
        (0..s.count).rev().find(|i|!self.allocated(s.bit_offset+i))
            .and_then(|i|s.base.checked_add(i*PAGE))
    }
    /// 7. Find first allocated physical frame for this span.
    fn first_busy_in_span(&self,index:usize)->Option<u64>{
        self.span_frames(index)?;let s=self.spans[index];
        (0..s.count).find(|i|self.allocated(s.bit_offset+i))
            .and_then(|i|s.base.checked_add(i*PAGE))
    }
    /// 8. Find last allocated frame without exposing memory contents.
    fn last_busy_in_span(&self,index:usize)->Option<u64>{
        self.span_frames(index)?;let s=self.spans[index];
        (0..s.count).rev().find(|i|self.allocated(s.bit_offset+i))
            .and_then(|i|s.base.checked_add(i*PAGE))
    }
    /// 9. Fragmentation: longest free region in a specific RAM span.
    fn longest_free_in_span(&self,index:usize)->Option<u64>{
        self.span_frames(index)?;let s=self.spans[index];
        let mut best=0;let mut current=0;
        for i in 0..s.count{
            if self.allocated(s.bit_offset+i){current=0;}
            else{current+=1;best=best.max(current);}
        }
        Some(best)
    }
    /// 10. Fragmentation: count distinct free runs for one usable region.
    fn free_runs_in_span(&self,index:usize)->Option<u64>{
        self.span_frames(index)?;let s=self.spans[index];
        let mut total=0;let mut prev_busy=true;
        for i in 0..s.count{
            let free=!self.allocated(s.bit_offset+i);
            if free&&prev_busy{total+=1;}
            prev_busy=!free;
        }
        Some(total)
    }
    /// 11. Verify an exact current busy physical run.
    fn run_is_claimed(&self,start:u64,count:u32)->bool{
        self.validate_run(start,count,true).is_some()
    }
    /// 12. Verify an exact currently free physical run.
    fn run_is_available(&self,start:u64,count:u32)->bool{
        self.validate_run(start,count,false).is_some()
    }
    /// 13. Locate a physically aligned usable run, never spanning RAM holes.
    fn find_aligned_run(&self,count:u32,align_frames:u32)->Option<u64>{
        if count==0||count>16||align_frames==0||
          !align_frames.is_power_of_two()||align_frames>4096{return None;}
        if !self.verify_frame_accounting(){return None;}
        for s in &self.spans[..self.count]{
            if s.count<count as u64{continue;}
            for i in 0..=s.count-count as u64{
                let Some(p)=s.base.checked_add(i*PAGE)else{continue;};
                if (p/PAGE)%(align_frames as u64)!=0{continue;}
                if self.validate_run(p,count,false).is_some(){return Some(p);}
            }
        }
        None
    }
    /// 14. Atomically allocate the first correctly aligned run.
    fn claim_aligned_run(&mut self,count:u32,align_frames:u32)->Option<u64>{
        let physical=self.find_aligned_run(count,align_frames)?;
        if self.claim_run(physical,count){Some(physical)}else{None}
    }
    /// 15. Find the highest physical free run without crossing spans.
    fn highest_available_run(&self,count:u32)->Option<u64>{
        if count==0||count>16||!self.verify_frame_accounting(){return None;}
        let mut best=None;
        for s in &self.spans[..self.count]{
            if s.count<count as u64{continue;}
            for i in 0..=s.count-count as u64{
                let Some(p)=s.base.checked_add(i*PAGE)else{continue;};
                if best.is_none_or(|x|p>x)&&self.validate_run(p,count,false).is_some(){
                    best=Some(p);
                }
            }
        }
        best
    }
    /// 16. Reserve the highest available physical page run as one mutation.
    fn claim_highest_run(&mut self,count:u32)->Option<u64>{
        let physical=self.highest_available_run(count)?;
        if self.claim_run(physical,count){Some(physical)}else{None}
    }
    /// 17. Allocate a discontiguous exact frame set as one transaction.
    fn claim_exact_frame_set(&mut self,addresses:&[u64])->bool{
        if addresses.is_empty()||addresses.len()>16||
           !self.verify_frame_accounting()||
           addresses.len() as u64>self.total-self.used{
            return false;
        }
        let mut bits=[0u64;16];
        for (i,&physical) in addresses.iter().enumerate(){
            let Some(bit)=self.bit_for_physical(physical)else{return false;};
            if self.allocated(bit)||bits[..i].contains(&bit){return false;}
            bits[i]=bit;
        }
        for &bit in &bits[..addresses.len()]{self.set_bit(bit,true);}
        self.used+=addresses.len() as u64;
        self.cursor=(bits[0]+1)%self.total;true
    }
    /// 18. Release a discontiguous exact set without partial free.
    fn release_exact_frame_set(&mut self,addresses:&[u64])->bool{
        if addresses.is_empty()||addresses.len()>16||
           !self.verify_frame_accounting()||
           self.used<addresses.len() as u64 {
            return false;
        }
        let mut bits=[0u64;16];
        for (i,&physical) in addresses.iter().enumerate(){
            let Some(bit)=self.bit_for_physical(physical)else{return false;};
            if !self.allocated(bit)||bits[..i].contains(&bit){return false;}
            bits[i]=bit;
        }
        for &bit in &bits[..addresses.len()]{self.set_bit(bit,false);}
        self.used-=addresses.len() as u64;
        self.cursor=bits[0];true
    }
    /// 19. Validate allocator accounting against the actual bitmap.
    fn verify_frame_accounting(&self)->bool{
        self.single_frame_geometry_valid()&&
        (0..self.total).filter(|i|self.allocated(*i)).count() as u64==self.used
    }
    /// 20. Publish a complete per-span occupancy snapshot or nothing.
    fn snapshot_span_usage(&self,out:&mut[FrameSpanUsage])->Option<usize>{
        if !self.verify_frame_accounting()||out.len()<self.count{return None;}
        // Build the complete ownership snapshot before publishing even one
        // descriptor. An invalid late span cannot leak a partial view.
        let blank=FrameSpanUsage{base:0,frames:0,busy:0,longest_free:0};
        let mut staged=[blank;MAX_SPANS];
        for (i,s) in self.spans[..self.count].iter().enumerate(){
            staged[i]=FrameSpanUsage{
                base:s.base,frames:s.count,
                busy:self.busy_in_span(i)?,
                longest_free:self.longest_free_in_span(i)?,
            };
        }
        out[..self.count].copy_from_slice(&staged[..self.count]);
        Some(self.count)
    }
}
/// Trusted boot allocator API: physical alignment is counted in 4K pages.
/// # Safety: BSP, IRQ-disabled, exclusive output; not a userspace syscall.
#[no_mangle]
pub unsafe extern "C" fn pmm_allocate_aligned_frames(
    count:u32,align_frames:u32,out:*mut u64)->bool{
    if out.is_null(){return false;}
    let s=unsafe{state()};
    if let Some(physical)=s.claim_aligned_run(count,align_frames){
        unsafe{ptr::write(out,physical)};true
    }else{false}
}
/// # Safety: BSP only; no concurrent memory/DMA ownership transfers.
#[no_mangle]
pub unsafe extern "C" fn pmm_allocate_highest_frames(
    count:u32,out:*mut u64)->bool{
    if out.is_null(){return false;}
    let s=unsafe{state()};
    if let Some(physical)=s.claim_highest_run(count){
        unsafe{ptr::write(out,physical)};true
    }else{false}
}

#[cfg(test)]
mod r084_extended_pmm_tests{
    use super::*;
    unsafe extern "C" fn read(i:u64,out:*mut Region,ctx:*mut c_void)->bool{
        let regions=unsafe{&*(ctx.cast::<Vec<Region>>())};
        if let Some(r)=regions.get(i as usize){unsafe{out.write(*r)};true}else{false}
    }
    fn setup()->State{
        let mut s=State::empty();
        let mut map=vec![Region{base:PAGE,length:PAGE*8,kind:0},
                         Region{base:PAGE*20,length:PAGE*4,kind:0}];
        assert!(unsafe{initialize(&mut s,2,Some(read),
            (&mut map as *mut Vec<Region>).cast())});s
    }
    #[test]
    fn twenty_real_bitmap_services_validate_frame_ownership_and_fragmentation(){
        let mut s=setup();
        assert_eq!(s.frame_span_index(PAGE*20),Some(1));
        assert_eq!(s.active_span_count(),2);
        assert_eq!(s.span_frames(0),Some(8));
        assert_eq!(s.busy_in_span(0),Some(0));
        assert_eq!(s.first_free_in_span(1),Some(PAGE*20));
        assert_eq!(s.last_free_in_span(0),Some(PAGE*8));
        assert_eq!(s.first_busy_in_span(0),None);
        assert_eq!(s.last_busy_in_span(1),None);
        assert_eq!(s.longest_free_in_span(0),Some(8));
        assert_eq!(s.free_runs_in_span(1),Some(1));
        assert!(s.run_is_available(PAGE*20,2));
        assert!(!s.run_is_claimed(PAGE*20,2));
        assert_eq!(s.find_aligned_run(2,4),Some(PAGE*4));
        assert_eq!(s.claim_aligned_run(2,4),Some(PAGE*4));
        assert!(s.run_is_claimed(PAGE*4,2));
        assert_eq!(s.first_busy_in_span(0),Some(PAGE*4));
        assert_eq!(s.last_busy_in_span(0),Some(PAGE*5));
        assert_eq!(s.highest_available_run(2),Some(PAGE*22));
        assert_eq!(s.claim_highest_run(2),Some(PAGE*22));
        assert!(s.claim_exact_frame_set(&[PAGE,PAGE*20]));
        assert!(!s.claim_exact_frame_set(&[PAGE*2,PAGE*2]));
        assert!(!s.release_exact_frame_set(&[PAGE*2,PAGE*2]));
        assert!(s.verify_frame_accounting());
        let dummy=FrameSpanUsage{base:0,frames:0,busy:0,longest_free:0};
        let mut too_short=[dummy;1];
        assert_eq!(s.snapshot_span_usage(&mut too_short),None);
        assert_eq!(too_short,[dummy]);
        let mut usage=[dummy;2];
        assert_eq!(s.snapshot_span_usage(&mut usage),Some(2));
        assert_eq!(usage[0].busy,3);
        assert_eq!(usage[1].busy,3);
        assert!(s.release_exact_frame_set(&[PAGE,PAGE*20]));
        assert!(s.release_run(PAGE*4,2));
        assert!(s.release_run(PAGE*22,2));
        assert_eq!(s.used,0);
        assert!(s.verify_frame_accounting());
    }
    #[test]
    fn ram_holes_unaligned_and_duplicate_address_sets_are_all_or_nothing(){
        let mut s=setup();let start=s.total;
        assert_eq!(s.find_aligned_run(2,3),None);
        assert_eq!(s.claim_aligned_run(0,4),None);
        assert_eq!(s.highest_available_run(20),None);
        assert!(!s.claim_exact_frame_set(&[PAGE+1,PAGE*2]));
        assert!(!s.release_exact_frame_set(&[PAGE,PAGE*20]));
        assert_eq!(s.used,0);
        assert_eq!(s.total,start);
    }
}


// R084 extension III: 20 RAM-range allocation and memory-map inspection
// operations on the *actual* physical-frame bitmap. No synthetic allocator.
impl State {
    /// 1. Count the free managed pages in a physical half-open range.
    fn free_between(&self,start:u64,end:u64)->Option<u64>{
        if !self.initialized||start>=end||start%PAGE!=0||end%PAGE!=0{return None;}
        let mut n=0;
        for bit in 0..self.total{
            let p=self.physical_at(bit)?;
            if p>=start&&p<end&&!self.allocated(bit){n+=1;}
        }
        Some(n)
    }
    /// 2. Count busy managed pages, including separated usable regions.
    fn busy_between(&self,start:u64,end:u64)->Option<u64>{
        if !self.initialized||start>=end||start%PAGE!=0||end%PAGE!=0{return None;}
        let mut n=0;
        for bit in 0..self.total{
            let p=self.physical_at(bit)?;
            if p>=start&&p<end&&self.allocated(bit){n+=1;}
        }
        Some(n)
    }
    /// 3. Find first free frame at or after a physical address.
    fn next_free_at_or_after(&self,start:u64)->Option<u64>{
        if !self.initialized||start%PAGE!=0{return None;}
        for bit in 0..self.total{
            let p=self.physical_at(bit)?;
            if p>=start&&!self.allocated(bit){return Some(p);}
        }
        None
    }
    /// 4. Find last free frame strictly below a physical address.
    fn last_free_before(&self,exclusive_end:u64)->Option<u64>{
        if !self.initialized||exclusive_end%PAGE!=0{return None;}
        let mut best=None;
        for bit in 0..self.total{
            let p=self.physical_at(bit)?;
            if p<exclusive_end&&!self.allocated(bit)&&best.is_none_or(|v|p>v){
                best=Some(p);
            }
        }
        best
    }
    /// 5. First busy physical frame >= a validated page boundary.
    fn next_busy_at_or_after(&self,start:u64)->Option<u64>{
        if !self.initialized||start%PAGE!=0{return None;}
        for bit in 0..self.total{
            let p=self.physical_at(bit)?;
            if p>=start&&self.allocated(bit){return Some(p);}
        }
        None
    }
    /// 6. Last occupied frame below a validated bound.
    fn last_busy_before(&self,end:u64)->Option<u64>{
        if !self.initialized||end%PAGE!=0{return None;}
        let mut best=None;
        for bit in 0..self.total{
            let p=self.physical_at(bit)?;
            if p<end&&self.allocated(bit)&&best.is_none_or(|v|p>v){best=Some(p);}
        }
        best
    }
    /// 7. One-span contiguous physical-run query, with exclusive max.
    fn find_run_between(&self,from:u64,until:u64,count:u32)->Option<u64>{
        if !self.initialized||count==0||count>16||
           from>=until||from%PAGE!=0||until%PAGE!=0{return None;}
        for span in &self.spans[..self.count]{
            if span.count<count as u64{continue;}
            for offset in 0..=span.count-count as u64{
                let p=span.base.checked_add(offset*PAGE)?;
                let end=p.checked_add(count as u64*PAGE)?;
                if p>=from&&end<=until&&self.validate_run(p,count,false).is_some(){
                    return Some(p);
                }
            }
        }
        None
    }
    /// 8. Claim one complete run within physical placement bounds.
    fn claim_run_between(&mut self,from:u64,until:u64,count:u32)->Option<u64>{
        let address=self.find_run_between(from,until,count)?;
        if self.claim_run(address,count){Some(address)}else{None}
    }
    /// 9. Search for a contiguous run below one physical ceiling.
    fn find_run_below(&self,exclusive_end:u64,count:u32)->Option<u64>{
        self.find_run_between(0,exclusive_end,count)
    }
    /// 10. Restrict the actual allocation to low physical RAM.
    fn claim_run_below(&mut self,exclusive_end:u64,count:u32)->Option<u64>{
        let address=self.find_run_below(exclusive_end,count)?;
        if self.claim_run(address,count){Some(address)}else{None}
    }
    /// 11. Search for an available run at or above a physical floor.
    fn find_run_above(&self,minimum:u64,count:u32)->Option<u64>{
        if !self.initialized||minimum%PAGE!=0||count==0||count>16{return None;}
        for span in &self.spans[..self.count]{
            if span.count<count as u64{continue;}
            for offset in 0..=span.count-count as u64{
                let p=span.base.checked_add(offset*PAGE)?;
                if p>=minimum&&self.validate_run(p,count,false).is_some(){
                    return Some(p);
                }
            }
        }
        None
    }
    /// 12. Allocate an exact-sized run in high RAM without crossing gaps.
    fn claim_run_above(&mut self,minimum:u64,count:u32)->Option<u64>{
        let address=self.find_run_above(minimum,count)?;
        if self.claim_run(address,count){Some(address)}else{None}
    }
    /// 13. Find a viable run whose first page is closest to a target address.
    fn nearest_free_run(&self,target:u64,count:u32)->Option<u64>{
        if !self.initialized||count==0||count>16{return None;}
        let mut best=None;
        for span in &self.spans[..self.count]{
            if span.count<count as u64{continue;}
            for offset in 0..=span.count-count as u64{
                let p=span.base.checked_add(offset*PAGE)?;
                if self.validate_run(p,count,false).is_none(){continue;}
                let distance=p.abs_diff(target);
                if best.is_none_or(|(old,_):(u64,u64)|distance<old){
                    best=Some((distance,p));
                }
            }
        }
        best.map(|(_,p)|p)
    }
    /// 14. Claim a frame run nearest requested device DMA window.
    fn claim_nearest_run(&mut self,target:u64,count:u32)->Option<u64>{
        let p=self.nearest_free_run(target,count)?;
        if self.claim_run(p,count){Some(p)}else{None}
    }
    /// 15. Allocate last N pages only if that exact span tail is free.
    fn find_span_tail_run(&self,index:usize,count:u32)->Option<u64>{
        let n=self.span_frames(index)?;
        if count==0||count>16||count as u64>n{return None;}
        let s=self.spans[index];
        let p=s.base.checked_add((n-count as u64)*PAGE)?;
        self.validate_run(p,count,false).map(|_|p)
    }
    /// 16. Claim the last N pages of a selected usable firmware span.
    fn claim_span_tail_run(&mut self,index:usize,count:u32)->Option<u64>{
        let p=self.find_span_tail_run(index,count)?;
        if self.claim_run(p,count){Some(p)}else{None}
    }
    /// 17. Capacity check for a real physically contiguous allocation.
    fn contiguous_available(&self,count:u32)->bool{
        count>0&&count<=16&&self.longest_free_run()>=count as u64
    }
    /// 18. Number of spans which still contain at least one free frame.
    fn free_span_count(&self)->usize{
        if !self.initialized{return 0;}
        (0..self.count).filter(|i|self.free_in_span(*i).unwrap_or(0)>0).count()
    }
    /// 19. Number of spans with any busy frames.
    fn busy_span_count(&self)->usize{
        if !self.initialized{return 0;}
        (0..self.count).filter(|i|self.busy_in_span(*i).unwrap_or(0)>0).count()
    }
    /// 20. Read-only FNV checksum of actually allocated bitmap bytes.
    /// This is a debug integrity hint, NOT a cryptographic signature.
    fn bitmap_ownership_digest(&self)->Option<u64>{
        if !self.initialized{return None;}
        let bytes=self.total.div_ceil(8) as usize;
        Some(self.bitmap[..bytes].iter().fold(0xcbf29ce484222325u64,
            |hash,b|(hash^(*b as u64)).wrapping_mul(0x100000001b3)))
    }
}

#[cfg(test)]
mod r084_physical_placement_batch_tests{
    use super::*;
    unsafe extern "C" fn rd(i:u64,out:*mut Region,ctx:*mut c_void)->bool{
        let map=unsafe{&*(ctx.cast::<Vec<Region>>())};
        if let Some(x)=map.get(i as usize){unsafe{out.write(*x)};true}else{false}
    }
    fn state()->State{
        let mut state=State::empty();
        let mut map=vec![
            Region{base:PAGE,length:PAGE*8,kind:0},
            Region{base:PAGE*20,length:PAGE*4,kind:0}
        ];
        assert!(unsafe{initialize(&mut state,2,Some(rd),
          (&mut map as *mut Vec<Region>).cast())});state
    }
    #[test]
    fn physical_placement_and_ownership_are_real_bitmap_transactions(){
        let mut s=state();let initial=s.bitmap_ownership_digest().unwrap();
        assert_eq!(s.free_between(PAGE,PAGE*24),Some(12));
        assert_eq!(s.busy_between(PAGE,PAGE*24),Some(0));
        assert_eq!(s.next_free_at_or_after(PAGE*9),Some(PAGE*20));
        assert_eq!(s.last_free_before(PAGE*20),Some(PAGE*8));
        assert_eq!(s.next_busy_at_or_after(PAGE),None);
        assert_eq!(s.last_busy_before(PAGE*24),None);
        assert_eq!(s.find_run_between(PAGE*19,PAGE*24,2),Some(PAGE*20));
        assert_eq!(s.claim_run_between(PAGE*19,PAGE*24,2),Some(PAGE*20));
        assert_eq!(s.find_run_below(PAGE*9,2),Some(PAGE));
        assert_eq!(s.claim_run_below(PAGE*9,2),Some(PAGE));
        assert_eq!(s.find_run_above(PAGE*20,2),Some(PAGE*22));
        assert_eq!(s.claim_run_above(PAGE*20,2),Some(PAGE*22));
        assert!(s.nearest_free_run(PAGE*4,2).is_some());
        assert_eq!(s.claim_nearest_run(PAGE*4,2),Some(PAGE*4));
        assert_eq!(s.find_span_tail_run(0,2),Some(PAGE*7));
        assert_eq!(s.claim_span_tail_run(0,2),Some(PAGE*7));
        assert_eq!(s.free_between(PAGE,PAGE*24),Some(2));
        assert_eq!(s.busy_between(PAGE,PAGE*24),Some(10));
        assert_eq!(s.next_busy_at_or_after(PAGE*20),Some(PAGE*20));
        assert_eq!(s.last_busy_before(PAGE*24),Some(PAGE*23));
        assert!(!s.contiguous_available(2));
        assert_eq!(s.free_span_count(),1);
        assert_eq!(s.busy_span_count(),2);
        assert_ne!(s.bitmap_ownership_digest(),Some(initial));
        assert!(s.verify_frame_accounting());
    }
    #[test]
    fn invalid_boundaries_and_interspan_run_never_claim_half_a_reservation(){
        let mut s=state();
        assert_eq!(s.claim_run_between(PAGE*8,PAGE*22,3),None);
        assert_eq!(s.find_run_between(PAGE+1,PAGE*24,2),None);
        assert_eq!(s.claim_run_below(PAGE,2),None);
        assert_eq!(s.find_run_above(PAGE*25,1),None);
        assert_eq!(s.claim_span_tail_run(9,2),None);
        assert_eq!(s.free_between(1,PAGE*24),None);
        assert_eq!(s.used,0);
    }
}


// M1/R084 extension IV: 20 real span-scoped physical placement and
// fragmentation services. The bitmap is the only source of ownership truth.
// Call exclusively from BSP with IRQs disabled, as with every PMM operation.
impl State{
    /// 1. First managed physical frame in one firmware span.
    fn span_first_physical(&self,index:usize)->Option<u64>{
        self.span_frames(index).map(|_|self.spans[index].base)
    }
    /// 2. Exclusive physical end, checked against firmware overflow.
    fn span_end_physical(&self,index:usize)->Option<u64>{
        let count=self.span_frames(index)?;
        self.spans[index].base.checked_add(count.checked_mul(PAGE)?)
    }
    /// 3. Report the number of unavailable frames as a percentage.
    fn span_busy_percent(&self,index:usize)->Option<u8>{
        let count=self.span_frames(index)?;
        if count==0{return None;}
        Some(((self.busy_in_span(index)?*100)/count) as u8)
    }
    /// 4. Report remaining usable frames as a percentage.
    fn span_free_percent(&self,index:usize)->Option<u8>{
        let count=self.span_frames(index)?;
        if count==0{return None;}
        Some(((self.free_in_span(index)?*100)/count) as u8)
    }
    /// 5. Number of separate allocated physical runs in a span.
    fn span_busy_runs(&self,index:usize)->Option<u64>{
        self.span_frames(index)?;
        let span=self.spans[index];let mut n=0;let mut previous_free=true;
        for bit in span.bit_offset..span.bit_offset+span.count{
            let busy=self.allocated(bit);
            if busy&&previous_free{n+=1;}
            previous_free=!busy;
        }
        Some(n)
    }
    /// 6. Return the first free run of an exact size in the selected span.
    fn span_first_free_run(&self,index:usize,length:u32)->Option<u64>{
        let count=self.span_frames(index)?;
        if length==0||length>16||length as u64>count{return None;}
        let span=self.spans[index];
        for i in 0..=count-length as u64{
            let p=span.base.checked_add(i*PAGE)?;
            if self.validate_run(p,length,false).is_some(){return Some(p);}
        }
        None
    }
    /// 7. Return the final free run of the requested exact size.
    fn span_last_free_run(&self,index:usize,length:u32)->Option<u64>{
        let count=self.span_frames(index)?;
        if length==0||length>16||length as u64>count{return None;}
        let span=self.spans[index];
        for i in (0..=count-length as u64).rev(){
            let p=span.base.checked_add(i*PAGE)?;
            if self.validate_run(p,length,false).is_some(){return Some(p);}
        }
        None
    }
    /// 8. Allocate the first contiguous available run of this span.
    fn span_claim_first_run(&mut self,index:usize,length:u32)->Option<u64>{
        let p=self.span_first_free_run(index,length)?;
        if self.claim_run(p,length){Some(p)}else{None}
    }
    /// 9. Allocate the last contiguous available run of this span.
    fn span_claim_last_run(&mut self,index:usize,length:u32)->Option<u64>{
        let p=self.span_last_free_run(index,length)?;
        if self.claim_run(p,length){Some(p)}else{None}
    }
    /// 10. Aligned run search inside a single firmware span.
    fn span_find_aligned_run(&self,index:usize,length:u32,align:u32)->Option<u64>{
        let count=self.span_frames(index)?;
        if length==0||length>16||align==0||!align.is_power_of_two()||
            length as u64>count{return None;}
        let span=self.spans[index];
        for i in 0..=count-length as u64{
            let p=span.base.checked_add(i*PAGE)?;
            if p/PAGE%align as u64==0&&self.validate_run(p,length,false).is_some(){
                return Some(p);
            }
        }
        None
    }
    /// 11. Aligned run claim verifies all bitmap bits before changing any.
    fn span_claim_aligned_run(&mut self,index:usize,length:u32,align:u32)->Option<u64>{
        let p=self.span_find_aligned_run(index,length,align)?;
        if self.claim_run(p,length){Some(p)}else{None}
    }
    /// 12. True only if no frame is allocated within this usable region.
    fn span_is_empty(&self,index:usize)->Option<bool>{
        Some(self.busy_in_span(index)?==0)
    }
    /// 13. True only if every frame is currently allocated.
    fn span_is_full(&self,index:usize)->Option<bool>{
        Some(self.free_in_span(index)?==0)
    }
    /// 14. Number of free pages in runs of at least a chosen length.
    fn span_free_frames_in_runs(&self,index:usize,minimum:u32)->Option<u64>{
        self.span_frames(index)?;
        if minimum==0{return None;}
        let span=self.spans[index];let mut run=0;let mut total=0;
        for i in 0..span.count{
            if !self.allocated(span.bit_offset+i){run+=1;}
            else{if run>=minimum as u64{total+=run;}run=0;}
        }
        if run>=minimum as u64{total+=run;}
        Some(total)
    }
    /// 15. Compute unusable free fragments smaller than the allocation.
    fn span_fragmented_frames(&self,index:usize,minimum:u32)->Option<u64>{
        if minimum==0{return None;}
        Some(self.free_in_span(index)?-self.span_free_frames_in_runs(index,minimum)?)
    }
    /// 16. Count free managed pages strictly below a physical DMA ceiling.
    fn free_frames_below(&self,ceiling:u64)->Option<u64>{
        if ceiling%PAGE!=0{return None;}
        self.free_between(PAGE,ceiling)
    }
    /// 17. Count busy managed pages strictly below the same ceiling.
    fn busy_frames_below(&self,ceiling:u64)->Option<u64>{
        if ceiling%PAGE!=0{return None;}
        self.busy_between(PAGE,ceiling)
    }
    /// 18. Count free managed pages at or above a physical floor.
    fn free_frames_at_or_above(&self,minimum:u64)->Option<u64>{
        if minimum%PAGE!=0||!self.initialized{return None;}
        let mut count=0;
        for bit in 0..self.total{
            if self.physical_at(bit)? >=minimum&&!self.allocated(bit){count+=1;}
        }
        Some(count)
    }
    /// 19. Allocate exactly one frame only when its physical address is free.
    fn claim_exact_page(&mut self,physical:u64)->bool{
        self.claim_run(physical,1)
    }
    /// 20. Release exactly one allocated frame with stale-free protection.
    fn release_exact_page(&mut self,physical:u64)->bool{
        self.release_run(physical,1)
    }
}

#[cfg(test)]
mod r084_span_placement_tests{
    use super::*;
    unsafe extern "C" fn rd(i:u64,out:*mut Region,ctx:*mut c_void)->bool{
        let map=unsafe{&*(ctx.cast::<Vec<Region>>())};
        if let Some(x)=map.get(i as usize){unsafe{out.write(*x)};true}else{false}
    }
    fn fresh()->State{
        let mut state=State::empty();
        let mut map=vec![Region{base:PAGE,length:PAGE*8,kind:0},
            Region{base:PAGE*20,length:PAGE*4,kind:0}];
        assert!(unsafe{initialize(&mut state,2,Some(rd),
            (&mut map as *mut Vec<Region>).cast())});
        state
    }
    #[test]
    fn r084_span_placement_preserves_exact_firmware_regions_and_bitmap(){
        let mut p=fresh();
        assert_eq!(p.span_first_physical(0),Some(PAGE));
        assert_eq!(p.span_end_physical(0),Some(PAGE*9));
        assert_eq!(p.span_busy_percent(0),Some(0));
        assert_eq!(p.span_free_percent(0),Some(100));
        assert_eq!(p.span_busy_runs(0),Some(0));
        assert_eq!(p.span_first_free_run(0,2),Some(PAGE));
        assert_eq!(p.span_last_free_run(0,2),Some(PAGE*7));
        assert_eq!(p.span_find_aligned_run(0,2,4),Some(PAGE*4));
        assert_eq!(p.span_claim_aligned_run(0,2,4),Some(PAGE*4));
        assert_eq!(p.span_claim_first_run(0,2),Some(PAGE));
        assert_eq!(p.span_claim_last_run(1,2),Some(PAGE*22));
        assert_eq!(p.span_is_empty(0),Some(false));
        assert_eq!(p.span_is_full(0),Some(false));
        assert_eq!(p.span_busy_runs(0),Some(2));
        assert_eq!(p.span_fragmented_frames(0,4),Some(4));
        assert_eq!(p.span_free_frames_in_runs(0,4),Some(0));
        assert_eq!(p.free_frames_below(PAGE*9),Some(4));
        assert_eq!(p.busy_frames_below(PAGE*9),Some(4));
        assert_eq!(p.free_frames_at_or_above(PAGE*20),Some(2));
        assert!(p.claim_exact_page(PAGE*3));
        assert!(!p.claim_exact_page(PAGE*3));
        assert!(p.release_exact_page(PAGE*3));
        assert!(!p.release_exact_page(PAGE*3));
        assert!(p.verify_frame_accounting());
    }
    #[test]
    fn invalid_span_indices_and_physical_gap_cannot_commit_pages(){
        let mut p=fresh();
        assert_eq!(p.span_first_free_run(9,2),None);
        assert_eq!(p.span_find_aligned_run(0,2,3),None);
        assert_eq!(p.span_claim_first_run(1,5),None);
        assert_eq!(p.span_fragmented_frames(0,0),None);
        assert!(!p.claim_exact_page(PAGE*19));
        assert_eq!(p.used,0);
    }
}


// R084 extension V: physical-memory capacity, placement, and atomic multi-run
// ownership against the real single-BSP boot PMM bitmap. No SMP promises.
impl State {
    /// Number of firmware-described usable physical page spans.
    fn usable_region_count(&self)->usize {
        if self.initialized { self.count } else { 0 }
    }
    /// Count all managed frames in a bounded physical address region.
    fn managed_between(&self,begin:u64,end:u64)->Option<u64>{
        if !self.initialized||begin%PAGE!=0||end%PAGE!=0||begin>=end{return None;}
        let mut total=0;
        for span in &self.spans[..self.count] {
            let s=span.base.max(begin);
            let e=span.base.checked_add(span.count.checked_mul(PAGE)?)?.min(end);
            if e>s {total+=(e-s)/PAGE;}
        }
        Some(total)
    }
    /// Lookup the real owning firmware span for a bitmap bit.
    fn span_of_bit(&self,bit:u64)->Option<usize>{
        if !self.initialized||bit>=self.total {return None;}
        self.spans[..self.count].iter().position(|s|
            bit>=s.bit_offset&&bit<s.bit_offset+s.count)
    }
    /// The region where one page was described in the boot map.
    fn managed_span_bounds(&self,physical:u64)->Option<(u64,u64)>{
        let i=self.frame_span_index(physical)?;
        Some((self.spans[i].base,self.span_end_physical(i)?))
    }
    /// Whether an exact aligned physical address is a managed unclaimed frame.
    fn is_free_physical(&self,physical:u64)->bool{
        self.is_frame_busy(physical)==Some(false)
    }
    /// Whether an exact aligned address is an allocated managed frame.
    fn is_busy_physical(&self,physical:u64)->bool{
        self.is_frame_busy(physical)==Some(true)
    }
    /// Find first busy managed page in a bounded physical address range.
    fn first_busy_between(&self,begin:u64,end:u64)->Option<u64>{
        if !self.initialized||begin%PAGE!=0||end%PAGE!=0||begin>=end{return None;}
        for bit in 0..self.total {
            let p=self.physical_at(bit)?;
            if p>=begin&&p<end&&self.allocated(bit){return Some(p);}
        }
        None
    }
    /// Find last allocated managed page in an address range.
    fn last_busy_between(&self,begin:u64,end:u64)->Option<u64>{
        if !self.initialized||begin%PAGE!=0||end%PAGE!=0||begin>=end{return None;}
        let mut found=None;
        for bit in 0..self.total {
            let p=self.physical_at(bit)?;
            if p>=begin&&p<end&&self.allocated(bit)&&found.is_none_or(|x|p>x) {
                found=Some(p);
            }
        }
        found
    }
    /// Find last unclaimed managed frame in physical interval.
    fn last_free_between(&self,begin:u64,end:u64)->Option<u64>{
        if !self.initialized||begin%PAGE!=0||end%PAGE!=0||begin>=end{return None;}
        let mut found=None;
        for bit in 0..self.total {
            let p=self.physical_at(bit)?;
            if p>=begin&&p<end&&!self.allocated(bit)&&found.is_none_or(|x|p>x){
                found=Some(p);
            }
        }
        found
    }
    /// Current physical frame occupancy percentage, rounded down.
    fn total_busy_percent(&self)->Option<u8>{
        if !self.initialized||self.total==0{return None;}
        Some((self.used*100/self.total) as u8)
    }
    /// Current physical free percent, rounded down.
    fn total_free_percent(&self)->Option<u8>{
        if !self.initialized||self.total==0{return None;}
        Some(((self.total-self.used)*100/self.total) as u8)
    }
    /// Count live allocated frames whose physical addresses are <= 4GiB.
    fn occupied_low4g(&self)->Option<u64>{
        self.busy_between(PAGE,4*1024*1024*1024)
    }
    /// Count available frames whose physical addresses are <= 4GiB.
    fn available_low4g(&self)->Option<u64>{
        self.free_between(PAGE,4*1024*1024*1024)
    }
    /// Number of spans fully occupied by this allocator.
    fn full_span_count(&self)->usize{
        if !self.initialized{return 0;}
        (0..self.count).filter(|&i|self.span_is_full(i)==Some(true)).count()
    }
    /// Number of spans with neither allocated nor reserved active pages.
    fn empty_span_count(&self)->usize{
        if !self.initialized{return 0;}
        (0..self.count).filter(|&i|self.span_is_empty(i)==Some(true)).count()
    }
    /// Validate all exact owner allocations before releasing a batch.
    fn can_release_frames(&self,addresses:&[u64])->bool{
        if !self.verify_frame_accounting()||
           addresses.is_empty()||addresses.len()>16{return false;}
        for (i,&addr) in addresses.iter().enumerate(){
            if !self.is_busy_physical(addr)||addresses[..i].contains(&addr){return false;}
        }
        true
    }
    /// Validate the whole atomic claim set; never claim managed RAM holes.
    fn can_claim_frames(&self,addresses:&[u64])->bool{
        if !self.verify_frame_accounting()||
           addresses.is_empty()||addresses.len()>16{return false;}
        for (i,&addr) in addresses.iter().enumerate(){
            if !self.is_free_physical(addr)||addresses[..i].contains(&addr){return false;}
        }
        true
    }
    /// Find a first available physical frame set from verified candidates.
    fn first_free_candidate(&self,candidates:&[u64])->Option<u64>{
        candidates.iter().copied().find(|&addr|self.is_free_physical(addr))
    }
    /// Allocate an exact frame from candidates with no partial side effects.
    fn claim_first_candidate(&mut self,candidates:&[u64])->Option<u64>{
        let addr=self.first_free_candidate(candidates)?;
        if self.claim_exact_page(addr){Some(addr)}else{None}
    }
    /// Release a verified set of up to 16 distinct physical frames at once.
    fn release_selected(&mut self,addresses:&[u64])->bool{
        if !self.can_release_frames(addresses){return false;}
        self.release_exact_frame_set(addresses)
    }
}
#[cfg(test)]
mod r084_v5_pmm_tests{
    use super::*;
    unsafe extern "C" fn read(i:u64,out:*mut Region,ctx:*mut c_void)->bool{
        let map=unsafe{&*(ctx.cast::<Vec<Region>>())};
        if let Some(r)=map.get(i as usize){unsafe{out.write(*r)};true}else{false}
    }
    fn fresh()->State{
        let mut s=State::empty();
        let mut map=vec![Region{base:PAGE,length:8*PAGE,kind:0},
            Region{base:20*PAGE,length:4*PAGE,kind:0}];
        assert!(unsafe{initialize(&mut s,2,Some(read),
            (&mut map as *mut Vec<Region>).cast())});s
    }
    #[test]
    fn physical_frames_and_atomic_candidates_are_bound_to_live_bitmap(){
        let mut s=fresh();
        assert_eq!(s.usable_region_count(),2);
        assert_eq!(s.managed_between(PAGE,24*PAGE),Some(12));
        assert_eq!(s.span_of_bit(9),Some(1));
        assert_eq!(s.managed_span_bounds(21*PAGE),Some((20*PAGE,24*PAGE)));
        assert!(s.is_free_physical(PAGE));
        assert!(!s.is_busy_physical(PAGE));
        assert_eq!(s.first_busy_between(PAGE,24*PAGE),None);
        assert_eq!(s.last_busy_between(PAGE,24*PAGE),None);
        assert_eq!(s.last_free_between(PAGE,24*PAGE),Some(23*PAGE));
        assert_eq!(s.total_busy_percent(),Some(0));
        assert_eq!(s.total_free_percent(),Some(100));
        assert_eq!(s.occupied_low4g(),Some(0));
        assert_eq!(s.available_low4g(),Some(12));
        assert_eq!(s.full_span_count(),0);
        assert_eq!(s.empty_span_count(),2);
        assert!(s.can_claim_frames(&[PAGE,21*PAGE]));
        assert!(!s.can_release_frames(&[PAGE]));
        assert_eq!(s.first_free_candidate(&[19*PAGE,22*PAGE]),Some(22*PAGE));
        assert_eq!(s.claim_first_candidate(&[19*PAGE,22*PAGE]),Some(22*PAGE));
        assert!(s.can_release_frames(&[22*PAGE]));
        assert!(!s.can_claim_frames(&[22*PAGE]));
        assert_eq!(s.first_busy_between(PAGE,24*PAGE),Some(22*PAGE));
        assert_eq!(s.last_busy_between(PAGE,24*PAGE),Some(22*PAGE));
        assert!(s.release_selected(&[22*PAGE]));
        assert!(!s.release_selected(&[22*PAGE]));
        assert!(s.verify_frame_accounting());
    }
}


// R084 / batch VI: 20 bitmap-real span layout and bounded reservation
// operations. All mutations use the same BSP-only single-CPU PMM bitmap.
impl State {
    // 1. Find the earliest free page in a span at/above a requested address.
    fn span_next_free_from(&self,index:usize,floor:u64)->Option<u64>{
        self.span_frames(index)?;
        let s=self.spans[index];
        for i in 0..s.count {
            let p=s.base.checked_add(i.checked_mul(PAGE)?)?;
            if p>=floor&&!self.allocated(s.bit_offset+i){return Some(p);}
        }
        None
    }
    // 2. Find the last free page in a span below a ceiling.
    fn span_last_free_before(&self,index:usize,ceiling:u64)->Option<u64>{
        self.span_frames(index)?;
        let s=self.spans[index];
        for i in (0..s.count).rev(){
            let p=s.base.checked_add(i.checked_mul(PAGE)?)?;
            if p<ceiling&&!self.allocated(s.bit_offset+i){return Some(p);}
        }
        None
    }
    // 3. First busy physical page at or beyond an aligned floor in one span.
    fn span_next_busy_from(&self,index:usize,floor:u64)->Option<u64>{
        self.span_frames(index)?;let s=self.spans[index];
        for i in 0..s.count{
            let p=s.base.checked_add(i.checked_mul(PAGE)?)?;
            if p>=floor&&self.allocated(s.bit_offset+i){return Some(p);}
        }
        None
    }
    // 4. Last occupied physical page strictly below a ceiling in one span.
    fn span_last_busy_before(&self,index:usize,ceiling:u64)->Option<u64>{
        self.span_frames(index)?;let s=self.spans[index];
        for i in (0..s.count).rev(){
            let p=s.base.checked_add(i.checked_mul(PAGE)?)?;
            if p<ceiling&&self.allocated(s.bit_offset+i){return Some(p);}
        }
        None
    }
    // 5. Count unclaimed physical pages in a span's selected address window.
    fn span_free_in_window(&self,index:usize,from:u64,until:u64)->Option<u64>{
        self.span_frames(index)?;
        if from>=until||from%PAGE!=0||until%PAGE!=0{return None;}
        let s=self.spans[index];let mut count=0;
        for i in 0..s.count{
            let p=s.base.checked_add(i.checked_mul(PAGE)?)?;
            if p>=from&&p<until&&!self.allocated(s.bit_offset+i){count+=1;}
        }
        Some(count)
    }
    // 6. Count allocated frames in an aligned span address window.
    fn span_busy_in_window(&self,index:usize,from:u64,until:u64)->Option<u64>{
        self.span_frames(index)?;
        if from>=until||from%PAGE!=0||until%PAGE!=0{return None;}
        let s=self.spans[index];let mut count=0;
        for i in 0..s.count{
            let p=s.base.checked_add(i.checked_mul(PAGE)?)?;
            if p>=from&&p<until&&self.allocated(s.bit_offset+i){count+=1;}
        }
        Some(count)
    }
    // 7. Index of a physical address's frame relative to its firmware span.
    fn index_in_span(&self,physical:u64)->Option<(usize,u64)>{
        let index=self.frame_span_index(physical)?;
        Some((index,(physical-self.spans[index].base)/PAGE))
    }
    // 8. Physical frame at an exact span-relative frame index.
    fn physical_at_span_offset(&self,index:usize,offset:u64)->Option<u64>{
        let count=self.span_frames(index)?;
        if offset>=count{return None;}
        self.spans[index].base.checked_add(offset.checked_mul(PAGE)?)
    }
    // 9. Search an exact-sized free run starting no earlier than floor.
    fn span_find_run_from(&self,index:usize,floor:u64,count:u32)->Option<u64>{
        let frames=self.span_frames(index)?;
        if count==0||count>16||count as u64>frames||floor%PAGE!=0{return None;}
        for i in 0..=frames-count as u64{
            let p=self.physical_at_span_offset(index,i)?;
            if p>=floor&&self.validate_run(p,count,false).is_some(){return Some(p);}
        }
        None
    }
    // 10. Search a run ending before ceiling, descending physical order.
    fn span_find_run_below(&self,index:usize,ceiling:u64,count:u32)->Option<u64>{
        let frames=self.span_frames(index)?;
        if count==0||count>16||count as u64>frames||ceiling%PAGE!=0{return None;}
        for i in (0..=frames-count as u64).rev(){
            let p=self.physical_at_span_offset(index,i)?;
            let end=p.checked_add(count as u64*PAGE)?;
            if end<=ceiling&&self.validate_run(p,count,false).is_some(){return Some(p);}
        }
        None
    }
    // 11. Atomically claim the first free run beyond floor in this span.
    fn span_claim_run_from(&mut self,index:usize,floor:u64,count:u32)->Option<u64>{
        let p=self.span_find_run_from(index,floor,count)?;
        if self.claim_run(p,count){Some(p)}else{None}
    }
    // 12. Atomically claim the highest eligible run below a ceiling.
    fn span_claim_run_below(&mut self,index:usize,ceiling:u64,count:u32)->Option<u64>{
        let p=self.span_find_run_below(index,ceiling,count)?;
        if self.claim_run(p,count){Some(p)}else{None}
    }
    // 13. First free usable contiguous run length starting at a physical page.
    fn free_run_length_at(&self,physical:u64)->Option<u32>{
        let (index,offset)=self.index_in_span(physical)?;
        let span=self.spans[index];
        if self.allocated(span.bit_offset+offset){return Some(0);}
        let mut run=0u32;
        for i in offset..span.count{
            if self.allocated(span.bit_offset+i){break;}
            run=run.saturating_add(1);
        }
        Some(run)
    }
    // 14. First busy contiguous run length from an aligned physical page.
    fn busy_run_length_at(&self,physical:u64)->Option<u32>{
        let (index,offset)=self.index_in_span(physical)?;
        let span=self.spans[index];
        if !self.allocated(span.bit_offset+offset){return Some(0);}
        let mut run=0u32;
        for i in offset..span.count{
            if !self.allocated(span.bit_offset+i){break;}
            run=run.saturating_add(1);
        }
        Some(run)
    }
    // 15. Number of currently free runs of at least a requested size.
    fn free_run_count_at_least(&self,minimum:u32)->Option<u64>{
        if !self.initialized||minimum==0{return None;}
        let mut count=0;
        for span in &self.spans[..self.count]{
            let mut run=0u64;
            for i in 0..span.count{
                if self.allocated(span.bit_offset+i){
                    if run>=minimum as u64{count+=1;}
                    run=0;
                }else{run+=1;}
            }
            if run>=minimum as u64{count+=1;}
        }
        Some(count)
    }
    // 16. Number of allocated contiguous runs across all managed spans.
    fn busy_run_count_total(&self)->Option<u64>{
        if !self.initialized{return None;}
        let mut count=0;
        for span in &self.spans[..self.count]{
            let mut was_free=true;
            for i in 0..span.count{
                let busy=self.allocated(span.bit_offset+i);
                if busy&&was_free{count+=1;}
                was_free=!busy;
            }
        }
        Some(count)
    }
    // 17. Count free frames in runs large enough for an allocation.
    fn usable_free_for_run(&self,minimum:u32)->Option<u64>{
        if !self.initialized||minimum==0{return None;}
        let mut total=0;
        for i in 0..self.count{total+=self.span_free_frames_in_runs(i,minimum)?;}
        Some(total)
    }
    // 18. Count stranded free pages in too-small contiguous fragments.
    fn fragmented_free_for_run(&self,minimum:u32)->Option<u64>{
        if !self.initialized||minimum==0{return None;}
        Some((self.total-self.used).saturating_sub(self.usable_free_for_run(minimum)?))
    }
    // 19. Validate multiple disjoint physical spans in a map consistency scan.
    fn validate_active_spans(&self)->bool{
        if !self.initialized||self.count==0||self.count>MAX_SPANS{return false;}
        let mut sum=0u64;
        for (i,s) in self.spans[..self.count].iter().enumerate(){
            if s.count==0||s.base%PAGE!=0||s.bit_offset!=sum{return false;}
            let Some(end)=s.base.checked_add(s.count.checked_mul(PAGE).unwrap_or(0))else{return false;};
            for p in &self.spans[..i]{
                let Some(prev_end)=p.base.checked_add(p.count.checked_mul(PAGE).unwrap_or(0))else{return false;};
                if s.base<prev_end&&p.base<end{return false;}
            }
            sum=match sum.checked_add(s.count){Some(v)=>v,None=>return false};
        }
        sum==self.total
    }
    // 20. Verify busy/free accounting and span layout as one boot integrity gate.
    fn verify_allocator_layout(&self)->bool{
        self.validate_active_spans()&&self.verify_frame_accounting()
    }
}
#[cfg(test)]
mod r084_batch_vi_pmm_tests{
    use super::*;
    unsafe extern "C" fn read(i:u64,out:*mut Region,ctx:*mut c_void)->bool{
        let map=unsafe{&*(ctx.cast::<Vec<Region>>())};
        if let Some(r)=map.get(i as usize){unsafe{out.write(*r)};true}else{false}
    }
    fn fixture()->State{
        let mut s=State::empty();
        let mut map=vec![Region{base:PAGE,length:PAGE*8,kind:0},
            Region{base:PAGE*20,length:PAGE*4,kind:0}];
        assert!(unsafe{initialize(&mut s,2,Some(read),
            (&mut map as *mut Vec<Region>).cast())});s
    }
    #[test]
    fn bounded_allocation_obeys_span_capacity_holes_and_free_run_counters(){
        let mut s=fixture();
        assert_eq!(s.span_next_free_from(0,PAGE*4),Some(PAGE*4));
        assert_eq!(s.span_last_free_before(0,PAGE*5),Some(PAGE*4));
        assert_eq!(s.span_next_busy_from(0,PAGE),None);
        assert_eq!(s.span_last_busy_before(0,PAGE*9),None);
        assert_eq!(s.span_free_in_window(1,20*PAGE,24*PAGE),Some(4));
        assert_eq!(s.span_busy_in_window(1,20*PAGE,24*PAGE),Some(0));
        assert_eq!(s.index_in_span(20*PAGE),Some((1,0)));
        assert_eq!(s.physical_at_span_offset(0,7),Some(8*PAGE));
        assert_eq!(s.span_find_run_from(1,20*PAGE,2),Some(20*PAGE));
        assert_eq!(s.span_find_run_below(1,24*PAGE,2),Some(22*PAGE));
        assert_eq!(s.span_claim_run_from(0,PAGE*3,2),Some(3*PAGE));
        assert_eq!(s.span_claim_run_below(1,24*PAGE,2),Some(22*PAGE));
        assert_eq!(s.free_run_length_at(3*PAGE),Some(0));
        assert_eq!(s.busy_run_length_at(3*PAGE),Some(2));
        assert_eq!(s.free_run_count_at_least(2),Some(3));
        assert_eq!(s.busy_run_count_total(),Some(2));
        assert_eq!(s.usable_free_for_run(2),Some(8));
        assert_eq!(s.fragmented_free_for_run(2),Some(0));
        assert!(s.validate_active_spans());
        assert!(s.verify_allocator_layout());
        assert!(!s.claim_run(9*PAGE,2));
        assert_eq!(s.used,4);
    }
}


// P0 batch19 W1: live PMM must be one-shot; failed initial boot can retry.
#[cfg(test)]
mod kernel_p0_batch19_w1_tests {
    use super::*;
    unsafe extern "C" fn read(i:u64,out:*mut Region,ctx:*mut c_void)->bool {
        if i!=0 || out.is_null() || ctx.is_null() {return false;}
        unsafe {out.write(*(ctx.cast::<Region>()));}
        true
    }
    #[test]
    fn kernel_p0_batch19_w1_reinitialize_keeps_owned_frames() {
        let mut s=State::empty();
        let mut region=Region{base:PAGE,length:PAGE*4,kind:0};
        let context=(&mut region as *mut Region).cast();
        assert!(unsafe {initialize_once(&mut s,1,Some(read),context)});
        assert!(s.claim_run(PAGE,2));
        let before=(s.total,s.used,s.cursor);
        assert!(!unsafe {initialize_once(&mut s,1,Some(read),context)});
        assert_eq!((s.total,s.used,s.cursor),before);
        assert_eq!(s.is_frame_busy(PAGE),Some(true));
        assert_eq!(s.is_frame_busy(2*PAGE),Some(true));
        assert!(s.verify_frame_accounting());
    }
    #[test]
    fn kernel_p0_batch19_w1_invalid_boot_map_allows_safe_retry() {
        let mut s=State::empty();
        let mut region=Region{base:PAGE,length:0,kind:0};
        let context=(&mut region as *mut Region).cast();
        assert!(!unsafe {initialize_once(&mut s,1,Some(read),context)});
        assert!(!s.initialized);
        region.length=PAGE;
        assert!(unsafe {initialize_once(&mut s,1,Some(read),context)});
        assert_eq!(s.total,1);
    }
}


// P0-481–487: release/claim corruption must not mutate live bitmap ownership.
#[cfg(test)]
mod kernel_p0_batch20_w1_tests{
    use super::*;
    unsafe extern "C" fn reader(i:u64,out:*mut Region,ctx:*mut c_void)->bool{
        if i!=0||out.is_null()||ctx.is_null(){return false;}
        unsafe{out.write(*(ctx.cast::<Region>()));}
        true
    }
    fn fresh()->State{
        let mut state=State::empty();
        let mut region=Region{base:PAGE,length:4*PAGE,kind:0};
        assert!(unsafe{initialize_once(&mut state,1,Some(reader),
            (&mut region as *mut Region).cast())});
        state
    }
    #[test]
    fn kernel_p0_batch20_w1_reject_corrupt_underflow_and_overflow(){
        let mut s=fresh();
        assert!(s.claim_run(PAGE,2));
        assert_eq!(s.used,2);
        s.used=1;
        let bitmap=s.bitmap[..2].to_vec();
        assert!(!s.release_run(PAGE,2));
        assert_eq!(&s.bitmap[..2],bitmap.as_slice());
        s.used=5;
        assert!(!s.claim_run(PAGE*3,1));
        assert!(!s.release_run(PAGE,1));
        assert_eq!(&s.bitmap[..2],bitmap.as_slice());
        assert_eq!(s.used,5);
    }
    #[test]
    fn kernel_p0_batch20_w1_preserves_normal_contiguous_lifecycle(){
        let mut s=fresh();
        assert!(s.claim_run(PAGE,4));
        assert_eq!(s.used,4);
        assert!(!s.claim_run(PAGE,1));
        assert!(s.release_run(PAGE,4));
        assert!(!s.release_run(PAGE,4));
        assert_eq!(s.used,0);
        assert!(s.verify_frame_accounting());
    }
}


// P0-521: actual PMM C API free-count and frame retirement fail closed
// on impossible allocator counters. This remains BSP-only, not SMP safe.
#[cfg(test)]
mod kernel_p0_batch21_w1_tests {
    use super::*;
    unsafe extern "C" fn read(i:u64,out:*mut Region,ctx:*mut c_void)->bool {
        if i!=0||out.is_null()||ctx.is_null(){return false;}
        unsafe{out.write(*(ctx.cast::<Region>()));}true
    }
    fn fresh()->State{
        let mut s=State::empty();
        let mut r=Region{base:PAGE,length:PAGE*3,kind:0};
        assert!(unsafe{initialize_once(&mut s,1,Some(read),(&mut r as *mut Region).cast())});
        s
    }
    #[test]fn kernel_p0_batch21_w1_checked_free_never_underflows(){
        let mut s=fresh();
        assert_eq!(s.checked_free(),3);
        assert!(s.claim_run(PAGE,2));
        assert_eq!(s.checked_free(),1);
        s.used=4;
        assert_eq!(s.checked_free(),0);
        assert!(!s.release_run(PAGE,1));
        assert!(!s.claim_run(PAGE*3,1));
        assert_eq!(s.is_frame_busy(PAGE),Some(true));
    }
}


// Batch22 W1 P0-561..563: exercise the actual PMM self-test algorithm
// with failure injection, alias rejection and balanced frame reclamation.
#[cfg(test)]
mod kernel_p0_batch22_w1_tests{
    use super::*;
    #[derive(Default)]
    struct Frames{used:[bool;3],fail_after:Option<usize>,calls:usize,alias:bool}
    fn alloc(s:&mut Frames)->Option<u64>{
        s.calls+=1;
        if s.fail_after.is_some_and(|limit|s.calls>limit){return None;}
        if s.alias&&s.calls==2{return Some(PAGE);}
        for i in 0..3{
            if !s.used[i]{s.used[i]=true;return Some((i as u64+1)*PAGE);}
        }
        None
    }
    fn free(s:&mut Frames,frame:u64)->bool{
        if frame<PAGE||frame%PAGE!=0{return false;}
        let i=(frame/PAGE-1) as usize;
        if i>=s.used.len()||!s.used[i]{return false;}
        s.used[i]=false;true
    }
    #[test]fn kernel_p0_batch22_w1_561_cleanup_after_second_allocation_failure(){
        let mut s=Frames{fail_after:Some(1),..Default::default()};
        assert!(!pmm_self_test_roundtrip(&mut s,alloc,free));
        assert_eq!(s.used,[false;3]);
    }
    #[test]fn kernel_p0_batch22_w1_562_duplicate_address_never_double_freed(){
        let mut s=Frames{alias:true,..Default::default()};
        assert!(!pmm_self_test_roundtrip(&mut s,alloc,free));
        assert_eq!(s.used,[false;3]);
    }
    #[test]fn kernel_p0_batch22_w1_563_roundtrip_reclaims_every_frame(){
        let mut s=Frames::default();
        assert!(pmm_self_test_roundtrip(&mut s,alloc,free));
        assert_eq!(s.used,[false;3]);
        assert!(pmm_self_test_roundtrip(&mut s,alloc,free));
        assert_eq!(s.used,[false;3]);
    }
}


// Batch23 W1 P0-581..584: real discontiguous PMM claim/release paths
// cannot overrun or underflow the bitmap-owner accounting transaction.
#[cfg(test)]
mod kernel_p0_batch23_w1_tests{
    use super::*;
    unsafe extern "C" fn map(i:u64,out:*mut Region,ctx:*mut c_void)->bool{
        if i!=0||out.is_null()||ctx.is_null(){return false;}
        unsafe{out.write(*(ctx.cast::<Region>()));}true
    }
    fn state()->State{
        let mut s=State::empty();
        let mut r=Region{base:PAGE,length:PAGE*4,kind:0};
        assert!(unsafe{initialize_once(&mut s,1,Some(map),(&mut r as *mut Region).cast())});
        s
    }
    #[test]fn kernel_p0_batch23_w1_581_claim_fails_on_corrupt_used_overflow(){
        let mut s=state();s.used=s.total+1;
        assert!(!s.claim_exact_frame_set(&[PAGE]));
        assert!(!s.allocated(0));assert_eq!(s.used,5);
    }
    #[test]fn kernel_p0_batch23_w1_582_claim_budget_has_no_partial_effect(){
        let mut s=state();s.used=3;
        assert!(!s.claim_exact_frame_set(&[PAGE,2*PAGE]));
        assert!(!s.allocated(0));assert!(!s.allocated(1));
    }
    #[test]fn kernel_p0_batch23_w1_583_release_underflow_preserves_bitmap(){
        let mut s=state();assert!(s.claim_exact_frame_set(&[PAGE,2*PAGE]));
        s.used=1;
        assert!(!s.release_exact_frame_set(&[PAGE,2*PAGE]));
        assert!(s.allocated(0));assert!(s.allocated(1));
    }
    #[test]fn kernel_p0_batch23_w1_584_good_claim_release_reuses_frames(){
        let mut s=state();
        assert!(s.claim_exact_frame_set(&[PAGE,3*PAGE]));
        assert!(!s.claim_exact_frame_set(&[PAGE]));
        assert!(s.release_exact_frame_set(&[PAGE,3*PAGE]));
        assert!(!s.release_exact_frame_set(&[PAGE,3*PAGE]));
        assert_eq!(s.used,0);assert!(s.verify_frame_accounting());
    }
}


// Batch24 W1 P0-606..609: one-shot C ABI compatibility and allocator
// counter/cursor guards exercise State::claim_single_frame, not a mock.
#[cfg(test)]
mod kernel_p0_batch24_w1_tests{
    use super::*;
    unsafe extern "C" fn map(i:u64,out:*mut Region,ctx:*mut c_void)->bool{
        if i!=0||out.is_null()||ctx.is_null(){return false;}
        unsafe{out.write(*(ctx.cast::<Region>()));}true
    }
    fn fresh()->State{
        let mut s=State::empty();let mut r=Region{base:PAGE,length:PAGE*3,kind:0};
        assert!(unsafe{initialize_once(&mut s,1,Some(map),(&mut r as *mut Region).cast())});
        s
    }
    #[test]fn kernel_p0_batch24_w1_606_one_shot_reinit_preserves_live_frames(){
        let mut s=fresh();
        let first=s.claim_single_frame().unwrap();
        let mut alternate=Region{base:PAGE*100,length:PAGE,kind:0};
        assert!(!unsafe{initialize_once(&mut s,1,Some(map),(&mut alternate as *mut Region).cast())});
        assert_eq!(s.total,3);assert_eq!(s.used,1);
        assert_eq!(s.is_frame_busy(first),Some(true));
        assert!(s.release_run(first,1));assert!(s.verify_frame_accounting());
    }
    #[test]fn kernel_p0_batch24_w1_607_corrupt_cursor_refuses_allocation(){
        let mut s=fresh();s.cursor=s.total;
        assert_eq!(s.claim_single_frame(),None);
        assert_eq!(s.used,0);assert!(!s.verify_frame_accounting());
        s.cursor=0;assert!(s.verify_frame_accounting());
    }
    #[test]fn kernel_p0_batch24_w1_608_corrupt_count_refuses_allocation(){
        let mut s=fresh();s.used=s.total+1;
        assert_eq!(s.claim_single_frame(),None);
        assert_eq!(s.used,4);assert!(!s.allocated(0));
    }
    #[test]fn kernel_p0_batch24_w1_609_exhaustion_recycle_is_one_owner(){
        let mut s=fresh();
        let a=s.claim_single_frame().unwrap();let b=s.claim_single_frame().unwrap();
        let c=s.claim_single_frame().unwrap();
        assert_ne!(a,b);assert_ne!(b,c);
        assert_eq!(s.claim_single_frame(),None);
        assert!(s.release_run(b,1));
        assert_eq!(s.claim_single_frame(),Some(b));
        assert!(s.verify_frame_accounting());
    }
}

// P0-626..630: real bitmap geometry gates before any single-frame mutation.
#[cfg(test)]
mod kernel_p0_batch25_w1_tests {
    use super::*;
    unsafe extern "C" fn reader(i:u64,out:*mut Region,ctx:*mut c_void)->bool{
        if i!=0||out.is_null()||ctx.is_null(){return false;}
        unsafe{out.write(*(ctx.cast::<Region>()));}true
    }
    fn fresh()->State{
        let mut s=State::empty();
        let mut r=Region{base:PAGE,length:3*PAGE,kind:0};
        assert!(unsafe{initialize_once(&mut s,1,Some(reader),(&mut r as *mut Region).cast())});
        s
    }
    #[test]fn kernel_p0_batch25_w1_626_healthy_geometry_allocates_exact_owner(){
        let mut s=fresh();assert!(s.single_frame_geometry_valid());
        assert_eq!(s.claim_single_frame(),Some(PAGE));
        assert_eq!(s.used,1);assert!(s.verify_frame_accounting());
    }
    #[test]fn kernel_p0_batch25_w1_627_oversized_total_never_indexes_bitmap(){
        let mut s=fresh();s.total=MAX_FRAMES as u64+1;
        assert!(!s.single_frame_geometry_valid());
        assert_eq!(s.claim_single_frame(),None);assert_eq!(s.used,0);
    }
    #[test]fn kernel_p0_batch25_w1_628_span_count_overflow_denied(){
        let mut s=fresh();s.count=MAX_SPANS+1;
        assert_eq!(s.claim_single_frame(),None);
        assert_eq!(s.used,0);assert_eq!(s.bitmap[0],0);
    }
    #[test]fn kernel_p0_batch25_w1_629_bit_offset_gap_denied_atomically(){
        let mut s=fresh();s.spans[0].bit_offset=1;
        assert_eq!(s.claim_single_frame(),None);
        assert_eq!(s.used,0);assert_eq!(s.bitmap[0],0);
    }
    #[test]fn kernel_p0_batch25_w1_630_bad_alignment_and_overflow_denied(){
        let mut s=fresh();s.spans[0].base=PAGE+1;
        assert_eq!(s.claim_single_frame(),None);
        s.spans[0].base=u64::MAX-PAGE+1;
        assert_eq!(s.claim_single_frame(),None);assert_eq!(s.used,0);
    }
}

// P0-656..659: contiguous PMM shares the one-frame geometry/ownership gate.
#[cfg(test)]
mod kernel_p0_batch26_w1_tests {
    use super::*;
    unsafe extern "C" fn reader(i:u64,out:*mut Region,ctx:*mut c_void)->bool {
        if i!=0||out.is_null()||ctx.is_null(){return false;}
        unsafe{out.write(*(ctx.cast::<Region>()));}true
    }
    fn fresh()->State {
        let mut s=State::empty();
        let mut r=Region{base:PAGE,length:4*PAGE,kind:0};
        assert!(unsafe{initialize_once(&mut s,1,Some(reader),(&mut r as *mut Region).cast())});
        s
    }
    #[test]fn kernel_p0_batch26_w1_656_contiguous_allocates_exact_run(){
        let mut s=fresh();assert_eq!(s.claim_contiguous_run(2),Some(PAGE));
        assert_eq!(s.used,2);assert!(s.verify_frame_accounting());
    }
    #[test]fn kernel_p0_batch26_w1_657_corrupt_span_never_mutates_bitmap(){
        let mut s=fresh();s.spans[0].bit_offset=1;
        assert_eq!(s.claim_contiguous_run(2),None);
        assert_eq!(s.used,0);assert_eq!(s.bitmap[0],0);
    }
    #[test]fn kernel_p0_batch26_w1_658_impossible_accounting_denies_run(){
        let mut s=fresh();s.used=s.total+1;
        assert_eq!(s.claim_contiguous_run(2),None);
        assert_eq!(s.bitmap[0],0);
    }
    #[test]fn kernel_p0_batch26_w1_659_occupied_page_skips_to_free_run(){
        let mut s=fresh();assert_eq!(s.claim_single_frame(),Some(PAGE));
        assert_eq!(s.claim_contiguous_run(2),Some(PAGE*2));
        assert_eq!(s.used,3);assert!(s.verify_frame_accounting());
    }
}

#[cfg(test)]
mod kernel_p0_batch27_w1_tests {
    use super::*;
    fn with_span()->State {
        let mut state=State::empty();
        state.count=1;
        state.spans[0]=Span{base:PAGE,count:3,bit_offset:2};
        state
    }
    #[test]fn kernel_p0_batch27_w1_676_valid_owned_first_frame(){
        let s=with_span();assert_eq!(s.physical_at(2),Some(PAGE));
    }
    #[test]fn kernel_p0_batch27_w1_677_respects_span_end_and_offset(){
        let s=with_span();assert_eq!(s.physical_at(4),Some(3*PAGE));
        assert_eq!(s.physical_at(1),None);assert_eq!(s.physical_at(5),None);
    }
    #[test]fn kernel_p0_batch27_w1_678_rejects_physical_address_overflow(){
        let mut s=with_span();s.spans[0].base=u64::MAX-PAGE+1;
        assert_eq!(s.physical_at(3),None);
    }
    #[test]fn kernel_p0_batch27_w1_679_rejects_corrupt_metadata_without_panic(){
        let mut s=with_span();s.count=MAX_SPANS+1;
        assert_eq!(s.physical_at(2),None);
        s.count=1;s.spans[0]=Span{base:PAGE,count:u64::MAX,bit_offset:0};
        assert_eq!(s.physical_at(u64::MAX),None);
    }
}

#[cfg(test)]
mod kernel_p0_batch28_w1 {
 use super::*;
 fn state()->State {
  let mut s=State::empty();s.initialized=true;s.count=2;s.total=4;
  s.spans[0]=Span{base:PAGE,count:2,bit_offset:0};
  s.spans[1]=Span{base:10*PAGE,count:2,bit_offset:2};s
 }
 #[test]fn kernel_p0_batch28_w1_701_disjoint_spans_allocate_unique_pages(){
  let mut s=state();assert_eq!(s.claim_single_frame(),Some(PAGE));
  assert_eq!(s.claim_single_frame(),Some(2*PAGE));
  assert_eq!(s.claim_single_frame(),Some(10*PAGE));assert_eq!(s.used,3);
 }
 #[test]fn kernel_p0_batch28_w1_702_overlap_rejected_without_allocation(){
  let mut s=state();s.spans[1].base=2*PAGE;
  assert!(!s.single_frame_geometry_valid());
  assert_eq!(s.claim_single_frame(),None);assert_eq!(s.used,0);
 }
 #[test]fn kernel_p0_batch28_w1_703_adjacent_physical_spans_still_valid(){
  let mut s=state();s.spans[1].base=3*PAGE;
  assert!(s.single_frame_geometry_valid());
  for page in 1..=4{assert_eq!(s.claim_single_frame(),Some(page*PAGE));}
 }
 #[test]fn kernel_p0_batch28_w1_704_reordered_overlap_keeps_bitmap_unchanged(){
  let mut s=state();s.spans[0].base=10*PAGE;s.spans[1].base=11*PAGE;
  assert_eq!(s.claim_single_frame(),None);
  assert_eq!(s.bitmap[0],0);assert_eq!(s.cursor,0);
 }
}

#[cfg(test)]
mod kernel_p0_batch29_w1 {
 use super::*;
 fn state()->State{
  let mut s=State::empty();s.initialized=true;s.count=2;s.total=4;
  s.spans[0]=Span{base:PAGE,count:2,bit_offset:0};
  s.spans[1]=Span{base:10*PAGE,count:2,bit_offset:2};s
 }
 #[test] fn kernel_p0_batch29_w1_721_exact_run_claim(){let mut s=state();assert!(s.claim_run(PAGE,2));assert!(s.verify_frame_accounting());}
 #[test] fn kernel_p0_batch29_w1_722_overlap_no_mutation(){let mut s=state();s.spans[1].base=2*PAGE;assert!(!s.claim_run(PAGE,2));assert_eq!(s.bitmap[0],0);}
 #[test] fn kernel_p0_batch29_w1_723_corrupt_offset_no_release(){let mut s=state();assert!(s.claim_run(PAGE,1));s.spans[1].bit_offset=1;assert!(!s.release_run(PAGE,1));assert!(s.allocated(0));}
 #[test] fn kernel_p0_batch29_w1_724_cursor_corruption_no_claim(){let mut s=state();s.cursor=s.total;assert!(!s.claim_run(10*PAGE,1));assert_eq!(s.used,0);}
 #[test] fn kernel_p0_batch29_w1_725_atomic_release_no_double_free(){let mut s=state();assert!(s.claim_run(PAGE,2));assert!(!s.release_run(PAGE,3));assert_eq!(s.used,2);assert!(s.release_run(PAGE,2));assert!(!s.release_run(PAGE,2));}
}

#[cfg(test)]
mod kernel_p0_batch30_w1 {
    use super::*;
    fn state()->State {
        let mut s=State::empty();s.initialized=true;s.count=2;s.total=4;
        s.spans[0]=Span{base:PAGE,count:2,bit_offset:0};
        s.spans[1]=Span{base:10*PAGE,count:2,bit_offset:2};
        s
    }
    #[test] fn kernel_p0_batch30_w1_746_valid_exact_set_is_one_transaction(){
        let mut s=state();assert!(s.claim_exact_frame_set(&[PAGE,10*PAGE]));
        assert_eq!(s.used,2);assert!(s.verify_frame_accounting());
        assert!(s.release_exact_frame_set(&[PAGE,10*PAGE]));
        assert_eq!(s.used,0);
    }
    #[test] fn kernel_p0_batch30_w1_747_overlap_rejects_exact_set_without_mutation(){
        let mut s=state();s.spans[1].base=2*PAGE;
        assert!(!s.claim_exact_frame_set(&[PAGE,2*PAGE]));
        assert_eq!(s.used,0);assert_eq!(s.bitmap[0],0);
    }
    #[test] fn kernel_p0_batch30_w1_748_corrupt_offset_denies_release_of_live_frame(){
        let mut s=state();assert!(s.claim_exact_frame_set(&[PAGE]));
        s.spans[1].bit_offset=1;
        assert!(!s.release_exact_frame_set(&[PAGE]));
        assert!(s.allocated(0));assert_eq!(s.used,1);
    }
    #[test] fn kernel_p0_batch30_w1_749_corrupt_geometry_denies_snapshot_and_accounting(){
        let mut s=state();s.cursor=s.total;
        let sentinel=FrameSpanUsage{base:77,frames:7,busy:7,longest_free:7};
        let mut out=[sentinel;2];
        assert!(!s.verify_frame_accounting());
        assert_eq!(s.snapshot_span_usage(&mut out),None);
        assert_eq!(out,[sentinel;2]);
    }
}

#[cfg(test)]
mod kernel_p0_batch31_w1 {
    use super::*;
    fn fresh()->State {
        let mut s=State::empty();s.initialized=true;s.count=2;s.total=4;
        s.spans[0]=Span{base:PAGE,count:2,bit_offset:0};
        s.spans[1]=Span{base:10*PAGE,count:2,bit_offset:2};s
    }
    #[test]fn kernel_p0_batch31_w1_766_corrupt_cursor_fails_closed_and_recovers(){
        let mut s=fresh();s.cursor=s.total;
        assert_eq!(s.claim_single_frame(),None);
        assert!(!s.verify_frame_accounting());assert_eq!(s.bitmap[0],0);
        s.cursor=0;assert!(s.verify_frame_accounting());
    }
    #[test]fn kernel_p0_batch31_w1_767_claim_candidates_reject_aliased_spans(){
        let mut s=fresh();assert!(s.can_claim_frames(&[PAGE,10*PAGE]));
        s.spans[1].base=2*PAGE;
        assert!(!s.can_claim_frames(&[PAGE,2*PAGE]));
        assert_eq!(s.frame_span_index(PAGE),None);assert_eq!(s.bitmap[0],0);
    }
    #[test]fn kernel_p0_batch31_w1_768_release_candidates_reject_corrupt_offsets(){
        let mut s=fresh();assert!(s.claim_exact_frame_set(&[PAGE]));
        assert!(s.can_release_frames(&[PAGE]));s.spans[1].bit_offset=1;
        assert!(!s.can_release_frames(&[PAGE]));
        assert!(!s.release_selected(&[PAGE]));assert!(s.allocated(0));assert_eq!(s.used,1);
    }
    #[test]fn kernel_p0_batch31_w1_769_snapshot_requires_exact_accounting(){
        let mut s=fresh();assert!(s.claim_exact_frame_set(&[PAGE]));
        let sentinel=FrameSpanUsage{base:77,frames:7,busy:7,longest_free:7};
        let mut out=[sentinel;2];
        assert_eq!(s.snapshot_span_usage(&mut out),Some(2));assert_eq!(out[0].busy,1);
        s.used=0;let mut unchanged=[sentinel;2];
        assert_eq!(s.snapshot_span_usage(&mut unchanged),None);
        assert_eq!(unchanged,[sentinel;2]);
    }
}

#[cfg(test)]
mod kernel_p0_batch32_w1 {
    use super::*;
    fn state()->State {
        let mut s=State::empty();
        s.initialized=true;s.count=1;s.total=4;
        s.spans[0]=Span{base:PAGE,count:4,bit_offset:0};
        s
    }
    #[test] fn kernel_p0_batch32_w1_786_valid_atomic_claim_is_releasable(){
        let mut s=state();
        assert!(s.claim_exact_frame_set(&[PAGE,3*PAGE]));
        assert!(s.verify_frame_accounting());assert_eq!(s.used,2);
        assert!(s.release_exact_frame_set(&[3*PAGE,PAGE]));
        assert_eq!(s.used,0);assert!(s.verify_frame_accounting());
    }
    #[test] fn kernel_p0_batch32_w1_787_mismatched_used_denies_claim_without_bitmap_mutation(){
        let mut s=state();s.used=1;
        assert!(!s.claim_exact_frame_set(&[PAGE]));assert_eq!(s.bitmap[0],0);
        assert_eq!(s.used,1);s.used=0;
        assert!(s.claim_exact_frame_set(&[PAGE]));
    }
    #[test] fn kernel_p0_batch32_w1_788_mismatched_used_denies_release_of_owned_frame(){
        let mut s=state();assert!(s.claim_exact_frame_set(&[PAGE]));
        s.used=0;assert!(!s.release_exact_frame_set(&[PAGE]));
        assert!(s.allocated(0));s.used=1;
        assert!(s.release_exact_frame_set(&[PAGE]));
    }
    #[test] fn kernel_p0_batch32_w1_789_claim_and_release_preflight_refuse_corrupt_accounting(){
        let mut s=state();assert!(s.claim_exact_frame_set(&[PAGE]));
        s.used=2;
        assert!(!s.can_claim_frames(&[2*PAGE]));
        assert!(!s.can_release_frames(&[PAGE]));
        s.used=1;assert!(s.can_claim_frames(&[2*PAGE]));
        assert!(s.can_release_frames(&[PAGE]));
    }
}

#[cfg(test)]
mod kernel_p0_batch33_w1 {
    use super::*;
    fn state()->State{
        let mut s=State::empty();
        s.initialized=true;s.count=1;s.total=8;
        s.spans[0]=Span{base:PAGE,count:8,bit_offset:0};s
    }
    #[test] fn kernel_p0_batch33_w1_806_corrupted_accounting_denies_exact_dma_claim(){
        let mut s=state();s.used=1;
        assert!(!s.claim_run(PAGE,2));assert_eq!(s.bitmap[0],0);
        assert_eq!(s.used,1);
    }
    #[test] fn kernel_p0_batch33_w1_807_corrupted_accounting_denies_release(){
        let mut s=state();assert!(s.claim_run(PAGE,2));
        s.used=1;assert!(!s.release_run(PAGE,2));
        assert!(s.allocated(0));assert!(s.allocated(1));assert_eq!(s.used,1);
        s.used=2;assert!(s.release_run(PAGE,2));
    }
    #[test] fn kernel_p0_batch33_w1_808_corrupted_accounting_denies_contiguous_claim(){
        let mut s=state();s.used=1;
        assert_eq!(s.claim_contiguous_run(2),None);
        assert_eq!(s.bitmap[0],0);
    }
    #[test] fn kernel_p0_batch33_w1_809_valid_aligned_highest_and_contiguous_runs(){
        let mut s=state();
        assert_eq!(s.claim_aligned_run(2,2),Some(2*PAGE));
        assert_eq!(s.claim_highest_run(2),Some(7*PAGE));
        assert_eq!(s.claim_contiguous_run(2),Some(4*PAGE));
        assert_eq!(s.used,6);assert!(s.verify_frame_accounting());
        assert!(s.release_run(2*PAGE,2));
        assert!(s.release_run(7*PAGE,2));
        assert!(s.release_run(4*PAGE,2));
        assert_eq!(s.used,0);assert!(s.verify_frame_accounting());
    }
}

#[cfg(test)]
mod p0_hhdm_zero_page_allocator_regression {
    use super::*;
    unsafe extern "C" fn read(index:u64,out:*mut Region,context:*mut c_void)->bool {
        let map=unsafe{&*(context.cast::<Vec<Region>>())};
        if let Some(region)=map.get(index as usize){
            unsafe{out.write(*region)};true
        }else{false}
    }
    #[test]
    fn firmware_zero_origin_is_not_an_allocatable_pmm_frame(){
        let mut s=State::empty();
        let mut map=vec![Region{base:0,length:PAGE*4,kind:0}];
        assert!(unsafe{initialize(&mut s,1,Some(read),
            (&mut map as *mut Vec<Region>).cast())});
        assert!(s.verify_frame_accounting());
        assert_eq!(s.total,3);
        assert_eq!(s.spans[0].base,PAGE);
        assert_eq!(s.bit_for_physical(0),None);
        assert!(!s.is_managed_frame(0));
        assert_eq!(s.claim_single_frame(),Some(PAGE));
        assert!(s.verify_frame_accounting());
    }
}

#[cfg(test)]
mod kernel_p0_batch34_w1 {
    use super::*;
    unsafe extern "C" fn read(i:u64,out:*mut Region,ctx:*mut c_void)->bool {
        let regions=unsafe{&*(ctx.cast::<Vec<Region>>())};
        if let Some(v)=regions.get(i as usize){unsafe{out.write(*v)};true}else{false}
    }
    fn setup()->State{
        let mut s=State::empty();
        let mut map=vec![Region{base:PAGE,length:4*PAGE,kind:0},
            Region{base:20*PAGE,length:3*PAGE,kind:0}];
        assert!(unsafe{initialize(&mut s,2,Some(read),
            (&mut map as *mut Vec<Region>).cast())});
        s
    }
    const SENTINEL:FrameSpanUsage=FrameSpanUsage{
        base:0xfeed,frames:0xfeed,busy:0xfeed,longest_free:0xfeed,
    };
    #[test] fn kernel_p0_batch34_w1_826_two_span_snapshot_matches_ownership(){
        let mut s=setup();assert!(s.claim_run(PAGE*2,2));
        let mut out=[SENTINEL;3];
        assert_eq!(s.snapshot_span_usage(&mut out),Some(2));
        assert_eq!(out[0],FrameSpanUsage{base:PAGE,frames:4,busy:2,longest_free:1});
        assert_eq!(out[1],FrameSpanUsage{base:20*PAGE,frames:3,busy:0,longest_free:3});
        assert_eq!(out[2],SENTINEL);
    }
    #[test] fn kernel_p0_batch34_w1_827_short_snapshot_has_no_partial_publication(){
        let s=setup();let mut out=[SENTINEL;1];
        assert_eq!(s.snapshot_span_usage(&mut out),None);
        assert_eq!(out,[SENTINEL;1]);
    }
    #[test] fn kernel_p0_batch34_w1_828_corrupt_bitmap_denies_snapshot_without_writes(){
        let mut s=setup();s.set_bit(0,true); // bookkeeping still reports zero
        let mut out=[SENTINEL;2];
        assert_eq!(s.snapshot_span_usage(&mut out),None);
        assert_eq!(out,[SENTINEL;2]);
    }
    #[test] fn kernel_p0_batch34_w1_829_release_updates_snapshot_without_losing_span(){
        let mut s=setup();assert!(s.claim_run(20*PAGE,2));
        assert!(s.release_run(20*PAGE,2));
        let mut out=[SENTINEL;2];
        assert_eq!(s.snapshot_span_usage(&mut out),Some(2));
        assert_eq!(out[1].busy,0);assert_eq!(out[1].longest_free,3);
    }
}

#[cfg(test)] mod kernel_p0_batch35_w1 {
 use super::*;
 fn state()->State {
  let mut s=State::empty();s.initialized=true;s.count=1;s.total=8;
  s.spans[0]=Span{base:PAGE,count:8,bit_offset:0};s
 }
 #[test] fn kernel_p0_batch35_w1_846_corrupt_accounting_rejects_aligned_search(){
  let mut s=state();s.used=1;
  assert_eq!(s.find_aligned_run(2,2),None);assert_eq!(s.bitmap[0],0);
 }
 #[test] fn kernel_p0_batch35_w1_847_corrupt_bitmap_rejects_highest_search(){
  let mut s=state();s.set_bit(0,true);
  assert_eq!(s.highest_available_run(2),None);assert_eq!(s.used,0);
 }
 #[test] fn kernel_p0_batch35_w1_848_valid_aligned_and_highest_disjoint(){
  let mut s=state();assert_eq!(s.find_aligned_run(2,2),Some(2*PAGE));
  assert!(s.claim_run(2*PAGE,2));
  assert_eq!(s.highest_available_run(2),Some(7*PAGE));
  assert!(s.claim_run(7*PAGE,2));assert_eq!(s.used,4);
 }
 #[test] fn kernel_p0_batch35_w1_849_release_makes_highest_run_available_again(){
  let mut s=state();assert!(s.claim_run(7*PAGE,2));
  assert_eq!(s.highest_available_run(2),Some(5*PAGE));
  assert!(s.release_run(7*PAGE,2));
  assert_eq!(s.highest_available_run(2),Some(7*PAGE));
  assert!(s.verify_frame_accounting());
 }
}

#[cfg(test)]
mod kernel_p0_batch36_w1 {
    use super::*;
    fn state()->State {
        let mut s=State::empty();
        s.initialized=true;s.count=1;s.total=4;
        s.spans[0]=Span{base:PAGE,count:4,bit_offset:0};s
    }
    #[test] fn kernel_p0_batch36_w1_866_live_ownership_free_count_roundtrip(){
        let mut s=state();assert_eq!(s.trusted_free_count(),4);
        assert!(s.claim_run(PAGE,2));assert_eq!(s.trusted_free_count(),2);
        assert!(s.release_run(PAGE,2));assert_eq!(s.trusted_free_count(),4);
    }
    #[test] fn kernel_p0_batch36_w1_867_forged_usage_count_never_published(){
        let mut s=state();s.used=1;
        assert_eq!(s.checked_free(),3);
        assert_eq!(s.trusted_free_count(),0);
    }
    #[test] fn kernel_p0_batch36_w1_868_unaccounted_bitmap_occupancy_denied(){
        let mut s=state();s.set_bit(3,true);
        assert_eq!(s.trusted_free_count(),0);
        s.used=1;assert_eq!(s.trusted_free_count(),3);
    }
    #[test] fn kernel_p0_batch36_w1_869_bad_cursor_span_and_uninitialized_denied(){
        let mut s=state();s.cursor=s.total;
        assert_eq!(s.trusted_free_count(),0);
        s=state();s.spans[0].count=5;
        assert_eq!(s.trusted_free_count(),0);
        s=State::empty();assert_eq!(s.trusted_free_count(),0);
    }
}

/// Allocate a page-rounded compositor buffer (at most 10,240,000 bytes).
/// # Safety: serialized BSP, IRQs disabled; returned frames exclusively owned.
pub(crate) unsafe fn allocate_buffer_frames(count:u32)->Option<u64>{
    unsafe{state()}.claim_contiguous_run_bounded(count,2500)
}
/// # Safety: same allocator admission; all users of this owned run stopped.
pub(crate) unsafe fn release_buffer_frames(physical:u64,count:u32)->bool{
    unsafe{state()}.release_run_bounded(physical,count,2500)
}
#[cfg(test)]
mod kernel_buffer_run_tests{
    use super::*;
    unsafe extern "C" fn read(index:u64,out:*mut Region,_:*mut c_void)->bool{
        if index!=0||out.is_null(){return false;}
        unsafe{out.write(Region{base:PAGE,length:PAGE*3000,kind:0});}true
    }
    #[test]fn desktop_sized_runs_use_real_bitmap_without_widening_dma_abi(){
        let mut s=State::empty();
        assert!(unsafe{initialize(&mut s,1,Some(read),ptr::null_mut())});
        assert!(s.claim_contiguous_run(17).is_none());
        let start=s.claim_contiguous_run_bounded(2500,2500).unwrap();
        assert_eq!(start,PAGE);assert_eq!(s.used,2500);
        assert!(s.claim_contiguous_run_bounded(501,2500).is_none());
        assert!(!s.release_run(start,2500));assert_eq!(s.used,2500);
        assert!(s.release_run_bounded(start,2500,2500));assert_eq!(s.used,0);
        assert!(!s.release_run_bounded(start,2500,2500));
        assert!(s.claim_contiguous_run_bounded(2501,2500).is_none());
        assert!(s.claim_contiguous_run_bounded(0,2500).is_none());
        assert!(s.verify_frame_accounting());
    }
    #[test]fn buffer_fragmentation_and_partial_release_are_atomic(){
        let mut s=State::empty();
        assert!(unsafe{initialize(&mut s,1,Some(read),ptr::null_mut())});
        assert!(s.claim_run(PAGE*1500,1));
        assert!(s.claim_contiguous_run_bounded(2500,2500).is_none());
        assert_eq!(s.used,1);
        let first=s.claim_contiguous_run_bounded(1000,2500).unwrap();
        assert!(!s.release_run_bounded(first,1001,2500));assert_eq!(s.used,1001);
        assert!(s.release_run_bounded(first,1000,2500));
        assert!(s.release_run(PAGE*1500,1));assert_eq!(s.used,0);
        assert!(s.claim_contiguous_run_bounded(2500,2500).is_some());
    }
}
