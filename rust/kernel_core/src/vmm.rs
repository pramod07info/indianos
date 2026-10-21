//! Detached, four-level x86-64 page-table builder (IndianOS R003).
//!
//! This is not a CR3 switcher or a TLB manager. The C backend owns physical
//! table frames and provides a 4-KiB supervisor mapping for each allocated
//! frame. Mapping ownership and leaf-data-frame lifetime remain with caller.
//! Functions are exported only with Cargo feature "vmm" to permit an exact
//! independent comparison with the historical C implementation.

use core::{ffi::c_void, ptr};

const PAGE: u64 = 4096;
const ADDRESS: u64 = 0x000f_ffff_ffff_f000;
const PRESENT: u64 = 1;
const WRITE: u64 = 2;
const USER: u64 = 4;
const HUGE: u64 = 0x80;
const NX: u64 = 1u64 << 63;
const ENTRIES: usize = 512;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Backend {
    pub allocate: Option<unsafe extern "C" fn(*mut u64, *mut c_void) -> bool>,
    pub release: Option<unsafe extern "C" fn(u64, *mut c_void) -> bool>,
    pub access: Option<unsafe extern "C" fn(u64, *mut c_void) -> *mut c_void>,
    pub context: *mut c_void,
}
#[repr(C)]
pub struct Space {
    pub root_physical: u64,
    pub backend: Backend,
    pub initialized: bool,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Permissions {
    pub writable: bool,
    pub executable: bool,
    pub user: bool,
}
#[repr(C)]
pub struct Mapping {
    pub physical: u64,
    pub permissions: Permissions,
}

fn physical_page(p: u64) -> bool {
    p != 0 && p & (PAGE - 1) == 0 && p & !ADDRESS == 0
}
fn canonical(va: u64) -> bool {
    if (va & (1 << 47)) != 0 { va >> 48 == 0xffff }
    else { va >> 48 == 0 }
}
fn valid_va(va: u64) -> bool { canonical(va) && va & (PAGE - 1) == 0 }
fn index(va: u64, level: usize) -> usize {
    ((va >> (39 - (level * 9))) & 0x1ff) as usize
}
fn ready(s: &Space) -> bool {
    s.initialized && physical_page(s.root_physical) &&
    s.backend.allocate.is_some() && s.backend.release.is_some() &&
    s.backend.access.is_some()
}
unsafe fn at(s: &Space, phys: u64) -> *mut u64 {
    if !physical_page(phys) { return ptr::null_mut(); }
    // SAFETY: callback is supplied by trusted kernel / host-test backend.
    unsafe { (s.backend.access.unwrap())(phys, s.backend.context).cast::<u64>() }
}
unsafe fn entry(table: *mut u64, i: usize) -> u64 {
    // SAFETY: the backend contract guarantees 512 u64s per table frame.
    unsafe { ptr::read(table.add(i)) }
}
unsafe fn set_entry(table: *mut u64, i: usize, value: u64) {
    unsafe { ptr::write_volatile(table.add(i), value); }
}
unsafe fn zero_table(table: *mut u64) -> bool {
    if table.is_null() { return false; }
    for i in 0..ENTRIES { unsafe { set_entry(table, i, 0); } }
    true
}
unsafe fn empty_table(table: *mut u64) -> bool {
    if table.is_null() { return false; }
    for i in 0..ENTRIES {
        if unsafe { entry(table, i) } != 0 { return false; }
    }
    true
}
unsafe fn release(s: &Space, phys: u64) -> bool {
    unsafe { (s.backend.release.unwrap())(phys, s.backend.context) }
}
unsafe fn new_table(s: &Space) -> Option<(u64, *mut u64)> {
    let mut physical = 0;
    if !unsafe { (s.backend.allocate.unwrap())(&mut physical, s.backend.context) } {
        return None;
    }
    // Match the C original: do not hand a malformed allocator address back
    // to a release callback that might itself be unsafe for invalid frames.
    if !physical_page(physical) { return None; }
    let table = unsafe { at(s, physical) };
    if !unsafe { zero_table(table) } {
        let _ = unsafe { release(s, physical) };
        return None;
    }
    Some((physical, table))
}

/// # Safety
/// The C caller owns a valid writable Space (or NULL) and a readable
/// Backend struct (or NULL). Callback pointers follow their declared ABI.
#[no_mangle]
pub unsafe extern "C" fn vmm_create(space: *mut Space, backend: *const Backend) -> bool {
    if space.is_null() || backend.is_null() { return false; }
    let s = unsafe { &mut *space };
    let b = unsafe { &*backend };
    if s.initialized || b.allocate.is_none() || b.release.is_none() ||
        b.access.is_none() { return false; }
    s.backend = *b;
    s.root_physical = 0;
    match unsafe { new_table(s) } {
        Some((phys, _)) => {
            s.root_physical = phys;
            s.initialized = true;
            true
        }
        None => { s.initialized = false; false }
    }
}

unsafe fn undo(s: &Space, slots: &[*mut u64; 3],
               frames: &[u64; 3], count: usize) {
    for i in (0..count).rev() {
        unsafe { ptr::write_volatile(slots[i], 0); }
        let _ = unsafe { release(s, frames[i]) };
    }
}

/// # Safety
/// \`space\` is a valid exclusively writable Space, initialized via
/// vmm_create. Callbacks provide distinct mapped tables, not user pointers.
#[no_mangle]
pub unsafe extern "C" fn vmm_map_page(
    space: *mut Space, va: u64, pa: u64, perm: Permissions,
) -> bool {
    let Some(s) = (unsafe { space.as_mut() }) else { return false; };
    if !ready(s) || !valid_va(va) || !physical_page(pa) ||
        (perm.writable && perm.executable) ||
        (perm.user && va & (1u64 << 47) != 0) ||
        (!perm.user && va & (1u64 << 47) == 0) {
        return false;
    }
    let mut next = s.root_physical;
    let mut slots: [*mut u64; 3] = [ptr::null_mut(); 3];
    let mut frames: [u64; 3] = [0; 3];
    let mut count = 0usize;
    for level in 0..3 {
        let table = unsafe { at(s, next) };
        if table.is_null() { unsafe { undo(s, &slots, &frames, count); } return false; }
        let slot = unsafe { table.add(index(va, level)) };
        let value = unsafe { ptr::read(slot) };
        if value & PRESENT != 0 {
            if value & HUGE != 0 ||
                ((value & USER != 0) != perm.user) ||
                (value & WRITE == 0) ||
                !physical_page(value & ADDRESS) {
                unsafe { undo(s, &slots, &frames, count); }
                return false;
            }
            next = value & ADDRESS;
            continue;
        }
        if value != 0 {
            unsafe { undo(s, &slots, &frames, count); }
            return false;
        }
        let Some((child, _)) = (unsafe { new_table(s) }) else {
            unsafe { undo(s, &slots, &frames, count); }
            return false;
        };
        unsafe {
            ptr::write_volatile(slot, child | PRESENT | WRITE |
                if perm.user { USER } else { 0 });
        }
        slots[count] = slot;
        frames[count] = child;
        count += 1;
        next = child;
    }
    let leaf = unsafe { at(s, next) };
    if leaf.is_null() {
        unsafe { undo(s, &slots, &frames, count); }
        return false;
    }
    let leafslot = unsafe { leaf.add(index(va, 3)) };
    if unsafe { ptr::read(leafslot) } != 0 {
        unsafe { undo(s, &slots, &frames, count); }
        return false;
    }
    unsafe { ptr::write_volatile(leafslot, pa | PRESENT |
        if perm.writable { WRITE } else { 0 } |
        if perm.user { USER } else { 0 } |
        if perm.executable { 0 } else { NX }); }
    true
}

/// # Safety
/// \`space\` is readable initialized kernel metadata. \`mapping\` is a
/// writable Mapping or null. It remains unchanged if translation fails.
#[no_mangle]
pub unsafe extern "C" fn vmm_query_page(
    space: *const Space, va: u64, mapping: *mut Mapping,
) -> bool {
    let Some(s) = (unsafe { space.as_ref() }) else { return false; };
    if mapping.is_null() || !ready(s) || !valid_va(va) { return false; }
    let mut next = s.root_physical;
    // Effective permissions are the intersection of the entire page-table
    // path. A supervisor-only/read-only/NX ancestor constrains its leaf.
    let (mut can_write, mut can_execute, mut can_user) = (true, true, true);
    for level in 0..4 {
        let table = unsafe { at(s, next) };
        if table.is_null() { return false; }
        let value = unsafe { entry(table, index(va, level)) };
        if value & PRESENT == 0 ||
            (level < 3 && value & HUGE != 0) ||
            !physical_page(value & ADDRESS) {
            return false;
        }
        can_write &= value & WRITE != 0;
        can_execute &= value & NX == 0;
        can_user &= value & USER != 0;
        if level == 3 {
            unsafe { ptr::write(mapping, Mapping {
                physical: value & ADDRESS,
                permissions: Permissions {
                    writable: can_write,
                    executable: can_execute,
                    user: can_user,
                },
            }); }
            return true;
        }
        next = value & ADDRESS;
    }
    false
}

/// # Safety
/// \`space\` is exclusively writable kernel metadata. The caller still owns
/// the unmapped leaf data frame; only empty page-table frames are released.
#[no_mangle]
pub unsafe extern "C" fn vmm_unmap_page(space: *mut Space, va: u64) -> bool {
    let Some(s) = (unsafe { space.as_mut() }) else { return false; };
    if !ready(s) || !valid_va(va) { return false; }
    let mut tables = [ptr::null_mut::<u64>(); 4];
    let mut frames = [0u64; 4];
    let mut indices = [0usize; 4];
    frames[0] = s.root_physical;
    for level in 0..4 {
        tables[level] = unsafe { at(s, frames[level]) };
        if tables[level].is_null() { return false; }
        indices[level] = index(va, level);
        let value = unsafe { entry(tables[level], indices[level]) };
        if value & PRESENT == 0 ||
            (level < 3 && value & HUGE != 0) ||
            !physical_page(value & ADDRESS) {
            return false;
        }
        if level < 3 { frames[level + 1] = value & ADDRESS; }
    }
    unsafe { set_entry(tables[3], indices[3], 0); }
    for level in (1..4).rev() {
        if !unsafe { empty_table(tables[level]) } { break; }
        unsafe { set_entry(tables[level - 1], indices[level - 1], 0); }
        if !unsafe { release(s, frames[level]) } { return false; }
    }
    true
}

/// # Safety
/// \`space\` is valid and exclusively owned; no leaf mappings may remain.
#[no_mangle]
pub unsafe extern "C" fn vmm_destroy_empty(space: *mut Space) -> bool {
    let Some(s) = (unsafe { space.as_mut() }) else { return false; };
    if !ready(s) { return false; }
    let root = unsafe { at(s, s.root_physical) };
    if !unsafe { empty_table(root) } ||
       !unsafe { release(s, s.root_physical) } { return false; }
    s.root_physical = 0;
    s.initialized = false;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{size_of, align_of, offset_of};
    #[test]
    fn c_abi_layout() {
        assert_eq!(size_of::<Backend>(), 32);
        assert_eq!(size_of::<Space>(), 48);
        assert_eq!(offset_of!(Space, initialized), 40);
        assert_eq!(size_of::<Permissions>(), 3);
        assert_eq!(size_of::<Mapping>(), 16);
        assert_eq!(align_of::<Space>(), 8);
        assert!(valid_va(0xffff_fe00_0000_0000));
        assert!(!valid_va(0x0000_8000_0000_0000));
        assert!(!physical_page(0));
        assert!(!physical_page(0x1234));
    }

    // An intentionally inconsistent ancestor cannot grant leaf privileges.
    struct MockTables { pages: [[u64;512];4] }
    unsafe extern "C" fn deny_alloc(_: *mut u64, _: *mut c_void) -> bool { false }
    unsafe extern "C" fn deny_free(_: u64, _: *mut c_void) -> bool { false }
    unsafe extern "C" fn mock_access(pa: u64, ctx: *mut c_void) -> *mut c_void {
        if pa < 0x1000 || pa > 0x4000 || pa & 4095 != 0 { return ptr::null_mut(); }
        let t = unsafe { &mut *(ctx as *mut MockTables) };
        t.pages[(pa as usize / 4096) - 1].as_mut_ptr().cast()
    }
    #[test]
    fn effective_permissions_respect_ancestors() {
        let mut t = MockTables { pages: [[0u64;512];4] };
        t.pages[0][0] = 0x2000 | 7;
        t.pages[1][0] = 0x3000 | 7;
        t.pages[2][2] = 0x4000 | 7;
        t.pages[3][0] = 0x5000 | 7 | NX;
        let space = Space {
            root_physical: 0x1000,
            backend: Backend {
                allocate: Some(deny_alloc), release: Some(deny_free),
                access: Some(mock_access),
                context: (&mut t as *mut MockTables).cast(),
            },
            initialized: true,
        };
        let mut output = Mapping {
            physical: 0,
            permissions: Permissions { writable: false, executable: true, user: false },
        };
        unsafe {
            assert!(vmm_query_page(&space, 0x400000, &mut output));
            assert!(output.permissions.user && output.permissions.writable);
            assert!(!output.permissions.executable);
            t.pages[0][0] &= !USER;
            assert!(vmm_query_page(&space, 0x400000, &mut output));
            assert!(!output.permissions.user);
            t.pages[0][0] |= USER;
            t.pages[1][0] &= !WRITE;
            assert!(vmm_query_page(&space, 0x400000, &mut output));
            assert!(!output.permissions.writable);
            t.pages[1][0] |= WRITE;
            t.pages[3][0] &= !NX;
            t.pages[2][2] |= NX;
            assert!(vmm_query_page(&space, 0x400000, &mut output));
            assert!(!output.permissions.executable);
        }
    }
}
