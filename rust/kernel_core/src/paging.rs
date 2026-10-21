//! Ownership-aware x86-64 kernel page-table cloning and CPU paging policy.
//!
//! R003 candidate replacement for paging_policy.c and paging_clone.c.
//! All functions operate on detached page roots, except the pure CPU
//! preflight. No CR3 writes, APIC, IRQ masking, or TLB flush happens here.

use core::{ffi::c_void, ptr};
const PHYS: u64 = 0x000f_ffff_ffff_f000;
const PRESENT: u64 = 1;
const HUGE: u64 = 0x80;
const ACCESSED: u64 = 0x20;
const TABLE_ENTRIES: usize = 512;
const CLONE_LIMIT: u32 = 8192;

#[repr(C)]
pub struct CpuState {
    long_mode: bool, nx: bool, msr: bool, pae: bool,
    paging_enabled: bool, pae_enabled: bool, la57_enabled: bool,
    nxe_enabled: bool, write_protect_enabled: bool, pcid_enabled: bool,
    physical_bits: u8, cr3: u64,
}
#[no_mangle]
pub unsafe extern "C" fn paging_preflight(state: *const CpuState) -> bool {
    let Some(s) = (unsafe { state.as_ref() }) else { return false; };
    if !s.long_mode || !s.nx || !s.msr || !s.pae || !s.paging_enabled ||
        !s.pae_enabled || s.la57_enabled || !s.nxe_enabled ||
        !s.write_protect_enabled || !(36..=52).contains(&s.physical_bits) {
        return false;
    }
    let source = s.cr3 & PHYS;
    source != 0 && (source >> s.physical_bits) == 0
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Backend {
    allocate: Option<unsafe extern "C" fn(*mut u64, *mut c_void) -> bool>,
    release: Option<unsafe extern "C" fn(u64, *mut c_void) -> bool>,
    access: Option<unsafe extern "C" fn(u64, *mut c_void) -> *mut u64>,
    context: *mut c_void,
}
#[repr(C)]
pub struct CloneResult { root: u64, table_count: u32 }
fn valid_frame(frame: u64) -> bool {
    frame != 0 && frame & 4095 == 0 && frame & !PHYS == 0
}
fn canonical(va: u64) -> bool {
    if (va & (1 << 47)) != 0 { va >> 48 == 0xffff }
    else { va >> 48 == 0 }
}
fn child(e: u64, level: u8) -> bool {
    level > 1 && e & PRESENT != 0 && e & HUGE == 0
}
fn backend_valid(b: &Backend) -> bool {
    b.allocate.is_some() && b.release.is_some() && b.access.is_some()
}
unsafe fn at(b: &Backend, frame: u64) -> *mut u64 {
    if !valid_frame(frame) { return ptr::null_mut(); }
    unsafe { (b.access.unwrap())(frame, b.context) }
}
unsafe fn release(b: &Backend, frame: u64) -> bool {
    unsafe { (b.release.unwrap())(frame, b.context) }
}
unsafe fn table_clear(table: *mut u64) -> bool {
    if table.is_null() { return false; }
    for i in 0..TABLE_ENTRIES {
        unsafe { ptr::write_volatile(table.add(i), 0); }
    }
    true
}
unsafe fn discard_tree(b: &Backend, frame: u64, level: u8) -> bool {
    let t = unsafe { at(b, frame) };
    if t.is_null() { return false; }
    let mut ok = true;
    for i in 0..TABLE_ENTRIES {
        let entry = unsafe { ptr::read(t.add(i)) };
        if child(entry, level) {
            let next = entry & PHYS;
            if !valid_frame(next) ||
                !unsafe { discard_tree(b, next, level - 1) } {
                ok = false;
            }
        }
    }
    if !unsafe { release(b, frame) } { ok = false; }
    ok
}
unsafe fn clone_level(
    b: &Backend, source: u64, level: u8, allocated: &mut u32,
    kernel_only: bool,
) -> Option<u64> {
    if !valid_frame(source) || !(1..=4).contains(&level) ||
        *allocated >= CLONE_LIMIT { return None; }
    let src = unsafe { at(b, source) };
    if src.is_null() { return None; }
    let mut phys = 0;
    if !unsafe { (b.allocate.unwrap())(&mut phys, b.context) } {
        return None;
    }
    if !valid_frame(phys) || phys == source {
        if valid_frame(phys) && phys != source {
            let _ = unsafe { release(b, phys) };
        }
        return None;
    }
    let dst = unsafe { at(b, phys) };
    if !unsafe { table_clear(dst) } {
        let _ = unsafe { release(b, phys) };
        return None;
    }
    *allocated += 1;
    for i in 0..TABLE_ENTRIES {
        if kernel_only && level == 4 && i < 256 { continue; }
        let entry = unsafe { ptr::read(src.add(i)) };
        if level == 4 && entry & (PRESENT | HUGE) == (PRESENT | HUGE) {
            let _ = unsafe { discard_tree(b, phys, level) };
            return None;
        }
        let mut result = if child(entry, level) {
            let Some(child_phys) = (unsafe {
                clone_level(b, entry & PHYS, level - 1, allocated, kernel_only)
            }) else {
                let _ = unsafe { discard_tree(b, phys, level) };
                return None;
            };
            (entry & !PHYS) | child_phys
        } else {
            entry
        };
        if kernel_only && level == 4 { result &= !4u64; }
        unsafe { ptr::write_volatile(dst.add(i), result); }
    }
    Some(phys)
}
unsafe fn clone_root(
    backend: *const Backend, original: u64, result: *mut CloneResult,
    kernel_only: bool,
) -> bool {
    let Some(b) = (unsafe { backend.as_ref() }) else { return false; };
    let Some(result) = (unsafe { result.as_mut() }) else { return false; };
    if !backend_valid(b) || !valid_frame(original) || result.root != 0 {
        return false;
    }
    let mut count = 0u32;
    let Some(root) = (unsafe { clone_level(b, original, 4, &mut count, kernel_only) })
    else { return false; };
    result.root = root;
    result.table_count = count;
    true
}
/// # Safety
/// Caller supplies non-overlapping valid C backend/result pointers and holds
/// all source frames live. No concurrent page-table access is permitted.
#[no_mangle]
pub unsafe extern "C" fn paging_clone_root(
    backend: *const Backend, source: u64, result: *mut CloneResult,
) -> bool {
    unsafe { clone_root(backend, source, result, false) }
}
/// # Safety
/// As paging_clone_root, but only upper canonical PML4 entries are copied
/// and all their PML4 U/S flags are forced supervisor-only.
#[no_mangle]
pub unsafe extern "C" fn paging_clone_kernel_half(
    backend: *const Backend, source: u64, result: *mut CloneResult,
) -> bool {
    unsafe { clone_root(backend, source, result, true) }
}
unsafe fn graft(
    backend: *const Backend, kernel_root: u64, user_root: u64, attach: bool,
) -> bool {
    let Some(b) = (unsafe { backend.as_ref() }) else { return false; };
    if !backend_valid(b) || !valid_frame(kernel_root) ||
        !valid_frame(user_root) || kernel_root == user_root { return false; }
    let kernel = unsafe { at(b, kernel_root) };
    let user = unsafe { at(b, user_root) };
    if kernel.is_null() || user.is_null() { return false; }
    // Validate ALL 256 entries before mutating a single child.
    for i in 0..256 {
        let e = unsafe { ptr::read(user.add(i)) };
        let k = unsafe { ptr::read(kernel.add(i)) };
        if attach {
            if k != 0 || (e != 0 && ((e & 5) != 5 || e & HUGE != 0)) {
                return false;
            }
        } else if k & !ACCESSED != e & !ACCESSED {
            // CPU-set Accessed bit is harmless; different frames or U/S not.
            return false;
        }
    }
    for i in 0..256 {
        let value = if attach { unsafe { ptr::read(user.add(i)) } } else { 0 };
        unsafe { ptr::write_volatile(kernel.add(i), value); }
    }
    true
}
/// # Safety
/// Caller owns an INACTIVE supervisor clone and a separate DETACHED user
/// root. User children remain owned by the original user-image VMM.
#[no_mangle]
pub unsafe extern "C" fn paging_attach_user_half(
    backend: *const Backend, kernel_root: u64, user_root: u64,
) -> bool {
    unsafe { graft(backend, kernel_root, user_root, true) }
}
/// # Safety
/// Detach after kernel CR3 is restored, before freeing either page table.
#[no_mangle]
pub unsafe extern "C" fn paging_detach_user_half(
    backend: *const Backend, kernel_root: u64, user_root: u64,
) -> bool {
    unsafe { graft(backend, kernel_root, user_root, false) }
}
/// # Safety
/// Result refers to owned, inactive root. Borrowed user children MUST be
/// detached first. This recurses only up to 4 page-table levels.
#[no_mangle]
pub unsafe extern "C" fn paging_clone_release(
    backend: *const Backend, result: *mut CloneResult,
) -> bool {
    let Some(b) = (unsafe { backend.as_ref() }) else { return false; };
    let Some(r) = (unsafe { result.as_mut() }) else { return false; };
    if !backend_valid(b) || !valid_frame(r.root) { return false; }
    if !unsafe { discard_tree(b, r.root, 4) } { return false; }
    r.root = 0;
    r.table_count = 0;
    true
}
/// # Safety
/// Callback table pointers remain readable; \`physical\` and \`flags\` are
/// writable outputs, modified only when a complete translation succeeds.
#[no_mangle]
pub unsafe extern "C" fn paging_lookup(
    backend: *const Backend, root: u64, va: u64,
    physical: *mut u64, flags: *mut u64,
) -> bool {
    let Some(b) = (unsafe { backend.as_ref() }) else { return false; };
    if !backend_valid(b) || !valid_frame(root) || !canonical(va) ||
        physical.is_null() || flags.is_null() { return false; }
    let mut frame = root;
    for level in (1u8..=4).rev() {
        let table = unsafe { at(b, frame) };
        if table.is_null() { return false; }
        let shift = 12 + (level as u64 - 1) * 9;
        let index = ((va >> shift) & 511) as usize;
        let entry = unsafe { ptr::read(table.add(index)) };
        if entry & PRESENT == 0 || (level == 4 && entry & HUGE != 0) {
            return false;
        }
        if level == 1 || (entry & HUGE != 0 && level <= 3) {
            let size = 1u64 << shift;
            let base = (entry & PHYS) & !(size - 1);
            unsafe {
                ptr::write(physical, base | (va & (size - 1)));
                ptr::write(flags, entry & !PHYS);
            }
            return true;
        }
        frame = entry & PHYS;
        if !valid_frame(frame) { return false; }
    }
    false
}
#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{size_of, offset_of};
    #[test]
    fn compatible_layout_and_preflight() {
        assert_eq!(size_of::<CpuState>(), 24);
        assert_eq!(offset_of!(CpuState, cr3), 16);
        assert_eq!(size_of::<Backend>(), 32);
        assert_eq!(size_of::<CloneResult>(), 16);
        assert!(canonical(0xffff800000000000));
        assert!(!canonical(0x0000800000000000));
        assert!(valid_frame(0x1000));
        assert!(!valid_frame(0x1234));
    }
}
