//! Issue #124 HD2: signed, fail-closed GPU/NPU driver-compatibility decisions.
//!
//! Hardware discovery NEVER means the driver can run. Default early boot has
//! NO trusted signed driver catalog and therefore binds NO device driver.
//! A catalog can become usable only after an independently authenticated
//! firmware/TPM trust root provisions the Ed25519 publisher registry.
//! Real firmware roots, installed driver binaries, IOMMU and DMA/MMIO
//! authorization, GPU/NPU command validation, and driver execution are
//! separate production gates and deliberately NOT implemented here.
//!
//! Catalog matching is deterministic and allocation-free. A checked, signed
//! policy produces an ELIGIBLE METADATA candidate, NEVER hardware authority.

use crate::hardware_inventory::{self as hd0, Accelerator, Cpu, Device, PciHeader};
pub const POLICY_ABI:u16=1;
pub const DRIVER_ABI:u16=1;
pub const MAX_DRIVERS:usize=8;
pub const DRIVER_GPU:u8=1;
pub const DRIVER_NPU:u8=2;
pub const WANT_MMIO:u8=1;
pub const WANT_DMA:u8=2;
pub const ALLOWED_DEVICE_REQUESTS:u8=WANT_MMIO|WANT_DMA;
pub const MAX_DEVICES:usize=hd0::MAX_ACCELERATORS;

