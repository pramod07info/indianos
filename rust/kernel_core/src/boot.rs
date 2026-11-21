//! Limine revision-6 boot *consumer* and framebuffer status driver.
//!
//! Rust-owned request markers and opaque response getters replace the C
//! boot protocol shim in the default build. Rust checks every boot response
//! before it is used. Limine still owns the response storage and module.
//! Call before changing the bootloader's memory map or reclaiming frames.

use core::{ffi::{c_char,c_void},ptr};
use crate::initramfs::Ramfile;
use crate::initramfs::indianos_initramfs_lookup;

#[repr(C)]
#[derive(Clone,Copy,Debug)]
pub struct Region { pub base:u64, pub length:u64, pub kind:u64 }
#[repr(C)]
struct MemoryMap { revision:u64, count:u64, entries:*const *const Region }
#[repr(C)]
struct Hhdm { revision:u64, offset:u64 }
#[repr(C)]
struct Modules { revision:u64, count:u64, modules:*const *const LimineFile }
#[repr(C)]
struct LimineFile { revision:u64, address:*const u8, size:u64 }
#[repr(C)]
struct Framebuffers { revision:u64, count:u64, framebuffers:*const *const Framebuffer }
#[repr(C)]
struct Framebuffer {
    address:*mut u8, width:u64, height:u64, pitch:u64, bpp:u16,
    model:u8, red_size:u8, red_shift:u8, green_size:u8,
    green_shift:u8, blue_size:u8, blue_shift:u8,
}
unsafe extern "C" {
    fn boot_limine_revision_supported() -> bool;
    fn boot_limine_memmap() -> *const MemoryMap;
    fn boot_limine_hhdm() -> *const Hhdm;
    fn boot_limine_modules() -> *const Modules;
    fn boot_limine_framebuffers() -> *const Framebuffers;
    fn serial_write(message:*const c_char);
}
fn log(s: &'static [u8]) { unsafe { serial_write(s.as_ptr().cast()) }; }
/// # Safety: pointer references a live, mapped Limine-owned response of T.
/// Alignment is checked *before* creating any Rust reference.
unsafe fn limine_response<'a,T>(pointer:*const T)->Option<&'a T>{
    if !limine_pointer_aligned(pointer){return None;}
    unsafe{pointer.as_ref()}
}
unsafe fn memory_map() -> Option<&'static MemoryMap> {
    unsafe { limine_response(boot_limine_memmap()) }
}
/// Reject misaligned Limine pointers before constructing Rust references
/// or issuing aligned volatile loads. Firmware must also guarantee validity.
fn limine_pointer_aligned<T>(p:*const T)->bool {
    // Validate the entire referenced descriptor, not only its start address:
    // a canonical/aligned pointer near the 48-bit boundary can still straddle
    // the noncanonical hole. Mapping/provenance remains Limine's obligation.
    buffer_span_ok(p as usize,core::mem::size_of::<T>() as u64,
                   core::mem::align_of::<T>())
}
/// Preflight every pointer-table slot before indexing Limine-owned arrays.
/// This only validates numeric bounds; firmware still owns real mappings.
fn limine_pointer_table_valid<T>(p:*const *const T,count:u64,max:u64)->bool {
    count!=0 && count<=max &&
    count.checked_mul(core::mem::size_of::<*const T>() as u64)
        .is_some_and(|bytes|buffer_span_ok(
            p as usize,bytes,core::mem::align_of::<*const T>()))
}
fn valid_region(r:&Region)->bool{
    r.length!=0 && r.base.checked_add(r.length).is_some()
}
/// A firmware-provided direct-map base must be a canonical high-half page
/// boundary and leave room for at least one mapped page without overflow.
/// This is a numeric guard, not proof of a valid mapped physical page.
fn hhdm_offset_sane(offset:u64)->bool{
    offset>=0xffff_8000_0000_0000 &&
    offset&0xfff==0 &&
    offset.checked_add(4096).is_some()
}
fn spans_overlap(a:&Region,b:&Region)->bool {
    // Numeric overflow has been rejected for both regions by callers.
    a.base < b.base+b.length && b.base < a.base+a.length
}
fn validate_ranges(regions:&[Region]) -> bool {
    let mut usable=false;
    for (i,r) in regions.iter().enumerate() {
        if !valid_region(r) {return false;}
        if r.kind==0 {
            usable=true;
            for (j,other) in regions.iter().enumerate() {
                if i!=j && (!valid_region(other)||spans_overlap(r,other)){return false;}
            }
        }
    }
    usable
}
/// Preflight the complete Limine memory map before exporting its count.
/// An invalid late entry must never let the PMM consume a partial map.
fn validated_map_count(map:&MemoryMap)->Option<u64>{
    if !limine_pointer_table_valid(map.entries,map.count,4096){return None;}
    let mut usable=false;
    for i in 0..map.count as usize{
        let item=unsafe{ptr::read_volatile(map.entries.add(i))};
        if !limine_pointer_aligned(item){return None;}
        let region=unsafe{item.as_ref()}?;
        if !valid_region(region){return None;}
        // Inspect actual Limine memory-map pointers. Any usable byte
        // overlapping another firmware/reserved region denies boot.
        if region.kind==0 {
            usable=true;
            for j in 0..map.count as usize {
                if i==j {continue;}
                let ptr=unsafe{ptr::read_volatile(map.entries.add(j))};
                if !limine_pointer_aligned(ptr){return None;}
                let other=unsafe{ptr.as_ref()}?;
                if !valid_region(other)||spans_overlap(region,other){return None;}
            }
        }
    }
    if usable{Some(map.count)}else{None}
}
/// Every usable physical span must fit within the chosen direct-map
/// high-half range. This is an arithmetic preflight, not proof of PTE mapping.
fn usable_ram_page_aligned(region:&Region)->bool {
    (region.base|region.length)&0xfff==0
}
fn hhdm_usable_region_valid(region:&Region,offset:u64)->bool {
    const MAX_PHYSICAL_EXCLUSIVE:u64=1u64<<52;
    // A firmware usable span may start at physical zero. HHDM validation
    // verifies representable mapping only; PMM initialization separately
    // excludes page zero from any allocatable physical frame.
    if region.kind!=0 || !valid_region(region) ||
       !usable_ram_page_aligned(region) ||
       region.base>=MAX_PHYSICAL_EXCLUSIVE ||
       region.base.checked_add(region.length)
           .is_none_or(|end|end>MAX_PHYSICAL_EXCLUSIVE) ||
       !hhdm_offset_sane(offset) {return false;}
    let Some(first)=offset.checked_add(region.base)else{return false;};
    let Some(last)=region.base.checked_add(region.length-1)
        .and_then(|p|offset.checked_add(p))else{return false;};
    first>=0xffff_8000_0000_0000 && last>=first
}
fn hhdm_covers_usable_map(map:&MemoryMap,offset:u64)->bool{
    if !hhdm_offset_sane(offset) ||
        validated_map_count(map).is_none(){return false;}
    for i in 0..map.count as usize{
        let region_pointer=unsafe{ptr::read_volatile(map.entries.add(i))};
        if !limine_pointer_aligned(region_pointer){return false;}
        let region=unsafe{&*region_pointer};
        if region.kind!=0{continue;}
        if !hhdm_usable_region_valid(region,offset){return false;}
    }
    true
}
/// # Safety
/// Called while Limine response pointers are valid (before bootloader
/// memory reclamation) and only after Limine has prepared the x86-64 kernel.
#[no_mangle]
pub unsafe extern "C" fn boot_validate() -> bool {
    if !unsafe { boot_limine_revision_supported() } {
        log(b"IndianOS: unsupported Limine base revision\n\0"); return false;
    }
    let Some(map) = (unsafe { memory_map() }) else {
        log(b"IndianOS: missing or oversized memory map\n\0"); return false;
    };
    if validated_map_count(map).is_none(){
        log(b"IndianOS: invalid or unusable memory map\n\0");return false;
    }
    // The allocator and ELF loader depend on this direct mapping. Refuse
    // boot *before* other initialization if the Limine handoff omitted it.
    let Some(hhdm)=(unsafe {limine_response(boot_limine_hhdm())}) else {
        log(b"IndianOS: missing HHDM handoff\n\0");return false;
    };
    if !hhdm_covers_usable_map(map,hhdm.offset){
        log(b"IndianOS: invalid HHDM mapping\n\0");return false;
    }
    true
}
#[no_mangle]
pub unsafe extern "C" fn boot_memory_region_count() -> u64 {
    let Some(m)=(unsafe { memory_map() }) else {return 0};
    validated_map_count(m).unwrap_or(0)
}
/// # Safety
/// Valid writable C region output or null. The context parameter is ignored.
/// The caller must not invalidate Limine's response while iterating.
#[no_mangle]
pub unsafe extern "C" fn boot_memory_region_at(index:u64,out:*mut Region,_context:*mut c_void)->bool {
    if out.is_null(){return false}
    let Some(m)=(unsafe {memory_map()}) else {return false};
    if validated_map_count(m).is_none() || index>=m.count {return false}
    let item=unsafe {ptr::read_volatile(m.entries.add(index as usize))};
    if !limine_pointer_aligned(item){return false;}
    let Some(region)=(unsafe {item.as_ref()}) else {return false};
    if !valid_region(region) {return false;}
    unsafe {ptr::write(out,*region);}
    true
}
/// # Safety
/// Output pointer is writable and Limine-owned HHDM response still valid.
#[no_mangle]
pub unsafe extern "C" fn boot_hhdm_offset(out:*mut u64)->bool {
    if out.is_null(){return false}
    let Some(h)=(unsafe {limine_response(boot_limine_hhdm())}) else {return false};
    let Some(map)=(unsafe{memory_map()})else{return false;};
    if !hhdm_covers_usable_map(map,h.offset){return false;}
    unsafe {ptr::write(out,h.offset);}
    true
}
/// # Safety
/// \`path\` is a NUL-terminated, readable supervisor pointer; outputs are
/// writable and do not alias. Returned bytes are borrowed from Limine module.
#[no_mangle]
pub unsafe extern "C" fn boot_initramfs_file(
    path:*const c_char,data:*mut *const u8,size:*mut usize
)->bool {
    if path.is_null() || data.is_null() || size.is_null() {return false}
    let Some(modules)=(unsafe {limine_response(boot_limine_modules())}) else {return false};
    if !limine_pointer_table_valid(modules.modules,modules.count,1){return false}
    let file=unsafe {ptr::read_volatile(modules.modules)};
    if !limine_pointer_aligned(file){return false;}
    let Some(file)=(unsafe {file.as_ref()}) else {return false};
    if !(16..=32*1024*1024).contains(&file.size) ||
       !buffer_span_ok(file.address as usize,file.size,1){return false}
    let mut found=Ramfile{bytes:ptr::null(),length:0};
    if !unsafe {indianos_initramfs_lookup(file.address,file.size as usize,path,&mut found)} {
        return false
    }
    // Only borrow file contents from the validated Limine module. A future
    // archive decoder regression must not return supervisor memory outside
    // that module through the ordinary initramfs API.
    if !borrowed_ramfile_span_ok(file.address as usize,file.size as usize,
        found.bytes,found.length as usize){return false;}
    unsafe {
        ptr::write(data,found.bytes);
        ptr::write(size,found.length as usize);
    }
    true
}
/// Numeric boundary check; the caller is still responsible for validated
/// mapped Limine module lifetime. Allows a zero-length file at archive end.
fn borrowed_ramfile_span_ok(module:usize,size:usize,
    bytes:*const u8,length:usize)->bool {
    if module==0 || size<16 || bytes.is_null() ||
       !buffer_span_ok(module,size as u64,1){return false;}
    let Some(limit)=module.checked_add(size)else{return false;};
    let start=bytes as usize;
    start>=module && start<=limit &&
       start.checked_add(length).is_some_and(|end|end<=limit)
}
fn mask(size:u8,shift:u8)->Option<u32>{
    if size==0 || size>8 || shift>=32 || (size as u32)+(shift as u32)>32 {return None}
    Some(((1u32<<size)-1)<<shift)
}
/// Validate arithmetic before creating a slice or using pointer offsets.
/// This does not establish mapping, allocation provenance, or exclusive access:
/// those remain requirements of the Limine boot caller. IndianOS uses four-level
/// x86-64 paging, so both ends must belong to the same 48-bit canonical half.
fn buffer_span_ok(start:usize,length:u64,alignment:usize)->bool {
    if start==0 || length==0 || !alignment.is_power_of_two() ||
       start&(alignment-1)!=0 || length>isize::MAX as u64 {
        return false
    }
    let Ok(length)=usize::try_from(length) else {return false};
    let Some(end)=start.checked_add(length) else {return false};
    let first_half=(start as u64)>>47;
    let last_half=((end-1) as u64)>>47;
    (first_half==0 || first_half==0x1ffff) && first_half==last_half
}
fn limine_framebuffer_table_valid(resp:&Framebuffers)->bool{
 if !limine_pointer_table_valid(resp.framebuffers,resp.count,4096){
  return false;
 }
 for i in 0..resp.count as usize{
  let entry=unsafe{ptr::read_volatile(resp.framebuffers.add(i))};
  if !limine_pointer_aligned(entry){return false;}
 }
 true
}
fn framebuffer_ok(f:&Framebuffer)->bool {
    if f.width==0 || f.height==0 || f.model!=1 || f.bpp!=32 ||
       f.pitch%4!=0 || f.width>u64::MAX/4 || f.pitch<f.width*4 {
        return false
    }
    let Some(length)=f.pitch.checked_mul(f.height) else {return false};
    if !buffer_span_ok(f.address as usize,length,core::mem::align_of::<u32>()) {
        return false
    }
    let Some(r)=mask(f.red_size,f.red_shift) else {return false};
    let Some(g)=mask(f.green_size,f.green_shift) else {return false};
    let Some(b)=mask(f.blue_size,f.blue_shift) else {return false};
    (r&g)==0 && (r&b)==0 && (g&b)==0
}
fn framebuffer_status_rect_valid(
    f:&Framebuffer,x:u64,y:u64,width:u64,height:u64
)->bool {
    if !framebuffer_ok(f)||width==0||height==0{return false;}
    let Some(x_end)=x.checked_add(width)else{return false;};
    let Some(y_end)=y.checked_add(height)else{return false;};
    if x_end>f.width||y_end>f.height{return false;}
    let Some(end)=y_end.checked_sub(1)
        .and_then(|row|row.checked_mul(f.pitch))
        .and_then(|at|x_end.checked_mul(4).and_then(|pix|at.checked_add(pix)))
        else{return false;};
    f.pitch.checked_mul(f.height).is_some_and(|size|end<=size)
}
fn chan(c:u8,bits:u8,shift:u8)->u32 {
    (((c as u32)*((1u32<<bits)-1)/255)<<shift)
}
fn pixel(f:&Framebuffer,r:u8,g:u8,b:u8)->u32 {
    chan(r,f.red_size,f.red_shift) |
    chan(g,f.green_size,f.green_shift) |
    chan(b,f.blue_size,f.blue_shift)
}
/// # Safety
/// Called once at boot while framebuffer allocation is mapped and exclusive.
#[no_mangle]
pub unsafe extern "C" fn boot_draw_status(){
    let Some(resp)=(unsafe {limine_response(boot_limine_framebuffers())}) else {
        log(b"IndianOS: framebuffer unavailable (serial mode)\n\0");return
    };
    if !limine_framebuffer_table_valid(resp){
        log(b"IndianOS: framebuffer unavailable (serial mode)\n\0");return
    }
    let first=unsafe {ptr::read_volatile(resp.framebuffers)};
    if !limine_pointer_aligned(first){
        log(b"IndianOS: unsupported framebuffer (serial mode)\n\0");return
    }
    let Some(f)=(unsafe {first.as_ref()}) else {
        log(b"IndianOS: unsupported framebuffer (serial mode)\n\0");return
    };
    if !framebuffer_ok(f){
        log(b"IndianOS: unsupported framebuffer (serial mode)\n\0");return
    }
    let w=f.width.min(480);
    let h=f.height.min(120);
    let x0=(f.width-w)/2;
    let y0=(f.height-h)/2;
    if !framebuffer_status_rect_valid(f,x0,y0,w,h){
        log(b"IndianOS: framebuffer rectangle unsafe (serial mode)\n\0");
        return;
    }
    let saffron=pixel(f,255,153,51);
    let white=pixel(f,255,255,255);
    let green=pixel(f,19,136,8);
    let navy=pixel(f,0,0,128);
    for y in 0..h{
        let stripe=if y<h/3{saffron}else if y<(h*2)/3{white}else{green};
        for x in 0..w{
            let xmid=w/2;let ymid=h/2;
            let center=x>=xmid.saturating_sub(9) && x<xmid+9 &&
                y>=ymid.saturating_sub(9) && y<ymid+9;
            let offset=((y0+y)*f.pitch+(x0+x)*4) as usize;
            unsafe {ptr::write_volatile(f.address.add(offset).cast::<u32>(),
                                       if center{navy}else{stripe});}
        }
    }
    log(b"IndianOS: framebuffer status rendered\n\0");
}
/// An optional and bounded framebuffer view for the R014.2 VGA console.
/// This view borrows Limine framebuffer mappings for the lifetime of boot.
#[cfg(any(feature="test-irq-keyboard",feature="test-r029-desktop",feature="normal-desktop"))]
pub(crate) struct ConsoleFramebuffer {
    pub address:*mut u8,
    pub width:usize,
    pub height:usize,
    pub pitch:usize,
    pub fg:u32,
    pub bg:u32,
}
#[cfg(any(feature="test-irq-keyboard",feature="test-r029-desktop",feature="normal-desktop"))]
pub(crate) fn console_framebuffer()->Option<ConsoleFramebuffer>{
    let response=unsafe{boot_limine_framebuffers()};
    if !limine_pointer_aligned(response){return None;}
    let resp=unsafe{&*response};
    if !limine_framebuffer_table_valid(resp){return None;}
    let first=unsafe{ptr::read_volatile(resp.framebuffers)};
    if !limine_pointer_aligned(first){return None;}
    let f=unsafe{&*first};
    if !framebuffer_ok(f){return None;}
    Some(ConsoleFramebuffer{
        address:f.address,
        width:f.width as usize,height:f.height as usize,pitch:f.pitch as usize,
        fg:pixel(f,235,242,255),bg:pixel(f,10,18,35),
    })
}

