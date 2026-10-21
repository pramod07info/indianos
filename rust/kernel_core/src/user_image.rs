//! PMM-backed, detached user ELF staging and user-copy (R003).
//!
//! Every data page belongs to this image, page tables belong to VMM, and
//! the caller's trusted backend supplies a supervisor mapping of physical
//! frames. These C ABI functions never activate CR3. The page-table clone
//! must be detached and kernel CR3 restored BEFORE destroying the image.

use core::{ptr, ffi::c_void};
use crate::program::{indianos_program_plan_elf64, indianos_program_user_range};
use crate::syscall::{ProgramPlan, ProgramSegment, MAX_SEGMENTS};
use crate::vmm::{
    Backend, Space, Mapping, Permissions, vmm_create, vmm_map_page,
    vmm_query_page, vmm_unmap_page, vmm_destroy_empty,
};

const PAGE: usize = 4096;
const MAX_IMAGE_PAGES: usize = 128;
const MAX_WRITE: usize = 256;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UserPage {
    pub virtual_address: u64,
    pub physical_address: u64,
}
#[repr(C)]
pub struct UserImage {
    pub tables: Space,
    pub plan: ProgramPlan,
    pub pages: [UserPage; MAX_IMAGE_PAGES],
    pub page_count: u16,
    pub ready: bool,
}

unsafe fn allocate(image: &UserImage) -> Option<u64> {
    let backend = &image.tables.backend;
    let mut frame = 0u64;
    if !unsafe { (backend.allocate?)(&mut frame, backend.context) } {
        return None;
    }
    Some(frame)
}
unsafe fn release(image: &UserImage, physical: u64) -> bool {
    let b = &image.tables.backend;
    let Some(release) = b.release else { return false; };
    unsafe { release(physical, b.context) }
}
unsafe fn access(image: &UserImage, physical: u64) -> *mut u8 {
    let b = &image.tables.backend;
    let Some(access) = b.access else { return ptr::null_mut(); };
    unsafe { access(physical, b.context).cast::<u8>() }
}

/// Allocate a data frame, initialize bytes through HHDM, map it into a
/// detached root and record ownership; page tables are managed by VMM.
unsafe fn stage(
    image: &mut UserImage, va: u64, perms: Permissions,
) -> Option<u64> {
    if image.page_count as usize >= MAX_IMAGE_PAGES {
        return None;
    }
    let phys = unsafe { allocate(image) }?;
    let frame = unsafe { access(image, phys) };
    if frame.is_null() {
        let _ = unsafe { release(image, phys) };
        return None;
    }
    // SAFETY: backend promises a full 4K mapped physical frame.
    for i in 0..PAGE {
        unsafe { ptr::write_volatile(frame.add(i), 0); }
    }
    if !unsafe { vmm_map_page(&mut image.tables, va, phys, perms) } {
        let _ = unsafe { release(image, phys) };
        return None;
    }
    let index = image.page_count as usize;
    image.pages[index] = UserPage { virtual_address: va, physical_address: phys };
    image.page_count += 1;
    Some(phys)
}

/// # Safety
/// \`image\` is an exclusive, initialized user-image pointer. CR3 must have
/// been restored and any kernel PML4 borrowing of user children detached.
#[no_mangle]
pub unsafe extern "C" fn indianos_user_image_destroy(
    image: *mut UserImage,
) -> bool {
    let Some(img) = (unsafe { image.as_mut() }) else { return false; };
    if !img.tables.initialized { return false; }
    // Revoke user-copy access before any frame is freed or unmapped.
    img.ready = false;
    let mut ok = true;
    for i in (0..img.page_count as usize).rev() {
        let page = img.pages[i];
        if !unsafe { vmm_unmap_page(&mut img.tables, page.virtual_address) } {
            ok = false;
            continue;
        }
        if !unsafe { release(img, page.physical_address) } { ok = false; }
    }
    if !ok { return false; } // preserve root metadata on partial failure
    if !unsafe { vmm_destroy_empty(&mut img.tables) } { return false; }
    img.page_count = 0;
    img.ready = false;
    // Valid representation includes normal padding; zero each semantic field.
    img.plan = ProgramPlan {
        entry: 0, stack_begin: 0, stack_end: 0, guard_begin: 0,
        segment_count: 0,
        segments: [ProgramSegment::empty(); MAX_SEGMENTS],
    };
    true
}

