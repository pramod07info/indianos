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
        // A refused release retains ownership. Keep this empty table and
        // its ancestors reachable for a later map or destroy operation.
        if !unsafe { release(s, frames[i]) } { break; }
        unsafe { ptr::write_volatile(slots[i], 0); }
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
/// Once the leaf is removed this returns true, even if optional table
/// pruning is refused. Retained empty tables remain owned by the space.
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
        // The leaf is already unmapped. Pruning is optional: a refused
        // release must keep the empty table linked and owned for retry.
        if !unsafe { release(s, frames[level]) } { break; }
        unsafe { set_entry(tables[level - 1], indices[level - 1], 0); }
    }
    true
}

/// # Safety
/// \`space\` is valid and exclusively owned; no leaf mappings may remain.
#[no_mangle]
pub unsafe extern "C" fn vmm_destroy_empty(space: *mut Space) -> bool {
    let Some(s) = (unsafe { space.as_mut() }) else { return false; };
    if !ready(s) { return false; }
    if !unsafe { has_no_mappings(s, s.root_physical, 0) } ||
       !unsafe { reclaim_empty_children(s, s.root_physical, 0) } ||
       !unsafe { release(s, s.root_physical) } { return false; }
    s.root_physical = 0;
    s.initialized = false;
    true
}

// Check every branch before destroying anything: a refused cleanup may
// leave linked empty tables alongside still-live mappings in other branches.
unsafe fn has_no_mappings(s: &Space, physical: u64, level: usize) -> bool {
    let table = unsafe { at(s, physical) };
    if table.is_null() { return false; }
    for i in 0..ENTRIES {
        let value = unsafe { entry(table, i) };
        if value == 0 { continue; }
        if level == 3 || value & PRESENT == 0 || value & HUGE != 0 ||
            !physical_page(value & ADDRESS) ||
            !unsafe { has_no_mappings(s, value & ADDRESS, level + 1) } {
            return false;
        }
    }
    true
}

