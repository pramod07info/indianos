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
// Limine MP protocol: APs remain waiting in the bootloader because the
// kernel never writes a goto_address. Reading CPU topology does NOT bring
// up an AP, install per-AP IDTs or enable remote TLB shootdowns.
#[repr(C)]
struct MpRequest {
    id:[u64;4],
    revision:u64,
    response:*const MpResponse,
    flags:u64,
}
#[repr(C)]
struct MpResponse {
    revision:u64,
    flags:u32,
    bsp_lapic_id:u32,
    cpu_count:u64,
    cpus:*const *const MpInfo,
}
#[repr(C)]
struct MpInfo {
    processor_id:u32,
    lapic_id:u32,
    reserved:u64,
    goto_address:*const c_void,
    extra_argument:u64,
}
#[used]
#[link_section=".limine_requests"]
static mut MP:MpRequest=MpRequest {
    id:[COMMON[0],COMMON[1],0x95a67b819a1b857e,0xa0b61b723b6a73e0],
    revision:0,response:ptr::null(),flags:0,
};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) struct MpInventory {
    pub cpus:u32,
    pub bsp_lapic_id:u32,
}
const MAX_BOOT_CPUS:usize=64;

/// R118-014/028 foundation: only validate Limine-supplied topology on
/// the bootstrap processor. Bootloader owns every listed pointer, and all
/// CPUs except the BSP MUST remain parked. No AP is started here.
///
/// SAFETY: the caller must be the BSP during initial Limine handoff, while
/// response pages and CPU pointer arrays are valid and immutable. A corrupt
/// bootloader pointer is not recoverable by a memory-safe parser.
unsafe fn validate_mp_response(response:&MpResponse)->Option<MpInventory>{
    let count=usize::try_from(response.cpu_count).ok()?;
    if count==0 || count>MAX_BOOT_CPUS || response.cpus.is_null() ||
       (response.cpus as usize)%core::mem::align_of::<*const MpInfo>()!=0 {
        return None;
    }
    let mut lapic_ids=[0u32;MAX_BOOT_CPUS];
    let mut processor_ids=[0u32;MAX_BOOT_CPUS];
    let mut bsp_found=0usize;
    for index in 0..count {
        // Trusted Limine pointer table only: never use this on untrusted
        // firmware bytes, from ring3, or after bootloader reclamation.
        let info_ptr=unsafe{ptr::read_volatile(response.cpus.add(index))};
        if info_ptr.is_null() ||
           (info_ptr as usize)%core::mem::align_of::<MpInfo>()!=0 {
            return None;
        }
        let info=unsafe{&*info_ptr};
        if lapic_ids[..index].contains(&info.lapic_id) ||
           processor_ids[..index].contains(&info.processor_id) {
            return None;
        }
        // A bootloader-armed AP would execute without an IndianOS-owned
        // GDT/IDT/TSS/stack. Reject such a handoff before treating it as
        // a safe, parked secondary processor.
        if info.lapic_id!=response.bsp_lapic_id &&
           !unsafe{ptr::read_volatile(ptr::addr_of!(info.goto_address))}.is_null() {
            return None;
        }
        bsp_found+=usize::from(info.lapic_id==response.bsp_lapic_id);
        lapic_ids[index]=info.lapic_id;
        processor_ids[index]=info.processor_id;
    }
    if bsp_found!=1{return None;}
    Some(MpInventory{cpus:count as u32,bsp_lapic_id:response.bsp_lapic_id})
}

/// Read-only topology discovery. No AP startup and no concurrency claim.
pub(crate) fn observed_mp_inventory()->Option<MpInventory>{
    let response=unsafe{ptr::read_volatile(ptr::addr_of!(MP.response))};
    if response.is_null() ||
       (response as usize)%core::mem::align_of::<MpResponse>()!=0 {
        return None;
    }
    unsafe{validate_mp_response(&*response)}
}

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

#[cfg(test)]
mod r125_mp_inventory_tests {
    use super::*;
    use core::mem::{offset_of,size_of};

    fn info(processor_id:u32,lapic_id:u32)->MpInfo{
        MpInfo{processor_id,lapic_id,reserved:0,
               goto_address:ptr::null(),extra_argument:0}
    }
    fn response(table:&[*const MpInfo],count:u64,bsp:u32)->MpResponse{
        MpResponse{revision:0,flags:0,bsp_lapic_id:bsp,
                   cpu_count:count,cpus:table.as_ptr()}
    }
    #[test]
    fn r125_mp_wire_layout_exact_limine_revision_six(){
        assert_eq!(size_of::<MpRequest>(),56);
        assert_eq!(size_of::<MpResponse>(),32);
        assert_eq!(size_of::<MpInfo>(),32);
        assert_eq!(offset_of!(MpRequest,response),40);
        assert_eq!(offset_of!(MpRequest,flags),48);
        assert_eq!(offset_of!(MpResponse,cpus),24);
        assert_eq!(offset_of!(MpInfo,goto_address),16);
    }
    #[test]
    fn r125_two_parked_cpus_have_one_authoritative_bsp(){
        let cpus=[info(0,0),info(1,1)];
        let table=[&cpus[0] as *const MpInfo,&cpus[1] as *const MpInfo];
        let data=response(&table,2,0);
        assert_eq!(unsafe{validate_mp_response(&data)},
                   Some(MpInventory{cpus:2,bsp_lapic_id:0}));
    }
    #[test]
    fn r125_duplicate_missing_bsp_and_invalid_count_fail_closed(){
        let cpus=[info(0,0),info(1,1)];
        let table=[&cpus[0] as *const MpInfo,&cpus[1] as *const MpInfo];
        for (count,bsp) in [(0,0),(65,0),(u64::MAX,0),(2,3)] {
            assert_eq!(unsafe{validate_mp_response(&response(&table,count,bsp))},None);
        }
        let duplicate=[&cpus[0] as *const MpInfo,&cpus[0] as *const MpInfo];
        assert_eq!(unsafe{validate_mp_response(&response(&duplicate,2,0))},None);
        let no_table=MpResponse{cpus:ptr::null(),..response(&table,2,0)};
        assert_eq!(unsafe{validate_mp_response(&no_table)},None);
        let same_id=[info(0,0),info(0,1)];
        let same_ptr=[&same_id[0] as *const MpInfo,&same_id[1] as *const MpInfo];
        assert_eq!(unsafe{validate_mp_response(&response(&same_ptr,2,0))},None);
    }
    #[test]
    fn r125_prestarted_ap_is_not_qualified_as_parked(){
        let bsp=info(0,0);
        let mut ap=info(1,1);
        ap.goto_address=1usize as *const c_void;
        let table=[&bsp as *const MpInfo,&ap as *const MpInfo];
        assert_eq!(unsafe{validate_mp_response(&response(&table,2,0))},None);
    }
}
