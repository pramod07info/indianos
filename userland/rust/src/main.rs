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
#[cfg(any(feature="shell",feature="interactive",feature="r026-shell"))]
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


#[cfg(any(feature="ipc-a",feature="ipc-b"))]
fn ipc_send(receiver:u32, data:&[u8]){
    if data.len()>64 {exit(60);}
    let mut packet=[0u8;68];
    packet[..4].copy_from_slice(&receiver.to_le_bytes());
    packet[4..4+data.len()].copy_from_slice(data);
    if call(8,packet.as_ptr() as u64,(4+data.len()) as u64)!=0 {exit(61);}
}
#[cfg(any(feature="ipc-a",feature="ipc-b",feature="r025-a",feature="r025-b"))]
fn ipc_receive(expected_sender:u32,expected_data:&[u8]){
    let mut packet=[0xa5u8;72];
    let n=call(9,packet.as_mut_ptr() as u64,packet.len() as u64);
    if n!=(8+expected_data.len()) as u64 {exit(62);}
    if u32::from_le_bytes(packet[..4].try_into().unwrap())!=expected_sender ||
       u32::from_le_bytes(packet[4..8].try_into().unwrap())!=expected_data.len() as u32 ||
       &packet[8..8+expected_data.len()]!=expected_data ||
       packet[8+expected_data.len()..].iter().any(|&b|b!=0) {exit(63);}
}

// R025 full integration: users obtain only their own kernel-issued tokens.
// No application can mint a token or choose its authenticated sender identity.
#[cfg(any(feature="r025-a",feature="r025-b"))]
const FILE_BASE:u64=0x10000;
#[cfg(any(feature="r025-a",feature="r025-b"))]
fn r025_token(resource:u64)->u64{
    let token=call(10,resource,0);
    if token==u64::MAX||token==0{exit(69)}
    token
}
#[cfg(any(feature="r025-a",feature="r025-b"))]
fn r025_send(receiver:u32,token:u64,data:&[u8]){
    if data.len()>64{exit(70);}
    let mut packet=[0u8;76];
    packet[..4].copy_from_slice(&receiver.to_le_bytes());
    packet[4..12].copy_from_slice(&token.to_le_bytes());
    packet[12..12+data.len()].copy_from_slice(data);
    if call(8,packet.as_ptr() as u64,(12+data.len()) as u64)!=0{exit(71);}
}
#[cfg(any(feature="r025-a",feature="r025-b"))]
fn r025_file(operation:u64,token:u64,path:&[u8],address:u64,length:u64)->u64{
    if path.is_empty()||path.len()>=24{exit(72);}
    let mut descriptor=[0u8;48];
    descriptor[..8].copy_from_slice(&token.to_le_bytes());
    descriptor[8..8+path.len()].copy_from_slice(path);
    descriptor[32..40].copy_from_slice(&address.to_le_bytes());
    descriptor[40..48].copy_from_slice(&length.to_le_bytes());
    call(operation,descriptor.as_ptr() as u64,48)
}
#[cfg(any(feature="r025-a",feature="r025-b"))]
fn r025_create(token:u64,path:&[u8],data:&[u8]){
    if r025_file(11,token,path,data.as_ptr() as u64,data.len() as u64)!=0{exit(73);}
}
#[cfg(any(feature="r025-a",feature="r025-b"))]
fn r025_read(token:u64,path:&[u8],expected:&[u8]){
    let mut output=[0xccu8;64];
    let n=r025_file(12,token,path,output.as_mut_ptr() as u64,output.len() as u64);
    if n!=expected.len() as u64||&output[..expected.len()]!=expected{exit(74);}
}
#[cfg(any(feature="r025-a",feature="r025-b"))]
fn r025_delete(token:u64,path:&[u8]){
    if r025_file(13,token,path,0,0)!=0{exit(75);}
}

