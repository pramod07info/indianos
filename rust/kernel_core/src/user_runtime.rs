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
use crate::syscall::{SyscallRequest,indianos_syscall_validate,ring3_frame_valid,USER_INTERRUPTS_ENABLED,USER_TOP,STACK_BYTES,PAGE_BYTES};
use crate::program::indianos_program_user_range;
use crate::boot::Region;
use crate::user_image::{indianos_user_can_write,indianos_user_copy_writable};
#[cfg(any(feature="test-interactive",feature="normal-init"))]
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
 authority_live:bool,
 #[cfg(feature="test-r031-vfs")]
 file_generation:u64,
 #[cfg(feature="test-ipc")]
 ipc_wait:crate::ipc::ReceiveWait,
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
#[cfg(feature="test-r031-vfs")]
static mut R031_HANDLES:crate::fs_handles::FileHandles=
    crate::fs_handles::FileHandles::new();
/// Trusted process lifecycle events (single-core kernel context).
/// Events carry the authenticated PCB PID and its current file incarnation.
#[cfg(feature="test-r031-vfs")]
static mut R114_FILE_EVENTS:crate::process_events::ProcessEvents<16>=
    crate::process_events::ProcessEvents::new();
/// One-BSP, exclusive INT 0x80 local transport. Reset on session rollback.
#[cfg(feature="test-r031-files-gui")]
static mut R114_LOCAL_UDP:crate::network::EpochUdpEndpoints<2,8>=
    crate::network::EpochUdpEndpoints::new();
// The same kernel-only owner table is now consulted by real ring-3 local
// socket syscalls, ingress authorization and process retirement.
#[cfg(feature="test-r031-files-gui")]
static mut R115_UDP_OWNERS:crate::udp_owner_epoch::Registry<2>=
    crate::udp_owner_epoch::Registry::new();
/// The authenticated R120 ring-3 process and live-socket authority owner.
/// FileHandles is the trusted globally monotonic PID-generation allocator.
/// This is single-BSP state: interrupts and other CPUs must never mutate it.
#[cfg(feature="test-r031-files-gui")]
static mut R120_RETIREMENT:crate::process_retirement_txn::ProcessRetirement<2,2,16>=
    crate::process_retirement_txn::ProcessRetirement::new();
#[cfg(feature="test-r031-files-gui")]
static R120_GATE:crate::retirement_gate::RetirementGate=
    crate::retirement_gate::RetirementGate::new();
#[cfg(feature="test-r031-files-gui")]
fn r120_cpu_id()->u32{
    // A host test thread can migrate; tests use a stable synthetic BSP ID
    // and separately verify wrong-core rejection with explicit IDs.
    #[cfg(test)]{0}
    #[cfg(not(test))]{
        use core::arch::x86_64::{__cpuid,__cpuid_count};
        let max=unsafe{__cpuid(0)}.eax;
        // Use the full 32-bit x2APIC topology ID when exposed. A legacy
        // 8-bit APIC ID cannot distinguish >255 hardware processor IDs.
        if max>=0x1f {
            let topology=unsafe{__cpuid_count(0x1f,0)};
            if topology.ebx&0xffff!=0{return topology.edx;}
        }
        if max>=0xb {
            let topology=unsafe{__cpuid_count(0xb,0)};
            if topology.ebx&0xffff!=0{return topology.edx;}
        }
        (unsafe{__cpuid(1)}.ebx>>24)&255
    }
}
#[cfg(feature="test-r031-files-gui")]
fn r120_masked()->bool{
    #[cfg(test)]{true}
    #[cfg(not(test))]{
        let flags:u64;
        unsafe{asm!("pushfq; pop {}",out(reg)flags,options(preserves_flags));}
        flags&(1<<9)==0
    }
}
#[cfg(feature="test-r031-files-gui")]
fn r120_setup()->bool{
    R120_GATE.register_bsp(r120_cpu_id(),r120_masked()).is_ok()
}
#[cfg(feature="test-r031-files-gui")]
fn r120_enter()->Result<crate::retirement_gate::Permit<'static>,
                       crate::retirement_gate::GateError>{
    #[cfg(test)]{let _=r120_setup();}
    R120_GATE.try_enter(r120_cpu_id(),r120_masked())
}
#[cfg(feature="test-r031-files-gui")]
fn r120_halt()->!{
    log(b"IndianOS: FATAL R120 retirement contention, no frame reclamation\n\0");
    #[cfg(test)]panic!("R120 retirement gate/transaction refused");
    #[cfg(not(test))]loop{unsafe{asm!("cli; hlt",options(nomem,nostack));}}
}
// R116: trusted kernel issues PID2 a narrow read-only directory lease only
// after both process incarnations have been authenticated by FileHandles.
#[cfg(feature="test-r031-files-gui")]
static mut R116_SHARED_LEASES:crate::agent_03_cap_epoch::LeaseTable<2>=
    crate::agent_03_cap_epoch::LeaseTable::new();

#[cfg(feature="test-r031-files-gui")]
fn r116_prepare_shared_directory_lease(
    owner:crate::batch5_session::Identity
)->bool{
    use crate::agent_03_cap_epoch::{
        Grant,Request,TimedGrant,SHARED_DIRECTORY_RESOURCE,SHARED_DIRECTORY_READ
    };
    if owner.pid!=2||owner.generation==0{return false;}
    let init_generation=unsafe{(*task_ptr(0)).file_generation};
    let init=crate::batch5_session::Identity{pid:1,generation:init_generation};
    let handles=unsafe{&*ptr::addr_of!(R031_HANDLES)};
    if init_generation==0||!handles.owns(init)||!handles.owns(owner){
        return false;
    }
    let parent=Grant{owner:1,resource:SHARED_DIRECTORY_RESOURCE,
        rights:SHARED_DIRECTORY_READ,epoch:1,
        generation:init_generation,revoked:false};
    let issuer=Request{caller:1,resource:SHARED_DIRECTORY_RESOURCE,
        required_rights:SHARED_DIRECTORY_READ,current_epoch:1,
        current_generation:init_generation};
    let child=TimedGrant{grant:Grant{owner:2,resource:SHARED_DIRECTORY_RESOURCE,
        rights:SHARED_DIRECTORY_READ,epoch:1,
        generation:owner.generation,revoked:false},expires_at:u64::MAX};
    unsafe{(&mut *ptr::addr_of_mut!(R116_SHARED_LEASES))}
        .delegate(parent,issuer,child,0).is_ok()
}

/// Kernel-only PID1 lifecycle event acknowledgement. A user-supplied PID or
/// file descriptor cannot select a consumer; the current exact FileHandles
/// incarnation is required, and failed verification never dequeues events.
/// This queue is single-BSP syscall context, NOT interrupt- or SMP-safe.
#[cfg(feature="test-r031-vfs")]
pub(crate) fn peek_pid1_lifecycle_receipt(
    consumer:crate::batch5_session::Identity,
)->Result<crate::process_events::EventReceipt,()>{
    if consumer.pid!=1 || consumer.generation==0 ||
       !unsafe{(&*ptr::addr_of!(R031_HANDLES)).owns(consumer)}{
        return Err(());
    }
    unsafe{(&*ptr::addr_of!(R114_FILE_EVENTS))}.peek_receipt().ok_or(())
}
#[cfg(feature="test-r031-vfs")]
pub(crate) fn consume_pid1_lifecycle_receipt(
    consumer:crate::batch5_session::Identity,
    expected:crate::process_events::EventReceipt,
)->Result<crate::process_events::ProcessEvent,()>{
    if consumer.pid!=1 || consumer.generation==0 ||
       !unsafe{(&*ptr::addr_of!(R031_HANDLES)).owns(consumer)}{
        return Err(());
    }
    let queue=unsafe{&mut *ptr::addr_of_mut!(R114_FILE_EVENTS)};
    queue.acknowledge_receipt(expected).ok_or(())
}

#[cfg(feature="test-r031-vfs")]
fn authenticated_file_identity()->Option<crate::batch5_session::Identity>{
    let process=unsafe{ptr::read(ptr::addr_of!(ACTIVE_PROCESS))};
    let task=active_task();
    let image=active_image();
    if process.is_null()||task.is_null()||image.is_null(){return None;}
    let p=unsafe{&*process};
    let t=unsafe{&*task};
    if !(1..=2).contains(&p.pid)||p.state!=3||t.pcb!=process||
       unsafe{ACTIVE_PID}!=p.pid||
       !ptr::eq(&t.image as *const UserImage,image as *const UserImage){
        return None;
    }
    let owner=crate::batch5_session::Identity{pid:p.pid,generation:t.file_generation};
    unsafe{(&*ptr::addr_of!(R031_HANDLES)).owns(owner)}.then_some(owner)
}
#[cfg(feature="test-r031-vfs")]
fn authenticated_file_pid()->Option<u32>{
    authenticated_file_identity().map(|owner|owner.pid)
}
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

