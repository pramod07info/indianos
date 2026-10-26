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