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
#[derive(Default)]
pub(crate) struct CpuState {
    pub(crate) long_mode: bool, pub(crate) nx: bool, pub(crate) msr: bool, pub(crate) pae: bool,
    pub(crate) paging_enabled: bool, pub(crate) pae_enabled: bool, pub(crate) la57_enabled: bool,
    pub(crate) nxe_enabled: bool, pub(crate) write_protect_enabled: bool, pub(crate) pcid_enabled: bool,
    pub(crate) physical_bits: u8, pub(crate) cr3: u64,
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
    pub(crate) allocate: Option<unsafe extern "C" fn(*mut u64, *mut c_void) -> bool>,
    pub(crate) release: Option<unsafe extern "C" fn(u64, *mut c_void) -> bool>,
    pub(crate) access: Option<unsafe extern "C" fn(u64, *mut c_void) -> *mut u64>,
    pub(crate) context: *mut c_void,
}
#[repr(C)]
pub struct CloneResult { pub(crate) root: u64, pub(crate) table_count: u32 }
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
            } else {
                // Detach only children whose entire owned subtree has been
                // released. A retry must never visit their freed frames.
                unsafe { ptr::write_volatile(t.add(i), 0); }
            }
        }
    }
    // Keep this table and all failed-child links live for the caller's next
    // serialized retry. Releasing a parent here would lose owned descendants
    // and leave CloneResult pointing at a freed root.
    if !ok { return false; }
    unsafe { release(b, frame) }
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

// Build the replacement for one huge leaf privately. Only after every table
// and the guard hole are ready is its parent entry published. The surrounding
// data frames remain borrowed and must never be passed to release().
#[derive(Debug,Default)]
pub(crate) struct GuardFailure{pub retained:[u64;2]}
impl GuardFailure{
    unsafe fn release_or_retain(&mut self,b:&Backend,frame:u64){
        if !unsafe{release(b,frame)}{
            // A split has at most two private tables (1GiB -> 2MiB -> 4KiB).
            let slot=if self.retained[0]==0{0}else{1};
            self.retained[slot]=frame;
        }
    }
}
unsafe fn split_guard_leaf(b:&Backend,entry:u64,level:u8,va:u64,
    forbidden:&[u64])->Result<u64,GuardFailure>{
    if !(2..=3).contains(&level)||entry&(PRESENT|HUGE)!=(PRESENT|HUGE){return Err(GuardFailure::default());}
    let size=1u64<<(12+9*(level as u32-1));
    // Bit12 is PAT in a huge leaf; the remaining low physical-address bits
    // are reserved. Reject malformed leaves instead of rounding them down.
    if entry&PHYS&(size-1)&!(1<<12)!=0{return Err(GuardFailure::default());}
    let mut physical=0;
    if !unsafe{(b.allocate.unwrap())(&mut physical,b.context)}{return Err(GuardFailure::default());}
    if !valid_frame(physical)||forbidden.contains(&physical){return Err(GuardFailure::default());}
    let table=unsafe{at(b,physical)};
    if !unsafe{table_clear(table)}{
        let mut failure=GuardFailure::default();
        unsafe{failure.release_or_retain(b,physical)};return Err(failure);
    }
    let base=entry&PHYS&!(size-1);
    let child_size=size/512;
    let mut flags=entry&!PHYS;
    if level==2{
        // Huge PDE PS(bit7) disappears; its PAT(bit12) moves to PTE bit7.
        flags&=!HUGE;
        if entry&(1<<12)!=0{flags|=1<<7;}
    }else{
        // Child PDEs remain huge and keep their huge-leaf PAT bit12.
        flags|=entry&(1<<12);
    }
    for i in 0..512{
        unsafe{ptr::write_volatile(table.add(i),base+i as u64*child_size|flags);}
    }
    let selected=((va>>(12+9*(level as u32-2)))&511) as usize;
    if level==2{
        unsafe{ptr::write_volatile(table.add(selected),0);}
    }else{
        let mut ancestors=[0u64;6];
        if forbidden.len()>=ancestors.len(){
            let mut failure=GuardFailure::default();
            unsafe{failure.release_or_retain(b,physical)};return Err(failure);
        }
        ancestors[..forbidden.len()].copy_from_slice(forbidden);
        ancestors[forbidden.len()]=physical;
        let child=unsafe{ptr::read(table.add(selected))};
        let replacement=match unsafe{split_guard_leaf(b,child,2,va,
            &ancestors[..forbidden.len()+1])}{
            Ok(replacement)=>replacement,
            Err(mut failure)=>{
                unsafe{failure.release_or_retain(b,physical)};return Err(failure);
            }
        };
        unsafe{ptr::write_volatile(table.add(selected),replacement);}
    }
    // Cache/PAT/global/dirty/protection-key leaf attributes stay in children;
    // these are table pointers, not data mappings. Preserve RW/U/NX limits.
    Ok(physical|PRESENT|(entry&(2|4|(1u64<<63))))
}

