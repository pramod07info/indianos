//! R014: bounded polling console input for single-core QEMU/OVMF.
//! COM1 receives bytes from QEMU -serial stdio; legacy PS/2 set-1 keyboard
//! is a best-effort secondary source. No keyboard IRQs or SMP yet.
//! No untrusted byte is interpreted as an escape sequence.
use core::arch::asm;
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
/// Bounded, stateful Set-1 decoder for the actual legacy 8042 keyboard.
/// Only committed make codes become characters. Ctrl/Alt modified keys do
/// not accidentally perform the unmodified GUI/Terminal shortcut.
#[derive(Default)]
pub(crate) struct ScanDecoder{
    shift_left:bool,shift_right:bool,caps:bool,caps_down:bool,extended:bool,
    ctrl_left:bool,ctrl_right:bool,alt_left:bool,alt_right:bool,
    pause_remaining:u8,
}
impl ScanDecoder{
    /// A discarded primary transfer may have been a modifier break or prefix.
    /// Keep the committed lock choice, but forget all untrustworthy held and
    /// multi-byte state. The discarded byte never produces input.
    fn feed_transfer(&mut self,status:u8,code:u8)->Option<u8>{
        if status&1==0 || status&0x20!=0{return None;}
        if !valid_keyboard_status(status){
            let caps=self.caps;
            *self=Self::default();
            self.caps=caps;
            return None;
        }
        self.feed(code)
    }

