//! HD3B.4: fail-closed virtio-GPU 2D command and framebuffer policy.
//!
//! This decoder validates PROPOSED command bytes, never executes them.
//! A numeric HD3B.2 DMA aperture and IOMMU domain are not proof of real
//! page pinning, signed drivers, isolation or device readiness. No MMIO
//! writes, device queue submission, or GPU acceleration occur here.
use crate::agent_07_pci_aperture::Aperture;
use crate::batch5_session::Identity;
use crate::hardware_virtio_gpu as gpu;
use crate::hardware_virtio_queue::{StagedQueue,PAGE};

pub const MAX_FRAME:u64=4*1024*1024;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Denied,Malformed,Unsupported,Range,Unisolated,Overlap}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind{GetDisplayInfo,Create2D,AttachBacking,SetScanout,Transfer,Flush}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Checked{pub kind:Kind,pub resource_id:u32}
fn u32_at(src:&[u8],at:usize)->Result<u32,Error>{
 let b=src.get(at..at+4).ok_or(Error::Malformed)?;
 Ok(u32::from_le_bytes([b[0],b[1],b[2],b[3]]))
}
fn u64_at(src:&[u8],at:usize)->Result<u64,Error>{
 let b=src.get(at..at+8).ok_or(Error::Malformed)?;
 Ok(u64::from_le_bytes([b[0],b[1],b[2],b[3],
                        b[4],b[5],b[6],b[7]]))
}
fn end(a:Aperture)->Result<u64,Error>{
 if a.base==0||a.length==0{return Err(Error::Range)}
 a.base.checked_add(a.length).ok_or(Error::Range)
}
fn overlaps(a:Aperture,b:Aperture)->Result<bool,Error>{
 Ok(a.base<end(b)?&&b.base<end(a)?)
}
fn validate_backing(queue:&StagedQueue,a:Aperture)
 ->Result<(),Error>{
 if a.base%PAGE!=0||a.length%PAGE!=0||a.length>MAX_FRAME{
  return Err(Error::Range)
 }
 if a.base<queue.dma.base||end(a)?>end(queue.dma)?{
  return Err(Error::Unisolated)
 }
 for ring in [queue.rings.descriptors,queue.rings.available,
              queue.rings.used]{
  if overlaps(a,ring)?{return Err(Error::Overlap)}
 }
 Ok(())
}
fn rectangle(frame:&[u8],start:usize)->Result<(u32,u32,u32,u32),Error>{
 let x=u32_at(frame,start)?;
 let y=u32_at(frame,start+4)?;
 let width=u32_at(frame,start+8)?;
 let height=u32_at(frame,start+12)?;
 if width==0||height==0||width>gpu::MAX_WIDTH||
    height>gpu::MAX_HEIGHT||
    x.checked_add(width).is_none_or(|n|n>gpu::MAX_WIDTH)||
    y.checked_add(height).is_none_or(|n|n>gpu::MAX_HEIGHT){
  return Err(Error::Range)
 }
 Ok((x,y,width,height))
}
fn pixels(width:u32,height:u32)->Result<u64,Error>{
 let bytes=u64::from(width).checked_mul(u64::from(height))
  .and_then(|n|n.checked_mul(4)).ok_or(Error::Range)?;
 if bytes==0||bytes>MAX_FRAME{return Err(Error::Range)}
 Ok(bytes)
}
fn require_size(frame:&[u8],size:usize)->Result<(),Error>{
 if frame.len()!=size{return Err(Error::Malformed)}
 Ok(())
}
/// The command is accepted as DATA ONLY. All six supported command encodings
/// are produced by HD3B.1's software 2D protocol. This is not an approval to
/// submit them to a device.
pub fn inspect(queue:&StagedQueue,actor:Identity,
 frame:&[u8],backing:Option<Aperture>)->Result<Checked,Error>{
 if actor.pid==0||actor.generation==0||actor!=queue.owner{
  return Err(Error::Denied)
 }
 if queue.iommu_domain==0||
    queue.features&crate::hardware_virtio_queue::REQUIRED_FEATURES!=
       crate::hardware_virtio_queue::REQUIRED_FEATURES{
  return Err(Error::Unisolated)
 }
 if frame.len()<gpu::CONTROL_HEADER||frame.len()>56{
  return Err(Error::Malformed)
 }
 // Protocol's fence flag, context id and reserved bytes must all be zero.
 if frame[4..gpu::CONTROL_HEADER].iter().any(|&x|x!=0){
  return Err(Error::Malformed)
 }
 let ty=u32_at(frame,0)?;
 let (kind,resource_id)=match ty{
  gpu::GET_DISPLAY_INFO=>{
   require_size(frame,24)?;
   (Kind::GetDisplayInfo,0)
  },
  gpu::RESOURCE_CREATE_2D=>{
   require_size(frame,40)?;
   let id=u32_at(frame,24)?;
   let format=u32_at(frame,28)?;
   let w=u32_at(frame,32)?;
   let h=u32_at(frame,36)?;
   if id==0||format!=gpu::FORMAT_B8G8R8A8_UNORM{
    return Err(Error::Unsupported)
   }
   if w>gpu::MAX_WIDTH||h>gpu::MAX_HEIGHT{
    return Err(Error::Range)
   }
   pixels(w,h)?;
   (Kind::Create2D,id)
  },
  gpu::RESOURCE_ATTACH_BACKING=>{
   require_size(frame,48)?;
   let id=u32_at(frame,24)?;
   let count=u32_at(frame,28)?;
   let addr=u64_at(frame,32)?;
   let len=u32_at(frame,40)?;
   let reserved=u32_at(frame,44)?;
   if id==0||count!=1||reserved!=0{return Err(Error::Malformed)}
   let ap=backing.ok_or(Error::Unisolated)?;
   if ap.base!=addr||ap.length!=u64::from(len){
    return Err(Error::Unisolated)
   }
   validate_backing(queue,ap)?;
   (Kind::AttachBacking,id)
  },
  gpu::SET_SCANOUT=>{
   require_size(frame,48)?;
   let (x,y,w,h)=rectangle(frame,24)?;
   let scanout=u32_at(frame,40)?;
   let id=u32_at(frame,44)?;
   if x!=0||y!=0||id==0||scanout>=16{
    return Err(Error::Unsupported)
   }
   pixels(w,h)?;
   (Kind::SetScanout,id)
  },
  gpu::TRANSFER_TO_HOST_2D=>{
   require_size(frame,56)?;
   let (x,y,w,h)=rectangle(frame,24)?;
   let offset=u64_at(frame,40)?;
   let id=u32_at(frame,48)?;
   let pad=u32_at(frame,52)?;
   if id==0||pad!=0||x!=0||y!=0||offset!=0{
    return Err(Error::Unsupported)
   }
   let a=backing.ok_or(Error::Unisolated)?;
   validate_backing(queue,a)?;
   if pixels(w,h)?>a.length{return Err(Error::Range)}
   (Kind::Transfer,id)
  },
  gpu::RESOURCE_FLUSH=>{
   require_size(frame,48)?;
   let (x,y,w,h)=rectangle(frame,24)?;
   let id=u32_at(frame,40)?;
   if x!=0||y!=0||id==0||u32_at(frame,44)?!=0{
    return Err(Error::Unsupported)
   }
   pixels(w,h)?;
   (Kind::Flush,id)
  },
  _=>return Err(Error::Unsupported),
 };
 Ok(Checked{kind,resource_id})
}
#[cfg(test)]
mod hd3b4_tests{
 use super::*;
 use crate::hardware_driver_stage::Bdf;
 use crate::hardware_virtio_queue::{Rings,REQUIRED_FEATURES};
 fn owner()->Identity{Identity{pid:17,generation:7}}
 fn ap(base:u64,length:u64)->Aperture{Aperture{base,length}}
 fn queue()->StagedQueue{
  StagedQueue{owner:owner(),bdf:Bdf{bus:1,slot:2,function:0},
   iommu_domain:8,features:REQUIRED_FEATURES,
   notify_offset:0x300c,dma:ap(0x2000_0000,0x400000),
   rings:Rings{descriptors:ap(0x2000_0000,PAGE),
    available:ap(0x2000_1000,PAGE),
    used:ap(0x2000_2000,PAGE),queue_size:8}}
 }
 fn buffer()->Aperture{ap(0x2001_0000,64*32*4)}
 fn put32(frame:&mut[u8],at:usize,v:u32){
  frame[at..at+4].copy_from_slice(&v.to_le_bytes());
 }
 fn put64(frame:&mut[u8],at:usize,v:u64){
  frame[at..at+8].copy_from_slice(&v.to_le_bytes());
 }
 fn frame(kind:u32,n:usize)->[u8;56]{
  let mut b=[0u8;56];put32(&mut b,0,kind);
  assert!(n<=56);b
 }
 #[test]fn hd3b4_accepts_exact_safe_2d_command_sequence(){
  let mut f=frame(gpu::GET_DISPLAY_INFO,24);
  assert_eq!(inspect(&queue(),owner(),&f[..24],None).unwrap().kind,
             Kind::GetDisplayInfo);
  f=frame(gpu::RESOURCE_CREATE_2D,40);
  put32(&mut f,24,9);put32(&mut f,28,gpu::FORMAT_B8G8R8A8_UNORM);
  put32(&mut f,32,64);put32(&mut f,36,32);
  assert_eq!(inspect(&queue(),owner(),&f[..40],None).unwrap().kind,
             Kind::Create2D);
  f=frame(gpu::RESOURCE_ATTACH_BACKING,48);
  put32(&mut f,24,9);put32(&mut f,28,1);
  put64(&mut f,32,buffer().base);put32(&mut f,40,buffer().length as u32);
  assert_eq!(inspect(&queue(),owner(),&f[..48],Some(buffer())).unwrap().kind,
             Kind::AttachBacking);
  f=frame(gpu::SET_SCANOUT,48);
  put32(&mut f,32,64);put32(&mut f,36,32);
  put32(&mut f,40,0);put32(&mut f,44,9);
  assert_eq!(inspect(&queue(),owner(),&f[..48],None).unwrap().kind,
             Kind::SetScanout);
  f=frame(gpu::TRANSFER_TO_HOST_2D,56);
  put32(&mut f,32,64);put32(&mut f,36,32);put32(&mut f,48,9);
  assert_eq!(inspect(&queue(),owner(),&f,Some(buffer())).unwrap().kind,
             Kind::Transfer);
  f=frame(gpu::RESOURCE_FLUSH,48);
  put32(&mut f,32,64);put32(&mut f,36,32);put32(&mut f,40,9);
  assert_eq!(inspect(&queue(),owner(),&f[..48],None).unwrap().kind,
             Kind::Flush);
 }
 #[test]fn hd3b4_rejects_stale_generation_and_unisolated_queue(){
  let f=frame(gpu::GET_DISPLAY_INFO,24);
  assert_eq!(inspect(&queue(),Identity{generation:8,..owner()},
             &f[..24],None),Err(Error::Denied));
  let mut q=queue();q.iommu_domain=0;
  assert_eq!(inspect(&q,owner(),&f[..24],None),Err(Error::Unisolated));
  let mut q=queue();q.features=0;
  assert_eq!(inspect(&q,owner(),&f[..24],None),Err(Error::Unisolated));
 }
 #[test]fn hd3b4_rejects_forged_headers_and_unknown_operations(){
  let mut f=frame(gpu::GET_DISPLAY_INFO,24);
  f[4]=1;
  assert_eq!(inspect(&queue(),owner(),&f[..24],None),
             Err(Error::Malformed));
  let f=frame(0x2ff,24);
  assert_eq!(inspect(&queue(),owner(),&f[..24],None),
             Err(Error::Unsupported));
  let f=frame(gpu::GET_DISPLAY_INFO,24);
  assert_eq!(inspect(&queue(),owner(),&f[..23],None),
             Err(Error::Malformed));
 }
 #[test]fn hd3b4_rejects_oversized_create_and_unapproved_format(){
  let mut f=frame(gpu::RESOURCE_CREATE_2D,40);
  put32(&mut f,24,1);put32(&mut f,28,1);
  put32(&mut f,32,1025);put32(&mut f,36,32);
  assert_eq!(inspect(&queue(),owner(),&f[..40],None),
             Err(Error::Range));
  put32(&mut f,32,64);put32(&mut f,28,7);
  assert_eq!(inspect(&queue(),owner(),&f[..40],None),
             Err(Error::Unsupported));
 }
 #[test]fn hd3b4_rejects_unmapped_backing_and_ring_alias(){
  let mut f=frame(gpu::RESOURCE_ATTACH_BACKING,48);
  put32(&mut f,24,1);put32(&mut f,28,1);
  put64(&mut f,32,0x2000_0000);put32(&mut f,40,4096);
  assert_eq!(inspect(&queue(),owner(),&f[..48],
             Some(ap(0x2000_0000,4096))),Err(Error::Overlap));
  put64(&mut f,32,0x2001_0000);
  assert_eq!(inspect(&queue(),owner(),&f[..48],
             Some(buffer())),Err(Error::Unisolated));
  put32(&mut f,40,buffer().length as u32);
  assert_eq!(inspect(&queue(),owner(),&f[..48],None),
             Err(Error::Unisolated));
 }
 #[test]fn hd3b4_transfer_requires_checked_framebuffer_bounds(){
  let mut f=frame(gpu::TRANSFER_TO_HOST_2D,56);
  put32(&mut f,32,128);put32(&mut f,36,128);put32(&mut f,48,2);
  assert_eq!(inspect(&queue(),owner(),&f,Some(buffer())),
             Err(Error::Range));
  put32(&mut f,32,64);put32(&mut f,36,32);
  put64(&mut f,40,4096);
  assert_eq!(inspect(&queue(),owner(),&f,Some(buffer())),
             Err(Error::Unsupported));
  put64(&mut f,40,0);put32(&mut f,52,1);
  assert_eq!(inspect(&queue(),owner(),&f,Some(buffer())),
             Err(Error::Unsupported));
 }
 #[test]fn hd3b4_rejects_malformed_scanout_and_flush(){
  let mut f=frame(gpu::SET_SCANOUT,48);
  put32(&mut f,32,64);put32(&mut f,36,32);
  put32(&mut f,40,16);put32(&mut f,44,1);
  assert_eq!(inspect(&queue(),owner(),&f[..48],None),
             Err(Error::Unsupported));
  f=frame(gpu::RESOURCE_FLUSH,48);
  put32(&mut f,32,64);put32(&mut f,36,32);
  put32(&mut f,40,1);put32(&mut f,44,1);
  assert_eq!(inspect(&queue(),owner(),&f[..48],None),
             Err(Error::Unsupported));
 }
}