#[cfg(any(feature="test-r029-desktop",feature="normal-desktop"))]
pub(crate) fn desktop_rgb(r:u8,g:u8,b:u8)->Option<u32>{
    let response=unsafe{boot_limine_framebuffers()};
    if !limine_pointer_aligned(response){return None;}
    let resp=unsafe{&*response};
    if !limine_framebuffer_table_valid(resp){return None;}
    let first=unsafe{ptr::read_volatile(resp.framebuffers)};
    if !limine_pointer_aligned(first){return None;}
    let framebuffer=unsafe{&*first};
    if !framebuffer_ok(framebuffer){return None;}
    Some(pixel(framebuffer,r,g,b))
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn memory_map_checks(){
        assert!(validate_ranges(&[Region{base:4096,length:4096,kind:0}]));
        assert!(!validate_ranges(&[Region{base:4096,length:0,kind:0}]));
        assert!(!validate_ranges(&[Region{base:u64::MAX,length:4096,kind:0}]));
        assert!(!validate_ranges(&[Region{base:4096,length:4096,kind:1}]));
    }
    #[test]
    fn framebuffer_masks(){
        assert_eq!(mask(8,16),Some(0xff0000));
        assert_eq!(mask(8,24),Some(0xff000000));
        assert_eq!(mask(9,3),None);
        assert_eq!(mask(8,29),None);
    }
    // These are arithmetic descriptors only; no synthetic pointer is read.
    #[test]
    fn buffer_span_boundaries(){
        assert!(buffer_span_ok(0x1000,4096,4));
        assert!(buffer_span_ok(0xffff_8000_0000_0000,4096,4));
        assert!(buffer_span_ok(0x0000_7fff_ffff_fffc,4,4));
        assert!(!buffer_span_ok(0,4,4));
        assert!(!buffer_span_ok(0x1000,0,4));
        assert!(!buffer_span_ok(0x1001,4,4));
        assert!(!buffer_span_ok(0x1000,4,3));
        assert!(!buffer_span_ok(0x1000,4,0));
        assert!(!buffer_span_ok(0x1000,isize::MAX as u64+1,1));
        assert!(!buffer_span_ok(usize::MAX-3,4,4));
        assert!(!buffer_span_ok(0x0000_8000_0000_0000,4,4));
        assert!(!buffer_span_ok(0xffff_7fff_ffff_fffc,4,4));
        assert!(!buffer_span_ok(0x0000_7fff_ffff_fffc,8,4));
        assert!(!buffer_span_ok(0x0000_7fff_ffff_ffff,0xffff_0000_0000_0002,1));
        // Module buffers have byte alignment but the same range constraints.
        assert!(buffer_span_ok(0x1001,16,1));
        assert!(!buffer_span_ok(0x0000_7fff_ffff_fff8,16,1));
    }
    fn framebuffer_descriptor(address:usize)->Framebuffer {
        Framebuffer{address:address as *mut u8,width:2,height:2,pitch:8,
            bpp:32,model:1,red_size:8,red_shift:16,green_size:8,
            green_shift:8,blue_size:8,blue_shift:0}
    }
    #[test]
    fn framebuffer_span_checks(){
        let mut f=framebuffer_descriptor(0xffff_8000_0000_1000);
        assert!(framebuffer_ok(&f));
        f.address=0x0000_7fff_ffff_fff8 as *mut u8;
        assert!(!framebuffer_ok(&f));
        f.address=(usize::MAX-7) as *mut u8;
        assert!(!framebuffer_ok(&f));
        f.address=0x1001 as *mut u8;
        assert!(!framebuffer_ok(&f));
        f.address=0x1000 as *mut u8;
        f.height=u64::MAX;
        assert!(!framebuffer_ok(&f));
        f.height=1;
        f.pitch=1u64<<63;
        assert!(!framebuffer_ok(&f));
        f.pitch=4;
        assert!(!framebuffer_ok(&f));
        f.pitch=8;
        f.green_shift=16;
        assert!(!framebuffer_ok(&f));
    }
    #[test]
    fn framebuffer_prefix_layout(){
        assert_eq!(core::mem::offset_of!(Framebuffer,pitch),24);
        assert_eq!(core::mem::offset_of!(Framebuffer,bpp),32);
        assert_eq!(core::mem::size_of::<Region>(),24);
        assert_eq!(core::mem::offset_of!(LimineFile,size),16);
    }
}


