//! Issue #124 / HD3: authenticated GPU/NPU driver-image quarantine and
//! capability-scoped hardware-resource planning.
//!
//! A signed HD2 catalog alone is NOT proof of the driver binary's bytes.
//! This phase authenticates both with real Ed25519 + SHA256 (when the M2
//! crypto feature is built), binds staging to an exact PCI function and
//! PID-generation, and validates tentative MMIO/DMA resource plans.
//!
//! STAGING IS NOT DRIVER INSTALLATION/EXECUTION. No config-space writes,
//! bus mastering, BAR probing, IOMMU tables, device queues, firmware or
//! downloaded drivers. Plans are data-only and NEVER grant live access.
//! Real isolation evidence MUST come from a separately trusted platform
//! implementation. At ordinary boot no catalog/root/driver is provisioned.

use crate::agent_07_pci_aperture::{self,Aperture};
use crate::hardware_driver_policy as hd2;
use crate::hardware_inventory::{Accelerator,Cpu,Device};
use crate::batch5_session::Identity;
use crate::batch6_security::{Access,INSTALL,DRIVER};
pub const STAGE_ABI:u16=1;
pub const MAX_DRIVER_BYTES:usize=65536;
pub const PAGE_ALIGN:u64=4096;
const MAX_PLAN_BYTES:u64=16*1024*1024;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{
 Denied,Invalid,TooLarge,ImageHash,NoDriver,DeviceMismatch,
 Stale,Revoked,Memory,NotIsolated,Unrequested,Overlap,
 ResourceBounds,Catalog(hd2::Error),
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Bdf{pub bus:u8,pub slot:u8,pub function:u8}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Staged{
 pub abi:u16,pub owner:Identity,pub bdf:Bdf,
 pub driver_id:u32,pub version:u64,pub generation:u64,
 pub digest:[u8;32],pub image_len:u32,pub requested:u8,
}
/// Real operation never installs the module or marks it runnable:
/// only an owned, quarantined, hash-checked byte region is produced.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Quarantine{record:Option<Staged>}
impl Quarantine{
 pub const fn new()->Self{Self{record:None}}
 pub fn metadata(&self)->Option<Staged>{self.record}
 pub fn retire(&mut self,slot:&mut[u8],owner:Identity)->Result<(),Error>{
  let existing=self.record.ok_or(Error::NoDriver)?;
  if existing.owner!=owner{return Err(Error::Denied);}
  slot.fill(0);self.record=None;Ok(())
 }
}
pub fn validate_device(device:Device,identity:hd2::PciIdentity)
 ->Result<Bdf,Error>{
 if device.slot>=32||device.function>=8||
  device.header.vendor!=identity.vendor||
  device.header.device!=identity.device||
  device.header.class!=identity.class||
  device.header.subclass!=identity.subclass||
  !matches!((device.kind,identity.kind()),
   (Accelerator::Gpu,Some(hd2::DRIVER_GPU))|
   (Accelerator::Npu,Some(hd2::DRIVER_NPU))){
  return Err(Error::DeviceMismatch);
 }
 Ok(Bdf{bus:device.bus,slot:device.slot,function:device.function})
}
fn check_admin<const N:usize>(auth:&Access<N>,actor:Identity,now:u64)
 ->Result<(),Error>{
 auth.require(actor,INSTALL|DRIVER,now).map_err(|_|Error::Denied)
}
#[cfg(feature="test-m2-developer-alpha")]
pub fn stage_verified<const N:usize,const K:usize,B:crate::m2_trust::SignatureBackend>(
 state:&mut Quarantine,quarantine:&mut[u8],
 auth:&Access<N>,actor:Identity,now:u64,
 device:Device,identity:hd2::PciIdentity,cpu:&Cpu,
 catalog:&hd2::SignedCatalog,registry:&crate::m2_trust::Registry<K>,
 backend:&B,minimum_generation:u64,untrusted_image:&[u8],
)->Result<Staged,Error>{
 // An attempted update erases any prior staged image; the caller cannot
 // continue using it after a failed verification.
 quarantine.fill(0);
 state.record=None;
 let verified=(||{
  check_admin(auth,actor,now)?;
  let bdf=validate_device(device,identity)?;
  let catalog=catalog.verify(registry,backend,minimum_generation)
   .map_err(Error::Catalog)?;
  let decision=catalog.choose(identity,cpu).map_err(Error::Catalog)?;
  let hd2::Decision::SignedCandidate(candidate)=decision
   else{return Err(Error::NoDriver)};
  if untrusted_image.is_empty()||untrusted_image.len()>MAX_DRIVER_BYTES||
     untrusted_image.len()>quarantine.len(){
   return Err(Error::TooLarge);
  }
  let image_digest=crate::m2_sha256::digest(untrusted_image);
  if image_digest!=candidate.digest{return Err(Error::ImageHash);}
  Ok((bdf,candidate))
 })();
 let (bdf,candidate)=verified?;
 quarantine[..untrusted_image.len()].copy_from_slice(untrusted_image);
 let record=Staged{abi:STAGE_ABI,owner:actor,bdf,
  driver_id:candidate.id,version:candidate.version,
  generation:candidate.signed_generation,
  digest:candidate.digest,image_len:untrusted_image.len() as u32,
  requested:candidate.requested};
 state.record=Some(record);
 Ok(record)
}

/// Hardware limits MUST be independently validated against the platform
/// physical memory map, an IOMMU implementation and PCI BAR sizing by
/// later, trusted code. This flag is not asserted by the HD3 boot probe.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PlatformEvidence{
 pub mmio_allowed:Aperture,
 pub dma_allowed:Aperture,
 pub iommu_domain:u32,
 pub iommu_ready:bool,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Request{
 pub mmio:Option<Aperture>,pub dma:Option<Aperture>,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct ResourcePlan{
 pub owner:Identity,pub bdf:Bdf,pub driver_id:u32,
 pub mmio:Option<Aperture>,pub dma:Option<Aperture>,
 pub iommu_domain:u32,
 /// This is NOT an access token; no hardware capability is granted.
 pub activated:bool,
}
fn inside(range:Aperture,allowed:Aperture)->Result<(),Error>{
 if range.length>MAX_PLAN_BYTES{return Err(Error::ResourceBounds);}
 let end=allowed.base.checked_add(allowed.length)
  .ok_or(Error::ResourceBounds)?;
 agent_07_pci_aperture::validate(range,allowed.base,end,PAGE_ALIGN)
  .map_err(|_|Error::ResourceBounds)
}
fn overlaps(a:Aperture,b:Aperture)->bool{
 match (a.base.checked_add(a.length),b.base.checked_add(b.length)){
  (Some(a_end),Some(b_end))=>a.base<b_end&&b.base<a_end,
  _=>true,
 }
}
/// Pure numeric resource proposal. This never maps MMIO, enables DMA or
/// writes IOMMU tables, even if a test fixture supplies positive evidence.
pub fn propose_resources<const N:usize>(
 state:&Quarantine,quarantine:&[u8],
 auth:&Access<N>,actor:Identity,now:u64,
 request:Request,platform:PlatformEvidence,
)->Result<ResourcePlan,Error>{
 check_admin(auth,actor,now)?;
 let staged=state.record.ok_or(Error::NoDriver)?;
 if staged.owner!=actor{return Err(Error::Denied);}
 if staged.abi!=STAGE_ABI||staged.image_len==0||
  staged.image_len as usize>quarantine.len(){
  return Err(Error::Invalid);
 }
 // Catch post-install quarantine tampering. A successful previous
 // signed-catalog check does not make mutable memory permanently trusted.
 #[cfg(feature="test-m2-developer-alpha")]
 if crate::m2_sha256::digest(&quarantine[..staged.image_len as usize])
    !=staged.digest{return Err(Error::ImageHash);}
 // This API cannot grant unrequested device rights.
 let wants_mmio=staged.requested&hd2::WANT_MMIO!=0;
 let wants_dma=staged.requested&hd2::WANT_DMA!=0;
 if request.mmio.is_some()!=wants_mmio||
  request.dma.is_some()!=wants_dma{return Err(Error::Unrequested);}
 if let Some(mmio)=request.mmio{inside(mmio,platform.mmio_allowed)?;}
 if let Some(dma)=request.dma{
  if !platform.iommu_ready||platform.iommu_domain==0{
   return Err(Error::NotIsolated);
  }
  inside(dma,platform.dma_allowed)?;
 }
 if let (Some(mmio),Some(dma))=(request.mmio,request.dma){
  if overlaps(mmio,dma){return Err(Error::Overlap);}
 }
 Ok(ResourcePlan{owner:actor,bdf:staged.bdf,driver_id:staged.driver_id,
  mmio:request.mmio,dma:request.dma,
  iommu_domain:if wants_dma{platform.iommu_domain}else{0},
  activated:false})
}

/// Re-authenticate the catalog, signer epoch, current device and image
/// whenever a privileged manager wants to request a new plan. A revoked
/// publisher or replaced PCI function cannot borrow a stale staged image.
/// Still only returns a NUMERIC proposal, never live MMIO/DMA authority.
#[cfg(feature="test-m2-developer-alpha")]
pub fn propose_verified_resources<const A:usize,const K:usize,
 B:crate::m2_trust::SignatureBackend>(
 state:&Quarantine,quarantine:&[u8],auth:&Access<A>,
 actor:Identity,now:u64,device:Device,
 identity:hd2::PciIdentity,cpu:&Cpu,
 catalog:&hd2::SignedCatalog,registry:&crate::m2_trust::Registry<K>,
 backend:&B,minimum_generation:u64,
 request:Request,platform:PlatformEvidence
)->Result<ResourcePlan,Error>{
 check_admin(auth,actor,now)?;
 let staged=state.record.ok_or(Error::NoDriver)?;
 let bdf=validate_device(device,identity)?;
 if staged.owner!=actor||staged.bdf!=bdf{return Err(Error::DeviceMismatch);}
 let verified=catalog.verify(registry,backend,minimum_generation)
   .map_err(Error::Catalog)?;
 let hd2::Decision::SignedCandidate(candidate)=
   verified.choose(identity,cpu).map_err(Error::Catalog)?
  else{return Err(Error::NoDriver)};
 if candidate.id!=staged.driver_id||
    candidate.version!=staged.version||
    candidate.digest!=staged.digest||
    candidate.requested!=staged.requested||
    candidate.signed_generation!=staged.generation{
  return Err(Error::Stale);
 }
 propose_resources(state,quarantine,auth,actor,now,request,platform)
}

/// Runtime is not wired to a verified platform/IOMMU driver. Always
/// preserve the CPU-only fallback on ordinary boots and hot-plug probes.
pub const fn boot_driver_status()->&'static[u8]{
 b"IndianOS HD3 driver image=not installed; MMIO=not mapped; DMA=disabled; IOMMU=not attested; GPU/NPU=CPU fallback; QPU=unavailable\n\0"
}
#[cfg(test)]
mod hd3_tests{
 use super::*;
 use crate::hardware_inventory::{PciHeader,CPU_REQUIRED};
 fn owner()->Identity{Identity{pid:3,generation:4}}
 fn cpu()->Cpu{Cpu{vendor:*b"GenuineIntel",family:6,model:58,stepping:3,
  features:CPU_REQUIRED,max_basic:7,max_extended:0x8000_0001}}
 fn device()->(Device,hd2::PciIdentity){
  let id=hd2::PciIdentity{vendor:0x1af4,device:0x1050,
   class:3,subclass:0,revision:2,
   subsystem_vendor:0,subsystem_device:0};
  let dev=Device{bus:1,slot:2,function:0,kind:Accelerator::Gpu,
   header:PciHeader{vendor:id.vendor,device:id.device,
    class:id.class,subclass:id.subclass,prog_if:0,header_type:0}};
  (dev,id)
 }
 fn auth()->Access<1>{
  let mut a=Access::new();a.grant(owner(),INSTALL|DRIVER,99).unwrap();a
 }
 fn range(base:u64,length:u64)->Aperture{Aperture{base,length}}
 fn platform()->PlatformEvidence{PlatformEvidence{
  mmio_allowed:range(0x1000_0000,0x1000000),
  dma_allowed:range(0x2000_0000,0x1000000),
  iommu_domain:17,iommu_ready:true}}
 fn staged(requested:u8)->(Quarantine,[u8;64]){
  let mut state=Quarantine::new();
  let mut bytes=[0u8;64];
  bytes[..8].copy_from_slice(b"mockdata");
  #[cfg(feature="test-m2-developer-alpha")]
  let digest=crate::m2_sha256::digest(&bytes[..8]);
  #[cfg(not(feature="test-m2-developer-alpha"))]
  let digest=[3;32];
  state.record=Some(Staged{abi:STAGE_ABI,owner:owner(),
   bdf:Bdf{bus:1,slot:2,function:0},
   driver_id:5,version:2,generation:1,
   digest,image_len:8,requested});
  (state,bytes)
 }
 #[test]fn hd3_unknown_gpu_and_npu_identity_cannot_be_spoofed(){
  let (dev,id)=device();
  assert_eq!(validate_device(dev,id),
   Ok(Bdf{bus:1,slot:2,function:0}));
  let mut swapped=id;swapped.vendor=0x8086;
  assert_eq!(validate_device(dev,swapped),Err(Error::DeviceMismatch));
  let mut malicious=dev;malicious.kind=Accelerator::Npu;
  assert_eq!(validate_device(malicious,id),Err(Error::DeviceMismatch));
  let mut invalid=dev;invalid.slot=32;
  assert_eq!(validate_device(invalid,id),Err(Error::DeviceMismatch));
  assert!(cpu().supported());
 }
 #[test]fn hd3_missing_driver_permission_or_reused_pid_cannot_stage_or_plan(){
  let mut only_install=Access::<1>::new();
  only_install.grant(owner(),INSTALL,99).unwrap();
  assert_eq!(check_admin(&only_install,owner(),1),Err(Error::Denied));
  let old=Identity{generation:5,..owner()};
  assert_eq!(check_admin(&auth(),old,1),Err(Error::Denied));
  let (state,bytes)=staged(hd2::WANT_MMIO);
  let req=Request{mmio:Some(range(0x1000_0000,4096)),dma:None};
  assert_eq!(propose_resources(&state,&bytes,&auth(),old,1,req,platform()),
   Err(Error::Denied));
 }
 #[test]fn hd3_no_driver_boot_and_no_unrequested_mmio_dma_capabilities(){
  let (state,bytes)=staged(hd2::WANT_MMIO);
  let req=Request{mmio:Some(range(0x1000_0000,4096)),dma:None};
  #[cfg(not(feature="test-m2-developer-alpha"))]
  {
   let proposal=propose_resources(&state,&bytes,&auth(),owner(),1,
    req,platform()).unwrap();
   assert!(!proposal.activated);
   assert_eq!(proposal.iommu_domain,0);
  }
  assert_eq!(propose_resources(&state,&bytes,&auth(),owner(),1,
   Request{mmio:None,dma:Some(range(0x2000_0000,4096))},
   platform()),Err(Error::Unrequested));
  assert!(core::str::from_utf8(boot_driver_status()).unwrap()
   .contains("DMA=disabled"));
 }
 #[test]fn hd3_dma_denied_without_verified_iommu_and_outside_resource_bounds(){
  let (state,bytes)=staged(hd2::WANT_MMIO|hd2::WANT_DMA);
  let req=Request{mmio:Some(range(0x1000_0000,4096)),
   dma:Some(range(0x2000_0000,4096))};
  let mut not_isolated=platform();
  not_isolated.iommu_ready=false;
  assert_eq!(propose_resources(&state,&bytes,&auth(),owner(),1,
   req,not_isolated),
   Err(Error::NotIsolated));
  let mut zero_domain=platform();
  zero_domain.iommu_domain=0;
  assert_eq!(propose_resources(&state,&bytes,&auth(),owner(),1,
   req,zero_domain),Err(Error::NotIsolated));
  let out_of_bounds=Request{mmio:Some(range(0x5000_0000,4096)),
   dma:req.dma};
  assert_eq!(propose_resources(&state,&bytes,&auth(),owner(),1,
   out_of_bounds,platform()),Err(Error::ResourceBounds));
 }
 #[test]fn hd3_quarantine_retire_zeroes_bytes_and_denies_pid_reuse(){
  let (mut state,mut bytes)=staged(0);
  assert_eq!(state.retire(&mut bytes,Identity{generation:5,..owner()}),
   Err(Error::Denied));
  assert!(bytes.iter().any(|v|*v!=0));
  state.retire(&mut bytes,owner()).unwrap();
  assert!(bytes.iter().all(|v|*v==0));
  assert_eq!(state.metadata(),None);
  assert_eq!(state.retire(&mut bytes,owner()),Err(Error::NoDriver));
 }
}
#[cfg(all(test,feature="test-m2-developer-alpha"))]
mod hd3_crypto_tests{
 use super::*;
 use crate::hardware_inventory::{PciHeader,CPU_REQUIRED};
 use crate::m2_trust::Registry;
 use crate::m2_ed25519::Ed25519Backend;
 use ed25519_compact::{KeyPair,Seed};
 fn owner()->Identity{Identity{pid:17,generation:3}}
 fn admin()->Access<1>{let mut a=Access::new();
  a.grant(owner(),INSTALL|DRIVER,50).unwrap();a}
 fn cpu()->Cpu{Cpu{vendor:*b"GenuineIntel",family:6,model:58,stepping:1,
  features:CPU_REQUIRED,max_basic:7,max_extended:0x8000_0001}}
 fn device()->(Device,hd2::PciIdentity){
  let id=hd2::PciIdentity{vendor:0x1af4,device:0x1050,
   class:3,subclass:0,revision:1,subsystem_vendor:0,subsystem_device:0};
  (Device{bus:1,slot:2,function:0,kind:Accelerator::Gpu,
   header:PciHeader{vendor:id.vendor,device:id.device,
    class:3,subclass:0,prog_if:0,header_type:0}},id)
 }
 fn fixture(bytes:&[u8])->(hd2::SignedCatalog,Registry<1>){
  let kp=KeyPair::from_seed(Seed::new([0x44;32]));
  let mut key=[0u8;32];key.copy_from_slice(&kp.pk[..]);
  let mut roots=Registry::<1>::new();
  roots.provision(&admin(),owner(),1,7,2,key).unwrap();
  let mut c=hd2::SignedCatalog::empty();
  c.generation=3;c.signer=7;c.epoch=2;c.count=1;
  c.entries[0]=hd2::Driver{id:56,version:8,abi:hd2::DRIVER_ABI,
   kind:hd2::DRIVER_GPU,vendor:0x1af4,device:0x1050,
   subsystem_vendor:0,subsystem_device:0,
   revision_min:1,revision_max:2,
   min_cpu:CPU_REQUIRED,wants:hd2::WANT_MMIO|hd2::WANT_DMA,
   digest:crate::m2_sha256::digest(bytes)};
  let hash=crate::m2_sha256::digest(&c.canonical());
  let sig=kp.sk.sign(&hash,None);
  c.signature.copy_from_slice(&sig[..]);
  (c,roots)
 }
 fn data()->&'static[u8]{b"HD3 trusted fixture only; no executable module"}
 fn platform()->PlatformEvidence{PlatformEvidence{
  mmio_allowed:Aperture{base:0x1000_0000,length:0x100000},
  dma_allowed:Aperture{base:0x2000_0000,length:0x100000},
  iommu_domain:9,iommu_ready:true}}
 fn stage(image:&[u8])->(Quarantine,[u8;256]){
  let (c,keys)=fixture(image);
  let mut state=Quarantine::new();let mut scratch=[0u8;256];
  let (dev,id)=device();
  stage_verified(&mut state,&mut scratch,&admin(),owner(),1,
   dev,id,&cpu(),&c,&keys,&Ed25519Backend,3,image).unwrap();
  (state,scratch)
 }
 #[test]fn hd3_signed_driver_image_stages_only_matching_bytes_and_exact_pci(){
  let (state,slot)=stage(data());
  let record=state.metadata().unwrap();
  assert_eq!(record.driver_id,56);
  assert_eq!(record.version,8);
  assert_eq!(record.generation,3);
  assert_eq!(record.requested,hd2::WANT_MMIO|hd2::WANT_DMA);
  assert_eq!(&slot[..data().len()],data());
  assert!(slot[data().len()..].iter().all(|b|*b==0));
  let ok=propose_resources(&state,&slot,&admin(),owner(),1,
   Request{
    mmio:Some(Aperture{base:0x1000_0000,length:4096}),
    dma:Some(Aperture{base:0x2000_0000,length:4096})
   },platform()).unwrap();
  assert_eq!(ok.bdf,Bdf{bus:1,slot:2,function:0});
  assert_eq!(ok.iommu_domain,9);
  assert!(!ok.activated);
  let (catalog,roots)=fixture(data());
  let (dev,id)=device();
  let secure=propose_verified_resources(&state,&slot,&admin(),owner(),1,
   dev,id,&cpu(),&catalog,&roots,&Ed25519Backend,3,
   Request{
    mmio:Some(Aperture{base:0x1000_0000,length:4096}),
    dma:Some(Aperture{base:0x2000_0000,length:4096})
   },platform()).unwrap();
  assert_eq!(secure,ok);
 }
 #[test]fn hd3_image_tampering_or_bad_catalog_signature_erases_quarantine(){
  let (mut c,keys)=fixture(data());
  let mut state=Quarantine::new();let mut mem=[0x5a;128];
  let (dev,id)=device();
  let mut corrupted=data().to_vec();corrupted[0]^=3;
  assert_eq!(stage_verified(&mut state,&mut mem,&admin(),owner(),1,
    dev,id,&cpu(),&c,&keys,&Ed25519Backend,3,&corrupted),
   Err(Error::ImageHash));
  assert!(mem.iter().all(|b|*b==0));assert!(state.metadata().is_none());
  c.signature[32]^=1;mem.fill(0x11);
  assert_eq!(stage_verified(&mut state,&mut mem,&admin(),owner(),1,
   dev,id,&cpu(),&c,&keys,&Ed25519Backend,3,data()),
   Err(Error::Catalog(hd2::Error::Signature)));
  assert!(mem.iter().all(|b|*b==0));
 }
 #[test]fn hd3_expired_permission_revoked_signer_and_rollback_denied(){
  let (c,mut keys)=fixture(data());
  let (dev,id)=device();
  let mut state=Quarantine::new();let mut mem=[0;128];
  assert_eq!(stage_verified(&mut state,&mut mem,&admin(),owner(),50,
   dev,id,&cpu(),&c,&keys,&Ed25519Backend,3,data()),
   Err(Error::Denied));
  assert_eq!(stage_verified(&mut state,&mut mem,&admin(),owner(),1,
   dev,id,&cpu(),&c,&keys,&Ed25519Backend,4,data()),
   Err(Error::Catalog(hd2::Error::Rollback)));
  keys.revoke(&admin(),owner(),1,7).unwrap();
  assert_eq!(stage_verified(&mut state,&mut mem,&admin(),owner(),1,
   dev,id,&cpu(),&c,&keys,&Ed25519Backend,3,data()),
   Err(Error::Catalog(hd2::Error::Revoked)));
 }
 #[test]fn hd3_quarantine_mutation_cannot_reuse_valid_signed_record(){
  let (state,mut mem)=stage(data());
  mem[0]^=1;
  let result=propose_resources(&state,&mem,&admin(),owner(),1,
   Request{
    mmio:Some(Aperture{base:0x1000_0000,length:4096}),
    dma:Some(Aperture{base:0x2000_0000,length:4096})
   },platform());
  assert_eq!(result,Err(Error::ImageHash));
 }
 #[test]fn hd3_driver_resources_denied_after_publisher_revocation(){
  let (state,slot)=stage(data());
  let (catalog,mut roots)=fixture(data());
  let (dev,id)=device();
  roots.revoke(&admin(),owner(),1,7).unwrap();
  let result=propose_verified_resources(&state,&slot,&admin(),owner(),1,
   dev,id,&cpu(),&catalog,&roots,&Ed25519Backend,3,
   Request{
    mmio:Some(Aperture{base:0x1000_0000,length:4096}),
    dma:Some(Aperture{base:0x2000_0000,length:4096})
   },platform());
  assert_eq!(result,Err(Error::Catalog(hd2::Error::Revoked)));
 }
 #[test]fn hd3_driver_signed_identity_rejects_gpu_hot_swap(){
  let (c,keys)=fixture(data());
  let (dev,mut id)=device();
  id.device^=1;
  let mut state=Quarantine::new();let mut slot=[0;256];
  assert_eq!(stage_verified(&mut state,&mut slot,&admin(),owner(),1,
   dev,id,&cpu(),&c,&keys,&Ed25519Backend,3,data()),
   Err(Error::DeviceMismatch));
  assert_eq!(state.metadata(),None);
 }
}