/// Remove one upper-half 4KiB guard mapping without reclaiming its borrowed
/// data frame. Huge leaves are split privately and published with one store;
/// allocation/access failures leave the original mapping intact. Newly
/// installed table frames belong permanently to this source root, so callers
/// must not include them in a transient process rollback baseline.
/// Failed releases are returned as retained frame ownership; callers must keep
/// that record live and refuse re-entry rather than forget or reuse the frames.
///
/// # Safety
/// The caller exclusively owns this root and its tables, supplies a backend
/// that allocates fresh non-aliasing frames, and serializes all access. For an
/// active root, mask interrupts on the BSP and invalidate the guard VA's TLB
/// translation before using the root or cloning it. No SMP shootdown occurs.
pub(crate) unsafe fn guard_kernel_page(b:&Backend,root:u64,va:u64)->Result<(),GuardFailure>{
    if !backend_valid(b)||!valid_frame(root)||!canonical(va)||
       va&(1<<47)==0||va&4095!=0{return Err(GuardFailure::default());}
    let mut frame=root;
    let mut ancestors=[0u64;4];
    for level in (1u8..=4).rev(){
        ancestors[(4-level) as usize]=frame;
        let table=unsafe{at(b,frame)};
        if table.is_null(){return Err(GuardFailure::default());}
        let index=((va>>(12+9*(level as u32-1)))&511) as usize;
        let entry=unsafe{ptr::read(table.add(index))};
        if entry&PRESENT==0{
            return if entry==0{Ok(())}else{Err(GuardFailure::default())};
        }
        if level==1{
            unsafe{ptr::write_volatile(table.add(index),0);}
            return Ok(());
        }
        if entry&HUGE!=0{
            let replacement=unsafe{split_guard_leaf(b,entry,level,va,
                &ancestors[..(5-level) as usize])}?;
            unsafe{ptr::write_volatile(table.add(index),replacement);}
            return Ok(());
        }
        frame=entry&PHYS;
        if !valid_frame(frame){return Err(GuardFailure::default());}
    }
    Err(GuardFailure::default())
}

