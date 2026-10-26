//! M2 signed persisted app -> real detached ring-3 UserImage staging.
//! Bounded kernel-only integration. This does NOT perform IRETQ, CR3 switch,
//! scheduler activation, third-party app execution or key provisioning.
//! The host tests' verifier is NEVER suitable for production trust.
use crate::block::BlockDevice;
use crate::batch4_signature::{Root,Verify};
use crate::batch5_session::Identity;
use crate::batch6_security::Access;
use crate::m2_inventory::{Inventory,Record,Error as InventoryError,MAX_IMAGE_BYTES};
use crate::user_image::{UserImage,indianos_user_image_load};
use crate::vmm::Backend;
use crate::m2_consent::Ledger;
use crate::{Process,indianos_process_ready,indianos_process_bind};

#[derive(Debug,PartialEq,Eq)]
pub enum Error{Busy,Stored(InventoryError),InvalidElf,MappingFailed,InvalidProcess,
 GuestExecutionFailed,ConsentDenied}

/// The caller is a trusted supervisor and owns an uninitialized detached
/// UserImage. The backend must allocate/access/release real kernel-managed
/// pages. The caller must ensure an exclusive, lifetime-stable backing store.
/// No caller-provided "verified" boolean can bypass the installed image
/// signature check. On failure the image is not activated or queued.
///
/// The returned identity is the installed app/version/digest, NOT a PID grant.
/// PID creation, stack registers, IRETQ and subsequent health confirmation
/// remain the scheduler's separate responsibility.
pub fn stage_verified<D:BlockDevice,V:Verify,const N:usize>(
    inventory:&mut Inventory<D>, auth:&Access<N>, owner:Identity, now:u64,
    roots:&[Root], verifier:&V, id:u32,
    image:&mut UserImage, backend:&Backend,
)->Result<Record,Error>{
    if image.tables.initialized||image.ready{return Err(Error::Busy);}
    let mut bytes=[0u8;MAX_IMAGE_BYTES];
    let (record,size)=inventory.verified_image(
        auth,owner,now,roots,verifier,id,&mut bytes
    ).map_err(Error::Stored)?;
    // Both validation and PMM mapping use the exact same strict shared
    // ELF planner, so an image accepted at install cannot later disagree
    // on offset alignment, W^X, executable entry or virtual address range.
    crate::m2_elf::validate_static(&bytes[..size])
        .map_err(|_|Error::InvalidElf)?;
    // SAFETY: local bytes are available for the synchronous full copy;
    // image is exclusive and uninitialized. Backend is a trusted kernel
    // allocator and maps valid supervisor-accessible physical frames.
    let mapped=unsafe{
        indianos_user_image_load(image as *mut UserImage,
            backend as *const Backend,bytes.as_ptr(),size)
    };
    if !mapped||!image.ready{return Err(Error::MappingFailed);}
    Ok(record)
}

/// Supervisor-only receipt for a real PCB with a verified, mapped image.
/// This is NOT execution and does not authorize the process to invoke syscalls.
/// The owning runtime must retain the detached UserImage until a true
/// ring-3 context switch and must recover kernel CR3 before teardown.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PreparedProcess{
    pub identity:Identity,
    pub app:Record,
    pub page_root:u64,
    pub entry:u64,
    pub stack_top:u64,
}