/// An exact PCI identity from read-only config-space fields; driver
/// binding requires vendor/device, revision and subsystem validation.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PciIdentity{
 pub vendor:u16,pub device:u16,
 pub class:u8,pub subclass:u8,pub revision:u8,
 pub subsystem_vendor:u16,pub subsystem_device:u16,
}
impl PciIdentity{
 pub fn from_header(header:PciHeader,revision:u8,sub_vendor:u16,sub_device:u16)->Self{
  Self{vendor:header.vendor,device:header.device,class:header.class,
   subclass:header.subclass,revision,
   subsystem_vendor:sub_vendor,subsystem_device:sub_device}
 }
 pub fn kind(self)->Option<u8>{
  match hd0::classify_pci(PciHeader{
   vendor:self.vendor,device:self.device,
   class:self.class,subclass:self.subclass,prog_if:0,header_type:0
  }){
   Some(Accelerator::Gpu)=>Some(DRIVER_GPU),
   Some(Accelerator::Npu)=>Some(DRIVER_NPU),
   None=>None
  }
 }
}
pub trait ExtendedPciProbe{
 /// Read 8-bit PCI revision ID (offset 0x08) and PCI subsystem
 /// vendor/device IDs (offset 0x2c) without writing PCI config registers.
 fn identify(&mut self,bus:u8,slot:u8,function:u8,
  header:PciHeader)->Result<PciIdentity,Error>;
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Driver{
 pub id:u32,pub version:u64,pub abi:u16,pub kind:u8,
 pub vendor:u16,pub device:u16,
 /// Both zeros means only the PCI subsystem is not constrained.
 pub subsystem_vendor:u16,pub subsystem_device:u16,
 pub revision_min:u8,pub revision_max:u8,
 pub min_cpu:u64,pub wants:u8,pub digest:[u8;32],
}
pub const EMPTY_DRIVER:Driver=Driver{
 id:0,version:0,abi:0,kind:0,vendor:0,device:0,
 subsystem_vendor:0,subsystem_device:0,
 revision_min:0,revision_max:0,min_cpu:0,wants:0,digest:[0;32],
};
const DOMAIN:&[u8]=b"IndianOS/HD2/TrustedDriverCatalog/V1\0";
pub const ENTRY_BYTES:usize=4+8+2+1+2+2+2+2+1+1+8+1+32;
pub const TRANSCRIPT_BYTES:usize=DOMAIN.len()+2+8+4+8+1+ENTRY_BYTES*MAX_DRIVERS;
pub const CATALOG_BYTES:usize=TRANSCRIPT_BYTES+64;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{
 Invalid,Unsigned,Revoked,StaleSigner,Rollback,Signature,
 Duplicate,Ambiguous,TooManyDevices,ReadFailed,InsufficientCpu,
}
#[derive(Clone,Copy,Debug)]
pub struct SignedCatalog{
 pub generation:u64,pub signer:u32,pub epoch:u64,pub count:u8,
 pub entries:[Driver;MAX_DRIVERS],pub signature:[u8;64],
}
impl SignedCatalog{
 pub const fn empty()->Self{
  Self{generation:0,signer:0,epoch:0,count:0,
   entries:[EMPTY_DRIVER;MAX_DRIVERS],signature:[0;64]}
 }
 /// Entire fixed-width transcript includes the ABSENT slots, no trailing
 /// fields, deterministic endianness and a domain separate from app signing.
 pub fn canonical(&self)->[u8;TRANSCRIPT_BYTES]{
  let mut out=[0u8;TRANSCRIPT_BYTES];
  let mut at=0usize;
  fn put(out:&mut[u8],at:&mut usize,data:&[u8]){
   out[*at..*at+data.len()].copy_from_slice(data);
   *at+=data.len();
  }
  put(&mut out,&mut at,DOMAIN);
  put(&mut out,&mut at,&POLICY_ABI.to_le_bytes());
  put(&mut out,&mut at,&self.generation.to_le_bytes());
  put(&mut out,&mut at,&self.signer.to_le_bytes());
  put(&mut out,&mut at,&self.epoch.to_le_bytes());
  put(&mut out,&mut at,&[self.count]);
  for entry in &self.entries{
   put(&mut out,&mut at,&entry.id.to_le_bytes());
   put(&mut out,&mut at,&entry.version.to_le_bytes());
   put(&mut out,&mut at,&entry.abi.to_le_bytes());
   put(&mut out,&mut at,&[entry.kind]);
   put(&mut out,&mut at,&entry.vendor.to_le_bytes());
   put(&mut out,&mut at,&entry.device.to_le_bytes());
   put(&mut out,&mut at,&entry.subsystem_vendor.to_le_bytes());
   put(&mut out,&mut at,&entry.subsystem_device.to_le_bytes());
   put(&mut out,&mut at,&[entry.revision_min,entry.revision_max]);
   put(&mut out,&mut at,&entry.min_cpu.to_le_bytes());
   put(&mut out,&mut at,&[entry.wants]);
   put(&mut out,&mut at,&entry.digest);
  }
  debug_assert_eq!(at,TRANSCRIPT_BYTES);
  out
 }
 pub fn validate(&self)->Result<(),Error>{
  if self.generation==0||self.signer==0||self.epoch==0||
   self.count==0||self.count as usize>MAX_DRIVERS{
   return Err(Error::Invalid);
  }
  for (i,e) in self.entries.iter().enumerate(){
   if i>=self.count as usize{
    if *e!=EMPTY_DRIVER{return Err(Error::Invalid);}
    continue;
   }
   if e.id==0||e.version==0||e.abi!=DRIVER_ABI||
    !(e.kind==DRIVER_GPU||e.kind==DRIVER_NPU)||
    e.vendor==0||e.vendor==0xffff||e.device==0xffff||
    e.revision_min>e.revision_max||
    (e.subsystem_vendor==0)!=(e.subsystem_device==0)||
    e.min_cpu&!hd0::CPU_REQUIRED!=0||
    e.wants&!ALLOWED_DEVICE_REQUESTS!=0||
    e.digest==[0;32]{
    return Err(Error::Invalid);
   }
   // Duplicate driver ID or an equally specific match on intersecting
   // revision ranges makes policy selection ambiguous.
   for previous in &self.entries[..i]{
    if e.id==previous.id{return Err(Error::Duplicate);}
    if e.kind==previous.kind&&e.vendor==previous.vendor&&
      e.device==previous.device&&
      e.subsystem_vendor==previous.subsystem_vendor&&
      e.subsystem_device==previous.subsystem_device&&
      e.revision_min<=previous.revision_max&&
      previous.revision_min<=e.revision_max{
     return Err(Error::Ambiguous);
    }
   }
  }
  Ok(())
 }
}

/// Returning a Candidate does NOT allocate IOMMU space, activate the device,
/// install the binary or confer a kernel/driver capability.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Candidate{
 pub id:u32,pub version:u64,pub digest:[u8;32],
 pub requested:u8,pub signed_generation:u64,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Decision{
 CpuFallback,
 /// Metadata-only eligible. Explicit signed-driver image verification,
 /// kernel capability authorization and IOMMU isolation still required.
 SignedCandidate(Candidate),
}
fn available_cpu(cpu:&Cpu)->u64{cpu.features&hd0::CPU_REQUIRED}
fn match_signed_entry<'a>(cat:&'a SignedCatalog,identity:PciIdentity,
 cpu:&Cpu)->Result<Option<&'a Driver>,Error>{
 let Some(kind)=identity.kind()else{return Ok(None)};
 let mut selected:Option<&Driver>=None;
 for e in cat.entries.iter().take(cat.count as usize){
  if e.kind!=kind||e.vendor!=identity.vendor||
   e.device!=identity.device||
   identity.revision<e.revision_min||
   identity.revision>e.revision_max||
   e.min_cpu&!available_cpu(cpu)!=0{continue;}
  let specific=e.subsystem_vendor!=0;
  if specific&&(e.subsystem_vendor!=identity.subsystem_vendor||
    e.subsystem_device!=identity.subsystem_device){continue;}
  match selected{
   Some(old) if old.subsystem_vendor!=0&&!specific=>continue,
   Some(old) if (old.subsystem_vendor!=0)==specific=>
    return Err(Error::Ambiguous),
   _=>selected=Some(e)
  }
 }
 Ok(selected)
}
/// The default boot policy is NEVER an implicit trusted catalog.
pub const fn no_catalog_decision()->Decision{Decision::CpuFallback}