/// # Safety
/// \`image\` points to initialized, exclusively owned kernel memory;
/// \`backend\` contains trusted C function pointers, \`elf\` is readable
/// for \`size\` bytes (within 32 MiB), and none of these pointers alias.
#[no_mangle]
pub unsafe extern "C" fn indianos_user_image_load(
    image: *mut UserImage, backend: *const Backend, elf: *const u8, size: usize,
) -> bool {
    if image.is_null() || backend.is_null() || elf.is_null() { return false; }
    let img = unsafe { &mut *image };
    if img.tables.initialized || img.ready { return false; }
    let mut plan = ProgramPlan {
        entry: 0, stack_begin: 0, stack_end: 0, guard_begin: 0,
        segment_count: 0,
        segments: [ProgramSegment::empty(); MAX_SEGMENTS],
    };
    if !unsafe { indianos_program_plan_elf64(elf, size, &mut plan) } {
        return false;
    }
    let mut requested = (plan.stack_end - plan.stack_begin) / PAGE as u64;
    for seg in &plan.segments[..plan.segment_count as usize] {
        let count = (seg.page_end - seg.page_begin) / PAGE as u64;
        if count > MAX_IMAGE_PAGES as u64 - requested { return false; }
        requested += count;
    }
    if !unsafe { vmm_create(&mut img.tables, backend) } { return false; }
    img.plan = plan;
    img.page_count = 0;

    let mut ok = true;
    // Copy segment source bytes page-wise and leave the BSS tail zero.
    for segment in plan.segments.iter().take(plan.segment_count as usize) {
        if !ok { break; }
        let perm = Permissions {
            writable: segment.writable, executable: segment.executable, user: true,
        };
        let mut va = segment.page_begin;
        while va < segment.page_end {
            let Some(physical) = (unsafe { stage(img, va, perm) }) else {
                ok = false;
                break;
            };
            let start = va.max(segment.virtual_address);
            let file_end = segment.virtual_address + segment.file_size;
            let end = file_end.min(va + PAGE as u64);
            if start < end {
                let frame = unsafe { access(img, physical) };
                if frame.is_null() { ok = false; break; }
                let dest_offset = (start - va) as usize;
                let source_offset =
                    (segment.file_offset + start - segment.virtual_address) as usize;
                for i in 0..(end - start) as usize {
                    // SAFETY: ELF planner bounded the source and mapped frame.
                    let byte = unsafe { ptr::read(elf.add(source_offset + i)) };
                    unsafe { ptr::write_volatile(frame.add(dest_offset + i), byte); }
                }
            }
            va += PAGE as u64;
        }
    }
    let stack_perm = Permissions {
        writable: true, executable: false, user: true,
    };
    let mut va = plan.stack_begin;
    while va < plan.stack_end && ok {
        ok = unsafe { stage(img, va, stack_perm) }.is_some();
        va += PAGE as u64;
    }
    if ok {
        let mut query = Mapping {
            physical: 0, permissions: Permissions {
                writable: false, executable: false, user: false,
            },
        };
        let root = &img.tables;
        ok = !unsafe { vmm_query_page(root, plan.guard_begin, &mut query) } &&
             !unsafe { vmm_query_page(root, 0, &mut query) } &&
             unsafe { vmm_query_page(root, plan.entry & !(PAGE as u64 - 1), &mut query) } &&
             query.permissions.user && query.permissions.executable &&
             !query.permissions.writable;
    }
    if !ok {
        let _ = unsafe { indianos_user_image_destroy(image) };
        return false;
    }
    img.ready = true;
    true
}

/// # Safety
/// \`image\` is a valid live user image and \`destination\` a separate
/// writable kernel buffer of at least \`length\` bytes. User virtual
/// addresses are checked numerically, never dereferenced as raw pointers.
fn page_owned(image: &UserImage, va: u64, physical: u64) -> bool {
    if image.page_count as usize > MAX_IMAGE_PAGES { return false; }
    image.pages[..image.page_count as usize].iter().any(|page| {
        page.virtual_address == va && page.physical_address == physical
    })
}

