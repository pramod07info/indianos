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
    s.reset();
    let Some(callback) = reader else { return false; };
    if !(1..=MAX_ENTRIES).contains(&region_count) { return false; }
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
        for other_index in 0..region_count {
            if other_index == index { continue; }
            let Some(other) = (unsafe { read_region(callback, other_index, context) }) else {
                s.reset();
                return false;
            };
            if overlap(begin, end, other.base, other.base + other.length) {
                s.reset();
                return false;
            }
        }
        if s.count == MAX_SPANS { s.reset(); return false; }
        let mut count = (end - begin) / PAGE;
        if s.total == MAX_FRAMES as u64 { break; }
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