#[cfg(any(feature="interactive",feature="r026-shell"))]
static mut LAST_COMMAND:[u8;64]=[0;64];
#[cfg(any(feature="interactive",feature="r026-shell"))]
static mut LAST_LEN:usize=0;
#[cfg(any(feature="interactive",feature="r026-shell"))]
fn read_line(buffer:&mut [u8;64])->usize{
    let mut used=0usize;
    loop{
        let mut byte=[0u8;1];
        let count=call(6,byte.as_mut_ptr() as u64,1);
        if count==0 {core::hint::spin_loop();continue;}
        if count!=1 {exit(40);}
        match byte[0] {
            b'\n'=>return used,
            0x10=>{
                // Recall the last complete command into the bounded buffer.
                let len=unsafe{LAST_LEN};
                if len<=buffer.len(){
                    buffer[..len].copy_from_slice(unsafe{&LAST_COMMAND[..len]});
                    used=len;
                }
            },
            0x0e=>{buffer.fill(0);used=0;},
            8=>{used=used.saturating_sub(1);},
            32..=126=> {
                if used<buffer.len(){buffer[used]=byte[0];used+=1;}
            }
            _=>{}
        }
    }
}
#[cfg(any(feature="interactive",feature="r026-shell"))]
fn interactive_shell()->!{
    #[cfg(feature="r026-shell")]
    write(b"R026 persistent interactive shell ready");
    #[cfg(not(feature="r026-shell"))]
    write(b"R014 interactive shell ready");
    loop {
        write(b"indianos> ");
        let mut line=[0u8;64];
        let count=read_line(&mut line);
        if count>0{
            unsafe{
                LAST_COMMAND[..count].copy_from_slice(&line[..count]);
                LAST_LEN=count;
            }
        }
        match shell::parse(&line[..count]){
            Some(shell::Command::Help)=>
                write(b"R014 shell: commands help ps run /bin/worker exit"),
            Some(shell::Command::Ps)=>{
                if call(4,0,0)!=1 {exit(41);}
                #[cfg(feature="r026-shell")]
                {
                    let state=call(16,2,0);
                    match state {
                        1=>write(b"R026 shell: ps PID1 RUNNING PID2 NOT_STARTED"),
                        4=>write(b"R026 shell: ps PID1 RUNNING PID2 REAPED"),
                        _=>exit(42),
                    }
                }
                #[cfg(not(feature="r026-shell"))]
                write(b"R014 shell: ps PID 1 RUNNING");
            },
            Some(shell::Command::RunWorker)=>{
                #[cfg(feature="r026-shell")]
                {
                    if call(5,1,0)!=0{
                        write(b"R026 shell: duplicate worker launch rejected");
                    }else{
                        write(b"R026 shell: worker started; yielding to scheduler");
                        yield_now();
                        if call(17,2,0)!=1{exit(43);}
                        write(b"R026 shell: resumed after worker completion");
                    }
                }
                #[cfg(not(feature="r026-shell"))]
                {
                    if call(5,1,0)!=0 {
                        write(b"R014 shell: worker request rejected");
                    }else{
                        write(b"R014 shell: worker queued for after exit");
                    }
                }
            },
            Some(shell::Command::WaitWorker)=>{
                #[cfg(feature="r026-shell")]
                if call(17,2,0)==1{
                    write(b"R026 shell: wait /bin/worker completed");
                }else{exit(44);}
                #[cfg(not(feature="r026-shell"))]
                write(b"R014 shell: unknown command");
            },
            Some(shell::Command::Version)=>write(b"R015 IndianOS Rust userland shell v0.15"),
            Some(shell::Command::History)=>{
                write(b"R015 shell: history (one-entry recall with Up arrow)");
            },
            Some(shell::Command::Exit)=>{
                #[cfg(feature="r026-shell")]
                write(b"R026 shell: user requested shutdown");
                #[cfg(not(feature="r026-shell"))]
                write(b"R014 shell: exit");
                exit(0);
            },
            None=>write(b"R014 shell: unknown command"),
        }
    }
}

