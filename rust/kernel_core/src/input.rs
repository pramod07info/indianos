//! R014: bounded polling console input for single-core QEMU/OVMF.
//! COM1 receives bytes from QEMU -serial stdio; legacy PS/2 set-1 keyboard
//! is a best-effort secondary source. No keyboard IRQs or SMP yet.
//! No untrusted byte is interpreted as an escape sequence.
use core::arch::asm;
#[cfg(feature="test-irq-keyboard")]
use core::sync::atomic::{AtomicU64,Ordering};
#[cfg(feature="test-irq-keyboard")]
unsafe extern "C" {fn serial_write(message:*const core::ffi::c_char);}

#[inline(always)]
unsafe fn inb(port:u16)->u8 {
    let value:u8;
    unsafe{asm!("in al, dx",out("al")value,in("dx")port,options(nomem,nostack,preserves_flags));}
    value
}
#[inline(always)]
unsafe fn outb(port:u16,value:u8){
    unsafe{asm!("out dx, al",in("dx")port,in("al")value,options(nomem,nostack,preserves_flags));}
}
const COM1:u16=0x3f8;
const PS2_STATUS:u16=0x64;
const PS2_DATA:u16=0x60;
const KEYMAP:[u8;128]={
    let mut a=[0;128];
    a[2]=b'1';a[3]=b'2';a[4]=b'3';a[5]=b'4';a[6]=b'5';
    a[7]=b'6';a[8]=b'7';a[9]=b'8';a[10]=b'9';a[11]=b'0';
    a[12]=b'-';a[13]=b'=';a[14]=8;a[15]=b'\t';
    a[16]=b'q';a[17]=b'w';a[18]=b'e';a[19]=b'r';a[20]=b't';
    a[21]=b'y';a[22]=b'u';a[23]=b'i';a[24]=b'o';a[25]=b'p';
    a[26]=b'[';a[27]=b']';a[28]=b'\n';
    a[30]=b'a';a[31]=b's';a[32]=b'd';a[33]=b'f';a[34]=b'g';
    a[35]=b'h';a[36]=b'j';a[37]=b'k';a[38]=b'l';a[39]=b';';
    a[40]=b'\'';a[41]=b'\x60';a[43]=b'\\';
    a[44]=b'z';a[45]=b'x';a[46]=b'c';a[47]=b'v';a[48]=b'b';
    a[49]=b'n';a[50]=b'm';a[51]=b',';a[52]=b'.';a[53]=b'/';
    a[57]=b' ';
    a
};
const SHIFTED:[u8;128]={
    let mut a=KEYMAP;
    a[2]=b'!';a[3]=b'@';a[4]=b'#';a[5]=b'$';a[6]=b'%';
    a[7]=b'^';a[8]=b'&';a[9]=b'*';a[10]=b'(';a[11]=b')';
    a[12]=b'_';a[13]=b'+';a[26]=b'{';a[27]=b'}';
    a[39]=b':';a[40]=b'"';a[41]=b'~';a[43]=b'|';
    a[51]=b'<';a[52]=b'>';a[53]=b'?';
    a
};
#[derive(Default)]
pub(crate) struct ScanDecoder{shift_left:bool,shift_right:bool,caps:bool,extended:bool}
impl ScanDecoder{
    pub(crate) fn feed(&mut self, code:u8)->Option<u8>{
        if code==0xe0||code==0xe1{self.extended=true;return None;}
        if self.extended{self.extended=false;return None;}
        let up=code&0x80!=0;
        let key=(code&0x7f) as usize;
        match key{
            42=>{self.shift_left=!up;return None;}
            54=>{self.shift_right=!up;return None;}
            58=>{if !up{self.caps=!self.caps;}return None;}
            _=>{}
        }
        if up{return None;}
        let shift=self.shift_left||self.shift_right;
        let mut byte=if shift{SHIFTED[key]}else{KEYMAP[key]};
        // Shift and CapsLock combine using XOR for alphabetic keys.
        // SHIFTED maps punctuation; letters remain lowercase in both maps.
        if byte.is_ascii_alphabetic() && (shift!=self.caps) {
            byte=byte.to_ascii_uppercase();
        }
        filter(byte)
    }
}
pub(crate) fn filter(b:u8)->Option<u8>{
    match b {
        b'\r'|b'\n'=>Some(b'\n'),
        8|127=>Some(8),
        32..=126=>Some(b),
        _=>None,
    }
}
static mut SCAN:ScanDecoder=ScanDecoder{
    shift_left:false,shift_right:false,caps:false,extended:false
};
#[cfg(feature="test-irq-keyboard")]
const QUEUE_SIZE:usize=128;
#[cfg(feature="test-irq-keyboard")]
static mut QUEUE:[u8;QUEUE_SIZE]=[0;QUEUE_SIZE];
#[cfg(feature="test-irq-keyboard")]
static mut READ_INDEX:usize=0;
#[cfg(feature="test-irq-keyboard")]
static mut WRITE_INDEX:usize=0;
#[cfg(feature="test-irq-keyboard")]
static IRQ_EVENTS:AtomicU64=AtomicU64::new(0);
#[cfg(feature="test-irq-keyboard")]
static mut IRQ_ACTIVE:bool=false;
#[cfg(feature="test-irq-keyboard")]
static mut REPORTED_PS2_PENDING:bool=false;

