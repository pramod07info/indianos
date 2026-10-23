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
#[cfg(any(feature="r029-client",feature="r029-files",feature="r029-terminal",
          feature="r029-frame-server",feature="r029-frame-client",
          feature="r029-suite-shell",feature="r029-suite-files",feature="r029-suite-terminal"))]
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
#[cfg(any(feature="r029-client",feature="r029-files",feature="r029-terminal",
          feature="r029-frame-server",feature="r029-frame-client",
          feature="r029-suite-shell",feature="r029-suite-files",feature="r029-suite-terminal"))]
fn r029_draw(cmd:&[u8;64]){
    if call(18,cmd.as_ptr() as u64,64)!=0{exit(91);}
}
#[cfg(any(feature="r029-client",feature="r029-files",feature="r029-terminal",
          feature="r029-frame-server",feature="r029-frame-client",
          feature="r029-suite-shell",feature="r029-suite-files",feature="r029-suite-terminal"))]
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

// R029.3: distinct freestanding ELF images; neither program may draw the
// other process's surface. The kernel never accepts an asserted PID from wire.
#[cfg(any(feature="r029-files",feature="r029-terminal"))]
fn r029_app()->!{
    #[cfg(feature="r029-files")]
    let (pid,owned,foreign,color,title)=(1u64,1u32,2u32,0x0045967cu32,&b"FILES PID1"[..]);
    #[cfg(feature="r029-terminal")]
    let (pid,owned,foreign,color,title)=(2u64,2u32,1u32,0x003e589fu32,&b"TERMINAL PID2"[..]);
    if call(4,0,0)!=pid||call(20,1,0)!=u64::MAX{exit(110);}
    let invalid=r029_command(foreign,1,12,110,180,30,color,&[]);
    if call(18,invalid.as_ptr() as u64,64)!=u64::MAX||
       call(19,foreign as u64,0)!=u64::MAX||
       call(18,0,64)!=u64::MAX{exit(111);}
    #[cfg(feature="r029-files")]
    write(b"R029.3 PID1 Files ready");
    #[cfg(feature="r029-terminal")]
    write(b"R029.3 PID2 Terminal ready");
    r029_wait_for_host();
    let y=if owned==1{102u16}else{175u16};
    r029_draw(&r029_command(owned,1,15,y,264,32,color,&[]));
    r029_draw(&r029_command(owned,2,18,y+6,240,14,0x00ffffff,title));
    #[cfg(feature="r029-files")]
    write(b"R029.3 PID1 Files rendered own surface");
    #[cfg(feature="r029-terminal")]
    write(b"R029.3 PID2 Terminal rendered own surface");
    yield_now();
    #[cfg(feature="r029-files")]
    write(b"R029.3 PID1 ready for focus");
    #[cfg(feature="r029-terminal")]
    write(b"R029.3 PID2 ready for focus");
    r029_wait_for_host();
    if call(19,owned as u64,0)!=0||
       call(19,foreign as u64,0)!=u64::MAX{exit(112);}
    let mut event=[0u8;16];
    if call(21,0,16)!=u64::MAX||
       call(21,event.as_mut_ptr() as u64,15)!=u64::MAX||
       call(21,event.as_mut_ptr() as u64,16)!=16{exit(113);}
    let sequence=u32::from_le_bytes(event[..4].try_into().unwrap());
    let window=u32::from_le_bytes(event[4..8].try_into().unwrap());
    let kind=u32::from_le_bytes(event[8..12].try_into().unwrap());
    if sequence!=owned||window!=owned||kind!=1||event[12..]!=[0;4]||
       call(21,event.as_mut_ptr() as u64,16)!=0{exit(114);}
    #[cfg(feature="r029-files")]
    {
        write(b"R029.3 PID1 isolated focus event");
        yield_now();
        write(b"R029.3 PID1 closed its own GUI service");
        exit(0);
    }
    #[cfg(feature="r029-terminal")]
    {
        write(b"R029.3 PID2 awaiting actual PS2 input");
        let mut received=false;
        for _ in 0..15_000_000{
            match call(21,event.as_mut_ptr() as u64,16){
                0=>core::hint::spin_loop(),
                16=>{
                    let seq=u32::from_le_bytes(event[..4].try_into().unwrap());
                    let win=u32::from_le_bytes(event[4..8].try_into().unwrap());
                    let kind=u32::from_le_bytes(event[8..12].try_into().unwrap());
                    if seq!=3||win!=2||kind!=2||event[12..]!=[0;4]{exit(115);}
                    received=true;break;
                },
                _=>exit(116),
            }
        }
        if !received||(call(20,0,0)>>24)!=2{exit(117);}
        write(b"R029.3 PID2 received real PS2 mouse event");
        yield_now();
        if call(19,1,0)!=u64::MAX||call(20,0,0)==u64::MAX{exit(118);}
        write(b"R029.3 PID2 retained its independent GUI surface");
        exit(0);
    }
}
// R029.4: separate frame-server and client ELF images, each with its own
// virtual address space. PID2 publishes commands but cannot present pixels.
#[cfg(feature="r029-frame-server")]
fn r029_frame_server()->!{
    if call(4,0,0)!=1 || call(22,0,0)!=0 || call(23,1,0)!=u64::MAX{
        exit(130);
    }
    let forbidden=r029_command(2,1,15,120,100,20,0x00bb2211,&[]);
    if call(18,forbidden.as_ptr() as u64,64)!=u64::MAX{exit(131);}
    write(b"R029.4 frame server PID1 ready");
    r029_wait_for_host();
    r029_draw(&r029_command(1,1,15,102,264,32,0x0045967c,&[]));
    r029_draw(&r029_command(1,2,18,109,240,14,0x00ffffff,b"USER FRAME SERVER"));
    write(b"R029.4 Files surface queued, not presented");
    yield_now();
    write(b"R029.4 frame server ready for first present");
    r029_wait_for_host();
    if call(22,0,0)!=(2|(2<<8)) ||
       call(23,0,0)!=u64::MAX || call(23,1,0)!=0 ||
       call(22,0,0)!=(2|(2<<8)|(1<<16)){exit(132);}
    write(b"R029.4 first user-scheduled frame presented");
    yield_now();
    write(b"R029.4 frame server ready for second present");
    r029_wait_for_host();
    if call(22,0,0)!=(2|(2<<8)|(1<<16)) ||
       call(23,2,0)!=0 ||
       call(22,0,0)!=(2|(2<<8)|(2<<16)) ||
       call(23,1,0)!=u64::MAX {exit(133);}
    write(b"R029.4 second user-scheduled frame presented");
    exit(0);
}
#[cfg(feature="r029-frame-client")]
fn r029_frame_client()->!{
    if call(4,0,0)!=2 || call(22,0,0)!=u64::MAX ||
       call(23,2,0)!=u64::MAX{exit(134);}
    let foreign=r029_command(1,1,16,102,120,20,0x00ff0000,&[]);
    if call(18,foreign.as_ptr() as u64,64)!=u64::MAX ||
       call(19,1,0)!=u64::MAX {exit(135);}
    write(b"R029.4 client PID2 ready");
    r029_wait_for_host();
    r029_draw(&r029_command(2,1,20,175,264,32,0x003e589f,&[]));
    r029_draw(&r029_command(2,2,24,181,240,14,0x00ffffff,b"FRAME CLIENT PID2"));
    write(b"R029.4 Terminal queued, no framebuffer draw");
    yield_now();
    write(b"R029.4 client waiting for real PS2 event");
    let mut event=[0u8;16];
    let mut arrived=false;
    for _ in 0..18_000_000 {
        match call(21,event.as_mut_ptr() as u64,16) {
            0=>core::hint::spin_loop(),
            16=>{
                let seq=u32::from_le_bytes(event[..4].try_into().unwrap());
                let window=u32::from_le_bytes(event[4..8].try_into().unwrap());
                let kind=u32::from_le_bytes(event[8..12].try_into().unwrap());
                if seq!=1||window!=2||kind!=2||event[12..]!=[0;4]{
                    exit(136);
                }
                arrived=true;break;
            },
            _=>exit(137),
        }
    }
    if !arrived||(call(20,0,0)>>24)!=2 {exit(138);}
    write(b"R029.4 physical PS2 event queued without kernel present");
    yield_now();
    if call(22,0,0)!=u64::MAX || call(23,2,0)!=u64::MAX ||
       call(18,foreign.as_ptr() as u64,64)!=u64::MAX ||
       call(20,0,0)==u64::MAX {exit(139);}
    write(b"R029.4 client survived independent compositor teardown");
    exit(0);
}
// Integrated R029.5-7: PID1 desktop controller, PID2 Files, PID3 Terminal.
// All clients are distinct protected x86-64 ELF64 images.
#[cfg(any(feature="r029-suite-shell",feature="r029-suite-files",feature="r029-suite-terminal"))]
fn suite_send(dst:u32,bytes:&[u8]){
    if bytes.len()>64{exit(140);}
    let mut packet=[0u8;68];
    packet[..4].copy_from_slice(&dst.to_le_bytes());
    packet[4..4+bytes.len()].copy_from_slice(bytes);
    if call(8,packet.as_ptr() as u64,(bytes.len()+4) as u64)!=0{exit(141);}
}
#[cfg(any(feature="r029-suite-shell",feature="r029-suite-files",feature="r029-suite-terminal"))]
fn suite_receive(sender:u32,payload:&[u8]){
    let mut response=[0u8;72];
    let len=call(9,response.as_mut_ptr() as u64,72);
    if len!=(payload.len()+8) as u64||
       u32::from_le_bytes(response[..4].try_into().unwrap())!=sender||
       u32::from_le_bytes(response[4..8].try_into().unwrap())!=payload.len() as u32||
       &response[8..8+payload.len()]!=payload {exit(142);}
}
#[cfg(feature="r029-suite-shell")]
fn suite_shell()->!{
    if call(4,0,0)!=1 ||call(22,0,0)!=0||call(24,7,0)!=u64::MAX{exit(143);}
    write(b"R029.5 PID1 desktop shell ready");
    r029_wait_for_host();
    yield_now();
    suite_receive(2,b"FILES");
    suite_receive(3,b"TERMINAL");
    if call(22,0,0)!=(2|(2<<8))||call(23,1,0)!=0{exit(144);}
    write(b"R029.5 IPC joined apps and first frame rendered");
    r029_wait_for_host();
    if call(24,6,0)!=0||call(23,1,0)!=0{exit(145);}
    write(b"R029.7 launcher opened");
    r029_wait_for_host();
    if call(24,6,0)!=0||call(24,2,0)!=0||call(23,2,0)!=0{exit(146);}
    write(b"R029.7 launcher closed and Terminal focused");
    r029_wait_for_host();
    if call(24,3,0)!=0||call(24,4,0)!=0||call(23,1,0)!=0{exit(147);}
    write(b"R029.7 Files moved, Terminal minimized");
    r029_wait_for_host();
    if call(24,5,0)!=0||call(23,2,0)!=0||call(23,2,0)!=u64::MAX{exit(148);}
    write(b"R029.7 Terminal restored, fifth frame committed");
    suite_send(2,b"EXIT");
    write(b"R029.5 three-process suite awaiting physical pointer");
    yield_now();
    write(b"R029.7 desktop shell exit");
    exit(0);
}
#[cfg(feature="r029-suite-files")]
fn suite_files()->!{
    if call(4,0,0)!=2||call(22,0,0)!=u64::MAX||
       call(23,1,0)!=u64::MAX||call(24,6,0)!=u64::MAX{exit(149);}
    let foreign=r029_command(2,1,16,100,100,30,0x00ff0000,&[]);
    if call(18,foreign.as_ptr() as u64,64)!=u64::MAX{exit(150);}
    r029_draw(&r029_command(1,1,15,102,264,32,0x0045967c,&[]));
    r029_draw(&r029_command(1,2,18,109,180,14,0x00ffffff,b"FILES PID2"));
    suite_send(1,b"FILES");
    write(b"R029.5 PID2 Files surface staged");
    yield_now();
    suite_receive(1,b"EXIT");
    if call(18,foreign.as_ptr() as u64,64)!=u64::MAX{exit(151);}
    write(b"R029.5 PID2 Files exited and grants revoked");
    exit(0);
}
#[cfg(feature="r029-suite-terminal")]
fn suite_terminal()->!{
    if call(4,0,0)!=3||call(22,0,0)!=u64::MAX||
       call(23,2,0)!=u64::MAX||call(24,6,0)!=u64::MAX{exit(152);}
    let foreign=r029_command(1,1,16,100,100,30,0x00ff0000,&[]);
    if call(18,foreign.as_ptr() as u64,64)!=u64::MAX{exit(153);}
    r029_draw(&r029_command(2,1,20,175,270,32,0x003e589f,&[]));
    r029_draw(&r029_command(2,2,24,181,180,14,0x00ffffff,b"TERMINAL PID3"));
    suite_send(1,b"TERMINAL");
    write(b"R029.5 PID3 Terminal surface staged");
    yield_now();
    write(b"R029.6 PID3 waiting for physical PS2 focus");
    let mut ev=[0u8;16];let mut received=false;
    for _ in 0..18_000_000 {
        match call(21,ev.as_mut_ptr() as u64,16){
            0=>core::hint::spin_loop(),
            16=>{
                let target=u32::from_le_bytes(ev[4..8].try_into().unwrap());
                let kind=u32::from_le_bytes(ev[8..12].try_into().unwrap());
                if target!=2||kind!=2 {exit(154);}
                received=true;break;
            }
            _=>exit(155),
        }
    }
    if !received{exit(156);}
    write(b"R029.6 PID3 authenticated physical input delivered");
    exit(0);
}
// R030.1/30.2 on-device QEMU-only bounded persistent file ABI.
// The syscall carries a validated 48-byte descriptor; no app can pass a
// forged kernel process ID or a host filesystem path to the disk driver.
#[cfg(feature="r030-ondevice-init")]
fn r030_file(op:u64,path:&[u8],data:&mut [u8],used:usize)->u64{
    if path.len()<2||path.len()>=24||data.len()>256||used>data.len(){
        exit(201);
    }
    let mut desc=[0u8;48];
    desc[..path.len()].copy_from_slice(path);
    if matches!(op,1|2|3){
        desc[24..32].copy_from_slice(&(data.as_ptr() as u64).to_le_bytes());
        desc[32..40].copy_from_slice(&(if op==2{data.len()}else{used} as u64).to_le_bytes());
    }
    call(30,op,desc.as_ptr() as u64)
}
#[cfg(feature="r030-ondevice-init")]
fn r030_exec()->!{
    if call(4,0,0)!=1 ||call(31,9,0)!=u64::MAX{exit(202);}
    let mut phase=[0u8;4];
    let size=r030_file(2,b"/phase",&mut phase,0);
    let stage=if size==u64::MAX{0}else if size==1 && (b'1'..=b'3').contains(&phase[0]){
        phase[0]-b'0'
    }else{exit(203)};
    let mut note=[0u8;32];
    match stage{
        0=>{
            if call(31,0,0)!=0 ||call(5,1,0)!=u64::MAX{exit(204);}
            note[..5].copy_from_slice(b"HELLO");
            if r030_file(1,b"/note",&mut note,5)!=0{exit(205);}
            if call(31,1,0)!=1 ||call(31,1,0)!=u64::MAX{exit(206);}
            phase[0]=b'1';
            if r030_file(1,b"/phase",&mut phase,1)!=0{exit(207);}
            write(b"R030.1 first boot persistent note created; R030.2 app v1 installed");
        }
        1=>{
            if call(31,0,0)!=1 || r030_file(2,b"/note",&mut note,0)!=5||
               &note[..5]!=b"HELLO"{exit(208);}
            if call(31,2,0)!=2||call(31,2,0)!=u64::MAX{exit(209);}
            phase[0]=b'2';
            if r030_file(3,b"/phase",&mut phase,1)!=0{exit(210);}
            write(b"R030.1 reboot persisted user file; R030.2 upgraded installed app to v2");
        }
        2=>{
            if call(31,0,0)!=2 || call(31,3,0)!=1 ||
               call(31,3,0)!=u64::MAX{exit(211);}
            phase[0]=b'3';
            if r030_file(3,b"/phase",&mut phase,1)!=0{exit(212);}
            write(b"R030.2 previously installed app rollback to v1 persisted");
        }
        3=>{
            if call(31,0,0)!=1 ||r030_file(2,b"/note",&mut note,0)!=5||
               &note[..5]!=b"HELLO"{exit(213);}
            write(b"R030.1 disk note survived fourth QEMU reboot");
        }
        _=>exit(214)
    }
    if call(5,1,0)!=0 {exit(215);}
    write(b"R030.2 verified allowlisted native ELF scheduled by PID1");
    yield_now();
    if call(17,2,0)!=1{exit(216);}
    if stage==3{
        if call(31,4,0)!=0 ||call(31,0,0)!=0 ||call(5,1,0)!=u64::MAX{
            exit(217);
        }
        // File ACL denies writes into internal app records.
        if r030_file(1,b"/app",&mut note,5)!=u64::MAX{exit(218);}
        if r030_file(4,b"/note",&mut note,0)!=0{exit(219);}
        write(b"R030.2 native app removed; launch denied; user file deleted");
    }else if call(31,0,0)!=(if stage==1{2}else{1}){exit(220);}
    write(b"R030.1-2 ring3 lifecycle clean on persistent QEMU volume");
    exit(0);
}
#[no_mangle]
pub extern "C" fn _start()->!{
    #[cfg(feature="r030-ondevice-init")]
    r030_exec();
    #[cfg(feature="r030-ondevice-app")]
    {
        if call(4,0,0)!=2 || call(30,1,0)!=u64::MAX ||
           call(31,0,0)!=u64::MAX ||call(5,1,0)!=u64::MAX {exit(221);}
        write(b"R030.2 installed native ELF ran in isolated PID2 ring3");
        exit(0);
    }
    #[cfg(feature="r029-suite-shell")]
    suite_shell();
    #[cfg(feature="r029-suite-files")]
    suite_files();
    #[cfg(feature="r029-suite-terminal")]
    suite_terminal();
    #[cfg(feature="r029-frame-server")]
    r029_frame_server();
    #[cfg(feature="r029-frame-client")]
    r029_frame_client();
    #[cfg(any(feature="r029-files",feature="r029-terminal"))]
    r029_app();
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