static mut SESSION_DIRTY:bool=false;
#[cfg(feature="preempt")]
static mut SESSION_TIMER_ARMED:bool=false;
#[cfg(any(feature="preempt",test))]
fn stop_owned_timer(armed:&mut bool,stop:impl FnOnce()->bool)->bool{
 if !*armed{return true;}
 if !stop(){return false;}
 *armed=false;true
}
#[cfg(feature="preempt")]
fn stop_session_timer()->bool{
 // Single-BSP scheduler entry/return only. Preserve ownership on failed stop
 // across guard destruction and later scheduler invocations.
 unsafe{stop_owned_timer(&mut *ptr::addr_of_mut!(SESSION_TIMER_ARMED),
  ||crate::timer::x86_64_xapic_timer_stop())}
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
 // CPU-supplied kernel stack contains 15 registers followed by 5 QWORDs.
 let src=unsafe{&*(regs as *const [u64; WORDS])};
 capture_frame(task,src,via_timer)
}
fn capture_frame(task:&mut Task,src:&[u64;WORDS],via_timer:bool)->bool{
 let Some(pcb)=(unsafe{task.pcb.as_ref()})else{return false};
 if pcb.state!=3{return false;}
 // User IF is a property of the runtime profile, including INT 0x80
 // continuations; it does not depend on whether this capture came from IRQ.
 if !ring3_frame_valid(src,&task.image.plan,USER_INTERRUPTS_ENABLED){return false;}
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
 if !ring3_frame_valid(r,unsafe{&(*img).plan},USER_INTERRUPTS_ENABLED){return BAD;}
 // SYS_EXIT and SYS_WRITE must originate from a mapped user executable
 // instruction, not a writable or supervisor-only page.
 let mut source=Mapping{
  physical:0,permissions:Permissions{writable:false,executable:false,user:false}
 };
 if !unsafe{vmm_query_page(&(*img).tables,r[15]&!(PAGE-1),&mut source)} ||
    !source.permissions.user||!source.permissions.executable||
    source.permissions.writable{return BAD;}
 let request=SyscallRequest{number:r[14],arg0:r[9],arg1:r[10]};
 // The installed-app permission mask applies in the REAL INT 0x80 trap.
 // Authenticate the PID from ACTIVE_PROCESS, never from syscall registers.
 // Normal system tasks remain unchanged when no installed app is armed.
 #[cfg(feature="test-m2-developer-alpha")]
 {
  let pcb=unsafe{ptr::read(ptr::addr_of!(ACTIVE_PROCESS))};
  let caller=unsafe{pcb.as_ref()}.filter(|p|p.state==3).map_or(0,|p|p.pid);
  if !crate::m2_syscall_guard::allow_active(
      caller,request.number,request.arg0){
   #[cfg(feature="test-m2-disk-exec")]
   log(b"IndianOS: M2 installed app privileged syscall DENIED\n\0");
   return BAD;
  }
 }
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
   // Publish before consuming. Even a failed kernel copy preserves FIFO.
   match boxes.receive_with(receiver,|message|{
    let response=message.wire();
    unsafe{indianos_user_copy_writable(&*img,request.arg0,
        response.as_ptr(),response.len())}
   }){
    Ok(message)=>(8+message.payload().len()) as u64,
    Err(IpcError::Empty)=>0,
    Err(_)=>BAD,
   }
  },
  #[cfg(feature="test-ipc")]
  35=>{
   let Some(receiver)=authenticated_ipc_pid()else{return BAD;};
   if !unsafe{indianos_user_can_write(&*img,request.arg0,
       crate::ipc::RECEIVE_BYTES)}{return BAD;}
   let Some(boxes)=router()else{return BAD;};
   match boxes.receive_with(receiver,|message|{
    let response=message.wire();
    unsafe{indianos_user_copy_writable(&*img,request.arg0,
        response.as_ptr(),response.len())}
   }){
    Ok(message)=>return (8+message.payload().len()) as u64,
    Err(IpcError::Empty)=>{},
    Err(_)=>return BAD,
   }
   // The same serialized context preflights buffers, binds the current
   // mailbox epoch and captures its exact task continuation before sleeping.
   let Ok(wait)=crate::ipc::ReceiveWait::begin(boxes,receiver,
       request.arg0,request.arg1)else{return BAD;};
   if !capture(regs,false){return BAD;}
   let task=active_task();
   if task.is_null(){return BAD;}
   unsafe{(*task).ipc_wait=wait;}
   unsafe{x86_64_user_exit_to_kernel(YIELD)}
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
  #[cfg(any(feature="test-interactive",feature="normal-init"))]
  6=>{
   #[cfg(feature="test-r029-multiprocess")]
   if authenticated_gui_pid().is_none(){return BAD;}
   #[cfg(all(not(feature="test-r029-multiprocess"),feature="test-shell"))]
   if unsafe{ACTIVE_PID}!=1{return BAD;}
   #[cfg(all(feature="normal-init",not(feature="test-shell")))]
   if unsafe{ACTIVE_PROCESS.as_ref()}.is_none_or(|p|p.pid!=1||p.state!=3){return BAD;}
   if !unsafe{indianos_user_can_write(&*img,request.arg0,1)}{return BAD;}
   #[cfg(feature="normal-init")]
   let byte=input::normal_boot_byte();
   #[cfg(not(feature="normal-init"))]
   let byte=input::poll_byte();
   let Some(byte)=byte else{return 0;};
   if !unsafe{indianos_user_copy_writable(&*img,request.arg0,&byte,1)}{
       return BAD;
   }
   #[cfg(not(feature="normal-init"))]
   input::echo(byte);
   1
  },
  #[cfg(feature="test-shell")]
  4=>unsafe{ACTIVE_PID as u64},
  #[cfg(all(feature="test-ipc",not(feature="test-shell")))]
  4=>authenticated_ipc_pid().map_or(BAD,u64::from),
  #[cfg(all(feature="normal-init",not(any(feature="test-shell",feature="test-ipc"))))]
  4=>unsafe{ACTIVE_PROCESS.as_ref()}.filter(|p|p.state==3).map_or(BAD,|p|p.pid as u64),
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
  #[cfg(feature="test-r031-vfs")]
  32=>{
   if !authenticated_r026_init(){return BAD;}
   // Descriptor: path[24], new_path[24], user_buffer u64, bytes u64.
   // Copy every field before calling the block device. Reject dirty padding
   // so an attacker cannot bypass path canonicalization.
   let mut desc=[0u8;64];
   if !unsafe{indianos_user_copy_readonly(img,request.arg1,
        desc.as_mut_ptr().cast(),desc.len())}{return BAD;}
   let Some(n1)=desc[..24].iter().position(|&b|b==0)else{return BAD;};
   if (n1<2 && !(request.arg0==8 && n1==1 && desc[0]==b'/')) ||
      desc[n1..24].iter().any(|&b|b!=0){return BAD;}
   let Ok(path)=core::str::from_utf8(&desc[..n1])else{return BAD;};
   let (target,offset)=if matches!(request.arg0,7|9){
    let Some(n2)=desc[24..48].iter().position(|&b|b==0)else{return BAD;};
    if n2<2||desc[24+n2..48].iter().any(|&b|b!=0){return BAD;}
    let Ok(name)=core::str::from_utf8(&desc[24..24+n2])else{return BAD;};
    (name,0u64)
   }else if matches!(request.arg0,10|11){
    // Range ABI: new_path[0..8] is LE byte offset; remaining 16 bytes
    // MUST be zero. Reject offsets outside R027 fixed 2048-byte extent.
    if desc[32..48].iter().any(|&b|b!=0){return BAD;}
    let offset=u64::from_le_bytes(desc[24..32].try_into().unwrap());
    if offset>crate::durablefs::MAX_FILE_BYTES as u64{return BAD;}
    ("",offset)
   }else{
    if desc[24..48].iter().any(|&b|b!=0){return BAD;}
    ("",0u64)
   };
   let address=u64::from_le_bytes(desc[48..56].try_into().unwrap());
   let length=u64::from_le_bytes(desc[56..64].try_into().unwrap());
   let mut data=[0u8;crate::hier_vfs::MAX_TRANSFER];
   match request.arg0{
    3|5|11=>{
     if length==0||length>data.len() as u64||
        !unsafe{indianos_user_copy_readonly(img,address,
                data.as_mut_ptr().cast(),length as usize)}{return BAD;}
     if request.arg0==11{
      crate::hier_vfs::range_operation(request.arg0,path,offset,&mut data,
                                      length as usize).map(|n|n as u64).unwrap_or(BAD)
     }else{
      crate::hier_vfs::operation(request.arg0,path,target,&mut data,length as usize)
           .map(|_|0).unwrap_or(BAD)
     }
    }
    4|8|10=>{
     if length==0||length>data.len() as u64||
        (request.arg0==10 && length>72){return BAD;}
     // Read/list has no side effects. Stage all bytes privately first,
     // then preflight the ACTUAL copy size. The preexisting ring3 copy-out
     // ABI is currently limited to 72 bytes per syscall.
     let result=if request.arg0==10{
      crate::hier_vfs::range_operation(request.arg0,path,offset,
                                      &mut data[..length as usize],0)
     }else{crate::hier_vfs::operation(request.arg0,path,target,&mut data,0)};
     let Ok(n)=result else{return BAD;};
     if n>length as usize || n>72{return BAD;}
     if n>0 && (!unsafe{indianos_user_can_write(&*img,address,n)} ||
        !unsafe{indianos_user_copy_writable(&*img,address,data.as_ptr(),n)}){
       return BAD;
     }
     n as u64
    }
    1|2|6|7|9=>{
     if address!=0||length!=0{return BAD;}
     crate::hier_vfs::operation(request.arg0,path,target,&mut data,0)
         .map(|_|0).unwrap_or(BAD)
    }
    _=>BAD
   }
  },
  #[cfg(feature="test-r031-vfs")]
  33=>{
   // Serialize real GUI-profile filesystem access with the same privileged
   // admission permit as generation-owned UDP and scheduler retirement.
   // A nested IRQ/core cannot remount or use a half-retired owner.
   #[cfg(feature="test-r031-files-gui")]
   let Ok(_filesystem_permit)=r120_enter()else{return BAD;};
   let Some(owner)=authenticated_file_identity()else{return BAD;};
   // Descriptor: path[24], opaque handle u64, argument u64,
   // pointer u64, length u64, reserved u64. Never dereference an
   // untrusted ring3 pointer outside validated copy helpers.
   let mut desc=[0u8;64];
   if !unsafe{indianos_user_copy_readonly(img,request.arg1,
                desc.as_mut_ptr().cast(),desc.len())}{return BAD;}
   if desc[56..64].iter().any(|&b|b!=0){return BAD;}
   let handle=u64::from_le_bytes(desc[24..32].try_into().unwrap());
   let argument=u64::from_le_bytes(desc[32..40].try_into().unwrap());
   let address=u64::from_le_bytes(desc[40..48].try_into().unwrap());
   let length=u64::from_le_bytes(desc[48..56].try_into().unwrap());
   let path=if request.arg0==1{
    let Some(end)=desc[..24].iter().position(|&b|b==0)else{return BAD;};
    if end<2||desc[end..24].iter().any(|&b|b!=0) ||
       handle!=0||address!=0||length!=0{return BAD;}
    let Ok(s)=core::str::from_utf8(&desc[..end])else{return BAD;};
    s
   }else{
    if desc[..24].iter().any(|&b|b!=0){return BAD;}
    ""
   };
   // Validate every byte of requested writable destination before
   // changing cursor state; cannot expose kernel contents on bad pages.
   let mut data=[0u8;256];
   let size=if matches!(request.arg0,2|3){
    if length==0||length>if request.arg0==2{72}else{256} {
     return BAD;
    }
    let n=length as usize;
    if request.arg0==2{
     if !unsafe{indianos_user_can_write(&*img,address,n)}{return BAD;}
    }else if !unsafe{indianos_user_copy_readonly(
                    img,address,data.as_mut_ptr().cast(),n)}{return BAD;}
    n
   }else{
    if address!=0||length!=0{return BAD;}
    0
   };
   if matches!(request.arg0,2|3|5) && argument!=0{return BAD;}
   if matches!(request.arg0,4|6) && argument>crate::durablefs::MAX_FILE_BYTES as u64{
    return BAD;
   }
   let handles=unsafe{&mut *ptr::addr_of_mut!(R031_HANDLES)};
   // R115: the real ring-3 PID2 Files OPEN always uses an alias minted
   // from the CURRENT kernel process namespace. The user cannot provide
   // an alias or choose a wider root; all other operations still authenticate
   // their existing descriptor and incarnation before touching the device.
   let res=if request.arg0==1 && owner.pid==2 {
    match handles.mount_alias_for_current_root_kernel(owner) {
     Ok(alias)=>crate::fs_handles::operation_authenticated_mount_alias(
          handles,alias,request.arg0,owner,handle,argument,path,&mut data[..size]),
     Err(err)=>Err(err),
    }
   }else {
    crate::fs_handles::operation_authenticated(handles,request.arg0,owner,
           handle,argument,path,&mut data[..size])
   };
   let Ok(n)=res else{return BAD;};
   if request.arg0==2 && n>0{
    if n>size as u64|| !unsafe{indianos_user_copy_writable(
          &*img,address,data.as_ptr(),n as usize)}{return BAD;}
   }
   n
  },
  #[cfg(feature="test-r031-files-gui")]
  36=>{
   let Some(owner)=authenticated_file_identity()else{return BAD;};
   // Never derive authority from the claimed PID or user buffer. Source
   // identity and incarnation come from ACTIVE_PROCESS and FileHandles.
   let mut desc=[0u8;64];
   if !unsafe{indianos_user_copy_readonly(img,request.arg1,
             desc.as_mut_ptr().cast(),desc.len())}{return BAD;}
   if desc[4..8].iter().any(|&b|b!=0)||
      desc[24..].iter().any(|&b|b!=0){return BAD;}
   let claim=u32::from_le_bytes(desc[..4].try_into().unwrap());
   let source=u16::from_le_bytes(desc[8..10].try_into().unwrap());
   let destination=u16::from_le_bytes(desc[10..12].try_into().unwrap());
   let count=u32::from_le_bytes(desc[12..16].try_into().unwrap()) as usize;
   let address=u64::from_le_bytes(desc[16..24].try_into().unwrap());
   if claim!=owner.pid||
      source!=(if owner.pid==1{5501}else{5502}){return BAD;}
   let mut data=[0u8;64];
   match request.arg0{
    1|4 if destination==0&&count==0&&address==0=>{},
    2 if destination!=0&&count>0&&count<=data.len()&&
     unsafe{indianos_user_copy_readonly(img,address,data.as_mut_ptr().cast(),count)}=>{},
    3 if destination==0&&count==data.len()&&
     unsafe{indianos_user_can_write(&*img,address,count)}=>{},
    _=>return BAD
   }
   let Ok(_permit)=r120_enter()else{return BAD;};
   let socket=unsafe{&mut *ptr::addr_of_mut!(R114_LOCAL_UDP)};
   let coordinator=unsafe{&mut *ptr::addr_of_mut!(R120_RETIREMENT)};
   let live=crate::process_retirement_txn::Owner{
       pid:owner.pid,generation:owner.generation};
   match request.arg0{
    1=>coordinator.bind_live_udp(socket,live,source).map_or(BAD,|_|0),
    2=>coordinator.send_live_udp(socket,live,
           source,destination,&data[..count]).map_or(BAD,|_|0),
    3=>{
     let n=match coordinator.receive_live_udp(socket,live,source,&mut data){
      Ok(n)=>n,
      Err(crate::process_retirement_txn::RetirementError::Sockets(
          crate::network::SocketError::Empty))=>return 0,
      Err(_)=>return BAD
     };
     if n>0 && !unsafe{indianos_user_copy_writable(&*img,address,
                           data.as_ptr(),n)}{return BAD;}
     n as u64
    },
    4=>coordinator.close_live_udp(socket,live,source).map_or(BAD,|_|0),
    _=>BAD
   }
  },
  #[cfg(feature="test-r031-files-gui")]
  34=>{
   let Ok(_filesystem_permit)=r120_enter()else{return BAD;};
   if request.arg0!=1||authenticated_file_pid()!=Some(2){return BAD;}
   let Some(owner)=authenticated_file_identity()else{return BAD;};
   if !unsafe{(&*ptr::addr_of!(R116_SHARED_LEASES))}
      .permits_shared_directory(owner){return BAD;}
   // READ-ONLY directory enumeration is limited to /shared, AND a live
   // delegated read lease bound to this exact kernel-issued incarnation.
   // PID2 never gains general traversal, mutation or a private path.
   let mut desc=[0u8;64];
   if !unsafe{indianos_user_copy_readonly(img,request.arg1,
             desc.as_mut_ptr().cast(),desc.len())}{return BAD;}
   if &desc[..8]!=b"/shared\0" ||
      desc[8..48].iter().any(|&b|b!=0){return BAD;}
   let address=u64::from_le_bytes(desc[48..56].try_into().unwrap());
   let requested=u64::from_le_bytes(desc[56..64].try_into().unwrap());
   if requested==0||requested>72 ||
      !unsafe{indianos_user_can_write(&*img,address,requested as usize)}{
    return BAD;
   }
   let mut data=[0u8;72];
   let Ok(n)=crate::hier_vfs::operation(8,"/shared","",&mut data,0)
     else{return BAD;};
   if n>requested as usize{return BAD;}
   if n>0 && !unsafe{indianos_user_copy_writable(
                &*img,address,data.as_ptr(),n)}{return BAD;}
   n as u64
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
 if p.is_null(){return false;}
 task.pcb=p;task.authority_live=true;
 if !unsafe{x86_64_user_interrupt_stack_select(slot as u32)} ||
    !unsafe{indianos_user_image_load(&mut task.image,vmm,elf,size)}{return false;}
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
 #[cfg(feature="test-r031-vfs")]
 if (1..=2).contains(&unsafe{(*p).pid}){
  // Bind the real FileHandles owner and coordinator generation while holding
  // one gate. No observer may see an owner that is only half-registered.
  #[cfg(feature="test-r031-files-gui")]
  let Ok(_registration_permit)=r120_enter()else{return false;};
  // Bind only fully prepared contexts. The table and epoch allocator survive
  // scheduler invocations; never reset them when PCB PIDs restart at one.
  let handles=unsafe{&mut *ptr::addr_of_mut!(R031_HANDLES)};
  let Ok(owner)=handles.bind_process(unsafe{(*p).pid})else{return false;};
  task.file_generation=owner.generation;
  #[cfg(feature="test-r031-files-gui")]
  {
   let live=crate::process_retirement_txn::Owner{
     pid:owner.pid,generation:owner.generation
   };
   if unsafe{(&mut *ptr::addr_of_mut!(R120_RETIREMENT))}
      .start_exact(live).is_err(){return false;}
   if owner.pid==2 && !r116_prepare_shared_directory_lease(owner){
     return false;
   }
  }
 }
 pid_log(unsafe{(*p).pid},b"READY\0");true
}
#[cfg(feature="test-ipc")]
fn service_ipc_waiters()->bool{
 use crate::ipc::WaitPoll;
 let Some(boxes)=router()else{return false;};
 for slot in 0..TASKS{
  let task=unsafe{&mut *task_ptr(slot)};
  if !task.ipc_wait.active(){continue;}
  let Some(process)=(unsafe{task.pcb.as_ref()})else{return false;};
  if process.pid!=task.ipc_wait.receiver()||process.state!=2||
      !task.saved||!task.image.ready{
   task.ipc_wait.cancel();return false;
  }
  let address=task.ipc_wait.address();
  let image=&task.image;
  let result=task.ipc_wait.poll(boxes,|message|{
   // Revalidate after the wait and copy privately staged bytes before dequeue.
   let response=message.wire();
   unsafe{indianos_user_can_write(image,address,response.len())&&
    indianos_user_copy_writable(image,address,response.as_ptr(),response.len())}
  });
  match result{
   WaitPoll::Pending=>{},
   WaitPoll::Delivered(length)=>task.frame[14]=length as u64,
   WaitPoll::TimedOut=>task.frame[14]=0,
   WaitPoll::Failed(_)=>task.frame[14]=BAD,
  }
 }
 true
}
#[cfg(feature="test-ipc")]
fn next_ipc_ready(scheduler:&mut Scheduler)->*mut Process{
 // Each pass is one cooperative selection round, not an IRQ or clock tick.
 // If every task sleeps, bounded empty passes expire all deadlines without
 // dispatching user code or fabricating scheduler/hardware accounting.
 for _ in 0..=crate::ipc::MAX_WAIT_ROUNDS as usize{
  if !service_ipc_waiters(){return ptr::null_mut();}
  let mut waiting=[0u32;TASKS];
  for (slot,pid) in waiting.iter_mut().enumerate(){
   let task=unsafe{&*task_ptr(slot)};
   if task.ipc_wait.active(){*pid=task.ipc_wait.receiver();}
  }
  let process=unsafe{crate::scheduler_next_matching(scheduler,
      |p|!waiting.contains(&p.pid))};
  if !process.is_null()||waiting.iter().all(|&p|p==0){return process;}
 }
 ptr::null_mut()
}
fn saved_valid(t:&Task)->bool{
 if !t.saved||!ring3_frame_valid(&t.frame,&t.image.plan,USER_INTERRUPTS_ENABLED){
  return false;
 }
 let rip=t.frame[15];let rsp=t.frame[18];
 let mut map=Mapping{physical:0,permissions:Permissions{
  writable:false,executable:false,user:false}};
 (unsafe{indianos_program_user_range(&t.image.plan,rip,1,false)}) &&
 rsp>=t.image.plan.stack_begin&&rsp<=t.image.plan.stack_end&&
 (unsafe{vmm_query_page(&t.image.tables,rip&!(PAGE-1),&mut map)}) &&
 map.permissions.user&&map.permissions.executable&&!map.permissions.writable
}
fn revoke_task_authority(task:&mut Task){
 revoke_task_authority_with_kind(task,crate::process_events::EventKind::Exited);
}

#[cfg(feature="test-r031-vfs")]
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
enum FileRetireFailure { EventQueue, FileHandles }

/// Atomic publication of the scheduler's *real* filesystem and PID1 event
/// authorities. Stage an exit/fault notice before touching FileHandles;
/// failure to enqueue must not orphan a live handle or publish a partial
/// event. A valid single-owner FileHandles retirement cannot fail after
/// ownership is checked under the BSP admission permit, but any unexpected
/// error must be treated as fatal by the live scheduler.
///
/// A retired/replaced PID incarnation is a no-op: never revoke a new
/// generation or emit a delayed duplicate notification.
#[cfg(feature="test-r031-vfs")]
fn retire_file_owner_epoch<const E:usize>(
    files:&mut crate::fs_handles::FileHandles,
    events:&mut crate::process_events::ProcessEvents<E>,
    owner:crate::batch5_session::Identity,
    kind:crate::process_events::EventKind,
)->Result<bool,FileRetireFailure>{
    if !files.owns(owner){return Ok(false);}
    let mut staged=events.clone();
    match kind {
        crate::process_events::EventKind::Exited =>
            staged.enqueue_exit_once(owner.pid,owner.generation),
        crate::process_events::EventKind::Faulted =>
            staged.enqueue_fault_once(owner.pid,owner.generation),
        _=>return Err(FileRetireFailure::EventQueue),
    }.map_err(|_|FileRetireFailure::EventQueue)?;
    if files.retire_process_batch_kernel(&[owner])!=Ok(1){
        return Err(FileRetireFailure::FileHandles);
    }
    *events=staged;
    Ok(true)
}

/// R118-009/027/073: a bounded, fail-atomic single-BSP transaction for
/// multiple authenticated process incarnations. The real scheduler retains
/// the non-spinning R120_GATE permit throughout staging and publication.
/// This does not implement physical APIC/IPI/SMP coordination.
#[cfg(feature="test-r031-files-gui")]
fn retire_full_owner_batch<
    const C:usize,const U:usize,const K:usize,
    const P:usize,const Q:usize,const E:usize,const L:usize
>(
    files:&mut crate::fs_handles::FileHandles,
    events:&mut crate::process_events::ProcessEvents<E>,
    coordinator:&mut crate::process_retirement_txn::ProcessRetirement<C,U,K>,
    sockets:&mut crate::network::EpochUdpEndpoints<P,Q>,
    leases:&mut crate::agent_03_cap_epoch::LeaseTable<L>,
    owners:&[crate::batch5_session::Identity],
    kind:crate::process_events::EventKind,
)->Result<usize,()> {
    use crate::process_events::EventKind;
    if owners.is_empty(){return Ok(0);}
    if owners.len()>2 || !matches!(kind,EventKind::Exited|EventKind::Faulted){
        return Err(());
    }
    // Authenticate the WHOLE batch before taking any mutable live access.
    // One stale or duplicate PID must not revoke a valid earlier member.
    for (index,owner) in owners.iter().enumerate(){
        if owner.pid==0 || owner.generation==0 ||
           owners[..index].iter().any(|previous|previous.pid==owner.pid) ||
           !files.owns(*owner) ||
           coordinator.active_generation(owner.pid)!=Some(owner.generation) {
            return Err(());
        }
    }

    let mut candidate_files=files.staged_copy_kernel();
    let mut candidate_events=events.clone();
    let mut candidate_coordinator=coordinator.clone();
    let mut candidate_sockets=sockets.clone();
    let mut candidate_leases=*leases;
    let mut policy_owners=[crate::process_retirement_txn::Owner{
        pid:0,generation:0
    };2];
    let mut lease_owners=[(0u32,0u64);2];
    for (index,owner) in owners.iter().enumerate(){
        policy_owners[index]=crate::process_retirement_txn::Owner{
            pid:owner.pid,generation:owner.generation
        };
        lease_owners[index]=(owner.pid,owner.generation);
        match kind{
            EventKind::Exited=>
                candidate_events.enqueue_exit_once(owner.pid,owner.generation),
            EventKind::Faulted=>
                candidate_events.enqueue_fault_once(owner.pid,owner.generation),
            _=>return Err(()),
        }.map_err(|_|())?;
    }
    if candidate_files.retire_process_batch_kernel(owners)!=Ok(owners.len()){
        return Err(());
    }
    let identities=&policy_owners[..owners.len()];
    match kind{
        EventKind::Exited=>candidate_coordinator.retire_live_exited_batch(
            &mut candidate_sockets,identities),
        EventKind::Faulted=>candidate_coordinator.retire_live_faulted_batch(
            &mut candidate_sockets,identities),
        _=>return Err(()),
    }.map_err(|_|())?;
    candidate_leases.revoke_identity_batch_kernel(
        &lease_owners[..owners.len()]).map_err(|_|())?;

    // Internal policy notices are not the user-visible PID1 queue. Drain
    // only AFTER all live/public event notices have been staged successfully.
    while let Some(receipt)=candidate_coordinator.peek_notice(){
        if candidate_coordinator.acknowledge_notice(receipt).is_none(){
            return Err(());
        }
    }
    if !candidate_coordinator.live_sockets_consistent(&candidate_sockets){
        return Err(());
    }
    for owner in owners{
        if candidate_files.owns(*owner) ||
           candidate_coordinator.active_generation(owner.pid).is_some() ||
           candidate_sockets.pending_owned(owner.pid,owner.generation).is_ok(){
            return Err(());
        }
    }
    // No fallible operation follows. The original tables cannot be observed
    // by an untrusted peer while the scheduler's BSP/IRQ permit is held.
    *files=candidate_files;
    *events=candidate_events;
    *coordinator=candidate_coordinator;
    *sockets=candidate_sockets;
    *leases=candidate_leases;
    Ok(owners.len())
}

/// Live scheduler single-owner convenience path, backed by the same
/// multi-incarnation transaction as supervisor batch retirement.
#[cfg(feature="test-r031-files-gui")]
fn retire_full_owner_epoch<
    const C:usize,const U:usize,const K:usize,
    const P:usize,const Q:usize,const E:usize,const L:usize
>(
    files:&mut crate::fs_handles::FileHandles,
    events:&mut crate::process_events::ProcessEvents<E>,
    coordinator:&mut crate::process_retirement_txn::ProcessRetirement<C,U,K>,
    sockets:&mut crate::network::EpochUdpEndpoints<P,Q>,
    leases:&mut crate::agent_03_cap_epoch::LeaseTable<L>,
    owner:crate::batch5_session::Identity,
    kind:crate::process_events::EventKind,
)->Result<bool,()> {
    // A delayed scheduler task with a retired incarnation must never
    // remove a newer PID's network rights or emit a duplicate event.
    if !files.owns(owner){return Ok(false);}
    retire_full_owner_batch(
        files,events,coordinator,sockets,leases,&[owner],kind
    ).map(|count|count==1)
}

#[cfg(feature="test-r031-vfs")]
fn r118_file_retirement_fatal()->!{
    log(b"IndianOS: FATAL R118 filesystem/event retirement incomplete\n\0");
    #[cfg(feature="test-r031-files-gui")]
    {r120_halt()}
    #[cfg(not(feature="test-r031-files-gui"))]
    {panic!("R118 live file/event retirement denied")}
}

fn revoke_task_authority_with_kind(
 task:&mut Task,kind:crate::process_events::EventKind
){
 // This runs before any fallible release, including scheduler-abort rollback.
 // Clear the continuation even when no PCB was bound during preparation.
 task.saved=false;task.frame=[0;WORDS];task.image.ready=false;
 #[cfg(feature="test-ipc")]
 task.ipc_wait.cancel();
 if !task.authority_live{return;}
 task.authority_live=false;
 if let Some(process)=unsafe{task.pcb.as_ref()}{
  let pid=process.pid;
  cleanup_quantum_jobs(pid);
  #[cfg(feature="test-r029-multiprocess")]
  crate::gui::user_surface_terminate(pid);
  #[cfg(feature="test-ipc")]
  if let Some(boxes)=router(){let _=boxes.unregister(pid);}
  #[cfg(feature="test-r025")]
  if let Some(svc)=services(){let _=svc.terminate(pid);}
 }

 #[cfg(feature="test-r031-vfs")]
 if task.file_generation!=0{
  // Revoke before fallible page-table/frame reclamation. A retry or delayed
  // cleanup cannot revoke a replacement incarnation with the same PID.
  if !task.pcb.is_null(){
   // One non-spinning permit covers event production, capability/UDP
   // retirement, delegated leases and FileHandles revocation, including
   // all postconditions. Do NOT reacquire inside this teardown path.
   #[cfg(feature="test-r031-files-gui")]
   let _retirement_permit=r120_enter().unwrap_or_else(|_|r120_halt());
   let owner=crate::batch5_session::Identity{
    pid:unsafe{(*task.pcb).pid},generation:task.file_generation};
   // Only a previously live trusted PID2 lease makes a teardown
   // assertion meaningful. Missing/failed task preparation must not produce
   // a false QEMU "revocation succeeded" marker.
   #[cfg(feature="test-r031-files-gui")]
   let had_live_directory_lease=owner.pid==2 &&
      unsafe{(&*ptr::addr_of!(R116_SHARED_LEASES))}
        .permits_shared_directory(owner);
   #[cfg(feature="test-r031-files-gui")]
   let mut full_retirement_committed=false;
   // Do not advertise lifecycle exit/fault yet. The notice must be
   // published atomically with FileHandles revocation AFTER the network,
   // delegated capability and scheduler-resource teardown is verified.
   #[cfg(feature="test-r031-files-gui")]
   {
    // Mandatory exact-incarnation revocation on ordinary exit, fault and
    // rollback. Network grants are retired before FileHandles, while the
    // scheduler owns the only BSP mutator; no nested lock acquisition.
    let sockets=unsafe{&mut *ptr::addr_of_mut!(R114_LOCAL_UDP)};
    let coordinator=unsafe{&mut *ptr::addr_of_mut!(R120_RETIREMENT)};
    let live=crate::process_retirement_txn::Owner{
      pid:owner.pid,generation:owner.generation};
    let current=coordinator.active_generation(owner.pid);
    if current==Some(owner.generation){
      // Preflight and stage UDP, exact-generation file handles, delegated
      // directory grants and PID1 notice BEFORE modifying any live store.
      // No peer can observe partial retirement through a failed event queue.
      let staged=retire_full_owner_epoch(
          unsafe{&mut *ptr::addr_of_mut!(R031_HANDLES)},
          unsafe{&mut *ptr::addr_of_mut!(R114_FILE_EVENTS)},
          coordinator,sockets,
          unsafe{&mut *ptr::addr_of_mut!(R116_SHARED_LEASES)},
          owner,kind);
      if staged!=Ok(true){
        log(b"IndianOS: FATAL R120 atomic file/network/lease retirement failed\n\0");
        r120_halt();
      }
      full_retirement_committed=true;
      // Printed only after the active R120 incarnation has committed the
      // unified FileHandles, UDP, capability, event and lease transaction.
      // Host-side markers cannot satisfy this real guest scheduler proof.
      if owner.pid==2 {
        log(b"IndianOS: R121 QEMU five-table PID2 retirement transaction PASSED\n\0");
      }
    }else if current.is_none(){
      // Compatibility path for host tests creating synthetic FileHandles
      // owners without initializing a full production scheduler session.
      let registry=unsafe{&mut *ptr::addr_of_mut!(R115_UDP_OWNERS)};
      if sockets.retire_exact_kernel(registry,owner.pid,owner.generation).is_err(){
        log(b"IndianOS: FATAL R118 legacy UDP retirement failed\n\0");
        r120_halt();
      }
    }else{
      // A delayed task for the same PID must not remove a new incarnation.
      log(b"IndianOS: R120 stale scheduler retirement ignored\n\0");
    }
   }
   // Legacy/synthetic host setup without a live R120 owner still follows
   // the old separately verified file path. A real active owner has already
   // committed all five tables above and must not retire twice.
   #[cfg(feature="test-r031-files-gui")]
   if !full_retirement_committed {
     unsafe{(&mut *ptr::addr_of_mut!(R116_SHARED_LEASES))}
        .revoke_identity(owner.pid,owner.generation);
   }
   #[cfg(feature="test-r031-files-gui")]
   let needs_legacy_file_retire=!full_retirement_committed;
   #[cfg(not(feature="test-r031-files-gui"))]
   let needs_legacy_file_retire=true;
   if needs_legacy_file_retire {
     let result=retire_file_owner_epoch(
         unsafe{&mut *ptr::addr_of_mut!(R031_HANDLES)},
         unsafe{&mut *ptr::addr_of_mut!(R114_FILE_EVENTS)},
         owner,kind);
     if result.is_err(){r118_file_retirement_fatal();}
   }
   #[cfg(feature="test-r031-files-gui")]
   if had_live_directory_lease {
     let lease_gone=!unsafe{(&*ptr::addr_of!(R116_SHARED_LEASES))}
           .permits_shared_directory(owner);
     // Independent post-condition: neither the real queue nor the
     // generation-bound policy may retain this incarnation after cleanup.
     let port_gone=unsafe{(&*ptr::addr_of!(R114_LOCAL_UDP))}
        .pending_owned(owner.pid,owner.generation).is_err() &&
        unsafe{(&*ptr::addr_of!(R120_RETIREMENT))}
        .authorize_udp(crate::process_retirement_txn::Owner{
           pid:owner.pid,generation:owner.generation},5502).is_err() &&
        unsafe{(&*ptr::addr_of!(R115_UDP_OWNERS))}
        .owner_of(5502).is_none();
     let file_gone=!unsafe{(&*ptr::addr_of!(R031_HANDLES))}.owns(owner);
     if lease_gone && port_gone && file_gone {
       log(b"R117 QEMU PID2 delegated directory and UDP authority revoked at scheduler exit\n\0");
     }else{
       log(b"IndianOS: FATAL R117 PID2 delegated authority retained after exit\n\0");
     }
   }
  }
  task.file_generation=0;
 }
}
fn discard(t:*mut Task,b:&CloneBackend,terminal:bool)->bool{
 let task=unsafe{&mut *t};
 revoke_task_authority(task);
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
// A failed release retains its ownership metadata for the next serialized
// invocation. Never overwrite a partially reclaimed image with zeroed storage.
fn cleanup_session(backend:&CloneBackend,reclaim:bool)->bool{
 let mut complete=reclaim;
 for slot in 0..TASKS{
  let task=unsafe{&mut *task_ptr(slot)};
  revoke_task_authority(task);
  // The old DirectMap lived on the previous invocation's stack. Rebind only
  // the trusted backend context before retrying retained image reclamation.
  task.image.tables.backend.context=backend.context;
  if reclaim && !discard(task as *mut Task,backend,false){complete=false;}
  // PCBs belong to the caller's stack-local ProcessTable. No static task may
  // retain a pointer after that table's lifetime, including failed teardown.
  task.pcb=ptr::null_mut();
 }
 #[cfg(feature="test-ipc")]
 unsafe{IPC_ROUTER=None;}
 #[cfg(feature="test-r025")]
 unsafe{R025_SERVICES=None;}
 #[cfg(feature="test-r031-files-gui")]
 {
  // Release the permit before entering any scheduler or user code.
  let Ok(_permit)=r120_enter()else{return false;};
  unsafe{R114_LOCAL_UDP=crate::network::EpochUdpEndpoints::new();
         R115_UDP_OWNERS=crate::udp_owner_epoch::Registry::new();
         R116_SHARED_LEASES=crate::agent_03_cap_epoch::LeaseTable::new();}
 }
 unsafe{SESSION_DIRTY=!complete;}
 complete
}
struct SessionRollback{backend:CloneBackend,kernel_root:u64}
impl Drop for SessionRollback{
 fn drop(&mut self){
  #[cfg(feature="preempt")]
  let timer_stopped=stop_session_timer();
  #[cfg(not(feature="preempt"))]
  let timer_stopped=true;
  if !unsafe{SESSION_DIRTY}{return;}
  let current:u64;
  unsafe{asm!("mov {}, cr3",out(reg)current,options(nostack,nomem,preserves_flags));}
  // If the assembly trampoline failed to restore CR3, revoke authority but
  // retain all potentially active tables instead of freeing the current root.
  if !cleanup_session(&self.backend,timer_stopped&&current&PHYS==self.kernel_root){
   log(b"IndianOS: M009 rollback retained unreclaimed resources\n\0");
  }
 }
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
 #[cfg(feature="test-r031-files-gui")]
 if !r120_setup(){log(b"IndianOS: R120 denied unsafe CPU/IRQ scheduler entry\n\0");return false;}
 let mut direct=DirectMap{offset};
 let vmm=vmm_backend(&mut direct);
 let backend=clone_backend(&mut direct);
 #[cfg(feature="preempt")]
 if !stop_session_timer(){
  log(b"IndianOS: M009 prior timer stop remains uncertain\n\0");return false;
 }
 let active_retained_root=unsafe{SESSION_DIRTY} && (0..TASKS).any(|slot|{
  let task=unsafe{&*task_ptr(slot)};
  task.root.root!=0 && task.root.root==cpu.cr3&PHYS
 });
 if unsafe{SESSION_DIRTY} && (active_retained_root||!cleanup_session(&backend,true)){
  log(b"IndianOS: M009 prior rollback still owns resources\n\0");return false;
 }
 #[cfg(feature="test-r031-files-gui")]
 {
  // Release the permit before entering any scheduler or user code.
  let Ok(_permit)=r120_enter()else{return false;};
  unsafe{R114_LOCAL_UDP=crate::network::EpochUdpEndpoints::new();
         R115_UDP_OWNERS=crate::udp_owner_epoch::Registry::new();
         R116_SHARED_LEASES=crate::agent_03_cap_epoch::LeaseTable::new();}
 }
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
 // Bind every created PCB before the first fallible image preparation. GUI
 // grants may already exist for a peer whose ELF has not been attempted yet.
 unsafe{
  (*task_ptr(0)).pcb=p;(*task_ptr(0)).authority_live=true;
  (*task_ptr(1)).pcb=q;(*task_ptr(1)).authority_live=true;
 }
 #[cfg(feature="test-r029-desktop-suite")]
 unsafe{(*task_ptr(2)).pcb=third;(*task_ptr(2)).authority_live=true;}
 unsafe{SESSION_DIRTY=true;}
 let _rollback=SessionRollback{backend,kernel_root:cpu.cr3&PHYS};
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
  return false;
 }
 // Save the two genuine ring-3 PID epochs, before teardown clears the
 // Task.file_generation fields. These are minted by the privileged
 // FileHandles allocator, never derived from a guest syscall argument.
 #[cfg(feature="test-r031-files-gui")]
 let r120_expected=unsafe{[
    (*task_ptr(0)).file_generation,
    (*task_ptr(1)).file_generation
 ]};
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
 #[cfg(all(feature="test-r026",not(feature="test-r031-files-gui")))]
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
  // Arm rollback before calibration: a partially failed start must also mask
  // the source before any task tables or authority can be reclaimed.
  unsafe{SESSION_TIMER_ARMED=true;}
  if !unsafe{crate::timer::x86_64_xapic_start_qemu_100hz()}{
   log(b"IndianOS: R024 FATAL xAPIC/PIT2 calibration failed\n\0");
   return false;
  }
  log(b"IndianOS: R024 calibrated periodic xAPIC IRQ at 100 Hz\n\0");
 }
 let mut ok=true;
 while ok&&!unsafe{indianos_scheduler_all_terminal(&scheduler)}{
  #[cfg(feature="test-ipc")]
  let pcb=next_ipc_ready(&mut scheduler);
  #[cfg(not(feature="test-ipc"))]
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
   // Both a normal process exit and a ring-3 fault must revoke live network,
   // file and delegated capability authority BEFORE potentially fallible
   // user page-table / frame reclamation below. The rollback path still
   // calls the same idempotent retirement when process termination fails.
   revoke_task_authority_with_kind(task,if v==0{
     crate::process_events::EventKind::Exited
   }else{
     crate::process_events::EventKind::Faulted
   });
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
  if !stop_session_timer(){ok=false;}
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
 #[cfg(all(feature="test-r026",not(feature="test-r031-files-gui")))]
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
 #[cfg(feature="test-r031-files-gui")]
 {
   // R120: the real ring-3 GUI/QEMU acceptance fixture executes PID1 and
   // PID2 UDP PING/PONG + forged foreign-close denial before this terminal
   // state. Validate both trusted incarnations have lost *all* authority
   // and the real socket queue agrees with its policy registry.
   let _permit=r120_enter().unwrap_or_else(|_|r120_halt());
   let coordinator=unsafe{&*ptr::addr_of!(R120_RETIREMENT)};
   let sockets=unsafe{&*ptr::addr_of!(R114_LOCAL_UDP)};
   let handles=unsafe{&*ptr::addr_of!(R031_HANDLES)};
   let first=crate::batch5_session::Identity{
       pid:1,generation:r120_expected[0]};
   let second=crate::batch5_session::Identity{
       pid:2,generation:r120_expected[1]};
   let retired=r120_expected.iter().all(|&gen|gen!=0) &&
       coordinator.active_generation(1).is_none() &&
       coordinator.active_generation(2).is_none() &&
       !handles.owns(first) && !handles.owns(second) &&
       sockets.pending_owned(1,first.generation).is_err() &&
       sockets.pending_owned(2,second.generation).is_err() &&
       coordinator.live_sockets_consistent(sockets);
   if !retired{
      ok=false;
      log(b"IndianOS: FATAL R120 ring3 dual-PID UDP epoch retirement mismatch\n\0");
   }else if ok {
      log(b"IndianOS: R120 QEMU ring3 dual-PID UDP epoch retirement PASSED\n\0");
   }
 }
 if !ok{
  log(b"IndianOS: M009 FATAL scheduler or memory ownership verification\n\0");

 }
 #[cfg(feature="preempt")]
 let can_reclaim=!unsafe{SESSION_TIMER_ARMED};
 #[cfg(not(feature="preempt"))]
 let can_reclaim=true;
 let reclaimed=cleanup_session(&backend,can_reclaim);
 if reclaimed && unsafe{pmm_free_frames()}!=before{
  log(b"IndianOS: M009 FATAL rollback frame count mismatch\n\0");return false;
 }
 ok&&reclaimed
}
#[cfg(test)]mod tests{
 use super::*;
 static LOCK:std::sync::Mutex<()>=std::sync::Mutex::new(());

 #[cfg(feature="test-r031-files-gui")]
 #[test]fn r117_real_scheduler_revoke_withdraws_shared_directory_and_udp(){
  use core::mem::ManuallyDrop;
  let _lock=LOCK.lock().unwrap();
  // Save/restore all touched static service tables. The runtime tests own
  // the single-BSP scheduler globals under one serialized host-test lock.
  struct Restore{
   tasks:ManuallyDrop<[MaybeUninit<Task>;TASKS]>,
   handles:ManuallyDrop<crate::fs_handles::FileHandles>,
   events:ManuallyDrop<crate::process_events::ProcessEvents<16>>,
   sockets:ManuallyDrop<crate::network::EpochUdpEndpoints<2,8>>,
   registry:ManuallyDrop<crate::udp_owner_epoch::Registry<2>>,
   leases:ManuallyDrop<crate::agent_03_cap_epoch::LeaseTable<2>>,
   retirement:ManuallyDrop<crate::process_retirement_txn::ProcessRetirement<2,2,16>>,
  }
  impl Drop for Restore{
   fn drop(&mut self){unsafe{
    ptr::write(ptr::addr_of_mut!(M009_TASKS),ManuallyDrop::take(&mut self.tasks));
    ptr::write(ptr::addr_of_mut!(R031_HANDLES),ManuallyDrop::take(&mut self.handles));
    ptr::write(ptr::addr_of_mut!(R114_FILE_EVENTS),ManuallyDrop::take(&mut self.events));
    ptr::write(ptr::addr_of_mut!(R114_LOCAL_UDP),ManuallyDrop::take(&mut self.sockets));
    ptr::write(ptr::addr_of_mut!(R115_UDP_OWNERS),ManuallyDrop::take(&mut self.registry));
    ptr::write(ptr::addr_of_mut!(R116_SHARED_LEASES),ManuallyDrop::take(&mut self.leases));
    ptr::write(ptr::addr_of_mut!(R120_RETIREMENT),ManuallyDrop::take(&mut self.retirement));
   }}
  }
  let _restore=unsafe{Restore{
   tasks:ManuallyDrop::new(ptr::read(ptr::addr_of!(M009_TASKS))),
   handles:ManuallyDrop::new(ptr::read(ptr::addr_of!(R031_HANDLES))),
   events:ManuallyDrop::new(ptr::read(ptr::addr_of!(R114_FILE_EVENTS))),
   sockets:ManuallyDrop::new(ptr::read(ptr::addr_of!(R114_LOCAL_UDP))),
   registry:ManuallyDrop::new(ptr::read(ptr::addr_of!(R115_UDP_OWNERS))),
   leases:ManuallyDrop::new(ptr::read(ptr::addr_of!(R116_SHARED_LEASES))),
   retirement:ManuallyDrop::new(ptr::read(ptr::addr_of!(R120_RETIREMENT))),
  }};
  let mut init=Process::empty();init.pid=1;init.state=2;
  let mut guest=Process::empty();guest.pid=2;guest.state=2;
  unsafe{
   for i in 0..TASKS{ptr::write(task_ptr(i),core::mem::zeroed());}
   R031_HANDLES=crate::fs_handles::FileHandles::new();
   R114_FILE_EVENTS=crate::process_events::ProcessEvents::new();
   R114_LOCAL_UDP=crate::network::EpochUdpEndpoints::new();
   R115_UDP_OWNERS=crate::udp_owner_epoch::Registry::new();
   R116_SHARED_LEASES=crate::agent_03_cap_epoch::LeaseTable::new();
   R120_RETIREMENT=crate::process_retirement_txn::ProcessRetirement::new();
   assert!(r120_setup());
   // Raw pointers are converted to short-lived exclusive borrows only for
   // each operation; teardown itself re-enters these same kernel globals.
   let table=ptr::addr_of_mut!(R031_HANDLES);
   let a=(&mut *table).bind_process(1).unwrap();
   let old=(&mut *table).bind_process(2).unwrap();
   let coordinator=ptr::addr_of_mut!(R120_RETIREMENT);
   for id in [a,old]{
     (&mut *coordinator).start_exact(crate::process_retirement_txn::Owner{
       pid:id.pid,generation:id.generation}).unwrap();
   }
   let first=&mut *task_ptr(0);first.pcb=&mut init;
   first.file_generation=a.generation;first.authority_live=true;
   let second=&mut *task_ptr(1);second.pcb=&mut guest;
   second.file_generation=old.generation;second.authority_live=true;
   assert!(r116_prepare_shared_directory_lease(old));
   assert!((&*ptr::addr_of!(R116_SHARED_LEASES))
           .permits_shared_directory(old));
   (&mut *coordinator).bind_live_udp(&mut *ptr::addr_of_mut!(R114_LOCAL_UDP),
      crate::process_retirement_txn::Owner{
       pid:old.pid,generation:old.generation},5502).unwrap();
   revoke_task_authority(second);
   assert!(!(&*ptr::addr_of!(R116_SHARED_LEASES))
           .permits_shared_directory(old));
   assert!(!(&*ptr::addr_of!(R031_HANDLES)).owns(old));
   assert!((&*ptr::addr_of!(R031_HANDLES)).owns(a));
   assert!((&*ptr::addr_of!(R120_RETIREMENT))
       .authorize_udp(crate::process_retirement_txn::Owner{
           pid:old.pid,generation:old.generation},5502).is_err());
   assert_eq!((&*ptr::addr_of!(R114_FILE_EVENTS)).len(),1);
   let newest=(&mut *ptr::addr_of_mut!(R031_HANDLES))
              .bind_process(2).unwrap();
   second.file_generation=newest.generation;
   second.authority_live=true;
   (&mut *coordinator).start_exact(crate::process_retirement_txn::Owner{
      pid:newest.pid,generation:newest.generation}).unwrap();
   assert!(r116_prepare_shared_directory_lease(newest));
   assert!((&*ptr::addr_of!(R116_SHARED_LEASES))
          .permits_shared_directory(newest));
   // A delayed cleanup of the former generation cannot clear the new grant.
   second.file_generation=old.generation;
   second.authority_live=true;
   revoke_task_authority(second);
   assert!((&*ptr::addr_of!(R031_HANDLES)).owns(newest));
   assert!((&*ptr::addr_of!(R116_SHARED_LEASES))
          .permits_shared_directory(newest));
   assert_eq!((&*ptr::addr_of!(R114_FILE_EVENTS)).len(),1);
   // R120: this is the actual ring3 task-authenticating fault branch.
   // Rebind an exact new generation and verify it is reported as Faulted
   // rather than an ordinary Exited notice, without removing old receipts.
   second.file_generation=newest.generation;
   second.authority_live=true;
   revoke_task_authority_with_kind(second,
      crate::process_events::EventKind::Faulted);
   let queue=&mut *ptr::addr_of_mut!(R114_FILE_EVENTS);
   assert_eq!(queue.len(),2);
   let old_receipt=queue.peek_receipt().unwrap();
   assert_eq!(old_receipt.event.kind,crate::process_events::EventKind::Exited);
   assert_eq!(queue.acknowledge_receipt(old_receipt),Some(old_receipt.event));
   let fault_receipt=queue.peek_receipt().unwrap();
   assert_eq!(fault_receipt.event.kind,crate::process_events::EventKind::Faulted);
   assert_eq!(fault_receipt.event.value,newest.generation);
   assert_eq!(queue.acknowledge_receipt(old_receipt),None);
   assert!(!(&*ptr::addr_of!(R031_HANDLES)).owns(newest));
  }
 }

 // Host tests link the real retirement/reclamation path; the serial hardware
 // boundary alone is supplied here because the kernel COM1 driver is assembly.
 #[export_name="serial_write"]
 extern "C" fn host_serial_write(_: *const c_char){}
 #[cfg(feature="test-ipc")]
 #[test]fn all_waiting_tasks_expire_without_fabricated_ticks_or_dispatches(){
  use core::mem::ManuallyDrop;
  let _lock=LOCK.lock().unwrap();
  struct Restore{
   tasks:ManuallyDrop<[MaybeUninit<Task>;TASKS]>,
   router:ManuallyDrop<Option<IpcRouter<4,8>>>,
  }
  impl Drop for Restore{
   fn drop(&mut self){unsafe{
    ptr::write(ptr::addr_of_mut!(M009_TASKS),ManuallyDrop::take(&mut self.tasks));
    ptr::write(ptr::addr_of_mut!(IPC_ROUTER),ManuallyDrop::take(&mut self.router));
   }}
  }
  let _restore=unsafe{Restore{
   tasks:ManuallyDrop::new(ptr::read(ptr::addr_of!(M009_TASKS))),
   router:ManuallyDrop::new(ptr::read(ptr::addr_of!(IPC_ROUTER))),
  }};
  let mut table=ProcessTable::empty();
  let mut boxes=IpcRouter::<4,8>::new().unwrap();
  let mut outputs=[[0xa5u8;crate::ipc::RECEIVE_BYTES];2];
  unsafe{
   for slot in 0..TASKS{ptr::write(task_ptr(slot),core::mem::zeroed());}
   indianos_process_table_init(&mut table);
   for (slot,output) in outputs.iter_mut().enumerate(){
    let process=indianos_process_create(&mut table);
    assert!(indianos_process_ready(process));
    assert!(indianos_process_bind(process,((slot+1) as u64)*PAGE));
    boxes.register((*process).pid).unwrap();
    let task=&mut *task_ptr(slot);
    task.pcb=process;task.saved=true;task.image.ready=true;task.frame[14]=BAD;
    task.ipc_wait=crate::ipc::ReceiveWait::begin(&boxes,(*process).pid,
        output.as_mut_ptr() as u64,32).unwrap();
   }
   IPC_ROUTER=Some(boxes);
   let mut scheduler=Scheduler::empty();assert!(indianos_scheduler_init(&mut scheduler,&mut table));
   let selected=next_ipc_ready(&mut scheduler);
   assert!(!selected.is_null());assert_eq!((*selected).pid,1);
   assert_eq!((scheduler.dispatch_count,scheduler.tick_count,scheduler.yield_count),(1,0,0));
   for slot in 0..2{
    let task=&*task_ptr(slot);assert!(!task.ipc_wait.active());assert_eq!(task.frame[14],0);
   }
  }
  assert_eq!(outputs,[[0xa5;crate::ipc::RECEIVE_BYTES];2]);
 }
 #[test]fn aborted_session_revokes_authority_before_failed_release_and_retries(){
  use core::mem::ManuallyDrop;
  let _lock=LOCK.lock().unwrap();
  struct Restore{
   tasks:ManuallyDrop<[MaybeUninit<Task>;TASKS]>,dirty:bool,
   #[cfg(feature="test-ipc")]
   router:ManuallyDrop<Option<IpcRouter<4,8>>>,
   #[cfg(feature="test-r025")]
   services:ManuallyDrop<Option<R025Services>>,
   #[cfg(feature="test-r031-vfs")]
   handles:ManuallyDrop<crate::fs_handles::FileHandles>,
   #[cfg(feature="test-r031-vfs")]
   event_log:ManuallyDrop<crate::process_events::ProcessEvents<16>>,
  }
  impl Drop for Restore{fn drop(&mut self){unsafe{
   ptr::write(ptr::addr_of_mut!(M009_TASKS),ManuallyDrop::take(&mut self.tasks));
   SESSION_DIRTY=self.dirty;
   #[cfg(feature="test-ipc")]
   ptr::write(ptr::addr_of_mut!(IPC_ROUTER),ManuallyDrop::take(&mut self.router));
   #[cfg(feature="test-r025")]
   ptr::write(ptr::addr_of_mut!(R025_SERVICES),ManuallyDrop::take(&mut self.services));
   #[cfg(feature="test-r031-vfs")]
   ptr::write(ptr::addr_of_mut!(R031_HANDLES),ManuallyDrop::take(&mut self.handles));
   #[cfg(feature="test-r031-vfs")]
   ptr::write(ptr::addr_of_mut!(R114_FILE_EVENTS),ManuallyDrop::take(&mut self.event_log));
  }}}
  let _restore=unsafe{Restore{
   tasks:ManuallyDrop::new(ptr::read(ptr::addr_of!(M009_TASKS))),dirty:SESSION_DIRTY,
   #[cfg(feature="test-ipc")]
   router:ManuallyDrop::new(ptr::read(ptr::addr_of!(IPC_ROUTER))),
   #[cfg(feature="test-r025")]
   services:ManuallyDrop::new(ptr::read(ptr::addr_of!(R025_SERVICES))),
   #[cfg(feature="test-r031-vfs")]
   handles:ManuallyDrop::new(ptr::read(ptr::addr_of!(R031_HANDLES))),
   #[cfg(feature="test-r031-vfs")]
   event_log:ManuallyDrop::new(ptr::read(ptr::addr_of!(R114_FILE_EVENTS))),
  }};
  struct Memory{root:[u64;512],refuse:bool,releases:usize}
  unsafe extern "C" fn allocate(_: *mut u64,_:*mut c_void)->bool{false}
  unsafe extern "C" fn access(p:u64,c:*mut c_void)->*mut u64{
   if p!=PAGE{return ptr::null_mut();}
   unsafe{(&mut *c.cast::<Memory>()).root.as_mut_ptr()}
  }
  unsafe extern "C" fn release(p:u64,c:*mut c_void)->bool{
   let m=unsafe{&mut *c.cast::<Memory>()};
   assert_eq!(p,PAGE);
   if m.refuse{return false;}
   m.releases+=1;true
  }
  let mut memory=Memory{root:[0;512],refuse:true,releases:0};
  let backend=CloneBackend{allocate:Some(allocate),release:Some(release),
   access:Some(access),context:(&mut memory as *mut Memory).cast()};
  let mut process=Process::empty();process.pid=1;process.state=2;
  unsafe{
   for slot in 0..TASKS{ptr::write(task_ptr(slot),core::mem::zeroed());}
   SESSION_DIRTY=true;
   let task=&mut *task_ptr(0);task.pcb=&mut process;task.authority_live=true;
   task.saved=true;task.frame=[0x55;WORDS];task.image.ready=true;
   task.root=CloneResult{root:PAGE,table_count:1};
   #[cfg(feature="test-ipc")]{
    let mut boxes=IpcRouter::<4,8>::new().unwrap();
    boxes.register(1).unwrap();boxes.register(2).unwrap();
    boxes.send(1,2,b"stale outbound").unwrap();
    task.ipc_wait=crate::ipc::ReceiveWait::begin(&boxes,1,0x400000,32).unwrap();
    IPC_ROUTER=Some(boxes);
   }
   #[cfg(feature="test-r025")]{R025_SERVICES=R025Services::new();}
   #[cfg(feature="test-r031-vfs")]{
    R031_HANDLES=crate::fs_handles::FileHandles::new();
    R114_FILE_EVENTS=crate::process_events::ProcessEvents::new();
    task.file_generation=(&mut *ptr::addr_of_mut!(R031_HANDLES)).bind_process(1).unwrap().generation;
   }
   // Even a CR3 mismatch must retire credentials, but never release a table.
   assert!(!cleanup_session(&backend,false));assert_eq!(memory.releases,0);
   assert!(SESSION_DIRTY);assert!((*task_ptr(0)).pcb.is_null());
   assert!(!(*task_ptr(0)).authority_live);assert!(!(*task_ptr(0)).saved);
   assert_eq!((*task_ptr(0)).frame,[0;WORDS]);assert!(!(*task_ptr(0)).image.ready);
   #[cfg(feature="test-ipc")]{
    assert!(!(*task_ptr(0)).ipc_wait.active());assert!(router().is_none());
   }
   #[cfg(feature="test-r025")]{assert!(services().is_none());}
   #[cfg(feature="test-r031-vfs")]{
    assert!(!(&*ptr::addr_of!(R031_HANDLES)).owns(crate::batch5_session::Identity{pid:1,generation:1}));
    assert_eq!((&*ptr::addr_of!(R114_FILE_EVENTS)).peek(),Some(
      crate::process_events::ProcessEvent{pid:1,
        kind:crate::process_events::EventKind::Exited,value:1}));
    assert_eq!((&*ptr::addr_of!(R114_FILE_EVENTS)).len(),1);
    let replacement=(&mut *ptr::addr_of_mut!(R031_HANDLES)).bind_process(1).unwrap();
    assert!(replacement.generation>1);
    let expected=crate::process_events::ProcessEvent{pid:1,
      kind:crate::process_events::EventKind::Exited,value:1};
    let receipt=peek_pid1_lifecycle_receipt(replacement).unwrap();
    assert_eq!(receipt.event,expected);
    assert_eq!(peek_pid1_lifecycle_receipt(
      crate::batch5_session::Identity{pid:1,generation:1}),Err(()));
    assert_eq!(consume_pid1_lifecycle_receipt(
      crate::batch5_session::Identity{pid:1,generation:1},receipt),Err(()));
    assert_eq!(consume_pid1_lifecycle_receipt(
      crate::batch5_session::Identity{pid:2,generation:2},receipt),Err(()));
    assert_eq!((&*ptr::addr_of!(R114_FILE_EVENTS)).len(),1);
    assert_eq!(consume_pid1_lifecycle_receipt(replacement,
      crate::process_events::EventReceipt{
        event:crate::process_events::ProcessEvent{value:999,..expected},
        ..receipt
      }),Err(()));
    assert_eq!(consume_pid1_lifecycle_receipt(replacement,receipt),Ok(expected));
    assert_eq!(consume_pid1_lifecycle_receipt(replacement,receipt),Err(()));
    assert_eq!((&*ptr::addr_of!(R114_FILE_EVENTS)).len(),0);
   }
   #[cfg(feature="test-ipc")]{
    let mut replacement=IpcRouter::<4,8>::new().unwrap();
    replacement.register(1).unwrap();replacement.register(2).unwrap();
    replacement.send(2,1,b"new incarnation").unwrap();IPC_ROUTER=Some(replacement);
   }
   // A delayed per-task retry cannot revoke a replacement PID's mailbox.
   assert!(!discard(task_ptr(0),&backend,false));
   #[cfg(feature="test-r031-vfs")]
   assert_eq!((&*ptr::addr_of!(R114_FILE_EVENTS)).len(),0);
   #[cfg(feature="test-ipc")]{assert_eq!(router().unwrap().pending(1),Ok(1));}
   // Release refusal preserves physical ownership for a later retry.
   assert!(!cleanup_session(&backend,true));
   assert_eq!((*task_ptr(0)).root.root,PAGE);assert_eq!(memory.releases,0);
   memory.refuse=false;
   assert!(cleanup_session(&backend,true));assert!(!SESSION_DIRTY);
   assert_eq!((*task_ptr(0)).root.root,0);assert_eq!(memory.releases,1);
   assert!(cleanup_session(&backend,true));assert_eq!(memory.releases,1);
   #[cfg(feature="test-r031-vfs")]{
    assert!((&*ptr::addr_of!(R031_HANDLES)).owns(crate::batch5_session::Identity{pid:1,generation:2}));
   }
  }
 }
 #[test]fn timer_stop_uncertainty_preserves_ownership_until_retry_succeeds(){
  let mut armed=true;
  assert!(!stop_owned_timer(&mut armed,||false));assert!(armed);
  // The same persistent flag is consumed by the next scheduler invocation.
  assert!(!stop_owned_timer(&mut armed,||false));assert!(armed);
  assert!(stop_owned_timer(&mut armed,||true));assert!(!armed);
  assert!(stop_owned_timer(&mut armed,||panic!("already stopped timer retried")));
 }
 #[test]fn syscall_capture_preserves_runtime_irq_policy_and_zero_return(){
  let mut process=Process::empty();process.pid=1;process.state=3;
  let mut task:Task=unsafe{core::mem::zeroed()};task.pcb=&mut process;
  task.image.plan.stack_begin=USER_TOP-STACK_BYTES;task.image.plan.stack_end=USER_TOP;
  let mut frame=[0u64;WORDS];frame[14]=35;frame[15]=0x400000;
  frame[16]=0x2b;frame[17]=2|if USER_INTERRUPTS_ENABLED{0x200}else{0};
  frame[18]=USER_TOP-16;frame[19]=0x33;
  assert!(capture_frame(&mut task,&frame,false));
  assert!(task.saved);assert_eq!(task.frame[14],0);assert_eq!(task.frame[17],frame[17]);
  let saved=task.frame;frame[17]^=0x200;
  assert!(!capture_frame(&mut task,&frame,false));assert_eq!(task.frame,saved);
  frame[17]^=0x200;frame[16]=0x08;
  assert!(!capture_frame(&mut task,&frame,false));assert_eq!(task.frame,saved);
 }
 #[test]fn saved_frame_uses_same_irq_policy_with_actual_executable_mapping(){
  unsafe extern "C" fn allocate(_: *mut u64,_:*mut c_void)->bool{false}
  unsafe extern "C" fn release(_:u64,_:*mut c_void)->bool{false}
  unsafe extern "C" fn access(physical:u64,context:*mut c_void)->*mut c_void{
   if physical%PAGE!=0||!(PAGE..=4*PAGE).contains(&physical){return ptr::null_mut();}
   unsafe{context.cast::<[u64;512]>().add((physical/PAGE-1) as usize).cast()}
  }
  let mut tables=[[0u64;512];4];
  tables[0][0]=0x2000|7;tables[1][0]=0x3000|7;
  tables[2][2]=0x4000|7;tables[3][0]=0x5000|5;
  let mut task:Task=unsafe{core::mem::zeroed()};
  task.image.tables=crate::vmm::Space{
   root_physical:PAGE,initialized:true,
   backend:VmmBackend{allocate:Some(allocate),release:Some(release),
    access:Some(access),context:tables.as_mut_ptr().cast()},
  };
  task.image.plan.stack_begin=USER_TOP-STACK_BYTES;task.image.plan.stack_end=USER_TOP;
  task.image.plan.segment_count=1;
  task.image.plan.segments[0].virtual_address=0x400000;
  task.image.plan.segments[0].memory_size=PAGE;
  task.image.plan.segments[0].page_begin=0x400000;
  task.image.plan.segments[0].page_end=0x401000;
  task.image.plan.segments[0].executable=true;
  task.saved=true;task.frame[15]=0x400000;task.frame[16]=0x2b;
  task.frame[17]=2|if USER_INTERRUPTS_ENABLED{0x200}else{0};
  task.frame[18]=USER_TOP-16;task.frame[19]=0x33;
  assert!(saved_valid(&task));
  let flags=task.frame[17];
  for rejected in [flags^0x200,flags|0x3000,flags|(1<<14),flags|(1<<17)]{
   task.frame[17]=rejected;assert!(!saved_valid(&task));
  }
  task.frame[17]=flags;task.frame[16]=0x08;assert!(!saved_valid(&task));
  task.frame[16]=0x2b;tables[3][0]|=2;assert!(!saved_valid(&task));
 }
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

