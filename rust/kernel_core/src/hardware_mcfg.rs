//! Issue #124 HD1: defensive ACPI MCFG / PCIe ECAM discovery metadata.
//!
//! Parse UNTRUSTED ACPI table bytes after ACPI checksum and length checks.
//! Numeric ECAM address computation is separate from privileged MMIO mapping.
//! This module intentionally does NOT dereference firmware physical ranges,
//! map a PCI device, allocate a BAR, activate DMA or choose a GPU driver.
use crate::batch3_acpi::{self,AcpiError};
pub const MAX_MCFG_WINDOWS:usize=16;
const ECAM_BUS_SIZE:u64=1<<20;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Window{
 pub base:u64,pub segment:u16,pub start_bus:u8,pub end_bus:u8
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{
 Acpi(AcpiError),WrongSignature,Malformed,TooMany,
 Overlap,Capacity,Address,
}
/// Transactionally decode a full ACPI MCFG table with allocations.
/// Output is left unchanged if ANY window is invalid or overlapping.
pub fn decode(bytes:&[u8],out:&mut[Option<Window>])
 ->Result<usize,Error>{
 const HEADER:usize=44;
 const ENTRY:usize=16;
 let table=batch3_acpi::parse_sdt(bytes).map_err(Error::Acpi)?;
 if table.signature!=*b"MCFG"{return Err(Error::WrongSignature);}
 let length=table.length as usize;
 if length<HEADER||(length-HEADER)%ENTRY!=0||
  bytes[36..HEADER].iter().any(|b|*b!=0){
  return Err(Error::Malformed);
 }
 let count=(length-HEADER)/ENTRY;
 if count==0||count>MAX_MCFG_WINDOWS{return Err(Error::TooMany);}
 if count>out.len(){return Err(Error::Capacity);}
 let mut staging:[Option<Window>;MAX_MCFG_WINDOWS]=[None;MAX_MCFG_WINDOWS];
 for i in 0..count{
  let start=HEADER+i*ENTRY;
  let entry=&bytes[start..start+ENTRY];
  let base=u64::from_le_bytes(entry[..8].try_into()
    .map_err(|_|Error::Malformed)?);
  let segment=u16::from_le_bytes([entry[8],entry[9]]);
  let start_bus=entry[10];let end_bus=entry[11];
  if entry[12..].iter().any(|byte|*byte!=0)||
    base==0||base%ECAM_BUS_SIZE!=0||start_bus>end_bus{
   return Err(Error::Malformed);
  }
  let bytes_required=((end_bus as u64-start_bus as u64)+1)
    .checked_mul(ECAM_BUS_SIZE).ok_or(Error::Address)?;
  let end=base.checked_add(bytes_required).ok_or(Error::Address)?;
  let window=Window{base,segment,start_bus,end_bus};
  for old in staging[..i].iter().flatten(){
   // Two independently authenticated ACPI MCFG ranges cannot claim
   // an overlapping physical aperture, even for different segments.
   let old_size=(old.end_bus as u64-old.start_bus as u64+1)
    *ECAM_BUS_SIZE;
   let old_end=old.base.checked_add(old_size).ok_or(Error::Address)?;
   if base<old_end && old.base<end{return Err(Error::Overlap);}
   // Within same segment, bus ranges must never be ambiguous.
   if old.segment==segment&&
    start_bus<=old.end_bus&&old.start_bus<=end_bus{
    return Err(Error::Overlap);
   }
  }
  staging[i]=Some(window);
 }
 out[..count].copy_from_slice(&staging[..count]);
 Ok(count)
}
/// Compute only the required numeric ECAM configuration-space byte
/// address. A privileged firmware/memory-map-validated VMM MMIO backend
/// must separately authorize and map it before actual PCIe accesses.
pub fn config_address(window:Window,segment:u16,bus:u8,
 device:u8,function:u8,offset:u16)->Result<u64,Error>{
 // Public Window values can bypass the checked MCFG decoder.
 if window.base==0||window.base%ECAM_BUS_SIZE!=0||
    window.start_bus>window.end_bus{return Err(Error::Address);}
 let span=(u64::from(window.end_bus)-u64::from(window.start_bus)+1)
    *ECAM_BUS_SIZE;
 if window.base.checked_add(span).is_none(){return Err(Error::Address);}
 if window.segment!=segment||bus<window.start_bus||
  bus>window.end_bus||device>=32||function>=8||offset>=4096{
  return Err(Error::Address);
 }
 let bus_offset=((bus-window.start_bus) as u64)<<20;
 let address=bus_offset|((device as u64)<<15)|
   ((function as u64)<<12)|offset as u64;
 window.base.checked_add(address).ok_or(Error::Address)
}
#[cfg(test)]
mod tests{
 use super::*;
 use std::vec::Vec;
 fn fixture(windows:&[Window])->Vec<u8>{
  let mut buffer=std::vec![0u8;44+16*windows.len()];
  buffer[..4].copy_from_slice(b"MCFG");
  let len=buffer.len() as u32;
  buffer[4..8].copy_from_slice(&len.to_le_bytes());
  buffer[8]=1;
  for (i,w) in windows.iter().enumerate(){
   let pos=44+16*i;
   buffer[pos..pos+8].copy_from_slice(&w.base.to_le_bytes());
   buffer[pos+8..pos+10].copy_from_slice(&w.segment.to_le_bytes());
   buffer[pos+10]=w.start_bus;buffer[pos+11]=w.end_bus;
  }
  let sum=buffer.iter().fold(0u8,|v,b|v.wrapping_add(*b));
  buffer[9]=0u8.wrapping_sub(sum);
  buffer
 }
 fn a()->Window{
  Window{base:0x8000_0000,segment:0,start_bus:0,end_bus:31}
 }
 #[test]fn hd1_mcfg_valid_checksum_segments_and_4k_function_address(){
  let other=Window{base:0x9000_0000,segment:2,
   start_bus:32,end_bus:47};
  let table=fixture(&[a(),other]);
  let mut output=[None;2];
  assert_eq!(decode(&table,&mut output),Ok(2));
  assert_eq!(output,[Some(a()),Some(other)]);
  assert_eq!(config_address(a(),0,1,2,3,0xffc),
   Ok(0x8000_0000+0x100000+0x10000+0x3000+0xffc));
  assert_eq!(config_address(other,2,32,0,0,0),Ok(0x9000_0000));
 }
 #[test]fn hd1_mcfg_rejects_checksum_corruption_without_partial_output(){
  let mut t=fixture(&[a()]);
  t[30]^=1;
  let mut out=[Some(a());2];
  assert_eq!(decode(&t,&mut out),Err(Error::Acpi(AcpiError::Checksum)));
  assert_eq!(out,[Some(a());2]);
 }
 #[test]fn hd1_mcfg_rejects_physically_overlapping_and_duplicate_segment_bus_ranges(){
  let collides=Window{base:0x8100_0000,segment:1,start_bus:0,end_bus:7};
  let t=fixture(&[a(),collides]);
  let mut out=[None;2];
  assert_eq!(decode(&t,&mut out),Err(Error::Overlap));
  let same_segment=Window{base:0xa000_0000,segment:0,
    start_bus:24,end_bus:48};
  assert_eq!(decode(&fixture(&[a(),same_segment]),&mut out),
    Err(Error::Overlap));
 }
 #[test]fn hd1_mcfg_rejects_short_capacity_reserved_and_malformed_ranges(){
  let mut out=[None;1];
  let first=fixture(&[a()]);
  let twice=fixture(&[a(),Window{
   base:0xa000_0000,segment:1,start_bus:0,end_bus:8
  }]);
  assert_eq!(decode(&twice,&mut out),Err(Error::Capacity));
  let mut damaged=first.clone();
  damaged[36]=1;
  let sum=damaged.iter().fold(0u8,|n,v|n.wrapping_add(*v));
  damaged[9]=damaged[9].wrapping_sub(sum);
  assert_eq!(decode(&damaged,&mut out),Err(Error::Malformed));
  let bad=Window{base:0x8000_0000,segment:0,start_bus:32,end_bus:2};
  assert_eq!(decode(&fixture(&[bad]),&mut out),Err(Error::Malformed));
 }
 #[test]fn hd1_mcfg_rejects_bogus_ecam_offsets_and_segments(){
  for (segment,bus,slot,function,offset) in [
   (1u16,0u8,0u8,0u8,0u16),
   (0,32,0,0,0),(0,0,32,0,0),(0,0,0,8,0),
   (0,0,0,0,4096)
  ]{
   assert_eq!(config_address(a(),segment,bus,slot,function,offset),
    Err(Error::Address));
  }
 }
}
#[cfg(test)]
mod g8_untrusted_ecam_tests {
 use super::*;
 fn good()->Window{Window{base:0x8000_0000,segment:0,start_bus:0,end_bus:7}}
 #[test] fn forged_windows_never_yield_numeric_ecam_authority() {
  for bad in [Window{base:0,..good()},Window{base:0x8000_0001,..good()},
              Window{start_bus:17,end_bus:16,..good()},
              Window{base:u64::MAX-0xfffff,start_bus:0,end_bus:0,..good()}]{
   assert_eq!(config_address(bad,0,0,0,0,0),Err(Error::Address));
  }
 }
 #[test] fn valid_ecam_edges_and_untrusted_device_parameters() {
  assert_eq!(config_address(good(),0,7,31,7,4095),
   Ok(0x8000_0000+7*0x100000+31*0x8000+7*0x1000+4095));
  for (bus,dev,fun,off) in [(8,0,0,0),(0,32,0,0),
                              (0,0,8,0),(0,0,0,4096)]{
   assert_eq!(config_address(good(),0,bus,dev,fun,off),Err(Error::Address));
  }
  assert_eq!(config_address(good(),1,0,0,0,0),Err(Error::Address));
 }
}

