//! Native freestanding Rust IndianOS user applications.
//!
//! These do NOT call Linux or any host system APIs. The user/kernel ABI is
//! RAX=0 query, 1 write, 2 exit, 3 voluntary yield, RDI/RSI arguments and
//! software interrupt INT 0x80. The image is x86-64 ET_EXEC at 0x400000.
//! All variants remain QEMU-only tests until the kernel is hardened.
#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;
#[cfg(any(feature="shell",feature="interactive"))]
mod shell;

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! { exit(125) }
#[inline(always)]
fn call(number:u64,arg0:u64,arg1:u64)->u64 {
    let result:u64;
    unsafe {
        asm!("int 0x80",
             inlateout("rax") number=>result,
             in("rdi") arg0, in("rsi") arg1,
             options(nostack));
    }
    result
}
fn write(data:&'static [u8]){
    if call(1,data.as_ptr() as u64,data.len() as u64)!=data.len() as u64 {
        exit(1)
    }
}
fn yield_now(){
    if call(3,0,0)!=0 {exit(2)}
}
fn exit(status:u64)->! {
    let _=call(2,status,0);
    loop{unsafe{asm!("pause", options(nomem,nostack,preserves_flags))}}
}
#[inline(never)]
fn spin_cpu_bound(){
    // Not a Rust loop: rustc must not optimize away a test that relies on
    // actual interrupted CPU work. Same work count as prior assembly app.
    let mut counter:u64=400_000_000;
    unsafe{
        asm!(
            "2:", "dec {counter}", "jnz 2b",
            counter=inout(reg) counter,
            options(nostack,nomem)
        );
    }
    let _=counter;
}


#[cfg(feature="interactive")]
fn read_line(buffer:&mut [u8;64])->usize{
    let mut used=0usize;
    loop{
        let mut byte=[0u8;1];
        let count=call(6,byte.as_mut_ptr() as u64,1);
        if count==0 {core::hint::spin_loop();continue;}
        if count!=1 {exit(40);}
        match byte[0] {
            b'\n'=>return used,
            8=>{used=used.saturating_sub(1);},
            32..=126=> {
                if used<buffer.len(){buffer[used]=byte[0];used+=1;}
            }
            _=>{}
        }
    }
}
#[cfg(feature="interactive")]
fn interactive_shell()->!{
    write(b"R014 interactive shell ready");
    loop {
        write(b"indianos> ");
        let mut line=[0u8;64];
        let count=read_line(&mut line);
        match shell::parse(&line[..count]){
            Some(shell::Command::Help)=>
                write(b"R014 shell: commands help ps run /bin/worker exit"),
            Some(shell::Command::Ps)=>{
                if call(4,0,0)!=1 {exit(41);}
                write(b"R014 shell: ps PID 1 RUNNING");
            },
            Some(shell::Command::RunWorker)=>{
                if call(5,1,0)!=0 {
                    write(b"R014 shell: worker request rejected");
                }else{
                    write(b"R014 shell: worker queued for after exit");
                }
            },
            Some(shell::Command::Exit)=>{
                write(b"R014 shell: exit");
                exit(0);
            },
            None=>write(b"R014 shell: unknown command"),
        }
    }
}

#[no_mangle]
pub extern "C" fn _start()->!{
    #[cfg(feature="demo")]
    {
        write(b"Namaste from IndianOS user mode!");
        exit(0);
    }
    #[cfg(feature="interactive")]
    interactive_shell();
    #[cfg(feature="shell")]
    {
        write(b"R013 shell: IndianOS init started");
        for &line in shell::BOOT_SCRIPT {
            match shell::parse(line) {
                Some(shell::Command::Help) =>
                    write(b"R013 shell: commands: help ps run /bin/worker exit"),
                Some(shell::Command::Ps) => {
                    if call(4,0,0)!=1 {exit(30);}
                    write(b"R013 shell: ps PID 1 RUNNING");
                }
                Some(shell::Command::RunWorker) => {
                    if call(5,1,0)!=0 {exit(31);}
                    write(b"R013 shell: run /bin/worker accepted");
                }
                Some(shell::Command::Exit) => {
                    write(b"R013 shell: exit");
                    exit(0);
                }
                None => exit(32),
            }
        }
        exit(33);
    }
    #[cfg(feature="worker")]
    {
        write(b"IndianOS worker process 2 completed");
        exit(0);
    }
    #[cfg(feature="sched-a")]
    {
        write(b"M009 task A step 1");
        yield_now();
        write(b"M009 task A step 2");
        yield_now();
        write(b"M009 task A step 3");
        exit(0);
    }
    #[cfg(feature="sched-b")]
    {
        write(b"M009 task B step 1");
        yield_now();
        write(b"M009 task B step 2");
        yield_now();
        write(b"M009 task B step 3");
        exit(0);
    }
    #[cfg(feature="fault-guard")]
    {
        // The first stack guard page must not be readable at CPL3.
        unsafe { asm!("mov rax, qword ptr [rdi]",
            in("rdi") 0x0000_7fff_effe_f000u64,
            out("rax") _, options(nostack,readonly)); }
        exit(99);
    }
    #[cfg(feature="fault-supervisor")]
    {
        unsafe { asm!("mov rax, qword ptr [rdi]",
            in("rdi") 0xffff_ffff_8000_0000u64,
            out("rax") _, options(nostack,readonly)); }
        exit(99);
    }
    #[cfg(feature="preempt-a")]
    {
        write(b"M009 timer task A started");
        spin_cpu_bound();
        write(b"M009 timer task A finished");
        exit(0);
    }
    #[cfg(feature="preempt-b")]
    {
        write(b"M009 timer task B started");
        spin_cpu_bound();
        write(b"M009 timer task B finished");
        exit(0);
    }
    // A harmless, valid ET_EXEC fixture for regular kernel boots.
    // It must never be launched outside the opt-in test variants.
    #[cfg(not(any(
        feature="demo",feature="worker",feature="shell",feature="interactive",feature="sched-a",
        feature="sched-b",feature="fault-guard",feature="fault-supervisor",
        feature="preempt-a",feature="preempt-b"
    )))]
    loop {unsafe{asm!("pause",options(nomem,nostack,preserves_flags))}}
}
