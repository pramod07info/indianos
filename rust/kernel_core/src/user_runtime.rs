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
const PHYS:u64=0x000f_ffff_ffff_f000;
const PAGE:u64=4096;
const YIELD:u64=0x200;
const WORDS:usize=20;
const TASKS:usize=2;
const BAD:u64=u64::MAX;
#[repr(C)]
struct DirectMap{offset:u64}
#[repr(C,align(16))]
struct Task{
 image:UserImage,root:CloneResult,pcb:*mut Process,
 frame:[u64;WORDS],saved:bool,attached:bool,timer_preemptions:u32,
}
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
// Quantum jobs belong to kernel-verified process IDs, not user-provided IDs.
// Single-core runtime only; do not access this from interrupt context.
static mut QUANTUM_JOBS:crate::quantum_service::QuantumService=
    crate::quantum_service::QuantumService::new();
fn cleanup_quantum_jobs(pid:u32){
 if pid==0{return;}
 let jobs=unsafe{&mut *ptr::addr_of_mut!(QUANTUM_JOBS)};
 let _=jobs.process_terminated(pid);
}

static mut M009_TASKS:[MaybeUninit<Task>;TASKS]=[const{MaybeUninit::uninit()};TASKS];
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
 #[cfg(feature="preempt")] fn x86_64_timer_start_100hz()->bool;
 #[cfg(feature="preempt")] fn x86_64_timer_irq_count()->u64;
 #[cfg(feature="preempt")] fn x86_64_timer_pic_mask()->u8;
 #[cfg(feature="preempt")] fn x86_64_timer_pic_pending()->u8;
 #[cfg(feature="preempt")] fn x86_64_timer_irq0_state()->u8;
 #[cfg(feature="preempt")] fn x86_64_timer_irq0_blockers()->u8;
 #[cfg(feature="preempt")] fn x86_64_timer_apic_mode_flags()->u8;
 #[cfg(feature="preempt")] fn x86_64_timer_initial_apic_mode()->u8;
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
   if unsafe{ACTIVE_PID}!=1 ||
       !unsafe{indianos_user_can_write(&*img,request.arg0,1)}{return BAD;}
   let Some(byte)=input::poll_byte()else{return 0;};
   if !unsafe{indianos_user_copy_writable(&*img,request.arg0,&byte,1)}{
       return BAD;
   }
   input::echo(byte);
   1
  },
  #[cfg(feature="test-shell")]
  4=>unsafe{ACTIVE_PID as u64},
  #[cfg(feature="test-shell")]
  5=>{
   unsafe{
    if ACTIVE_PID!=1||PENDING_WORKER{return BAD;}
    PENDING_WORKER=true;
   }
   log(b"IndianOS: R013 PID 1 requested /bin/worker\n\0");
   0
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
   let completed=if v==0{unsafe{indianos_process_exit(process,status)}}
   else{unsafe{indianos_process_fault(process,status,v,e,a)}};
   if !completed{
    log(b"IndianOS: FATAL PCB terminal state rejected\n\0");
    }else{
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
 let before=unsafe{pmm_free_frames()};
 for i in 0..TASKS{
  unsafe{ptr::write(task_ptr(i),core::mem::zeroed())};
 }
 let prepared=prepare(task_ptr(0),p,first,first_len,cpu.cr3&PHYS,&vmm,&backend,0)&&
  prepare(task_ptr(1),q,second,second_len,cpu.cr3&PHYS,&vmm,&backend,1);
 if !prepared||!unsafe{indianos_scheduler_init(&mut scheduler,&mut table)}{
  log(b"IndianOS: M009 FATAL cannot prepare two user contexts\n\0");
  let _=discard(task_ptr(1),&backend,false);
  let _=discard(task_ptr(0),&backend,false);
  return false;
 }
 log(b"IndianOS: M009 two simultaneous user address spaces ready\n\0");
 #[cfg(feature="preempt")]
 {
  if !unsafe{x86_64_timer_start_100hz()}{
   log(b"IndianOS: M009 FATAL cannot configure PIC/PIT timer\n\0");
   return false;
  }
  log(b"IndianOS: M009 hardware PIT IRQ0 enabled at 100 Hz\n\0");
 }
 let mut ok=true;
 while ok&&!unsafe{indianos_scheduler_all_terminal(&scheduler)}{
  let pcb=unsafe{indianos_scheduler_next(&mut scheduler)};
  if pcb.is_null(){ok=false;break;}
  let pid=unsafe{(*pcb).pid};
  if pid<1||pid>2{ok=false;break;}
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
  let status=if task.saved{
   unsafe{x86_64_resume_user(task.frame.as_ptr(),task.root.root)}
  }else{
   unsafe{x86_64_enter_user(task.image.plan.entry,
                             task.image.plan.stack_end-16,task.root.root)}
  };
  unsafe{ACTIVE_TASK=ptr::null_mut();ACTIVE_USER=ptr::null_mut();}
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
   let finished=if v!=0{
    unsafe{indianos_process_fault(pcb,status,v,e,a)}
   }else{unsafe{indianos_process_exit(pcb,status)}};
   if !finished{ok=false;break;}
   pid_log(pid,if v==0{&b"EXITED\0"[..]}else{&b"FAULTED\0"[..]});
   if !discard(task as *mut Task,&backend,true){ok=false;break;}
  }
 }
 if ok&&(!unsafe{indianos_scheduler_all_terminal(&scheduler)} ||
         !unsafe{indianos_process_success(p)} ||
         !unsafe{indianos_process_success(q)} ||
         unsafe{pmm_free_frames()}!=before){ok=false;}
 #[cfg(feature="preempt")]
 {
  if unsafe{x86_64_timer_irq_count()}==0{
   log(b"IndianOS: M009 PIC/PIT delivered zero timer IRQs\n\0");
   let initial_apic=unsafe{x86_64_timer_initial_apic_mode()};
   if initial_apic&2!=0{
    log(b"IndianOS: M010 firmware x2APIC mode active\n\0")
   }else if initial_apic&1!=0{
    log(b"IndianOS: M010 firmware xAPIC mode active\n\0")
   }else{
    log(b"IndianOS: M010 firmware local APIC disabled\n\0")
   }
   let apic=unsafe{x86_64_timer_apic_mode_flags()};
   if apic&2!=0{
    log(b"IndianOS: M010 x2APIC mode active\n\0")
   }else if apic&1!=0{
    log(b"IndianOS: M010 xAPIC mode active\n\0")
   }else{
    log(b"IndianOS: M010 local APIC disabled\n\0")
   }
   let blockers=unsafe{x86_64_timer_irq0_blockers()};
   if blockers&1!=0{log(b"IndianOS: R019 IRQ0 masked at PIC\n\0");}
   if blockers&2!=0{log(b"IndianOS: R019 IRQ0 pending but not in service\n\0");}
   if blockers&4!=0{log(b"IndianOS: R019 IRQ0 in PIC service\n\0");}
   if blockers&8!=0{log(b"IndianOS: R019 local APIC still enabled\n\0");}
   if blockers&16!=0{log(b"IndianOS: R019 firmware initially used x2APIC\n\0");}
   let irq0=unsafe{x86_64_timer_irq0_state()};
   if irq0&4!=0{
    log(b"IndianOS: M010 PIC IRQ0 unexpectedly masked\n\0")
   }
   if irq0&1!=0{
    log(b"IndianOS: M010 PIC IRQ0 pending (IRR=1)\n\0")
   }else{
    log(b"IndianOS: M010 PIC IRQ0 not pending (IRR=0)\n\0")
   }
   if irq0&2!=0{
    log(b"IndianOS: M010 PIC IRQ0 in service (ISR=1)\n\0")
   }else{
    log(b"IndianOS: M010 PIC IRQ0 not in service (ISR=0)\n\0")
   }
  }else{log(b"IndianOS: M009 PIC/PIT hardware timer interrupts observed\n\0")}
  if scheduler.yield_count<2||scheduler.tick_count<2||
     unsafe{(*task_ptr(0)).timer_preemptions}==0||
     unsafe{(*task_ptr(1)).timer_preemptions}==0||
     unsafe{x86_64_timer_irq_count()}<2{ok=false;}
  if ok{log(b"IndianOS: M009 timer-preemptive round-robin PASSED\n\0");}
 }
 #[cfg(not(feature="preempt"))]
 {
  if scheduler.dispatch_count!=6||scheduler.yield_count!=4||
     scheduler.tick_count!=4{ok=false;}
  if ok{log(b"IndianOS: M009 round-robin cooperative switching PASSED\n\0");}
 }
 if !ok{
  log(b"IndianOS: M009 FATAL scheduler or memory ownership verification\n\0");
  // On normal dispatch failure CR3 has been restored to the kernel.
  // Reclaim both remaining images, even if their PCB state was rejected.
  let first=discard(task_ptr(0),&backend,false);
  let second=discard(task_ptr(1),&backend,false);
  if !first||!second||unsafe{pmm_free_frames()}!=before{
   log(b"IndianOS: M009 FATAL partial rollback: memory not fully reclaimed\n\0");
  }
 }
 ok
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn saved_frame_requires_correct_privileges(){
  assert_eq!(WORDS,20);assert_eq!(TASKS,2);
  assert_eq!(core::mem::align_of::<Task>(),16);
  let mut t:Task=unsafe{core::mem::zeroed()};
  t.saved=true;t.frame[16]=0x2b;t.frame[19]=0x33;
  t.frame[17]=2;
  assert!(!saved_valid(&t));
 }
}
