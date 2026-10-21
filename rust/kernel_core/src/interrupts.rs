//! Native Rust GDT, IDT, TSS and ring-3 exception diagnostics.
//!
//! x86_64 entry stubs and the 64-bit long-mode GDT reload remain minimal
//! hand-written assembly; everything else is Rust-owned. Single CPU only.
//! The active IDT is used only after all entries have been initialized.

use core::{arch::asm, ffi::c_char, mem::{size_of}, ptr};

const TRAP_COUNT:usize=5;
const USER_STACK_COUNT:usize=2;
const STACK_BYTES:usize=32768;
const DF_STACK_BYTES:usize=16384;
const CODE:u16=0x08;
const TSS_SEL:u16=0x18;
const INTERRUPT_GATE:u8=0x8e;
const USER_GATE:u8=0xee;
#[repr(C, packed)]
struct DescriptorPtr { limit:u16, address:u64 }
#[repr(C, packed)]
#[derive(Clone,Copy)]
struct IdtGate {
    low:u16, selector:u16, ist:u8, attributes:u8,
    mid:u16, high:u32, reserved:u32,
}
impl IdtGate {
    const fn zero()->Self {
        Self{low:0,selector:0,ist:0,attributes:0,mid:0,high:0,reserved:0}
    }
    fn new(handler:usize,ist:u8,attributes:u8)->Self {
        let addr=handler as u64;
        Self{
            low:addr as u16,selector:CODE,ist,attributes,
            mid:(addr>>16) as u16,high:(addr>>32) as u32,reserved:0,
        }
    }
}
#[repr(C,packed)]
struct Tss {
    reserved0:u32,
    rsp0:u64,rsp1:u64,rsp2:u64,
    reserved1:u64,
    ist1:u64,ist2:u64,ist3:u64,ist4:u64,
    ist5:u64,ist6:u64,ist7:u64,
    reserved2:u64,
    reserved3:u16,iomap_base:u16,
}
impl Tss {
    const fn empty()->Self {
        Self {
            reserved0:0,rsp0:0,rsp1:0,rsp2:0,reserved1:0,
            ist1:0,ist2:0,ist3:0,ist4:0,ist5:0,ist6:0,ist7:0,
            reserved2:0,reserved3:0,iomap_base:0,
        }
    }
}
#[repr(C,align(16))]
struct AlignedGdt([u64;7]);
#[repr(C,align(16))]
struct AlignedIdt([IdtGate;256]);
#[repr(C,align(16))]
struct AlignedTss(Tss);
#[repr(C,align(16))]
struct AlignedStack<const SIZE:usize>([u8;SIZE]);
static mut GDT:AlignedGdt=AlignedGdt([
    0,0x00af9a000000ffff,0x00cf92000000ffff,0,0,
    0x00affa000000ffff,0x00cff2000000ffff
]);
static mut IDT:AlignedIdt=AlignedIdt([IdtGate::zero();256]);
static mut TSS:AlignedTss=AlignedTss(Tss::empty());
static mut DF_STACK:AlignedStack<DF_STACK_BYTES>=AlignedStack([0;DF_STACK_BYTES]);
static mut USER_STACKS:[AlignedStack<STACK_BYTES>;USER_STACK_COUNT]=[
    AlignedStack([0;STACK_BYTES]),AlignedStack([0;STACK_BYTES])
];