/// Validate width and alignment of a numeric PCIe configuration register access.
/// This never authorizes a BAR, MMIO mapping, bus master or DMA transaction.
pub fn config_access(window:Window,segment:u16,bus:u8,
 device:u8,function:u8,offset:u16,width:u8)->Result<u64,Error>{
 if !matches!(width,1|2|4)||offset%u16::from(width)!=0||
    u32::from(offset)+u32::from(width)>4096{return Err(Error::Address);}
 config_address(window,segment,bus,device,function,offset)
}
#[cfg(test)]
mod m1_ws08_ecam_width_tests {
 use super::*;
 #[test] fn config_register_width_cannot_cross_or_misalign_a_function() {
  let window=Window{base:0x8000_0000,segment:2,start_bus:8,end_bus:8};
  assert_eq!(config_access(window,2,8,0,0,4092,4),Ok(0x8000_0ffc));
  assert_eq!(config_access(window,2,8,0,0,4095,1),Ok(0x8000_0fff));
  for (off,width) in [(0,0),(0,3),(0,8),(1,2),(2,4),(4093,4),(4095,2)]{
   assert_eq!(config_access(window,2,8,0,0,off,width),Err(Error::Address));
  }
  assert_eq!(config_access(window,1,8,0,0,0,4),Err(Error::Address));
 }
}