#[cfg(test)]
mod m1_batch01_w3_boot_tests{
 use super::*;
 #[test]fn b01_08_valid_ram_entry_is_accepted(){
  assert!(valid_region(&Region{base:0x1000,length:0x2000,kind:0}));
 }
 #[test]fn b01_09_zero_length_memory_entry_is_rejected(){
  assert!(!valid_region(&Region{base:0,length:0,kind:0}));
 }
 #[test]fn b01_10_wrapping_boot_region_is_rejected(){
  assert!(!valid_region(&Region{base:u64::MAX-1,length:3,kind:0}));
 }
 #[test]fn b01_11_nonusable_only_map_is_rejected(){
  assert!(!validate_ranges(&[Region{base:0x1000,length:0x1000,kind:1}]));
 }
}
#[cfg(test)]
mod m1_gate20_w1_boot_tests {
 use super::*;
 #[test]fn exported_map_count_fails_closed_on_corrupt_tail(){
  let ok=Region{base:0x1000,length:0x1000,kind:0};
  let bad=Region{base:u64::MAX,length:2,kind:1};
  let entries=[&ok as *const Region,&bad as *const Region];
  let map=MemoryMap{revision:0,count:2,entries:entries.as_ptr()};
  assert_eq!(validated_map_count(&map),None);
  let good_entries=[&ok as *const Region];
  let good=MemoryMap{revision:0,count:1,entries:good_entries.as_ptr()};
  assert_eq!(validated_map_count(&good),Some(1));
 }
}

