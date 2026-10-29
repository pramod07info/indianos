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
// QEMU TCG can pause a vCPU while its host write/flush I/O completes.
// Keep a finite failure bound, but tolerate slow or oversubscribed CI hosts.
// This is an iteration bound, not a production-grade calibrated timer.
const TIMEOUT:usize=8_000_000;
const MAX_LBA28:u64=1<<28;

/// Progress marker retained when an ATA operation fails. These phases are
/// observable only on a deliberately selected test device; no probing occurs
/// in regular boots.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum AtaStage {
    Idle, WaitIdle, WaitData, ReadWords, ReadDone, WriteWords,
    WriteDone, FlushCommand, FlushDone,
}
impl AtaStage {
    fn label(self)->&'static [u8] {
        match self {
            Self::Idle=>b"idle", Self::WaitIdle=>b"wait-idle",
            Self::WaitData=>b"wait-data", Self::ReadWords=>b"read-words",
            Self::ReadDone=>b"read-done", Self::WriteWords=>b"write-words",
            Self::WriteDone=>b"write-done", Self::FlushCommand=>b"flush-command",
            Self::FlushDone=>b"flush-done",
        }
    }
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum PollResult { Pending, Ready, Failed }
/// Pure state-machine decision, intentionally unit-testable without port I/O.
fn poll_status(status:u8,require_data:bool)->PollResult {
    if status==0 || status==0xff {return PollResult::Failed;}
    if status&BSY!=0 {return PollResult::Pending;}
    if status&(ERR|DF)!=0 {return PollResult::Failed;}
    if (status&DRQ!=0)==require_data {PollResult::Ready} else {PollResult::Pending}
}
fn append_ascii(out:&mut [u8],at:&mut usize,bytes:&[u8]) {
    for &byte in bytes {
        if *at+1>=out.len() {break;}
        out[*at]=byte;*at+=1;
    }
}
fn append_hex(out:&mut [u8],at:&mut usize,value:u8) {
    const HEX:&[u8;16]=b"0123456789abcdef";
    append_ascii(out,at,&[HEX[(value>>4) as usize],HEX[(value&15) as usize]]);
}
/// Null-terminated diagnostic, no heap, panic, or formatting allocation.
fn describe_failure(stage:AtaStage,status:u8,error:u8)->[u8;112] {
    let mut out=[0u8;112];let mut at=0;
    append_ascii(&mut out,&mut at,b"IndianOS: R015 ATA phase=");
    append_ascii(&mut out,&mut at,stage.label());
    append_ascii(&mut out,&mut at,b" status=0x");append_hex(&mut out,&mut at,status);
    append_ascii(&mut out,&mut at,b" error=0x");append_hex(&mut out,&mut at,error);
    append_ascii(&mut out,&mut at,b"\n");
    out
}

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
fn wait_status(require_data:bool)->Result<(),BlockError>{
    for _ in 0..TIMEOUT {
        let status=unsafe{inb(STATUS)};
        match poll_status(status,require_data) {
            PollResult::Ready=>return Ok(()),
            PollResult::Failed=>return Err(BlockError::Io),
            PollResult::Pending=>core::hint::spin_loop(),
        }
    }
    Err(BlockError::Io)
}
fn ready()->Result<(),BlockError>{wait_status(true)}
/// Wait for a PIO transfer or FLUSH CACHE to finish, requiring DRQ clear.
fn complete()->Result<(),BlockError>{wait_status(false)}
/// Optional IDENTIFY words use bit 14 set and bit 15 clear as their
/// validity signature. Other patterns mean "not reported", not zero support.
fn identify_word_valid(word:u16)->bool { word&0xc000==0x4000 }
fn logical_sector_words(words:&[u16;256])->Option<u32>{
    if !identify_word_valid(words[106]) || words[106]&(1<<12)==0 {
        return Some(256);
    }
    let size=((words[118] as u32)<<16)|words[117] as u32;
    // Bit 12 declares a sector longer than 256 words; smaller values
    // contradict that declaration, including zero/uninitialized sizes.
    if size<=256 {None} else {Some(size)}
}
fn capacity(words:&[u16;256])->Option<u64>{
    // Word 0 bit 15 identifies a non-ATA device; bit 2 means that the
    // IDENTIFY response is incomplete (e.g. requires spin-up first).
    if words[0]&0x8004!=0 {return None;}
    // Word 49 bit 9 indicates LBA support (it has no optional validity tag).
    if words[49]&(1<<9)==0 {return None;}
    // Words 117/118 count words, not bytes. Reject both malformed sizes
    // and valid larger logical sectors before exposing a 512-byte device.
    if logical_sector_words(words)?!=256 {return None;}
    // A valid command-set declaration explicitly denying FLUSH CACHE is
    // incompatible with our write path. Legacy devices with no declaration
    // remain test-only candidates; successful command completion is required.
    if identify_word_valid(words[83]) && words[83]&(1<<12)==0 {return None;}
    let sectors=((words[61] as u64)<<16)|(words[60] as u64);
    if sectors==0{return None;}
    Some(sectors.min(MAX_LBA28))
}
pub struct AtaPio { sectors:u64, last_stage:AtaStage }
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
        ready()?;
        let mut words=[0u16;256];
        for word in &mut words{*word=unsafe{inw(DATA)};}
        complete()?;
        let sectors=capacity(&words).ok_or(BlockError::Io)?;
        Ok(Self{sectors,last_stage:AtaStage::Idle})
    }
    /// Capture the last failed phase and live ATA task-file status.
    /// This does not reset or retry a possibly partially committed write.
    pub fn failure_diagnostic(&self)->[u8;112] {
        let status=unsafe{inb(CONTROL)}; // alternate status does not acknowledge IRQ
        let error=if status&BSY==0 {unsafe{inb(ERROR)}} else {0xff};
        describe_failure(self.last_stage,status,error)
    }
    fn select(&mut self,lba:u64,command:u8)->Result<(),BlockError>{
        check_range(self.sectors,lba,1)?;
        if lba>=MAX_LBA28{return Err(BlockError::OutOfRange);}
        // A preceding PIO read may have deasserted BSY before its final
        // DRQ transition. Never program a new command until DRQ clears.
        self.last_stage=AtaStage::WaitIdle;
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
        self.last_stage=AtaStage::WaitData;
        ready()
    }
}
impl BlockDevice for AtaPio {
    fn sector_count(&self)->u64{self.sectors}
    fn read_sector(&mut self,lba:u64,out:&mut [u8;SECTOR_BYTES])->Result<(),BlockError>{
        self.select(lba,0x20)?;
        self.last_stage=AtaStage::ReadWords;
        for pair in out.chunks_exact_mut(2){
            let value=unsafe{inw(DATA)}.to_le_bytes();
            pair.copy_from_slice(&value);
        }
        // All 256 words must be consumed and DRQ deasserted before
        // issuing the next command (especially WRITE SECTORS).
        self.last_stage=AtaStage::ReadDone;
        complete()?;
        self.last_stage=AtaStage::Idle;
        Ok(())
    }
    fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
        self.select(lba,0x30)?;
        self.last_stage=AtaStage::WriteWords;
        for pair in data.chunks_exact(2){
            unsafe{outw(DATA,u16::from_le_bytes([pair[0],pair[1]]));}
        }
        // ATA requires >=400ns settling after the last PIO data word;
        // polling too soon is especially unreliable under QEMU TCG.
        settle();
        // Do not issue FLUSH while the drive still has DRQ asserted.
        self.last_stage=AtaStage::WriteDone;
        complete()?;
        self.last_stage=AtaStage::FlushCommand;
        unsafe{outb(STATUS,0xe7);} // FLUSH CACHE (ATA command)
        settle();
        self.last_stage=AtaStage::FlushDone;
        complete()?;
        self.last_stage=AtaStage::Idle;
        Ok(())
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
    fn identify_512()->[u16;256] {
        let mut words=[0;256];
        words[49]=1<<9;words[60]=1024;
        words
    }
    #[test]
    fn identify_rejects_4kn_and_malformed_long_sector_declarations(){
        for size in [0u32,1,255,256,257,2048,u32::MAX] {
            let mut words=identify_512();
            words[106]=0x5000;
            words[117]=size as u16;words[118]=(size>>16) as u16;
            assert_eq!(capacity(&words),None,"logical words={size}");
        }
    }
    #[test]
    fn identify_optional_geometry_uses_ata_validity_signature(){
        let mut words=identify_512();
        // Legacy absent/unreported geometry defaults to 512-byte logical
        // sectors. Invalid signatures must not activate the size fields.
        for declaration in [0,0x1000,0x9000,0xd000,0x4000,0x6003] {
            words[106]=declaration;
            words[117]=2048;words[118]=0xffff;
            assert_eq!(capacity(&words),Some(1024));
        }
        // 512e physical-sector multiples do not change logical transfer size.
        words[106]=0x6003;
        assert_eq!(capacity(&words),Some(1024));
    }
    #[test]
    fn identify_rejects_non_ata_incomplete_and_explicitly_missing_flush(){
        let mut words=identify_512();
        for identity in [0x8000,0x0004,0xffff] {
            words[0]=identity;
            assert_eq!(capacity(&words),None);
        }
        words[0]=0x0040;
        words[83]=0x4000;
        assert_eq!(capacity(&words),None);
        words[83]=0x5000;
        assert_eq!(capacity(&words),Some(1024));
        for unreported in [0,0x1000,0x8000,0xffff] {
            words[83]=unreported;
            assert_eq!(capacity(&words),Some(1024));
        }
    }
    #[test]
    fn identify_lba28_bounds(){
        let mut words=identify_512();
        for sectors in [1,MAX_LBA28-1,MAX_LBA28,MAX_LBA28+1,u32::MAX as u64] {
            words[60]=sectors as u16;words[61]=(sectors>>16) as u16;
            assert_eq!(capacity(&words),Some(sectors.min(MAX_LBA28)));
        }
    }
    #[test]
    fn transfer_states_require_expected_drq_and_no_error(){
        assert_eq!(poll_status(0x48,true),PollResult::Ready);
        assert_eq!(poll_status(0x40,false),PollResult::Ready);
        assert_eq!(poll_status(0x48,false),PollResult::Pending);
        assert_eq!(poll_status(0x40,true),PollResult::Pending);
        assert_eq!(poll_status(0x80,true),PollResult::Pending);
        assert_eq!(poll_status(0x81,true),PollResult::Pending);
    }
    #[test]
    fn absent_broken_and_failing_controller_fail_closed(){
        for status in [0x00,0xff,0x41,0x60,0x49] {
            assert_eq!(poll_status(status,true),PollResult::Failed);
            assert_eq!(poll_status(status,false),PollResult::Failed);
        }
    }
    #[test]
    fn diagnostic_reports_exact_failed_phase_status_and_error(){
        let report=describe_failure(AtaStage::FlushDone,0x41,0x04);
        let expected=b"IndianOS: R015 ATA phase=flush-done status=0x41 error=0x04\n\0";
        assert_eq!(&report[..expected.len()],expected);
        assert!(report[expected.len()..].iter().all(|&x|x==0));
    }
    #[test]
    fn all_failure_stages_fit_static_serial_buffer(){
        for stage in [AtaStage::Idle,AtaStage::WaitIdle,AtaStage::WaitData,
            AtaStage::ReadWords,AtaStage::ReadDone,AtaStage::WriteWords,
            AtaStage::WriteDone,AtaStage::FlushCommand,AtaStage::FlushDone]{
            let report=describe_failure(stage,0x80,0xff);
            assert_eq!(report[111],0);
            assert!(report.iter().any(|&x|x==b'\n'));
        }
    }

}