/// Caller runs with IF=0 (IDT interrupt gate, single BSP). Queue producer
/// is IRQ1, queue consumer is the READ syscall's synchronous trap handler.
#[cfg(feature="test-irq-keyboard")]
fn enqueue(ch:u8) {
    unsafe{
        let next=(WRITE_INDEX+1)%QUEUE_SIZE;
        if next!=READ_INDEX{
            QUEUE[WRITE_INDEX]=ch;
            WRITE_INDEX=next;
        }
    }
}
#[cfg(feature="test-irq-keyboard")]
fn dequeue()->Option<u8>{
    unsafe{
        if READ_INDEX==WRITE_INDEX{return None;}
        let byte=QUEUE[READ_INDEX];
        READ_INDEX=(READ_INDEX+1)%QUEUE_SIZE;
        Some(byte)
    }
}
#[cfg(feature="test-irq-keyboard")]
fn io_wait(){unsafe{outb(0x80,0)}}
#[cfg(feature="test-irq-keyboard")]
fn wait_input()->bool{
    for _ in 0..100_000{
        if unsafe{inb(PS2_STATUS)}&2==0{return true;}
    }
    false
}
#[cfg(feature="test-irq-keyboard")]
fn wait_output()->Option<u8>{
    for _ in 0..100_000{
        if unsafe{inb(PS2_STATUS)}&1!=0{
            return Some(unsafe{inb(PS2_DATA)});
        }
    }
    None
}
/// Legacy PIC/PS2 initialization is opt-in, uniprocessor and must occur
/// after IDT 0x21 is ready, with IF masked. Other PIC IRQs stay masked.
#[cfg(feature="test-irq-keyboard")]
pub(crate) fn enable_irq1()->bool{
    let mut apic_lo:u32;
    let apic_hi:u32;
    unsafe{asm!("rdmsr",in("ecx")0x1bu32,
        out("eax")apic_lo,out("edx")apic_hi,options(nostack,preserves_flags));}
    if apic_lo&(1<<10)!=0{return false;}
    if apic_lo&(1<<11)!=0{
        apic_lo&=!(1<<11);
        unsafe{asm!("wrmsr",in("ecx")0x1bu32,
            in("eax")apic_lo,in("edx")apic_hi,options(nostack,preserves_flags));}
    }
    unsafe{
        outb(0x20,0x11);io_wait();outb(0xa0,0x11);io_wait();
        outb(0x21,0x20);io_wait();outb(0xa1,0x28);io_wait();
        outb(0x21,0x04);io_wait();outb(0xa1,0x02);io_wait();
        outb(0x21,0x01);io_wait();outb(0xa1,0x01);io_wait();
        outb(0x21,0xfd);outb(0xa1,0xff);
        outb(0x22,0x70);io_wait();outb(0x23,0);io_wait();
    }
    // Enable the PS/2 first port's clock and its IRQ1 config bit.
    if !wait_input(){return false;}
    unsafe{outb(PS2_STATUS,0xae);}
    if !wait_input(){return false;}
    unsafe{outb(PS2_STATUS,0x20);}
    let Some(conf)=wait_output()else{return false;};
    if !wait_input(){return false;}
    unsafe{outb(PS2_STATUS,0x60);}
    if !wait_input(){return false;}
    unsafe{outb(PS2_DATA,(conf|0x41)&!0x10);}
    // Scan codes arriving through the PS/2 controller must be read by IRQ1.
    unsafe{READ_INDEX=0;WRITE_INDEX=0;IRQ_ACTIVE=true;}
    IRQ_EVENTS.store(0,Ordering::Relaxed);
    true
}
#[cfg(feature="test-irq-keyboard")]
pub(crate) fn irq_count()->u64{IRQ_EVENTS.load(Ordering::Relaxed)}
#[cfg(feature="test-irq-keyboard")]
#[no_mangle]
pub extern "C" fn x86_64_keyboard_irq_dispatch(){
    let before=IRQ_EVENTS.fetch_add(1,Ordering::Relaxed);
    if before==0 {
        unsafe {serial_write(b"IndianOS: R014.2 IRQ1 first interrupt delivered\n\0".as_ptr().cast());}
    }
    let status=unsafe{inb(PS2_STATUS)};
    if status&1!=0 {
        let code=unsafe{inb(PS2_DATA)};
        if status&0x20==0 {
            if let Some(ch)=unsafe{(&mut *core::ptr::addr_of_mut!(SCAN)).feed(code)}{
                enqueue(ch);
            }
        }
    }
    unsafe{outb(0x20,0x20);} // specific master PIC EOI
}
pub(crate) fn poll_byte()->Option<u8>{
    #[cfg(feature="test-irq-keyboard")]
    if let Some(byte)=dequeue(){return Some(byte);}

    // Bounded non-blocking UART polling; the shell performs its own wait.
    if unsafe{inb(COM1+5)}&1!=0{
        let b=unsafe{inb(COM1)};
        return filter(b);
    }
    #[cfg(feature="test-irq-keyboard")]
    if unsafe{IRQ_ACTIVE}{
        let status=unsafe{inb(PS2_STATUS)};
        if status&1!=0&&status&0x20==0&&unsafe{!REPORTED_PS2_PENDING}{
            unsafe{
                REPORTED_PS2_PENDING=true;
                serial_write(b"IndianOS: R014.2 PS/2 data pending without IRQ1\n\0".as_ptr().cast());
            }
        }
        return None; // only IRQ1 may drain the keyboard data register
    }
    let status=unsafe{inb(PS2_STATUS)};
    // Avoid accidentally consuming auxiliary PS/2 mouse packets.
    if status&1!=0&&status&0x20==0{
        let code=unsafe{inb(PS2_DATA)};
        return unsafe{(&mut *core::ptr::addr_of_mut!(SCAN)).feed(code)};
    }
    None
}
pub(crate) fn echo(byte:u8){
    #[cfg(feature="test-irq-keyboard")]
    crate::console::echo(byte);
    fn send(ch:u8){
        for _ in 0..100_000 {
            if unsafe{inb(COM1+5)}&0x20!=0 {
                unsafe{outb(COM1,ch);}
                return;
            }
        }
    }
    match byte{
        8=>{send(8);send(b' ');send(8);}
        b'\n'=>{send(b'\r');send(b'\n');}
        32..=126=>send(byte),
        _=>{}
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn ps2_set1_letters_case_and_release(){
        let mut keys=ScanDecoder::default();
        assert_eq!(keys.feed(0x1e),Some(b'a'));
        assert_eq!(keys.feed(0x9e),None);
        assert_eq!(keys.feed(0x2a),None);
        assert_eq!(keys.feed(0x1e),Some(b'A'));
        assert_eq!(keys.feed(0xaa),None);
        assert_eq!(keys.feed(0x3a),None);
        assert_eq!(keys.feed(0x1e),Some(b'A'));
        assert_eq!(keys.feed(0x2a),None);
        assert_eq!(keys.feed(0x1e),Some(b'a'));
    }
    #[cfg(feature="test-irq-keyboard")]
    #[test]
    fn irq_fifo_wraps_and_never_overwrites(){
        unsafe{READ_INDEX=0;WRITE_INDEX=0;}
        for ch in 0..128u8{enqueue(ch);}
        let mut result=std::vec::Vec::new();
        while let Some(ch)=dequeue(){result.push(ch);}
        assert_eq!(result.len(),127);
        assert_eq!(result[0],0);
        assert_eq!(result[126],126);
        assert_eq!(dequeue(),None);
        enqueue(b'Z');assert_eq!(dequeue(),Some(b'Z'));
    }
    #[test]
    fn terminal_input_filters_controls(){
        assert_eq!(filter(b'\r'),Some(b'\n'));
        assert_eq!(filter(127),Some(8));
        assert_eq!(filter(0x1b),None);
        assert_eq!(filter(0),None);
        assert_eq!(filter(b'/'),Some(b'/'));
        let mut keys=ScanDecoder::default();
        assert_eq!(keys.feed(0xe0),None);
        assert_eq!(keys.feed(0x48),None);
        assert_eq!(keys.feed(0x1c),Some(b'\n'));
        assert_eq!(keys.feed(0x0e),Some(8));
    }
}