// Separate ring-3 application: owns *commands* for Files and Terminal,
// not a physical framebuffer pointer or a kernel window-data structure.
#[cfg(feature="r029-client")]
fn r029_command(window:u32,kind:u32,x:u16,y:u16,width:u16,height:u16,
    rgb:u32,text:&[u8])->[u8;64]
{
    let mut wire=[0u8;64];
    wire[..4].copy_from_slice(b"R29U");
    wire[4..8].copy_from_slice(&1u32.to_le_bytes());
    wire[8..12].copy_from_slice(&window.to_le_bytes());
    wire[12..16].copy_from_slice(&kind.to_le_bytes());
    wire[16..18].copy_from_slice(&x.to_le_bytes());
    wire[18..20].copy_from_slice(&y.to_le_bytes());
    wire[20..22].copy_from_slice(&width.to_le_bytes());
    wire[22..24].copy_from_slice(&height.to_le_bytes());
    wire[24..28].copy_from_slice(&rgb.to_le_bytes());
    wire[32..32+text.len()].copy_from_slice(text);
    wire
}
#[cfg(feature="r029-client")]
fn r029_draw(cmd:&[u8;64]){
    if call(18,cmd.as_ptr() as u64,64)!=0{exit(91);}
}
#[cfg(feature="r029-client")]
fn r029_wait_for_host(){
    let mut byte=[0u8;1];
    for _ in 0..80_000_000 {
        if call(6,byte.as_mut_ptr() as u64,1)==1{
            if byte[0]==b'g'{return;}
            exit(92);
        }
        core::hint::spin_loop();
    }
    exit(93)
}
#[cfg(feature="r029-client")]
fn r029_demo()->!{
    if call(4,0,0)!=1 || call(20,0,0)!=(2u64<<24){exit(94);}
    write(b"R029 ring3: user compositor client ready");
    r029_wait_for_host();
    r029_draw(&r029_command(1,1,15,102,220,32,0x0045967c,&[]));
    r029_draw(&r029_command(1,2,20,109,252,14,0x00ffffff,b"RING3 FILES CONTENT"));
    write(b"R029 ring3: Files content rendered by usermode syscall");
    r029_wait_for_host();
    r029_draw(&r029_command(2,1,20,175,270,32,0x003e589f,&[]));
    r029_draw(&r029_command(2,2,24,181,240,14,0x00ffffff,b"RING3 TERMINAL READY"));
    write(b"R029 ring3: Terminal content rendered by usermode syscall");
    r029_wait_for_host();
    let mut ev=[0xa5u8;16];
    if call(21,ev.as_mut_ptr() as u64,16)!=0 ||
       ev!=[0xa5u8;16] || call(21,0,16)!=u64::MAX {exit(98);}
    if call(19,1,0)!=0 || call(19,3,0)!=u64::MAX ||
       call(20,0,0)!=(1u64<<24|1u64<<16|2u64<<8|2u64) {exit(95);}
    if call(21,ev.as_mut_ptr() as u64,15)!=u64::MAX ||
       call(21,ev.as_mut_ptr() as u64,16)!=16 ||
       &ev[..12]!=&[1,0,0,0,1,0,0,0,1,0,0,0] ||
       &ev[12..]!=&[0,0,0,0] ||
       call(21,ev.as_mut_ptr() as u64,16)!=0 {exit(99);}
    write(b"R029 ring3: authenticated window focus event delivered");
    let mut invalid=r029_command(1,1,1200,900,400,300,0x00445566,&[]);
    if call(18,invalid.as_ptr() as u64,64)!=u64::MAX ||
       call(18,0,64)!=u64::MAX ||
       call(18,0xffff_ffff_8000_0000,64)!=u64::MAX ||
       call(18,invalid.as_ptr() as u64,63)!=u64::MAX {exit(96);}
    invalid[0]=0;
    if call(18,invalid.as_ptr() as u64,64)!=u64::MAX ||
       call(20,1,0)!=u64::MAX ||
       call(20,0,0)!=(1u64<<24|1u64<<16|2u64<<8|2u64) {exit(97);}
    write(b"R029 ring3: isolated drawing bounds and forged pointers rejected");
    #[cfg(feature="r029-physical")]
    {
        let mut physical=[0u8;16];
        let mut received=false;
        for _ in 0..8_000_000 {
            match call(21,physical.as_mut_ptr() as u64,16){
                0=>core::hint::spin_loop(),
                16=>{
                    let seq=u32::from_le_bytes(physical[..4].try_into().unwrap());
                    let window=u32::from_le_bytes(physical[4..8].try_into().unwrap());
                    let kind=u32::from_le_bytes(physical[8..12].try_into().unwrap());
                    if seq!=2||window!=2||kind!=2||physical[12..]!=[0;4] {
                        exit(100);
                    }
                    received=true;break;
                }
                _=>exit(101),
            }
        }
        if !received||call(20,0,0)>>24!=2 {exit(102);}
        write(b"R029 ring3: physical PS2 mouse focus event delivered");
    }
    r029_wait_for_host();
    write(b"R029 ring3: user surface client exited cleanly");
    exit(0)
}

