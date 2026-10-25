//! Batch 5 tasks 11–13,20: combine preflight for DMA storage with
//! authenticated driver grants. These APIs do NOT touch physical hardware.
use crate::batch3_driver_isolation::{Drivers,DriverError};
use crate::batch3_ahci::{self,Region,AhciError};
use crate::batch3_acpi::{self,Table,AcpiError};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error{Driver(DriverError),Storage(AhciError),Acpi(AcpiError),Capacity}
pub struct IoPolicy<const N:usize>{grants:Drivers<N>}
impl<const N:usize> IoPolicy<N>{
 pub const fn new()->Self{Self{grants:Drivers::new()}}
 /// Trusted PCI enumeration supplies reserved MMIO/IRQ grants only after
 /// successful IOMMU ownership mapping; this module cannot establish that.
 pub fn grant(&mut self,driver:u32,epoch:u64,base:u64,bytes:u64,irq:u16)
  ->Result<(),Error>{
  self.grants.allocate(driver,epoch,base,bytes,irq).map_err(Error::Driver)
 }
 pub fn guard_mmio(&self,driver:u32,epoch:u64,addr:u64,size:u64)
  ->Result<(),Error>{
  self.grants.validate(driver,epoch,addr,size).map_err(Error::Driver)
 }
 /// Verified DMA buffers are NOT the BAR MMIO range; this method only checks
 /// descriptor validity, never authorizes DMA without an actual IOMMU allocator.
 pub fn ahci_preflight(&self,driver:u32,epoch:u64,
  port_mmio:u64,lba:u64,sectors:u16,total_sectors:u64,
  dma:&[Region])->Result<u64,Error>{
  self.guard_mmio(driver,epoch,port_mmio,4)?;
  batch3_ahci::validate_command(lba,sectors,total_sectors,dma).map_err(Error::Storage)
 }
 pub fn quarantine(&mut self,driver:u32,epoch:u64)->Result<(),Error>{
  self.grants.quarantine(driver,epoch).map_err(Error::Driver)
 }
}
pub fn valid_acpi_madt(table_bytes:&[u8])->Result<Table,Error>{
 let table=batch3_acpi::parse_sdt(table_bytes).map_err(Error::Acpi)?;
 if table.signature!=*b"APIC"{return Err(Error::Acpi(AcpiError::Signature));}
 Ok(table)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn stale_driver_cannot_authorize_io_after_quarantine(){
  let mut p=IoPolicy::<1>::new();
  p.grant(1,5,0x1000,0x1000,11).unwrap();
  assert_eq!(p.ahci_preflight(1,5,0x1100,0,8,100,
   &[Region{dma_address:0x4000,byte_count:4096}]),Ok(4096));
  p.quarantine(1,5).unwrap();
  assert!(p.ahci_preflight(1,5,0x1100,0,8,100,
   &[Region{dma_address:0x4000,byte_count:4096}]).is_err());
  assert!(p.guard_mmio(1,4,0x1100,4).is_err());
 }
 #[test]fn malformed_acpi_never_probes_irq_layout(){
  assert_eq!(valid_acpi_madt(&[]),Err(Error::Acpi(AcpiError::Truncated)));
 }
}