#[cfg(all(test,feature="test-r031-vfs"))]
mod r118_scheduler_file_event_atomicity_tests {
    use super::*;
    use crate::process_events::{EventKind,ProcessEvents};
    use crate::fs_handles::FileHandles;

    #[test]
    fn full_event_queue_denies_file_retirement_without_partial_notice() {
        let mut files=FileHandles::new();
        let init=files.bind_process(1).unwrap();
        let worker=files.bind_process(2).unwrap();
        let mut queue=ProcessEvents::<1>::new();
        let original=files.mount_alias_for_current_root_kernel(worker).unwrap();

        assert_eq!(retire_file_owner_epoch(
            &mut files,&mut queue,init,EventKind::Exited),Ok(true));
        let first=queue.peek_receipt().unwrap();
        assert_eq!(first.event.pid,init.pid);
        assert!(!files.owns(init));

        assert_eq!(retire_file_owner_epoch(
            &mut files,&mut queue,worker,EventKind::Faulted),
            Err(FileRetireFailure::EventQueue));
        assert!(files.owns(worker));
        assert!(files.mount_alias_allows(original,worker,"/shared/note"));
        assert_eq!(queue.peek_receipt(),Some(first));

        assert_eq!(queue.acknowledge_receipt(first),Some(first.event));
        assert_eq!(retire_file_owner_epoch(
            &mut files,&mut queue,worker,EventKind::Faulted),Ok(true));
        assert!(!files.owns(worker));
        assert!(!files.mount_alias_allows(original,worker,"/shared/note"));
        let final_notice=queue.peek_receipt().unwrap();
        assert_eq!(final_notice.event.pid,worker.pid);
        assert_eq!(final_notice.event.kind,EventKind::Faulted);
    }

