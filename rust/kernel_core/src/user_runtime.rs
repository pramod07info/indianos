//! Rust owner of ring-3 process launch, syscall/fault dispatch and M009 run queue.
//! The only retained assembly is the privileged IRETQ/CR3/register trampoline.
//! Single-core experimental OS: no SMP and no safe runtime reentrancy.
use core::{arch::asm,ffi::{c_char,c_void},mem::MaybeUninit,ptr};
use crate::{Process,ProcessTable,Scheduler,
 indianos_process_table_init,indianos_process_create,indianos_process_ready,
 indianos_process_bind,indianos_process_start,indianos_process_resume,
 indianos_process_exit,indianos_process_fault,indianos_process_reap,
 indianos_process_success,indianos_scheduler_init,indianos_scheduler_next,
 indianos_scheduler_yield,indianos_scheduler_all_terminal,
 indianos_scheduler_account_tick,indianos_user_fault_eligible};
use crate::paging::{CpuState,Backend as CloneBackend,CloneResult,
 paging_preflight,paging_clone_kernel_half,paging_clone_release,
 paging_attach_user_half,paging_detach_user_half,paging_lookup};
use crate::paging_cpu::x86_64_paging_detect;
use crate::vmm::{Backend as VmmBackend,Mapping,Permissions,vmm_query_page};
use crate::user_image::{UserImage,indianos_user_image_load,
 indianos_user_image_destroy,indianos_user_copy_readonly};
use crate::syscall::{SyscallRequest,indianos_syscall_validate,ring3_frame_valid,USER_TOP,STACK_BYTES,PAGE_BYTES};
use crate::program::indianos_program_user_range;
use crate::boot::Region;
use crate::user_image::{indianos_user_can_write,indianos_user_copy_writable};
#[cfg(feature="test-interactive")]
use crate::input;
#[cfg(feature="test-ipc")]
use crate::ipc::{IpcRouter,IpcError};
#[cfg(feature="test-r025")]
use crate::r025_services::{R025Services,FILE_BASE,MAX_FILE_TRANSFER};
const PHYS:u64=0x000f_ffff_ffff_f000;
const PAGE:u64=4096;
const YIELD:u64=0x200;
const WORDS:usize=20;
#[cfg(feature="test-r031-supervisor")]
const TASKS:usize=4;
#[cfg(all(feature="test-r029-desktop-suite",not(feature="test-r031-supervisor")))]
const TASKS:usize=3;
#[cfg(not(feature="test-r029-desktop-suite"))]
const TASKS:usize=2;
const BAD:u64=u64::MAX;
#[repr(C)]
struct DirectMap{offset:u64}
#[repr(C,align(16))]
struct Task{
 image:UserImage,root:CloneResult,pcb:*mut Process,
 frame:[u64;WORDS],saved:bool,attached:bool,timer_preemptions:u32,
}
#[cfg(feature="test-r029-desktop-suite")]
static mut THIRD_ELF:(*const u8,usize)=(ptr::null(),0);
static mut ACTIVE_USER:*mut UserImage=ptr::null_mut();
// R013: tightly gated and single-core. Only PID 1 can request worker 1.
#[cfg(feature="test-shell")]
static mut ACTIVE_PID:u32=0;
#[cfg(feature="test-shell")]
static mut PENDING_WORKER:bool=false;
#[cfg(feature="test-shell")]
#[no_mangle]
pub extern "C" fn indianos_r013_take_worker_request()->bool {
 unsafe {let requested=PENDING_WORKER;PENDING_WORKER=false;requested}
}
static mut ACTIVE_TASK:*mut Task=ptr::null_mut();
static mut ACTIVE_PROCESS:*mut Process=ptr::null_mut();
#[cfg(feature="test-r026")]
static mut R026_WORKER_REQUESTED:bool=false;
#[cfg(feature="test-r026")]
static mut R026_WORKER_REAPED:bool=false;
// Quantum jobs belong to kernel-verified process IDs, not user-provided IDs.
// Single-core runtime only; do not access this from interrupt context.
static mut QUANTUM_JOBS:crate::quantum_service::QuantumService=
    crate::quantum_service::QuantumService::new();
// Two scheduler-authenticated process mailboxes, off in normal boots.
#[cfg(feature="test-ipc")]
static mut IPC_ROUTER:Option<IpcRouter<4,8>>=None;
#[cfg(feature="test-ipc")]
fn router()->Option<&'static mut IpcRouter<4,8>>{
    // Single BSP; INT 0x80 interrupt gate masks CPU interrupts.
    // No SMP or nested use of this non-atomic router is permitted.
    unsafe{(&mut *ptr::addr_of_mut!(IPC_ROUTER)).as_mut()}
}
#[cfg(feature="test-r025")]
static mut R025_SERVICES:Option<R025Services>=None;
#[cfg(feature="test-r025")]
fn services()->Option<&'static mut R025Services>{
    // Single-BSP INT 0x80 interrupt gate. No IRQ/SMP caller.
    unsafe{(&mut *ptr::addr_of_mut!(R025_SERVICES)).as_mut()}
}
#[cfg(feature="test-r029-userspace")]
fn authenticated_gui_pid()->Option<u32>{
    let process=unsafe{ptr::read(ptr::addr_of!(ACTIVE_PROCESS))};
    let p=unsafe{process.as_ref()}?;
    if p.state!=3||unsafe{ACTIVE_PID}!=p.pid{return None;}
    #[cfg(feature="test-r029-multiprocess")]
    {
        let task=active_task();
        let image=active_image();
        if !(1..=if cfg!(feature="test-r031-supervisor"){4}
                  else if cfg!(feature="test-r029-desktop-suite"){3}else{2})
            .contains(&p.pid)||task.is_null()||image.is_null(){
            return None;
        }
        let task=unsafe{&*task};
        if task.pcb!=process ||
           !core::ptr::eq(&task.image as *const UserImage,image as *const UserImage){
            return None;
        }
    }
    #[cfg(not(feature="test-r029-multiprocess"))]
    if p.pid!=1||!active_task().is_null(){return None;}
    Some(p.pid)
}
#[cfg(feature="test-ipc")]
fn authenticated_ipc_pid()->Option<u32>{
    let process=unsafe{ptr::read(ptr::addr_of!(ACTIVE_PROCESS))};
    let task=active_task();
    let image=active_image();
    if process.is_null()||task.is_null()||image.is_null(){return None;}
    let p=unsafe{&*process};
    let t=unsafe{&*task};
    if p.pid==0 || p.state!=3 || t.pcb!=process ||
       !ptr::eq(&t.image as *const UserImage,image as *const UserImage){
       return None;
    }
    Some(p.pid)
}
#[cfg(feature="test-r026")]
fn authenticated_r026_init()->bool{
    let pcb=unsafe{ptr::read(ptr::addr_of!(ACTIVE_PROCESS))};
    let task=active_task();
    let img=active_image();
    if pcb.is_null()||task.is_null()||img.is_null(){return false;}
    let p=unsafe{&*pcb};
    let t=unsafe{&*task};
    p.pid==1 && p.state==3 && t.pcb==pcb &&
        ptr::eq(&t.image as *const UserImage,img as *const UserImage)
}
fn cleanup_quantum_jobs(pid:u32){
 if pid==0{return;}
 let jobs=unsafe{&mut *ptr::addr_of_mut!(QUANTUM_JOBS)};
 let _=jobs.process_terminated(pid);
}

static mut M009_TASKS:[MaybeUninit<Task>;TASKS]=[const{MaybeUninit::uninit()};TASKS];
#[cfg(feature="test-r031-supervisor")]
static mut SUPERVISED_TERMINAL:crate::service_supervisor::TerminalSupervisor=
    crate::service_supervisor::TerminalSupervisor::new();
