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
/// Read all PIO words into private stack storage. A controller that fails
/// its final status check must not leave a half-read sector in caller memory.
/// This is invoked by the real hardware read path and is testable without
/// touching privileged I/O ports on the host.
fn read_words_staged<R,F>(out:&mut[u8;SECTOR_BYTES],
    mut read_word:R,finish:F)->Result<(),BlockError>
where R:FnMut()->u16,F:FnOnce()->Result<(),BlockError>{
    let mut staged=[0u8;SECTOR_BYTES];
    for pair in staged.chunks_exact_mut(2){
        pair.copy_from_slice(&read_word().to_le_bytes());
    }
    if let Err(error)=finish(){
        // Do not retain a partial hardware sector on the stack after failure.
        staged.fill(0);
        return Err(error);
    }
    out.copy_from_slice(&staged);
    Ok(())
}
/// Wait for a PIO transfer or FLUSH CACHE to finish, requiring DRQ clear.
fn complete()->Result<(),BlockError>{wait_status(false)}
/// Optional IDENTIFY words use bit 14 set and bit 15 clear as their
/// validity signature. Other patterns mean "not reported", not zero support.
fn identify_word_valid(word:u16)->bool { word&0xc000==0x4000 }
/// Optional ATA IDENTIFY integrity word: low byte A5 declares an eight-bit
/// checksum across the complete 512-byte response. Never authorize geometry
/// from a response which advertises integrity but fails that check.
fn identify_integrity_valid(words:&[u16;256])->bool{
    if words[255] as u8!=0xa5{return true;}
    words.iter().fold(0u8,|sum,word|
        sum.wrapping_add(*word as u8).wrapping_add((word>>8) as u8))==0
}
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
    if !identify_integrity_valid(words){return None;}
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
pub struct AtaPio { sectors:u64, last_stage:AtaStage, retired:bool }
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
        Ok(Self{sectors,last_stage:AtaStage::Idle,retired:false})
    }
    /// A failed transfer may have consumed data or partially changed media.
    /// Do not issue another command on that uncertain session. Recovery owns
    /// explicit device reprobe; this driver never resets/retries a command.
    fn run_io(&mut self,operation:impl FnOnce(&mut Self)->Result<(),BlockError>)
        ->Result<(),BlockError>{
        if self.retired{return Err(BlockError::Io);}
        self.retired=true;
        operation(self)?;
        self.retired=false;
        Ok(())
    }
    fn flush_cache(&mut self)->Result<(),BlockError>{
        self.last_stage=AtaStage::WaitIdle;
        complete()?;
        self.last_stage=AtaStage::FlushCommand;
        unsafe{outb(STATUS,0xe7);} // ATA FLUSH CACHE
        settle();
        self.last_stage=AtaStage::FlushDone;
        complete()?;
        self.last_stage=AtaStage::Idle;
        Ok(())
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
        check_range(self.sectors,lba,1)?;
        self.run_io(|device|{
        device.select(lba,0x20)?;
        device.last_stage=AtaStage::ReadWords;
        // All 256 words must be consumed and DRQ deasserted before
        // publishing bytes or issuing the next command.
        let last_stage=&mut device.last_stage;
        read_words_staged(out,||unsafe{inw(DATA)},||{
            *last_stage=AtaStage::ReadDone;
            // ATA requires a 400ns settle after the last PIO word before
            // its completion status is trusted (as in the write path).
            settle();
            complete()
        })?;
        device.last_stage=AtaStage::Idle;
        Ok(())
        })
    }
    fn write_sector(&mut self,lba:u64,data:&[u8;SECTOR_BYTES])->Result<(),BlockError>{
        check_range(self.sectors,lba,1)?;
        self.run_io(|device|{
        device.select(lba,0x30)?;
        device.last_stage=AtaStage::WriteWords;
        for pair in data.chunks_exact(2){
            unsafe{outw(DATA,u16::from_le_bytes([pair[0],pair[1]]));}
        }
        // ATA requires >=400ns settling after the last PIO data word;
        // polling too soon is especially unreliable under QEMU TCG.
        settle();
        // Do not issue FLUSH while the drive still has DRQ asserted.
        device.last_stage=AtaStage::WriteDone;
        complete()?;
        device.flush_cache()
        })
    }
    fn sync(&mut self)->Result<(),BlockError>{
        // Physical ATA FLUSH CACHE is an explicit durability barrier; do
        // not acknowledge filesystem fsync before the controller completes.
        self.run_io(Self::flush_cache)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_retirement_rejects_followup_hardware_commands_after_uncertain_io(){
        for stage in [AtaStage::ReadDone,AtaStage::WriteDone,AtaStage::FlushDone]{
            let mut disk=AtaPio{sectors:16,last_stage:AtaStage::Idle,retired:false};
            let mut commands=0;
            assert_eq!(disk.run_io(|device|{
                commands+=1;device.last_stage=stage;Err(BlockError::Io)
            }),Err(BlockError::Io));
            assert!(disk.retired);assert_eq!(disk.last_stage,stage);
            assert_eq!(disk.run_io(|_|{commands+=1;Ok(())}),Err(BlockError::Io));
            // These are the actual hardware entrypoints. On a retired session
            // they must return before any privileged port instruction executes.
            let mut out=[0xa5;SECTOR_BYTES];
            assert_eq!(disk.read_sector(0,&mut out),Err(BlockError::Io));
            assert_eq!(disk.write_sector(0,&[7;SECTOR_BYTES]),Err(BlockError::Io));
            assert_eq!(disk.sync(),Err(BlockError::Io));
            assert_eq!(out,[0xa5;SECTOR_BYTES]);assert_eq!(commands,1);
        }
    }
    #[test]
    fn storage_retirement_success_and_range_errors_do_not_revoke_live_device(){
        let mut disk=AtaPio{sectors:16,last_stage:AtaStage::Idle,retired:false};
        for _ in 0..2{
            assert_eq!(disk.run_io(|_|Ok(())),Ok(()));assert!(!disk.retired);
        }
        assert_eq!(disk.read_sector(16,&mut[0;SECTOR_BYTES]),Err(BlockError::OutOfRange));
        assert_eq!(disk.write_sector(u64::MAX,&[0;SECTOR_BYTES]),Err(BlockError::OutOfRange));
        assert!(!disk.retired);
    }

    #[test]
    fn m1_r088_staged_pio_read_exposes_no_bytes_on_completion_error(){
        let mut out=[0xa5;SECTOR_BYTES];
        let mut words=0u16;
        assert_eq!(read_words_staged(&mut out,||{
            words+=1;0x1234
        },||Err(BlockError::Io)),Err(BlockError::Io));
        assert_eq!(words,256); // Drain full sector before final status.
        assert_eq!(out,[0xa5;SECTOR_BYTES]);
    }
    #[test]
    fn m1_r088_staged_pio_read_publishes_full_little_endian_sector(){
        let mut out=[0xa5;SECTOR_BYTES];
        let mut words=0u16;
        assert_eq!(read_words_staged(&mut out,||{
            words+=1;
            if words%2==0{0x6655}else{0x4433}
        },||Ok(())),Ok(()));
        assert_eq!(words,256);
        for (i,pair) in out.chunks_exact(2).enumerate(){
            assert_eq!(pair,if i%2==0{&[0x33,0x44]}else{&[0x55,0x66]});
        }
    }
    #[test]
    fn r089_identify_rejects_non_ata_and_incomplete_identify_independently(){
        let base=identify_512();
        for flag in [0x8000u16,0x0004,0x8004]{
            let mut reported=base;
            reported[0]|=flag;
            assert_eq!(capacity(&reported),None);
        }
        assert_eq!(capacity(&base),Some(1024));
    }
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
    fn identify_declared_checksum_protects_geometry_and_all_response_words(){
        let mut words=identify_512();
        words[255]=0xa5;
        let sum=words.iter().fold(0u8,|sum,word|
            sum.wrapping_add(*word as u8).wrapping_add((word>>8) as u8));
        words[255]|=(sum.wrapping_neg() as u16)<<8;
        assert!(identify_integrity_valid(&words));
        assert_eq!(capacity(&words),Some(1024));
        for index in 0..256{
            let mut corrupt=words;
            // Preserve the optional signature while corrupting its checksum.
            corrupt[index]^=if index==255{0x100}else{1};
            assert!(!identify_integrity_valid(&corrupt),"word {index}");
            assert_eq!(capacity(&corrupt),None);
        }
        // Complete response bytes, not only the geometry fields, contribute.
        let mut corrupt=words;corrupt[100]^=0x8000;
        assert_eq!(capacity(&corrupt),None);
    }
    #[test]
    fn identify_legacy_optional_checksum_absence_preserves_supported_devices(){
        let mut words=identify_512();
        for unreported in [0,0xffff,0x5aa4,0xa500]{
            words[255]=unreported;
            assert!(identify_integrity_valid(&words));
            assert_eq!(capacity(&words),Some(1024));
        }
        words[255]=0x00a5;
        assert!(!identify_integrity_valid(&words));
        assert_eq!(capacity(&words),None);
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

#[cfg(test)]
mod r090_ata_acceptance {
 use super::*;
 fn base()->[u16;256]{
  let mut id=[0u16;256];
  id[49]=1<<9;
  id[60]=1024;
  id
 }
 /// R090 A01: absent controller status is failure
 #[test]fn r090_a01_absent_controller_status_is_failure(){assert_eq!(poll_status(0,true),PollResult::Failed);assert_eq!(poll_status(0,false),PollResult::Failed);}
 /// R090 A02: all ones controller status is failure
 #[test]fn r090_a02_all_ones_controller_status_is_failure(){assert_eq!(poll_status(0xff,true),PollResult::Failed);assert_eq!(poll_status(0xff,false),PollResult::Failed);}
 /// R090 A03: busy without error is pending
 #[test]fn r090_a03_busy_without_error_is_pending(){assert_eq!(poll_status(BSY,true),PollResult::Pending);assert_eq!(poll_status(BSY,false),PollResult::Pending);}
 /// R090 A04: busy with error waits for completion
 #[test]fn r090_a04_busy_with_error_waits_for_completion(){assert_eq!(poll_status(BSY|ERR,true),PollResult::Pending);}
 /// R090 A05: data ready requires drq
 #[test]fn r090_a05_data_ready_requires_drq(){assert_eq!(poll_status(0x40|DRQ,true),PollResult::Ready);}
 /// R090 A06: data ready rejects missing drq
 #[test]fn r090_a06_data_ready_rejects_missing_drq(){assert_eq!(poll_status(0x40,true),PollResult::Pending);}
 /// R090 A07: completion ready requires cleared drq
 #[test]fn r090_a07_completion_ready_requires_cleared_drq(){assert_eq!(poll_status(0x40,false),PollResult::Ready);}
 /// R090 A08: completion waits for drq to clear
 #[test]fn r090_a08_completion_waits_for_drq_to_clear(){assert_eq!(poll_status(0x40|DRQ,false),PollResult::Pending);}
 /// R090 A09: completed command error is rejected
 #[test]fn r090_a09_completed_command_error_is_rejected(){assert_eq!(poll_status(0x40|ERR,false),PollResult::Failed);assert_eq!(poll_status(0x40|ERR,true),PollResult::Failed);}
 /// R090 A10: completed command drive fault is rejected
 #[test]fn r090_a10_completed_command_drive_fault_is_rejected(){assert_eq!(poll_status(0x40|DF,false),PollResult::Failed);assert_eq!(poll_status(0x40|DF,true),PollResult::Failed);}
 /// R090 A11: identify rejects missing lba capability
 #[test]fn r090_a11_identify_rejects_missing_lba_capability(){let mut id=base();id[49]=0;assert_eq!(capacity(&id),None);}
 /// R090 A12: identify rejects zero sector capacity
 #[test]fn r090_a12_identify_rejects_zero_sector_capacity(){let mut id=base();id[60]=0;id[61]=0;assert_eq!(capacity(&id),None);}
 /// R090 A13: identify limits capacity to lba28
 #[test]fn r090_a13_identify_limits_capacity_to_lba28(){let mut id=base();id[60]=0xffff;id[61]=0xffff;assert_eq!(capacity(&id),Some(MAX_LBA28));}
 /// R090 A14: identify rejects non ata device
 #[test]fn r090_a14_identify_rejects_non_ata_device(){let mut id=base();id[0]=0x8000;assert_eq!(capacity(&id),None);}
 /// R090 A15: identify rejects incomplete response
 #[test]fn r090_a15_identify_rejects_incomplete_response(){let mut id=base();id[0]=0x0004;assert_eq!(capacity(&id),None);}
 /// R090 A16: identify rejects explicit no flush support
 #[test]fn r090_a16_identify_rejects_explicit_no_flush_support(){let mut id=base();id[83]=0x4000;assert_eq!(capacity(&id),None);}
 /// R090 A17: identify rejects large logical sector
 #[test]fn r090_a17_identify_rejects_large_logical_sector(){let mut id=base();id[106]=0x5000;id[117]=2048;id[118]=0;assert_eq!(capacity(&id),None);}
 /// R090 A18: identify rejects contradictory sector declaration
 #[test]fn r090_a18_identify_rejects_contradictory_sector_declaration(){let mut id=base();id[106]=0x5000;id[117]=256;id[118]=0;assert_eq!(capacity(&id),None);}
 /// R090 A19: identify accepts unreported optional sector size
 #[test]fn r090_a19_identify_accepts_unreported_optional_sector_size(){let mut id=base();id[106]=0;id[117]=2048;id[118]=0;assert_eq!(capacity(&id),Some(1024));}
 /// R090 A20: failed staged pio read never exposes partial words
 #[test]fn r090_a20_failed_staged_pio_read_never_exposes_partial_words(){let mut out=[0xa5;SECTOR_BYTES];let mut count=0;assert_eq!(read_words_staged(&mut out,||{count+=1;0x1234},||Err(BlockError::Io)),Err(BlockError::Io));assert_eq!(count,256);assert_eq!(out,[0xa5;SECTOR_BYTES]);}
}

#[cfg(test)]
mod r091_ata_staging {
 use super::*;
 #[test]fn failed_finish_keeps_caller_sector_untouched(){
  let mut out=[0x55;SECTOR_BYTES];let mut words=0;
  assert_eq!(read_words_staged(&mut out,||{words+=1;0xaa55},||Err(BlockError::Io)),Err(BlockError::Io));
  assert_eq!(words,256);assert_eq!(out,[0x55;SECTOR_BYTES]);
 }
 #[test]fn successful_finish_publishes_exact_words(){
  let mut out=[0;SECTOR_BYTES];
  assert_eq!(read_words_staged(&mut out,||0x55aa,||Ok(())),Ok(()));
  assert_eq!(&out[..4],&[0xaa,0x55,0xaa,0x55]);
 }
}