    #[test]
    fn empty_notice_queue_and_recycled_pid_do_not_discard_live_authority() {
        let mut files=FileHandles::new();
        let old=files.bind_process(1).unwrap();
        let mut zero=ProcessEvents::<0>::new();
        assert_eq!(retire_file_owner_epoch(
            &mut files,&mut zero,old,EventKind::Exited),
            Err(FileRetireFailure::EventQueue));
        assert!(files.owns(old));
        assert_eq!(zero.len(),0);

        let mut events=ProcessEvents::<2>::new();
        assert_eq!(retire_file_owner_epoch(
            &mut files,&mut events,old,EventKind::Exited),Ok(true));
        let fresh=files.bind_process(1).unwrap();
        assert_ne!(fresh.generation,old.generation);
        let before=events.peek_receipt();
        assert_eq!(retire_file_owner_epoch(
            &mut files,&mut events,old,EventKind::Faulted),Ok(false));
        assert_eq!(events.peek_receipt(),before);
        assert!(files.owns(fresh));
        assert_eq!(retire_file_owner_epoch(
            &mut files,&mut events,fresh,EventKind::Message),
            Err(FileRetireFailure::EventQueue));
        assert!(files.owns(fresh));
        assert_eq!(events.peek_receipt(),before);
    }
}

