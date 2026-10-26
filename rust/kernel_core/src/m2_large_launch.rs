//! M2 Phase 5B: 64KiB signed bundle -> mapped, consent-approved user ELF.
//!
//! This is a privileged supervisor bridge between M2 Phase 5A separate
//! A/B blob partition and existing W^X page mapper / guarded native CPU
//! runtime. No unchecked installed package can become a runnable process.
//!
//! The caller supplies a single exclusively owned, kernel-addressable 64KiB
//! scratch buffer (NO 64KiB kernel-stack allocation). The caller MUST hold
//! exclusive access across verification and synchronous ring3 execution.
//! Verified bytes come from the *same* selected data bank and are checked
//! against an AUTHENTICATED signed manifest on every launch.
//!
//! The signed manifest must be retrieved independently from an authenticated
//! persistent catalog on reboot. This slice does NOT provide that catalog.
use crate::block::BlockDevice;
use crate::batch4_signature::{Root,Verify};
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,INSTALL};
use crate::m2_blob_store::{BlobStore,Error as StorageError,CAPACITY};
use crate::m2_bundle_manifest::{SignedBundle,Error as PackageError};
use crate::m2_inventory::Record;
use crate::m2_consent::Ledger;
use crate::user_image::{UserImage,indianos_user_image_load};
use crate::vmm::Backend;
use crate::{Process,indianos_process_ready,indianos_process_bind};