/// Effective hardware permissions for one kernel stack page. Unlike the
/// historical paging_lookup ABI (which returns leaf attributes), RW and U/S
/// are ANDed across the entire walk and NX is ORed across it. The caller has
/// already established CR0.WP/NXE in the CPU paging preflight.
///
/// # Safety
/// The root and its readable backend remain live and are exclusively held or
/// otherwise protected against modification for the duration of this query.
pub(crate) unsafe fn kernel_stack_page_flags(b:&Backend,root:u64,va:u64)->Option<u64>{
    if !backend_valid(b)||!valid_frame(root)||!canonical(va)||
       va&(1<<47)==0||va&4095!=0{return None;}
    let mut frame=root;
    let(mut writable,mut user,mut nx)=(true,true,false);
    for level in (1u8..=4).rev(){
        let table=unsafe{at(b,frame)};
        if table.is_null(){return None;}
        let shift=12+9*(level as u32-1);
        let entry=unsafe{ptr::read(table.add(((va>>shift)&511) as usize))};
        if entry&PRESENT==0||(level==4&&entry&HUGE!=0){return None;}
        writable&=entry&2!=0;
        user&=entry&4!=0;
        nx|=entry&(1u64<<63)!=0;
        if level==1||entry&HUGE!=0{
            if level>1&&entry&PHYS&((1u64<<shift)-1)&!(1<<12)!=0{return None;}
            return Some(PRESENT|if writable{2}else{0}|if user{4}else{0}|
                if nx{1u64<<63}else{0});
        }
        frame=entry&PHYS;
        if !valid_frame(frame){return None;}
    }
    None
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
/// detached first. This recurses only up to 4 page-table levels. On failure,
/// result retains the still-owned root and unfreed descendants for retry;
/// successfully released child links are cleared before returning. The table
/// count remains the original allocation count until complete release.
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

#[cfg(test)]
mod kernel_guard_tests{
 use super::*;
 const VA:u64=0xffff800000205000;
 const NX:u64=1<<63;
 struct Memory{
  tables:[[u64;512];12],live:[bool;12],allocations:usize,releases:usize,
  fail_allocation:usize,deny_access:u64,fail_release:bool,
 }
 impl Memory{
  fn huge(level:u8,flags:u64)->Self{
   let mut m=Self{tables:[[0;512];12],live:[false;12],allocations:0,
    releases:0,fail_allocation:usize::MAX,deny_access:0,fail_release:false};
   m.live[0]=true;m.live[1]=true;
   m.tables[0][256]=0x2000|3;
   if level==3{m.tables[1][0]=0x40000000|flags|PRESENT|HUGE;}
   else{
    m.live[2]=true;m.tables[1][0]=0x3000|3;
    m.tables[2][1]=0x600000|flags|PRESENT|HUGE;
   }
   m
  }
  fn backend(&mut self)->Backend{
   Backend{allocate:Some(allocate),release:Some(free),access:Some(access),
    context:(self as *mut Self).cast()}
  }
 }
 unsafe extern "C" fn allocate(out:*mut u64,context:*mut c_void)->bool{
  let m=unsafe{&mut*(context as *mut Memory)};
  m.allocations+=1;
  if m.allocations==m.fail_allocation{return false;}
  let Some(slot)=m.live.iter().position(|&live|!live)else{return false;};
  m.live[slot]=true;
  unsafe{ptr::write(out,(slot as u64+1)*4096)};true
 }
 unsafe extern "C" fn free(frame:u64,context:*mut c_void)->bool{
  let m=unsafe{&mut*(context as *mut Memory)};
  let index=(frame/4096-1) as usize;
  if m.fail_release||!m.live[index]{return false;}
  m.live[index]=false;m.releases+=1;true
 }
 unsafe extern "C" fn access(frame:u64,context:*mut c_void)->*mut u64{
  let m=unsafe{&mut*(context as *mut Memory)};
  if frame==m.deny_access||!valid_frame(frame){return ptr::null_mut();}
  let i=(frame/4096-1) as usize;
  if i>=m.live.len()||!m.live[i]{return ptr::null_mut();}
  m.tables[i].as_mut_ptr()
 }
 fn translated(b:&Backend,va:u64)->Option<(u64,u64)>{
  let(mut physical,mut flags)=(0,0);
  unsafe{paging_lookup(b,0x1000,va,&mut physical,&mut flags)}
    .then_some((physical,flags))
 }
 #[test]fn kernel_stack_effective_rw_rejects_readonly_pml4_pdpt_pd_and_pt(){
  for (table,index) in [(0,256),(1,0),(2,1),(3,5)]{
   let mut m=Memory::huge(2,2|NX);
   m.tables[2][1]=0x4000|3;m.live[3]=true;
   m.tables[3][5]=0x900000|3|NX;
   m.tables[table][index]&=!2;
   let b=m.backend();
   let flags=unsafe{kernel_stack_page_flags(&b,0x1000,VA)}.unwrap();
   assert_eq!(flags&3,1,"readonly level table {table}");
   assert_eq!(flags&NX,NX);
   // The previous leaf-only query is intentionally unchanged, and would
   // report a writable leaf under any of the three readonly ancestors.
   if table!=3{assert_eq!(translated(&b,VA).unwrap().1&2,2);}
   assert_eq!(m.allocations,0);assert_eq!(m.releases,0);
  }
 }
 #[test]fn kernel_stack_effective_rw_applies_to_two_megabyte_and_one_gigabyte_leaves(){
  for level in [2,3]{
   let leaf=if level==3{(1,0)}else{(2,1)};
   let paths=if level==3{&[(0,256),(1,0)][..]}else{&[(0,256),(1,0),(2,1)][..]};
   for &(table,index) in paths{
    let mut m=Memory::huge(level,2|NX|(1<<12));
    let b=m.backend();
    assert_eq!(unsafe{kernel_stack_page_flags(&b,0x1000,VA)}.unwrap()&3,3);
    m.tables[table][index]&=!2;
    assert_eq!(unsafe{kernel_stack_page_flags(&b,0x1000,VA)}.unwrap()&3,1);
    if (table,index)!=leaf{assert_eq!(translated(&b,VA).unwrap().1&2,2);}
   }
  }
 }
 #[test]fn kernel_stack_effective_nx_and_supervisor_permissions_include_ancestors(){
  let mut m=Memory::huge(2,2|4);let b=m.backend();
  m.tables[0][256]|=NX;
  // Leaf NX is clear and its U/S flag is set. The supervisor PML4 and NX
  // ancestor still make this a nonexecutable supervisor mapping on hardware.
  assert_eq!(translated(&b,VA).unwrap().1&NX,0);
  assert_eq!(unsafe{kernel_stack_page_flags(&b,0x1000,VA)},Some(3|NX));
  m.tables[0][256]|=4;m.tables[1][0]|=4;
  assert_eq!(unsafe{kernel_stack_page_flags(&b,0x1000,VA)},Some(7|NX));
  m.tables[2][1]=2;
  assert!(unsafe{kernel_stack_page_flags(&b,0x1000,VA)}.is_none());
  m.tables[2][1]=0x602000|3|NX|HUGE;
  assert!(unsafe{kernel_stack_page_flags(&b,0x1000,VA)}.is_none());
  m.tables[0][256]|=HUGE;
  assert!(unsafe{kernel_stack_page_flags(&b,0x1000,VA)}.is_none());
 }
 #[test]fn kernel_guard_splits_two_megabyte_leaf_and_preserves_pat_and_neighbors(){
  for pat in [false,true]{
   let mut m=Memory::huge(2,2|NX|0x100|0x40|if pat{1<<12}else{0});
   let b=m.backend();let before=translated(&b,VA+4096).unwrap().0;
   assert!(unsafe{guard_kernel_page(&b,0x1000,VA)}.is_ok());
   assert!(translated(&b,VA).is_none());
   let (physical,flags)=translated(&b,VA+4096).unwrap();
   assert_eq!(physical,before);
   assert_eq!(flags&3,3);assert_eq!(flags&NX,NX);
   assert_eq!(flags&0x100,0x100);assert_eq!(flags&0x40,0x40);
   assert_eq!(flags&(1<<7),if pat{1<<7}else{0});
   assert_eq!(m.allocations,1);assert_eq!(m.releases,0);
   assert!(unsafe{guard_kernel_page(&b,0x1000,VA)}.is_ok());
   assert_eq!(m.allocations,1);
  }
 }
 #[test]fn kernel_guard_splits_one_gigabyte_leaf_without_changing_other_huge_ranges(){
  let mut m=Memory::huge(3,2|NX|(1<<12)|0x100);
  let b=m.backend();let distant=translated(&b,VA+0x200000).unwrap().0;
  let near=translated(&b,VA-4096).unwrap().0;
  assert!(unsafe{guard_kernel_page(&b,0x1000,VA)}.is_ok());
  assert!(translated(&b,VA).is_none());
  assert_eq!(translated(&b,VA-4096).unwrap().0,near);
  assert_eq!(translated(&b,VA+0x200000).unwrap().0,distant);
  let (physical,flags)=translated(&b,VA+4096).unwrap();
  assert_eq!(physical,0x40206000);assert_ne!(flags&(1<<7),0);
  assert_eq!(m.allocations,2);assert_eq!(m.releases,0);
 }
 #[test]fn kernel_guard_split_allocation_failure_never_publishes_partial_mapping(){
  for failed in [1,2]{
   let mut m=Memory::huge(3,2|NX|(1<<12));
   let original=m.tables[1][0];let live=m.live;
   m.fail_allocation=failed;
   let b=m.backend();let before=translated(&b,VA).unwrap();
   assert!(unsafe{guard_kernel_page(&b,0x1000,VA)}.is_err());
   assert_eq!(m.tables[1][0],original);assert_eq!(m.live,live);
   assert_eq!(translated(&b,VA),Some(before));
  }
 }
 #[test]fn kernel_guard_split_inaccessible_table_reclaims_private_allocation(){
  let mut m=Memory::huge(2,2|NX);m.deny_access=0x4000;
  let original=m.tables[2][1];let live=m.live;
  let b=m.backend();
  assert!(unsafe{guard_kernel_page(&b,0x1000,VA)}.is_err());
  assert_eq!(m.tables[2][1],original);assert_eq!(m.live,live);
  assert_eq!(m.releases,1);
 }
 #[test]fn kernel_guard_failed_cleanup_retains_both_private_frame_owners(){
  let mut m=Memory::huge(3,2|NX);m.deny_access=0x4000;m.fail_release=true;
  let original=m.tables[1][0];let b=m.backend();
  let failure=unsafe{guard_kernel_page(&b,0x1000,VA)}.unwrap_err();
  assert_eq!(failure.retained,[0x4000,0x3000]);
  assert_eq!(m.tables[1][0],original);
  assert!(m.live[2]&&m.live[3]);assert_eq!(m.releases,0);
  assert!(translated(&b,VA).is_some());
 }
 #[test]fn kernel_guard_rejects_low_unaligned_and_reserved_huge_entries_before_allocation(){
  let mut m=Memory::huge(2,2|NX);let b=m.backend();
  for va in [0x400000,VA+1,0x0000800000000000]{
   assert!(unsafe{guard_kernel_page(&b,0x1000,va)}.is_err());
  }
  m.tables[2][1]|=1<<13;
  assert!(unsafe{guard_kernel_page(&b,0x1000,VA)}.is_err());
  m.tables[2][1]=2; // non-present software state is not a reserved guard hole
  assert!(unsafe{guard_kernel_page(&b,0x1000,VA)}.is_err());
  m.tables[0][256]|=HUGE; // a PML4 entry cannot be a huge leaf
  assert!(unsafe{guard_kernel_page(&b,0x1000,VA)}.is_err());
  assert_eq!(m.allocations,0);
 }
 #[test]fn kernel_guard_removes_four_kib_leaf_without_allocating_or_freeing_payload(){
  let mut m=Memory::huge(2,2|NX);m.tables[2][1]=0x4000|3;
  m.live[3]=true;m.tables[3][5]=0x900000|3|NX;
  m.tables[3][6]=0x901000|3|NX;
  let b=m.backend();
  assert!(unsafe{guard_kernel_page(&b,0x1000,VA)}.is_ok());
  assert!(translated(&b,VA).is_none());
  assert_eq!(translated(&b,VA+4096).unwrap().0,0x901000);
  assert_eq!(m.allocations,0);assert_eq!(m.releases,0);
 }
}

