//! HD3B.2: allocation-free, fail-closed virtio-GPU control virtqueue plans.
//!
//! These types validate NUMERIC layouts and model descriptor ownership only.
//! They DO NOT map BARs, program an IOMMU, share memory with a device, enable
//! bus mastering, negotiate with a physical device or display GPU pixels.
//! Hardware activation requires independently attested IOMMU mappings,
//! pinned pages, a signed executable driver and isolated MMIO transport.
//! No boot path calls this planning module; CPU fallback remains mandatory.
use crate::agent_07_pci_aperture::Aperture;
use crate::batch5_session::Identity;
use crate::hardware_driver_stage::{Bdf,ResourcePlan};
use crate::hardware_virtio_gpu::Capabilities;

pub const PAGE:u64=4096;
pub const MAX_QUEUE_SIZE:u16=256;
pub const VIRTIO_F_VERSION_1:u64=1u64<<32;
pub const VIRTIO_F_ACCESS_PLATFORM:u64=1u64<<33;
pub const REQUIRED_FEATURES:u64=VIRTIO_F_VERSION_1|VIRTIO_F_ACCESS_PLATFORM;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{
 Denied,NotIsolated,Invalid,Range,Overlap,Unsupported,Full,
 UnknownCompletion,BadCompletion,Exhausted,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Rings{
 pub descriptors:Aperture,pub available:Aperture,pub used:Aperture,
 pub queue_size:u16,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct StagedQueue{
 pub owner:Identity,pub bdf:Bdf,pub iommu_domain:u32,
 pub features:u64,pub notify_offset:u32,
 pub dma:Aperture,pub rings:Rings,
}
impl StagedQueue{
 /// A plan never enables hardware, even when tests inject positive evidence.
 pub const fn hardware_enabled(&self)->bool{false}
}
fn end(a:Aperture)->Result<u64,Error>{
 if a.length==0{return Err(Error::Range)}
 a.base.checked_add(a.length).ok_or(Error::Range)
}
fn contains(outer:Aperture,inner:Aperture)->Result<(),Error>{
 let outer_end=end(outer)?;
 let inner_end=end(inner)?;
 if inner.base<outer.base||inner_end>outer_end{return Err(Error::Range)}
 Ok(())
}
fn overlaps(a:Aperture,b:Aperture)->Result<bool,Error>{
 let ae=end(a)?;let be=end(b)?;
 Ok(a.base<be&&b.base<ae)
}
fn validate_ring(dma:Aperture,ring:Aperture,minimum:u64)->Result<(),Error>{
 if ring.base==0||ring.base%PAGE!=0||
    ring.length!=PAGE||minimum>ring.length{return Err(Error::Range)}
 contains(dma,ring)
}
/// Pure feature-mask decision: ACCESS_PLATFORM permits IOMMU-aware virtio
/// semantics, but the bits alone do NOT prove that a real IOMMU is enabled.
pub fn select_features(offered:u64)->Result<u64,Error>{
 if offered&REQUIRED_FEATURES!=REQUIRED_FEATURES{
  return Err(Error::Unsupported)
 }
 Ok(REQUIRED_FEATURES)
}
/// Checked numeric notify location relative to a validated PCI capability.
/// Does not dereference or map the PCI BAR; the queue notification write is
/// deliberately absent until the hardware isolation milestone is complete.
pub fn notify_offset(caps:&Capabilities,queue_notify_off:u16)
 ->Result<u32,Error>{
 let n=&caps.notify;
 if n.length<2||n.notify_multiplier==0{return Err(Error::Range)}
 let offset=(queue_notify_off as u32).checked_mul(n.notify_multiplier)
  .ok_or(Error::Range)?;
 let end_offset=offset.checked_add(2).ok_or(Error::Range)?;
 if end_offset>n.length{return Err(Error::Range)}
 n.offset.checked_add(offset).ok_or(Error::Range)
}
/// Request an isolated layout inside a previously *proposed* HD3A DMA
/// aperture. The upstream ResourcePlan is untrusted numeric data, NOT an
/// IOMMU authorization token. Recheck every field on any eventual binding.
pub fn stage(plan:&ResourcePlan,actor:Identity,device:Bdf,
 offered:u64,caps:&Capabilities,rings:Rings,notify_index:u16)
 ->Result<StagedQueue,Error>{
 if actor.pid==0||actor.generation==0||actor!=plan.owner||
    device!=plan.bdf||device.bus!=caps.bdf.bus||
    device.slot!=caps.bdf.slot||device.function!=caps.bdf.function||
    plan.driver_id==0||plan.activated{
  return Err(Error::Denied);
 }
 let dma=plan.dma.ok_or(Error::NotIsolated)?;
 if plan.iommu_domain==0||plan.mmio.is_none(){
  return Err(Error::NotIsolated);
 }
 if rings.queue_size<2||rings.queue_size>MAX_QUEUE_SIZE||
    !rings.queue_size.is_power_of_two(){
  return Err(Error::Invalid);
 }
 let q=rings.queue_size as u64;
 // Virtio split-ring layout: 16 bytes/descriptor, 4+2*q avail ring,
 // 4+8*q used ring; reserve 2 bytes for EVENT_IDX on both rings.
 validate_ring(dma,rings.descriptors,16*q)?;
 validate_ring(dma,rings.available,6+2*q)?;
 validate_ring(dma,rings.used,6+8*q)?;
 let ranges=[rings.descriptors,rings.available,rings.used];
 for (i,a) in ranges.iter().enumerate(){
  for b in &ranges[..i]{
   if overlaps(*a,*b)?{return Err(Error::Overlap)}
  }
  if overlaps(*a,plan.mmio.ok_or(Error::NotIsolated)?)?{
   return Err(Error::Overlap);
  }
 }
 let features=select_features(offered)?;
 let notify_offset=notify_offset(caps,notify_index)?;
 Ok(StagedQueue{owner:actor,bdf:device,iommu_domain:plan.iommu_domain,
  features,notify_offset,dma,rings})
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Pending{ticket:u64,request:Aperture,response:Aperture}
/// Host-verifiable descriptor accounting. Neither this queue model nor its
/// completion method writes any guest physical memory or PCI registers.
pub struct QueueModel<const N:usize>{
 pub staged:StagedQueue,
 slots:[Option<Pending>;N],
 next:u64,
}
impl<const N:usize> QueueModel<N>{
 pub fn new(staged:StagedQueue)->Result<Self,Error>{
  let rings=staged.rings;
  if staged.owner.pid==0||staged.owner.generation==0||
     staged.iommu_domain==0||
     staged.features&REQUIRED_FEATURES!=REQUIRED_FEATURES||
     rings.queue_size<2||rings.queue_size>MAX_QUEUE_SIZE||
     !rings.queue_size.is_power_of_two()||
     N==0||N>rings.queue_size as usize||staged.hardware_enabled(){
   return Err(Error::Invalid);
  }
  // Public StagedQueue values can bypass stage(). Repeat numeric checks
  // before descriptor accounting. This never proves hardware isolation.
  let q=rings.queue_size as u64;
  validate_ring(staged.dma,rings.descriptors,16*q)?;
  validate_ring(staged.dma,rings.available,6+2*q)?;
  validate_ring(staged.dma,rings.used,6+8*q)?;
  let list=[rings.descriptors,rings.available,rings.used];
  for (i,ap) in list.iter().enumerate(){
   for other in &list[..i]{
    if overlaps(*ap,*other)?{return Err(Error::Overlap);}
   }
  }
  Ok(Self{staged,slots:[None;N],next:1})
 }
 fn buffer(&self,ap:Aperture)->Result<(),Error>{
  if ap.base==0||ap.base%4!=0||ap.length==0||
     ap.length>1024*1024{return Err(Error::Range)}
  contains(self.staged.dma,ap)?;
  let rings=self.staged.rings;
  for ring in [rings.descriptors,rings.available,rings.used]{
   if overlaps(ap,ring)?{return Err(Error::Overlap)}
  }
  Ok(())
 }
 /// Safe numeric ownership check only: never ring doorbells or DMA writes.
 pub fn submit(&mut self,actor:Identity,request:Aperture,
               response:Aperture)->Result<u64,Error>{
  if actor!=self.staged.owner{return Err(Error::Denied)}
  self.buffer(request)?;self.buffer(response)?;
  if overlaps(request,response)?{return Err(Error::Overlap)}
  // Reusing a buffer while another command owns it may corrupt the guest.
  for entry in self.slots.iter().flatten(){
   if overlaps(request,entry.request)?||overlaps(request,entry.response)?||
      overlaps(response,entry.request)?||overlaps(response,entry.response)?{
    return Err(Error::Overlap)
   }
  }
  let slot=self.slots.iter().position(Option::is_none).ok_or(Error::Full)?;
  let ticket=self.next;
  self.next=self.next.checked_add(1).ok_or(Error::Exhausted)?;
  self.slots[slot]=Some(Pending{ticket,request,response});
  Ok(ticket)
 }
 /// Validate a reported completion length and free its numeric buffers.
 /// Actual used-ring reads must come from a future trusted backend.
 pub fn complete(&mut self,actor:Identity,ticket:u64,used:u32)
  ->Result<(),Error>{
  if actor!=self.staged.owner{return Err(Error::Denied)}
  let slot=self.slots.iter().position(|x|
   x.is_some_and(|p|p.ticket==ticket)).ok_or(Error::UnknownCompletion)?;
  let item=self.slots[slot].ok_or(Error::UnknownCompletion)?;
  if used==0||u64::from(used)>item.response.length{
   return Err(Error::BadCompletion)
  }
  self.slots[slot]=None;Ok(())
 }
 /// Quiesce every staged descriptor on process teardown or device loss.
 pub fn retire(&mut self,actor:Identity)->Result<(),Error>{
  if actor!=self.staged.owner{return Err(Error::Denied)}
  self.slots.fill(None);Ok(())
 }
 pub fn inflight(&self)->usize{
  self.slots.iter().filter(|entry|entry.is_some()).count()
 }
}
#[cfg(test)]
mod hd3b2_tests{
 use super::*;
 use crate::hardware_virtio_gpu::{Capability,Capabilities,Bdf as GpuBdf};
 fn owner()->Identity{Identity{pid:7,generation:3}}
 fn bdf()->Bdf{Bdf{bus:1,slot:2,function:0}}
 fn ap(base:u64,length:u64)->Aperture{Aperture{base,length}}
 fn plan()->ResourcePlan{ResourcePlan{
  owner:owner(),bdf:bdf(),driver_id:5,
  mmio:Some(ap(0x1000_0000,0x4000)),
  dma:Some(ap(0x2000_0000,0x20000)),
  iommu_domain:9,activated:false
 }}
 fn caps()->Capabilities{
  let cap=Capability{bar:4,offset:0,length:4096,notify_multiplier:0};
  Capabilities{bdf:GpuBdf{bus:1,slot:2,function:0},
   common:cap,isr:cap,device:cap,
   notify:Capability{bar:4,offset:0x3000,length:4096,
    notify_multiplier:4},visited:4}
 }
 fn rings()->Rings{Rings{
  descriptors:ap(0x2000_0000,PAGE),
  available:ap(0x2000_1000,PAGE),
  used:ap(0x2000_2000,PAGE),queue_size:8}}
 fn staged()->StagedQueue{
  stage(&plan(),owner(),bdf(),REQUIRED_FEATURES,&caps(),rings(),3).unwrap()
 }
 #[test]fn hd3b2_correct_split_queue_geometry_and_default_disabled(){
  let s=staged();
  assert!(!s.hardware_enabled());assert_eq!(s.notify_offset,0x300c);
  assert_eq!(s.features,REQUIRED_FEATURES);
  assert_eq!(s.iommu_domain,9);
 }
 #[test]fn hd3b2_no_access_platform_or_modern_features(){
  assert_eq!(select_features(VIRTIO_F_VERSION_1),Err(Error::Unsupported));
  assert_eq!(select_features(VIRTIO_F_ACCESS_PLATFORM),Err(Error::Unsupported));
  assert_eq!(select_features(REQUIRED_FEATURES),Ok(REQUIRED_FEATURES));
 }
 #[test]fn hd3b2_rejects_untrusted_dma_owner_or_pci_binding(){
  let mut p=plan();p.iommu_domain=0;
  assert_eq!(stage(&p,owner(),bdf(),REQUIRED_FEATURES,&caps(),rings(),3),
             Err(Error::NotIsolated));
  let mut p=plan();p.dma=None;
  assert_eq!(stage(&p,owner(),bdf(),REQUIRED_FEATURES,&caps(),rings(),3),
             Err(Error::NotIsolated));
  let mut p=plan();p.activated=true;
  assert_eq!(stage(&p,owner(),bdf(),REQUIRED_FEATURES,&caps(),rings(),3),
             Err(Error::Denied));
  let stale=Identity{generation:4,..owner()};
  assert_eq!(stage(&plan(),stale,bdf(),REQUIRED_FEATURES,&caps(),rings(),3),
             Err(Error::Denied));
  let wrong=Bdf{slot:3,..bdf()};
  assert_eq!(stage(&plan(),owner(),wrong,REQUIRED_FEATURES,&caps(),rings(),3),
             Err(Error::Denied));
 }
 #[test]fn hd3b2_malformed_ring_geometry_overlap_overflow_and_zero(){
  let mut r=rings();r.queue_size=3;
  assert_eq!(stage(&plan(),owner(),bdf(),REQUIRED_FEATURES,&caps(),r,3),
             Err(Error::Invalid));
  let mut r=rings();r.queue_size=512;
  assert_eq!(stage(&plan(),owner(),bdf(),REQUIRED_FEATURES,&caps(),r,3),
             Err(Error::Invalid));
  let mut r=rings();r.used=r.available;
  assert_eq!(stage(&plan(),owner(),bdf(),REQUIRED_FEATURES,&caps(),r,3),
             Err(Error::Overlap));
  let mut r=rings();r.used.base=u64::MAX-4095;
  assert_eq!(stage(&plan(),owner(),bdf(),REQUIRED_FEATURES,&caps(),r,3),
             Err(Error::Range));
  let mut r=rings();r.used=ap(0x2002_0000,PAGE);
  assert_eq!(stage(&plan(),owner(),bdf(),REQUIRED_FEATURES,&caps(),r,3),
             Err(Error::Range));
 }
 #[test]fn hd3b2_notify_multiplier_out_of_bounds(){
  assert_eq!(notify_offset(&caps(),1024),Err(Error::Range));
  let mut c=caps();c.notify.notify_multiplier=u32::MAX;
  assert_eq!(notify_offset(&c,3),Err(Error::Range));
 }
 #[test]fn hd3b2_numeric_descriptor_lifetime_and_completion(){
  let mut q=QueueModel::<2>::new(staged()).unwrap();
  let req=ap(0x2000_3000,64);
  let resp=ap(0x2000_4000,128);
  let t=q.submit(owner(),req,resp).unwrap();
  assert_eq!(q.inflight(),1);
  assert_eq!(q.submit(owner(),req,ap(0x2000_5000,128)),Err(Error::Overlap));
  assert_eq!(q.complete(owner(),t,129),Err(Error::BadCompletion));
  assert_eq!(q.inflight(),1);
  assert_eq!(q.complete(owner(),t,64),Ok(()));
  assert_eq!(q.complete(owner(),t,64),Err(Error::UnknownCompletion));
  assert_eq!(q.inflight(),0);
 }
 #[test]fn hd3b2_denies_cross_process_buffer_reuse_and_overflow(){
  let mut q=QueueModel::<1>::new(staged()).unwrap();
  let stale=Identity{generation:4,..owner()};
  let req=ap(0x2000_3000,64);let resp=ap(0x2000_4000,64);
  assert_eq!(q.submit(stale,req,resp),Err(Error::Denied));
  let ticket=q.submit(owner(),req,resp).unwrap();
  assert_eq!(q.submit(owner(),ap(0x2000_5000,64),ap(0x2000_6000,64)),
             Err(Error::Full));
  assert_eq!(q.complete(stale,ticket,64),Err(Error::Denied));
  assert_eq!(q.retire(stale),Err(Error::Denied));
  assert_eq!(q.retire(owner()),Ok(()));
  assert_eq!(q.inflight(),0);
 }
 #[test]fn r055_forged_queue_plan_cannot_bypass_ring_boundaries(){
  let mut fake=staged();
  fake.rings.used=fake.rings.available;
  assert!(matches!(QueueModel::<2>::new(fake),Err(Error::Overlap)));
  fake=staged();fake.rings.descriptors=ap(0x2000_0001,4096);
  assert!(matches!(QueueModel::<2>::new(fake),Err(Error::Range)));
  fake=staged();fake.rings.queue_size=3;
  assert!(matches!(QueueModel::<2>::new(fake),Err(Error::Invalid)));
  fake=staged();fake.rings.used=ap(u64::MAX-4095,4096);
  assert!(QueueModel::<2>::new(fake).is_err());
 }
 #[test]fn r055_forged_owner_feature_or_isolation_denied(){
  let mut fake=staged();fake.owner.pid=0;
  assert!(matches!(QueueModel::<2>::new(fake),Err(Error::Invalid)));
  fake=staged();fake.features=VIRTIO_F_VERSION_1;
  assert!(matches!(QueueModel::<2>::new(fake),Err(Error::Invalid)));
  fake=staged();fake.iommu_domain=0;
  assert!(matches!(QueueModel::<2>::new(fake),Err(Error::Invalid)));
  assert!(QueueModel::<2>::new(staged()).is_ok());
 }
 #[test]fn hd3b2_rejects_ring_aliases_and_device_buffers(){
  let mut q=QueueModel::<2>::new(staged()).unwrap();
  assert_eq!(q.submit(owner(),ap(0x2000_0000,64),ap(0x2000_4000,64)),
             Err(Error::Overlap));
  assert_eq!(q.submit(owner(),ap(0x2000_3000,4096),ap(0x2000_3800,64)),
             Err(Error::Overlap));
  assert_eq!(q.submit(owner(),ap(u64::MAX-3,64),ap(0x2000_4000,64)),
             Err(Error::Range));
 }
}