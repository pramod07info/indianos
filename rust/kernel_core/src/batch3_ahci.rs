//! R037/22: AHCI/SATA DMA transfer descriptor preflight.
//! Real HBA initialization, MMIO, DMA mapping and flush barriers are NOT here.
pub const SECTOR_BYTES:u64=512;
pub const MAX_PRDT:usize=8;
pub const MAX_TRANSFER_BYTES:u64=128*1024;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Region{pub dma_address:u64,pub byte_count:u32}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum AhciError{InvalidRange,InvalidBuffer,TooMany,WrongSize,Overlap,Overflow}
/// Only accepts page-aligned DMA buffers owned/mapped by an external IOMMU
/// allocator; callers must never treat an unchecked guest physical address as safe.
pub fn validate_command(lba:u64,sectors:u16,disk_sectors:u64,
    regions:&[Region])->Result<u64,AhciError>{
    if sectors==0||lba>=disk_sectors||
        u64::from(sectors)>disk_sectors-lba{return Err(AhciError::InvalidRange);}
    if regions.is_empty()||regions.len()>MAX_PRDT{return Err(AhciError::TooMany);}
    let bytes=u64::from(sectors)*SECTOR_BYTES;
    if bytes>MAX_TRANSFER_BYTES{return Err(AhciError::WrongSize);}
    let mut total=0u64;
    for (i,region) in regions.iter().enumerate(){
        if region.byte_count==0||region.dma_address==0||region.dma_address%4096!=0 ||
            region.byte_count%SECTOR_BYTES as u32!=0{
            return Err(AhciError::InvalidBuffer);
        }
        let end=region.dma_address.checked_add(u64::from(region.byte_count))
            .ok_or(AhciError::Overflow)?;
        for prev in &regions[..i]{
            let prev_end=prev.dma_address.checked_add(u64::from(prev.byte_count))
                .ok_or(AhciError::Overflow)?;
            if region.dma_address<prev_end&&prev.dma_address<end{
                return Err(AhciError::Overlap);
            }
        }
        total=total.checked_add(u64::from(region.byte_count)).ok_or(AhciError::Overflow)?;
    }
    if total!=bytes{return Err(AhciError::WrongSize);}
    Ok(bytes)
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn valid_scatter_gather_read(){
        assert_eq!(validate_command(8,16,100,&[
            Region{dma_address:0x1000,byte_count:4096},
            Region{dma_address:0x3000,byte_count:4096}]),Ok(8192));
    }
    #[test]fn bounds_alignment_and_overlapping_dma_are_rejected(){
        assert_eq!(validate_command(99,2,100,&[Region{
            dma_address:0x1000,byte_count:1024}]),Err(AhciError::InvalidRange));
        assert_eq!(validate_command(1,1,100,&[Region{
            dma_address:0x1001,byte_count:512}]),Err(AhciError::InvalidBuffer));
        assert_eq!(validate_command(1,16,100,&[
            Region{dma_address:0x1000,byte_count:4096},
            Region{dma_address:0x1000,byte_count:4096}]),Err(AhciError::Overlap));
    }
    #[test]fn short_dma_never_accepted(){
        assert_eq!(validate_command(1,2,100,&[Region{
            dma_address:0x2000,byte_count:512}]),Err(AhciError::WrongSize));
        assert_eq!(validate_command(1,1,100,&[]),Err(AhciError::TooMany));
    }
}

#[cfg(test)]
mod m1_ws01_ahci_dma_tests {
 use super::*;
 #[test] fn rejects_unowned_zero_and_wrapping_dma_ranges() {
  assert_eq!(validate_command(0,8,64,&[Region{dma_address:0,byte_count:4096}]),
   Err(AhciError::InvalidBuffer));
  assert_eq!(validate_command(0,16,64,&[Region{
    dma_address:u64::MAX & !4095,byte_count:8192}]),Err(AhciError::Overflow));
  assert_eq!(validate_command(0,16,64,&[
    Region{dma_address:0x1000,byte_count:4096},
    Region{dma_address:0x4000,byte_count:4096}]),Ok(8192));
 }
}

/// Requires both structurally safe PRDT geometry and externally verified
/// DMA buffer ownership. A trusted IOMMU allocator supplies the callback;
/// a software predicate alone does NOT establish physical DMA isolation.
pub fn validate_mapped_command<F>(lba:u64,sectors:u16,disk_sectors:u64,
 regions:&[Region],mut mapped:F)->Result<u64,AhciError>
