//! HD3B.5: checked GPU command-to-split-ring integration, mock-only.
//! Never touches real guest RAM, MMIO, IOMMU registers, or virtqueues.
//! Command/response bytes and DMA apertures are untrusted caller-supplied data.
use crate::agent_07_pci_aperture::Aperture;
use crate::batch5_session::Identity;
use crate::hardware_gpu_control_policy::{self as policy,Kind};
use crate::hardware_virtio_gpu as gpu;
use crate::hardware_virtio_queue::{self as queue,StagedQueue};
use crate::hardware_virtio_split::{SplitCodec,PAGE_BYTES};

pub const NODATA_BYTES:usize=gpu::CONTROL_HEADER;
pub const DISPLAY_BYTES:usize=gpu::DISPLAY_INFO_RESPONSE;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{
 Denied,Malformed,Response,Unknown,Policy(policy::Error),Queue(queue::Error),
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Submitted{pub ticket:u64,pub head:u16,pub kind:Kind}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Pending{head:u16,kind:Kind}
pub struct GuardedControlQueue<const N:usize>{
 codec:SplitCodec<N>,
 staged:StagedQueue,
 pending:[Option<Pending>;N],
 retired:bool,
}
fn read16(b:&[u8],at:usize)->Result<u16,Error>{
 let a=b.get(at..at+2).ok_or(Error::Malformed)?;
 Ok(u16::from_le_bytes([a[0],a[1]]))
}
fn read32(b:&[u8],at:usize)->Result<u32,Error>{
 let a=b.get(at..at+4).ok_or(Error::Malformed)?;
 Ok(u32::from_le_bytes([a[0],a[1],a[2],a[3]]))
}
fn wanted_len(kind:Kind)->usize{
 if kind==Kind::GetDisplayInfo{DISPLAY_BYTES}else{NODATA_BYTES}
}
fn validate_response(kind:Kind,response:&[u8])->Result<(),Error>{
 if response.len()!=wanted_len(kind){return Err(Error::Response)}
 let expected=if kind==Kind::GetDisplayInfo{
  gpu::RESP_OK_DISPLAY_INFO
 }else{gpu::RESP_OK_NODATA};
 if read32(response,0)?!=expected||
   response[4..gpu::CONTROL_HEADER].iter().any(|&b|b!=0){
  return Err(Error::Response)
 }
 if kind==Kind::GetDisplayInfo{
  let mut enabled=false;
  for i in 0..16{
   let at=gpu::CONTROL_HEADER+i*24;
   let x=read32(response,at)?;
   let y=read32(response,at+4)?;
   let w=read32(response,at+8)?;
   let h=read32(response,at+12)?;
   let active=read32(response,at+16)?;
   let flags=read32(response,at+20)?;
   if active>1||flags!=0{return Err(Error::Response)}
   if active==1{
    if x!=0||y!=0||w==0||h==0||
       w>gpu::MAX_WIDTH||h>gpu::MAX_HEIGHT{return Err(Error::Response)}
    enabled=true;
   }else if x!=0||y!=0||w!=0||h!=0{
    return Err(Error::Response)
   }
  }
  if !enabled{return Err(Error::Response)}
 }
 Ok(())
}
impl<const N:usize> GuardedControlQueue<N>{
 pub fn new(staged:StagedQueue)->Result<Self,Error>{
  if staged.owner.pid==0||staged.owner.generation==0{
   return Err(Error::Denied)
  }
  let codec=SplitCodec::<N>::new(staged).map_err(Error::Queue)?;
  Ok(Self{codec,staged,pending:[None;N],retired:false})
 }
 pub fn inflight(&self)->usize{self.codec.in_flight()}
 pub fn descriptors(&self)->&[u8;PAGE_BYTES]{self.codec.descriptors()}
 pub fn available(&self)->&[u8;PAGE_BYTES]{self.codec.available()}
 /// Never creates any descriptor before the complete HD3B.4 policy check.
 /// Numerical request addresses have not been independently DMA-attested.
 pub fn submit(&mut self,actor:Identity,command:&[u8],
  request:Aperture,response:Aperture,backing:Option<Aperture>)
  ->Result<Submitted,Error>{
  if self.retired||actor!=self.staged.owner{return Err(Error::Denied)}
  if request.length!=command.len() as u64{return Err(Error::Malformed)}
  let checked=policy::inspect(&self.staged,actor,command,backing)
   .map_err(Error::Policy)?;
  if response.length!=wanted_len(checked.kind) as u64{
   return Err(Error::Response)
  }
  // Check capacity first so bookkeeping cannot fail after enqueue.
  let slot=self.pending.iter().position(Option::is_none)
   .ok_or(Error::Queue(queue::Error::Full))?;
  let enqueued=self.codec.enqueue(actor,request,response).map_err(Error::Queue)?;
  self.pending[slot]=Some(Pending{head:enqueued.descriptor_head,
                                  kind:checked.kind});
  Ok(Submitted{ticket:enqueued.ticket,head:enqueued.descriptor_head,
               kind:checked.kind})
 }
 /// Host fixture only: verifies the used ring index/head/length against a
 /// matching checked command, then validates the exact response header.
 /// Invalid responses leave the descriptor in flight for controlled teardown.
 pub fn complete_mock(&mut self,actor:Identity,
  used:&[u8;PAGE_BYTES],response:&[u8])->Result<u64,Error>{
  if self.retired||actor!=self.staged.owner{return Err(Error::Denied)}
  let idx=read16(used,2)?;
  let at=4+(idx.wrapping_sub(1) as usize%
             self.staged.rings.queue_size as usize)*8;
  let head=read32(used,at)?;
  let length=read32(used,at+4)?;
  let pending=self.pending.iter().flatten()
    .find(|p|p.head as u32==head).copied().ok_or(Error::Unknown)?;
  if length as usize!=response.len(){return Err(Error::Response)}
  validate_response(pending.kind,response)?;
  let ticket=self.codec.accept_mock_used(actor,used).map_err(Error::Queue)?;
  for slot in &mut self.pending{
   if slot.is_some_and(|p|p.head==pending.head){*slot=None;break}
  }
  Ok(ticket)
 }
 pub fn retire(&mut self,actor:Identity)->Result<(),Error>{
  if self.retired||actor!=self.staged.owner{return Err(Error::Denied)}
  self.codec.retire(actor).map_err(Error::Queue)?;
  self.pending.fill(None);self.retired=true;Ok(())
 }
}
#[cfg(test)]
mod hd3b5_tests{
 use super::*;
 use crate::hardware_driver_stage::Bdf;
 use crate::hardware_virtio_queue::{Rings,REQUIRED_FEATURES,PAGE};
 fn actor()->Identity{Identity{pid:13,generation:9}}
 fn ap(base:u64,length:u64)->Aperture{Aperture{base,length}}
 fn queue()->GuardedControlQueue<2>{
  GuardedControlQueue::new(StagedQueue{
   owner:actor(),bdf:Bdf{bus:1,slot:2,function:0},
   iommu_domain:31,features:REQUIRED_FEATURES,notify_offset:0x300c,
   dma:ap(0x2000_0000,0x20000),
   rings:Rings{descriptors:ap(0x2000_0000,PAGE),
    available:ap(0x2000_1000,PAGE),used:ap(0x2000_2000,PAGE),
    queue_size:8},
  }).unwrap()
 }
 fn command(kind:u32)->[u8;56]{
  let mut b=[0u8;56];b[..4].copy_from_slice(&kind.to_le_bytes());b
 }
 fn get()->[u8;56]{command(gpu::GET_DISPLAY_INFO)}
 fn response()->[u8;DISPLAY_BYTES]{
  let mut r=[0u8;DISPLAY_BYTES];
  r[..4].copy_from_slice(&gpu::RESP_OK_DISPLAY_INFO.to_le_bytes());
  r[32..36].copy_from_slice(&64u32.to_le_bytes());
  r[36..40].copy_from_slice(&32u32.to_le_bytes());
  r[40..44].copy_from_slice(&1u32.to_le_bytes());r
 }
 fn used(idx:u16,head:u32,length:u32)->[u8;PAGE_BYTES]{
  let mut b=[0u8;PAGE_BYTES];
  b[2..4].copy_from_slice(&idx.to_le_bytes());
  let at=4+(idx.wrapping_sub(1) as usize%8)*8;
  b[at..at+4].copy_from_slice(&head.to_le_bytes());
  b[at+4..at+8].copy_from_slice(&length.to_le_bytes());b
 }
 fn req()->Aperture{ap(0x2000_3000,24)}
 fn resp()->Aperture{ap(0x2000_4000,DISPLAY_BYTES as u64)}
 #[test]fn hd3b5_policy_approved_request_to_mock_completion(){
  let mut q=queue();let c=get();
  let s=q.submit(actor(),&c[..24],req(),resp(),None).unwrap();
  assert_eq!(s.kind,Kind::GetDisplayInfo);
  assert_eq!(s.head,0);
  assert_eq!(q.inflight(),1);
  assert_eq!(q.complete_mock(actor(),&used(1,0,DISPLAY_BYTES as u32),
    &response()),Ok(s.ticket));
  assert_eq!(q.inflight(),0);
 }
 #[test]fn hd3b5_rejected_command_does_not_publish_descriptors(){
  let mut q=queue();let mut c=get();c[4]=1;
  assert_eq!(q.submit(actor(),&c[..24],req(),resp(),None),
    Err(Error::Policy(policy::Error::Malformed)));
  assert_eq!(q.inflight(),0);
  assert!(q.descriptors().iter().all(|&b|b==0));
  assert!(q.available().iter().all(|&b|b==0));
 }
 #[test]fn hd3b5_wrong_owner_and_wrong_lengths_are_rejected(){
  let mut q=queue();let c=get();
  let stale=Identity{generation:10,..actor()};
  assert_eq!(q.submit(stale,&c[..24],req(),resp(),None),Err(Error::Denied));
  assert_eq!(q.submit(actor(),&c[..24],ap(req().base,40),resp(),None),
   Err(Error::Malformed));
  assert_eq!(q.submit(actor(),&c[..24],req(),ap(resp().base,24),None),
   Err(Error::Response));
 }
 #[test]fn hd3b5_bad_used_or_response_cannot_release_request(){
  let mut q=queue();let c=get();
  let s=q.submit(actor(),&c[..24],req(),resp(),None).unwrap();
  let mut r=response();
  r[..4].copy_from_slice(&gpu::RESP_OK_NODATA.to_le_bytes());
  assert_eq!(q.complete_mock(actor(),&used(1,0,DISPLAY_BYTES as u32),
    &r),Err(Error::Response));
  assert_eq!(q.complete_mock(actor(),&used(1,7,DISPLAY_BYTES as u32),
    &response()),Err(Error::Unknown));
  assert_eq!(q.complete_mock(actor(),&used(1,0,24),
    &response()),Err(Error::Response));
  assert_eq!(q.inflight(),1);
  assert_eq!(q.complete_mock(actor(),&used(1,0,DISPLAY_BYTES as u32),
    &response()),Ok(s.ticket));
 }
 #[test]fn hd3b5_invalid_display_dimensions_and_reserved_bits(){
  let mut q=queue();let c=get();
  q.submit(actor(),&c[..24],req(),resp(),None).unwrap();
  let mut r=response();
  r[32..36].copy_from_slice(&2048u32.to_le_bytes());
  assert_eq!(q.complete_mock(actor(),&used(1,0,DISPLAY_BYTES as u32),
    &r),Err(Error::Response));
  let mut r=response();r[44]=1;
  assert_eq!(q.complete_mock(actor(),&used(1,0,DISPLAY_BYTES as u32),
    &r),Err(Error::Response));
  assert_eq!(q.inflight(),1);
 }
 #[test]fn hd3b5_nodata_reply_requires_matching_command_kind(){
  let mut q=queue();let mut c=command(gpu::RESOURCE_CREATE_2D);
  c[24..28].copy_from_slice(&1u32.to_le_bytes());
  c[28..32].copy_from_slice(&gpu::FORMAT_B8G8R8A8_UNORM.to_le_bytes());
  c[32..36].copy_from_slice(&64u32.to_le_bytes());
  c[36..40].copy_from_slice(&32u32.to_le_bytes());
  let s=q.submit(actor(),&c[..40],ap(req().base,40),
    ap(resp().base,24),None).unwrap();
  let mut r=[0u8;24];
  r[..4].copy_from_slice(&gpu::RESP_OK_NODATA.to_le_bytes());
  assert_eq!(q.complete_mock(actor(),&used(1,0,24),&r),Ok(s.ticket));
 }
 #[test]fn hd3b5_ring_overlap_and_inflight_buffer_reuse_fail_closed(){
  let mut q=queue();let c=get();
  assert_eq!(q.submit(actor(),&c[..24],ap(0x2000_0000,24),
   resp(),None),Err(Error::Queue(queue::Error::Overlap)));
  let s=q.submit(actor(),&c[..24],req(),resp(),None).unwrap();
  assert_eq!(q.submit(actor(),&c[..24],req(),
    ap(0x2000_5000,DISPLAY_BYTES as u64),None),
    Err(Error::Queue(queue::Error::Overlap)));
  assert_eq!(q.inflight(),1);
  assert_eq!(q.complete_mock(actor(),&used(1,s.head as u32,
    DISPLAY_BYTES as u32),&response()),Ok(s.ticket));
 }
 #[test]fn hd3b5_retired_owner_cannot_reactivate_descriptors(){
  let mut q=queue();let c=get();
  q.submit(actor(),&c[..24],req(),resp(),None).unwrap();
  let stale=Identity{generation:10,..actor()};
  assert_eq!(q.retire(stale),Err(Error::Denied));
  q.retire(actor()).unwrap();
  assert_eq!(q.inflight(),0);
  assert_eq!(q.submit(actor(),&c[..24],req(),resp(),None),Err(Error::Denied));
 }
}