#[cfg(test)]
mod m1_gate20_w1_next_boot_tests{
 use super::*;
 #[test]fn complete_memory_map_preflight_blocks_late_corruption(){
   let good=Region{base:0x1000,length:0x2000,kind:0};
   let bad=Region{base:u64::MAX-1,length:4,kind:1};
   let refs=[&good as *const Region,&bad as *const Region];
   let map=MemoryMap{revision:0,count:2,entries:refs.as_ptr()};
   assert_eq!(validated_map_count(&map),None);
   let refs=[&good as *const Region];
   let map=MemoryMap{revision:0,count:1,entries:refs.as_ptr()};
   assert_eq!(validated_map_count(&map),Some(1));
 }
 #[test]fn direct_map_must_be_canonical_aligned_and_nonwrapping(){
   assert!(hhdm_offset_sane(0xffff_8000_0000_0000));
   assert!(!hhdm_offset_sane(0));
   assert!(!hhdm_offset_sane(0x8000_0000));
   assert!(!hhdm_offset_sane(0xffff_8000_0000_0001));
   assert!(!hhdm_offset_sane(0xffff_ffff_ffff_f000));
 }
}


// P0-475: exercise the production Limine memory-map preflight.
#[cfg(test)]
mod kernel_p0_batch19_w6_tests {
    use super::*;
    fn observed(a:Region,b:Region)->Option<u64> {
        let regions=[a,b];
        let pointers:[*const Region;2]=[&regions[0],&regions[1]];
        let map=MemoryMap{revision:0,count:2,entries:pointers.as_ptr()};
        validated_map_count(&map)
    }
    #[test]
    fn kernel_p0_batch19_w6_rejects_reserved_and_usable_overlap() {
        let ram=Region{base:0x2000,length:0x3000,kind:0};
        assert_eq!(observed(ram,Region{base:0x4001,length:1,kind:1}),None);
        assert_eq!(observed(ram,Region{base:0x2000,length:0x1000,kind:0}),None);
        assert_eq!(observed(ram,Region{base:0x5000,length:0x1000,kind:1}),Some(2));
    }
    #[test]
    fn kernel_p0_batch19_w6_rejects_corrupted_reserved_tail() {
        let ram=Region{base:0x2000,length:0x2000,kind:0};
        assert_eq!(observed(ram,Region{base:u64::MAX,length:3,kind:1}),None);
        assert_eq!(observed(ram,Region{base:0x5000,length:0,kind:1}),None);
        assert_eq!(observed(Region{kind:1,..ram},
            Region{base:0x5000,length:0x1000,kind:1}),None);
        assert!(!validate_ranges(&[ram,Region{base:0x3fff,length:1,kind:2}]));
    }
}


