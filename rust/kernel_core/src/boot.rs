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
unsafe fn memory_map() -> Option<&'static MemoryMap> {
    unsafe { boot_limine_memmap().as_ref() }
}
fn validate_ranges(regions:&[Region]) -> bool {
    let mut usable=false;
    for r in regions {
        if r.length==0 || r.base.checked_add(r.length).is_none() { return false; }
        usable |= r.kind==0;
    }
    usable
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
    if map.entries.is_null() || !(1..=4096).contains(&map.count) {
        log(b"IndianOS: missing or oversized memory map\n\0"); return false;
    }
    let mut usable=false;
    for i in 0..map.count as usize {
        let item=unsafe { ptr::read_volatile(map.entries.add(i)) };
        let Some(region)=(unsafe { item.as_ref() }) else {
            log(b"IndianOS: invalid memory region\n\0"); return false;
        };
        if region.length==0 || region.base.checked_add(region.length).is_none() {
            log(b"IndianOS: invalid memory region\n\0"); return false;
        }
        usable |= region.kind==0;
    }
    if !usable { log(b"IndianOS: no usable RAM in memory map\n\0"); }
    usable
}
#[no_mangle]
pub unsafe extern "C" fn boot_memory_region_count() -> u64 {
    let Some(m)=(unsafe { memory_map() }) else {return 0};
    m.count
}
/// # Safety
/// Valid writable C region output or null. The context parameter is ignored.
/// The caller must not invalidate Limine's response while iterating.
#[no_mangle]
pub unsafe extern "C" fn boot_memory_region_at(index:u64,out:*mut Region,_context:*mut c_void)->bool {
    if out.is_null(){return false}
    let Some(m)=(unsafe {memory_map()}) else {return false};
    if m.entries.is_null() || index>=m.count || m.count>4096 {return false}
    let item=unsafe {ptr::read_volatile(m.entries.add(index as usize))};
    let Some(region)=(unsafe {item.as_ref()}) else {return false};
    unsafe {ptr::write(out,*region);}
    true
}
/// # Safety
/// Output pointer is writable and Limine-owned HHDM response still valid.
#[no_mangle]
pub unsafe extern "C" fn boot_hhdm_offset(out:*mut u64)->bool {
    if out.is_null(){return false}
    let Some(h)=(unsafe {boot_limine_hhdm().as_ref()}) else {return false};
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
    let Some(modules)=(unsafe {boot_limine_modules().as_ref()}) else {return false};
    if modules.count!=1 || modules.modules.is_null(){return false}
    let file=unsafe {ptr::read_volatile(modules.modules)};
    let Some(file)=(unsafe {file.as_ref()}) else {return false};
    if !(16..=32*1024*1024).contains(&file.size) ||
       !buffer_span_ok(file.address as usize,file.size,1){return false}
    let mut found=Ramfile{bytes:ptr::null(),length:0};
    if !unsafe {indianos_initramfs_lookup(file.address,file.size as usize,path,&mut found)} {
        return false
    }
    unsafe {
        ptr::write(data,found.bytes);
        ptr::write(size,found.length as usize);
    }
    true
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
    let Some(resp)=(unsafe {boot_limine_framebuffers().as_ref()}) else {
        log(b"IndianOS: framebuffer unavailable (serial mode)\n\0");return
    };
    if resp.count==0 || resp.framebuffers.is_null(){
        log(b"IndianOS: framebuffer unavailable (serial mode)\n\0");return
    }
    let first=unsafe {ptr::read_volatile(resp.framebuffers)};
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
#[cfg(any(feature="test-irq-keyboard",feature="test-r029-desktop"))]
pub(crate) struct ConsoleFramebuffer {
    pub address:*mut u8,
    pub width:usize,
    pub height:usize,
    pub pitch:usize,
    pub fg:u32,
    pub bg:u32,
}
#[cfg(any(feature="test-irq-keyboard",feature="test-r029-desktop"))]
pub(crate) fn console_framebuffer()->Option<ConsoleFramebuffer>{
    let resp=unsafe{boot_limine_framebuffers().as_ref()}?;
    if resp.count==0||resp.framebuffers.is_null(){return None;}
    let first=unsafe{ptr::read_volatile(resp.framebuffers)};
    let f=unsafe{first.as_ref()}?;
    if !framebuffer_ok(f){return None;}
    Some(ConsoleFramebuffer{
        address:f.address,
        width:f.width as usize,height:f.height as usize,pitch:f.pitch as usize,
        fg:pixel(f,235,242,255),bg:pixel(f,10,18,35),
    })
}

#[cfg(feature="test-r029-desktop")]
pub(crate) fn desktop_rgb(r:u8,g:u8,b:u8)->Option<u32>{
    let resp=unsafe{boot_limine_framebuffers().as_ref()}?;
    if resp.count==0||resp.framebuffers.is_null(){return None;}
    let first=unsafe{ptr::read_volatile(resp.framebuffers)};
    let framebuffer=unsafe{first.as_ref()}?;
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