unsafe extern "C" {
    fn x86_64_load_gdt(descriptor:*const DescriptorPtr);
    static x86_64_exception_stubs:[unsafe extern "C" fn();32];
    fn x86_64_unexpected_interrupt();
    fn x86_64_user_interrupt();
    fn x86_64_timer_interrupt();
    fn x86_64_user_fault_handle(vector:u64,cs:u64,error:u64,cr2:u64);
    fn serial_write(message:*const c_char);
}
fn log(message:&'static [u8]){unsafe{serial_write(message.as_ptr().cast())}}
unsafe fn install_gate(vector:usize,handler:usize,ist:u8,attr:u8){
    // SAFETY: setup is single-threaded before IDTR is loaded; no active ISR.
    unsafe{ptr::write_volatile(
        ptr::addr_of_mut!(IDT.0).cast::<IdtGate>().add(vector),
        IdtGate::new(handler,ist,attr));
    }
}
fn tss_descriptor(base:u64,limit:u64)->u64{
    (limit&0xffff) | ((base&0xffffff)<<16) |
    (0x89u64<<40) | (((limit>>16)&15)<<48) |
    (((base>>24)&0xff)<<56)
}
/// # Safety
/// Called once on the BSP, with interrupts disabled, before scheduling
/// user-mode code. Cannot be used for live IDT replacement or SMP.
#[no_mangle]
pub unsafe extern "C" fn x86_64_exceptions_init()->bool{
    let tss=ptr::addr_of_mut!(TSS.0);
    let df_top=(ptr::addr_of!(DF_STACK.0) as usize)+DF_STACK_BYTES;
    unsafe{
        ptr::write_unaligned(ptr::addr_of_mut!((*tss).ist1),df_top as u64);
        ptr::write_unaligned(ptr::addr_of_mut!((*tss).iomap_base),size_of::<Tss>() as u16);
        let base=tss as usize as u64;
        ptr::write_volatile(ptr::addr_of_mut!(GDT.0).cast::<u64>().add(3),
                            tss_descriptor(base,size_of::<Tss>() as u64-1));
        ptr::write_volatile(ptr::addr_of_mut!(GDT.0).cast::<u64>().add(4),base>>32);
        let gdtr=DescriptorPtr{
            limit:size_of::<AlignedGdt>() as u16-1,
            address:ptr::addr_of!(GDT.0) as usize as u64,
        };
        x86_64_load_gdt(&gdtr);
        let sel:u16=TSS_SEL;
        asm!("ltr {selector:x}",selector=in(reg)sel,options(nostack,preserves_flags));
        for i in 0..256{
            install_gate(i,x86_64_unexpected_interrupt as usize,0,INTERRUPT_GATE);
        }
        for i in 0..32{
            let handler=ptr::read(ptr::addr_of!(x86_64_exception_stubs).cast::<usize>().add(i));
            install_gate(i,handler,if i==8 {1} else {0},INTERRUPT_GATE);
        }
        install_gate(0x80,x86_64_user_interrupt as usize,0,USER_GATE);
        #[cfg(feature="preempt")]
        install_gate(0x20,x86_64_timer_interrupt as usize,0,INTERRUPT_GATE);
        let idtr=DescriptorPtr{
            limit:(size_of::<IdtGate>()*256-1) as u16,
            address:ptr::addr_of!(IDT.0) as usize as u64
        };
        asm!("lidt [{pointer}]", pointer=in(reg)&idtr,
             options(readonly,nostack,preserves_flags));
    }
    true
}
/// # Safety
/// Slot select must execute on CPU 0, with interrupts masked, before
/// IRETQ enters the corresponding user process.
#[no_mangle]
pub unsafe extern "C" fn x86_64_user_interrupt_stack_select(slot:u32)->bool{
    if slot as usize>=USER_STACK_COUNT {return false}
    let tss=ptr::addr_of_mut!(TSS.0);
    let ist=unsafe{ptr::read_unaligned(ptr::addr_of!((*tss).ist1))};
    if ist==0{return false}
    let bottom=ptr::addr_of!(USER_STACKS).cast::<AlignedStack<STACK_BYTES>>();
    let top=unsafe{bottom.add(slot as usize)} as usize+size_of::<AlignedStack<STACK_BYTES>>();
    unsafe{ptr::write_unaligned(ptr::addr_of_mut!((*tss).rsp0),top as u64);}
    top!=0
}
#[no_mangle]
pub unsafe extern "C" fn x86_64_user_interrupt_stack_enable()->bool{
    unsafe{x86_64_user_interrupt_stack_select(0)}
}
/// # Safety
/// \`result\` points to 5 writable u64s and will be used only after IDTR
/// initialization and selecting an active kernel stack.
#[no_mangle]
pub unsafe extern "C" fn x86_64_user_trap_mapping_addresses(result:*mut u64)->bool{
    if result.is_null(){return false}
    let tss=ptr::addr_of!(TSS.0);
    let ist=unsafe{ptr::read_unaligned(ptr::addr_of!((*tss).ist1))};
    let rsp=unsafe{ptr::read_unaligned(ptr::addr_of!((*tss).rsp0))};
    if ist==0 || rsp==0 {return false}
    let values=[
        ptr::addr_of!(GDT.0) as usize as u64,
        ptr::addr_of!(IDT.0) as usize as u64,
        tss as usize as u64,rsp-8,ist-8
    ];
    for (i,value) in values.iter().enumerate(){
        unsafe{ptr::write(result.add(i),*value);}
    }
    true
}
#[repr(C)]
pub struct ExceptionContext{
    vector:u64,error_code:u64,rip:u64,cs:u64,rflags:u64
}
fn hex(value:u64){
    let mut chars=[0u8;19];chars[0]=b'0';chars[1]=b'x';
    const DIGITS:&[u8;16]=b"0123456789abcdef";
    for i in 0..16{chars[2+i]=DIGITS[((value>>(60-4*i))&15) as usize];}
    chars[18]=0;
    unsafe{serial_write(chars.as_ptr().cast())}
}
#[inline(never)]
fn halt()->!{loop{unsafe{asm!("cli; hlt",options(nostack,nomem))}}}
/// # Safety
/// Entry assembly provides a readable CPU exception frame and only calls
/// this function from the trusted kernel exception stub (never from user).
#[no_mangle]
pub unsafe extern "C" fn x86_64_exception_dispatch(context:*const ExceptionContext)->! {
    let Some(ctx)=(unsafe{context.as_ref()}) else {halt()};
    let cr2=if ctx.vector==14 {
        let value:u64;unsafe{asm!("mov {}, cr2",out(reg)value,options(nomem,nostack))};value
    } else {0};
    unsafe{x86_64_user_fault_handle(ctx.vector,ctx.cs,ctx.error_code,cr2);}
    log(b"IndianOS: FATAL CPU exception vector=\0");hex(ctx.vector);
    log(b" error=\0");hex(ctx.error_code);
    log(b" rip=\0");hex(ctx.rip);log(b"\n\0");
    if ctx.vector==14{
        log(b"IndianOS: page fault cr2=\0");hex(cr2);
        log(b" present=\0");log(if ctx.error_code&1!=0 {b"1\0"}else{b"0\0"});
        log(b" write=\0");log(if ctx.error_code&2!=0 {b"1\0"}else{b"0\0"});
        log(b" user=\0");log(if ctx.error_code&4!=0 {b"1\0"}else{b"0\0"});
        log(b" reserved=\0");log(if ctx.error_code&8!=0 {b"1\0"}else{b"0\0"});
        log(b" instruction=\0");log(if ctx.error_code&16!=0 {b"1\0"}else{b"0\0"});
        log(b"\n\0");
    }
    halt()
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn cpu_layout_is_compatible(){
        assert_eq!(size_of::<Tss>(),104);
        assert_eq!(size_of::<IdtGate>(),16);
        assert_eq!(size_of::<DescriptorPtr>(),10);
        assert_eq!(TRAP_COUNT,5);
    }
    #[test]
    fn gdt_tss_flags(){
        assert_eq!((tss_descriptor(0x123456789000,103)>>40)&0xff,0x89);
        assert_eq!(IdtGate::new(0x1234,1,0x8e).low,0x1234);
    }
}
