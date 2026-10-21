//! Boot-time detached VMM and PMM-backed ELF-image self tests.
//!
//! Replaces vmm_boot.c and user_image_boot.c only under feature "boot-staging".
//! The Limine protocol request structures still live in boot.c; these
//! functions import only the checked boot handoff's HHDM offset and reuse
//! the no_std Rust PMM/VMM/user-image implementations.

use core::{ffi::c_void, ptr};
use crate::vmm::{
    Backend, Space, Permissions, Mapping, vmm_create, vmm_map_page,
    vmm_query_page, vmm_unmap_page, vmm_destroy_empty,
};
use crate::pmm::{pmm_allocate_frame, pmm_release_frame, pmm_free_frames};
use crate::user_image::{
    UserImage, indianos_user_image_load, indianos_user_image_destroy,
};

extern "C" { fn boot_hhdm_offset(out: *mut u64) -> bool; }
#[repr(C)]
struct DirectMap { offset: u64 }
unsafe extern "C" fn allocate(physical: *mut u64, _: *mut c_void) -> bool {
    unsafe { pmm_allocate_frame(physical) }
}
unsafe extern "C" fn release(physical: u64, _: *mut c_void) -> bool {
    unsafe { pmm_release_frame(physical) }
}
unsafe extern "C" fn access(physical: u64, context: *mut c_void) -> *mut c_void {
    let Some(map) = (unsafe { (context as *const DirectMap).as_ref() }) else {
        return ptr::null_mut();
    };
    if physical == 0 || physical & 4095 != 0 { return ptr::null_mut(); }
    let Some(address) = physical.checked_add(map.offset) else {
        return ptr::null_mut();
    };
    if address >> 48 != 0xffff || (address & (1u64 << 47)) == 0 {
        return ptr::null_mut();
    }
    address as usize as *mut c_void
}
fn backend(map: &mut DirectMap) -> Backend {
    Backend {
        allocate: Some(allocate), release: Some(release), access: Some(access),
        context: (map as *mut DirectMap).cast(),
    }
}
#[no_mangle]
pub extern "C" fn vmm_boot_self_test() -> bool {
    let mut offset = 0;
    if !unsafe { boot_hhdm_offset(&mut offset) } || offset == 0 { return false; }
    let mut map = DirectMap { offset };
    let b = backend(&mut map);
    let mut space: Space = unsafe { core::mem::zeroed() };
    let before = pmm_free_frames();
    let mut data = 0u64;
    let mut mapped = false;
    let mut ok = unsafe { vmm_create(&mut space, &b) } &&
        unsafe { pmm_allocate_frame(&mut data) };
    if ok {
        let permissions = Permissions {
            writable: true, executable: false, user: false,
        };
        let va = 0xffff_fe00_0000_0000;
        mapped = unsafe { vmm_map_page(&mut space, va, data, permissions) };
        let mut query: Mapping = unsafe { core::mem::zeroed() };
        ok = mapped && unsafe { vmm_query_page(&space, va, &mut query) } &&
            query.physical == data && query.permissions.writable &&
            !query.permissions.executable && !query.permissions.user;
    }
    if mapped {
        ok = unsafe { vmm_unmap_page(&mut space, 0xffff_fe00_0000_0000) } && ok;
    }
    if space.initialized {
        ok = unsafe { vmm_destroy_empty(&mut space) } && ok;
    }
    if data != 0 {
        ok = unsafe { pmm_release_frame(data) } && ok;
    }
    ok && pmm_free_frames() == before
}
/// # Safety
/// The elf pointer references a readable, validated Limine initramfs
/// entry for \`image_bytes\`; the function does not activate user CR3.
#[no_mangle]
pub unsafe extern "C" fn user_image_boot_stage_test(
    elf: *const u8, image_bytes: usize,
) -> bool {
    if elf.is_null() || image_bytes == 0 { return false; }
    let mut offset = 0;
    if !unsafe { boot_hhdm_offset(&mut offset) } || offset == 0 { return false; }
    let mut map = DirectMap { offset };
    let b = backend(&mut map);
    let before = pmm_free_frames();
    let mut image: UserImage = unsafe { core::mem::zeroed() };
    if !unsafe { indianos_user_image_load(&mut image, &b, elf, image_bytes) } {
        return false;
    }
    let mut query: Mapping = unsafe { core::mem::zeroed() };
    let valid = unsafe {
        vmm_query_page(&image.tables, image.plan.entry & !4095, &mut query)
    } && query.permissions.user && query.permissions.executable &&
        !query.permissions.writable &&
        !unsafe { vmm_query_page(&image.tables, image.plan.guard_begin, &mut query) };
    if !unsafe { indianos_user_image_destroy(&mut image) } { return false; }
    valid && pmm_free_frames() == before
}
