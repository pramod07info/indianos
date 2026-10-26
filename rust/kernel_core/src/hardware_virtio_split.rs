//! Issue #124 HD3B.3: safe Rust wire image of a mock split virtqueue.
//!
//! The static descriptor and available-ring byte arrays follow virtio 1.2.
//! This module NEVER maps PCI/MMIO, performs DMA, reads a device-owned ring,
//! submits to a real device, or assumes a claimed IOMMU domain is attested.
//! All "used" bytes are explicitly injected by trusted host tests. Actual
//! driver submission requires pinned, isolated memory and correct DMA fences.
//! This is NOT a working virtio transport or GPU acceleration.
use crate::agent_07_pci_aperture::Aperture;
use crate::batch5_session::Identity;
use crate::hardware_virtio_queue::{QueueModel,StagedQueue,Error,PAGE};

pub const DESC_F_NEXT:u16=1;
pub const DESC_F_WRITE:u16=2;
pub const DESCRIPTOR_BYTES:usize=16;
pub const AVAIL_HEADER_BYTES:usize=4;
pub const USED_HEADER_BYTES:usize=4;
pub const USED_ELEMENT_BYTES:usize=8;
pub const PAGE_BYTES:usize=PAGE as usize;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Enqueued{
 pub ticket:u64,
 pub descriptor_head:u16,
 pub available_index:u16,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Pending{head:u16,ticket:u64}
/// Allocation-free, test-only byte encoding; host code can inspect snapshots.
/// Each command reserves two direct descriptors: device-readable request and
/// device-writable response. Indirect descriptors and EVENT_IDX are omitted.
pub struct SplitCodec<const N:usize>{
 model:QueueModel<N>,
 descriptors:[u8;PAGE_BYTES],
 available:[u8;PAGE_BYTES],
 pending:[Option<Pending>;N],
 avail_index:u16,
 used_index:u16,
 retired:bool,
}
fn le16(input:&[u8],offset:usize)->u16{
 u16::from_le_bytes([input[offset],input[offset+1]])
}
fn le32(input:&[u8],offset:usize)->u32{
 u32::from_le_bytes([input[offset],input[offset+1],
                     input[offset+2],input[offset+3]])
}
fn write16(dst:&mut[u8],offset:usize,value:u16){
 dst[offset..offset+2].copy_from_slice(&value.to_le_bytes());
}
fn write32(dst:&mut[u8],offset:usize,value:u32){
 dst[offset..offset+4].copy_from_slice(&value.to_le_bytes());
}
fn write64(dst:&mut[u8],offset:usize,value:u64){
 dst[offset..offset+8].copy_from_slice(&value.to_le_bytes());
}
fn descriptor(out:&mut[u8;PAGE_BYTES],index:usize,
              address:u64,length:u32,flags:u16,next:u16){
 let at=index*DESCRIPTOR_BYTES;
 write64(out,at,address);
 write32(out,at+8,length);
 write16(out,at+12,flags);
 write16(out,at+14,next);
}
impl<const N:usize> SplitCodec<N>{
 pub fn new(staged:StagedQueue)->Result<Self,Error>{
  // Two direct descriptors per inflight request. The qsz supplied by the
  // device is only numeric planning metadata, not an enabled virtqueue.
  let qsz=staged.rings.queue_size;
  if N==0||N>128||qsz<2||qsz>256||!qsz.is_power_of_two()||
     N*2>qsz as usize{
   return Err(Error::Invalid)
  }
  // StagedQueue has public fields: never trust a hand-built instance as
  // proof of the HD3B.2 feature, owner or IOMMU gates.
  if staged.owner.pid==0||staged.owner.generation==0||
     staged.iommu_domain==0||
     staged.features&crate::hardware_virtio_queue::REQUIRED_FEATURES!=
       crate::hardware_virtio_queue::REQUIRED_FEATURES{
   return Err(Error::NotIsolated)
  }
  Ok(Self{model:QueueModel::new(staged)?,
    descriptors:[0;PAGE_BYTES],available:[0;PAGE_BYTES],
    pending:[None;N],avail_index:0,used_index:0,retired:false})
 }
 pub fn descriptors(&self)->&[u8;PAGE_BYTES]{&self.descriptors}
 pub fn available(&self)->&[u8;PAGE_BYTES]{&self.available}
 pub fn in_flight(&self)->usize{self.model.inflight()}
 /// Encode a request descriptor and matching writable response descriptor.
 /// This is an in-memory MODEL, never a real device-visible producer.
 pub fn enqueue(&mut self,actor:Identity,request:Aperture,
                response:Aperture)->Result<Enqueued,Error>{
  if self.retired||actor!=self.model.staged.owner{
   return Err(Error::Denied)
  }
  let slot=self.pending.iter().position(Option::is_none)
   .ok_or(Error::Full)?;
  // QueueModel checks all DMA bounds, overlap, identity and ticket wrap.
  let ticket=self.model.submit(actor,request,response)?;
  let head=(slot*2) as u16;
  let base=head as usize;
  descriptor(&mut self.descriptors,base,request.base,
             request.length as u32,DESC_F_NEXT,head+1);
  descriptor(&mut self.descriptors,base+1,response.base,
             response.length as u32,DESC_F_WRITE,0);
  let qsz=self.model.staged.rings.queue_size;
  let ring_offset=AVAIL_HEADER_BYTES+
       (self.avail_index as usize%qsz as usize)*2;
  write16(&mut self.available,ring_offset,head);
  self.avail_index=self.avail_index.wrapping_add(1);
  write16(&mut self.available,2,self.avail_index);
  self.pending[slot]=Some(Pending{head,ticket});
  Ok(Enqueued{ticket,descriptor_head:head,
              available_index:self.avail_index})
 }
 /// Read a mock device-owned used ring with virtio le16 idx and
 /// le32 {descriptor_head, byte_count} at the next ring position.
 /// A correct production backend MUST handle DMA synchronisation, volatile
 /// accesses and cache coherence BEFORE calling any equivalent validator.
 pub fn accept_mock_used(&mut self,actor:Identity,
  device_used:&[u8;PAGE_BYTES])->Result<u64,Error>{
  if self.retired||actor!=self.model.staged.owner{
   return Err(Error::Denied)
  }
  let device_index=le16(device_used,2);
  if device_index!=self.used_index.wrapping_add(1){
   return Err(Error::BadCompletion)
  }
  let qsz=self.model.staged.rings.queue_size as usize;
  let at=USED_HEADER_BYTES+(self.used_index as usize%qsz)*USED_ELEMENT_BYTES;
  let id=le32(device_used,at);
  let length=le32(device_used,at+4);
  let entry=self.pending.iter().position(|p|
     p.is_some_and(|v|v.head as u32==id))
     .ok_or(Error::UnknownCompletion)?;
  let pending=self.pending[entry].ok_or(Error::UnknownCompletion)?;
  self.model.complete(actor,pending.ticket,length)?;
  self.pending[entry]=None;
  self.descriptors[entry*2*DESCRIPTOR_BYTES..
    (entry*2+2)*DESCRIPTOR_BYTES].fill(0);
  self.used_index=device_index;
  Ok(pending.ticket)
 }
 /// Tear down the model on process exit/device loss. Pending memory is
 /// cleared; this codec can never be reused after retirement.
 pub fn retire(&mut self,actor:Identity)->Result<(),Error>{
  if self.retired||actor!=self.model.staged.owner{
   return Err(Error::Denied)
  }
  self.model.retire(actor)?;
  self.pending.fill(None);
  self.descriptors.fill(0);self.available.fill(0);
  self.retired=true;
  Ok(())
 }
}
#[cfg(test)]
mod hd3b3_tests{
 use super::*;
 use crate::hardware_driver_stage::Bdf;
 use crate::hardware_virtio_queue::{Rings,REQUIRED_FEATURES};
 fn owner()->Identity{Identity{pid:9,generation:4}}
 fn ap(base:u64,length:u64)->Aperture{Aperture{base,length}}
 fn staged()->StagedQueue{
  StagedQueue{
   owner:owner(),bdf:Bdf{bus:1,slot:2,function:0},iommu_domain:8,
   features:REQUIRED_FEATURES,notify_offset:0x300c,
   dma:ap(0x2000_0000,0x20000),
   rings:Rings{descriptors:ap(0x2000_0000,PAGE),
    available:ap(0x2000_1000,PAGE),
    used:ap(0x2000_2000,PAGE),queue_size:8},
  }
 }
 fn request(n:u64)->Aperture{ap(0x2000_3000+n*0x1000,64)}
 fn response(n:u64)->Aperture{ap(0x2000_4000+n*0x1000,128)}
 fn used(idx:u16,head:u32,len:u32,qsz:usize)->[u8;PAGE_BYTES]{
  let mut b=[0;PAGE_BYTES];write16(&mut b,2,idx);
  let at=USED_HEADER_BYTES+((idx.wrapping_sub(1) as usize)%qsz)*8;
  write32(&mut b,at,head);write32(&mut b,at+4,len);b
 }
 #[test]fn hd3b3_descriptor_layout_is_exact_virtio_little_endian(){
  let mut q=SplitCodec::<2>::new(staged()).unwrap();
  let e=q.enqueue(owner(),request(0),response(0)).unwrap();
  assert_eq!(e.descriptor_head,0);
  assert_eq!(e.available_index,1);
  let d=q.descriptors();
  assert_eq!(&d[0..8],&request(0).base.to_le_bytes());
  assert_eq!(le32(d,8),64);assert_eq!(le16(d,12),DESC_F_NEXT);
  assert_eq!(le16(d,14),1);
  assert_eq!(le32(d,24),128);assert_eq!(le16(d,28),DESC_F_WRITE);
  assert_eq!(le16(d,30),0);
  let a=q.available();assert_eq!(le16(a,2),1);
  assert_eq!(le16(a,4),0);
  assert_eq!(q.in_flight(),1);
 }
 #[test]fn hd3b3_ooo_mock_used_completions_and_reclaim(){
  let mut q=SplitCodec::<2>::new(staged()).unwrap();
  let a=q.enqueue(owner(),request(0),response(0)).unwrap();
  // Use distant disjoint buffer addresses for the second request.
  let b=q.enqueue(owner(),request(2),response(2)).unwrap();
  assert_eq!((a.descriptor_head,b.descriptor_head),(0,2));
  assert_eq!(q.accept_mock_used(owner(),&used(1,2,40,8)),Ok(b.ticket));
  assert_eq!(q.in_flight(),1);
  assert!(q.descriptors()[32..64].iter().all(|&v|v==0));
  assert_eq!(q.accept_mock_used(owner(),&used(2,0,24,8)),Ok(a.ticket));
  assert_eq!(q.in_flight(),0);
 }
 #[test]fn hd3b3_rejects_stale_generation_bad_used_idx_and_bad_len(){
  let mut q=SplitCodec::<1>::new(staged()).unwrap();
  let a=q.enqueue(owner(),request(0),response(0)).unwrap();
  let stale=Identity{generation:5,..owner()};
  assert_eq!(q.accept_mock_used(stale,&used(1,0,32,8)),Err(Error::Denied));
  assert_eq!(q.accept_mock_used(owner(),&used(2,0,32,8)),
             Err(Error::BadCompletion));
  assert_eq!(q.accept_mock_used(owner(),&used(1,0,129,8)),
             Err(Error::BadCompletion));
  assert_eq!(q.in_flight(),1);
  assert_eq!(q.accept_mock_used(owner(),&used(1,0,64,8)),Ok(a.ticket));
  assert_eq!(q.accept_mock_used(owner(),&used(1,0,64,8)),
             Err(Error::BadCompletion));
 }
 #[test]fn hd3b3_rejects_unknown_descriptor_and_preserves_ticket(){
  let mut q=SplitCodec::<1>::new(staged()).unwrap();
  let a=q.enqueue(owner(),request(0),response(0)).unwrap();
  assert_eq!(q.accept_mock_used(owner(),&used(1,6,64,8)),
             Err(Error::UnknownCompletion));
  assert_eq!(q.in_flight(),1);
  assert_eq!(q.accept_mock_used(owner(),&used(1,0,64,8)),Ok(a.ticket));
 }
 #[test]fn hd3b3_bounded_capacity_and_reuse_without_alias(){
  let mut q=SplitCodec::<1>::new(staged()).unwrap();
  let a=q.enqueue(owner(),request(0),response(0)).unwrap();
  assert_eq!(q.enqueue(owner(),request(2),response(2)),Err(Error::Full));
  assert_eq!(q.accept_mock_used(owner(),&used(1,0,32,8)),Ok(a.ticket));
  let b=q.enqueue(owner(),request(0),response(0)).unwrap();
  assert!(b.ticket>a.ticket);
  assert_eq!(le16(q.available(),2),2);
  assert_eq!(le16(q.available(),6),0); // second available slot
  assert_eq!(q.accept_mock_used(owner(),&used(2,0,16,8)),Ok(b.ticket));
 }
 #[test]fn hd3b3_mock_buffers_cannot_overlap_or_alias_rings(){
  let mut q=SplitCodec::<2>::new(staged()).unwrap();
  assert_eq!(q.enqueue(owner(),ap(0x2000_0000,64),response(0)),
             Err(Error::Overlap));
  assert_eq!(q.enqueue(owner(),request(0),request(0)),
             Err(Error::Overlap));
  let e=q.enqueue(owner(),request(0),response(0)).unwrap();
  assert_eq!(q.enqueue(owner(),response(0),response(2)),
             Err(Error::Overlap));
  assert_eq!(q.in_flight(),1);
  assert_eq!(q.accept_mock_used(owner(),&used(1,e.descriptor_head as u32,20,8)),
             Ok(e.ticket));
 }
 #[test]fn hd3b3_teardown_revokes_all_pending_wire_images(){
  let mut q=SplitCodec::<2>::new(staged()).unwrap();
  q.enqueue(owner(),request(0),response(0)).unwrap();
  let stale=Identity{generation:10,..owner()};
  assert_eq!(q.retire(stale),Err(Error::Denied));
  assert_eq!(q.in_flight(),1);
  q.retire(owner()).unwrap();
  assert_eq!(q.in_flight(),0);
  assert!(q.descriptors().iter().all(|&v|v==0));
  assert!(q.available().iter().all(|&v|v==0));
  assert_eq!(q.enqueue(owner(),request(0),response(0)),Err(Error::Denied));
  assert_eq!(q.accept_mock_used(owner(),&used(1,0,12,8)),
             Err(Error::Denied));
 }
 #[test]fn hd3b3_rejects_forged_staging_without_isolation_or_power_of_two(){
  let mut p=staged();p.rings.queue_size=3;
  assert!(matches!(SplitCodec::<1>::new(p),Err(Error::Invalid)));
  let mut p=staged();p.iommu_domain=0;
  assert!(matches!(SplitCodec::<1>::new(p),Err(Error::NotIsolated)));
  let mut p=staged();p.features=0;
  assert!(matches!(SplitCodec::<1>::new(p),Err(Error::NotIsolated)));
  let mut p=staged();p.owner.generation=0;
  assert!(matches!(SplitCodec::<1>::new(p),Err(Error::NotIsolated)));
  let mut p=staged();p.rings.queue_size=1024;
  assert!(matches!(SplitCodec::<1>::new(p),Err(Error::Invalid)));
 }
 #[test]fn hd3b3_rejects_descriptor_capacity_overcommit(){
  assert!(matches!(SplitCodec::<5>::new(staged()),Err(Error::Invalid)));
  assert!(matches!(SplitCodec::<0>::new(staged()),Err(Error::Invalid)));
 }
}