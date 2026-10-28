//! HD3B.1: virtio-gpu modern PCI capability discovery and bounded 2D protocol.
//!
//! PCI config probing is read-only; no device, IOMMU, BAR, or DMA mapping
//! is activated. CPU fallback is ALWAYS available. The injected transport
//! trait is for conformance tests; production/QEMU virtqueue execution
//! requires separate authenticated driver, IOMMU and page ownership.
//!
//! The GPU control commands here are valid virtio-gpu 2D protocol bytes,
//! but a successful mock exchange is NOT proof of hardware acceleration.

use crate::hardware_inventory::{Accelerator,Device};
use crate::hardware_pcie::Topology;

pub const VENDOR:u16=0x1af4;
pub const GPU_DEVICE:u16=0x1050;
pub const PCI_VENDOR_CAP:u8=0x09;
pub const COMMON:u8=1;
pub const NOTIFY:u8=2;
pub const ISR:u8=3;
pub const DEVICE:u8=4;
pub const MAX_CAPS:usize=32;
pub const MAX_WIDTH:u32=1024;
pub const MAX_HEIGHT:u32=768;
pub const CONTROL_HEADER:usize=24;
pub const DISPLAY_INFO_RESPONSE:usize=CONTROL_HEADER+16*24;
pub const GET_DISPLAY_INFO:u32=0x0100;
pub const RESOURCE_CREATE_2D:u32=0x0101;
pub const SET_SCANOUT:u32=0x0103;
pub const RESOURCE_FLUSH:u32=0x0104;
pub const TRANSFER_TO_HOST_2D:u32=0x0105;
pub const RESOURCE_ATTACH_BACKING:u32=0x0106;
pub const RESP_OK_NODATA:u32=0x1100;
pub const RESP_OK_DISPLAY_INFO:u32=0x1101;
pub const FORMAT_B8G8R8A8_UNORM:u32=1;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{
 Absent,Identity,Capability,Duplicate,Cycle,Range,
 TooMany,Malformed,Unsupported,Transport,Response,Dimensions,
 Unisolated,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Bdf{pub bus:u8,pub slot:u8,pub function:u8}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Capability{
 pub bar:u8,pub offset:u32,pub length:u32,
 pub notify_multiplier:u32,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Capabilities{
 pub bdf:Bdf,pub common:Capability,pub notify:Capability,
 pub isr:Capability,pub device:Capability,
 pub visited:u8,
}
pub trait PciRead {
 /// Must perform read-only PCI configuration reads of dword-aligned
 /// offsets. Device MMIO and command register writes are forbidden.
 fn read32(&mut self,bdf:Bdf,offset:u16)->Result<u32,Error>;
}
pub fn locate(topology:&Topology)->Result<Bdf,Error>{
 let mut found=None;
 for item in topology.inventory.accelerators.iter().flatten(){
  if item.kind!=Accelerator::Gpu||item.header.vendor!=VENDOR||
   item.header.device!=GPU_DEVICE{continue;}
  if found.is_some(){return Err(Error::Duplicate);}
  found=Some(Bdf{bus:item.bus,slot:item.slot,function:item.function});
 }
 found.ok_or(Error::Absent)
}
fn byte<R:PciRead>(pci:&mut R,bdf:Bdf,offset:u16)->Result<u8,Error>{
 if offset>=256{return Err(Error::Range);}
 let word=pci.read32(bdf,offset&!3)?;
 Ok((word>>((offset&3)*8)) as u8)
}
fn word<R:PciRead>(pci:&mut R,bdf:Bdf,offset:u16)->Result<u32,Error>{
 if offset>252||offset&3!=0{return Err(Error::Range);}
 pci.read32(bdf,offset)
}
/// Parse actual virtio PCI vendor-specific capabilities. The generic
/// GPU PCI class is NOT sufficient; every required capability must be
/// present, unambiguous, in-bounds and point to a memory BAR.
pub fn inspect<R:PciRead>(pci:&mut R,bdf:Bdf)->Result<Capabilities,Error>{
 if bdf.slot>=32||bdf.function>=8{return Err(Error::Range);}
 let id=word(pci,bdf,0)?;
 if id as u16!=VENDOR||(id>>16) as u16!=GPU_DEVICE{
  return Err(Error::Identity);
 }
 let class=word(pci,bdf,8)?;
 if (class>>24) as u8!=3{return Err(Error::Identity);}
 let command=word(pci,bdf,4)?;
 // PCI status bit4 is located in the upper half of dword 4.
 if command&(1<<20)==0{return Err(Error::Absent);}
 let mut pointer=byte(pci,bdf,0x34)?;
 let mut seen=[false;256];
 let mut visited=0u8;
 let mut caps:[Option<Capability>;4]=[None;4];
 while pointer!=0{
  if pointer<0x40||pointer>0xfc||pointer&3!=0{
   return Err(Error::Malformed);
  }
  if seen[pointer as usize]{return Err(Error::Cycle);}
  seen[pointer as usize]=true;
  visited=visited.checked_add(1).ok_or(Error::TooMany)?;
  if visited as usize>MAX_CAPS{return Err(Error::TooMany);}
  let header=word(pci,bdf,pointer as u16)?;
  let id=header as u8;
  let next=(header>>8) as u8;
  let len=(header>>16) as u8;
  let kind=(header>>24) as u8;
  if id==PCI_VENDOR_CAP{
   if !(1..=4).contains(&kind){pointer=next;continue;}
   if len<16||(kind==NOTIFY&&len<20)||
    pointer as u16+len as u16>256{return Err(Error::Malformed);}
   let bar=byte(pci,bdf,pointer as u16+4)?;
   if bar>5{return Err(Error::Range);}
   let bar_cfg=word(pci,bdf,0x10+bar as u16*4)?;
   if bar_cfg&1!=0{return Err(Error::Unsupported);}
   // 0x6 is a 64-bit memory BAR; the high half is not itself a BAR.
   let bar_type=(bar_cfg>>1)&3;
   if bar_type==3||(bar_type==2&&bar==5){
    return Err(Error::Unsupported);
   }
   let offset=word(pci,bdf,pointer as u16+8)?;
   let length=word(pci,bdf,pointer as u16+12)?;
   if length==0||length>0x200000||
    offset.checked_add(length).is_none()||
    (kind==COMMON&&length<56)||
    (kind==ISR&&length<1)||
    (kind==NOTIFY&&length<2){
    return Err(Error::Range);
   }
   let multiplier=if kind==NOTIFY{
    word(pci,bdf,pointer as u16+16)?
   }else{0};
   if kind==NOTIFY&&multiplier==0{return Err(Error::Malformed);}
   let slot=(kind-1) as usize;
   if caps[slot].is_some(){return Err(Error::Duplicate);}
   caps[slot]=Some(Capability{
    bar,offset,length,notify_multiplier:multiplier,
   });
  }
  pointer=next;
 }
 let [Some(common),Some(notify),Some(isr),Some(device)]=caps
  else{return Err(Error::Absent)};
 // Numeric BAR offsets for distinct MMIO structures may not overlap
 // on the same BAR. Real BAR SIZE and physical accessibility still need
 // to be validated through a trusted platform memory-map/IOMMU layer.
 let all=[common,notify,isr,device];
 for (i,a) in all.iter().enumerate(){
  for b in &all[..i]{
   if a.bar==b.bar&&a.offset<b.offset+b.length&&
    b.offset<a.offset+a.length{return Err(Error::Capability);}
  }
 }
 Ok(Capabilities{bdf,common,notify,isr,device,visited})
}
fn put32(dst:&mut[u8],off:usize,n:u32){dst[off..off+4].copy_from_slice(&n.to_le_bytes());}
fn put64(dst:&mut[u8],off:usize,n:u64){dst[off..off+8].copy_from_slice(&n.to_le_bytes());}
fn get32(src:&[u8],off:usize)->Result<u32,Error>{
 let data=src.get(off..off+4).ok_or(Error::Response)?;
 Ok(u32::from_le_bytes(data.try_into().map_err(|_|Error::Response)?))
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Rect{pub x:u32,pub y:u32,pub width:u32,pub height:u32}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Scanout{pub width:u32,pub height:u32,pub scanout:u32}
pub trait ControlTransport{
 /// The implementation MUST own pinned, isolated physical memory,
 /// validate response descriptor length and fence ordering, and submit
 /// the command on actual virtqueue 0. No native backend yet exists.
 fn exchange(&mut self,command:&[u8],response:&mut [u8])
  ->Result<usize,Error>;
}
pub struct Protocol<T:ControlTransport>{transport:T,ready:bool}
impl<T:ControlTransport> Protocol<T>{
 pub fn new(transport:T)->Self{Self{transport,ready:false}}
 fn command(&mut self,type_:u32,extra:&[u8],expected:u32,
  response:&mut[u8])->Result<usize,Error>{
  if extra.len()>64-CONTROL_HEADER{return Err(Error::Range);}
  let mut request=[0u8;64];
  put32(&mut request,0,type_);
  request[CONTROL_HEADER..CONTROL_HEADER+extra.len()].copy_from_slice(extra);
  response.fill(0);
  let n=self.transport.exchange(&request[..CONTROL_HEADER+extra.len()],response)?;
  if n<CONTROL_HEADER||n>response.len()||
    get32(response,0)?!=expected||
    get32(response,4)?!=0||
    response[8..24].iter().any(|&v|v!=0){
   return Err(Error::Response);
  }
  Ok(n)
 }
 pub fn discover_display(&mut self)->Result<Scanout,Error>{
  let mut response=[0u8;DISPLAY_INFO_RESPONSE];
  let n=self.command(GET_DISPLAY_INFO,&[],RESP_OK_DISPLAY_INFO,&mut response)?;
  if n<DISPLAY_INFO_RESPONSE{return Err(Error::Response);}
  for index in 0..16{
   let off=CONTROL_HEADER+index*24;
   if get32(&response,off+16)?!=1{continue;}
   let x=get32(&response,off)?;
   let y=get32(&response,off+4)?;
   let width=get32(&response,off+8)?;
   let height=get32(&response,off+12)?;
   if x!=0||y!=0||width==0||height==0||
    width>MAX_WIDTH||height>MAX_HEIGHT{return Err(Error::Dimensions);}
   self.ready=true;
   return Ok(Scanout{width,height,scanout:index as u32});
  }
  Err(Error::Absent)
 }
 /// End-to-end 2D command sequence. A real transport must enforce IOMMU
 /// DMA ownership first. This abstraction never touches the PCI BAR.
 pub fn paint(&mut self,display:Scanout,resource_id:u32,
  physical:u64,buffer_bytes:u32)->Result<(),Error>{
  if !self.ready||resource_id==0||physical==0||
   physical&4095!=0||display.width==0||display.height==0||
   display.width>MAX_WIDTH||display.height>MAX_HEIGHT{
   return Err(Error::Unisolated);
  }
  let required=display.width.checked_mul(display.height)
   .and_then(|x|x.checked_mul(4)).ok_or(Error::Dimensions)?;
  if buffer_bytes<required||buffer_bytes>4*1024*1024{
   return Err(Error::Dimensions);
  }
  let mut extra=[0u8;40];
  let mut response=[0u8;CONTROL_HEADER];
  // RESOURCE_CREATE_2D: [id, format, width, height].
  put32(&mut extra,0,resource_id);
  put32(&mut extra,4,FORMAT_B8G8R8A8_UNORM);
  put32(&mut extra,8,display.width);
  put32(&mut extra,12,display.height);
  self.command(RESOURCE_CREATE_2D,&extra[..16],RESP_OK_NODATA,&mut response)?;
  // RESOURCE_ATTACH_BACKING: [id, one entry, physical addr, byte len, pad].
  extra.fill(0);
  put32(&mut extra,0,resource_id);put32(&mut extra,4,1);
  put64(&mut extra,8,physical);
  put32(&mut extra,16,buffer_bytes);
  self.command(RESOURCE_ATTACH_BACKING,&extra[..24],
   RESP_OK_NODATA,&mut response)?;
  // SET_SCANOUT: [x,y,w,h,scanout_id,resource_id].
  extra.fill(0);
  put32(&mut extra,8,display.width);put32(&mut extra,12,display.height);
  put32(&mut extra,16,display.scanout);put32(&mut extra,20,resource_id);
  self.command(SET_SCANOUT,&extra[..24],RESP_OK_NODATA,&mut response)?;
  // TRANSFER_TO_HOST_2D: [rect 16 bytes, offset u64, resource id, pad].
  extra.fill(0);
  put32(&mut extra,8,display.width);put32(&mut extra,12,display.height);
  put32(&mut extra,24,resource_id);
  self.command(TRANSFER_TO_HOST_2D,&extra[..32],
   RESP_OK_NODATA,&mut response)?;
  // RESOURCE_FLUSH: rect followed by resource id + padding.
  extra.fill(0);
  put32(&mut extra,8,display.width);put32(&mut extra,12,display.height);
  put32(&mut extra,16,resource_id);
  self.command(RESOURCE_FLUSH,&extra[..24],RESP_OK_NODATA,&mut response)?;
  Ok(())
 }
 /// Preflight the COMPLETE compositor damage batch before sending any
 /// command to the authenticated virtio-gpu transport. If the transport
 /// fails after submission, callers must reset/recover the device separately.
 pub fn refresh_batch(&mut self,display:Scanout,resource:u32,
   dirty:&[Rect])->Result<usize,Error>{
  if dirty.is_empty()||dirty.len()>8{return Err(Error::Dimensions);}
  if !self.ready||resource==0||display.width==0||display.height==0||
    display.width>MAX_WIDTH||display.height>MAX_HEIGHT{
   return Err(Error::Unisolated);
  }
  for r in dirty{
   if r.width==0||r.height==0||
      r.x.checked_add(r.width).is_none_or(|v|v>display.width)||
      r.y.checked_add(r.height).is_none_or(|v|v>display.height){
    return Err(Error::Dimensions);
   }
  }
  for &r in dirty{
   self.refresh_subrect(display,resource,r)?;
  }
  Ok(dirty.len())
 }
 /// Update an already allocated and attached scanout resource using
 /// strict bounds and actual virtio-gpu transfer/flush command encoding.
 /// Native transport, page-pinning and IOMMU ownership remain separate.
 pub fn refresh_subrect(&mut self,display:Scanout,resource:u32,
  rect:Rect)->Result<(),Error>{
  if !self.ready||resource==0||display.width==0||display.height==0||
     display.width>MAX_WIDTH||display.height>MAX_HEIGHT{
   return Err(Error::Unisolated);
  }
  if rect.width==0||rect.height==0||
     rect.x.checked_add(rect.width).is_none_or(|v|v>display.width)||
     rect.y.checked_add(rect.height).is_none_or(|v|v>display.height){
   return Err(Error::Dimensions);
  }
  let offset=u64::from(rect.y).checked_mul(u64::from(display.width))
    .and_then(|x|x.checked_add(u64::from(rect.x)))
    .and_then(|x|x.checked_mul(4)).ok_or(Error::Dimensions)?;
  let mut extra=[0u8;32];
  let mut response=[0u8;CONTROL_HEADER];
  put32(&mut extra,0,rect.x);put32(&mut extra,4,rect.y);
  put32(&mut extra,8,rect.width);put32(&mut extra,12,rect.height);
  put64(&mut extra,16,offset);put32(&mut extra,24,resource);
  self.command(TRANSFER_TO_HOST_2D,&extra[..32],RESP_OK_NODATA,&mut response)?;
  extra.fill(0);
  put32(&mut extra,0,rect.x);put32(&mut extra,4,rect.y);
  put32(&mut extra,8,rect.width);put32(&mut extra,12,rect.height);
  put32(&mut extra,16,resource);
  self.command(RESOURCE_FLUSH,&extra[..24],RESP_OK_NODATA,&mut response)?;
  Ok(())
 }
}
/// This report deliberately states *capability discovery only*.
/// GPU queues, shaders, DMA and all drivers remain disabled at normal boot.
pub fn status_line(available:bool)->&'static [u8]{
 if available{
  b"IndianOS HD3B virtio-gpu modern PCI capabilities verified; queue=disabled; GPU acceleration=not active; CPU fallback=enabled\n\0"
 }else{
  b"IndianOS HD3B virtio-gpu unavailable/unsupported; queue=disabled; GPU acceleration=not active; CPU fallback=enabled\n\0"
 }
}
#[cfg(all(target_arch="x86_64",target_os="none"))]
pub mod native{
 use super::*;
 use crate::hardware_inventory::native::BootPci;
 pub struct ReadOnlyConfig;
 impl PciRead for ReadOnlyConfig{
  fn read32(&mut self,bdf:Bdf,offset:u16)->Result<u32,Error>{
   if offset>252||offset&3!=0{return Err(Error::Range);}
   Ok(unsafe{BootPci::read_config(bdf.bus,bdf.slot,bdf.function,offset as u8)})
  }
 }
 pub fn probe(topology:&Topology)->Result<Capabilities,Error>{
  let bdf=locate(topology)?;
  inspect(&mut ReadOnlyConfig,bdf)
 }
}
#[cfg(test)]
mod hd3b_tests{
 use super::*;
 use crate::hardware_inventory::{
  Cpu,Inventory,Accelerator,Device,PciHeader,MAX_ACCELERATORS,PROFILE_ABI,
 };
 use crate::hardware_pcie::TOPOLOGY_ABI;
 struct Pci{config:[u8;256],reads:u32}
 impl Pci{
  fn valid()->Self{
   let mut config=[0u8;256];
   config[..2].copy_from_slice(&VENDOR.to_le_bytes());
   config[2..4].copy_from_slice(&GPU_DEVICE.to_le_bytes());
   config[6]=0x10;config[11]=3;config[0x34]=0x40;
   let specs=[(0x40u8,0x54u8,COMMON,0u32,56u32,0u32),
    (0x54,0x6c,NOTIFY,0x3000,0x1000,4),
    (0x6c,0x7c,ISR,0x1000,0x1000,0),
    (0x7c,0,DEVICE,0x2000,0x1000,0)];
   for (ptr,next,kind,offset,length,multiplier) in specs{
    let idx=ptr as usize;
    config[idx]=PCI_VENDOR_CAP;config[idx+1]=next;
    config[idx+2]=if kind==NOTIFY{20}else{16};
    config[idx+3]=kind;config[idx+4]=4; // BAR4 modern MMIO.
    config[idx+8..idx+12].copy_from_slice(&offset.to_le_bytes());
    config[idx+12..idx+16].copy_from_slice(&length.to_le_bytes());
    if kind==NOTIFY{
     config[idx+16..idx+20].copy_from_slice(&multiplier.to_le_bytes());
    }
   }
   config[0x20]=0x04; // BAR4 64-bit MMIO type (low bits 0x4).
   Self{config,reads:0}
  }
 }
 impl PciRead for Pci{
  fn read32(&mut self,_bdf:Bdf,offset:u16)->Result<u32,Error>{
   self.reads+=1;
   let p=offset as usize;
   if p>252||p%4!=0{return Err(Error::Range);}
   Ok(u32::from_le_bytes(self.config[p..p+4].try_into().unwrap()))
  }
 }
 fn bdf()->Bdf{Bdf{bus:1,slot:2,function:0}}
 fn cpu()->Cpu{Cpu{vendor:*b"GenuineIntel",family:6,model:58,
  stepping:1,features:7,max_basic:7,max_extended:0x8000_0001}}
 fn topo()->Topology{
  let mut inventory=Inventory{abi:PROFILE_ABI,cpu:cpu(),
   accelerators:[None;MAX_ACCELERATORS],count:1,gpu_count:1,
   npu_count:0,physical_qpu_available:false,cpu_software_fallback:true};
  inventory.accelerators[0]=Some(Device{bus:1,slot:2,function:0,
   kind:Accelerator::Gpu,header:PciHeader{vendor:VENDOR,
    device:GPU_DEVICE,class:3,subclass:0,prog_if:0,header_type:0}});
  Topology{abi:TOPOLOGY_ABI,inventory,
   scanned_buses:2,discovered_bridges:1,pcie_ecam_active:false}
 }
 #[test]fn hd3b_pci_detects_modern_capability_chain_on_secondary_bus(){
  assert_eq!(locate(&topo()),Ok(bdf()));
  let mut pci=Pci::valid();
  let c=inspect(&mut pci,bdf()).unwrap();
  assert_eq!(c.visited,4);
  assert_eq!(c.common.length,56);
  assert_eq!(c.notify.notify_multiplier,4);
  assert_eq!(c.device.bar,4);
  assert!(pci.reads>12);
  assert!(core::str::from_utf8(status_line(true)).unwrap()
    .contains("GPU acceleration=not active"));
 }
 #[test]fn hd3b_pci_capability_cycle_duplicate_and_bad_next_rejected(){
  let mut pci=Pci::valid();pci.config[0x7d]=0x40;
  assert_eq!(inspect(&mut pci,bdf()),Err(Error::Cycle));
  let mut pci=Pci::valid();pci.config[0x7f]=COMMON;
  assert_eq!(inspect(&mut pci,bdf()),Err(Error::Duplicate));
  let mut pci=Pci::valid();pci.config[0x41]=0x43;
  assert_eq!(inspect(&mut pci,bdf()),Err(Error::Malformed));
 }
 #[test]fn hd3b_pci_rejects_wrong_gpu_and_invalid_io_bar(){
  let mut pci=Pci::valid();pci.config[2]=0x51;
  assert_eq!(inspect(&mut pci,bdf()),Err(Error::Identity));
  let mut pci=Pci::valid();pci.config[0x20]=1;
  assert_eq!(inspect(&mut pci,bdf()),Err(Error::Unsupported));
  let mut pci=Pci::valid();pci.config[0x4f]=0xff;
  assert_eq!(inspect(&mut pci,bdf()),Err(Error::Range));
 }
 #[test]fn hd3b_pci_rejects_overlaps_notify_multiplier_and_cap_absence(){
  let mut pci=Pci::valid();
  pci.config[0x78..0x7c].copy_from_slice(&0x2000u32.to_le_bytes());
  assert_eq!(inspect(&mut pci,bdf()),Err(Error::Capability));
  let mut pci=Pci::valid();pci.config[0x64..0x68].fill(0);
  assert_eq!(inspect(&mut pci,bdf()),Err(Error::Malformed));
  let mut pci=Pci::valid();pci.config[0x7c]=0x01;
  assert_eq!(inspect(&mut pci,bdf()),Err(Error::Absent));
 }
 struct MockTransport{types:[u32;8],index:usize,mode:u8}
 impl ControlTransport for MockTransport{
  fn exchange(&mut self,cmd:&[u8],out:&mut[u8])->Result<usize,Error>{
   let kind=get32(cmd,0)?;self.types[self.index]=kind;self.index+=1;
   let response=if kind==GET_DISPLAY_INFO{
    RESP_OK_DISPLAY_INFO
   }else{RESP_OK_NODATA};
   put32(out,0,if self.mode==1{0x1200}else{response});
   if kind==GET_DISPLAY_INFO{
    if self.mode==2{return Ok(24);}
    // scanout 0: x/y zero, enabled 1, 64x32 rectangle.
    put32(out,24+8,64);put32(out,24+12,32);put32(out,24+16,1);
    return Ok(DISPLAY_INFO_RESPONSE);
   }
   Ok(CONTROL_HEADER)
  }
 }
 fn transport(mode:u8)->MockTransport{
  MockTransport{types:[0;8],index:0,mode}
 }
 #[test]fn hd3b_protocol_emits_valid_full_2d_command_sequence(){
  let mut gpu=Protocol::new(transport(0));
  let display=gpu.discover_display().unwrap();
  assert_eq!(display,Scanout{width:64,height:32,scanout:0});
  gpu.paint(display,1,0x2000_0000,8192).unwrap();
  assert_eq!(&gpu.transport.types[..6],&[
   GET_DISPLAY_INFO,RESOURCE_CREATE_2D,RESOURCE_ATTACH_BACKING,
   SET_SCANOUT,TRANSFER_TO_HOST_2D,RESOURCE_FLUSH,
  ]);
 }
 #[test]fn hd3b_protocol_refuses_bad_response_short_display_and_unaligned_dma(){
  let mut gpu=Protocol::new(transport(1));
  assert_eq!(gpu.discover_display(),Err(Error::Response));
  let mut gpu=Protocol::new(transport(2));
  assert_eq!(gpu.discover_display(),Err(Error::Response));
  let mut gpu=Protocol::new(transport(0));
  let display=gpu.discover_display().unwrap();
  assert_eq!(gpu.paint(display,0,0x2000_0000,8192),
   Err(Error::Unisolated));
  assert_eq!(gpu.paint(display,1,0x2000_0001,8192),
   Err(Error::Unisolated));
  assert_eq!(gpu.paint(display,1,0x2000_0000,8191),
   Err(Error::Dimensions));
  assert_eq!(gpu.transport.index,1); // no command submitted on failure
 }

 #[test]fn virtio_gpu_partial_damage_emits_transfer_then_flush(){
  let mut gpu=Protocol::new(transport(0));
  let display=gpu.discover_display().unwrap();
  assert_eq!(gpu.refresh_subrect(display,1,Rect{
   x:2,y:3,width:10,height:5}),Ok(()));
  assert_eq!(&gpu.transport.types[..3],
   &[GET_DISPLAY_INFO,TRANSFER_TO_HOST_2D,RESOURCE_FLUSH]);
 }
 #[test]fn virtio_gpu_offscreen_damage_is_rejected_before_queue_submit(){
  let mut gpu=Protocol::new(transport(0));
  let display=gpu.discover_display().unwrap();
  assert_eq!(gpu.refresh_subrect(display,1,Rect{
   x:60,y:0,width:10,height:2}),Err(Error::Dimensions));
  assert_eq!(gpu.refresh_subrect(display,1,Rect{
   x:0,y:0,width:0,height:2}),Err(Error::Dimensions));
  assert_eq!(gpu.refresh_subrect(display,0,Rect{
   x:0,y:0,width:10,height:2}),Err(Error::Unisolated));
  assert_eq!(gpu.transport.index,1);
 }


 #[test]fn valid_gpu_damage_batch_sends_transfer_flush_per_rectangle(){
  let mut gpu=Protocol::new(transport(0));
  let scanout=gpu.discover_display().unwrap();
  let dirty=[Rect{x:1,y:2,width:4,height:3},
             Rect{x:10,y:6,width:5,height:6}];
  assert_eq!(gpu.refresh_batch(scanout,1,&dirty),Ok(2));
  assert_eq!(&gpu.transport.types[..5],&[
    GET_DISPLAY_INFO,TRANSFER_TO_HOST_2D,RESOURCE_FLUSH,
    TRANSFER_TO_HOST_2D,RESOURCE_FLUSH]);
 }
 #[test]fn invalid_gpu_batch_makes_no_partial_commands(){
  let mut gpu=Protocol::new(transport(0));
  let scanout=gpu.discover_display().unwrap();
  let dirty=[Rect{x:1,y:2,width:4,height:3},
             Rect{x:60,y:6,width:100,height:6}];
  assert_eq!(gpu.refresh_batch(scanout,1,&dirty),Err(Error::Dimensions));
  assert_eq!(gpu.transport.index,1);
  assert_eq!(gpu.refresh_batch(scanout,1,&[]),Err(Error::Dimensions));
 }

}

 
/// R009 compositor-to-virtio-GPU dirty region reduction. Bounds are checked
/// before the first transport command. No physical memory is mapped here.
pub fn coalesce_damage(display:Scanout,dirty:&[Rect],
 out:&mut[Rect])->Result<usize,Error>{
 if display.width==0||display.height==0||
   display.width>MAX_WIDTH||display.height>MAX_HEIGHT||
   dirty.is_empty()||dirty.len()>8||out.is_empty(){return Err(Error::Dimensions);}
 let mut combined=[Rect{x:0,y:0,width:0,height:0};8];
 let mut count=0usize;
 for &r in dirty{
  let right=r.x.checked_add(r.width).ok_or(Error::Dimensions)?;
  let bottom=r.y.checked_add(r.height).ok_or(Error::Dimensions)?;
  if r.width==0||r.height==0||right>display.width||
    bottom>display.height{return Err(Error::Dimensions);}
  let mut item=r;
  let mut pos=0;
  while pos<count{
   let other=combined[pos];
   let a_right=other.x+other.width;let a_bottom=other.y+other.height;
   let b_right=item.x+item.width;let b_bottom=item.y+item.height;
   if item.x<=a_right&&other.x<=b_right&&
      item.y<=a_bottom&&other.y<=b_bottom{
    let x=item.x.min(other.x);let y=item.y.min(other.y);
    item=Rect{x,y,width:b_right.max(a_right)-x,
      height:b_bottom.max(a_bottom)-y};
    count-=1;
    combined[pos]=combined[count];
    pos=0; // Revisit earlier rectangles after a union expands.
   }else{pos+=1;}
  }
  combined[count]=item;count+=1;
 }
 if count>out.len(){return Err(Error::Dimensions);}
 out[..count].copy_from_slice(&combined[..count]);Ok(count)
}
impl<T:ControlTransport> Protocol<T>{
 /// Coalesce and refresh using the already authenticated control transport.
 /// Damage input is never trusted to provide a physical DMA address.
 pub fn refresh_coalesced(&mut self,display:Scanout,resource:u32,
   dirty:&[Rect])->Result<usize,Error>{
  let mut combined=[Rect{x:0,y:0,width:0,height:0};8];
  let n=coalesce_damage(display,dirty,&mut combined)?;
  self.refresh_batch(display,resource,&combined[..n])
 }
}
#[cfg(test)]
mod r009_gpu_damage_tests{
 use super::*;
 fn display()->Scanout{Scanout{width:128,height:96,scanout:0}}
 #[test]fn overlapping_regions_are_coalesced(){
  let mut output=[Rect{x:0,y:0,width:0,height:0};8];
  let dirty=[Rect{x:10,y:10,width:20,height:20},
    Rect{x:20,y:20,width:20,height:20},
    Rect{x:80,y:80,width:10,height:10}];
  assert_eq!(coalesce_damage(display(),&dirty,&mut output),Ok(2));
  assert!(output[..2].contains(&Rect{x:10,y:10,width:30,height:30}));
  assert!(output[..2].contains(&Rect{x:80,y:80,width:10,height:10}));
 }
 #[test]fn bad_damage_has_no_partial_output(){
  let sentinel=Rect{x:7,y:7,width:7,height:7};
  let mut output=[sentinel;2];
  let dirty=[Rect{x:1,y:1,width:2,height:2},
    Rect{x:127,y:0,width:3,height:1}];
  assert_eq!(coalesce_damage(display(),&dirty,&mut output),Err(Error::Dimensions));
  assert_eq!(output,[sentinel;2]);
 }
 #[test]fn zero_size_and_short_capacity_are_rejected(){
  let mut out=[Rect{x:0,y:0,width:0,height:0};1];
  assert_eq!(coalesce_damage(display(),&[],&mut out),Err(Error::Dimensions));
  assert_eq!(coalesce_damage(display(),&[
   Rect{x:1,y:1,width:1,height:1},Rect{x:90,y:70,width:1,height:1}
  ],&mut out),Err(Error::Dimensions));
 }
}

 
/// R010 display backing *metadata* preflight. A native GPU worker must also
/// prove DMA pinning, IOMMU ownership and device isolation before calling
/// paint; arithmetic validity alone is not a DMA authorization.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct BackingPlan{
 pub physical:u64,pub allocated_bytes:u32,pub stride_bytes:u32,
 pub visible_bytes:u32,
}
pub fn validate_backing_plan(display:Scanout,physical:u64,allocated:u32,
  stride:u32)->Result<BackingPlan,Error>{
 if display.width==0||display.height==0||
   display.width>MAX_WIDTH||display.height>MAX_HEIGHT||
   display.scanout>=16||physical==0||physical&4095!=0||
   allocated==0||allocated>4*1024*1024{return Err(Error::Dimensions);}
 let row=display.width.checked_mul(4).ok_or(Error::Dimensions)?;
 if stride<row||stride&3!=0{return Err(Error::Dimensions);}
 let visible=u64::from(display.height-1).checked_mul(u64::from(stride))
   .and_then(|x|x.checked_add(u64::from(row))).ok_or(Error::Dimensions)?;
 if visible>u64::from(allocated)||
    physical.checked_add(u64::from(allocated)).is_none(){
   return Err(Error::Dimensions);
 }
 Ok(BackingPlan{physical,allocated_bytes:allocated,
    stride_bytes:stride,visible_bytes:visible as u32})
}
impl<T:ControlTransport> Protocol<T>{
 /// The existing virtio-gpu 2D protocol accepts only tightly packed scanout
 /// backing. An independent DMA ownership check is still mandatory.
 pub fn paint_tightly_packed(&mut self,display:Scanout,resource:u32,
   backing:BackingPlan)->Result<(),Error>{
  let stride=display.width.checked_mul(4).ok_or(Error::Dimensions)?;
  let checked=validate_backing_plan(display,backing.physical,
    backing.allocated_bytes,backing.stride_bytes)?;
  if checked.stride_bytes!=stride{return Err(Error::Unsupported);}
  self.paint(display,resource,checked.physical,checked.allocated_bytes)
 }
}
#[cfg(test)]
mod r010_gpu_backing_tests{
 use super::*;
 fn screen()->Scanout{Scanout{width:800,height:600,scanout:0}}
 #[test]fn strict_scanout_memory_layout_is_bounded(){
  let bytes=800*600*4;
  assert_eq!(validate_backing_plan(screen(),0x200000,bytes,3200),
    Ok(BackingPlan{physical:0x200000,allocated_bytes:bytes,
      stride_bytes:3200,visible_bytes:bytes}));
 }
 #[test]fn rejects_unaligned_physical_and_short_surface(){
  assert_eq!(validate_backing_plan(screen(),0x200001,800*600*4,3200),
    Err(Error::Dimensions));
  assert_eq!(validate_backing_plan(screen(),0x200000,800,3200),
    Err(Error::Dimensions));
  assert_eq!(validate_backing_plan(screen(),0x200000,800*600*4,3199),
    Err(Error::Dimensions));
 }
 #[test]fn validates_padded_stride_without_claiming_native_dma(){
  let plan=validate_backing_plan(screen(),0x200000,2048*1024,3264).unwrap();
  assert_eq!(plan.stride_bytes,3264);
  assert!(plan.visible_bytes>800*600*4);
  assert_eq!(validate_backing_plan(screen(),u64::MAX-4095,0x2000,3200),
    Err(Error::Dimensions));
 }
}

 
/// R011 scanout coordination in trusted compositor context. Mapping GPU BAR
/// and granting IOMMU-backed DMA are separate privileged driver operations.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct DisplayLease{pub owner:u32,pub resource:u32,pub scanout:Scanout,
 pub backing:BackingPlan,pub pending_frames:u32}