// P0-515–520: numeric HHDM sanity is mandatory for production boot preflight.
#[cfg(test)]
mod kernel_p0_batch20_w6_tests{
    use super::*;
    #[test]
    fn kernel_p0_batch20_w6_hhdm_rejects_unmapped_aliases(){
        assert!(hhdm_offset_sane(0xffff_8000_0000_0000));
        assert!(!hhdm_offset_sane(0));
        assert!(!hhdm_offset_sane(0xffff_7fff_ffff_f000));
        assert!(!hhdm_offset_sane(0xffff_8000_0000_0001));
        assert!(!hhdm_offset_sane(0xffff_ffff_ffff_f000));
    }
}


// P0-555: validated initramfs lookup cannot hand an external supervisor
// pointer to the ring3 ELF loader or native startup path.
#[cfg(test)]
mod kernel_p0_batch21_w6_tests {
    use super::*;
    #[test]fn kernel_p0_batch21_w6_archive_file_must_stay_in_boot_module(){
        let base=0x2000usize;
        assert!(borrowed_ramfile_span_ok(base,256,(base+16) as *const u8,240));
        assert!(borrowed_ramfile_span_ok(base,256,(base+256) as *const u8,0));
        assert!(!borrowed_ramfile_span_ok(base,256,(base-1) as *const u8,1));
        assert!(!borrowed_ramfile_span_ok(base,256,(base+250) as *const u8,7));
        assert!(!borrowed_ramfile_span_ok(0,256,base as *const u8,8));
        assert!(!borrowed_ramfile_span_ok(base,256,core::ptr::null(),8));
        assert!(!borrowed_ramfile_span_ok(usize::MAX-4,32,
            (usize::MAX-4) as *const u8,1));
    }
}


// Batch22 W6 P0-578..580: validate Limine pointer alignment and high-half
// arithmetic on the actual boot memory-map consumer.
#[cfg(test)]
mod kernel_p0_batch22_w6_tests{
    use super::*;
    #[test]fn kernel_p0_batch22_w6_578_reject_misaligned_entry_table(){
        let region=Region{base:0x1000,length:0x2000,kind:0};
        let entry:[*const Region;1]=[&region];
        let valid=MemoryMap{revision:0,count:1,entries:entry.as_ptr()};
        assert_eq!(validated_map_count(&valid),Some(1));
        let invalid=MemoryMap{entries:((entry.as_ptr() as usize)+1) as *const *const Region,
            ..valid};
        assert_eq!(validated_map_count(&invalid),None);
    }
    #[test]fn kernel_p0_batch22_w6_579_reject_misaligned_region_pointer(){
        let invalid_pointer=0x1001usize as *const Region;
        let entries=[invalid_pointer];
        let map=MemoryMap{revision:0,count:1,entries:entries.as_ptr()};
        assert_eq!(validated_map_count(&map),None);
    }
    #[test]fn kernel_p0_batch22_w6_580_direct_map_must_cover_usable_span(){
        let region=Region{base:0x1000,length:0x2000,kind:0};
        let entries:[*const Region;1]=[&region];
        let map=MemoryMap{revision:0,count:1,entries:entries.as_ptr()};
        assert!(hhdm_covers_usable_map(&map,0xffff_8000_0000_0000));
        assert!(!hhdm_covers_usable_map(&map,0));
        let huge=Region{base:0x1_0000_0000,length:0x1000,kind:0};
        let huge_entries:[*const Region;1]=[&huge];
        let huge_map=MemoryMap{revision:0,count:1,entries:huge_entries.as_ptr()};
        assert!(!hhdm_covers_usable_map(&huge_map,0xffff_ffff_0000_0000));
    }
}


