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
    fn physical_at(&self, bit: u64) -> Option<u64> {
        for span in &self.spans[..self.count] {
            if bit >= span.bit_offset && bit - span.bit_offset < span.count {
                return Some(span.base + (bit - span.bit_offset) * PAGE);
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
    unsafe { initialize(s, region_count, reader, context) }
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
#[no_mangle]
pub unsafe extern "C" fn pmm_allocate_frame(physical: *mut u64) -> bool {
    if physical.is_null() { return false; }
    let s = unsafe { state() };
    if !s.initialized || s.used == s.total { return false; }
    for scanned in 0..s.total {
        let mut bit = s.cursor + scanned;
        if bit >= s.total { bit -= s.total; }
        if !s.allocated(bit) {
            let Some(address) = s.physical_at(bit) else { return false; };
            s.set_bit(bit, true);
            s.used += 1;
            s.cursor = if bit + 1 == s.total { 0 } else { bit + 1 };
            unsafe { ptr::write(physical, address); }
            return true;
        }
    }
    false
}

/// Acquire N physically contiguous 4 KiB frames from ONE usable RAM span.
/// Required for legacy virtio vrings which do not support scattered ring pages.
/// # Safety
/// Single BSP with interrupts disabled; out is exclusive, aligned and writable.
#[no_mangle]
pub unsafe extern "C" fn pmm_allocate_contiguous_frames(count:u32,out:*mut u64)->bool{
    if out.is_null() || count==0 || count>16{return false;}
    let s=unsafe{state()};
    let want=count as u64;
    if !s.initialized || want>s.total-s.used{return false;}
    for span_index in 0..s.count{
        let span=s.spans[span_index];
        if span.count<want{continue;}
        for offset in 0..=span.count-want{
            let first_bit=span.bit_offset+offset;
            if (0..want).any(|i|s.allocated(first_bit+i)){continue;}
            let Some(first)=span.base.checked_add(offset*PAGE)else{continue;};
            for i in 0..want{s.set_bit(first_bit+i,true);}
            s.used+=want;
            s.cursor=(first_bit+want)%s.total;
            unsafe{ptr::write(out,first);}
            return true;
        }
    }
    false
}

/// # Safety
/// Caller passes a previously allocated physical frame and uses PMM only on
/// one CPU. Duplicate frees, unaligned frames and reserved RAM are rejected.
#[no_mangle]
pub unsafe extern "C" fn pmm_release_frame(physical: u64) -> bool {
    let s = unsafe { state() };
    if !s.initialized || physical & (PAGE - 1) != 0 { return false; }
    for i in 0..s.count {
        let span = s.spans[i];
        if physical < span.base { continue; }
        let diff = physical - span.base;
        if diff / PAGE >= span.count { continue; }
        let bit = span.bit_offset + diff / PAGE;
        if !s.allocated(bit) { return false; }
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
    if s.initialized { s.total - s.used } else { 0 }
}
#[no_mangle]
pub extern "C" fn pmm_self_test() -> bool {
    if pmm_free_frames() < 2 { return false; }
    let before = pmm_free_frames();
    let (mut first, mut second, mut recycled) = (0, 0, 0);
    let worked = unsafe {
        pmm_allocate_frame(&mut first) &&
        pmm_allocate_frame(&mut second) &&
        first != second && first != 0 && second != 0 &&
        pmm_release_frame(second) &&
        pmm_allocate_frame(&mut recycled) && recycled == second &&
        pmm_release_frame(first) && pmm_release_frame(recycled)
    };
    worked && pmm_free_frames() == before
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
        if count==0||count>16{return None;}
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
        let Some(first)=self.validate_run(physical,count,false)else{return false;};
        for i in 0..count as u64{self.set_bit(first+i,true);}
        self.used+=count as u64;
        self.cursor=(first+count as u64)%self.total;
        true
    }
    /// Release a complete frame run atomically or none of its frames.
    fn release_run(&mut self,physical:u64,count:u32)->bool{
        let Some(first)=self.validate_run(physical,count,true)else{return false;};
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
        if !self.initialized{return None;}
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
        if count==0||count>16||!self.initialized{return None;}
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
        if addresses.is_empty()||addresses.len()>16||!self.initialized{return false;}
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
        if addresses.is_empty()||addresses.len()>16||!self.initialized{return false;}
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
        self.initialized&&self.used<=self.total&&
        (0..self.total).filter(|i|self.allocated(*i)).count() as u64==self.used
    }
    /// 20. Publish a complete per-span occupancy snapshot or nothing.
    fn snapshot_span_usage(&self,out:&mut[FrameSpanUsage])->Option<usize>{
        if !self.initialized||out.len()<self.count{return None;}
        for (i,s) in self.spans[..self.count].iter().enumerate(){
            out[i]=FrameSpanUsage{
                base:s.base,frames:s.count,
                busy:self.busy_in_span(i)?,
                longest_free:self.longest_free_in_span(i)?,
            };
        }
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
        if !self.initialized||addresses.is_empty()||addresses.len()>16{return false;}
        for (i,&addr) in addresses.iter().enumerate(){
            if !self.is_busy_physical(addr)||addresses[..i].contains(&addr){return false;}
        }
        true
    }
    /// Validate the whole atomic claim set; never claim managed RAM holes.
    fn can_claim_frames(&self,addresses:&[u64])->bool{
        if !self.initialized||addresses.is_empty()||addresses.len()>16{return false;}
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