/// Check the entire requested range before copying its first byte. A valid
/// PTE does not establish ownership of the underlying physical frame.
unsafe fn copy_preflight(image: &UserImage, address: u64, len: usize, write: bool) -> bool {
    let mut done = 0usize;
    while done < len {
        let Some(at) = address.checked_add(done as u64) else { return false; };
        let va = at & !(PAGE as u64 - 1);
        let offset = (at & (PAGE as u64 - 1)) as usize;
        let mut mapped = Mapping {
            physical: 0, permissions: Permissions {
                writable: false, executable: false, user: false,
            },
        };
        if !unsafe { vmm_query_page(&image.tables, va, &mut mapped) } ||
            !mapped.permissions.user || (write && !mapped.permissions.writable) ||
            (mapped.permissions.writable && mapped.permissions.executable) ||
            !page_owned(image, va, mapped.physical) {
            return false;
        }
        done += (PAGE - offset).min(len - done);
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn indianos_user_copy_readonly(
    image: *const UserImage, address: u64,
    destination: *mut c_void, length: usize,
) -> bool {
    let Some(img) = (unsafe { image.as_ref() }) else { return false; };
    if !img.ready || destination.is_null() || !(1..=MAX_WRITE).contains(&length) ||
        !unsafe { indianos_program_user_range(&img.plan, address, length as u64, false) } ||
        !unsafe { copy_preflight(img, address, length, false) } {
        return false;
    }
    let mut done = 0usize;
    while done < length {
        let Some(at) = address.checked_add(done as u64) else { return false; };
        let va = at & !(PAGE as u64 - 1);
        let offset = (at & (PAGE as u64 - 1)) as usize;
        let mut mapping = Mapping {
            physical: 0, permissions: Permissions {
                writable: false, executable: false, user: false,
            },
        };
        // Recheck in the copy pass; trap-gate syscall execution masks IRQs.
        if !unsafe { vmm_query_page(&img.tables, va, &mut mapping) } ||
            !mapping.permissions.user ||
            (mapping.permissions.writable && mapping.permissions.executable) ||
            !page_owned(img, va, mapping.physical) { return false; }
        let source = unsafe { access(img, mapping.physical) };
        if source.is_null() { return false; }
        let count = (PAGE - offset).min(length - done);
        for i in 0..count {
            let byte = unsafe { ptr::read(source.add(offset + i)) };
            unsafe { ptr::write(destination.cast::<u8>().add(done + i), byte); }
        }
        done += count;
    }
    true
}


#[cfg(feature="test-interactive")]
pub(crate) unsafe fn indianos_user_can_write(
    img:&UserImage,address:u64,length:usize,
)->bool{
    img.ready && (1..=64).contains(&length) &&
        crate::syscall::user_range(&img.plan,address,length as u64,true) &&
        unsafe{copy_preflight(img,address,length,true)}
}

/// # Safety
/// Image and kernel source remain live; user addresses are translated using
/// owned page tables, not directly dereferenced.
#[cfg(feature="test-interactive")]
pub(crate) unsafe fn indianos_user_copy_writable(
    img:&UserImage,address:u64,source:*const u8,length:usize,
)->bool{
    if source.is_null() || !unsafe{indianos_user_can_write(img,address,length)}{
        return false;
    }
    let mut done=0usize;
    while done<length{
        let Some(at)=address.checked_add(done as u64)else{return false;};
        let va=at&!(PAGE as u64-1);
        let offset=(at&(PAGE as u64-1)) as usize;
        let mut mapping=Mapping{
            physical:0,permissions:Permissions{writable:false,executable:false,user:false}
        };
        if !unsafe{vmm_query_page(&img.tables,va,&mut mapping)} ||
            !mapping.permissions.user||!mapping.permissions.writable||
            mapping.permissions.executable||!page_owned(img,va,mapping.physical){
            return false;
        }
        let dest=unsafe{access(img,mapping.physical)};
        if dest.is_null(){return false;}
        let count=(PAGE-offset).min(length-done);
        for j in 0..count{
            let byte=unsafe{ptr::read(source.add(done+j))};
            unsafe{ptr::write(dest.add(offset+j),byte);}
        }
        done+=count;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{size_of, offset_of};
    #[test]
    fn c_abi_and_user_copy_bounds() {
        assert_eq!(size_of::<UserPage>(), 16);
        assert_eq!(offset_of!(UserImage, plan), 48);
        assert_eq!(offset_of!(UserImage, pages), 536);
        assert_eq!(offset_of!(UserImage, page_count), 2584);
        assert_eq!(size_of::<UserImage>(), 2592);
        assert_eq!(MAX_WRITE, 256);
    }

    #[test]
    fn user_copy_rejects_foreign_frames_and_invalid_counts() {
        let mut img: UserImage = unsafe { core::mem::zeroed() };
        img.page_count = 2;
        img.pages[0] = UserPage { virtual_address: 0x400000, physical_address: 0x1000 };
        img.pages[1] = UserPage { virtual_address: 0x401000, physical_address: 0x2000 };
        assert!(page_owned(&img, 0x400000, 0x1000));
        assert!(page_owned(&img, 0x401000, 0x2000));
        assert!(!page_owned(&img, 0x400000, 0x2000));
        assert!(!page_owned(&img, 0x402000, 0x1000));
        assert!(!page_owned(&img, 0x401000, 0xffff_8000_0000_0000));
        img.page_count = 129;
        assert!(!page_owned(&img, 0x400000, 0x1000));
    }
}