static mut LAST_VECTOR:u64=0;
static mut LAST_ERROR:u64=0;
static mut LAST_ADDRESS:u64=0;
#[cfg(feature="preempt")]
static mut REPORTED_IF:bool=false;
unsafe extern "C"{
 fn serial_write(s:*const c_char);
 fn boot_hhdm_offset(out:*mut u64)->bool;
 fn boot_memory_region_count()->u64;
 fn boot_memory_region_at(i:u64,out:*mut Region,context:*mut c_void)->bool;
 fn pmm_allocate_frame(frame:*mut u64)->bool;
 fn pmm_release_frame(frame:u64)->bool;
 fn pmm_free_frames()->u64;
 fn x86_64_user_interrupt_stack_enable()->bool;
 fn x86_64_user_interrupt_stack_select(slot:u32)->bool;
 fn x86_64_user_trap_mapping_addresses(out:*mut u64)->bool;
 fn x86_64_enter_user(entry:u64,stack:u64,cr3:u64)->u64;
 fn x86_64_resume_user(frame:*const u64,cr3:u64)->u64;
 fn x86_64_user_exit_to_kernel(status:u64)->!;
 fn x86_64_user_interrupt();
 fn x86_64_timer_ack_irq0();
}
fn log(b:&'static [u8]){unsafe{serial_write(b.as_ptr().cast())}}
fn pid_log(pid:u32,event:&'static [u8]){
 let mut digits=[0u8;12];let mut n=pid;let mut k=0;
 loop{digits[k]=b'0'+(n%10) as u8;k+=1;n/=10;if n==0{break;}}
 let mut out=[0u8;12];for i in 0..k{out[i]=digits[k-1-i];}
 log(b"IndianOS: PID \0");unsafe{serial_write(out.as_ptr().cast())};
 log(b" \0");log(event);log(b"\n\0");
}
fn active_image()->*mut UserImage{unsafe{ptr::read(ptr::addr_of!(ACTIVE_USER))}}
fn active_task()->*mut Task{unsafe{ptr::read(ptr::addr_of!(ACTIVE_TASK))}}
fn fault_hex(label:&'static [u8],value:u64){
 log(label);
 let mut digits=[0u8;17];
 for i in 0..16{
  let nibble=((value>>((15-i)*4))&15) as u8;
  digits[i]=if nibble<10{b'0'+nibble}else{b'a'+nibble-10};
 }
 digits[16]=0;
 unsafe{serial_write(digits.as_ptr().cast());}
 log(b"\n\0");
}
fn clear_faults(){unsafe{LAST_VECTOR=0;LAST_ERROR=0;LAST_ADDRESS=0}}
fn last_fault()->(u64,u64,u64){unsafe{(LAST_VECTOR,LAST_ERROR,LAST_ADDRESS)}}
fn capture(regs:*const u64,via_timer:bool)->bool{
 let t=active_task();if t.is_null()||regs.is_null(){return false;}
 let task=unsafe{&mut *t};
 let Some(pcb)=(unsafe{task.pcb.as_ref()})else{return false};
 if pcb.state!=3{return false;}
 // CPU-supplied kernel stack contains 15 registers followed by 5 QWORDs.
 let src=unsafe{&*(regs as *const [u64; WORDS])};
 if !ring3_frame_valid(src,&task.image.plan,via_timer){return false;}
 task.frame.copy_from_slice(src);
 if !via_timer{task.frame[14]=0;}
 task.saved=true;
 true
}
#[no_mangle]
pub unsafe extern "C" fn x86_64_scheduler_timer_dispatch(regs:*const u64){
 unsafe{x86_64_timer_ack_irq0()};
 #[cfg(feature="preempt")]
 if capture(regs,true){
  let t=active_task();
  if !t.is_null(){
   let task=unsafe{&mut *t};
   task.timer_preemptions=task.timer_preemptions.saturating_add(1);
   pid_log(unsafe{(*task.pcb).pid},b"TIMER_PREEMPTED\0");
   unsafe{x86_64_user_exit_to_kernel(YIELD)}
  }
 }
 #[cfg(not(feature="preempt"))]{let _=regs;}
}
/// R019.9: dedicated local APIC timer dispatch, distinct from PIC IRQ0.
/// The APIC EOI must be sent before any non-returning scheduler transfer.
#[no_mangle]
pub unsafe extern "C" fn x86_64_scheduler_apic_timer_dispatch(regs:*const u64){
    let acknowledged=unsafe{crate::timer::x86_64_xapic_eoi()};
    if !acknowledged{return;}
    if !crate::timer::x86_64_xapic_timer_preemption_ready(){return;}
    #[cfg(feature="preempt")]
    if capture(regs,true){
        let t=active_task();
        if !t.is_null(){
            let task=unsafe{&mut *t};
            task.timer_preemptions=task.timer_preemptions.saturating_add(1);
            crate::timer::xapic_record_preemption();
            pid_log(unsafe{(*task.pcb).pid},b"APIC_TIMER_PREEMPTED\0");
            unsafe{x86_64_user_exit_to_kernel(YIELD)}
        }
    }
    #[cfg(not(feature="preempt"))]{let _=regs;}
}
#[no_mangle]
pub extern "C" fn x86_64_user_fault_handle(vector:u64,cs:u64,error:u64,cr2:u64){
 if !indianos_user_fault_eligible(vector,cs,error,!active_image().is_null()){return;}
 unsafe{LAST_VECTOR=vector;LAST_ERROR=error;LAST_ADDRESS=cr2;}
 fault_hex(b"IndianOS: fault vector=0x\\0",vector);
 fault_hex(b"IndianOS: fault error=0x\\0",error);
 fault_hex(b"IndianOS: fault CR2=0x\\0",cr2);
 match vector{
  14=>log(b"IndianOS: isolated ring-3 page fault\n\0"),
  13=>log(b"IndianOS: isolated ring-3 general protection fault\n\0"),
  _=>log(b"IndianOS: isolated ring-3 invalid opcode\n\0")
 }
 unsafe{x86_64_user_exit_to_kernel(0x100+vector)}
}
#[no_mangle]
pub unsafe extern "C" fn x86_64_user_syscall_dispatch(regs:*const u64)->u64{
 let img=active_image();
 if img.is_null()||regs.is_null(){return BAD;}
 let r=unsafe{&*(regs as *const [u64; WORDS])};
 if !ring3_frame_valid(r,unsafe{&(*img).plan},cfg!(any(feature="preempt",feature="test-irq-keyboard"))){return BAD;}
 // SYS_EXIT and SYS_WRITE must originate from a mapped user executable
 // instruction, not a writable or supervisor-only page.
 let mut source=Mapping{
  physical:0,permissions:Permissions{writable:false,executable:false,user:false}
 };
 if !unsafe{vmm_query_page(&(*img).tables,r[15]&!(PAGE-1),&mut source)} ||
    !source.permissions.user||!source.permissions.executable||
    source.permissions.writable{return BAD;}
 let request=SyscallRequest{number:r[14],arg0:r[9],arg1:r[10]};
 if !unsafe{indianos_syscall_validate(ptr::addr_of!((*img).plan),request)}{return BAD;}
 match request.number{
  #[cfg(feature="test-r029-userspace")]
  18=>{
   let Some(pid)=authenticated_gui_pid()else{return BAD;};
   let mut command=[0u8;64];
   if !unsafe{indianos_user_copy_readonly(img,request.arg0,
       command.as_mut_ptr().cast(),command.len())}{return BAD;}
   if crate::gui::user_surface_submit(pid,&command){0}else{BAD}
  },
  #[cfg(feature="test-r029-userspace")]
  19=>{
   let Some(pid)=authenticated_gui_pid()else{return BAD;};
   if crate::gui::user_surface_focus(pid,request.arg0 as u32){0}else{BAD}
  },
  #[cfg(feature="test-r029-userspace")]
  20=>{
   let Some(pid)=authenticated_gui_pid()else{return BAD;};
   crate::gui::user_surface_status(pid).unwrap_or(BAD)
  },
  #[cfg(feature="test-r031-supervisor")]
  26=>{
   let Some(pid)=authenticated_gui_pid()else{return BAD;};
   if pid!=1{return BAD;}
   let state=unsafe{SUPERVISED_TERMINAL.active_pid()};
   state.unwrap_or(0) as u64
  },
  #[cfg(feature="test-r031-boot-desktop")]
  25=>{
   let Some(pid)=authenticated_gui_pid()else{return BAD;};
   crate::gui::user_desktop_pump(pid).unwrap_or(BAD)
  },
  #[cfg(feature="test-r029-desktop-suite")]
  24=>{
   let Some(pid)=authenticated_gui_pid()else{return BAD;};
   if crate::gui::user_desktop_action(pid,request.arg0 as u32){0}else{BAD}
  },
  #[cfg(feature="test-r029-frame-server")]
  22=>{
   let Some(pid)=authenticated_gui_pid()else{return BAD;};
   crate::gui::user_frame_state(pid).unwrap_or(BAD)
  },
  #[cfg(feature="test-r029-frame-server")]
  23=>{
   let Some(pid)=authenticated_gui_pid()else{return BAD;};
   if crate::gui::user_frame_present(pid,request.arg0 as u32){0}else{BAD}
  },
  #[cfg(feature="test-r029-userspace")]
  21=>{
   let Some(pid)=authenticated_gui_pid()else{return BAD;};
   // Validate writable destination before peek; copy before acknowledge.
   if !unsafe{indianos_user_can_write(&*img,request.arg0,16)}{return BAD;}
   let Some(event)=crate::gui::user_surface_peek(pid)else{return 0;};
   let wire=event.wire();
   if !unsafe{indianos_user_copy_writable(&*img,request.arg0,
       wire.as_ptr(),wire.len())}{return BAD;}
   if crate::gui::user_surface_ack(pid,event.sequence){16}else{BAD}
  },
  #[cfg(feature="test-r025")]
  10=>{
   let Some(pid)=authenticated_ipc_pid()else{return BAD;};
   services().and_then(|svc|svc.token(pid,request.arg0).ok()).unwrap_or(BAD)
  },
  #[cfg(feature="test-r025")]
  14=>{
   let Some(pid)=authenticated_ipc_pid()else{return BAD;};
   if services().is_some_and(|svc|svc.revoke(pid,request.arg0).is_ok()){
     0
   }else{BAD}
  },
  #[cfg(feature="test-r025")]
  15=>{
   let Some(_caller)=authenticated_ipc_pid()else{return BAD;};
   let index=(request.arg0-1) as usize; // numeric syscall validator bounds 1..=2
   let task=unsafe{&*task_ptr(index)};
   let Some(pcb)=(unsafe{task.pcb.as_ref()})else{return BAD;};
   if pcb.pid as u64!=request.arg0{return BAD;}
   pcb.state as u64
  },
  #[cfg(feature="test-r025")]
  11..=13=>{
   let Some(pid)=authenticated_ipc_pid()else{return BAD;};
   let mut desc=[0u8;48];
   if !unsafe{indianos_user_copy_readonly(img,request.arg0,
       desc.as_mut_ptr().cast(),desc.len())}{return BAD;}
   let token=u64::from_le_bytes(desc[0..8].try_into().unwrap());
   // Fixed-size, NUL-terminated pathname in the syscall descriptor.
   let Some(path_len)=desc[8..32].iter().position(|&x|x==0)else{return BAD;};
   if path_len==0 || desc[8+path_len+1..32].iter().any(|&x|x!=0){
       return BAD;
   }
   let Ok(path)=core::str::from_utf8(&desc[8..8+path_len])else{return BAD;};
   let address=u64::from_le_bytes(desc[32..40].try_into().unwrap());
   let length=u64::from_le_bytes(desc[40..48].try_into().unwrap());
   let Some(svc)=services()else{return BAD;};
   match request.number {
    11=>{
     if length==0 || length>MAX_FILE_TRANSFER as u64{return BAD;}
     let mut data=[0u8;MAX_FILE_TRANSFER];
     if !unsafe{indianos_user_copy_readonly(img,address,
          data.as_mut_ptr().cast(),length as usize)}{return BAD;}
     if svc.create(pid,token,path,&data[..length as usize]).is_ok(){0}else{BAD}
    }
    12=>{
     if length==0 || length>MAX_FILE_TRANSFER as u64 ||
        !unsafe{indianos_user_can_write(&*img,address,length as usize)}{
        return BAD;
     }
     let mut output=[0u8;MAX_FILE_TRANSFER];
     let Ok(len)=svc.read(pid,token,path,&mut output[..length as usize])
       else{return BAD;};
     if !unsafe{indianos_user_copy_writable(&*img,address,
          output.as_ptr(),len)}{return BAD;}
     len as u64
    }
    13=>{
     if address!=0 || length!=0{return BAD;}
     if svc.delete(pid,token,path).is_ok(){0}else{BAD}
    }
    _=>BAD,
   }
  },
  #[cfg(feature="test-ipc")]
  8=>{
   // Wire packet: destination u32 little-endian, then 0..64 payload bytes.
   // User cannot specify sender; we authenticate against the active PCB.
   let Some(sender)=authenticated_ipc_pid()else{return BAD;};
   let mut packet=[0u8;76];
   let length=request.arg1 as usize;
   if !unsafe{indianos_user_copy_readonly(img,request.arg0,
        packet.as_mut_ptr().cast(),length)}{return BAD;}
   let target=u32::from_le_bytes(packet[..4].try_into().unwrap());
   #[cfg(feature="test-r025")]
   {
    let token=u64::from_le_bytes(packet[4..12].try_into().unwrap());
    if services().is_none_or(|svc|
        svc.authorize_send(sender,token,target).is_err()){
        return BAD;
    }
   }
   let Some(boxes)=router()else{return BAD;};
   #[cfg(feature="test-r025")]
   let payload=&packet[12..length];
   #[cfg(not(feature="test-r025"))]
   let payload=&packet[4..length];
   match boxes.send(sender,target,payload){
    Ok(())=>0, Err(_)=>BAD,
   }
  },
  #[cfg(feature="test-ipc")]
  9=>{
   // Out: sender u32 + payload length u32 + 64 zero-padded bytes.
   // Check *entire* writable buffer BEFORE removing any queued message.
   let Some(receiver)=authenticated_ipc_pid()else{return BAD;};
   if !unsafe{indianos_user_can_write(&*img,request.arg0,72)}{
       return BAD;
   }
   let Some(boxes)=router()else{return BAD;};
   let msg=match boxes.receive(receiver){
      Ok(m)=>m,Err(IpcError::Empty)=>return 0,Err(_)=>return BAD,
   };
   let mut response=[0u8;72];
   response[..4].copy_from_slice(&msg.sender().to_le_bytes());
   response[4..8].copy_from_slice(&(msg.payload().len() as u32).to_le_bytes());
   let len=msg.payload().len();
   response[8..8+len].copy_from_slice(msg.payload());
   if !unsafe{indianos_user_copy_writable(&*img,request.arg0,
       response.as_ptr(),response.len())}{return BAD;}
   (8+len) as u64
  },
  7=>{
   // Four little-endian u64 words: operation, buffer, length, job ID.
   // Descriptor and payload are copied from verified user-owned pages.
   let pcb=unsafe{ptr::read(ptr::addr_of!(ACTIVE_PROCESS))};
   let Some(process)=(unsafe{pcb.as_ref()})else{return BAD;};
   if process.pid==0 || process.state!=3{return BAD;}
   let mut descriptor=[0u8;32];
   if !unsafe{indianos_user_copy_readonly(img,request.arg0,
       descriptor.as_mut_ptr().cast(),descriptor.len())}{return BAD;}
   let word=|n:usize|u64::from_le_bytes(
       descriptor[n*8..n*8+8].try_into().unwrap());
   let Ok(q)=crate::quantum_abi::QuantumRequest::parse(
       word(0),word(1),word(2),word(3))else{return BAD;};
   if crate::quantum_abi::validate_user_request(unsafe{&(*img).plan},q).is_err(){
       return BAD;
   }
   use crate::quantum_abi::QuantumOperation;
   use crate::quantum_dispatch::{dispatch,Reply};
   use crate::quantum_queue::RESULT_BYTES;
   let jobs=unsafe{&mut *ptr::addr_of_mut!(QUANTUM_JOBS)};
   let reply=match q.operation{
    QuantumOperation::Submit=>{
     let mut bytes=[0u8;crate::quantum_protocol::MAX_REQUEST_BYTES];
     if !unsafe{indianos_user_copy_readonly(img,q.user_address,
         bytes.as_mut_ptr().cast(),q.length)}{return BAD;}
     dispatch(jobs,process.pid,q,Some(&bytes[..q.length]),None)
    }
    QuantumOperation::Result=>{
     if !unsafe{indianos_user_can_write(&*img,q.user_address,RESULT_BYTES)}{
         return BAD;
     }
     let mut out=[0u8;RESULT_BYTES];
     let result=dispatch(jobs,process.pid,q,None,Some(&mut out));
     if result.is_err(){return BAD;}
     if !unsafe{indianos_user_copy_writable(&*img,q.user_address,
         out.as_ptr(),out.len())}{return BAD;}
     result
    }
    _=>dispatch(jobs,process.pid,q,None,None),
   };
   match reply{
    Ok(Reply::Submitted(id))=>id,
    Ok(Reply::Status(crate::quantum_queue::JobStatus::Queued))=>0,
    Ok(Reply::Status(crate::quantum_queue::JobStatus::Completed))=>1,
    Ok(Reply::Status(crate::quantum_queue::JobStatus::Failed(_)))=>2,
    Ok(Reply::Result|Reply::Released)=>0,
    Err(_)=>BAD,
   }
  },
  0=>1,
  #[cfg(feature="test-interactive")]
  6=>{
   #[cfg(feature="test-r029-multiprocess")]
   if authenticated_gui_pid().is_none(){return BAD;}
   #[cfg(not(feature="test-r029-multiprocess"))]
   if unsafe{ACTIVE_PID}!=1{return BAD;}
   if !unsafe{indianos_user_can_write(&*img,request.arg0,1)}{return BAD;}
   let Some(byte)=input::poll_byte()else{return 0;};
   if !unsafe{indianos_user_copy_writable(&*img,request.arg0,&byte,1)}{
       return BAD;
   }
   input::echo(byte);
   1
  },
  #[cfg(feature="test-shell")]
  4=>unsafe{ACTIVE_PID as u64},
  #[cfg(all(feature="test-ipc",not(feature="test-shell")))]
  4=>authenticated_ipc_pid().map_or(BAD,u64::from),
  #[cfg(all(feature="test-shell",feature="test-r026"))]
  5=>{
   // PID1 is the only service manager. Grant exactly one worker lifetime.
   if unsafe{ACTIVE_PID}!=1 || !authenticated_r026_init(){return BAD;}
   let worker=unsafe{&mut *task_ptr(1)};
   let Some(p)=(unsafe{worker.pcb.as_mut()})else{return BAD;};
   #[cfg(feature="test-r030-ondevice")]
   if !crate::r030_ondevice::installed(){return BAD;}
   if unsafe{R026_WORKER_REQUESTED} || p.pid!=2 ||
      p.state!=1 || worker.root.root==0 || worker.saved{return BAD;}
   // Prepared image/table ownership exists but kernel held PID2 in NEW
   // until an authenticated command requested the allowlisted executable.
   p.state=2; // READY
   unsafe{R026_WORKER_REQUESTED=true;}
   log(b"IndianOS: R026 /bin/worker launch granted to PID1\n\0");
   0
  },
  #[cfg(all(feature="test-shell",not(feature="test-r026")))]
  5=>{
   unsafe{
    if ACTIVE_PID!=1||PENDING_WORKER{return BAD;}
    PENDING_WORKER=true;
   }
   log(b"IndianOS: R013 PID 1 requested /bin/worker\n\0");
   0
  },
  #[cfg(feature="test-r030-ondevice")]
  30=>{
   if !authenticated_r026_init(){return BAD;}
   // The path/buffer descriptor is not trusted merely because its *pointer*
   // is valid. Make private copies and validate every referenced user page.
   let mut descriptor=[0u8;48];
   if !unsafe{indianos_user_copy_readonly(img,request.arg1,
        descriptor.as_mut_ptr().cast(),descriptor.len())}{return BAD;}
   if descriptor[40..].iter().any(|&b|b!=0){return BAD;}
   let Some(nul)=descriptor[..24].iter().position(|&b|b==0)else{return BAD;};
   if nul<2||descriptor[nul..24].iter().any(|&b|b!=0){return BAD;}
   let Ok(path)=core::str::from_utf8(&descriptor[..nul])else{return BAD;};
   let addr=u64::from_le_bytes(descriptor[24..32].try_into().unwrap());
   let len=u64::from_le_bytes(descriptor[32..40].try_into().unwrap());
   let mut copy=[0u8;crate::r030_ondevice::MAX_TRANSFER];
   match request.arg0{
    1|3=>{
     if len==0||len>copy.len() as u64||
        !unsafe{indianos_user_copy_readonly(img,addr,copy.as_mut_ptr().cast(),len as usize)}{
        return BAD;
     }
     crate::r030_ondevice::operation(request.arg0,path,&mut copy,len as usize)
       .map(|_|0).unwrap_or(BAD)
    }
    2=>{
     if len==0||len>copy.len() as u64||
        !unsafe{indianos_user_can_write(&*img,addr,len as usize)}{return BAD;}
     let Ok(read)=crate::r030_ondevice::operation(2,path,&mut copy,0)else{return BAD;};
     if read>len as usize || !unsafe{indianos_user_copy_writable(
         &*img,addr,copy.as_ptr(),read)}{return BAD;}
     read as u64
    }
    4=>{
     if addr!=0||len!=0{return BAD;}
     crate::r030_ondevice::operation(4,path,&mut copy,0)
       .map(|_|0).unwrap_or(BAD)
    }
    _=>BAD
   }
  },
  #[cfg(feature="test-r030-ondevice")]
  31=>{
   if !authenticated_r026_init(){return BAD;}
   crate::r030_ondevice::package(request.arg0).unwrap_or(BAD)
  },
  #[cfg(feature="test-r026")]
  16=>{
   if !authenticated_r026_init(){return BAD;}
   let worker=unsafe{&*task_ptr(1)};
   let Some(p)=(unsafe{worker.pcb.as_ref()})else{return BAD;};
   if p.pid!=2{return BAD;}
   p.state as u64
  },
  #[cfg(feature="test-r026")]
  17=>{
   // Non-blocking wait: 0 if not reaped yet; 1 after exit and cleanup.
   if !authenticated_r026_init(){return BAD;}
   if unsafe{R026_WORKER_REAPED}{1}else{0}
  },
  2=>unsafe{x86_64_user_exit_to_kernel(request.arg0)},
  3=>{
   if !capture(regs,false){return BAD;}
   unsafe{x86_64_user_exit_to_kernel(YIELD)}
  }
  1=>{
   #[cfg(feature="preempt")]
   unsafe{
    if !REPORTED_IF{
     if r[17]&0x200!=0 {
      log(b"IndianOS: M009 user IF=1 (hardware IRQs permitted)\n\0")
     }else{
      log(b"IndianOS: M009 user IF=0 (hardware IRQs masked)\n\0")
     }
     REPORTED_IF=true;
    }
   }
   let mut copied=[0u8;257];
   if !unsafe{indianos_user_copy_readonly(img,request.arg0,
         copied.as_mut_ptr().cast(),request.arg1 as usize)}{return BAD;}
   copied[request.arg1 as usize]=0;
   log(b"IndianOS user: \0");
   unsafe{serial_write(copied.as_ptr().cast())};
   log(b"\n\0");
   #[cfg(feature="test-irq-keyboard")]
   crate::console::write(&copied[..request.arg1 as usize]);
   request.arg1
  }
  _=>BAD
 }
}
fn allowed_frame(p:u64)->bool{
 if p==0||p&(PAGE-1)!=0||p.checked_add(PAGE).is_none(){return false;}
 let n=unsafe{boot_memory_region_count()};
 if n==0||n>4096{return false;}
 for i in 0..n{
  let mut r=Region{base:0,length:0,kind:0};
  if !unsafe{boot_memory_region_at(i,&mut r,ptr::null_mut())}{return false;}
  let Some(end)=r.base.checked_add(r.length)else{return false};
  if p>=r.base&&p.checked_add(PAGE).is_some_and(|e|e<=end){
   return matches!(r.kind,0|5|6|8);
  }
 }
 false
}
unsafe extern "C" fn allocate(p:*mut u64,_:*mut c_void)->bool{unsafe{pmm_allocate_frame(p)}}
unsafe extern "C" fn release(p:u64,_:*mut c_void)->bool{unsafe{pmm_release_frame(p)}}
unsafe extern "C" fn access(p:u64,context:*mut c_void)->*mut c_void{
 let Some(m)=(unsafe{(context as *const DirectMap).as_ref()})else{return ptr::null_mut()};
 if !allowed_frame(p){return ptr::null_mut();}
 let Some(addr)=p.checked_add(m.offset)else{return ptr::null_mut()};
 if addr>>48!=0xffff||addr&(1u64<<47)==0{return ptr::null_mut();}
 addr as usize as *mut c_void
}
unsafe extern "C" fn table_access(p:u64,context:*mut c_void)->*mut u64{
 unsafe{access(p,context)}.cast()
}
fn vmm_backend(m:&mut DirectMap)->VmmBackend{
 VmmBackend{allocate:Some(allocate),release:Some(release),access:Some(access),
 context:(m as *mut DirectMap).cast()}
}
fn clone_backend(m:&mut DirectMap)->CloneBackend{
 CloneBackend{allocate:Some(allocate),release:Some(release),access:Some(table_access),
 context:(m as *mut DirectMap).cast()}
}
fn lookup(b:&CloneBackend,root:u64,va:u64)->Option<(u64,u64)>{
 let(mut phys,mut flags)=(0,0);
 unsafe{paging_lookup(b,root,va,&mut phys,&mut flags)}.then_some((phys,flags))
}
fn kernel_mapped(b:&CloneBackend,original:u64,clone:u64,va:u64)->bool{
 if va&(1<<47)==0{return false;}
 let root=unsafe{(b.access.unwrap())(clone,b.context)};
 let index=((va>>39)&511) as usize;
 if root.is_null()||index<256{return false;}
 if unsafe{ptr::read(root.add(index))}&5!=1{return false;}
 match (lookup(b,original,va),lookup(b,clone,va)){
  (Some((a,af)),Some((z,zf)))=>a==z&&(af&1!=0)&&(zf&1!=0),
  _=>false
 }
}
fn trap_mapped(b:&CloneBackend,original:u64,clone:u64)->bool{
 let mut trap=[0u64;5];
 if !unsafe{x86_64_user_trap_mapping_addresses(trap.as_mut_ptr())}{return false;}
 trap.iter().all(|&p|kernel_mapped(b,original,clone,p))
}
fn image_mapped(b:&CloneBackend,root:u64,img:&UserImage)->bool{
 if !img.ready||img.page_count==0||img.page_count as usize>img.pages.len()||
    lookup(b,root,img.plan.guard_begin).is_some(){return false;}
 // Every staged user frame must appear in the cloned CR3 with its exact
 // owned physical address and expected W^X/NX rights, not just entry/stack.
 for page in img.pages.iter().take(img.page_count as usize){
  let va=page.virtual_address;
  let stack=va>=img.plan.stack_begin&&va<img.plan.stack_end;
  let segment=img.plan.segments.iter()
    .take(img.plan.segment_count as usize)
    .find(|s|va>=s.page_begin&&va<s.page_end);
  let (write,execute)=if stack{(true,false)}
  else if let Some(s)=segment{(s.writable,s.executable)}
  else{return false;};
  if write&&execute{return false;}
  let Some((pa,flags))=lookup(b,root,va)else{return false};
  if pa!=page.physical_address||flags&1==0||flags&4==0||
     (flags&2!=0)!=write||(flags&(1u64<<63)==0)!=execute{return false;}
  let mut mapping=Mapping{physical:0,permissions:Permissions{
   writable:false,executable:false,user:false}};
  if !unsafe{vmm_query_page(&img.tables,va,&mut mapping)}||
     mapping.physical!=pa||!mapping.permissions.user||
     mapping.permissions.writable!=write||
     mapping.permissions.executable!=execute{return false;}
 }
 true
}
fn preflight()->Option<(CpuState,u64)>{
 let mut cpu=CpuState::default();let mut offset=0;
 if !unsafe{x86_64_paging_detect(&mut cpu)}{
  log(b"IndianOS: ring-3 preflight failed: CPU paging probe\n\0");return None;
 }
 if !unsafe{paging_preflight(&cpu)}{
  log(b"IndianOS: ring-3 preflight failed: NXE/WP/PAE/LA57 or CR3\n\0");return None;
 }
 if cpu.pcid_enabled {
  log(b"IndianOS: ring-3 preflight failed: unsupported PCID mode\n\0");return None;
 }
 if !unsafe{boot_hhdm_offset(&mut offset)} ||offset==0{
  log(b"IndianOS: ring-3 preflight failed: missing HHDM\n\0");return None;
 }
 Some((cpu,offset))
}
#[no_mangle]
pub unsafe extern "C" fn x86_64_user_process_run(
 process:*mut Process,elf:*const u8,size:usize)->bool{
 let Some(p)=(unsafe{process.as_ref()})else{return false};
 let pid=p.pid;
 if pid==0||p.state!=1||!active_image().is_null(){return false;}
 #[cfg(feature="test-shell")]
 unsafe{
  if ACTIVE_PID!=0{return false;}
  if pid==1{PENDING_WORKER=false;}
 }
 let Some((cpu,offset))=preflight()else{return false};
 if !unsafe{x86_64_user_interrupt_stack_enable()}{
  log(b"IndianOS: ring-3 preflight failed: TSS RSP0\n\0");return false;
 }
 log(b"IndianOS: ring-3 paging prerequisites verified\n\0");
 let mut mem=DirectMap{offset};
 let vmm=vmm_backend(&mut mem);
 let backend=clone_backend(&mut mem);
 let before=unsafe{pmm_free_frames()};
 let mut staged:UserImage=unsafe{core::mem::zeroed()};
 let mut clone=CloneResult{root:0,table_count:0};
 if !unsafe{indianos_user_image_load(&mut staged,&vmm,elf,size)}{
  log(b"IndianOS: ring-3 failed: ELF segment staging\n\0");return false;
 }
 log(b"IndianOS: ring-3 user image staged\n\0");
 if !unsafe{indianos_process_ready(process)}{
  let _=unsafe{indianos_user_image_destroy(&mut staged)};return false;
 }
 pid_log(pid,b"READY\0");
 let original=cpu.cr3&PHYS;
 let cloned=unsafe{paging_clone_kernel_half(&backend,original,&mut clone)};
 if !cloned{log(b"IndianOS: ring-3 failed: supervisor mapping clone\n\0");}
 let mut grafted=false;
 let mut success=false;
 if cloned&&unsafe{paging_attach_user_half(&backend,clone.root,staged.tables.root_physical)}{
  grafted=true;
  log(b"IndianOS: ring-3 user page tables attached\n\0");
  let stack:u64;unsafe{asm!("mov {}, rsp",out(reg)stack,options(nostack,nomem,preserves_flags))}
  let addresses=[
   x86_64_user_demo_run as usize as u64,
   x86_64_enter_user as usize as u64,
   x86_64_user_interrupt as usize as u64,
   x86_64_user_exit_to_kernel as usize as u64,
   x86_64_user_syscall_dispatch as usize as u64,
   ptr::addr_of!(ACTIVE_USER) as usize as u64,stack,
  ];
  let mut safe=trap_mapped(&backend,original,clone.root)&&
   addresses.iter().all(|&a|kernel_mapped(&backend,original,clone.root,a))&&
   image_mapped(&backend,clone.root,&staged);
  if !safe{log(b"IndianOS: ring-3 mapping preflight rejected unsafe address space\n\0");}
  if safe&&!unsafe{indianos_process_start(process,clone.root)}{
   log(b"IndianOS: ring-3 failed: PCB state transition\n\0");safe=false;
  }
  if safe{
   pid_log(pid,b"RUNNING\0");clear_faults();
   unsafe{ACTIVE_USER=&mut staged;ACTIVE_PROCESS=process;}
   #[cfg(feature="test-shell")]
   unsafe{ACTIVE_PID=pid;}
   log(b"IndianOS: entering ring-3 /bin/init (QEMU experimental)\n\0");
   let status=unsafe{x86_64_enter_user(staged.plan.entry,staged.plan.stack_end-16,clone.root)};
   unsafe{ACTIVE_USER=ptr::null_mut();ACTIVE_PROCESS=ptr::null_mut();}
   #[cfg(feature="test-shell")]
   unsafe{ACTIVE_PID=0;}
   let (v,e,a)=last_fault();
   cleanup_quantum_jobs(pid);
   #[cfg(feature="test-r029-userspace")]
   crate::gui::user_surface_terminate(pid);
   let completed=if v==0{unsafe{indianos_process_exit(process,status)}}
   else{unsafe{indianos_process_fault(process,status,v,e,a)}};
   if !completed{
    log(b"IndianOS: FATAL PCB terminal state rejected\n\0");
    }else{
   #[cfg(feature="test-r030-ondevice")]
   fault_hex(b"IndianOS: R030 ring3 exit code=0x\0",status);
   pid_log(pid,if v==0{&b"EXITED\0"[..]}else{&b"FAULTED\0"[..]});
   #[cfg(any(feature="test-fault-guard",feature="test-fault-supervisor"))]
   {
    #[cfg(feature="test-fault-guard")]
    let (expected,present)=(USER_TOP-STACK_BYTES-PAGE_BYTES,false);
    #[cfg(feature="test-fault-supervisor")]
    let (expected,present)=(0xffff_ffff_8000_0000u64,true);
    success=status==0x100+14&&v==14&&a==expected&&(e&4)!=0&&
      ((e&1)!=0)==present&&(e&8)==0;
    if success{log(b"IndianOS: user page fault contained; kernel resumed\n\0")}
    else{log(b"IndianOS: user page fault validation failed\n\0")}
   }
   #[cfg(not(any(feature="test-fault-guard",feature="test-fault-supervisor")))]
   {
    success=status==0&&v==0;
    if success{log(b"IndianOS: /bin/init exited through int 0x80\n\0");}
   }
    }
  }
 }
 if grafted&&!unsafe{paging_detach_user_half(&backend,clone.root,staged.tables.root_physical)}{
  log(b"IndianOS: ring-3 cleanup failed: user PML4 detach\n\0");return false;
 }
 log(b"IndianOS: ring-3 borrowed user tables detached\n\0");
 if clone.root!=0&&!unsafe{paging_clone_release(&backend,&mut clone)}{
  log(b"IndianOS: ring-3 cleanup failed: kernel clone release\n\0");return false;
 }
 log(b"IndianOS: ring-3 supervisor clone released\n\0");
 if !unsafe{indianos_user_image_destroy(&mut staged)}{
  log(b"IndianOS: ring-3 cleanup failed: user image destroy\n\0");return false;
 }
 log(b"IndianOS: ring-3 user image released\n\0");
 if unsafe{pmm_free_frames()}!=before{
  log(b"IndianOS: ring-3 cleanup failed: PMM frame count mismatch\n\0");return false;
 }
 log(b"IndianOS: ring-3 PMM frame count restored\n\0");
 if !unsafe{indianos_process_reap(process)}{
  log(b"IndianOS: ring-3 failed: PCB reap transition\n\0");return false;
 }
 pid_log(pid,b"REAPED\0");
 success
}
#[no_mangle]
pub unsafe extern "C" fn x86_64_user_demo_run(elf:*const u8,size:usize)->bool{
 let mut table=ProcessTable::empty();
 if !unsafe{indianos_process_table_init(&mut table)}{return false;}
 let p=unsafe{indianos_process_create(&mut table)};
 !p.is_null()&&unsafe{x86_64_user_process_run(p,elf,size)}
}
fn task_ptr(i:usize)->*mut Task{
 unsafe{ptr::addr_of_mut!(M009_TASKS).cast::<Task>().add(i)}
}
fn prepare(t:*mut Task,p:*mut Process,elf:*const u8,size:usize,original:u64,
 vmm:&VmmBackend,b:&CloneBackend,slot:usize)->bool{
 let task=unsafe{&mut *t};
 if p.is_null()||!unsafe{x86_64_user_interrupt_stack_select(slot as u32)} ||
    !unsafe{indianos_user_image_load(&mut task.image,vmm,elf,size)}{return false;}
 task.pcb=p;
 if !unsafe{paging_clone_kernel_half(b,original,&mut task.root)} ||
    !unsafe{paging_attach_user_half(b,task.root.root,task.image.tables.root_physical)}{return false;}
 task.attached=true;
 let addresses=[
  x86_64_enter_user as usize as u64,
  x86_64_resume_user as usize as u64,
  x86_64_user_exit_to_kernel as usize as u64,
  x86_64_user_interrupt as usize as u64,
  x86_64_user_syscall_dispatch as usize as u64,
  task_ptr(0) as usize as u64,
  ptr::addr_of!(ACTIVE_TASK) as usize as u64,
 ];
 let safe=trap_mapped(b,original,task.root.root)&&
  addresses.iter().all(|&a|kernel_mapped(b,original,task.root.root,a))&&
  image_mapped(b,task.root.root,&task.image);
 if !safe||!unsafe{indianos_process_ready(p)} ||
    !unsafe{indianos_process_bind(p,task.root.root)}{return false;}
 pid_log(unsafe{(*p).pid},b"READY\0");true
}
fn saved_valid(t:&Task)->bool{
 if !t.saved||t.frame[16]!=0x2b||t.frame[19]!=0x33||
    t.frame[17]&0x3000!=0||t.frame[17]&2==0{return false;}
 if (t.frame[17]&0x200!=0)!=cfg!(feature="preempt"){return false;}
 let rip=t.frame[15];let rsp=t.frame[18];
 let mut map=Mapping{physical:0,permissions:Permissions{
  writable:false,executable:false,user:false}};
 (unsafe{indianos_program_user_range(&t.image.plan,rip,1,false)}) &&
 rsp>=t.image.plan.stack_begin&&rsp<=t.image.plan.stack_end&&
 (unsafe{vmm_query_page(&t.image.tables,rip&!(PAGE-1),&mut map)}) &&
 map.permissions.user&&map.permissions.executable&&!map.permissions.writable
}
fn discard(t:*mut Task,b:&CloneBackend,terminal:bool)->bool{
 let task=unsafe{&mut *t};
 if task.attached{
  if !unsafe{paging_detach_user_half(b,task.root.root,task.image.tables.root_physical)}{return false;}
  task.attached=false;
 }
 if task.root.root!=0&&!unsafe{paging_clone_release(b,&mut task.root)}{return false;}
 if task.image.tables.initialized&&!unsafe{indianos_user_image_destroy(&mut task.image)}{return false;}
 if terminal{
  if task.pcb.is_null()||!unsafe{indianos_process_reap(task.pcb)}{return false;}
  pid_log(unsafe{(*task.pcb).pid},b"REAPED\0");
 }
 true
}
#[cfg(feature="test-r029-desktop-suite")]
#[no_mangle]
pub unsafe extern "C" fn x86_64_r029_desktop_suite_run(
 a:*const u8,al:usize,b:*const u8,bl:usize,c:*const u8,cl:usize)->bool{
 if c.is_null()||cl==0{return false;}
 unsafe{THIRD_ELF=(c,cl);}
 let result=unsafe{x86_64_m009_schedule_run(a,al,b,bl)};
 unsafe{THIRD_ELF=(ptr::null(),0);}
 result
}
#[no_mangle]
pub unsafe extern "C" fn x86_64_m009_schedule_run(
 first:*const u8,first_len:usize,second:*const u8,second_len:usize)->bool{
 if first.is_null()||second.is_null()||!active_image().is_null()||
    !active_task().is_null(){return false;}
 let Some((cpu,offset))=preflight()else{return false;};
 let mut direct=DirectMap{offset};
 let vmm=vmm_backend(&mut direct);
 let backend=clone_backend(&mut direct);
 let mut table=ProcessTable::empty();
 let mut scheduler=Scheduler::empty();
 if !unsafe{indianos_process_table_init(&mut table)}{return false;}
 let p=unsafe{indianos_process_create(&mut table)};
 let q=unsafe{indianos_process_create(&mut table)};
 if p.is_null()||q.is_null()||unsafe{(*p).pid}!=1||unsafe{(*q).pid}!=2{return false;}
 #[cfg(feature="test-r029-desktop-suite")]
 let third=unsafe{indianos_process_create(&mut table)};
 #[cfg(feature="test-r029-desktop-suite")]
 if third.is_null()||unsafe{(*third).pid}!=3{return false;}
 #[cfg(feature="preempt")]
 {
  // Install UC xAPIC mapping before cloning any user CR3, otherwise
  // the interrupt handler would access an unmapped MMIO alias.
  if !unsafe{crate::timer::x86_64_xapic_install_uc_mapping()}{
   log(b"IndianOS: R024 FATAL xAPIC UC mapping failed\n\0");
   return false;
  }
  log(b"IndianOS: R024 xAPIC supervisor UC mapping installed\n\0");
 }
 let before=unsafe{pmm_free_frames()};
 for i in 0..TASKS{
  unsafe{ptr::write(task_ptr(i),core::mem::zeroed())};
 }
 #[cfg(feature="test-r031-supervisor")]
 unsafe{SUPERVISED_TERMINAL=crate::service_supervisor::TerminalSupervisor::new();}
 #[cfg(feature="test-r026")]
 unsafe{R026_WORKER_REQUESTED=false;R026_WORKER_REAPED=false;}
 let prepared_two=prepare(task_ptr(0),p,first,first_len,cpu.cr3&PHYS,&vmm,&backend,0)&&
  prepare(task_ptr(1),q,second,second_len,cpu.cr3&PHYS,&vmm,&backend,1);
 #[cfg(feature="test-r029-desktop-suite")]
 let prepared=prepared_two && {
  let (elf,len)=unsafe{THIRD_ELF};
  prepare(task_ptr(2),third,elf,len,cpu.cr3&PHYS,&vmm,&backend,2)
 };
 #[cfg(not(feature="test-r029-desktop-suite"))]
 let prepared=prepared_two;
 if !prepared||!unsafe{indianos_scheduler_init(&mut scheduler,&mut table)}{
  log(b"IndianOS: M009 FATAL cannot prepare two user contexts\n\0");
  #[cfg(feature="test-r029-desktop-suite")]
  let _=discard(task_ptr(2),&backend,false);
  let _=discard(task_ptr(1),&backend,false);
  let _=discard(task_ptr(0),&backend,false);
  return false;
 }
 #[cfg(feature="test-ipc")]
 {
  let Some(mut mailboxes)=IpcRouter::<4,8>::new().ok()else{return false;};
  let registered=mailboxes.register(1).is_ok()&&mailboxes.register(2).is_ok();
  #[cfg(feature="test-r029-desktop-suite")]
  let registered=registered&&mailboxes.register(3).is_ok();
  if !registered{
   return false;
  }
  unsafe{IPC_ROUTER=Some(mailboxes);}
  log(b"IndianOS: R025 authenticated IPC mailboxes ready\n\0");
 }
 #[cfg(feature="test-r025")]
 {
  let Some(svc)=R025Services::new()else{return false;};
  unsafe{R025_SERVICES=Some(svc);}
  log(b"IndianOS: R025 capability and isolated file services ready\n\0");
 }
 #[cfg(feature="test-r026")]
 {
  // Both ELF images have isolated mappings, but PID2 is not eligible
  // for dispatch until the init shell authorizes its launch.
  if unsafe{(*q).state}!=2{return false;}
  unsafe{(*q).state=1;}
  log(b"IndianOS: R026 worker prepared but not yet runnable\n\0");
 }
 #[cfg(not(feature="test-r029-desktop-suite"))]
 log(b"IndianOS: M009 two simultaneous user address spaces ready\n\0");
 #[cfg(feature="test-r029-desktop-suite")]
 log(b"IndianOS: R029.5 three protected ring3 address spaces READY\n\0");
 #[cfg(feature="preempt")]
 {
  if !unsafe{crate::timer::x86_64_xapic_start_qemu_100hz()}{
   log(b"IndianOS: R024 FATAL xAPIC/PIT2 calibration failed\n\0");
   let _=discard(task_ptr(1),&backend,false);
   let _=discard(task_ptr(0),&backend,false);
   return false;
  }
  log(b"IndianOS: R024 calibrated periodic xAPIC IRQ at 100 Hz\n\0");
 }
 let mut ok=true;
 while ok&&!unsafe{indianos_scheduler_all_terminal(&scheduler)}{
  let pcb=unsafe{indianos_scheduler_next(&mut scheduler)};
  if pcb.is_null(){ok=false;break;}
  let pid=unsafe{(*pcb).pid};
  if pid<1||pid>TASKS as u32{ok=false;break;}
  let slot=(pid-1) as usize;
  let task=unsafe{&mut *task_ptr(slot)};
  if task.pcb!=pcb||!unsafe{x86_64_user_interrupt_stack_select(slot as u32)}{
   ok=false;break;
  }
  if task.saved{
   if !saved_valid(task)||!unsafe{indianos_process_resume(pcb)}{ok=false;break;}
  }else if !unsafe{indianos_process_start(pcb,task.root.root)}{ok=false;break;}
  pid_log(pid,b"DISPATCHED\0");clear_faults();
  unsafe{ACTIVE_USER=&mut task.image;ACTIVE_TASK=task as *mut Task;ACTIVE_PROCESS=pcb;}
  #[cfg(any(feature="test-r026",feature="test-r029-multiprocess"))]
  unsafe{ACTIVE_PID=pid;}
  let status=if task.saved{
   unsafe{x86_64_resume_user(task.frame.as_ptr(),task.root.root)}
  }else{
   unsafe{x86_64_enter_user(task.image.plan.entry,
                             task.image.plan.stack_end-16,task.root.root)}
  };
  unsafe{ACTIVE_TASK=ptr::null_mut();ACTIVE_USER=ptr::null_mut();ACTIVE_PROCESS=ptr::null_mut();}
  #[cfg(any(feature="test-r026",feature="test-r029-multiprocess"))]
  unsafe{ACTIVE_PID=0;}
  let cr3:u64;
  unsafe{asm!("mov {}, cr3",out(reg)cr3,options(nostack,nomem,preserves_flags));}
  if cr3&PHYS!=cpu.cr3&PHYS{
   log(b"IndianOS: M009 FATAL CR3 was not restored\n\0");
   return false;
  }
  let (v,e,a)=last_fault();
  if status==YIELD&&task.saved&&v==0{
   if !unsafe{indianos_scheduler_yield(&mut scheduler,pcb)}{ok=false;break;}
   pid_log(pid,b"YIELDED\0");
   unsafe{indianos_scheduler_account_tick(&mut scheduler)};
  }else{
   if status==YIELD{ok=false;break;}
   cleanup_quantum_jobs(pid);
   #[cfg(feature="test-r029-multiprocess")]
   crate::gui::user_surface_terminate(pid);
   #[cfg(feature="test-ipc")]
   {
    // On process exit or fault all queued messages for that PID are gone.
    if router().is_none_or(|boxes|boxes.unregister(pid).is_err()){
     ok=false;break;
    }
   }
   #[cfg(feature="test-r025")]
   {
    if services().is_none_or(|svc|svc.terminate(pid)==0){
      ok=false;break;
    }
    pid_log(pid,b"CAPABILITIES_REVOKED\0");
   }
   let finished=if v!=0{
    unsafe{indianos_process_fault(pcb,status,v,e,a)}
   }else{unsafe{indianos_process_exit(pcb,status)}};
   if !finished{ok=false;break;}
   #[cfg(feature="test-r030-ondevice")]
   fault_hex(b"IndianOS: R030 scheduler exit status=0x\0",status);
   #[cfg(feature="test-r031-boot-desktop")]
   fault_hex(b"IndianOS: R031 scheduler process exit=0x\0",status);
   pid_log(pid,if v==0{&b"EXITED\0"[..]}else{&b"FAULTED\0"[..]});
   if !discard(task as *mut Task,&backend,true){ok=false;break;}
   #[cfg(feature="test-r031-supervisor")]
   if pid==3{
     use crate::service_supervisor::Decision;
     if unsafe{SUPERVISED_TERMINAL.crashed(pid,v)}!=
       (Decision::Restart{new_pid:4}){
       log(b"IndianOS: R031.2 FATAL unexpected Terminal failure\n\0");
       ok=false;break;
     }
     // The failed process has already lost its GUI grants, input events,
     // IPC mailbox, owned page tables, image frames and CR3. Do not reuse
     // PID3 or the old trap stack; PID4 is a new process generation.
     let replacement=unsafe{indianos_process_create(&mut table)};
     if replacement.is_null()||unsafe{(*replacement).pid}!=4{
       ok=false;break;
     }
     let (replacement_elf,replacement_len)=unsafe{THIRD_ELF};
     if !prepare(task_ptr(3),replacement,replacement_elf,replacement_len,
          cpu.cr3&PHYS,&vmm,&backend,3){
       ok=false;break;
     }
     if router().is_none_or(|boxes|boxes.register(4).is_err()){
       ok=false;break;
     }
     if !crate::gui::user_surface_restart_terminal(4) ||
        !unsafe{SUPERVISED_TERMINAL.prepared(4)}{
       ok=false;break;
     }
     log(b"IndianOS: R031.2 faulted Terminal PID3 was isolated and PID4 RESTARTED\n\0");
   }
   #[cfg(feature="test-r031-supervisor")]
   if pid==4 && (v!=0||status!=0){
     log(b"IndianOS: R031.2 FATAL replacement process failed (no second restart)\n\0");
     ok=false;break;
   }
   #[cfg(feature="test-r026")]
   if pid==2{
     unsafe{R026_WORKER_REAPED=true;}
     log(b"IndianOS: R026 worker exited and reaped while shell remains alive\n\0");
   }
  }
 }
 #[cfg(feature="test-r031-supervisor")]
 let third_ok=unsafe{(*third).fault_vector==6 &&
      (*third).state==5 && (*third).resources_reclaimed &&
      indianos_process_success((*task_ptr(3)).pcb) &&
      SUPERVISED_TERMINAL.ready()};
 #[cfg(all(feature="test-r029-desktop-suite",not(feature="test-r031-supervisor")))]
 let third_ok=unsafe{indianos_process_success(third)};
 #[cfg(not(feature="test-r029-desktop-suite"))]
 let third_ok=true;
 if ok&&(!unsafe{indianos_scheduler_all_terminal(&scheduler)} ||
         !unsafe{indianos_process_success(p)} ||
         !unsafe{indianos_process_success(q)} ||!third_ok||
         unsafe{pmm_free_frames()}!=before){ok=false;}
 #[cfg(feature="preempt")]
 {
  // Mask the local timer before reading counters and reclaiming memory.
  if !unsafe{crate::timer::x86_64_xapic_timer_stop()}{ok=false;}
  let irqs=crate::timer::x86_64_xapic_timer_irq_count();
  let captures=crate::timer::x86_64_xapic_timer_preemption_count();
  if irqs==0{log(b"IndianOS: R024 FATAL no xAPIC IRQs delivered\n\0");}
  if captures==0{log(b"IndianOS: R024 FATAL no ring-3 APIC captures\n\0");}
  if scheduler.yield_count<2 || scheduler.tick_count<2 ||
     unsafe{(*task_ptr(0)).timer_preemptions}==0 ||
     unsafe{(*task_ptr(1)).timer_preemptions}==0 ||
     !crate::timer::xapic_preemption_evidence(irqs,captures){ok=false;}
  if ok{log(b"IndianOS: R024 APIC timer-preemptive round-robin PASSED\n\0");}
 }
 #[cfg(all(not(feature="preempt"),not(feature="test-r026"),not(feature="test-r029-desktop-suite")))]
 {
  if scheduler.dispatch_count!=6||scheduler.yield_count!=4||
     scheduler.tick_count!=4{ok=false;}
  if ok{log(b"IndianOS: M009 round-robin cooperative switching PASSED\n\0");}
 }
 #[cfg(feature="test-r026")]
 {
  if !unsafe{R026_WORKER_REQUESTED && R026_WORKER_REAPED} ||
     scheduler.dispatch_count<3 || scheduler.yield_count<1 ||
     scheduler.tick_count<1 {ok=false;}
  if ok{log(b"IndianOS: R026 persistent interactive worker lifecycle PASSED\n\0");}
 }
 #[cfg(feature="test-r029-desktop-suite")]
 {
  if scheduler.dispatch_count<5||scheduler.yield_count<2{ok=false;}
  if ok{log(b"IndianOS: R029.5 three process lifecycle PASSED\n\0");}
 }
 #[cfg(feature="test-r031-supervisor")]
 {
  if scheduler.dispatch_count<8 || scheduler.yield_count<3{ok=false;}
  if ok{log(b"IndianOS: R031.2 bounded Terminal supervision and privileged isolation PASSED\n\0");}
 }
 #[cfg(feature="test-ipc")]
 {
  if let Some(boxes)=router(){
   #[cfg(feature="test-r031-supervisor")]
   let fourth_closed=boxes.pending(4)==Err(IpcError::UnknownReceiver);
   #[cfg(not(feature="test-r031-supervisor"))]
   let fourth_closed=true;
   #[cfg(feature="test-r029-desktop-suite")]
   let third_closed=boxes.pending(3)==Err(IpcError::UnknownReceiver);
   #[cfg(not(feature="test-r029-desktop-suite"))]
   let third_closed=true;
   if boxes.pending(1)!=Err(IpcError::UnknownReceiver)||
      boxes.pending(2)!=Err(IpcError::UnknownReceiver)||
      !third_closed||!fourth_closed{ok=false;}
  }else{ok=false;}
  unsafe{IPC_ROUTER=None;}
  if ok{log(b"IndianOS: R025 secure IPC two-process handshake PASSED\n\0");}
 }
 #[cfg(feature="test-r025")]
 {
  if let Some(svc)=services(){
   if svc.token(1,2).is_ok() || svc.token(2,1).is_ok() ||
      svc.token(1,FILE_BASE+1).is_ok() ||
      svc.token(2,FILE_BASE+2).is_ok(){ok=false;}
  }else{ok=false;}
  unsafe{R025_SERVICES=None;}
  if ok {
   log(b"IndianOS: R025 capability IPC filesystem process services PASSED\n\0");
  }
 }
 if !ok{
  log(b"IndianOS: M009 FATAL scheduler or memory ownership verification\n\0");
  // On normal dispatch failure CR3 has been restored to the kernel.
  // Reclaim both remaining images, even if their PCB state was rejected.
  let first=discard(task_ptr(0),&backend,false);
  let second=discard(task_ptr(1),&backend,false);
  #[cfg(feature="test-r029-desktop-suite")]
  let third_discard=discard(task_ptr(2),&backend,false);
  #[cfg(not(feature="test-r029-desktop-suite"))]
  let third_discard=true;
  #[cfg(feature="test-r031-supervisor")]
  let fourth_discard=discard(task_ptr(3),&backend,false);
  #[cfg(not(feature="test-r031-supervisor"))]
  let fourth_discard=true;
  if !first||!second||!third_discard||!fourth_discard||
     unsafe{pmm_free_frames()}!=before{
   log(b"IndianOS: M009 FATAL partial rollback: memory not fully reclaimed\n\0");
  }
 }
 ok
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn saved_frame_requires_correct_privileges(){
  assert_eq!(WORDS,20);
  assert_eq!(TASKS,if cfg!(feature="test-r031-supervisor"){4}
      else if cfg!(feature="test-r029-desktop-suite"){3}else{2});
  assert_eq!(core::mem::align_of::<Task>(),16);
  let mut t:Task=unsafe{core::mem::zeroed()};
  t.saved=true;t.frame[16]=0x2b;t.frame[19]=0x33;
  t.frame[17]=2;
  assert!(!saved_valid(&t));
 }
}