/// Bridge stored/signed executable staging to the actual IndianOS PCB
/// transitions NEW -> READY with a bound VMM address-space root.
/// The kernel process table must have created `process`, the trusted
/// supervisor must supply its nonzero generation, and no second thread
/// may mutate the PCB. No PID is invented and no user CPU execution occurs.
///
/// Mapping is committed only after signature, ELF, and process preflights.
/// Failed trust checks cannot create READY processes or allocate frames.
pub fn prepare_registered<D:BlockDevice,V:Verify,const N:usize>(
    inventory:&mut Inventory<D>,auth:&Access<N>,owner:Identity,now:u64,
    roots:&[Root],verifier:&V,id:u32,
    process:&mut Process,generation:u64,
    image:&mut UserImage,backend:&Backend,
)->Result<PreparedProcess,Error>{
    if process.pid==0||generation==0||process.state!=crate::NEW||
       process.address_space_root!=0||process.resources_reclaimed||
       image.tables.initialized||image.ready{
        return Err(Error::InvalidProcess);
    }
    let package=stage_verified(
        inventory,auth,owner,now,roots,verifier,id,image,backend
    )?;
    let root=image.tables.root_physical;
    let entry=image.plan.entry;
    let stack_top=image.plan.stack_end;
    // The detached VMM supplies a real page-aligned root. If its contract
    // was violated, release the image without touching the NEW PCB.
    if root==0||root&4095!=0||entry==0||stack_top<=entry{
        // SAFETY: the image is detached and no CPU has switched to its CR3.
        let _=unsafe{crate::user_image::indianos_user_image_destroy(image)};
        return Err(Error::MappingFailed);
    }
    // SAFETY: process is exclusively owned by the trusted supervisor and
    // was checked to be a newly allocated PCB. There is no concurrent CPU.
    if !unsafe{indianos_process_ready(process)}{
        let _=unsafe{crate::user_image::indianos_user_image_destroy(image)};
        return Err(Error::InvalidProcess);
    }
    // This cannot fail under the preflight assumptions. If it does, the
    // unbound READY PCB is not schedulable. The owner must retire its slot.
    if !unsafe{indianos_process_bind(process,root)}{
        let _=unsafe{crate::user_image::indianos_user_image_destroy(image)};
        return Err(Error::MappingFailed);
    }
    Ok(PreparedProcess{
        identity:Identity{pid:process.pid,generation},
        app:package,page_root:root,entry,stack_top,
    })
}

/// Re-verify the exact installed bytes and enforce NEW-PCB state *before*
/// handing any byte or CPU control to the native ring-3 runtime. This is
/// separate from `prepare_registered`, which already binds a READY PCB.
///
/// Caller supplies a kernel-created, exclusive PCB and trusted INSTALL
/// capability; the signature verifier must be the guest's real
/// digest+signature verifier, never the host-test stand-in.
fn verified_for_execution<D:BlockDevice,V:Verify,const N:usize>(
    inventory:&mut Inventory<D>,auth:&Access<N>,owner:Identity,now:u64,
    roots:&[Root],verifier:&V,id:u32,process:&Process,
    bytes:&mut [u8;MAX_IMAGE_BYTES],
)->Result<(Record,usize),Error>{
    if process.pid==0||process.state!=crate::NEW||
       process.address_space_root!=0||process.resources_reclaimed{
        return Err(Error::InvalidProcess);
    }
    let (app,n)=inventory.verified_image(auth,owner,now,roots,verifier,id,bytes)
        .map_err(Error::Stored)?;
    crate::m2_elf::validate_static(&bytes[..n])
        .map_err(|_|Error::InvalidElf)?;
    if crate::program::plan_elf64(&bytes[..n]).is_none(){
        return Err(Error::InvalidElf);
    }
    Ok((app,n))
}

/// Experimental true CPU ring-3 entry for a *signed installed* static ELF.
/// Only an opt-in bare-metal x86-64 kernel can compile this entry point.
///
/// This reuses `x86_64_user_process_run` (NX/W^X PMM pages, supervisor
/// kernel mappings, TSS stack, CR3/IRETQ, INT 0x80, fault containment,
/// return-to-kernel CR3, frame reclamation and PCB reap) instead of making
/// a second unsafe trampoline. It operates on NEW PCBs only and MUST NOT
/// be called on a PCB from `prepare_registered`.
///
/// A successful return means the existing runtime reported a clean guest
/// exit and release. Production deployment still requires protected guest
/// trust keys and actual on-device Ed25519. This is intentionally not
/// exported as a generic user syscall or enabled for regular boot.
/// Only the trusted kernel supervisor may invoke it, on its single BSP.
#[cfg(all(feature="user-runtime",target_arch="x86_64",target_os="none"))]
pub(crate) fn execute_installed<D:BlockDevice,V:Verify,const N:usize>(
    inventory:&mut Inventory<D>,auth:&Access<N>,owner:Identity,now:u64,
    roots:&[Root],verifier:&V,id:u32,process:&mut Process,
)->Result<Record,Error>{
    let mut bytes=[0u8;MAX_IMAGE_BYTES];
    let (app,size)=verified_for_execution(
        inventory,auth,owner,now,roots,verifier,id,process,&mut bytes
    )?;
    // SAFETY: this call is valid only from the trusted single-core supervisor
    // after paging/PMM/IDT/TSS setup. Its pointer and byte slice remain alive
    // for the entire synchronous user process and exit/teardown. The native
    // runtime separately preflights kernel CR3, NXE and mappings.
    let completed=unsafe{
        crate::user_runtime::x86_64_user_process_run(
            process as *mut Process,bytes.as_ptr(),size
        )
    };
    if completed{Ok(app)}else{Err(Error::GuestExecutionFailed)}
}

