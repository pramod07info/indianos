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
pub(crate) const MAX_USER_COPY_OUT: usize = 72;

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
    #[cfg(feature="normal-desktop")]
    if crate::zero_page::physical()==Some(physical){return true;}
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
    // Record ownership before any mapping/access callback can fail. Zero VA
    // means an unmapped allocation awaiting release, already understood by
    // the retryable image destructor. Failed rollback must never forget RAM.
    let index=image.page_count as usize;
    image.pages[index]=UserPage{virtual_address:0,physical_address:phys};
    image.page_count+=1;
    let frame = unsafe { access(image, phys) };
    if frame.is_null() {
        unsafe{rollback_unmapped_stage(image,index)};
        return None;
    }
    // SAFETY: backend promises a full 4K mapped physical frame.
    for i in 0..PAGE {
        unsafe { ptr::write_volatile(frame.add(i), 0); }
    }
    if !unsafe { vmm_map_page(&mut image.tables, va, phys, perms) } {
        unsafe{rollback_unmapped_stage(image,index)};
        return None;
    }
    image.pages[index].virtual_address=va;
    Some(phys)
}

/// The reserved slot is the last staging entry and has never been mapped.
/// Keep it charged against MAX_IMAGE_PAGES when physical release refuses.
unsafe fn rollback_unmapped_stage(image:&mut UserImage,index:usize){
    let physical=image.pages[index].physical_address;
    if unsafe{release(image,physical)}{
        image.pages[index]=UserPage{virtual_address:0,physical_address:0};
        image.page_count=index as u16;
    }
}

/// Reserved stack slots remain charged in page_count and the launch budget.
/// Keeping the top leaf resident pins every table on this 2-MiB path, so
/// faults never need to replace a PML4 child borrowed by the active root.
#[cfg(feature="normal-desktop")]
fn reserved_stack_slot(image:&UserImage,va:u64,physical:u64)->bool{
 let begin=image.plan.stack_begin;let end=image.plan.stack_end;
 let Some(top)=end.checked_sub(PAGE as u64)else{return false;};
 image.ready&&image.page_count as usize<=MAX_IMAGE_PAGES&&
  (begin|end|va)&(PAGE as u64-1)==0&&begin<top&&
  end-begin<=crate::syscall::STACK_BYTES&&
  image.plan.guard_begin.checked_add(PAGE as u64)==Some(begin)&&
  begin>>21==top>>21&&va>=begin&&va<top&&
  image.pages[..image.page_count as usize].iter()
   .filter(|p|p.virtual_address==va).count()==1&&
  image.pages[..image.page_count as usize].iter()
   .any(|p|p.virtual_address==va&&p.physical_address==physical)
}
#[cfg(feature="normal-desktop")]
pub(crate) fn deferred_stack_page(image:&UserImage,va:u64)->bool{reserved_stack_slot(image,va,0)}
#[cfg(feature="normal-desktop")]
pub(crate) fn cow_stack_page(image:&UserImage,va:u64)->bool{
 crate::zero_page::physical().is_some_and(|physical|reserved_stack_slot(image,va,physical))
}

#[cfg(feature="normal-desktop")]
fn stack_mapping()->Mapping{Mapping{physical:0,permissions:Permissions{
 writable:false,executable:false,user:false}}}

/// Inactive image only, after successful full-footprint quota admission.
/// No table is freed: the last stack page keeps the shared path populated.
#[cfg(feature="normal-desktop")]
pub(crate) unsafe fn defer_stack_pages(image:&mut UserImage)->bool{
 let begin=image.plan.stack_begin;let end=image.plan.stack_end;
 let Some(top)=end.checked_sub(PAGE as u64)else{return false;};
 if !image.ready||image.page_count as usize>MAX_IMAGE_PAGES||
    (begin|end)&4095!=0||begin>=top||end-begin>crate::syscall::STACK_BYTES||
    begin>>21!=top>>21||image.plan.guard_begin.checked_add(PAGE as u64)!=Some(begin){return false;}
 // Validate the entire requested conversion before altering its first leaf.
 for va in (begin..end).step_by(PAGE){
  let mut records=image.pages[..image.page_count as usize].iter().filter(|p|p.virtual_address==va);
  let Some(page)=records.next()else{return false;};
  let mut mapping=stack_mapping();
  if records.next().is_some()||page.physical_address==0||
     !unsafe{vmm_query_page(&image.tables,va,&mut mapping)}||
     mapping.physical!=page.physical_address||!mapping.permissions.user||
     !mapping.permissions.writable||mapping.permissions.executable{return false;}
 }
 for index in 0..image.page_count as usize{
  let page=image.pages[index];
  if page.virtual_address<begin||page.virtual_address>=top{continue;}
  let data=unsafe{access(image,page.physical_address)};if data.is_null(){return false;}
  for byte in 0..PAGE{unsafe{ptr::write_volatile(data.add(byte),0)}}
  if !unsafe{vmm_unmap_page(&mut image.tables,page.virtual_address)}{return false;}
  image.pages[index].virtual_address=0;
  if !unsafe{release(image,page.physical_address)}{return false;}
  image.pages[index]=UserPage{virtual_address:page.virtual_address,physical_address:0};
 }
 true
}

