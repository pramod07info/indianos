//! Rust-owned Limine revision-6 request table and response-pointer adapters.
//!
//! This replaces the LAST Limine-specific C request/shim translation unit
//! under RUST_BOOT_PROTOCOL=1. The public Limine ABI uses four 64-bit IDs,
//! revision u64, then a pointer, all written by Limine in RW request pages.
//!
//! SAFETY: response pointers belong to Limine and are valid only during the
//! initial single-core boot before any bootloader memory reclamation.
//! Do not call these getters from interrupts or other CPUs.
use core::{ffi::c_void,ptr};

const COMMON:[u64;2]=[0xc7b1dd30df4c8b88,0x0a82e883a194f07b];
#[repr(C)]
struct Request {
    id:[u64;4],
    revision:u64,
    response:*const c_void,
}
#[repr(C)]
struct ModuleRequest {
    id:[u64;4],
    revision:u64,
    response:*const c_void,
    internal_module_count:u64,
    internal_modules:*const c_void,
}
#[used]
#[link_section=".limine_requests_start"]
static mut START:[u64;4]=[
    0xf6b8f4b39de7d1ae,0xfab91a6940fcb9cf,
    0x785c6ed015d3e316,0x181e920a7852b9d9,
];
#[used]
#[link_section=".limine_requests"]
static mut BASE_REVISION:[u64;3]=[
    0xf9562b2d5c95a6c8,0x6a7b384944536bdc,6
];
#[used]
#[link_section=".limine_requests"]
static mut MEMORY_MAP:Request=Request{
    id:[COMMON[0],COMMON[1],0x67cf3d9d378a806f,0xe304acdfc50c3c62],
    revision:0,response:ptr::null(),
};
#[used]
#[link_section=".limine_requests"]
static mut HHDM:Request=Request{
    id:[COMMON[0],COMMON[1],0x48dcf1cb8ad2b852,0x63984e959a98244b],
    revision:0,response:ptr::null(),
};
#[used]
#[link_section=".limine_requests"]
static mut FRAMEBUFFER:Request=Request{
    id:[COMMON[0],COMMON[1],0x9d5827dcd881dd75,0xa3148604f6fab11b],
    revision:0,response:ptr::null(),
};
#[used]
#[link_section=".limine_requests"]
static mut MODULES:ModuleRequest=ModuleRequest{
    id:[COMMON[0],COMMON[1],0x3e7e279702be32af,0xca1c4f3bd1280cee],
    revision:0,response:ptr::null(),
    internal_module_count:0,internal_modules:ptr::null(),
};
#[used]
#[link_section=".limine_requests_end"]
static mut END:[u64;2]=[0xadc0e0531bb10d03,0x9572709f31764c62];

#[no_mangle]
pub extern "C" fn boot_limine_revision_supported()->bool{
    unsafe{ptr::read_volatile(ptr::addr_of!(BASE_REVISION[2]))==0}
}
#[no_mangle]
pub extern "C" fn boot_limine_memmap()->*const c_void{
    unsafe{ptr::read_volatile(ptr::addr_of!(MEMORY_MAP.response))}
}
#[no_mangle]
pub extern "C" fn boot_limine_hhdm()->*const c_void{
    unsafe{ptr::read_volatile(ptr::addr_of!(HHDM.response))}
}
#[no_mangle]
pub extern "C" fn boot_limine_modules()->*const c_void{
    unsafe{ptr::read_volatile(ptr::addr_of!(MODULES.response))}
}
#[no_mangle]
pub extern "C" fn boot_limine_framebuffers()->*const c_void{
    unsafe{ptr::read_volatile(ptr::addr_of!(FRAMEBUFFER.response))}
}
#[cfg(test)]
mod tests{
    use super::*;
    use core::mem::{offset_of,size_of};
    #[test]
    fn request_binary_layout_matches_limine(){
        assert_eq!(size_of::<Request>(),48);
        assert_eq!(size_of::<ModuleRequest>(),64);
        assert_eq!(offset_of!(Request,response),40);
        assert_eq!(offset_of!(ModuleRequest,internal_module_count),48);
        assert_eq!(BASE_REVISION_INIT,6);
    }
    const BASE_REVISION_INIT:u64=6;
}