#[cfg(test)]
mod release_retry_tests {
    use super::*;
    struct Memory {
        tables: [[u64; TABLE_ENTRIES]; 8],
        live: [bool; 8],
        released: [usize; 8],
        fail_frame: u64,
        deny_access: u64,
    }
    impl Memory {
        fn new() -> Self {
            let mut m = Self { tables: [[0; TABLE_ENTRIES]; 8],
                live: [true; 8], released: [0; 8], fail_frame: 0, deny_access: 0 };
            // Four levels, two siblings and borrowed huge/data leaf frames.
            m.tables[0][256] = 0x2000 | 3;
            m.tables[0][257] = 0x6000 | 3;
            m.tables[1][0] = 0x3000 | 3;
            m.tables[1][1] = 0x5000 | 3;
            m.tables[2][0] = 0x4000 | 3;
            m.tables[2][1] = 0x200000 | PRESENT | HUGE;
            m.tables[3][0] = 0x9000 | 3; // mapped payload is not owned here
            m.tables[5][0] = 0x7000 | 3;
            m.tables[6][0] = 0x8000 | 3;
            m
        }
        fn backend(&mut self) -> Backend {
            Backend { allocate: Some(allocate), release: Some(release_frame),
                access: Some(access), context: (self as *mut Self).cast() }
        }
    }
    unsafe extern "C" fn allocate(_: *mut u64, _: *mut c_void) -> bool { false }
    fn index(frame: u64) -> usize {
        assert!((0x1000..=0x8000).contains(&frame));
        assert_eq!(frame & 4095, 0);
        (frame / 4096 - 1) as usize
    }
    unsafe extern "C" fn access(frame: u64, context: *mut c_void) -> *mut u64 {
        let m = unsafe { &mut *context.cast::<Memory>() };
        let i = index(frame);
        assert!(m.live[i], "access to a freed table");
        if frame == m.deny_access { return ptr::null_mut(); }
        m.tables[i].as_mut_ptr()
    }
    unsafe extern "C" fn release_frame(frame: u64, context: *mut c_void) -> bool {
        let m = unsafe { &mut *context.cast::<Memory>() };
        let i = index(frame);
        assert!(m.live[i], "double release");
        if frame == m.fail_frame { return false; }
        m.live[i] = false;
        m.released[i] += 1;
        // Poison released tables: later traversal is forbidden even if memory
        // was not immediately reassigned by this synthetic frame allocator.
        m.tables[i].fill(u64::MAX);
        true
    }
    #[test]
    fn m1_clone_release_retries_each_level_without_freeing_live_ancestors() {
        for fail in 1..=8 {
            let mut m = Memory::new();
            m.fail_frame = fail * 4096;
            let backend = m.backend();
            let mut result = CloneResult { root: 0x1000, table_count: 8 };
            assert!(!unsafe { paging_clone_release(&backend, &mut result) });
            assert_eq!(result.root, 0x1000); assert_eq!(result.table_count, 8);
            assert!(m.live[0]); assert!(m.live[(fail - 1) as usize]);
            let releases = m.released;
            // Persistent refusal cannot double-free successful siblings.
            assert!(!unsafe { paging_clone_release(&backend, &mut result) });
            assert_eq!(m.released, releases);
            m.fail_frame = 0;
            assert!(unsafe { paging_clone_release(&backend, &mut result) });
            assert_eq!(result.root, 0); assert_eq!(result.table_count, 0);
            assert_eq!(m.released, [1; 8]); assert_eq!(m.live, [false; 8]);
            assert!(!unsafe { paging_clone_release(&backend, &mut result) });
            assert_eq!(m.released, [1; 8]);
        }
    }
    #[test]
    fn m1_clone_release_access_failure_retains_subtree_for_retry() {
        for denied in 1..=8 {
            let mut m = Memory::new();m.deny_access = denied * 4096;
            let backend = m.backend();
            let mut result = CloneResult { root: 0x1000, table_count: 8 };
            assert!(!unsafe { paging_clone_release(&backend, &mut result) });
            assert!(m.live[0]); assert!(m.live[(denied - 1) as usize]);
            m.deny_access = 0;
            assert!(unsafe { paging_clone_release(&backend, &mut result) });
            assert_eq!(m.released, [1; 8]);
        }
    }
    #[test]
    fn m1_clone_construction_allocation_failure_preserves_source_and_rolls_back() {
        struct Construction {
            source: [[u64; TABLE_ENTRIES]; 4],
            owned: [[u64; TABLE_ENTRIES]; 4],
            live: [bool; 4], next: usize, fail_at: usize,
        }
        unsafe extern "C" fn alloc(out:*mut u64,c:*mut c_void)->bool {
            let m=unsafe{&mut *c.cast::<Construction>()};
            if m.next==m.fail_at{return false;}
            let i=m.next;m.next+=1;m.live[i]=true;
            unsafe{*out=0x10000+i as u64*4096;}true
        }
        unsafe extern "C" fn access(p:u64,c:*mut c_void)->*mut u64 {
            let m=unsafe{&mut *c.cast::<Construction>()};
            if (0x1000..=0x4000).contains(&p){
                return m.source[(p/4096-1) as usize].as_mut_ptr();
            }
            let i=((p-0x10000)/4096) as usize;
            assert!(m.live[i]);m.owned[i].as_mut_ptr()
        }
        unsafe extern "C" fn free(p:u64,c:*mut c_void)->bool {
            assert!(p>=0x10000,"source frame must never be released");
            let m=unsafe{&mut *c.cast::<Construction>()};
            let i=((p-0x10000)/4096) as usize;
            assert!(m.live[i]);m.live[i]=false;true
        }
        for fail_at in 0..4 {
            let mut m=Construction{source:[[0;TABLE_ENTRIES];4],
                owned:[[0;TABLE_ENTRIES];4],live:[false;4],next:0,fail_at};
            m.source[0][256]=0x2000|3;m.source[1][0]=0x3000|3;
            m.source[2][0]=0x4000|3;m.source[3][0]=0x9000|3;
            let original=m.source;
            let b=Backend{allocate:Some(alloc),release:Some(free),access:Some(access),
                context:(&mut m as *mut Construction).cast()};
            let mut result=CloneResult{root:0,table_count:0};
            assert!(!unsafe{paging_clone_kernel_half(&b,0x1000,&mut result)});
            assert_eq!(result.root,0);assert_eq!(result.table_count,0);
            assert_eq!(m.live,[false;4]);assert_eq!(m.source,original);
        }
    }

}