// Batch23 W6 P0-602..605: boot consumers share the validated HHDM and
// Limine pointer alignment contract; no dereference of misaligned metadata.
#[cfg(test)]
mod kernel_p0_batch23_w6_tests{
 use super::*;
 #[test]fn kernel_p0_batch23_w6_602_hhdm_rejects_usable_overflow(){
    let r=Region{base:0x2000,length:4096,kind:0};
    let entries:[*const Region;1]=[&r];
    let map=MemoryMap{revision:0,count:1,entries:entries.as_ptr()};
    assert!(hhdm_covers_usable_map(&map,0xffff_8000_0000_0000));
    assert!(!hhdm_covers_usable_map(&map,0xffff_ffff_ffff_e000));
 }
 #[test]fn kernel_p0_batch23_w6_603_module_table_alignment(){
    let table:[*const u64;1]=[core::ptr::null()];
    let valid=table.as_ptr();
    assert!(limine_pointer_aligned(valid));
    let misaligned=(valid as usize+1) as *const *const u64;
    assert!(!limine_pointer_aligned(misaligned));
 }
 #[test]fn kernel_p0_batch23_w6_604_framebuffer_table_alignment(){
    let table:[*const u32;1]=[core::ptr::null()];
    assert!(limine_pointer_aligned(table.as_ptr()));
    assert!(!limine_pointer_aligned(
        (table.as_ptr() as usize+1) as *const *const u32));
 }
 #[test]fn kernel_p0_batch23_w6_605_file_ptr_alignment(){
    let file=1234u64;
    assert!(limine_pointer_aligned(&file as *const u64));
    assert!(!limine_pointer_aligned(
        (&file as *const u64 as usize+1) as *const u64));
    assert!(!limine_pointer_aligned(core::ptr::null::<u64>()));
 }
}


// Batch24 W6 P0-623..625: Limine response pointers are validated for
// null and alignment BEFORE constructing borrowed supervisor references.
// The trusted bootloader still supplies pointer validity and lifetime.
#[cfg(test)]
mod kernel_p0_batch24_w6_tests{
    use super::*;
    #[test]fn kernel_p0_batch24_w6_623_null_response_rejected(){
        assert!(unsafe{limine_response::<MemoryMap>(ptr::null())}.is_none());
        assert!(unsafe{limine_response::<Hhdm>(ptr::null())}.is_none());
    }
    #[test]fn kernel_p0_batch24_w6_624_misaligned_response_rejected(){
        let h=Hhdm{revision:0,offset:0xffff_8000_0000_0000};
        let forged=((&h as *const Hhdm as usize)+1) as *const Hhdm;
        assert!(unsafe{limine_response(forged)}.is_none());
    }
    #[test]fn kernel_p0_batch24_w6_625_well_aligned_responses_retained(){
        let h=Hhdm{revision:0,offset:0xffff_8000_0000_0000};
        let borrowed=unsafe{limine_response(&h as *const Hhdm)}.unwrap();
        assert_eq!(borrowed.offset,h.offset);
        let map=MemoryMap{revision:0,count:0,entries:ptr::null()};
        let found=unsafe{limine_response(&map as *const MemoryMap)}.unwrap();
        assert_eq!(found.count,0);
    }
}

// P0-651..655: reject unaddressable physical RAM before constructing HHDM.
#[cfg(test)]
mod kernel_p0_batch25_w6_tests {
    use super::*;
    const HHDM:u64=0xffff_8000_0000_0000;
    fn check(region:Region,offset:u64)->bool{
        let pointer=&region as *const Region;
        let entries=[pointer];
        let map=MemoryMap{revision:0,count:1,entries:entries.as_ptr()};
        hhdm_covers_usable_map(&map,offset)
    }
    #[test]fn kernel_p0_batch25_w6_651_low_ram_hhdm_valid(){
        assert!(check(Region{base:4096,length:8192,kind:0},HHDM));
    }
    #[test]fn kernel_p0_batch25_w6_652_unaddressable_start_denied(){
        assert!(!check(Region{base:1u64<<52,length:4096,kind:0},HHDM));
    }
    #[test]fn kernel_p0_batch25_w6_653_crossing_physical_limit_denied(){
        assert!(!check(Region{base:(1u64<<52)-4096,length:8192,kind:0},HHDM));
    }
    #[test]fn kernel_p0_batch25_w6_654_last_physical_page_requires_addressable_hhdm(){
        // Physical MAXPHYADDR does not imply direct-map addressability:
        // HHDM + (2^52 - 1) overflows the four-level high-half window.
        assert!(!check(Region{base:(1u64<<52)-4096,length:4096,kind:0},HHDM));
    }
    #[test]fn kernel_p0_batch25_w6_655_invalid_direct_map_offset_denied(){
        assert!(!check(Region{base:4096,length:4096,kind:0},0));
        assert!(!check(Region{base:4096,length:4096,kind:0},HHDM+1));
    }
}

// P0-673..675: PMM sees only page-aligned usable RAM from Limine.
#[cfg(test)]
mod kernel_p0_batch26_w6_tests {
    use super::*;
    const HHDM:u64=0xffff_8000_0000_0000;
    fn check(region:Region)->bool {
        let entries=[&region as *const Region];
        let map=MemoryMap{revision:0,count:1,entries:entries.as_ptr()};
        hhdm_covers_usable_map(&map,HHDM)
    }
    #[test]fn kernel_p0_batch26_w6_673_aligned_usable_ram_accepted(){
        assert!(check(Region{base:0x2000,length:0x3000,kind:0}));
    }
    #[test]fn kernel_p0_batch26_w6_674_unaligned_usable_base_denied(){
        assert!(!check(Region{base:0x2001,length:0x3000,kind:0}));
    }
    #[test]fn kernel_p0_batch26_w6_675_partial_last_page_denied(){
        assert!(!check(Region{base:0x2000,length:0x3001,kind:0}));
    }
}