// PR782 WS71-74: checked firmware window admission. All operations are
// read-only numeric policy, NOT physical MMIO authorization or DMA activation.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct RegisterAccess{pub device:u8,pub function:u8,pub offset:u16,pub width:u8}
/// WS71: Ensure a single public Window has complete 1 MiB bus-aligned
/// physical span and cannot wrap the 64-bit address space.
pub fn checked_window_span(window:Window)->Result<(u64,u64),Error>{
 if window.base==0||window.base%ECAM_BUS_SIZE!=0||
  window.start_bus>window.end_bus{return Err(Error::Address);}
 let buses=u64::from(window.end_bus)-u64::from(window.start_bus)+1;
 let bytes=buses.checked_mul(ECAM_BUS_SIZE).ok_or(Error::Address)?;
 let end=window.base.checked_add(bytes).ok_or(Error::Address)?;
 Ok((window.base,end))
}
/// WS72: A public firmware window array is not trusted merely because its
/// elements are aligned: reject duplicate segment/bus ownership and
/// physically overlapping ECAM apertures before selecting a bus.
pub fn find_unique_window(windows:&[Window],segment:u16,bus:u8)
 ->Result<Window,Error>{
 if windows.is_empty()||windows.len()>MAX_MCFG_WINDOWS{return Err(Error::Capacity);}
 let mut chosen=None;
 for (i,&w) in windows.iter().enumerate(){
  let (start,end)=checked_window_span(w)?;
  for &old in &windows[..i]{
   let (other_start,other_end)=checked_window_span(old)?;
   if start<other_end&&other_start<end{return Err(Error::Overlap);}
   if w.segment==old.segment&&w.start_bus<=old.end_bus
    &&old.start_bus<=w.end_bus{return Err(Error::Overlap);}
  }
  if w.segment==segment&&bus>=w.start_bus&&bus<=w.end_bus{
   if chosen.is_some(){return Err(Error::Overlap);}
   chosen=Some(w);
  }
 }
 chosen.ok_or(Error::Address)
}
/// WS73: Calculate an entire register request batch before writing any
/// caller output. Widths, offsets and same-function aliases cannot silently
/// cross a PCI function's 4KiB config page.
pub fn config_access_batch(window:Window,segment:u16,bus:u8,
 requests:&[RegisterAccess],out:&mut[u64])->Result<usize,Error>{
 if requests.is_empty()||requests.len()>32||requests.len()>out.len(){
  return Err(Error::Capacity);
 }
 for (i,&r) in requests.iter().enumerate(){
  config_access(window,segment,bus,r.device,r.function,r.offset,r.width)?;
  for &p in &requests[..i]{
   if p.device==r.device&&p.function==r.function{
    let left=u32::from(r.offset);let right=u32::from(p.offset);
    if left<right+u32::from(p.width)&&right<left+u32::from(r.width){
     return Err(Error::Overlap);
    }
   }
  }
 }
 for (i,&r) in requests.iter().enumerate(){
  out[i]=config_access(window,segment,bus,r.device,r.function,r.offset,r.width)?;
 }
 Ok(requests.len())
}
/// WS74: ECAM must never alias trusted firmware/device reserved apertures.
/// Every supplied window and exclusion is structurally checked first.
pub fn reject_reserved_apertures(windows:&[Window],
 reserved:&[(u64,u64)])->Result<(),Error>{
 if windows.is_empty()||windows.len()>MAX_MCFG_WINDOWS{return Err(Error::Capacity);}
 for (i,&w) in windows.iter().enumerate(){
  let (begin,end)=checked_window_span(w)?;
  for &earlier in &windows[..i]{
   let (a,b)=checked_window_span(earlier)?;
   if begin<b&&a<end{return Err(Error::Overlap);}
   if w.segment==earlier.segment&&w.start_bus<=earlier.end_bus
    &&earlier.start_bus<=w.end_bus{return Err(Error::Overlap);}
  }
  for &(start,length) in reserved{
   if length==0{return Err(Error::Address);}
   let stop=start.checked_add(length).ok_or(Error::Address)?;
   if begin<stop&&start<end{return Err(Error::Overlap);}
  }
 }
 Ok(())
}
#[cfg(test)]
mod pr782_ws71_74_tests{
 use super::*;
 fn w()->Window{Window{base:0x8000_0000,segment:0,start_bus:0,end_bus:1}}
 #[test]fn ws71_checked_span_handles_public_forged_windows(){
  assert_eq!(checked_window_span(w()),Ok((0x8000_0000,0x8020_0000)));
  assert_eq!(checked_window_span(Window{base:0,..w()}),Err(Error::Address));
  assert_eq!(checked_window_span(Window{base:u64::MAX-0xfffff,..w()}),
   Err(Error::Address));
 }
 #[test]fn ws72_unique_selection_checks_all_windows_even_unselected(){
  let second=Window{base:0x9000_0000,segment:2,start_bus:7,end_bus:7};
  assert_eq!(find_unique_window(&[w(),second],2,7),Ok(second));
  assert_eq!(find_unique_window(&[w(),w()],0,0),Err(Error::Overlap));
  assert_eq!(find_unique_window(&[w(),Window{base:0,..second}],0,0),
   Err(Error::Address));
  assert_eq!(find_unique_window(&[w()],1,0),Err(Error::Address));
 }
 #[test]fn ws73_batch_is_atomic_on_invalid_width_or_overlap(){
  let mut out=[99u64;2];
  let one=RegisterAccess{device:0,function:0,offset:0,width:4};
  let bad=RegisterAccess{offset:2,width:4,..one};
  assert_eq!(config_access_batch(w(),0,0,&[one,bad],&mut out),Err(Error::Address));
  assert_eq!(out,[99;2]);
  assert_eq!(config_access_batch(w(),0,0,&[one,one],&mut out),Err(Error::Overlap));
  assert_eq!(out,[99;2]);
  let next=RegisterAccess{offset:4,..one};
  assert_eq!(config_access_batch(w(),0,0,&[one,next],&mut out),Ok(2));
  assert_eq!(out,[0x8000_0000,0x8000_0004]);
 }
 #[test]fn ws74_reserved_apertures_and_overflows_are_denied(){
  assert_eq!(reject_reserved_apertures(&[w()],&[(0x8020_0000,0x1000)]),Ok(()));
  assert_eq!(reject_reserved_apertures(&[w()],&[(0x801f_f000,0x2000)]),
   Err(Error::Overlap));
  assert_eq!(reject_reserved_apertures(&[w()],&[(u64::MAX,2)]),
   Err(Error::Address));
 }
}