#[no_mangle]
pub extern "C" fn _start()->!{
    #[cfg(feature="r029-client")]
    r029_demo();
    #[cfg(feature="demo")]
    {
        write(b"Namaste from IndianOS user mode!");
        exit(0);
    }
    #[cfg(any(feature="interactive",feature="r026-shell"))]
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
                Some(shell::Command::WaitWorker)=>exit(34),
                Some(shell::Command::Version) => write(b"R015 IndianOS Rust userland shell v0.15"),
                Some(shell::Command::History) => write(b"R015 shell: history"),
                Some(shell::Command::Exit) => {
                    write(b"R013 shell: exit");
                    exit(0);
                }
                None => exit(32),
            }
        }
        exit(33);
    }
    #[cfg(feature="r026-worker")]
    {
        if call(4,0,0)!=2 || call(5,1,0)!=u64::MAX ||
           call(16,2,0)!=u64::MAX || call(17,2,0)!=u64::MAX{exit(56);}
        write(b"R026 /bin/worker ran in isolated ring-3 process");
        exit(0);
    }
    #[cfg(feature="worker")]
    {
        write(b"IndianOS worker process 2 completed");
        exit(0);
    }
    #[cfg(feature="r025-a")]
    {
        if call(4,0,0)!=1 || call(15,2,0)!=2 || call(15,3,0)!=u64::MAX{exit(76);}
        write(b"R025 PID1 process service validated");
        let send=r025_token(2);
        let file=r025_token(FILE_BASE+1);
        if call(10,FILE_BASE+2,0)!=u64::MAX{exit(77);}
        r025_create(file,b"/p1-note",b"owned-by-one");
        r025_read(file,b"/p1-note",b"owned-by-one");
        let mut foreign=[0xeeu8;64];
        if r025_file(12,file,b"/p2-note",foreign.as_mut_ptr() as u64,64)!=u64::MAX ||
           r025_file(12,file,b"/p1-note",0,64)!=u64::MAX ||
           r025_file(11,file,b"/p1-note/../p2-note",b"x".as_ptr() as u64,1)!=u64::MAX ||
           foreign.iter().any(|&b|b!=0xee){exit(78);}
        write(b"R025 PID1 isolated filesystem create/read verified");
        r025_send(2,send,b"cap-ping");
        write(b"R025 PID1 capability IPC ping sent");
        yield_now();
        ipc_receive(2,b"cap-pong");
        write(b"R025 PID1 authenticated IPC reply received");
        let mut forged=[0u8;13];
        forged[..4].copy_from_slice(&2u32.to_le_bytes());
        forged[4..12].copy_from_slice(&file.to_le_bytes());
        forged[12]=b'x';
        if call(8,forged.as_ptr() as u64,forged.len() as u64)!=u64::MAX ||
           call(10,FILE_BASE+2,0)!=u64::MAX ||
           call(8,0,12)!=u64::MAX{exit(79);}
        write(b"R025 PID1 invalid token and foreign resource rejected");
        r025_delete(file,b"/p1-note");
        if call(14,file,0)!=0 ||
           r025_file(11,file,b"/p1-new",b"x".as_ptr() as u64,1)!=u64::MAX {
           exit(80);
        }
        write(b"R025 PID1 file deletion and capability revocation verified");
        yield_now();
        write(b"R025 PID1 exited cleanly");
        exit(0);
    }
    #[cfg(feature="r025-b")]
    {
        if call(4,0,0)!=2 || call(15,1,0)!=2{exit(81);}
        write(b"R025 PID2 process service validated");
        let send=r025_token(1);
        let file=r025_token(FILE_BASE+2);
        ipc_receive(1,b"cap-ping");
        write(b"R025 PID2 authenticated IPC ping received");
        r025_create(file,b"/p2-note",b"owned-by-two");
        r025_read(file,b"/p2-note",b"owned-by-two");
        let mut denied=[0u8;64];
        if r025_file(12,file,b"/p1-note",denied.as_mut_ptr() as u64,64)!=u64::MAX ||
           r025_file(13,file,b"/p1-note",0,0)!=u64::MAX ||
           call(14,1,0)!=u64::MAX {exit(82);}
        write(b"R025 PID2 foreign filesystem and token access denied");
        r025_send(1,send,b"cap-pong");
        write(b"R025 PID2 capability IPC reply sent");
        yield_now();
        r025_read(file,b"/p2-note",b"owned-by-two");
        r025_delete(file,b"/p2-note");
        if call(14,file,0)!=0 ||
           r025_file(12,file,b"/p2-note",denied.as_mut_ptr() as u64,64)!=u64::MAX{
           exit(83);
        }
        write(b"R025 PID2 file deletion and rights teardown verified");
        yield_now();
        write(b"R025 PID2 exited cleanly");
        exit(0);
    }
    #[cfg(feature="ipc-a")]
    {
        if call(4,0,0)!=1 {exit(64);}
        let mut empty=[0xaau8;72];
        // Receiving an empty mailbox must not overwrite user memory.
        if call(9,empty.as_mut_ptr() as u64,72)!=0 ||
           empty.iter().any(|b|*b!=0xaa){exit(65);}
        ipc_send(2,b"ping-identity");
        write(b"R025 PID1 sent authenticated ping");
        yield_now();
        ipc_receive(2,b"pong-identity");
        write(b"R025 PID1 received authenticated reply");
        // Reject unknown destination and forbidden ring-3 pointers.
        let mut invalid=[0u8;8];
        invalid[..4].copy_from_slice(&99u32.to_le_bytes());
        if call(8,invalid.as_ptr() as u64,8)!=u64::MAX ||
           call(8,0,8)!=u64::MAX ||
           call(9,0,72)!=u64::MAX {exit(66);}
        write(b"R025 PID1 invalid destination and pointers denied");
        yield_now();
        write(b"R025 PID1 exited cleanly");
        exit(0);
    }
    #[cfg(feature="ipc-b")]
    {
        if call(4,0,0)!=2 {exit(67);}
        ipc_receive(1,b"ping-identity");
        write(b"R025 PID2 received authenticated ping");
        ipc_send(1,b"pong-identity");
        write(b"R025 PID2 sent authenticated reply");
        yield_now();
        let mut output=[0xccu8;72];
        if call(9,output.as_mut_ptr() as u64,72)!=0 ||
           output.iter().any(|b|*b!=0xcc){exit(68);}
        write(b"R025 PID2 empty queue is isolated");
        yield_now();
        write(b"R025 PID2 exited cleanly");
        exit(0);
    }
    #[cfg(feature="r030-demo-v1")]
    {
        write(b"R030 signed native package version 1 executed in ring-3");
        exit(0);
    }
    #[cfg(feature="r030-demo-v2")]
    {
        write(b"R030 signed native package version 2 executed in ring-3");
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
        feature="sched-b",feature="ipc-a",feature="ipc-b",feature="r026-shell",feature="r026-worker",feature="r025-a",feature="r025-b",feature="fault-guard",feature="fault-supervisor",
        feature="preempt-a",feature="preempt-b",feature="r030-demo-v1",feature="r030-demo-v2"
    )))]
    loop {unsafe{asm!("pause",options(nomem,nostack,preserves_flags))}}
}