/// Production-facing gate for an installed native process: re-verify on-disk
/// signed bytes + SHA-256 + dependency closure, require explicit administrator
/// approval of exactly THIS process identity and package digest/rights,
/// and only then enter the existing protected ring-3 runtime.
///
/// This is *not* a user syscall. Caller must be the privileged single-BSP
/// supervisor with exclusive NEW PCB ownership. It must retire a process's
/// approval during reaping and enforce actual rights on subsequent syscalls.
/// The signature provider MUST be a real cryptographic provider in releases.
#[cfg(all(feature="user-runtime",target_arch="x86_64",target_os="none"))]
pub fn execute_installed_approved<
    D:BlockDevice,V:Verify,const N:usize,const C:usize
>(
    inventory:&mut Inventory<D>,auth:&Access<N>,owner:Identity,now:u64,
    roots:&[Root],verifier:&V,id:u32,process:&mut Process,
    generation:u64,consent:&Ledger<C>,
)->Result<Record,Error>{
    if generation==0{return Err(Error::ConsentDenied);}
    let mut bytes=[0u8;MAX_IMAGE_BYTES];
    let (record,size)=verified_for_execution(
        inventory,auth,owner,now,roots,verifier,id,process,&mut bytes
    )?;
    let process_identity=Identity{pid:process.pid,generation};
    consent.require(process_identity,record,now)
        .map_err(|_|Error::ConsentDenied)?;
    // Scope the live syscall gate to the currently verified, approved NEW
    // PCB and remove it on either normal exit or guest failure.
    let policy=crate::m2_syscall_guard::Policy::new(
        process_identity,record.rights
    ).ok_or(Error::ConsentDenied)?;
    let armed=crate::m2_syscall_guard::arm(policy).ok_or(Error::Busy)?;
    // SAFETY: The single-core trusted supervisor exclusively owns a NEW
    // PCB; bytes are in kernel memory and live until the synchronous runner
    // restores kernel CR3 and finishes EXIT/FAULT/REAP cleanup.
    let completed=unsafe{
        crate::user_runtime::x86_64_user_process_run(
            process as *mut Process,bytes.as_ptr(),size
        )
    };
    drop(armed); // released after synchronous ring3 exit/fault in kernel CR3
    if completed{Ok(record)}else{Err(Error::GuestExecutionFailed)}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockError,SECTOR_BYTES};
    use crate::hier_vfs::HierVfs;
    use crate::batch4_signature::{Envelope};
    use crate::batch6_security::{INSTALL};
    use crate::m2_sdk::{ABI,Package};
    use crate::user_image::{indianos_user_image_destroy,indianos_user_copy_readonly};
    use core::ffi::c_void;
    use std::boxed::Box;
    use std::vec::Vec;

    const SECTORS:usize=100;
    struct Disk{sectors:[[u8;SECTOR_BYTES];SECTORS]}
    impl BlockDevice for Disk{
        fn sector_count(&self)->u64{SECTORS as u64}
        fn read_sector(&mut self,lba:u64,out:&mut[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            *out=*self.sectors.get(lba as usize).ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self,lba:u64,src:&[u8;SECTOR_BYTES])
            ->Result<(),BlockError>{
            *self.sectors.get_mut(lba as usize).ok_or(BlockError::OutOfRange)?=*src;
            Ok(())
        }
    }
    struct OnlySample;
    impl Verify for OnlySample{
        fn verify_digest_and_signature(&self,e:&Envelope,bytes:&[u8])->bool{
            // Explicit unit-test fake. Guest crypto and key trust still absent.
            e.signature[0]==42&&e.digest==crate::m2_sha256::digest(bytes)&&
            bytes==crate::m2_elf::tests::sample()
        }
    }
    struct Deny;
    impl Verify for Deny{
        fn verify_digest_and_signature(&self,_:&Envelope,_:&[u8])->bool{false}
    }
    fn identity()->Identity{Identity{pid:4,generation:1}}
    fn grants()->Access<1>{
        let mut a=Access::new();
        a.grant(identity(),INSTALL,100).unwrap();
        a
    }
    fn roots()->[Root;1]{[Root{signer:7,epoch:2,enabled:true}]}
    fn envelope()->Envelope{Envelope{signer:7,epoch:2,
     digest:crate::m2_sha256::digest(&crate::m2_elf::tests::sample()),signature:[42;64]}}
    fn package()->Package{
        Package{id:9,version:1,abi:ABI,rights:0,deps:[0;4],
         payload_digest:crate::m2_sha256::digest(&crate::m2_elf::tests::sample())}
    }
    fn installed()->Inventory<Disk>{
        let disk=Disk{sectors:[[0;SECTOR_BYTES];SECTORS]};
        let mut inventory=Inventory::from_mounted(HierVfs::format_blank(disk).unwrap());
        inventory.init_directory().unwrap();
        inventory.install_image(&grants(),identity(),1,&roots(),&OnlySample,
            &envelope(),&crate::m2_elf::tests::sample(),package()).unwrap();
        Inventory::mount(inventory.into_device()).unwrap()
    }
    struct Pool{frames:Vec<Box<[u8;4096]>>,next:usize,released:usize}
    impl Pool{
        fn new()->Self{
            let mut frames=Vec::new();
            for _ in 0..48{frames.push(Box::new([0u8;4096]));}
            Self{frames,next:0,released:0}
        }
        fn backend(&mut self)->Backend{
            Backend{
                allocate:Some(allocate),release:Some(release),
                access:Some(access),context:(self as *mut Pool).cast()
            }
        }
    }
    unsafe extern "C" fn allocate(out:*mut u64,ctx:*mut c_void)->bool{
        let p=unsafe{&mut*(ctx as *mut Pool)};
        if out.is_null()||p.next==p.frames.len(){return false;}
        p.next+=1;
        unsafe{*out=(p.next as u64)*4096};
        true
    }
    unsafe extern "C" fn release(pa:u64,ctx:*mut c_void)->bool{
        let p=unsafe{&mut*(ctx as *mut Pool)};
        if pa==0||pa%4096!=0||pa/4096>p.next as u64{return false;}
        p.released+=1;true
    }
    unsafe extern "C" fn access(pa:u64,ctx:*mut c_void)->*mut c_void{
        let p=unsafe{&mut*(ctx as *mut Pool)};
        if pa==0||pa%4096!=0{return core::ptr::null_mut();}
        p.frames.get_mut((pa/4096-1) as usize)
            .map_or(core::ptr::null_mut(),|b|b.as_mut_ptr().cast())
    }
    fn empty_image()->UserImage{
        // This exact zero-initialization is also used by the existing
        // user_image unit tests; no pointer fields are dereferenced.
        unsafe{core::mem::zeroed()}
    }
    #[test]fn m2_signed_installed_image_is_mapped_into_detached_ring3_memory(){
        let mut store=installed();
        let mut pool=Pool::new();
        let backend=pool.backend();
        let mut image=empty_image();
        let record=stage_verified(&mut store,&grants(),identity(),1,
           &roots(),&OnlySample,9,&mut image,&backend).unwrap();
        assert_eq!(record.id,9);
        assert_eq!(record.version,1);
        assert_eq!(image.plan.entry,0x400078);
        assert!(image.ready);
        assert!(image.page_count>1);
        let mut code=[0u8;4];
        assert!(unsafe{indianos_user_copy_readonly(&image,0x400078,
           code.as_mut_ptr().cast(),4)});
        assert_eq!(code,[7,8,9,10]);
        assert_eq!(stage_verified(&mut store,&grants(),identity(),1,
           &roots(),&OnlySample,9,&mut image,&backend),Err(Error::Busy));
        assert!(unsafe{indianos_user_image_destroy(&mut image)});
        assert_eq!(pool.released,pool.next);
    }
    #[test]fn m2_rejected_or_stale_signatures_cannot_allocate_user_pages(){
        let mut store=installed();
        let mut pool=Pool::new();
        let backend=pool.backend();
        let mut image=empty_image();
        assert_eq!(stage_verified(&mut store,&grants(),identity(),1,
            &roots(),&Deny,9,&mut image,&backend),
            Err(Error::Stored(InventoryError::Trust(
                crate::batch4_signature::Error::InvalidSignature))));
        assert_eq!(pool.next,0);
        assert_eq!(stage_verified(&mut store,&grants(),
           Identity{generation:2,..identity()},1,
           &roots(),&OnlySample,9,&mut image,&backend),
           Err(Error::Stored(InventoryError::Denied)));
        assert_eq!(pool.next,0);
        assert!(!image.ready);
    }
    #[test]fn m2_verified_guest_image_registers_real_pcb_and_scheduler_selects_it(){
        let mut store=installed();
        let mut pool=Pool::new();
        let backend=pool.backend();
        let mut image=empty_image();
        let mut table=crate::ProcessTable::empty();
        let pcb=unsafe{crate::indianos_process_create(&mut table)};
        assert!(!pcb.is_null());
        // No fake "verified" bit. Signed bytes have already been checked
        // by the inventory verifier and copied into real mock VMM pages.
        let prepared=prepare_registered(&mut store,&grants(),identity(),1,
            &roots(),&OnlySample,9,unsafe{&mut*pcb},17,&mut image,&backend)
            .unwrap();
        assert_eq!(prepared.identity.pid,unsafe{(*pcb).pid});
        assert_eq!(prepared.identity.generation,17);
        assert_eq!(prepared.app.version,1);
        assert_eq!(prepared.page_root,image.tables.root_physical);
        assert_eq!(prepared.entry,0x400078);
        assert_eq!(unsafe{(*pcb).address_space_root},prepared.page_root);
        let mut scheduler=crate::Scheduler::empty();
        assert!(unsafe{crate::indianos_scheduler_init(&mut scheduler,&mut table)});
        let selected=unsafe{crate::indianos_scheduler_next(&mut scheduler)};
        assert_eq!(selected,pcb);
        assert!(unsafe{crate::indianos_process_start(selected,prepared.page_root)});
        // Host emulates a clean termination after returning to kernel CR3.
        assert!(unsafe{crate::indianos_process_exit(selected,0)});
        assert!(unsafe{indianos_user_image_destroy(&mut image)});
        assert!(unsafe{crate::indianos_process_reap(selected)});
        assert!(unsafe{crate::indianos_process_success(selected)});
        assert_eq!(pool.released,pool.next);
    }
    #[test]fn m2_invalid_pcb_and_denied_signature_never_register_or_map(){
        let mut store=installed();
        let mut pool=Pool::new();
        let backend=pool.backend();
        let mut image=empty_image();
        let mut table=crate::ProcessTable::empty();
        let pcb=unsafe{crate::indianos_process_create(&mut table)};
        assert!(!pcb.is_null());
        let invalid=prepare_registered(&mut store,&grants(),identity(),1,
            &roots(),&OnlySample,9,unsafe{&mut*pcb},0,&mut image,&backend);
        assert_eq!(invalid,Err(Error::InvalidProcess));
        assert_eq!(pool.next,0);
        let denied=prepare_registered(&mut store,&grants(),identity(),1,
            &roots(),&Deny,9,unsafe{&mut*pcb},17,&mut image,&backend);
        assert_eq!(denied,Err(Error::Stored(InventoryError::Trust(
            crate::batch4_signature::Error::InvalidSignature))));
        assert_eq!(pool.next,0);
        assert_eq!(unsafe{(*pcb).state},crate::NEW);
        let expired=prepare_registered(&mut store,&grants(),identity(),101,
            &roots(),&OnlySample,9,unsafe{&mut*pcb},17,&mut image,&backend);
        assert_eq!(expired,Err(Error::Stored(InventoryError::Denied)));
        assert_eq!(pool.next,0);
        assert_eq!(unsafe{(*pcb).state},crate::NEW);
        assert_eq!(unsafe{(*pcb).address_space_root},0);
        let mut scheduler=crate::Scheduler::empty();
        assert!(unsafe{crate::indianos_scheduler_init(&mut scheduler,&mut table)});
        assert!(unsafe{crate::indianos_scheduler_next(&mut scheduler)}.is_null());
    }
    #[test]fn m2_duplicate_ready_process_refuses_second_image_mapping(){
        let mut store=installed();
        let mut pool=Pool::new();
        let backend=pool.backend();
        let mut image=empty_image();
        let mut table=crate::ProcessTable::empty();
        let pcb=unsafe{crate::indianos_process_create(&mut table)};
        let _=prepare_registered(&mut store,&grants(),identity(),1,
            &roots(),&OnlySample,9,unsafe{&mut*pcb},3,&mut image,&backend)
            .unwrap();
        let before=pool.next;
        let mut secondary=empty_image();
        assert_eq!(prepare_registered(&mut store,&grants(),identity(),1,
            &roots(),&OnlySample,9,unsafe{&mut*pcb},3,&mut secondary,&backend),
            Err(Error::InvalidProcess));
        assert_eq!(pool.next,before);
        assert!(unsafe{indianos_user_image_destroy(&mut image)});
    }

    #[test]fn m2_stored_signed_elf_feeds_native_runtime_with_same_exact_bytes(){
        let mut inventory=installed();
        let mut table=crate::ProcessTable::empty();
        let pcb=unsafe{crate::indianos_process_create(&mut table)};
        assert!(!pcb.is_null());
        let mut bytes=[0u8;MAX_IMAGE_BYTES];
        let (app,n)=verified_for_execution(
            &mut inventory,&grants(),identity(),1,
            &roots(),&OnlySample,9,unsafe{&*pcb},&mut bytes
        ).unwrap();
        assert_eq!(app.id,9);
        assert_eq!(app.version,1);
        assert_eq!(n,124);
        assert_eq!(&bytes[..n],&crate::m2_elf::tests::sample());
        assert_eq!(unsafe{(*pcb).state},crate::NEW);
        assert_eq!(unsafe{(*pcb).address_space_root},0);
    }
    #[test]fn m2_execution_rejects_invalid_state_or_untrusted_image(){
        let mut inventory=installed();
        let mut table=crate::ProcessTable::empty();
        let pcb=unsafe{crate::indianos_process_create(&mut table)};
        assert!(!pcb.is_null());
        let mut out=[0u8;MAX_IMAGE_BYTES];
        assert_eq!(verified_for_execution(
            &mut inventory,&grants(),identity(),1,&roots(),&Deny,9,
            unsafe{&*pcb},&mut out),
            Err(Error::Stored(InventoryError::Trust(
                crate::batch4_signature::Error::InvalidSignature))));
        assert_eq!(verified_for_execution(
            &mut inventory,&grants(),Identity{generation:2,..identity()},
            1,&roots(),&OnlySample,9,unsafe{&*pcb},&mut out),
            Err(Error::Stored(InventoryError::Denied)));
        assert_eq!(verified_for_execution(
            &mut inventory,&grants(),identity(),100,&roots(),&OnlySample,9,
            unsafe{&*pcb},&mut out),
            Err(Error::Stored(InventoryError::Denied)));
        assert!(unsafe{crate::indianos_process_ready(pcb)});
        assert_eq!(verified_for_execution(
            &mut inventory,&grants(),identity(),1,&roots(),&OnlySample,9,
            unsafe{&*pcb},&mut out),Err(Error::InvalidProcess));
        assert_eq!(unsafe{(*pcb).address_space_root},0);
    }

}