    pub(crate) fn feed(&mut self,code:u8)->Option<u8>{
        if self.pause_remaining>0{
            self.pause_remaining-=1;
            return None;
        }
        // Pause sends E1 1D 45 E1 9D C5, NOT a character or Ctrl.
        if code==0xe1{
            self.extended=false;
            self.pause_remaining=5;
            return None;
        }
        if code==0xe0{
            self.extended=true;
            return None;
        }
        let released=code&0x80!=0;
        let key=code&0x7f;
        if self.extended{
            self.extended=false;
            match key{
                0x1d=>{self.ctrl_right=!released;return None;},
                0x38=>{self.alt_right=!released;return None;},
                // Extended cursor keys are the existing bounded shell
                // history codes, not printable input or ANSI escapes.
                0x48 if !released&&!self.modifiers_active()=>return Some(0x10),
                0x50 if !released&&!self.modifiers_active()=>return Some(0x0e),
                // Keypad Enter and Slash remain usable in the native shell.
                0x1c if !released&&!self.modifiers_active()=>return Some(b'\n'),
                0x35 if !released&&!self.modifiers_active()=>return Some(b'/'),
                _=>return None,
            }
        }
        match key{
            0x2a=>{self.shift_left=!released;return None;},
            0x36=>{self.shift_right=!released;return None;},
            0x1d=>{self.ctrl_left=!released;return None;},
            0x38=>{self.alt_left=!released;return None;},
            0x3a=>{
                if !released&&!self.caps_down{self.caps=!self.caps;}
                self.caps_down=!released;
                return None;
            },
            _=>{},
        }
        if released||self.modifiers_active(){return None;}
        let shift=self.shift_left||self.shift_right;
        let mut byte=if shift{SHIFTED[key as usize]}else{KEYMAP[key as usize]};
        // Shift and CapsLock combine via XOR only for letters.
        if byte.is_ascii_alphabetic() && (shift!=self.caps){
            byte=byte.to_ascii_uppercase();
        }
        filter(byte)
    }
    fn modifiers_active(&self)->bool{
        self.ctrl_left||self.ctrl_right||self.alt_left||self.alt_right
    }
}
pub(crate) fn filter(b:u8)->Option<u8>{
    match b {
        b'\r'|b'\n'=>Some(b'\n'),
        b'\t'=>Some(b'\t'), // Bounded Terminal editing/completion control.
        8|127=>Some(8),
        0x10|0x0e=>Some(b), // R015 history navigation
        32..=126=>Some(b),
        _=>None,
    }
}
static mut SCAN:ScanDecoder=ScanDecoder{
    shift_left:false,shift_right:false,caps:false,caps_down:false,extended:false,
    ctrl_left:false,ctrl_right:false,alt_left:false,alt_right:false,
    pause_remaining:0
};
/// Counts physical bytes AFTER a valid 8042 keyboard make-code has been
/// consumed. Serial/UART input does not affect this counter.
static PS2_KEYS_ACCEPTED:AtomicU64=AtomicU64::new(0);
pub(crate) fn physical_keys_accepted()->u64{
    PS2_KEYS_ACCEPTED.load(Ordering::Relaxed)
}
#[cfg(not(test))]
fn mark_physical_key(byte:u8)->u8{
    if PS2_KEYS_ACCEPTED.fetch_add(1,Ordering::Relaxed)==0{
        unsafe extern "C" {fn serial_write(message:*const core::ffi::c_char);}
        unsafe{
            serial_write(b"IndianOS: R047 genuine PS2 keyboard byte accepted by kernel input\n\0"
                .as_ptr().cast());
        }
    }
    byte
}
#[cfg(test)]
fn mark_physical_key(byte:u8)->u8{
    PS2_KEYS_ACCEPTED.fetch_add(1,Ordering::Relaxed);
    byte
}
/// Read only error-free set-1 bytes from the primary PS/2 controller port.
fn valid_keyboard_status(status:u8)->bool{
    status&0x01!=0 && status&0xe0==0 // OBF, !AUX, !timeout, !parity
}
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
// Reserve one supervisor-only virtual page for local-APIC MMIO. Limine's
// HHDM need not map reserved device memory (0xFEE00000), so directly
// dereferencing APIC_BASE + HHDM can fault. Page-table frames themselves
// are ordinary PMM RAM and are reachable through the HHDM.
#[cfg(feature="test-irq-keyboard")]
unsafe fn map_local_apic(physical:u64)->Option<*mut u32>{
    const MASK:u64=0x000f_ffff_ffff_f000;
    const VA:u64=0xffff_fe00_0000_0000;
    const PRESENT_WRITE:u64=0x003;
    unsafe extern "C" {fn pmm_allocate_frame(out:*mut u64)->bool;}
    if physical&0xfff!=0{return None;}
    let mut offset=0u64;
    if !unsafe{crate::boot::boot_hhdm_offset(&mut offset)}{return None;}
    let cr3:u64;
    unsafe{asm!("mov {}, cr3",out(reg)cr3,options(nostack,nomem,preserves_flags));}
    let mut table=cr3&MASK;
    // Walk PML4, PDPT, PD, creating only missing intermediate tables.
    // VA lies in the kernel half and is inherited by user CR3 clones.
    for shift in [39u64,30,21]{
        let idx=((VA>>shift)&511) as usize;
        let entry=unsafe{((table+offset) as *mut u64).add(idx)};
        let value=unsafe{core::ptr::read_volatile(entry)};
        if value&1!=0{
            if value&(1<<7)!=0{return None;} // huge page: cannot descend
            table=value&MASK;
        }else{
            let mut frame=0u64;
            if !unsafe{pmm_allocate_frame(&mut frame)}||frame==0||frame&0xfff!=0{return None;}
            let page=frame.checked_add(offset)? as *mut u64;
            for i in 0..512{unsafe{core::ptr::write_volatile(page.add(i),0);}}
            unsafe{core::ptr::write_volatile(entry,frame|PRESENT_WRITE);}
            table=frame;
        }
    }
    let pte=unsafe{((table+offset) as *mut u64).add(((VA>>12)&511) as usize)};
    let old=unsafe{core::ptr::read_volatile(pte)};
    if old&1!=0 && old&MASK!=physical{return None;}
    // PCD + PWT for uncached device access; never user-accessible.
    unsafe{
        core::ptr::write_volatile(pte,physical|PRESENT_WRITE|0x18);
        asm!("invlpg [{}]",in(reg)VA,options(nostack,preserves_flags));
    }
    Some(VA as *mut u32)
}
/// Legacy PIC/PS2 initialization is opt-in, uniprocessor and must occur
/// after IDT 0x21 is ready, with IF masked. Other PIC IRQs stay masked.
#[cfg(feature="test-irq-keyboard")]
pub(crate) fn enable_irq1()->bool{
    // The PIC-only QEMU CPU profile has no IA32_APIC_BASE MSR; probing
    // the APIC CPUID bit avoids #GP from unconditional RDMSR.
    if unsafe{core::arch::x86_64::__cpuid(1)}.edx&(1<<9)!=0{
        let mut apic_lo:u32;
        let apic_hi:u32;
        unsafe{asm!("rdmsr",in("ecx")0x1bu32,
            out("eax")apic_lo,out("edx")apic_hi,options(nostack,preserves_flags));}
        if apic_lo&(1<<10)!=0{return false;} // x2APIC requires separate routing
        // OVMF leaves the local APIC active. The 8259 output is wired
        // to LINT0 (ExtINT), not directly to CPU INTR. Disabling the
        // APIC breaks firmware on some QEMU CPU profiles; configure the
        // architectural virtual-wire route instead.
        if apic_lo&(1<<11)!=0{
            let base=((apic_hi as u64)<<32)|((apic_lo as u64)&0xffff_f000);
            let Some(apic)=(unsafe{map_local_apic(base)})else{return false;};
            unsafe{
                let svr=core::ptr::read_volatile(apic.add(0xf0/4));
                core::ptr::write_volatile(apic.add(0xf0/4),svr|0x100);
                // LINT0 ExtINT, physical destination, unmasked.
                core::ptr::write_volatile(apic.add(0x350/4),0x700);
            }
        }
    }
    unsafe{
        outb(0x20,0x11);io_wait();outb(0xa0,0x11);io_wait();
        outb(0x21,0x20);io_wait();outb(0xa1,0x28);io_wait();
        outb(0x21,0x04);io_wait();outb(0xa1,0x02);io_wait();
        outb(0x21,0x01);io_wait();outb(0xa1,0x01);io_wait();
        outb(0x21,0xfd);outb(0xa1,0xff);
        // Complete any stale in-service state carried across firmware boot.
        // During single-core setup IF is disabled; no handler is active.
        for _ in 0..8{outb(0x20,0x20);io_wait();}
        outb(0x22,0x70);io_wait();outb(0x23,0);io_wait();
    }
    // Firmware may leave an unread scan code. If a stale byte holds OBF
    // high, later QEMU sendkey events cannot assert a new IRQ1 edge.
    // Disable port 1 before reading the controller configuration.
    if !wait_input(){return false;}
    unsafe{outb(PS2_STATUS,0xad);}
    for _ in 0..32 {
        if unsafe{inb(PS2_STATUS)}&1==0{break;}
        let _=unsafe{inb(PS2_DATA)};
    }
    if !wait_input(){return false;}
    unsafe{outb(PS2_STATUS,0x20);}
    let Some(conf)=wait_output()else{return false;};
    if !wait_input(){return false;}
    unsafe{outb(PS2_STATUS,0x60);}
    if !wait_input(){return false;}
    // IRQ1 on, translation to set-1 on, PS/2 port 1 clock enabled.
    unsafe{outb(PS2_DATA,(conf|0x41)&!0x10);}
    if !wait_input(){return false;}
    unsafe{outb(PS2_STATUS,0xae);}
    if !wait_input(){return false;}
    // Drop any controller responses from firmware/programming.
    for _ in 0..32{
        if unsafe{inb(PS2_STATUS)}&1==0{break;}
        let _=unsafe{inb(PS2_DATA)};
    }
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
    if status&1!=0&&status&0x20==0 {
        let code=unsafe{inb(PS2_DATA)};
        if let Some(ch)=unsafe{(&mut *core::ptr::addr_of_mut!(SCAN)).feed_transfer(status,code)}{
            enqueue(ch);
        }
    }
    unsafe{outb(0x20,0x20);} // specific master PIC EOI
}
/// Production bootstrap UART ingress preserves every byte. Error transfers
/// become a nonprinting poison byte so corruption cannot splice a command.
#[cfg(feature="normal-init")]
pub(crate) fn normal_boot_byte()->Option<u8>{
    let status=unsafe{inb(COM1+5)};
    if status&1==0{return None;}
    let byte=unsafe{inb(COM1)};
    Some(normal_uart_transfer(status,byte))
}
#[cfg(feature="normal-init")]
fn normal_uart_transfer(status:u8,byte:u8)->u8{
    if status&0x1e!=0{0}else{byte}
}
pub(crate) fn poll_byte()->Option<u8>{
    #[cfg(feature="test-irq-keyboard")]
    if let Some(byte)=dequeue(){return Some(mark_physical_key(byte));}

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
                let mask=inb(0x21);
                outb(0x20,0x0a);let irr=inb(0x20);
                outb(0x20,0x0b);let isr=inb(0x20);
                if mask&0x02!=0{
                    serial_write(b"IndianOS: R014.2 PIC IRQ1 masked\n\0".as_ptr().cast());
                }else{
                    serial_write(b"IndianOS: R014.2 PIC IRQ1 unmasked\n\0".as_ptr().cast());
                }
                if irr&0x02!=0{
                    serial_write(b"IndianOS: R014.2 PIC IRR IRQ1 pending\n\0".as_ptr().cast());
                }else{
                    serial_write(b"IndianOS: R014.2 PIC IRR IRQ1 absent\n\0".as_ptr().cast());
                }
                if isr&0x02!=0{
                    serial_write(b"IndianOS: R014.2 PIC ISR IRQ1 active\n\0".as_ptr().cast());
                }
                if isr&0x01!=0{
                    serial_write(b"IndianOS: R014.2 PIC ISR IRQ0 blocking\n\0".as_ptr().cast());
                }else{
                    serial_write(b"IndianOS: R014.2 PIC ISR IRQ0 clear\n\0".as_ptr().cast());
                }
            }
        }
        return None; // only IRQ1 may drain the keyboard data register
    }
    let status=unsafe{inb(PS2_STATUS)};
    // Avoid accidentally consuming auxiliary PS/2 mouse packets.
    // Do not drain AUX (mouse) packets: the GUI PS/2 mouse broker owns
    // those bytes and must be able to consume a complete three-byte frame.
    if status&1!=0&&status&0x20==0{
        let code=unsafe{inb(PS2_DATA)};
        return unsafe{(&mut *core::ptr::addr_of_mut!(SCAN)).feed_transfer(status,code)}
            .map(mark_physical_key);
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
    fn m1_r047_ctrl_and_alt_cannot_inject_unmodified_desktop_hotkeys(){
        let mut d=ScanDecoder::default();
        for (press,release) in [(0x1d,0x9d),(0x38,0xb8)]{
            assert_eq!(d.feed(press),None);
            for key in [0x1e,0x02,0x23,0x10,0x32,0x1c]{
                assert_eq!(d.feed(key),None,"hotkey {key:#x} must be blocked");
            }
            assert_eq!(d.feed(release),None);
            assert_eq!(d.feed(0x1e),Some(b'a'));
        }
    }
    #[test]
    fn m1_r047_extended_ctrl_alt_and_release_are_not_characters(){
        let mut d=ScanDecoder::default();
        for (down,up) in [(0x1d,0x9d),(0x38,0xb8)]{
            assert_eq!(d.feed(0xe0),None);assert_eq!(d.feed(down),None);
            assert_eq!(d.feed(0x30),None);
            assert_eq!(d.feed(0xe0),None);assert_eq!(d.feed(0x48),None);
            assert_eq!(d.feed(0xe0),None);assert_eq!(d.feed(up),None);
            assert_eq!(d.feed(0x30),Some(b'b'));
        }
    }
    #[test]
    fn m1_r047_pause_does_not_stick_ctrl_or_type_bogus_characters(){
        let mut d=ScanDecoder::default();
        for byte in [0xe1,0x1d,0x45,0xe1,0x9d,0xc5]{
            assert_eq!(d.feed(byte),None);
        }
        assert_eq!(d.feed(0x1e),Some(b'a'));
    }
    #[test]
    fn m1_r047_ps2_aux_parity_and_timeout_status_are_rejected(){
        for status in [0x00,0x20,0x21,0x41,0x81,0xc1,0xff]{
            assert!(!valid_keyboard_status(status));
        }
        for status in [0x01,0x03,0x11]{
            assert!(valid_keyboard_status(status));
        }
    }
    #[test]
    fn m1_r047_extended_keypad_enter_slash_and_break_discipline(){
        let mut d=ScanDecoder::default();
        for (scan,ch) in [(0x1c,b'\n'),(0x35,b'/'),(0x48,0x10),(0x50,0x0e)]{
            assert_eq!(d.feed(0xe0),None);
            assert_eq!(d.feed(scan),Some(ch));
            assert_eq!(d.feed(0xe0),None);
            assert_eq!(d.feed(scan|0x80),None);
        }
        assert_eq!(d.feed(0x1c),Some(b'\n'));
    }
    #[test]
    fn m1_r047_shift_caps_and_releases_are_balanced(){
        let mut d=ScanDecoder::default();
        assert_eq!(d.feed(0x2a),None);
        assert_eq!(d.feed(0x1e),Some(b'A'));
        assert_eq!(d.feed(0x36),None);
        assert_eq!(d.feed(0xaa),None);
        assert_eq!(d.feed(0x1e),Some(b'A'));
        assert_eq!(d.feed(0xb6),None);
        assert_eq!(d.feed(0x3a),None);
        assert_eq!(d.feed(0x1e),Some(b'A'));
        assert_eq!(d.feed(0xba),None);
        assert_eq!(d.feed(0x3a),None);
        assert_eq!(d.feed(0x1e),Some(b'a'));
        assert_eq!(d.feed(0x9e),None);
    }
    #[test]
    fn m1_r047_only_genuine_accepted_scan_codes_increment_physical_count(){
        let before=physical_keys_accepted();
        let mut d=ScanDecoder::default();
        for byte in [0x1d,0x1e,0x9e,0x9d,0xe0,0x48,0xe0,0xc8]{
            if let Some(ch)=d.feed(byte){assert_eq!(mark_physical_key(ch),ch);}
        }
        for status in [0x41,0x81,0x21]{
            if let Some(ch)=d.feed_transfer(status,0x1e){mark_physical_key(ch);}
        }
        assert_eq!(physical_keys_accepted(),before+1);
    }
    #[test]
    fn caps_typematic_toggles_once_per_press(){
        let mut d=ScanDecoder::default();
        for expected in [b'A',b'a']{
            for _ in 0..4{
                assert_eq!(d.feed_transfer(0x01,0x3a),None);
                assert_eq!(d.feed_transfer(0x01,0x1e),Some(expected));
            }
            assert_eq!(d.feed_transfer(0x01,0xba),None);
        }
    }
    #[test]
    fn corrupt_primary_transfers_release_stale_modifiers(){
        for status in [0x41,0x81,0xc1]{
            for prefix in [false,true]{
                for scan in [0x2a,0x36,0x1d,0x38]{
                    if prefix && (scan==0x2a||scan==0x36){continue;}
                    let mut d=ScanDecoder::default();
                    if prefix{assert_eq!(d.feed_transfer(1,0xe0),None);}
                    assert_eq!(d.feed_transfer(1,scan),None);
                    assert_eq!(d.feed_transfer(status,scan|0x80),None);
                    assert_eq!(d.feed_transfer(1,0x1e),Some(b'a'));
                    // Fresh Ctrl still blocks ordinary desktop shortcuts.
                    assert_eq!(d.feed_transfer(1,0x1d),None);
                    assert_eq!(d.feed_transfer(1,0x1e),None);
                }
            }
        }
    }
    #[test]
    fn corrupt_transfer_clears_prefix_pause_and_preserves_lock(){
        for status in [0x41,0x81]{
            for prefix in [0xe0,0xe1]{
                let mut d=ScanDecoder::default();
                assert_eq!(d.feed_transfer(1,0x3a),None);
                assert_eq!(d.feed_transfer(1,prefix),None);
                // Even a printable corrupt transfer cannot manufacture text.
                assert_eq!(d.feed_transfer(status,0x1e),None);
                assert_eq!(d.feed_transfer(1,0x1e),Some(b'A'));
                assert_eq!(d.feed_transfer(1,0xe0),None);
                assert_eq!(d.feed_transfer(1,0x48),Some(0x10));
            }
        }
    }
    #[test]
    fn aux_and_empty_status_do_not_change_keyboard_state(){
        let mut d=ScanDecoder::default();
        assert_eq!(d.feed_transfer(1,0xe0),None);
        for status in [0,0x40,0x80,0x21,0x61,0xa1,0xe1]{
            assert_eq!(d.feed_transfer(status,0x1e),None);
        }
        assert_eq!(d.feed_transfer(1,0x48),Some(0x10));
        assert_eq!(d.feed_transfer(1,0x2a),None);
        assert_eq!(d.feed_transfer(0xe1,0xaa),None);
        assert_eq!(d.feed_transfer(1,0x1e),Some(b'A'));
    }
    #[test]
    fn terminal_input_filters_controls(){
        assert_eq!(filter(b'\t'),Some(b'\t'));
        assert_eq!(filter(b'\r'),Some(b'\n'));
        assert_eq!(filter(127),Some(8));
        assert_eq!(filter(0x1b),None);
        assert_eq!(filter(0),None);
        assert_eq!(filter(b'/'),Some(b'/'));
        let mut keys=ScanDecoder::default();
        assert_eq!(keys.feed(0xe0),None);
        assert_eq!(keys.feed(0x48),Some(0x10));
        assert_eq!(keys.feed(0xe0),None);
        assert_eq!(keys.feed(0x50),Some(0x0e));
        assert_eq!(keys.feed(0x1c),Some(b'\n'));
        assert_eq!(keys.feed(0x0e),Some(8));
    }
    #[test]
    fn physical_tab_reaches_editor_without_leaking_modified_shortcuts(){
        let mut decoder=ScanDecoder::default();
        assert_eq!(decoder.feed_transfer(1,0x0f),Some(b'\t'));
        assert_eq!(decoder.feed_transfer(1,0x8f),None);
        assert_eq!(decoder.feed_transfer(1,0x1d),None);
        assert_eq!(decoder.feed_transfer(1,0x0f),None);
        assert_eq!(decoder.feed_transfer(1,0x9d),None);
        assert_eq!(decoder.feed_transfer(0x21,0x0f),None);
        assert_eq!(decoder.feed_transfer(0xc1,0x0f),None);
        assert_eq!(decoder.feed_transfer(1,0x0f),Some(b'\t'));
    }
}

#[cfg(all(test,feature="normal-init"))]
mod normal_uart_tests{
    use super::normal_uart_transfer;
    #[test]
    fn raw_uart_preserves_controls_and_poisons_line_errors(){
        for byte in 0..=255u8{
            assert_eq!(normal_uart_transfer(1,byte),byte);
            for error in [2,4,8,16]{assert_eq!(normal_uart_transfer(1|error,byte),0);}
        }
    }
}