#[cfg(all(test,feature="test-r031-files-gui"))]
mod r118_full_owner_retirement_integration_tests {
    use super::*;
    use crate::batch5_session::Identity;
    use crate::process_events::{EventKind,ProcessEvents};
    use crate::fs_handles::FileHandles;
    use crate::network::EpochUdpEndpoints;
    use crate::process_retirement_txn::{Owner,ProcessRetirement};
    use crate::agent_03_cap_epoch::LeaseTable;

    #[test]
    fn full_external_notice_ring_rolls_back_real_udp_and_file_retirement() {
        let mut files=FileHandles::new();
        let init=files.bind_process(1).unwrap();
        let peer=files.bind_process(2).unwrap();
        let mut notices=ProcessEvents::<1>::new();
        notices.enqueue_exit_once(99,1).unwrap();
        let old_receipt=notices.peek_receipt().unwrap();
        let mut coordinator=ProcessRetirement::<2,2,4>::new();
        coordinator.start_exact(Owner{
            pid:init.pid,generation:init.generation}).unwrap();
        coordinator.start_exact(Owner{
            pid:peer.pid,generation:peer.generation}).unwrap();
        let mut sockets=EpochUdpEndpoints::<2,4>::new();
        coordinator.bind_live_udp(&mut sockets,Owner{
            pid:init.pid,generation:init.generation},5501).unwrap();
        coordinator.bind_live_udp(&mut sockets,Owner{
            pid:peer.pid,generation:peer.generation},5502).unwrap();
        let mut leases=LeaseTable::<2>::new();

        assert_eq!(retire_full_owner_epoch(&mut files,&mut notices,
            &mut coordinator,&mut sockets,&mut leases,init,
            EventKind::Exited),Err(()));
        assert!(files.owns(init));
        assert!(files.owns(peer));
        assert!(coordinator.authorize_udp(Owner{
            pid:init.pid,generation:init.generation},5501).is_ok());
        assert!(coordinator.authorize_udp(Owner{
            pid:peer.pid,generation:peer.generation},5502).is_ok());
        assert!(coordinator.live_sockets_consistent(&sockets));
        assert_eq!(notices.peek_receipt(),Some(old_receipt));
        assert_eq!(notices.acknowledge_receipt(old_receipt),
                   Some(old_receipt.event));

        assert_eq!(retire_full_owner_epoch(&mut files,&mut notices,
            &mut coordinator,&mut sockets,&mut leases,init,
            EventKind::Exited),Ok(true));
        assert!(!files.owns(init));
        assert!(files.owns(peer));
        assert!(coordinator.authorize_udp(Owner{
            pid:init.pid,generation:init.generation},5501).is_err());
        assert!(coordinator.authorize_udp(Owner{
            pid:peer.pid,generation:peer.generation},5502).is_ok());
        assert!(coordinator.live_sockets_consistent(&sockets));
        let new_notice=notices.peek_receipt().unwrap();
        assert_eq!(new_notice.event.pid,init.pid);
        assert_eq!(new_notice.event.value,init.generation);
        assert_eq!(new_notice.event.kind,EventKind::Exited);
    }
    #[test]
    fn mismatched_socket_registry_and_stale_pid_are_all_or_none() {
        let mut files=FileHandles::new();
        let owner=files.bind_process(1).unwrap();
        let mut notices=ProcessEvents::<2>::new();
        let mut coordinator=ProcessRetirement::<2,2,4>::new();
        let proof=Owner{pid:owner.pid,generation:owner.generation};
        coordinator.start_exact(proof).unwrap();
        let mut sockets=EpochUdpEndpoints::<2,2>::new();
        coordinator.bind_live_udp(&mut sockets,proof,5501).unwrap();
        let mut leases=LeaseTable::<2>::new();
        // An unchecked endpoint bypassing its canonical policy registry
        // must deny all five-table publication without touching FileHandles.
        sockets.bind(owner.pid,owner.generation,5502).unwrap();
        assert_eq!(retire_full_owner_epoch(&mut files,&mut notices,
            &mut coordinator,&mut sockets,&mut leases,owner,
            EventKind::Faulted),Err(()));
        assert!(files.owns(owner));
        assert_eq!(notices.len(),0);
        assert_eq!(coordinator.active_generation(owner.pid),
                   Some(owner.generation));
        assert_eq!(sockets.pending_owned(owner.pid,owner.generation),Ok(0));
        // A fresh service with a new incarnation cannot be retired by an old
        // delayed scheduler notice.
        files.retire_process_batch_kernel(&[owner]).unwrap();
        let next=files.bind_process(1).unwrap();
        assert_ne!(owner.generation,next.generation);
        assert_eq!(retire_full_owner_epoch(&mut files,&mut notices,
            &mut coordinator,&mut sockets,&mut leases,owner,
            EventKind::Faulted),Ok(false));
        assert!(files.owns(next));
    }
}