#[cfg(feature="test-m2-developer-alpha")]
pub struct VerifiedCatalog<'a>{
 catalog:&'a SignedCatalog
}
#[cfg(feature="test-m2-developer-alpha")]
impl SignedCatalog{
 /// Only authenticated signer registries populated by *independent* verified
 /// boot roots may supply the public key. RAM-only QEMU test registries and
 /// deterministic test private keys do NOT establish production trust.
 pub fn verify<'a,const N:usize,B:crate::m2_trust::SignatureBackend>(
  &'a self,registry:&crate::m2_trust::Registry<N>,
  backend:&B,minimum_generation:u64
 )->Result<VerifiedCatalog<'a>,Error>{
  self.validate()?;
  if self.generation<minimum_generation{return Err(Error::Rollback);}
  let key=registry.public_key(self.signer,self.epoch).map_err(|e|{
   match e{
    crate::m2_trust::Error::Revoked=>Error::Revoked,
    crate::m2_trust::Error::StaleEpoch=>Error::StaleSigner,
    _=>Error::Unsigned
   }
  })?;
  let hash=crate::m2_sha256::digest(&self.canonical());
  if !backend.verify_ed25519(&key,&hash,&self.signature){
   return Err(Error::Signature);
  }
  Ok(VerifiedCatalog{catalog:self})
 }
}
#[cfg(feature="test-m2-developer-alpha")]
impl VerifiedCatalog<'_>{
 pub fn choose(&self,identity:PciIdentity,cpu:&Cpu)->Result<Decision,Error>{
  let e=match_signed_entry(self.catalog,identity,cpu)?;
  Ok(match e{
   None=>Decision::CpuFallback,
   Some(e)=>Decision::SignedCandidate(Candidate{
    id:e.id,version:e.version,digest:e.digest,requested:e.wants,
    signed_generation:self.catalog.generation
   })
  })
 }
}