#[derive(Debug,PartialEq,Eq)]
pub enum Error{
 InvalidProcess,Denied,ConsentDenied,Signature(PackageError),
 Storage(StorageError),StaleBundle,InvalidElf,MapFailed,Busy,
 GuestExecutionFailed,
}
pub fn record(bundle:&SignedBundle)->Record{
 let m=bundle.manifest;
 Record{id:m.app,version:m.version,rights:m.rights,
  digest:m.digest,deps:m.deps,minimums:m.minimums}
}
/// Check every authorization boundary BEFORE any page allocation:
/// admin INSTALL capability, trusted publisher signature and key epoch,
/// currently selected durable bank identity/hash/version/size,
/// exact PID-generation approval and actual streaming disk integrity.
/// Then validate full static ELF plan and return content in external scratch.
/// On EVERY rejection the scratch is erased (including a failed disk read).
pub fn verified_large_approved<D:BlockDevice,V:Verify,
 const N:usize,const C:usize>(
 store:&mut BlobStore<D>,signed:&SignedBundle,auth:&Access<N>,
 supervisor:Identity,now:u64,roots:&[Root],verifier:&V,
 process:Identity,consent:&Ledger<C>,scratch:&mut[u8;CAPACITY]
)->Result<(Record,usize),Error>{
 scratch.fill(0);
 let verify=(||{
  if process.pid==0||process.generation==0{
   return Err(Error::InvalidProcess);
  }
  auth.require(supervisor,INSTALL,now).map_err(|_|Error::Denied)?;
  signed.verify(roots,verifier).map_err(Error::Signature)?;
  let m=signed.manifest;
  let actual=store.active().ok_or(Error::Storage(StorageError::NotFound))?;
  if actual.id!=m.app||actual.version!=m.version||
     actual.length!=m.length||actual.digest!=m.digest{
   return Err(Error::StaleBundle);
  }
  let app=record(signed);
  consent.require(process,app,now).map_err(|_|Error::ConsentDenied)?;
  let n=m.length as usize;
  store.verified_range(0,&mut scratch[..n]).map_err(Error::Storage)?;
  crate::m2_elf::validate_static(&scratch[..n])
    .map_err(|_|Error::InvalidElf)?;
  Ok((app,n))
 })();
 if verify.is_err(){scratch.fill(0);}
 verify
}
/// Map a fully authenticated larger ELF into detached PMM/VMM pages,
/// enforce NX/W^X and deny use of an already initialized image.
/// This does not place any PCB in READY state or execute a user process.
pub fn stage_large_verified<D:BlockDevice,V:Verify,
 const N:usize,const C:usize>(
 store:&mut BlobStore<D>,signed:&SignedBundle,auth:&Access<N>,
 supervisor:Identity,now:u64,roots:&[Root],verifier:&V,
 process:Identity,consent:&Ledger<C>,scratch:&mut[u8;CAPACITY],
 image:&mut UserImage,backend:&Backend
)->Result<Record,Error>{
 if image.tables.initialized||image.ready{
  scratch.fill(0);
  return Err(Error::Busy);
 }
 let (app,n)=verified_large_approved(store,signed,auth,supervisor,
   now,roots,verifier,process,consent,scratch)?;
 let good=unsafe{
  indianos_user_image_load(
   image as *mut UserImage,backend as *const Backend,scratch.as_ptr(),n)
 };
 scratch.fill(0);
 if !good||!image.ready{return Err(Error::MapFailed);}
 Ok(app)
}
/// Detach and bind the mapped app to a NEW kernel-created PCB, exactly as
/// the existing smaller verified M2 process route does. The trusted caller
/// must retain the live UserImage until kernel CR3 is restored at exit.
pub fn prepare_large_registered<D:BlockDevice,V:Verify,
 const N:usize,const C:usize>(
 store:&mut BlobStore<D>,signed:&SignedBundle,auth:&Access<N>,
 supervisor:Identity,now:u64,roots:&[Root],verifier:&V,
 process:&mut Process,generation:u64,consent:&Ledger<C>,
 scratch:&mut[u8;CAPACITY],image:&mut UserImage,backend:&Backend,
)->Result<crate::m2_launch::PreparedProcess,Error>{
 if process.pid==0||generation==0||process.state!=crate::NEW||
   process.address_space_root!=0||process.resources_reclaimed||
   image.tables.initialized||image.ready{
  scratch.fill(0);
  return Err(Error::InvalidProcess);
 }
 let id=Identity{pid:process.pid,generation};
 let app=stage_large_verified(store,signed,auth,supervisor,
   now,roots,verifier,id,consent,scratch,image,backend)?;
 let root=image.tables.root_physical;
 let entry=image.plan.entry;
 let stack_top=image.plan.stack_end;
 if root==0||root&4095!=0||entry==0||stack_top<=entry{
  let _=unsafe{crate::user_image::indianos_user_image_destroy(image)};
  return Err(Error::MapFailed);
 }
 if !unsafe{indianos_process_ready(process)}{
  let _=unsafe{crate::user_image::indianos_user_image_destroy(image)};
  return Err(Error::InvalidProcess);
 }
 if !unsafe{indianos_process_bind(process,root)}{
  let _=unsafe{crate::user_image::indianos_user_image_destroy(image)};
  return Err(Error::MapFailed);
 }
 Ok(crate::m2_launch::PreparedProcess{
  identity:id,app,page_root:root,entry,stack_top
 })
}
/// Actual ring3 execution with the existing supervisor's guarded CR3,
/// IRETQ, trap syscall permissions and reaper. Only one core/process owns
/// the supplied 64KiB scratch at a time. NO generic user-mode API.
#[cfg(all(feature="user-runtime",target_arch="x86_64",target_os="none"))]
pub fn execute_large_approved<D:BlockDevice,V:Verify,
 const N:usize,const C:usize>(
 store:&mut BlobStore<D>,signed:&SignedBundle,auth:&Access<N>,
 supervisor:Identity,now:u64,roots:&[Root],verifier:&V,
 process:&mut Process,generation:u64,consent:&Ledger<C>,
 scratch:&mut[u8;CAPACITY],
)->Result<Record,Error>{
 if process.pid==0||generation==0||process.state!=crate::NEW||
  process.address_space_root!=0||process.resources_reclaimed{
  scratch.fill(0);
  return Err(Error::InvalidProcess);
 }
 let id=Identity{pid:process.pid,generation};
 let (app,n)=verified_large_approved(store,signed,auth,supervisor,
   now,roots,verifier,id,consent,scratch)?;
 let Some(policy)=crate::m2_syscall_guard::Policy::new(id,app.rights)
  else{scratch.fill(0);return Err(Error::ConsentDenied);};
 let Some(armed)=crate::m2_syscall_guard::arm(policy)
  else{scratch.fill(0);return Err(Error::Busy);};
 // Caller has exclusive supervisor-owned scratch, kernel NEW PCB, checked
 // W^X ELF and trusted signed/consent-bound package. This synchronous
 // runtime consumes bytes before restoring kernel CR3 and releasing PCB.
 let completed=unsafe{
  crate::user_runtime::x86_64_user_process_run(
   process as *mut Process,scratch.as_ptr(),n)
 };
 drop(armed);
 scratch.fill(0);
 if completed{Ok(app)}else{Err(Error::GuestExecutionFailed)}
}
#[cfg(test)]
mod m2_tests{
 use super::*;
 use crate::block::{BlockError,SECTOR_BYTES};
 use crate::m2_blob_store::{REQUIRED_SECTORS,BundleSource};
 use crate::m2_trust::{Registry,Trusted};
 use crate::m2_ed25519::Ed25519Backend;
 use crate::batch6_security::{FILE_READ,FILE_WRITE};
 use crate::m2_sdk::MAX_DEPS;
 use crate::user_image::{indianos_user_image_destroy,indianos_user_copy_readonly};
 use crate::vmm::Backend;
 use core::ffi::c_void;
 use std::boxed::Box;
 use std::vec::Vec;
 use ed25519_compact::{KeyPair,Seed};