pub struct MultiDisplay<const N:usize>{leases:[Option<DisplayLease>;N]}
impl<const N:usize> MultiDisplay<N>{
 pub const fn new()->Self{Self{leases:[None;N]}}
 pub fn attach(&mut self,owner:u32,resource:u32,
  scanout:Scanout,backing:BackingPlan)->Result<(),Error>{
  if owner==0||resource==0{return Err(Error::Dimensions);}
  let validated=validate_backing_plan(scanout,backing.physical,
   backing.allocated_bytes,backing.stride_bytes)?;
  if validated!=backing{return Err(Error::Dimensions);}
  if self.leases.iter().flatten().any(|lease|
      lease.scanout.scanout==scanout.scanout||lease.resource==resource){
   return Err(Error::Unsupported);
  }
  let slot=self.leases.iter_mut().find(|entry|entry.is_none())
    .ok_or(Error::Dimensions)?;
  *slot=Some(DisplayLease{owner,resource,scanout,backing,
    pending_frames:0});Ok(())
 }
 pub fn submit_damage(&mut self,owner:u32,resource:u32,
  regions:&[Rect])->Result<usize,Error>{
  let lease=self.leases.iter_mut().flatten().find(|s|
    s.resource==resource&&s.owner==owner).ok_or(Error::Unsupported)?;
  if regions.is_empty()||regions.len()>32{return Err(Error::Dimensions);}
  // Validate every rectangle before mutating submitted-frame accounting.
  for rect in regions{
   if rect.width==0||rect.height==0||
      rect.x.checked_add(rect.width).is_none_or(|v|v>lease.scanout.width)||
      rect.y.checked_add(rect.height).is_none_or(|v|v>lease.scanout.height){
    return Err(Error::Dimensions);
   }
  }
  let next=lease.pending_frames.checked_add(1).ok_or(Error::Dimensions)?;
  lease.pending_frames=next;Ok(regions.len())
 }
 pub fn present(&mut self,owner:u32,resource:u32)->Result<u32,Error>{
  let lease=self.leases.iter_mut().flatten().find(|s|
    s.resource==resource&&s.owner==owner).ok_or(Error::Unsupported)?;
  if lease.pending_frames==0{return Err(Error::Dimensions);}
  lease.pending_frames-=1;Ok(lease.pending_frames)
 }
 pub fn retire_owner(&mut self,owner:u32)->usize{
  let mut n=0;
  for slot in &mut self.leases{
   if slot.is_some_and(|s|s.owner==owner){*slot=None;n+=1;}
  }
  n
 }
 pub fn active(&self)->usize{self.leases.iter().flatten().count()}
}
#[cfg(test)]
mod r011_multiscanout_tests{
 use super::*;
 fn lease(id:u32)->(Scanout,BackingPlan){
  let scanout=Scanout{width:100,height:50,scanout:id};
  let backing=validate_backing_plan(scanout,0x1000+u64::from(id)*0x1000,
     100*50*4,400).unwrap();
  (scanout,backing)
 }
 #[test]fn two_distinct_displays_share_no_resource_or_scanout(){
  let mut display=MultiDisplay::<2>::new();
  let (s,b)=lease(0);display.attach(1,10,s,b).unwrap();
  let (s,b)=lease(1);display.attach(2,20,s,b).unwrap();
  assert_eq!(display.active(),2);
  assert_eq!(display.attach(3,20,s,b),Err(Error::Unsupported));
 }
 #[test]fn invalid_damage_and_foreign_owner_do_not_advance_frame(){
  let mut display=MultiDisplay::<1>::new();let(s,b)=lease(0);
  display.attach(1,5,s,b).unwrap();
  assert_eq!(display.submit_damage(2,5,&[Rect{x:0,y:0,width:2,height:2}]),
    Err(Error::Unsupported));
  assert_eq!(display.submit_damage(1,5,&[Rect{x:99,y:49,width:2,height:2}]),
    Err(Error::Dimensions));
  assert_eq!(display.present(1,5),Err(Error::Dimensions));
  assert_eq!(display.submit_damage(1,5,&[Rect{x:0,y:0,width:2,height:2}]),Ok(1));
  assert_eq!(display.present(1,5),Ok(0));
 }
 #[test]fn compositor_crash_releases_only_its_display(){
  let mut display=MultiDisplay::<2>::new();
  let(s,b)=lease(0);display.attach(1,5,s,b).unwrap();
  let(s,b)=lease(1);display.attach(2,6,s,b).unwrap();
  assert_eq!(display.retire_owner(1),1);
  assert_eq!(display.active(),1);
  assert_eq!(display.submit_damage(2,6,&[Rect{x:0,y:0,width:1,height:1}]),Ok(1));
 }
}