where F:FnMut(u64,u64)->bool{
 let bytes=validate_command(lba,sectors,disk_sectors,regions)?;
 for region in regions {
  if !mapped(region.dma_address,u64::from(region.byte_count)){
   return Err(AhciError::InvalidBuffer);
  }
 }
 Ok(bytes)
}
#[cfg(test)]
mod ws22_ahci_owned_buffer_tests{
 use super::*;
 use core::cell::Cell;
 #[test]fn geometry_failure_precedes_mapping_and_unowned_page_denies(){
  let regions=[Region{dma_address:0x1000,byte_count:4096},
               Region{dma_address:0x3000,byte_count:4096}];
  let calls=Cell::new(0u32);
  assert_eq!(validate_mapped_command(0,17,16,&regions,|_,_|{
   calls.set(calls.get()+1);true
  }),Err(AhciError::InvalidRange));
  assert_eq!(calls.get(),0);
  assert_eq!(validate_mapped_command(0,16,100,&regions,|addr,_|addr==0x1000),
    Err(AhciError::InvalidBuffer));
  assert_eq!(validate_mapped_command(0,16,100,&regions,|addr,size|{
    calls.set(calls.get()+1);size==4096&&addr>=0x1000
  }),Ok(8192));
 }
}


// PR #782 WS31-34: fail-closed media leases, reserved apertures, pinned
// per-page DMA ownership and a software-only flush completion fence.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum IoDirection { Read, Write }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct MediaLease {
    pub issued_epoch:u64, pub live_epoch:u64, pub read_only:bool
}
/// WS31: Bind the media generation and write privilege *before* trusting PRDT
/// mappings. Mapping verification MUST come from the trusted IOMMU owner.
pub fn preflight_io<F>(lease:MediaLease, direction:IoDirection, lba:u64,
 sectors:u16, disk_sectors:u64, regions:&[Region], mapped:F)
 -> Result<u64,AhciError> where F:FnMut(u64,u64)->bool {
 if lease.issued_epoch==0||lease.issued_epoch!=lease.live_epoch {
  return Err(AhciError::InvalidRange);
 }
 if direction==IoDirection::Write&&lease.read_only {
  return Err(AhciError::InvalidRange);
 }
 validate_mapped_command(lba,sectors,disk_sectors,regions,mapped)
}
/// WS32: A valid DMA descriptor must not overlap any trusted firmware/MMIO
/// exclusion [start, start+length), including by overflow or partial overlap.
pub fn validate_outside_reserved(lba:u64,sectors:u16,disk_sectors:u64,
 regions:&[Region],reserved:&[(u64,u64)]) -> Result<u64,AhciError> {
 let bytes=validate_command(lba,sectors,disk_sectors,regions)?;
 for &(start,length) in reserved {
  if length==0 {return Err(AhciError::InvalidRange);}
  let end=start.checked_add(length).ok_or(AhciError::Overflow)?;
  for r in regions {
   let stop=r.dma_address.checked_add(u64::from(r.byte_count))
      .ok_or(AhciError::Overflow)?;
   if r.dma_address<end&&start<stop{return Err(AhciError::Overlap);}
  }
 }
 Ok(bytes)
}
/// WS33: Validate the ownership of *each* 4K DMA page, not just its first
/// byte. A real allocator/IOMMU must provide the predicate. No DMA is started.
pub fn validate_owned_pages<F>(lba:u64,sectors:u16,disk_sectors:u64,
 regions:&[Region],mut owns_page:F)->Result<u64,AhciError>
 where F:FnMut(u64,u64)->bool {
 let bytes=validate_command(lba,sectors,disk_sectors,regions)?;
 for region in regions {
  let mut offset=0u64;
  while offset<u64::from(region.byte_count) {
   let address=region.dma_address.checked_add(offset).ok_or(AhciError::Overflow)?;
   if !owns_page(address,4096){return Err(AhciError::InvalidBuffer);}
   offset=offset.checked_add(4096).ok_or(AhciError::Overflow)?;
  }
 }
 Ok(bytes)
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum FlushState { Idle, Written, FlushSubmitted, Durable }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct WriteFlushFence { epoch:u64, ticket:u64, state:FlushState }
/// WS34: A write is never reported durable before an exact-generation flush
/// completion. Caller must separately issue/verify real controller commands.
impl WriteFlushFence {
 pub const fn new(epoch:u64)->Self {
  Self{epoch,ticket:0,state:FlushState::Idle}
 }
 pub fn begin(&mut self,epoch:u64,ticket:u64)->Result<(),AhciError>{
  if epoch==0||epoch!=self.epoch||ticket==0||
   !matches!(self.state,FlushState::Idle|FlushState::Durable){
   return Err(AhciError::InvalidRange);
  }
  self.ticket=ticket;self.state=FlushState::Written;Ok(())
 }
 pub fn submitted_flush(&mut self,epoch:u64,ticket:u64)->Result<(),AhciError>{
  if epoch!=self.epoch||ticket!=self.ticket||self.state!=FlushState::Written {
   return Err(AhciError::InvalidRange);
  }
  self.state=FlushState::FlushSubmitted;Ok(())
 }
 pub fn acknowledge_flush(&mut self,epoch:u64,ticket:u64)->Result<(),AhciError>{
  if epoch!=self.epoch||ticket!=self.ticket||self.state!=FlushState::FlushSubmitted {
   return Err(AhciError::InvalidRange);
  }
  self.state=FlushState::Durable;Ok(())
 }
 pub fn durable(&self,epoch:u64,ticket:u64)->bool{
  epoch!=0&&epoch==self.epoch&&ticket!=0&&ticket==self.ticket&&
   self.state==FlushState::Durable
 }
}
#[cfg(test)]
mod pr782_ws31_34_tests {
 use super::*;
 fn region()->[Region;1]{[Region{dma_address:0x4000,byte_count:4096}]}
 #[test] fn ws31_readonly_and_stale_epoch_do_not_call_mapper(){
  let mut called=0;
  let lease=MediaLease{issued_epoch:4,live_epoch:5,read_only:false};
  assert_eq!(preflight_io(lease,IoDirection::Read,0,8,100,&region(),|_,_|{
   called+=1;true
  }),Err(AhciError::InvalidRange));assert_eq!(called,0);
  let lease=MediaLease{issued_epoch:5,live_epoch:5,read_only:true};
  assert_eq!(preflight_io(lease,IoDirection::Write,0,8,100,&region(),|_,_|true),
   Err(AhciError::InvalidRange));
  assert_eq!(preflight_io(lease,IoDirection::Read,0,8,100,&region(),|a,n|{
   a==0x4000&&n==4096
  }),Ok(4096));
 }
 #[test] fn ws32_reserved_ranges_deny_partial_and_overflow(){
  assert_eq!(validate_outside_reserved(0,8,100,&region(),&[(0x4800,64)]),
   Err(AhciError::Overlap));
  assert_eq!(validate_outside_reserved(0,8,100,&region(),&[(0x5000,4096)]),
   Ok(4096));
  assert_eq!(validate_outside_reserved(0,8,100,&region(),&[(u64::MAX,3)]),
   Err(AhciError::Overflow));
 }
 #[test] fn ws33_each_dma_page_must_be_owned(){
  let rs=[Region{dma_address:0x4000,byte_count:8192}];
  let mut called=0;
  assert_eq!(validate_owned_pages(0,16,100,&rs,|address,bytes|{
   called+=1;address==0x4000&&bytes==4096
  }),Err(AhciError::InvalidBuffer));
  assert_eq!(called,2);
  assert_eq!(validate_owned_pages(0,16,100,&rs,|a,n|{
   (a==0x4000||a==0x5000)&&n==4096
  }),Ok(8192));
 }
 #[test] fn ws34_cannot_claim_durable_without_matching_flush_ack(){
  let mut fence=WriteFlushFence::new(8);
  fence.begin(8,91).unwrap();
  assert!(!fence.durable(8,91));
  assert_eq!(fence.acknowledge_flush(8,91),Err(AhciError::InvalidRange));
  fence.submitted_flush(8,91).unwrap();
  assert_eq!(fence.acknowledge_flush(7,91),Err(AhciError::InvalidRange));
  fence.acknowledge_flush(8,91).unwrap();
  assert!(fence.durable(8,91));assert!(!fence.durable(8,92));
 }
}
