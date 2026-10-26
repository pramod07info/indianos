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
        if region.byte_count==0||region.dma_address%4096!=0 ||
            region.byte_count%SECTOR_BYTES as u32!=0{
            return Err(AhciError::InvalidBuffer);
        }
        let end=region.dma_address.checked_add(u64::from(region.byte_count))
            .ok_or(AhciError::Overflow)?;
        for prev in &regions[..i]{
            let prev_end=prev.dma_address+u64::from(prev.byte_count);
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