// Successful partial reclamation is safe: accepted releases are unlinked,
// while refused releases remain linked and may be retried by the caller.
unsafe fn reclaim_empty_children(s: &Space, physical: u64, level: usize) -> bool {
    let table = unsafe { at(s, physical) };
    if table.is_null() { return false; }
    if level == 3 { return unsafe { empty_table(table) }; }
    for i in 0..ENTRIES {
        let value = unsafe { entry(table, i) };
        if value == 0 { continue; }
        let child = value & ADDRESS;
        if !unsafe { reclaim_empty_children(s, child, level + 1) } ||
            !unsafe { release(s, child) } { return false; }
        unsafe { set_entry(table, i, 0); }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{size_of, align_of, offset_of};
    struct Pool {
        pages: [[u64;512];16], busy:[bool;16],
        allocation_budget:Option<usize>, refuse_release:u64,
        released:Vec<u64>,
    }
    impl Pool {
        fn new()->Self{Self{pages:[[0;512];16],busy:[false;16],
            allocation_budget:None,refuse_release:0,released:Vec::new()}}
        fn backend(&mut self)->Backend{Backend{allocate:Some(pool_allocate),
            release:Some(pool_release),access:Some(pool_access),
            context:(self as *mut Self).cast()}}
        fn live(&self)->usize{self.busy.iter().filter(|&&b|b).count()}
    }
    unsafe extern "C" fn pool_allocate(out:*mut u64,ctx:*mut c_void)->bool{
        let pool=unsafe{&mut *ctx.cast::<Pool>()};
        if pool.allocation_budget==Some(0){return false;}
        let Some(slot)=pool.busy.iter().position(|&b|!b)else{return false;};
        if let Some(budget)=pool.allocation_budget.as_mut(){*budget-=1;}
        pool.busy[slot]=true;pool.pages[slot].fill(0xaaaa_aaaa_aaaa_aaaa);
        unsafe{out.write((slot as u64+1)*PAGE);}
        true
    }
    unsafe extern "C" fn pool_release(pa:u64,ctx:*mut c_void)->bool{
        let pool=unsafe{&mut *ctx.cast::<Pool>()};
        if pa==pool.refuse_release||pa<PAGE||pa>16*PAGE||pa%PAGE!=0{return false;}
        let slot=pa as usize/PAGE as usize-1;
        if !pool.busy[slot]{panic!("duplicate or unowned release {pa:#x}");}
        pool.busy[slot]=false;pool.released.push(pa);true
    }
    unsafe extern "C" fn pool_access(pa:u64,ctx:*mut c_void)->*mut c_void{
        let pool=unsafe{&mut *ctx.cast::<Pool>()};
        if pa<PAGE||pa>16*PAGE||pa%PAGE!=0{return ptr::null_mut();}
        let slot=pa as usize/PAGE as usize-1;
        if !pool.busy[slot]{return ptr::null_mut();}
        pool.pages[slot].as_mut_ptr().cast()
    }
    fn create(pool:&mut Pool)->Space{
        let mut s=Space{root_physical:0,backend:pool.backend(),initialized:false};
        assert!(unsafe{vmm_create(&mut s,&pool.backend())});s
    }
    fn table_frames(pool:&Pool,pa:u64,level:usize)->usize{
        let mut count=1;
        assert!(pool.busy[(pa/PAGE-1) as usize]);
        if level<3{for &e in &pool.pages[(pa/PAGE-1) as usize]{
            if e&PRESENT!=0{count+=table_frames(pool,e&ADDRESS,level+1);}
        }}
        count
    }
    const DATA:Permissions=Permissions{writable:true,executable:false,user:true};
    #[test]fn unmap_refused_table_release_retains_reachable_ownership(){
        for frame in [0x4000,0x3000,0x2000]{
            let mut pool=Pool::new();let mut s=create(&mut pool);
            assert!(unsafe{vmm_map_page(&mut s,0x400000,0x1000000,DATA)});
            pool.refuse_release=frame;
            assert!(unsafe{vmm_unmap_page(&mut s,0x400000)});
            let mut out=Mapping{physical:123,permissions:DATA};
            assert!(!unsafe{vmm_query_page(&s,0x400000,&mut out)});
            assert_eq!(out.physical,123);
            assert_eq!(table_frames(&pool,s.root_physical,0),pool.live());
            assert_eq!(pool.live(),frame as usize/PAGE as usize);
            pool.refuse_release=0;
            assert!(unsafe{vmm_destroy_empty(&mut s)});
            assert_eq!(pool.live(),0);assert!(!s.initialized);
            assert!(!pool.released.contains(&0x1000000));
        }
    }
    #[test]fn failed_mapping_rollback_keeps_refused_tables_available_for_destroy(){
        for budget in [1usize,2]{
            let mut pool=Pool::new();let mut s=create(&mut pool);
            pool.allocation_budget=Some(budget);
            pool.refuse_release=(budget as u64+1)*PAGE;
            assert!(!unsafe{vmm_map_page(&mut s,0x400000,0x1000000,DATA)});
            assert_eq!(pool.live(),budget+1);
            assert_eq!(table_frames(&pool,s.root_physical,0),pool.live());
            pool.refuse_release=0;
            assert!(unsafe{vmm_destroy_empty(&mut s)});assert_eq!(pool.live(),0);
        }
    }
    #[test]fn destroy_preflight_preserves_live_sibling_and_retries_partial_cleanup(){
        let mut pool=Pool::new();let mut s=create(&mut pool);
        assert!(unsafe{vmm_map_page(&mut s,0x400000,0x1000000,DATA)});
        assert!(unsafe{vmm_map_page(&mut s,1u64<<39,0x1001000,DATA)});
        pool.refuse_release=0x4000;
        assert!(unsafe{vmm_unmap_page(&mut s,0x400000)});
        let before=pool.live();let released=pool.released.len();
        assert!(!unsafe{vmm_destroy_empty(&mut s)});
        assert_eq!(pool.live(),before);assert_eq!(pool.released.len(),released);
        pool.refuse_release=0x7000;
        assert!(unsafe{vmm_unmap_page(&mut s,1u64<<39)});
        // First branch can be reclaimed, second remains owned on failure.
        assert!(!unsafe{vmm_destroy_empty(&mut s)});
        assert_eq!(table_frames(&pool,s.root_physical,0),pool.live());
        assert_eq!(pool.live(),4);
        pool.refuse_release=0x1000; // root refusal is also retryable
        assert!(!unsafe{vmm_destroy_empty(&mut s)});
        assert_eq!(pool.live(),1);assert!(s.initialized);
        pool.refuse_release=0;
        assert!(unsafe{vmm_destroy_empty(&mut s)});assert_eq!(pool.live(),0);
    }
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