#[cfg(all(test,feature="test-r031-files-gui"))]
mod r118_batched_real_scheduler_teardown_tests{
    use super::*;
    use crate::batch5_session::Identity;
    use crate::fs_handles::FileHandles;
    use crate::process_events::{EventKind,ProcessEvents};
    use crate::process_retirement_txn::{Owner,ProcessRetirement};
    use crate::network::EpochUdpEndpoints;
    use crate::agent_03_cap_epoch::LeaseTable;

    fn setup()->(
        FileHandles,ProcessRetirement<2,2,4>,
        EpochUdpEndpoints<2,4>,LeaseTable<2>,[Identity;2]
    ){
        let mut files=FileHandles::new();
        let a=files.bind_process(1).unwrap();
        let b=files.bind_process(2).unwrap();
        let mut c=ProcessRetirement::<2,2,4>::new();
        c.start_exact(Owner{pid:a.pid,generation:a.generation}).unwrap();
        c.start_exact(Owner{pid:b.pid,generation:b.generation}).unwrap();
        let mut sockets=EpochUdpEndpoints::<2,4>::new();
        c.bind_live_udp(&mut sockets,
            Owner{pid:a.pid,generation:a.generation},5501).unwrap();
        c.bind_live_udp(&mut sockets,
            Owner{pid:b.pid,generation:b.generation},5502).unwrap();
        (files,c,sockets,LeaseTable::<2>::new(),[a,b])
    }
    #[test]
    fn two_live_ring3_incarnations_retire_as_one_with_ordered_pid1_events(){
        let (mut files,mut coordinator,mut sockets,mut leases,owners)=setup();
        let mut notices=ProcessEvents::<3>::new();
        let receipt=files.selected_operation_guard(owners[1]).unwrap();
        assert_eq!(retire_full_owner_batch(
            &mut files,&mut notices,&mut coordinator,&mut sockets,
            &mut leases,&[owners[1],owners[0]],EventKind::Faulted),Ok(2));
        assert!(!files.owns(owners[0]) && !files.owns(owners[1]));
        assert!(!files.selected_operation_guard_is_current(receipt));
        assert!(coordinator.live_sockets_consistent(&sockets));
        assert_eq!(coordinator.active_udp_ports(),0);
        assert_eq!(coordinator.queued_notices(),0);
        assert_eq!(notices.len(),2);
        for expected in [owners[1],owners[0]]{
            let token=notices.peek_receipt().unwrap();
            assert_eq!(token.event.pid,expected.pid);
            assert_eq!(token.event.value,expected.generation);
            assert_eq!(token.event.kind,EventKind::Faulted);
            assert_eq!(notices.acknowledge_receipt(token),Some(token.event));
        }
    }
    #[test]
    fn late_stale_or_duplicate_owner_does_not_retire_any_live_peer(){
        let (mut files,mut coordinator,mut sockets,mut leases,owners)=setup();
        let mut notices=ProcessEvents::<2>::new();
        let stale=Identity{generation:owners[1].generation+1,..owners[1]};
        for batch in [
            [owners[0],stale],
            [owners[0],owners[0]],
            [Identity{pid:0,..owners[0]},owners[1]],
        ]{
            assert_eq!(retire_full_owner_batch(
                &mut files,&mut notices,&mut coordinator,&mut sockets,
                &mut leases,&batch,EventKind::Exited),Err(()));
            assert!(files.owns(owners[0])&&files.owns(owners[1]));
            assert!(coordinator.live_sockets_consistent(&sockets));
            assert_eq!(coordinator.active_udp_ports(),2);
            assert_eq!(notices.len(),0);
        }
    }
    #[test]
    fn late_event_backpressure_does_not_publish_a_partial_two_owner_exit(){
        let (mut files,mut coordinator,mut sockets,mut leases,owners)=setup();
        let mut notices=ProcessEvents::<1>::new();
        assert_eq!(retire_full_owner_batch(
            &mut files,&mut notices,&mut coordinator,&mut sockets,
            &mut leases,&owners,EventKind::Exited),Err(()));
        assert!(files.owns(owners[0]) && files.owns(owners[1]));
        assert!(coordinator.live_sockets_consistent(&sockets));
        assert_eq!(coordinator.active_udp_ports(),2);
        assert_eq!(notices.len(),0);
    }
}
