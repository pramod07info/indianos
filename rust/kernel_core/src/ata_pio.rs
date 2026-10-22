//! R015 experimental legacy ATA PIO driver for QEMU's IDE disk.
//! Polling only, primary master (I/O 0x1f0), LBA28, 512-byte sectors.
//! Never probe or write a physical disk without an explicit caller decision.
//! This is NOT AHCI, NVMe, virtio-blk, or a production storage stack.
use core::arch::asm;
use crate::block::{BlockDevice, BlockError, SECTOR_BYTES, check_range};

const DATA:u16=0x1f0;
const ERROR:u16=0x1f1;
const COUNT:u16=0x1f2;
const LBA0:u16=0x1f3;
const LBA1:u16=0x1f4;
const LBA2:u16=0x1f5;
const DRIVE:u16=0x1f6;
const STATUS:u16=0x1f7;
const CONTROL:u16=0x3f6;
const BSY:u8=0x80;
const DRQ:u8=0x08;
const ERR:u8=0x01;
const DF:u8=0x20;
const TIMEOUT:usize=1_000_000;
const MAX_LBA28:u64=1<<28;

#[inline]
unsafe fn inb(port:u16)->u8 {
    let value:u8;
    unsafe{asm!("in al, dx",in("dx")port,out("al")value,options(nomem,nostack,preserves_flags));}
    value
}
#[inline]
unsafe fn outb(port:u16,value:u8){
    unsafe{asm!("out dx, al",in("dx")port,in("al")value,options(nomem,nostack,preserves_flags));}
}
#[inline]
unsafe fn inw(port:u16)->u16 {
    let value:u16;
    unsafe{asm!("in ax, dx",in("dx")port,out("ax")value,options(nomem,nostack,preserves_flags));}
    value
}
#[inline]
unsafe fn outw(port:u16,value:u16){
    unsafe{asm!("out dx, ax",in("dx")port,in("ax")value,options(nomem,nostack,preserves_flags));}
}
fn settle(){
    for _ in 0..4 {let _=unsafe{inb(CONTROL)};}
}
fn ready(require_data:bool)->Result<(),BlockError>{
    for _ in 0..TIMEOUT {
        let status=unsafe{inb(STATUS)};
        if status==0 || status==0xff{return Err(BlockError::Io);}
        if status&BSY==0 {
            if status&(ERR|DF)!=0{return Err(BlockError::Io);}
            if !require_data || status&DRQ!=0{return Ok(());}
        }
        core::hint::spin_loop();
    }
    Err(BlockError::Io)
}
/// Wait for a PIO transfer or FLUSH CACHE command to finish. Unlike
/// ready(false), completion also requires DRQ to clear: a controller can
/// briefly expose BSY=0 while still requesting data.
fn complete()->Result<(),BlockError>{
    for _ in 0..TIMEOUT {
        let status=unsafe{inb(STATUS)};
        if status==0 || status==0xff{return Err(BlockError::Io);}
        if status&BSY==0 {
            if status&(ERR|DF)!=0{return Err(BlockError::Io);}
            if status&DRQ==0{return Ok(());}
        }
        core::hint::spin_loop();
    }
    Err(BlockError::Io)
}
fn capacity(words:&[u16;256])->Option<u64>{
    // IDENTIFY bit 0 of word 49 indicates LBA support.
    if words[49]&(1<<9)==0{return None;}
    let sectors=((words[61] as u64)<<16)|(words[60] as u64);
    if sectors==0{return None;}
    Some(sectors.min(MAX_LBA28))
}
pub struct AtaPio { sectors:u64 }
impl AtaPio {
    /// # Safety
    /// Only call on a single-core x86-64 guest with QEMU IDE primary master.
    /// The caller owns the ATA I/O ports and must exclude concurrent access.
    pub unsafe fn probe()->Result<Self,BlockError>{
        unsafe{outb(CONTROL,0x02);outb(DRIVE,0xa0);}
        settle();
        unsafe{
            outb(COUNT,0);outb(LBA0,0);outb(LBA1,0);outb(LBA2,0);
            outb(STATUS,0xec);
        }
        let first=unsafe{inb(STATUS)};
        if first==0 || first==0xff{return Err(BlockError::Io);}
        // ATAPI signatures are not ATA disks; never consume their payload.
        if unsafe{inb(LBA1)}!=0 || unsafe{inb(LBA2)}!=0{return Err(BlockError::Io);}
        ready(true)?;
        let mut words=[0u16;256];
        for word in &mut words{*word=unsafe{inw(DATA)};}
        let sectors=capacity(&words).ok_or(BlockError::Io)?;
        Ok(Self{sectors})
    }
    fn select(&self,lba:u64,command:u8)->Result<(),BlockError>{
        check_range(self.sectors,lba,1)?;
        if lba>=MAX_LBA28{return Err(BlockError::OutOfRange);}
        // A preceding PIO read may have deasserted BSY before its final
        // DRQ transition. Never program a new command until DRQ clears.
        complete()?;
        unsafe{
            outb(DRIVE,0xe0|((lba>>24) as u8&0x0f));
        }
        settle();
        unsafe{
            outb(ERROR,0); // features
            outb(COUNT,1);
            outb(LBA0,lba as u8);
            outb(LBA1,(lba>>8) as u8);
            outb(LBA2,(lba>>16) as u8);
            outb(STATUS,command);
        }
        ready(true)
    }
}
impl BlockDevice for AtaPio {
    fn sector_count(&self)->u64{self.sectors}
    fn read_sector(&mut self,lba:u64,out:&mut [u8;SECTOR_BYTES])->Result<(),BlockError>{
        self.select(lba,0x20)?;
        for pair in out.chunks_exact_mut(2){
            let value=unsafe{inw(DATA)}.to_le_bytes();
            pair.copy_from_slice(&value);
        }
        // All 256 words must be consumed and DRQ deasserted before
        // issuing the next command (especially WRITE SECTORS).
        complete()
    }
    fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
        self.select(lba,0x30)?;
        for pair in data.chunks_exact(2){
            unsafe{outw(DATA,u16::from_le_bytes([pair[0],pair[1]]));}
        }
        // Do not issue FLUSH while the drive still has DRQ asserted.
        complete()?;
        unsafe{outb(STATUS,0xe7);} // FLUSH CACHE (ATA command)
        complete()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identify_requires_lba_and_capacity(){
        let mut words=[0u16;256];
        assert_eq!(capacity(&words),None);
        words[49]=1<<9;
        assert_eq!(capacity(&words),None);
        words[60]=0x1234;words[61]=1;
        assert_eq!(capacity(&words),Some(0x11234));
        words[60]=0xffff;words[61]=0xffff;
        assert_eq!(capacity(&words),Some(MAX_LBA28));
    }
}