#[cfg(test)]
mod kernel_p0_batch27_w6_tests {
    use super::*;
    const HHDM:u64=0xffff_8000_0000_0000;
    fn two(a:Region,b:Region)->bool {
        let pointers=[&a as *const Region,&b as *const Region];
        let map=MemoryMap{revision:0,count:2,entries:pointers.as_ptr()};
        hhdm_covers_usable_map(&map,HHDM)
    }
    #[test]fn kernel_p0_batch27_w6_697_two_disjoint_usable_spans(){
        assert!(two(Region{base:0x1000,length:0x3000,kind:0},
                    Region{base:0x9000,length:0x1000,kind:0}));
    }
    #[test]fn kernel_p0_batch27_w6_698_reserved_overlap_prevents_pmm_handoff(){
        assert!(!two(Region{base:0x1000,length:0x3000,kind:0},
                     Region{base:0x2000,length:0x1000,kind:1}));
    }
    #[test]fn kernel_p0_batch27_w6_699_malformed_reserved_tail_fails_closed(){
        assert!(!two(Region{base:0x1000,length:0x1000,kind:0},
                     Region{base:u64::MAX-1,length:0x1000,kind:1}));
    }
    #[test]fn kernel_p0_batch27_w6_700_unaddressable_high_ram_denied(){
        assert!(!two(Region{base:0x1000,length:0x1000,kind:0},
                     Region{base:(1u64<<52)-4096,length:4096,kind:0}));
    }
}

#[cfg(test)]
mod kernel_p0_batch28_w6 {
 use super::*;
 #[test]fn kernel_p0_batch28_w6_718_aligned_response_pointer_valid(){
  let response=core::mem::MaybeUninit::<Framebuffers>::uninit();
  assert!(limine_pointer_aligned(response.as_ptr()));
 }
 #[test]fn kernel_p0_batch28_w6_719_misaligned_descriptor_pointer_refused(){
  let f=core::mem::MaybeUninit::<Framebuffer>::uninit();
  let p=f.as_ptr().cast::<u8>().wrapping_add(1).cast::<Framebuffer>();
  assert!(!limine_pointer_aligned(p));
 }
 #[test]fn kernel_p0_batch28_w6_720_null_response_table_frame_refused(){
  assert!(!limine_pointer_aligned(core::ptr::null::<Framebuffers>()));
  assert!(!limine_pointer_aligned(core::ptr::null::<*const Framebuffer>()));
  assert!(!limine_pointer_aligned(core::ptr::null::<Framebuffer>()));
 }
}

#[cfg(test)]
mod kernel_p0_batch29_w6{
 use super::*;
 #[test] fn kernel_p0_batch29_w6_742_low_canonical_pointer(){assert!(limine_pointer_aligned(0x1000usize as *const Region));}
 #[test] fn kernel_p0_batch29_w6_743_high_canonical_pointer(){assert!(limine_pointer_aligned(0xffff_8000_0000_1000usize as *const Region));}
 #[test] fn kernel_p0_batch29_w6_744_noncanonical_pointer_denied(){assert!(!limine_pointer_aligned(0x0000_8000_0000_0000usize as *const Region));assert!(!limine_pointer_aligned(0xffff_7fff_ffff_f000usize as *const Region));}
 #[test] fn kernel_p0_batch29_w6_745_null_misaligned_pointer_denied(){assert!(!limine_pointer_aligned(core::ptr::null::<Region>()));assert!(!limine_pointer_aligned(0x1001usize as *const Region));}
}

#[cfg(test)]
mod kernel_p0_batch30_w6 {
    use super::*;
    #[test] fn kernel_p0_batch30_w6_763_normal_canonical_descriptor_allowed(){
        assert!(limine_pointer_aligned(0x1000usize as *const Region));
        assert!(limine_pointer_aligned(0xffff_8000_0000_1000usize as *const Region));
    }
    #[test] fn kernel_p0_batch30_w6_764_descriptor_crossing_canonical_gap_denied(){
        assert!(!limine_pointer_aligned(0x0000_7fff_ffff_fff0usize as *const Region));
    }
    #[test] fn kernel_p0_batch30_w6_765_pointer_wrap_and_misalignment_denied(){
        assert!(!limine_pointer_aligned(0xffff_ffff_ffff_fff8usize as *const Region));
        assert!(!limine_pointer_aligned(0x1001usize as *const Region));
        assert!(!limine_pointer_aligned(core::ptr::null::<Region>()));
    }
}

#[cfg(test)]
mod kernel_p0_batch31_w6 {
    use super::*;
    #[test]fn kernel_p0_batch31_w6_783_canonical_module_subset_allowed(){
        assert!(borrowed_ramfile_span_ok(0x1000,64,0x1010usize as *const u8,8));
        assert!(borrowed_ramfile_span_ok(0x1000,64,0x1040usize as *const u8,0));
    }
    #[test]fn kernel_p0_batch31_w6_784_module_crossing_canonical_gap_denied(){
        let base=0x0000_7fff_ffff_fff0usize;
        assert!(!borrowed_ramfile_span_ok(base,64,(base+8) as *const u8,1));
    }
    #[test]fn kernel_p0_batch31_w6_785_module_wrap_and_outside_denied(){
        assert!(!borrowed_ramfile_span_ok(usize::MAX-15,64,usize::MAX as *const u8,1));
        assert!(!borrowed_ramfile_span_ok(0x1000,64,0x1040usize as *const u8,1));
        assert!(!borrowed_ramfile_span_ok(0x1000,64,0x1008usize as *const u8,64));
    }
}