/// Serialized active-image fault only. The sole repaired protection fault is
/// an exact write to an owned shared-zero RO/NX stack leaf. Invalidate the
/// active CPU TLB after success; every other protection/guard fault is terminal.
/// A refused rollback remains in caller-owned retained storage; the process
/// must retire and retry its release before image/PCB reclamation completes.
#[cfg(feature="normal-desktop")]
pub(crate) unsafe fn resolve_stack_fault(image:&mut UserImage,address:u64,error:u64,retained:&mut u64)->bool{
 let va=address&!(PAGE as u64-1);
 let cow=cow_stack_page(image,va);
 if *retained!=0||!(matches!(error,4|6)&&deferred_stack_page(image,va)||error==7&&cow){return false;}
 let mut top=stack_mapping();
 let top_va=image.plan.stack_end-PAGE as u64;
 if !unsafe{vmm_query_page(&image.tables,top_va,&mut top)}||
    !top.permissions.user||!top.permissions.writable||top.permissions.executable||
    !page_owned(image,top_va,top.physical){return false;}
 let mut existing=stack_mapping();
 let mapped=unsafe{vmm_query_page(&image.tables,va,&mut existing)};
 if cow{
  if !mapped||Some(existing.physical)!=crate::zero_page::physical()||
     !existing.permissions.user||existing.permissions.writable||existing.permissions.executable{return false;}
 }else if mapped{return false;}
 let Some(index)=image.pages[..image.page_count as usize].iter().position(|p|p.virtual_address==va)else{return false;};
 if error==4{
  if let Some(zero)=crate::zero_page::physical(){
   if !unsafe{vmm_map_page(&mut image.tables,va,zero,Permissions{writable:false,executable:false,user:true})}{return false;}
   image.pages[index].physical_address=zero;return true;
  }
 }
 let Some(physical)=(unsafe{allocate(image)})else{return false;};
 *retained=physical;
 let data=unsafe{access(image,physical)};
 let mapped=if data.is_null(){false}else{
  for byte in 0..PAGE{unsafe{ptr::write_volatile(data.add(byte),0)}}
  if cow{unsafe{crate::vmm::replace_readonly_user_page(&mut image.tables,va,existing.physical,physical)}}
  else{unsafe{vmm_map_page(&mut image.tables,va,physical,Permissions{writable:true,executable:false,user:true})}}
 };
 if mapped{image.pages[index].physical_address=physical;*retained=0;return true;}
 if unsafe{release(image,physical)}{*retained=0;}
 false
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
    if img.page_count as usize > MAX_IMAGE_PAGES { return false; }
    let mut ok = true;
    for i in (0..img.page_count as usize).rev() {
        let page = img.pages[i];
        // Preserve ownership across a failed teardown. Zero physical means
        // already released; zero virtual means unmapped, awaiting release.
        // Real user images never map virtual page zero or physical frame zero.
        if page.physical_address == 0 { continue; }
        if page.virtual_address != 0 {
            if !unsafe { vmm_unmap_page(&mut img.tables, page.virtual_address) } {
                ok = false;
                continue;
            }
            img.pages[i].virtual_address = 0;
        }
        if unsafe { release(img, page.physical_address) } {
            img.pages[i].physical_address = 0;
        } else {
            ok = false;
        }
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

fn planned_pages(plan:&ProgramPlan,limit:u16)->Option<u64>{
    if limit==0||limit as usize>MAX_IMAGE_PAGES||plan.segment_count==0||
       plan.segment_count as usize>MAX_SEGMENTS{return None;}
    if (plan.stack_begin|plan.stack_end)%PAGE as u64!=0{return None;}
    let stack=plan.stack_end.checked_sub(plan.stack_begin)?;
    if stack==0{return None;}
    let mut requested=stack/PAGE as u64;
    if requested>u64::from(limit){return None;}
    for segment in &plan.segments[..plan.segment_count as usize]{
        if (segment.page_begin|segment.page_end)%PAGE as u64!=0{return None;}
        let bytes=segment.page_end.checked_sub(segment.page_begin)?;
        if bytes==0{return None;}
        requested=requested.checked_add(bytes/PAGE as u64)?;
        if requested>u64::from(limit){return None;}
    }
    Some(requested)
}

/// Exact successful detached-image footprint: data/BSS/stack plus private
/// four-level tables. Guard holes and ELF source bytes allocate no data pages.
fn planned_private_frames(plan:&ProgramPlan,limit:u16)->Option<u64>{
    let data=planned_pages(plan,limit)?;let mut frames=data.checked_add(1)?;
    for shift in [21,30,39]{
        let mut prefixes=[0u64;MAX_IMAGE_PAGES];let mut count=0;
        for index in 0..=plan.segment_count as usize{
            let (mut address,end)=if index==plan.segment_count as usize{(plan.stack_begin,plan.stack_end)}
                else{(plan.segments[index].page_begin,plan.segments[index].page_end)};
            while address<end{
                let prefix=address>>shift;
                if !prefixes[..count].contains(&prefix){
                    if count==prefixes.len(){return None;}
                    prefixes[count]=prefix;count+=1;frames=frames.checked_add(1)?;
                }
                address=address.checked_add(PAGE as u64)?;
            }
        }
    }
    Some(frames)
}
/// # Safety: ELF bytes obey the same readable bounds as the image loader.
pub(crate) unsafe fn required_private_frames(elf:*const u8,size:usize,limit:u16)->Option<u64>{
    let mut plan=ProgramPlan{entry:0,stack_begin:0,stack_end:0,guard_begin:0,
        segment_count:0,segments:[ProgramSegment::empty();MAX_SEGMENTS]};
    if !unsafe{indianos_program_plan_elf64(elf,size,&mut plan)}{return None;}
    planned_private_frames(&plan,limit)
}

/// # Safety
/// \`image\` points to initialized, exclusively owned kernel memory;
/// \`backend\` contains trusted C function pointers, \`elf\` is readable
/// for \`size\` bytes (within 32 MiB), and none of these pointers alias.
#[no_mangle]
pub unsafe extern "C" fn indianos_user_image_load(
    image: *mut UserImage, backend: *const Backend, elf: *const u8, size: usize,
) -> bool {
    unsafe{indianos_user_image_load_with_limit(image,backend,elf,size,MAX_IMAGE_PAGES as u16)}
}
/// Trusted-kernel image admission. Limit covers ELF data/BSS and stack pages;
/// page tables, kernel buffers and shared mappings are separate resources.
/// # Safety: same exclusive pointer/backend contract as the legacy loader.
#[no_mangle]
pub unsafe extern "C" fn indianos_user_image_load_with_limit(
    image:*mut UserImage,backend:*const Backend,elf:*const u8,size:usize,page_limit:u16,
)->bool{
    if page_limit==0||page_limit as usize>MAX_IMAGE_PAGES{return false;}
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
    if planned_pages(&plan,page_limit).is_none(){return false;}
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
    unsafe{copy_read_bounded(image,address,destination,length,MAX_WRITE)}
}
unsafe fn copy_read_bounded(image:*const UserImage,address:u64,destination:*mut c_void,
 length:usize,limit:usize)->bool{
    let Some(img) = (unsafe { image.as_ref() }) else { return false; };
    if !img.ready || destination.is_null() || !(1..=limit).contains(&length) ||
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


pub(crate) unsafe fn indianos_user_can_write(
    img:&UserImage,address:u64,length:usize,
)->bool{
    // R025 IPC wire reply is 72 bytes; the dedicated syscall still validates
    // the complete writable range before consuming the queue.
    unsafe{can_write_bounded(img,address,length,MAX_USER_COPY_OUT)}
}
unsafe fn can_write_bounded(img:&UserImage,address:u64,length:usize,limit:usize)->bool{
    img.ready && (1..=limit).contains(&length) &&
        crate::syscall::user_range(&img.plan,address,length as u64,true) &&
        unsafe{copy_preflight(img,address,length,true)}
}

/// # Safety
/// Image and kernel source remain live; user addresses are translated using
/// owned page tables, not directly dereferenced.
pub(crate) unsafe fn indianos_user_copy_writable(
    img:&UserImage,address:u64,source:*const u8,length:usize,
)->bool{
    unsafe{copy_write_bounded(img,address,source,length,MAX_USER_COPY_OUT)}
}
unsafe fn copy_write_bounded(img:&UserImage,address:u64,source:*const u8,length:usize,limit:usize)->bool{
    if source.is_null() || !unsafe{can_write_bounded(img,address,length,limit)}{
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

/// A rejected request changes nothing. Partial means a mapping changed and
/// the runtime must terminate the caller rather than resume ambiguous state.
#[cfg(feature="normal-desktop")]
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub(crate) enum ScratchGuardError{Invalid,Partial}

/// Permanently reserve the two guards around app-owned pixel storage.
/// Data-frame ownership stays in UserImage until ordinary task destruction;
/// zero VA is the existing "unmapped, awaiting release" ownership state.
/// The caller must invalidate the active CR3 translations after this call.
#[cfg(feature="normal-desktop")]
pub(crate) unsafe fn install_pixel_scratch_guards(image:&mut UserImage,
    lower:u64,payload:u64,upper:u64)->Result<[u64;2],ScratchGuardError>{
    use ScratchGuardError::{Invalid,Partial};
    let size=PAGE as u64;
    let Some(end)=upper.checked_add(size)else{return Err(Invalid);};
    if !image.ready||image.page_count as usize>MAX_IMAGE_PAGES||
       image.plan.segment_count as usize>MAX_SEGMENTS||lower==0||
       lower&(size-1)!=0||lower.checked_add(size)!=Some(payload)||
       upper<=payload||upper%size!=0||
       upper-payload>crate::pixel_protocol::MAX_CANVAS_BYTES||lower>>39!=upper>>39||
       !crate::syscall::user_range(&image.plan,lower,end-lower,true){
        return Err(Invalid);
    }
    // Restrict this API to complete, real writable/NX ELF storage. It cannot
    // withdraw the current stack, code or loader's numerical page-tail slack.
    if !image.plan.segments.iter().take(image.plan.segment_count as usize)
        .any(|segment|segment.writable&&!segment.executable&&
            segment.memory_size!=0&&lower>=segment.virtual_address&&
            segment.virtual_address.checked_add(segment.memory_size)
                .is_some_and(|limit|end<=limit)){
        return Err(Invalid);
    }
    let mut guards=[(0usize,0u64);2];
    for va in (lower..end).step_by(PAGE) {
        let mut found=None;
        for (index,page) in image.pages[..image.page_count as usize].iter().enumerate(){
            if page.virtual_address==va{
                if found.is_some(){return Err(Invalid);}
                found=Some(index);
            }
        }
        let Some(index)=found else{return Err(Invalid);};
        let mut mapping=Mapping{physical:0,permissions:Permissions{
            writable:false,executable:false,user:false}};
        if !unsafe{vmm_query_page(&image.tables,va,&mut mapping)}||
           !mapping.permissions.user||!mapping.permissions.writable||mapping.permissions.executable||
           mapping.physical==0||mapping.physical!=image.pages[index].physical_address{
            return Err(Invalid);
        }
        if image.pages[..image.page_count as usize].iter()
            .filter(|page|page.physical_address==mapping.physical).count()!=1 {
            return Err(Invalid);
        }
        if va == lower { guards[0] = (index, mapping.physical); }
        if va == upper { guards[1] = (index, mapping.physical); }
    }
    for (position, &(index, _)) in guards.iter().enumerate() {
        let va=image.pages[index].virtual_address;
        if !unsafe{vmm_unmap_page(&mut image.tables,va)}{
            return Err(if position==0{Invalid}else{Partial});
        }
        image.pages[index].virtual_address=0;
    }
    Ok([guards[0].1,guards[1].1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{size_of, offset_of};
    struct Ram {
        frames: Vec<Box<[u64;512]>>, used: [bool;16], released: [u8;16],
        refuse_release: u64, refuse_access: u64,
        guard_leaf:usize,fail_after_guard_unmap:bool,
    }
    impl Ram {
        fn new()->Self {
            Self { frames:(0..16).map(|_|Box::new([0;512])).collect(),
                used:[false;16],released:[0;16],refuse_release:0,refuse_access:0,
                guard_leaf:usize::MAX,fail_after_guard_unmap:false }
        }
        fn index(physical:u64)->Option<usize> {
            if physical==0 || physical%PAGE as u64!=0 { return None; }
            let index=(physical/PAGE as u64-1) as usize;
            (index<16).then_some(index)
        }
    }
    unsafe extern "C" fn ram_alloc(out:*mut u64,ctx:*mut c_void)->bool {
        let ram=unsafe { &mut *ctx.cast::<Ram>() };
        let Some(index)=ram.used.iter().position(|&used|!used) else {return false;};
        ram.used[index]=true;
        unsafe { out.write((index as u64+1)*PAGE as u64); }
        true
    }
    unsafe extern "C" fn ram_release(physical:u64,ctx:*mut c_void)->bool {
        let ram=unsafe { &mut *ctx.cast::<Ram>() };
        if ram.refuse_release==physical { return false; }
        let Some(index)=Ram::index(physical) else {return false;};
        assert!(ram.used[index],"double release of {physical:x}");
        ram.used[index]=false;ram.released[index]+=1;
        true
    }
    unsafe extern "C" fn ram_access(physical:u64,ctx:*mut c_void)->*mut c_void {
        let ram=unsafe { &mut *ctx.cast::<Ram>() };
        if ram.fail_after_guard_unmap&&ram.guard_leaf<ram.frames.len()&&
            ram.frames[ram.guard_leaf][0]==0{return ptr::null_mut();}
        if ram.refuse_access==physical {return ptr::null_mut();}
        let Some(index)=Ram::index(physical) else {return ptr::null_mut();};
        if !ram.used[index] {return ptr::null_mut();}
        ram.frames[index].as_mut_ptr().cast()
    }
    fn staged(ram:&mut Ram)->UserImage {
        let backend=Backend {allocate:Some(ram_alloc),release:Some(ram_release),
            access:Some(ram_access),context:(ram as *mut Ram).cast()};
        let mut img:UserImage=unsafe {core::mem::zeroed()};
        assert!(unsafe {vmm_create(&mut img.tables,&backend)});
        let perms=Permissions {writable:true,executable:false,user:true};
        for va in [0x400000,0x401000] {assert!(unsafe {stage(&mut img,va,perms)}.is_some());}
        img.plan.stack_begin=0x400000;img.plan.stack_end=0x402000;
        img.ready=true;img
    }
    #[cfg(feature="normal-desktop")]
    fn demand_staged(ram:&mut Ram)->UserImage{
        let mut image=staged(ram);image.plan.guard_begin=image.plan.stack_begin-PAGE as u64;image
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn two_images_share_zero_reads_and_cow_write_does_not_change_peer(){
        let mut ram=Ram::new();let mut zero=0;
        assert!(unsafe{ram_alloc(&mut zero,(&mut ram as *mut Ram).cast())});
        crate::zero_page::with_test_frame(zero,||{
            let mut first=demand_staged(&mut ram);let mut second=demand_staged(&mut ram);
            let va=first.plan.stack_begin;let quota=(first.page_count,second.page_count);
            assert!(unsafe{defer_stack_pages(&mut first)&&defer_stack_pages(&mut second)});
            let before=ram.used.iter().filter(|&&v|v).count();let mut retained=0;
            assert!(unsafe{resolve_stack_fault(&mut first,va,4,&mut retained)});
            assert!(unsafe{resolve_stack_fault(&mut second,va,4,&mut retained)});
            assert_eq!(ram.used.iter().filter(|&&v|v).count(),before);
            let mut a=stack_mapping();let mut b=stack_mapping();
            assert!(unsafe{vmm_query_page(&first.tables,va,&mut a)&&vmm_query_page(&second.tables,va,&mut b)});
            assert_eq!((a.physical,b.physical),(zero,zero));assert!(!a.permissions.writable&&!b.permissions.writable);
            assert!(unsafe{resolve_stack_fault(&mut first,va,7,&mut retained)});
            assert!(unsafe{vmm_query_page(&first.tables,va,&mut a)});
            assert_ne!(a.physical,zero);assert!(a.permissions.writable&&!a.permissions.executable);
            ram.frames[Ram::index(a.physical).unwrap()][0]=0xfeed;
            assert!(ram.frames[Ram::index(zero).unwrap()].iter().all(|&v|v==0));
            assert_eq!((first.page_count,second.page_count),quota);
            assert!(unsafe{indianos_user_image_destroy(&mut first)});
            assert!(ram.used[Ram::index(zero).unwrap()]);
            assert!(unsafe{vmm_query_page(&second.tables,va,&mut b)});assert_eq!(b.physical,zero);
            assert!(unsafe{indianos_user_image_destroy(&mut second)});
            assert_eq!(ram.used.iter().filter(|&&v|v).count(),1);
        });
        assert!(unsafe{ram_release(zero,(&mut ram as *mut Ram).cast())});assert!(ram.used.iter().all(|&v|!v));
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn cow_oom_and_refused_rollback_leave_shared_leaf_unchanged(){
        let mut ram=Ram::new();let mut zero=0;
        assert!(unsafe{ram_alloc(&mut zero,(&mut ram as *mut Ram).cast())});
        crate::zero_page::with_test_frame(zero,||{
            let mut image=demand_staged(&mut ram);let va=image.plan.stack_begin;let mut retained=0;
            assert!(unsafe{defer_stack_pages(&mut image)&&resolve_stack_fault(&mut image,va,4,&mut retained)});
            let used=ram.used;ram.used.fill(true);
            assert!(!unsafe{resolve_stack_fault(&mut image,va,7,&mut retained)});
            assert_eq!(retained,0);ram.used=used;
            let free=ram.used.iter().position(|&v|!v).unwrap();let frame=(free as u64+1)*PAGE as u64;
            ram.refuse_access=frame;ram.refuse_release=frame;
            assert!(!unsafe{resolve_stack_fault(&mut image,va,7,&mut retained)});
            assert_eq!(retained,frame);
            let mut mapping=stack_mapping();assert!(unsafe{vmm_query_page(&image.tables,va,&mut mapping)});
            assert_eq!(mapping.physical,zero);assert!(!mapping.permissions.writable);
            ram.refuse_access=0;ram.refuse_release=0;
            assert!(unsafe{ram_release(retained,(&mut ram as *mut Ram).cast())});retained=0;
            assert!(unsafe{resolve_stack_fault(&mut image,va,7,&mut retained)});
            assert!(unsafe{indianos_user_image_destroy(&mut image)});
            assert!(ram.used[Ram::index(zero).unwrap()]);
        });
        assert!(unsafe{ram_release(zero,(&mut ram as *mut Ram).cast())});assert!(ram.used.iter().all(|&v|!v));
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn demand_stack_fault_restores_zero_nx_page_without_growing_quota(){
        let mut ram=Ram::new();let mut image=demand_staged(&mut ram);
        let count=image.page_count;let used=ram.used.iter().filter(|&&v|v).count();
        let va=image.plan.stack_begin;let top=image.plan.stack_end-PAGE as u64;
        assert!(unsafe{defer_stack_pages(&mut image)});
        assert!(deferred_stack_page(&image,va));assert_eq!(image.page_count,count);
        assert_eq!(ram.used.iter().filter(|&&v|v).count(),used-1);
        let mut mapping=stack_mapping();assert!(unsafe{vmm_query_page(&image.tables,top,&mut mapping)});
        let mut retained=0;
        assert!(unsafe{resolve_stack_fault(&mut image,va+17,6,&mut retained)});
        assert_eq!(retained,0);assert_eq!(image.page_count,count);
        assert!(unsafe{vmm_query_page(&image.tables,va,&mut mapping)});
        assert!(mapping.permissions.user&&mapping.permissions.writable&&!mapping.permissions.executable);
        let index=Ram::index(mapping.physical).unwrap();assert!(ram.frames[index].iter().all(|&v|v==0));
        assert_eq!(ram.used.iter().filter(|&&v|v).count(),used);
        assert!(!unsafe{resolve_stack_fault(&mut image,va,6,&mut retained)});
        assert!(unsafe{indianos_user_image_destroy(&mut image)});assert!(ram.used.iter().all(|&v|!v));
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn demand_stack_rejects_guard_nx_protection_and_foreign_mapping(){
        let mut ram=Ram::new();let mut image=demand_staged(&mut ram);
        assert!(unsafe{defer_stack_pages(&mut image)});let va=image.plan.stack_begin;
        let before=ram.used;let mut retained=0;
        for error in [0,1,2,3,5,7,12,20,36]{assert!(!unsafe{resolve_stack_fault(&mut image,va,error,&mut retained)});}
        for address in [0,image.plan.guard_begin,image.plan.stack_end,image.plan.stack_end-PAGE as u64]{
            assert!(!unsafe{resolve_stack_fault(&mut image,address,6,&mut retained)});
        }
        let physical=image.pages[1].physical_address;
        assert!(unsafe{vmm_map_page(&mut image.tables,va,physical,Permissions{user:true,writable:true,executable:false})});
        assert!(!unsafe{resolve_stack_fault(&mut image,va,6,&mut retained)});
        assert_eq!(ram.used,before);assert_eq!(retained,0);
        assert!(unsafe{vmm_unmap_page(&mut image.tables,va)});
        assert!(unsafe{resolve_stack_fault(&mut image,va,4,&mut retained)});
        assert!(unsafe{indianos_user_image_destroy(&mut image)});assert!(ram.used.iter().all(|&v|!v));
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn demand_fault_oom_and_failed_rollback_keep_exact_ownership(){
        let mut ram=Ram::new();let mut image=demand_staged(&mut ram);
        assert!(unsafe{defer_stack_pages(&mut image)});let va=image.plan.stack_begin;
        let used=ram.used;ram.used.fill(true);let mut retained=0;
        assert!(!unsafe{resolve_stack_fault(&mut image,va,6,&mut retained)});
        assert_eq!(retained,0);assert!(deferred_stack_page(&image,va));ram.used=used;
        let free=ram.used.iter().position(|&v|!v).unwrap();let frame=(free as u64+1)*PAGE as u64;
        ram.refuse_access=frame;ram.refuse_release=frame;
        assert!(!unsafe{resolve_stack_fault(&mut image,va,6,&mut retained)});
        assert_eq!(retained,frame);assert!(ram.used[free]);assert!(deferred_stack_page(&image,va));
        assert!(!unsafe{resolve_stack_fault(&mut image,va,6,&mut retained)});
        ram.refuse_access=0;ram.refuse_release=0;
        assert!(unsafe{ram_release(retained,(&mut ram as *mut Ram).cast())});
        assert!(unsafe{indianos_user_image_destroy(&mut image)});assert!(ram.used.iter().all(|&v|!v));
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn failed_stack_deferral_retains_unmapped_frame_for_image_teardown(){
        let mut ram=Ram::new();let mut image=demand_staged(&mut ram);
        let frame=image.pages[0].physical_address;ram.refuse_release=frame;
        assert!(!unsafe{defer_stack_pages(&mut image)});
        assert_eq!(image.pages[0].virtual_address,0);assert_eq!(image.pages[0].physical_address,frame);
        ram.refuse_release=0;assert!(unsafe{indianos_user_image_destroy(&mut image)});
        assert!(ram.used.iter().all(|&v|!v));
    }
    #[cfg(feature="normal-desktop")]
    fn pixel_staged(ram:&mut Ram)->UserImage{
        let mut img=staged(ram);
        let perms=Permissions{writable:true,executable:false,user:true};
        assert!(unsafe{stage(&mut img,0x402000,perms)}.is_some());
        // Numeric copy admission requires a valid stack layout even when
        // this ownership fixture only stages ELF data pages. Keep it away
        // from the scratch segment rather than disabling stack validation.
        img.plan.stack_begin=0x500000;img.plan.stack_end=0x501000;
        img.plan.segment_count=1;
        img.plan.segments[0]=ProgramSegment{virtual_address:0x400000,file_offset:0,file_size:0,
            memory_size:3*PAGE as u64,page_begin:0x400000,page_end:0x403000,
            writable:true,executable:false};
        img
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn private_pixel_guards_retain_frames_refuse_copies_and_release_exactly_once(){
        let mut ram=Ram::new();let mut img=pixel_staged(&mut ram);
        let owned=[img.pages[0].physical_address,img.pages[2].physical_address];
        assert_eq!(unsafe{install_pixel_scratch_guards(&mut img,0x400000,0x401000,0x402000)},Ok(owned));
        assert_eq!(img.pages[0].virtual_address,0);assert_eq!(img.pages[2].virtual_address,0);
        for frame in owned{assert!(ram.used[Ram::index(frame).unwrap()]);}
        let mut byte=[0xa5];
        for guard in [0x400000,0x402000]{
            assert!(!unsafe{indianos_user_copy_readonly(&img,guard,byte.as_mut_ptr().cast(),1)});
            assert!(!unsafe{indianos_user_can_write(&img,guard,1)});
            assert_eq!(byte,[0xa5]);
        }
        assert!(unsafe{indianos_user_copy_readonly(&img,0x401000,byte.as_mut_ptr().cast(),1)});
        assert_eq!(unsafe{install_pixel_scratch_guards(&mut img,0x400000,0x401000,0x402000)},Err(ScratchGuardError::Invalid));
        assert!(unsafe{indianos_user_image_destroy(&mut img)});
        assert!(ram.used.iter().all(|&used|!used));
        assert!(ram.released.iter().all(|&count|count<=1));
    }
    #[cfg(feature="normal-desktop")]
    #[test]
    fn multi_page_canvas_requires_owned_middle_pages_before_revoking_guards() {
        let mut ram = Ram::new();
        let mut image = pixel_staged(&mut ram);
        let perms = Permissions { writable:true, executable:false, user:true };
        assert!(unsafe { stage(&mut image, 0x403000, perms) }.is_some());
        image.plan.segments[0].memory_size += PAGE as u64;
        image.plan.segments[0].page_end += PAGE as u64;

        let middle_frame = image.pages[2].physical_address;
        image.pages[2].physical_address = image.pages[1].physical_address;
        let register = |img: &mut UserImage| unsafe {
            install_pixel_scratch_guards(img, 0x400000, 0x401000, 0x403000)
        };
        assert_eq!(register(&mut image), Err(ScratchGuardError::Invalid));
        assert!(image.pages[..4].iter().all(|page| page.virtual_address != 0));
        image.pages[2].physical_address = middle_frame;
        assert!(register(&mut image).is_ok());
        let mut pixels = [0xa5u8; 8];
        assert!(unsafe {
            indianos_user_copy_readonly(&image, 0x401ffc, pixels.as_mut_ptr().cast(), pixels.len())
        });
        assert_eq!(pixels, [0; 8]);
        assert!(unsafe { indianos_user_image_destroy(&mut image) });
        assert!(ram.used.iter().all(|&used| !used));
        assert!(ram.released.iter().all(|&count| count <= 1));
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn pixel_guard_admission_rejects_unowned_executable_and_outside_image_without_mutation(){
        let mut ram=Ram::new();let mut img=pixel_staged(&mut ram);
        for request in [(0x400001,0x401000,0x402000),(0x400000,0x402000,0x403000),
                        (0x401000,0x402000,0x403000),(0,0x1000,0x2000)]{
            assert_eq!(unsafe{install_pixel_scratch_guards(&mut img,request.0,request.1,request.2)},Err(ScratchGuardError::Invalid));
            assert!(img.pages[..3].iter().all(|page|page.virtual_address!=0));
        }
        img.plan.segments[0].executable=true;
        assert_eq!(unsafe{install_pixel_scratch_guards(&mut img,0x400000,0x401000,0x402000)},Err(ScratchGuardError::Invalid));
        img.plan.segments[0].executable=false;
        let physical=img.pages[2].physical_address;img.pages[2].physical_address=img.pages[0].physical_address;
        assert_eq!(unsafe{install_pixel_scratch_guards(&mut img,0x400000,0x401000,0x402000)},Err(ScratchGuardError::Invalid));
        img.pages[2].physical_address=physical;
        assert!(unsafe{indianos_user_image_destroy(&mut img)});
    }
    #[cfg(feature="normal-desktop")]
    #[test]fn partial_guard_failure_preserves_unmapped_frame_ownership_for_cleanup(){
        let mut ram=Ram::new();let mut img=pixel_staged(&mut ram);
        const MASK:u64=0x000f_ffff_ffff_f000;
        ram.guard_leaf=ram.frames.iter().position(|table|
            table[0]&MASK==img.pages[0].physical_address&&
            table[1]&MASK==img.pages[1].physical_address&&
            table[2]&MASK==img.pages[2].physical_address).unwrap();
        ram.fail_after_guard_unmap=true;
        assert_eq!(unsafe{install_pixel_scratch_guards(&mut img,0x400000,0x401000,0x402000)},Err(ScratchGuardError::Partial));
        assert_eq!(img.pages[0].virtual_address,0);assert_eq!(img.pages[2].virtual_address,0x402000);
        assert!(img.pages[..3].iter().all(|page|page.physical_address!=0));
        ram.fail_after_guard_unmap=false;
        assert!(unsafe{indianos_user_image_destroy(&mut img)});
        assert!(ram.used.iter().all(|&used|!used));
        assert!(ram.released.iter().all(|&count|count<=1));
    }
    #[test]
    fn failed_data_release_can_retry_without_reunmap_or_double_free() {
        let mut ram=Ram::new();let mut img=staged(&mut ram);
        let refused=img.pages[1].physical_address;
        ram.refuse_release=refused;
        assert!(!unsafe {indianos_user_image_destroy(&mut img)});
        assert!(!img.ready);assert!(img.tables.initialized);
        assert_eq!(img.pages[0].physical_address,0);
        assert_eq!(img.pages[1].virtual_address,0);
        assert_eq!(img.pages[1].physical_address,refused);
        assert!(!unsafe {indianos_user_can_write(&img,0x400000,1)});
        let mut out=[0xaa;4];
        assert!(!unsafe {indianos_user_copy_readonly(&img,0x400000,out.as_mut_ptr().cast(),4)});
        assert_eq!(out,[0xaa;4]);
        ram.refuse_release=0;
        assert!(unsafe {indianos_user_image_destroy(&mut img)});
        assert_eq!(img.page_count,0);assert!(!img.tables.initialized);
        assert!(ram.used.iter().all(|&used|!used));
        assert!(ram.released.iter().all(|&count|count<=1));
        assert!(!unsafe {indianos_user_image_destroy(&mut img)});
    }
    #[test]
    fn failed_root_access_and_release_preserve_retryable_image_ownership() {
        let mut ram=Ram::new();let mut img=staged(&mut ram);
        let root=img.tables.root_physical;
        ram.refuse_access=root;
        assert!(!unsafe {indianos_user_image_destroy(&mut img)});
        assert!(!img.ready);assert_eq!(img.page_count,2);
        assert!(img.pages[..2].iter().all(|p|p.virtual_address!=0&&p.physical_address!=0));
        ram.refuse_access=0;ram.refuse_release=root;
        assert!(!unsafe {indianos_user_image_destroy(&mut img)});
        assert!(img.pages[..2].iter().all(|p|p.physical_address==0));
        assert_eq!(img.tables.root_physical,root);
        ram.refuse_release=0;
        assert!(unsafe {indianos_user_image_destroy(&mut img)});
        assert!(ram.used.iter().all(|&used|!used));
    }
    #[test]
    fn invalid_page_count_denies_teardown_before_indexing_and_revokes_copy() {
        let mut ram=Ram::new();let mut img=staged(&mut ram);
        img.page_count=MAX_IMAGE_PAGES as u16+1;
        let before=ram.used;
        assert!(!unsafe {indianos_user_image_destroy(&mut img)});
        assert!(!img.ready);assert_eq!(ram.used,before);
        img.page_count=2;
        assert!(unsafe {indianos_user_image_destroy(&mut img)});
    }
    #[test]fn staging_access_failure_retains_a_refused_release_for_teardown_retry(){
        let mut ram=Ram::new();let mut image=staged(&mut ram);
        let before=image.page_count as usize;
        let next=ram.used.iter().position(|used|!*used).unwrap();let physical=(next as u64+1)*PAGE as u64;
        ram.refuse_access=physical;ram.refuse_release=physical;
        assert!(unsafe{stage(&mut image,0x402000,Permissions{writable:true,executable:false,user:true})}.is_none());
        assert_eq!(image.page_count as usize,before+1);
        assert_eq!((image.pages[before].virtual_address,image.pages[before].physical_address),(0,physical));
        assert!(ram.used[next]);ram.refuse_access=0;
        assert!(!unsafe{indianos_user_image_destroy(&mut image)});
        assert_eq!(image.pages[before].physical_address,physical);assert!(ram.used[next]);
        ram.refuse_release=0;assert!(unsafe{indianos_user_image_destroy(&mut image)});
        assert!(ram.used.iter().all(|used|!*used));assert!(ram.released.iter().all(|count|*count<=1));
    }
    #[test]fn conflicting_mapping_failure_preserves_existing_page_and_new_frame_ownership(){
        let mut ram=Ram::new();let mut image=staged(&mut ram);
        let before=image.page_count as usize;let original=image.pages[0].physical_address;
        let next=ram.used.iter().position(|used|!*used).unwrap();let physical=(next as u64+1)*PAGE as u64;
        ram.refuse_release=physical;
        assert!(unsafe{stage(&mut image,0x400000,Permissions{writable:true,executable:false,user:true})}.is_none());
        assert_eq!(image.pages[0].physical_address,original);
        assert_eq!((image.pages[before].virtual_address,image.pages[before].physical_address),(0,physical));
        assert!(ram.used[next]);assert_eq!(image.page_count as usize,before+1);
        ram.refuse_release=0;assert!(unsafe{indianos_user_image_destroy(&mut image)});
        assert!(ram.used.iter().all(|used|!*used));assert!(ram.released.iter().all(|count|*count<=1));
    }
    #[test]fn successful_staging_rollback_does_not_charge_an_unused_metadata_slot(){
        let mut ram=Ram::new();let mut image=staged(&mut ram);let count=image.page_count;
        let next=ram.used.iter().position(|used|!*used).unwrap();let physical=(next as u64+1)*PAGE as u64;
        ram.refuse_access=physical;
        assert!(unsafe{stage(&mut image,0x402000,Permissions{writable:true,executable:false,user:true})}.is_none());
        assert_eq!(image.page_count,count);assert!(!ram.used[next]);
        assert_eq!((image.pages[count as usize].virtual_address,image.pages[count as usize].physical_address),(0,0));
        ram.refuse_access=0;assert!(unsafe{indianos_user_image_destroy(&mut image)});
        assert!(ram.used.iter().all(|used|!*used));
    }
    #[test]fn retained_staging_frames_count_against_image_capacity_before_allocating(){
        let mut ram=Ram::new();let mut image=staged(&mut ram);let before=ram.used;
        let count=image.page_count;image.page_count=MAX_IMAGE_PAGES as u16;
        assert!(unsafe{stage(&mut image,0x402000,Permissions{writable:true,executable:false,user:true})}.is_none());
        assert_eq!(ram.used,before);image.page_count=count;
        assert!(unsafe{indianos_user_image_destroy(&mut image)});
    }
    #[test]fn image_budget_preflight_counts_stack_and_segments_without_underflow(){
        let mut plan=ProgramPlan{entry:0x400000,stack_begin:0x500000,stack_end:0x510000,
            guard_begin:0x4ff000,segment_count:1,segments:[ProgramSegment::empty();MAX_SEGMENTS]};
        plan.segments[0].page_begin=0x400000;plan.segments[0].page_end=0x404000;
        assert_eq!(planned_pages(&plan,20),Some(20));assert_eq!(planned_pages(&plan,19),None);
        assert_eq!(planned_pages(&plan,0),None);assert_eq!(planned_pages(&plan,129),None);
        plan.stack_end=0x600000;assert_eq!(planned_pages(&plan,128),None);
        plan.stack_end=0x4ff000;assert_eq!(planned_pages(&plan,128),None);
        plan.stack_end=0x510000;plan.segments[0].page_end=0x3ff000;
        assert_eq!(planned_pages(&plan,128),None);
        plan.segment_count=MAX_SEGMENTS as u32+1;assert_eq!(planned_pages(&plan,128),None);
    }
    #[test]fn private_frame_budget_counts_shared_prefixes_once_and_excludes_guard_holes(){
        let mut plan=ProgramPlan{entry:0x400000,stack_begin:0x500000,stack_end:0x510000,
            guard_begin:0x4ff000,segment_count:1,segments:[ProgramSegment::empty();MAX_SEGMENTS]};
        plan.segments[0].page_begin=0x400000;plan.segments[0].page_end=0x404000;
        // Twenty data pages; root plus one table at each child level.
        assert_eq!(planned_private_frames(&plan,20),Some(24));
        plan.stack_begin=0x800000;plan.stack_end=0x810000;plan.guard_begin=0x7ff000;
        assert_eq!(planned_private_frames(&plan,20),Some(25));
        plan.stack_begin=0x80000000;plan.stack_end=0x80010000;
        assert_eq!(planned_private_frames(&plan,20),Some(26));
        assert_eq!(planned_private_frames(&plan,19),None);
    }
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

    #[cfg(feature="normal-desktop")]
    #[test]fn g4_compositor_copy_extends_only_scoped_transport_and_checks_all_pages(){
        let mut ram=Ram::new();let mut img=staged(&mut ram);
        let address=0x400f80;let input=[0x73;512];let mut out=[0xab;512];
        assert!(!unsafe{indianos_user_can_write(&img,address,input.len())});
        assert!(!unsafe{indianos_user_copy_readonly(&img,address,out.as_mut_ptr().cast(),out.len())});
        assert_eq!(out,[0xab;512]);
        assert!(unsafe{compositor_can_write(&img,address,input.len())});
        assert!(unsafe{compositor_copy_out(&img,address,input.as_ptr(),input.len())});
        assert!(unsafe{compositor_copy_in(&img,address,out.as_mut_ptr(),out.len())});
        assert_eq!(out,input);
        for length in [0,crate::compositor_pixels::FRAME_COPY_LIMIT+1,usize::MAX]{
            assert!(!unsafe{compositor_can_write(&img,address,length)});
            assert!(!unsafe{compositor_copy_in(&img,address,out.as_mut_ptr(),length)});
        }
        let physical=img.pages[1].physical_address;
        assert!(unsafe{vmm_unmap_page(&mut img.tables,0x401000)});
        let first=Ram::index(img.pages[0].physical_address).unwrap();
        let prior=ram.frames[first].clone();let replacement=[0xaa;512];out.fill(0xcd);
        assert!(!unsafe{compositor_copy_in(&img,address,out.as_mut_ptr(),out.len())});
        assert_eq!(out,[0xcd;512]);
        assert!(!unsafe{compositor_copy_out(&img,address,replacement.as_ptr(),replacement.len())});
        assert_eq!(ram.frames[first],prior);
        assert!(unsafe{vmm_map_page(&mut img.tables,0x401000,physical,
            Permissions{writable:true,executable:false,user:true})});
        assert!(unsafe{indianos_user_image_destroy(&mut img)});
    }

    // R117: user pointers for syscall 33 are copied through the same
    // preflight as every live ring-3 descriptor. Crossing an unmapped page
    // must fail before the first byte of kernel output is modified.
    #[test]
    fn r117_guest_descriptor_cross_page_guard_is_fail_atomic() {
        let mut ram=Ram::new();
        let mut img=staged(&mut ram);
        let first=0x400ff8u64;
        let second_va=0x401000u64;
        let second_phys=img.pages[1].physical_address;
        let mut out=[0xabu8;32];
        assert!(unsafe{vmm_unmap_page(&mut img.tables,second_va)});
        assert!(!unsafe{indianos_user_copy_readonly(
            &img,first,out.as_mut_ptr().cast(),out.len())});
        assert_eq!(out,[0xab;32]);
        assert!(!unsafe{indianos_user_can_write(&img,first,32)});
        assert!(unsafe{vmm_map_page(&mut img.tables,second_va,second_phys,
            Permissions{writable:true,executable:false,user:true})});
        assert!(unsafe{indianos_user_image_destroy(&mut img)});
    }
    #[test]
    fn r117_guest_descriptor_unwritable_second_page_preserves_first_page() {
        let mut ram=Ram::new();
        let mut img=staged(&mut ram);
        let first=0x400ff8u64;
        let second_va=0x401000u64;
        let second_phys=img.pages[1].physical_address;
        let first_phys=img.pages[0].physical_address;
        let first_idx=Ram::index(first_phys).unwrap();
        let input=[0x73u8;32];
        // The first page is writable. The second page is mapped readable,
        // but must not allow a partial copy to the first eight bytes.
        let first_bytes=unsafe{
            core::slice::from_raw_parts(
                ram.frames[first_idx].as_ptr().cast::<u8>().add(PAGE-8),8)
        };
        assert_eq!(first_bytes,[0;8]);
        assert!(unsafe{vmm_unmap_page(&mut img.tables,second_va)});
        assert!(unsafe{vmm_map_page(&mut img.tables,second_va,second_phys,
            Permissions{writable:false,executable:false,user:true})});
        assert!(!unsafe{indianos_user_copy_writable(
            &img,first,input.as_ptr(),input.len())});
        let first_bytes=unsafe{
            core::slice::from_raw_parts(
                ram.frames[first_idx].as_ptr().cast::<u8>().add(PAGE-8),8)
        };
        assert_eq!(first_bytes,[0;8]);
        assert!(unsafe{vmm_unmap_page(&mut img.tables,second_va)});
        assert!(unsafe{vmm_map_page(&mut img.tables,second_va,second_phys,
            Permissions{writable:true,executable:false,user:true})});
        assert!(unsafe{indianos_user_copy_writable(
            &img,first,input.as_ptr(),input.len())});
        let mut out=[0u8;32];
        assert!(unsafe{indianos_user_copy_readonly(
            &img,first,out.as_mut_ptr().cast(),out.len())});
        assert_eq!(out,input);
        assert!(unsafe{indianos_user_image_destroy(&mut img)});
    }
    #[test]
    fn r117_guest_descriptor_noncanonical_and_guard_addresses_denied() {
        let mut ram=Ram::new();
        let mut img=staged(&mut ram);
        let mut out=[0x3cu8;64];
        for addr in [
            0u64,0x3fffc0,0x401fffu64,0x402000u64,
            0xffff_ffff_ffff_fff0u64,0x8000_0000_0000_0000u64,
        ]{
            assert!(!unsafe{indianos_user_copy_readonly(
                &img,addr,out.as_mut_ptr().cast(),out.len())});
            assert_eq!(out,[0x3c;64]);
            assert!(!unsafe{indianos_user_can_write(&img,addr,64)});
        }
        assert!(unsafe{indianos_user_image_destroy(&mut img)});
    }

}

/// Only authenticated compositor syscalls may use this larger, still bounded
/// transport. Generic file/IPC copies retain their original limits.
#[cfg(feature="normal-desktop")]
pub(crate) unsafe fn compositor_copy_in(img:*const UserImage,address:u64,out:*mut u8,n:usize)->bool{
 unsafe{copy_read_bounded(img,address,out.cast(),n,crate::compositor_pixels::FRAME_COPY_LIMIT)}
}
#[cfg(feature="normal-desktop")]
pub(crate) unsafe fn compositor_can_write(img:&UserImage,address:u64,n:usize)->bool{
 unsafe{can_write_bounded(img,address,n,crate::compositor_pixels::FRAME_COPY_LIMIT)}
}
#[cfg(feature="normal-desktop")]
pub(crate) unsafe fn compositor_copy_out(img:&UserImage,address:u64,source:*const u8,n:usize)->bool{
 unsafe{copy_write_bounded(img,address,source,n,crate::compositor_pixels::FRAME_COPY_LIMIT)}
}