/// Preflight the complete firmware MCFG window set before publishing one
/// selected read-only ECAM register address. Reserved physical apertures and
/// ambiguous segment ownership fail closed, with no MMIO dereference.
pub fn config_access_checked_windows(windows:&[Window],
 reserved:&[(u64,u64)],segment:u16,bus:u8,request:RegisterAccess)
 ->Result<u64,Error>{
 let window=find_unique_window(windows,segment,bus)?;
 reject_reserved_apertures(windows,reserved)?;
 config_access(window,segment,bus,request.device,request.function,
               request.offset,request.width)
}
#[cfg(test)]
mod m1_gate20_w6_next_acpi_tests{
 use super::*;
 #[test]fn selected_ecam_access_checks_all_windows_and_reserved_memory(){
  let first=Window{base:0x8000_0000,segment:0,start_bus:0,end_bus:1};
  let peer=Window{base:0x9000_0000,segment:1,start_bus:4,end_bus:4};
  let access=RegisterAccess{device:2,function:0,offset:4,width:4};
  assert_eq!(config_access_checked_windows(&[first,peer],&[],0,0,access),
             Ok(0x8001_0004));
  assert_eq!(config_access_checked_windows(&[first,first],&[],0,0,access),
             Err(Error::Overlap));
  assert_eq!(config_access_checked_windows(&[first,peer],
             &[(0x8000_0000,4096)],0,0,access),Err(Error::Overlap));
  assert_eq!(config_access_checked_windows(&[first,peer],&[],0,0,
             RegisterAccess{offset:3,..access}),Err(Error::Address));
 }
}