#[cfg(test)]
mod kernel_p0_batch32_w6 {
    use super::*;
    #[test] fn kernel_p0_batch32_w6_803_canonical_pointer_tables_admitted(){
        assert!(limine_pointer_table_valid(0x1000usize as *const *const Region,8,4096));
        assert!(limine_pointer_table_valid(
            0xffff_8000_0000_1000usize as *const *const Framebuffer,2,4096));
    }
    #[test] fn kernel_p0_batch32_w6_804_zero_excessive_and_null_tables_denied(){
        let p=0x1000usize as *const *const Region;
        assert!(!limine_pointer_table_valid(p,0,4096));
        assert!(!limine_pointer_table_valid(p,4097,4096));
        assert!(!limine_pointer_table_valid(core::ptr::null::<*const Region>(),1,4096));
        assert!(!limine_pointer_table_valid(p,u64::MAX,u64::MAX));
    }
    #[test] fn kernel_p0_batch32_w6_805_full_table_cannot_cross_canonical_gap(){
        let p=0x0000_7fff_ffff_fff8usize as *const *const Region;
        assert!(limine_pointer_table_valid(p,1,4096));
        assert!(!limine_pointer_table_valid(p,2,4096));
        assert!(!limine_pointer_table_valid(
            0x1001usize as *const *const Region,1,4096));
    }
}

#[cfg(test)]
mod kernel_p0_batch33_w6 {
    use super::*;
    const HHDM:u64=0xffff_8000_0000_0000;
    #[test] fn kernel_p0_batch33_w6_823_valid_usable_phys_region_maps_high_half(){
        assert!(hhdm_usable_region_valid(
            &Region{base:0x1000,length:0x2000,kind:0},HHDM));
        assert!(hhdm_usable_region_valid(
            &Region{base:0x1_0000_0000,length:0x1000,kind:0},HHDM));
    }
    #[test] fn kernel_p0_batch33_w6_824_zero_region_mapped_but_invalid_types_denied(){
        // Limine may mark page zero usable. This does NOT authorize the PMM
        // to allocate physical frame zero (that fence lives in initialize).
        let zero=Region{base:0,length:0x2000,kind:0};
        assert!(hhdm_usable_region_valid(&zero,HHDM));
        let entries=[&zero as *const Region];
        let map=MemoryMap{revision:0,count:1,entries:entries.as_ptr()};
        assert!(hhdm_covers_usable_map(&map,HHDM));
        assert!(!hhdm_usable_region_valid(
            &Region{base:0x1000,length:0x1000,kind:1},HHDM));
        assert!(!hhdm_usable_region_valid(
            &Region{base:1u64<<52,length:0x1000,kind:0},HHDM));
    }
    #[test] fn kernel_p0_batch33_w6_825_misaligned_overflowing_hhdm_region_denied(){
        assert!(!hhdm_usable_region_valid(
            &Region{base:0x1001,length:0x1000,kind:0},HHDM));
        assert!(!hhdm_usable_region_valid(
            &Region{base:0x2000,length:0x1000,kind:0},0xffff_ffff_ffff_f000));
        assert!(!hhdm_usable_region_valid(
            &Region{base:u64::MAX-0xfff,length:0x1000,kind:0},HHDM));
    }
}

#[cfg(test)]
mod kernel_p0_batch34_w6 {
    use super::*;
    fn fb()->Framebuffer {
        Framebuffer{address:0x1000 as *mut u8,width:80,height:30,
            pitch:80*4,bpp:32,model:1,
            red_size:8,red_shift:16,green_size:8,green_shift:8,
            blue_size:8,blue_shift:0}
    }
    #[test] fn kernel_p0_batch34_w6_843_center_and_full_frame_rects_fit(){
        let f=fb();
        assert!(framebuffer_status_rect_valid(&f,10,10,60,10));
        assert!(framebuffer_status_rect_valid(&f,0,0,80,30));
    }
    #[test] fn kernel_p0_batch34_w6_844_outside_or_wrapping_rect_denied(){
        let f=fb();
        assert!(!framebuffer_status_rect_valid(&f,79,0,2,1));
        assert!(!framebuffer_status_rect_valid(&f,0,29,1,2));
        assert!(!framebuffer_status_rect_valid(&f,u64::MAX,0,2,1));
        assert!(!framebuffer_status_rect_valid(&f,0,0,0,2));
    }
    #[test] fn kernel_p0_batch34_w6_845_invalid_pitch_or_mask_denies_render(){
        let mut f=fb();f.pitch=79*4;
        assert!(!framebuffer_status_rect_valid(&f,0,0,1,1));
        f=fb();f.green_shift=16;
        assert!(!framebuffer_status_rect_valid(&f,0,0,1,1));
    }
}

#[cfg(test)] mod kernel_p0_batch35_w6 {
 use super::*;
 fn fb()->Framebuffer{
  Framebuffer{address:0x1000 as *mut u8,width:80,height:30,
   pitch:320,bpp:32,model:1,
   red_size:8,red_shift:16,green_size:8,green_shift:8,
   blue_size:8,blue_shift:0}
 }
 #[test] fn kernel_p0_batch35_w6_863_two_valid_limine_framebuffers(){
  let a=fb();let b=fb();
  let slots=[&a as *const Framebuffer,&b as *const Framebuffer];
  let table=Framebuffers{revision:0,count:2,framebuffers:slots.as_ptr()};
  assert!(limine_framebuffer_table_valid(&table));
 }
 #[test] fn kernel_p0_batch35_w6_864_late_misaligned_pointer_denied(){
  let a=fb();
  let slots=[&a as *const Framebuffer,0x1001usize as *const Framebuffer];
  let table=Framebuffers{revision:0,count:2,framebuffers:slots.as_ptr()};
  assert!(!limine_framebuffer_table_valid(&table));
 }
 #[test] fn kernel_p0_batch35_w6_865_empty_null_and_oversize_denied(){
  let a=fb();let slots=[&a as *const Framebuffer];
  let mut table=Framebuffers{revision:0,count:0,framebuffers:slots.as_ptr()};
  assert!(!limine_framebuffer_table_valid(&table));
  table.count=1;table.framebuffers=core::ptr::null();
  assert!(!limine_framebuffer_table_valid(&table));
  table.count=4097;table.framebuffers=slots.as_ptr();
  assert!(!limine_framebuffer_table_valid(&table));
 }
}