 struct Disk{blocks:Vec<[u8;SECTOR_BYTES]>}
 impl Disk{fn new()->Self{Self{blocks:std::vec![[0;SECTOR_BYTES];REQUIRED_SECTORS as usize]}}}
 impl BlockDevice for Disk{
  fn sector_count(&self)->u64{self.blocks.len() as u64}
  fn read_sector(&mut self,n:u64,out:&mut[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *out=*self.blocks.get(n as usize).ok_or(BlockError::OutOfRange)?;
   Ok(())
  }
  fn write_sector(&mut self,n:u64,value:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
   *self.blocks.get_mut(n as usize).ok_or(BlockError::OutOfRange)?=*value;
   Ok(())
  }
 }
 struct Slice<'a>(&'a[u8]);
 impl BundleSource for Slice<'_>{
  fn read_exact(&mut self,offset:u32,out:&mut[u8])->Result<(),()>{
   let from=offset as usize;
   let length=out.len();
   out.copy_from_slice(self.0.get(from..from+length).ok_or(())?);
   Ok(())
  }
 }
 fn elf(len:usize,v:u8)->Vec<u8>{
  let mut image=std::vec![0x90;len];
  image[..124].copy_from_slice(&crate::m2_elf::tests::sample());
  image[96..104].copy_from_slice(&(len as u64).to_le_bytes());
  image[104..112].copy_from_slice(&(len as u64).to_le_bytes());
  image[len-1]=v;
  image
 }
 fn actor()->Identity{Identity{pid:2,generation:1}}
 fn process()->Identity{Identity{pid:41,generation:7}}
 fn access()->Access<1>{
  let mut a=Access::new();a.grant(actor(),INSTALL,100).unwrap();a
 }
 fn bundle(bytes:&[u8],v:u64)->(SignedBundle,Registry<1>){
  let pair=KeyPair::from_seed(Seed::new([0x61;32]));
  let mut key=[0u8;32];key.copy_from_slice(&pair.pk[..]);
  let mut registry=Registry::new();
  registry.provision(&access(),actor(),1,7,3,key).unwrap();
  let m=crate::m2_bundle_manifest::BundleManifest{
   app:9,version:v,length:bytes.len() as u32,
   digest:crate::m2_sha256::digest(bytes),rights:FILE_READ,
   deps:[0;MAX_DEPS],minimums:[0;MAX_DEPS],signer:7,epoch:3,
  };
  let digest=crate::m2_sha256::digest(&m.canonical());
  let signature=pair.sk.sign(&digest,None);
  let mut bytes_sig=[0u8;64];bytes_sig.copy_from_slice(&signature[..]);
  (SignedBundle{manifest:m,signature:bytes_sig},registry)
 }
 fn approval(bundle:&SignedBundle)->Ledger<1>{
  let mut l=Ledger::new();
  l.approve(&access(),actor(),1,process(),record(bundle),FILE_READ,50).unwrap();
  l
 }
 fn loaded(bytes:&[u8],version:u64)->(BlobStore<Disk>,SignedBundle,Registry<1>){
  let (signed,registry)=bundle(bytes,version);
  let mut storage=BlobStore::mount(Disk::new()).unwrap();
  storage.stage(9,version,bytes.len(),signed.manifest.digest,&mut Slice(bytes))
    .unwrap();
  (BlobStore::mount(storage.into_device()).unwrap(),signed,registry)
 }
 #[test]fn m2_large_approved_64k_elf_ed25519_verified_reboot_and_consent(){
  let image=elf(CAPACITY,1);
  let (mut store,signed,registry)=loaded(&image,1);
  let verifier=Trusted{registry:&registry,backend:&Ed25519Backend};
  let roots=[registry.root(7).unwrap()];
  let ledger=approval(&signed);
  let mut scratch=Box::new([0u8;CAPACITY]);
  let (r,n)=verified_large_approved(&mut store,&signed,&access(),actor(),
   1,&roots,&verifier,process(),&ledger,&mut scratch).unwrap();
  assert_eq!(r.version,1);assert_eq!(r.id,9);assert_eq!(n,CAPACITY);
  assert_eq!(&scratch[..n],&image);
  let mut denied=signed.manifest;
  denied.rights=FILE_WRITE;
  let changed=SignedBundle{manifest:denied,signature:signed.signature};
  assert_eq!(verified_large_approved(&mut store,&changed,&access(),actor(),
   1,&roots,&verifier,process(),&ledger,&mut scratch),
    Err(Error::Signature(PackageError::Signature(
     crate::batch4_signature::Error::InvalidSignature))));
  assert!(scratch.iter().all(|b|*b==0));
 }
 #[test]fn m2_large_approved_rejects_old_pid_revoked_key_stale_version_and_disk_tamper(){
  let image=elf(45000,2);
  let (mut store,signed,mut registry)=loaded(&image,2);
  let ledger=approval(&signed);
  let mut scratch=Box::new([0u8;CAPACITY]);
  {
   let verifier=Trusted{registry:&registry,backend:&Ed25519Backend};
   let roots=[registry.root(7).unwrap()];
   assert_eq!(verified_large_approved(&mut store,&signed,&access(),actor(),
    1,&roots,&verifier,Identity{generation:8,..process()},&ledger,&mut scratch),
    Err(Error::ConsentDenied));
   assert_eq!(verified_large_approved(&mut store,&signed,&access(),
    Identity{generation:2,..actor()},1,&roots,&verifier,process(),&ledger,
    &mut scratch),Err(Error::Denied));
   assert!(scratch.iter().all(|b|*b==0));
   let bytes=elf(45500,4);
   let (other,_) = bundle(&bytes,3);
   assert_eq!(verified_large_approved(&mut store,&other,&access(),actor(),
    1,&roots,&verifier,process(),&ledger,&mut scratch),
    Err(Error::StaleBundle));
  }
  registry.revoke(&access(),actor(),1,7).unwrap();
  let verifier=Trusted{registry:&registry,backend:&Ed25519Backend};
  let roots=[registry.root(7).unwrap()];
  assert_eq!(verified_large_approved(&mut store,&signed,&access(),actor(),
   1,&roots,&verifier,process(),&ledger,&mut scratch),
    Err(Error::Signature(PackageError::Signature(
     crate::batch4_signature::Error::Untrusted))));
  let mut disk=store.into_device();disk.blocks[2+5][14]^=1;
  let result=BlobStore::mount(disk);
  assert!(matches!(result,Err(StorageError::Corrupt)));
 }
 struct Pool{pages:Vec<Box<[u8;4096]>>,allocated:usize,released:usize}
 impl Pool{
  fn new()->Self{
   let mut pages=Vec::new();
   for _ in 0..80{pages.push(Box::new([0;4096]));}
   Self{pages,allocated:0,released:0}
  }
  fn backend(&mut self)->Backend{
   Backend{allocate:Some(allocate),release:Some(release),
    access:Some(frame_ptr),context:(self as *mut Pool).cast()}
  }
 }
 unsafe extern "C" fn allocate(out:*mut u64,ctx:*mut c_void)->bool{
  let pool=unsafe{&mut*(ctx as *mut Pool)};
  if out.is_null()||pool.allocated>=pool.pages.len(){return false;}
  pool.allocated+=1;
  unsafe{*out=pool.allocated as u64*4096};true
 }
 unsafe extern "C" fn release(pa:u64,ctx:*mut c_void)->bool{
  let pool=unsafe{&mut*(ctx as *mut Pool)};
  if pa==0||pa%4096!=0||pa/4096>pool.allocated as u64{return false;}
  pool.released+=1;true
 }
 unsafe extern "C" fn frame_ptr(pa:u64,ctx:*mut c_void)->*mut c_void{
  let pool=unsafe{&mut*(ctx as *mut Pool)};
  if pa==0||pa%4096!=0{return core::ptr::null_mut();}
  pool.pages.get_mut((pa/4096-1) as usize)
   .map_or(core::ptr::null_mut(),|p|p.as_mut_ptr().cast())
 }
 fn empty_image()->UserImage{
  unsafe{core::mem::zeroed()}
 }
 #[test]fn m2_large_mapped_45k_signed_elf_pages_are_nx_wx_safe_and_reclaimed(){
  let bytes=elf(45000,12);
  let (mut store,signed,registry)=loaded(&bytes,1);
  let verifier=Trusted{registry:&registry,backend:&Ed25519Backend};
  let roots=[registry.root(7).unwrap()];
  let ledger=approval(&signed);
  let mut scratch=Box::new([0u8;CAPACITY]);
  let mut pool=Pool::new();
  let backend=pool.backend();
  let mut image=empty_image();
  let app=stage_large_verified(&mut store,&signed,&access(),actor(),1,
   &roots,&verifier,process(),&ledger,&mut scratch,&mut image,&backend).unwrap();
  assert_eq!(app.id,9);
  assert!(image.ready);
  assert!(image.page_count>11);
  assert!(scratch.iter().all(|b|*b==0));
  let mut actual=[0u8;4];
  assert!(unsafe{indianos_user_copy_readonly(&image,0x400078,
   actual.as_mut_ptr().cast(),4)});
  assert_eq!(&actual[..],&bytes[120..124]);
  assert!(unsafe{indianos_user_image_destroy(&mut image)});
  assert_eq!(pool.released,pool.allocated);
 }
 #[test]fn m2_large_invalid_elf_and_denied_signer_allocates_no_pages(){
  let mut bytes=elf(18000,6);
  bytes[68]=7; // W+X program header
  let (mut store,signed,registry)=loaded(&bytes,1);
  let roots=[registry.root(7).unwrap()];
  let verifier=Trusted{registry:&registry,backend:&Ed25519Backend};
  let ledger=approval(&signed);
  let mut scratch=Box::new([0u8;CAPACITY]);
  let mut pool=Pool::new();let backend=pool.backend();
  let mut image=empty_image();
  assert_eq!(stage_large_verified(&mut store,&signed,&access(),actor(),1,
   &roots,&verifier,process(),&ledger,&mut scratch,&mut image,&backend),
   Err(Error::InvalidElf));
  assert_eq!(pool.allocated,0);assert!(!image.ready);
  assert!(scratch.iter().all(|b|*b==0));
 }
 #[test]fn m2_large_prepared_real_pcb_requires_exact_consent_before_pmm_alloc(){
  let bytes=elf(60000,3);
  let (mut store,signed,registry)=loaded(&bytes,1);
  let roots=[registry.root(7).unwrap()];
  let verifier=Trusted{registry:&registry,backend:&Ed25519Backend};
  let mut table=crate::ProcessTable::empty();
  let pcb=unsafe{crate::indianos_process_create(&mut table)};
  assert!(!pcb.is_null());
  let pid=unsafe{(*pcb).pid};
  let identity=Identity{pid,generation:17};
  let mut ledger=Ledger::<1>::new();
  ledger.approve(&access(),actor(),1,identity,record(&signed),
   FILE_READ,20).unwrap();
  let mut scratch=Box::new([0u8;CAPACITY]);
  let mut pool=Pool::new();let backend=pool.backend();
  let mut image=empty_image();
  assert_eq!(prepare_large_registered(&mut store,&signed,&access(),
    actor(),1,&roots,&verifier,unsafe{&mut*pcb},18,&ledger,&mut scratch,
    &mut image,&backend),Err(Error::ConsentDenied));
  assert_eq!(unsafe{(*pcb).state},crate::NEW);
  assert_eq!(pool.allocated,0);
  let receipt=prepare_large_registered(&mut store,&signed,&access(),
    actor(),1,&roots,&verifier,unsafe{&mut*pcb},17,&ledger,&mut scratch,
    &mut image,&backend).unwrap();
  assert_eq!(receipt.app.id,9);assert_eq!(receipt.app.version,1);
  assert_eq!(receipt.identity,identity);
  assert_eq!(receipt.page_root,image.tables.root_physical);
  assert_eq!(receipt.entry,0x400078);
  assert_eq!(unsafe{(*pcb).state},crate::READY);
  assert_eq!(unsafe{(*pcb).address_space_root},receipt.page_root);
  assert!(scratch.iter().all(|b|*b==0));
  assert!(unsafe{indianos_user_image_destroy(&mut image)});
  assert_eq!(pool.allocated,pool.released);
 }
 #[test]fn m2_large_rollback_requires_current_signed_manifest_and_fresh_consent(){
  let old=elf(21000,1);
  let new=elf(64000,2);
  let (mut store,signed_old,registry)=loaded(&old,1);
  let (signed_new,_)=bundle(&new,2);
  store.stage(9,2,new.len(),signed_new.manifest.digest,
    &mut Slice(&new)).unwrap();
  let roots=[registry.root(7).unwrap()];
  let verifier=Trusted{registry:&registry,backend:&Ed25519Backend};
  let mut scratch=Box::new([0u8;CAPACITY]);
  let ledger=approval(&signed_old);
  assert_eq!(verified_large_approved(&mut store,&signed_old,&access(),
   actor(),1,&roots,&verifier,process(),&ledger,&mut scratch),
   Err(Error::StaleBundle));
  store.rollback().unwrap();
  let (app,n)=verified_large_approved(&mut store,&signed_old,&access(),
   actor(),1,&roots,&verifier,process(),&ledger,&mut scratch).unwrap();
  assert_eq!(app.version,1);assert_eq!(&scratch[..n],&old);
  assert_eq!(store.active().unwrap().version,1);
 }

}
