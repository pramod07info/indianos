//! R040 tasks 11/12/13/20: enumeration-to-grant policy for storage devices.
//! PCI/MMIO discovery data MUST be supplied by a trusted kernel enumerator.
//! No physical BAR writes, DMA programming or interrupt routing occurs.
use crate::batch5_devices::{IoPolicy,Error as IoError};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind{Ahci,Nvme,Virtio}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Pci{pub vendor:u16,pub device:u16,pub class:u8,pub subclass:u8,
 pub prog_if:u8,pub mmio:u64,pub mmio_len:u64,pub irq:u16}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Invalid,Unsupported,Access(IoError)}
pub fn classify(p:Pci)->Result<Kind,Error>{
 if p.vendor==0||p.vendor==0xffff||p.device==0xffff||
  p.mmio==0||p.mmio%4096!=0||p.mmio_len<4096||
  p.mmio_len%4096!=0||p.irq==0||
  p.mmio.checked_add(p.mmio_len).is_none(){
  return Err(Error::Invalid);
 }
 if p.class==1&&p.subclass==6&&p.prog_if==1{return Ok(Kind::Ahci);}
 if p.class==1&&p.subclass==8&&p.prog_if==2{return Ok(Kind::Nvme);}
 if p.vendor==0x1af4{return Ok(Kind::Virtio);}
 Err(Error::Unsupported)
}
pub fn isolate_storage<const N:usize>(
 grants:&mut IoPolicy<N>,driver:u32,epoch:u64,p:Pci)->Result<Kind,Error>{
 let kind=classify(p)?;
 grants.grant(driver,epoch,p.mmio,p.mmio_len,p.irq)
  .map_err(Error::Access)?;
 Ok(kind)
}
#[cfg(test)]mod tests{
 use super::*;
 fn nvme()->Pci{Pci{vendor:0x8086,device:0x1234,class:1,subclass:8,
  prog_if:2,mmio:0x1000,mmio_len:4096,irq:9}}
 #[test]fn only_trusted_enumerated_mmio_is_granted(){
  let mut policy=IoPolicy::<1>::new();
  assert_eq!(isolate_storage(&mut policy,2,5,nvme()),Ok(Kind::Nvme));
  assert!(policy.guard_mmio(2,5,0x1000,4).is_ok());
  assert!(policy.guard_mmio(2,4,0x1000,4).is_err());
  policy.quarantine(2,5).unwrap();
  assert!(policy.guard_mmio(2,5,0x1000,4).is_err());
 }
 #[test]fn malformed_pci_config_not_accepted(){
  let mut p=nvme();p.mmio=u64::MAX-4095;
  assert_eq!(classify(p),Err(Error::Invalid));
  p=nvme();p.vendor=0xffff;assert_eq!(classify(p),Err(Error::Invalid));
 }
}