/// R012 functional CPU framebuffer fallback for GPU-less boot. The caller
/// owns a trusted scanout buffer; this does not grant or expose GPU DMA.
pub fn cpu_fill_rect(display:Scanout,stride_pixels:usize,
  framebuffer:&mut[u32],rect:Rect,argb:u32)->Result<usize,Error>{
 if display.width==0||display.height==0||
    display.width>MAX_WIDTH||display.height>MAX_HEIGHT||
    display.scanout>=16{return Err(Error::Dimensions);}
 let width=display.width as usize;let height=display.height as usize;
 if stride_pixels<width||rect.width==0||rect.height==0||
    rect.x.checked_add(rect.width).is_none_or(|v|v>display.width)||
    rect.y.checked_add(rect.height).is_none_or(|v|v>display.height){
   return Err(Error::Dimensions);
 }
 let size=stride_pixels.checked_mul(height).ok_or(Error::Dimensions)?;
 if framebuffer.len()<size{return Err(Error::Dimensions);}
 let pixels=(rect.width as usize).checked_mul(rect.height as usize)
   .ok_or(Error::Dimensions)?;
 for y in rect.y as usize..(rect.y+rect.height) as usize{
  let start=y*stride_pixels+rect.x as usize;
  framebuffer[start..start+rect.width as usize].fill(argb);
 }
 Ok(pixels)
}
/// Trusted compositor presenter can select CPU mode when GPU isolation
/// proof is absent, rather than issuing unsafe virtqueue commands.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum RenderBackend{CpuSafe,GpuIsolated}
pub fn choose_backend(gpu_present:bool,iommu_isolated:bool,
  pinned_backing:bool)->RenderBackend{
 if gpu_present&&iommu_isolated&&pinned_backing{
  RenderBackend::GpuIsolated
 }else{RenderBackend::CpuSafe}
}
#[cfg(test)]
mod r012_framebuffer_fallback_tests{
 use super::*;
 fn display()->Scanout{Scanout{width:4,height:3,scanout:0}}
 #[test]fn paints_bounded_region_without_touching_padding(){
  let mut pixels=[0x55u32;18];
  assert_eq!(cpu_fill_rect(display(),6,&mut pixels,
    Rect{x:1,y:1,width:2,height:2},0x11223344),Ok(4));
  assert_eq!(&pixels[7..9],&[0x11223344;2]);
  assert_eq!(&pixels[13..15],&[0x11223344;2]);
  assert_eq!(pixels[6],0x55);assert_eq!(pixels[9],0x55);
 }
 #[test]fn wrong_stride_and_overrun_rejected_without_pixels(){
  let mut pixels=[1u32;12];
  assert_eq!(cpu_fill_rect(display(),4,&mut pixels,
    Rect{x:3,y:0,width:2,height:1},2),Err(Error::Dimensions));
  assert_eq!(cpu_fill_rect(display(),3,&mut pixels,
    Rect{x:0,y:0,width:1,height:1},2),Err(Error::Dimensions));
  assert_eq!(pixels,[1u32;12]);
 }
 #[test]fn absent_dma_trust_selects_safe_cpu_fallback(){
  assert_eq!(choose_backend(true,false,true),RenderBackend::CpuSafe);
  assert_eq!(choose_backend(true,true,true),RenderBackend::GpuIsolated);
  assert_eq!(choose_backend(false,true,true),RenderBackend::CpuSafe);
 }
}