/// Hardware-only scan of devices that HD1 already discovered. The scan
/// cannot create an authenticated catalog and therefore NEVER selects an
/// executable driver. Read failures return a safe CPU fallback report.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Preflight{
 pub abi:u16,pub discovered:u8,
 pub identified:u8,pub candidate_count:u8,
 pub driver_activated:bool,pub qpu_available:bool,
 pub cpu_fallback:bool,
}
pub fn preflight<P:ExtendedPciProbe>(
 inventory:&hd0::Inventory,provider:&mut P
)->Preflight{
 let mut identified=0u8;
 for item in inventory.accelerators.iter().flatten(){
  if provider.identify(item.bus,item.slot,item.function,item.header).is_ok(){
   identified=identified.saturating_add(1);
  }
 }
 Preflight{
  abi:POLICY_ABI,discovered:inventory.count,identified,
  candidate_count:0,driver_activated:false,qpu_available:false,
  cpu_fallback:true
 }
}
pub fn format_report(result:&Preflight,out:&mut[u8])->Result<usize,Error>{
 let line=b"IndianOS HD2 driver catalog=unavailable; signed candidates=0; GPU/NPU drivers=not activated; QPU=unavailable; CPU fallback=enabled\n";
 if out.len()<=line.len(){return Err(Error::Invalid);}
 out.fill(0);
 out[..line.len()].copy_from_slice(line);
 Ok(line.len())
}
#[cfg(all(target_arch="x86_64",target_os="none"))]
pub mod native{
 use super::*;
 use crate::hardware_inventory::native::BootPci;
 pub struct ReadOnlyIds;
 impl ExtendedPciProbe for ReadOnlyIds{
  fn identify(&mut self,bus:u8,slot:u8,function:u8,header:PciHeader)
   ->Result<PciIdentity,Error>{
   // PCI header 0 endpoints expose subsystem at 0x2c. Nonzero type
   // headers don't contain that endpoint field: leave subsystem unknown.
   let revision=unsafe{BootPci::read_config(bus,slot,function,0x08)} as u8;
   let subsys=if header.header_type&0x7f==0{
    unsafe{BootPci::read_config(bus,slot,function,0x2c)}
   }else{0};
   Ok(PciIdentity::from_header(header,revision,
    subsys as u16,(subsys>>16) as u16))
  }
 }
}
#[cfg(test)]
mod hd2_tests{
 use super::*;
 use crate::hardware_inventory::{
  CPU_NX,CPU_SSE2,CPU_LONG_MODE,CPU_REQUIRED
 };
 pub(super) fn cpu()->Cpu{Cpu{vendor:*b"GenuineIntel",family:6,model:58,stepping:4,
  features:CPU_NX|CPU_SSE2|CPU_LONG_MODE,max_basic:7,
  max_extended:0x8000_0001}}
 pub(super) fn gpu()->PciIdentity{
  PciIdentity{vendor:0x1af4,device:0x1050,class:3,subclass:0,
   revision:2,subsystem_vendor:0x1af4,subsystem_device:0x1100}
 }
 struct Probe{failed:bool,reads:usize}
 impl ExtendedPciProbe for Probe{
  fn identify(&mut self,_bus:u8,_slot:u8,_function:u8,
   header:PciHeader)->Result<PciIdentity,Error>{
   self.reads+=1;
   if self.failed{return Err(Error::ReadFailed);}
   Ok(PciIdentity::from_header(header,2,0x1af4,0x1100))
  }
 }
 #[test]fn hd2_cpu_fallback_without_authenticated_driver_catalog(){
  assert_eq!(no_catalog_decision(),Decision::CpuFallback);
  let mut inv=hd0::Inventory{abi:hd0::PROFILE_ABI,cpu:cpu(),
   accelerators:[None;MAX_DEVICES],count:1,gpu_count:1,
   npu_count:0,physical_qpu_available:false,cpu_software_fallback:true};
  inv.accelerators[0]=Some(Device{
   bus:2,slot:0,function:0,kind:Accelerator::Gpu,
   header:PciHeader{vendor:0x1af4,device:0x1050,
    class:3,subclass:0,prog_if:0,header_type:0}});
  let mut provider=Probe{failed:false,reads:0};
  let result=preflight(&inv,&mut provider);
  assert_eq!(result.identified,1);
  assert_eq!(result.discovered,1);
  assert_eq!(result.candidate_count,0);
  assert!(!result.driver_activated);
  assert!(!result.qpu_available);
  assert!(result.cpu_fallback);
  assert_eq!(provider.reads,1);
  let mut out=[0u8;160];
  let n=format_report(&result,&mut out).unwrap();
  assert!(core::str::from_utf8(&out[..n]).unwrap()
   .contains("signed candidates=0"));
  provider.failed=true;
  assert_eq!(preflight(&inv,&mut provider).identified,0);
 }
 #[test]fn hd2_catalog_structural_defenses_against_ambiguous_subsystem_and_dma(){
  let mut c=SignedCatalog::empty();
  c.generation=5;c.signer=7;c.epoch=2;c.count=1;
  c.entries[0]=Driver{
   id:1,version:3,abi:DRIVER_ABI,kind:DRIVER_GPU,
   vendor:0x1af4,device:0x1050,
   subsystem_vendor:0,subsystem_device:0,
   revision_min:1,revision_max:9,min_cpu:CPU_REQUIRED,
   wants:WANT_DMA,digest:[9;32]
  };
  assert_eq!(c.validate(),Ok(()));
  c.entries[0].wants=8;assert_eq!(c.validate(),Err(Error::Invalid));
  c.entries[0].wants=WANT_MMIO;c.entries[0].abi=2;
  assert_eq!(c.validate(),Err(Error::Invalid));
  c.entries[0].abi=DRIVER_ABI;
  c.entries[0].subsystem_vendor=1;
  assert_eq!(c.validate(),Err(Error::Invalid));
  c.entries[0].subsystem_vendor=0;
  c.count=2;c.entries[1]=c.entries[0];c.entries[1].id=2;
  assert_eq!(c.validate(),Err(Error::Ambiguous));
  c.entries[1].revision_min=10;c.entries[1].revision_max=15;
  assert_eq!(c.validate(),Ok(()));
  c.entries[1].id=1;assert_eq!(c.validate(),Err(Error::Duplicate));
 }
 #[test]fn hd2_pci_gpu_npu_device_classes_do_not_identify_generic_quantum_hardware(){
  assert_eq!(gpu().kind(),Some(DRIVER_GPU));
  let mut unit=gpu();
  unit.class=0x12;
  assert_eq!(unit.kind(),Some(DRIVER_NPU));
  unit.class=0x08;unit.subclass=0x80;
  assert_eq!(unit.kind(),None);
 }
 #[test]fn hd2_transcript_binds_every_driver_field_and_unused_slot(){
  let mut catalog=SignedCatalog::empty();
  catalog.generation=2;catalog.signer=9;catalog.epoch=3;
  catalog.count=1;
  catalog.entries[0]=Driver{id:33,version:2,
   abi:1,kind:DRIVER_GPU,vendor:0x1af4,device:0x1050,
   subsystem_vendor:0,subsystem_device:0,
   revision_min:0,revision_max:255,
   min_cpu:CPU_REQUIRED,wants:0,digest:[7;32]};
  let canonical=catalog.canonical();
  assert_eq!(canonical.len(),TRANSCRIPT_BYTES);
  catalog.entries[0].version=3;
  assert_ne!(canonical,catalog.canonical());
  catalog.entries[0].version=2;
  catalog.entries[2].digest=[12;32];
  assert_ne!(canonical,catalog.canonical());
  assert_eq!(catalog.validate(),Err(Error::Invalid));
 }
}
#[cfg(all(test,feature="test-m2-developer-alpha"))]
mod hd2_crypto_tests{
 use super::*;
 use super::hd2_tests::{cpu,gpu};
 use crate::m2_ed25519::Ed25519Backend;
 use crate::m2_trust::Registry;
 use crate::batch5_session::Identity;
 use crate::batch6_security::{Access,INSTALL};
 use ed25519_compact::{KeyPair,Seed};
 fn admin()->Identity{Identity{pid:5,generation:2}}
 fn access()->Access<1>{let mut a=Access::new();
  a.grant(admin(),INSTALL,50).unwrap();a}
 fn fixture()->(SignedCatalog,Registry<1>){
  // HOST TEST ONLY — deterministic private signing seed is NOT compiled
  // into a freestanding kernel or used as a production signer.
  let signer=KeyPair::from_seed(Seed::new([0x52;32]));
  let mut public=[0;32];public.copy_from_slice(&signer.pk[..]);
  let mut keys=Registry::<1>::new();
  keys.provision(&access(),admin(),1,17,3,public).unwrap();
  let mut c=SignedCatalog::empty();
  c.generation=9;c.signer=17;c.epoch=3;c.count=2;
  c.entries[0]=Driver{
   id:101,version:3,abi:DRIVER_ABI,kind:DRIVER_GPU,
   vendor:0x1af4,device:0x1050,
   subsystem_vendor:0,subsystem_device:0,
   revision_min:0,revision_max:255,
   min_cpu:hd0::CPU_REQUIRED,wants:WANT_MMIO|WANT_DMA,
   digest:[21;32]};
  c.entries[1]=Driver{
   id:102,version:5,abi:DRIVER_ABI,kind:DRIVER_GPU,
   vendor:0x1af4,device:0x1050,
   subsystem_vendor:0x1af4,subsystem_device:0x1100,
   revision_min:1,revision_max:4,
   min_cpu:hd0::CPU_REQUIRED,wants:WANT_MMIO,
   digest:[22;32]};
  let hash=crate::m2_sha256::digest(&c.canonical());
  let sig=signer.sk.sign(&hash,None);
  c.signature.copy_from_slice(&sig[..]);
  (c,keys)
 }
 #[test]fn hd2_real_ed25519_catalog_matches_exact_gpu_and_subsystem(){
  let (signed,keys)=fixture();
  let verified=signed.verify(&keys,&Ed25519Backend,9).unwrap();
  assert_eq!(verified.choose(gpu(),&cpu()),
   Ok(Decision::SignedCandidate(Candidate{
    id:102,version:5,digest:[22;32],requested:WANT_MMIO,
    signed_generation:9
   })));
  let mut generic=gpu();generic.subsystem_device=0x1010;
  assert_eq!(verified.choose(generic,&cpu()),
   Ok(Decision::SignedCandidate(Candidate{
    id:101,version:3,digest:[21;32],
    requested:WANT_MMIO|WANT_DMA,signed_generation:9
   })));
  generic.vendor=0x8086;
  assert_eq!(verified.choose(generic,&cpu()),Ok(Decision::CpuFallback));
  let mut npu=gpu();npu.class=0x12;
  assert_eq!(verified.choose(npu,&cpu()),Ok(Decision::CpuFallback));
 }
 #[test]fn hd2_tampered_catalog_revoked_signer_and_counter_replay_fail_closed(){
  let (mut signed,mut keys)=fixture();
  assert!(matches!(signed.verify(&keys,&Ed25519Backend,10),
   Err(Error::Rollback)));
  signed.entries[0].wants=0;
  assert!(matches!(signed.verify(&keys,&Ed25519Backend,8),
   Err(Error::Signature)));
  signed.entries[0].wants=WANT_MMIO|WANT_DMA;
  signed.signature[1]^=1;
  assert!(matches!(signed.verify(&keys,&Ed25519Backend,8),
   Err(Error::Signature)));
  signed.signature[1]^=1;
  keys.revoke(&access(),admin(),1,17).unwrap();
  assert!(matches!(signed.verify(&keys,&Ed25519Backend,8),
   Err(Error::Revoked)));
 }
 #[test]fn hd2_signature_binds_revision_version_capabilities_and_signer_epoch(){
  let (mut signed,keys)=fixture();
  signed.entries[1].revision_max=254;
  assert!(matches!(signed.verify(&keys,&Ed25519Backend,8),
   Err(Error::Signature)));
  let (mut signed,keys)=fixture();
  signed.epoch=4;
  assert!(matches!(signed.verify(&keys,&Ed25519Backend,8),
   Err(Error::StaleSigner)));
  let (mut signed,keys)=fixture();
  signed.entries[0].digest[0]^=3;
  assert!(matches!(signed.verify(&keys,&Ed25519Backend,8),
   Err(Error::Signature)));
 }
}
