//! Freestanding Rust-owned Limine kernel entry and milestone orchestration.
//!
//! Limine enters long mode with a valid kernel stack. No heap or libc.
//! All CPU-bound hardware interrupt entry stubs remain external assembly.
//! Feature flags keep every existing QEMU negative and ring-3 test distinct.
use core::{arch::asm,ffi::{c_char,c_void},ptr};
use crate::{Process,ProcessTable,indianos_process_table_init,
            indianos_process_create,indianos_process_success};
use crate::boot::Region;
use crate::paging::CpuState;
unsafe extern "C" {
    fn serial_init();
    fn serial_write(message:*const c_char);
    fn boot_validate()->bool;
    fn boot_draw_status();
    fn boot_initramfs_file(name:*const c_char,data:*mut *const u8,size:*mut usize)->bool;
    fn boot_memory_region_count()->u64;
    fn boot_memory_region_at(index:u64,out:*mut Region,context:*mut c_void)->bool;
    fn x86_64_exceptions_init()->bool;
    fn pmm_initialize(count:u64,callback:Option<unsafe extern "C" fn(u64,*mut Region,*mut c_void)->bool>,context:*mut c_void)->bool;
    fn pmm_self_test()->bool;
    fn vmm_boot_self_test()->bool;
    fn x86_64_paging_detect(state:*mut CpuState)->bool;
    fn paging_preflight(state:*const CpuState)->bool;
    fn x86_64_paging_shadow_switch_test()->bool;
    fn indianos_program_self_test()->bool;
    fn indianos_syscall_self_test()->bool;
    fn user_image_boot_stage_test(elf:*const u8,size:usize)->bool;
    fn x86_64_user_demo_run(elf:*const u8,size:usize)->bool;
    fn x86_64_user_process_run(process:*mut Process,elf:*const u8,size:usize)->bool;
    fn x86_64_m009_schedule_run(a:*const u8,asize:usize,b:*const u8,bsize:usize)->bool;
    #[cfg(feature="test-shell")]
    fn indianos_r013_take_worker_request()->bool;
}
fn log(msg:&'static [u8]){
    unsafe{serial_write(msg.as_ptr().cast())};
}
#[inline(never)]
fn halt()->!{
    loop{unsafe{asm!("cli; hlt",options(nostack,nomem))}}
}
fn need(ok:bool,message:&'static [u8]){
    if !ok{log(message);halt()}
}
fn module(name:&'static [u8])->Option<(*const u8,usize)>{
    let mut data:*const u8=ptr::null();
    let mut len:usize=0;
    if !unsafe{boot_initramfs_file(name.as_ptr().cast(),&mut data,&mut len)} {
        return None;
    }
    Some((data,len))
}
#[no_mangle]
pub extern "C" fn kmain()->!{
    unsafe{asm!("cli",options(nomem,nostack,preserves_flags))};
    unsafe{serial_init();}
    log(b"IndianOS: kernel entered\n\0");
    need(unsafe{boot_validate()},b"IndianOS: FATAL boot information invalid\n\0");
    log(b"IndianOS: boot information validated\n\0");
    unsafe{boot_draw_status();}
    #[cfg(feature="test-irq-keyboard")]
    {
        need(crate::console::init(),
             b"IndianOS: FATAL R014.2 framebuffer text console unavailable\n\0");
        log(b"IndianOS: R014.2 framebuffer text console ready\n\0");
    }
    log(b"IndianOS: milestone 001 complete\n\0");

    need(unsafe{x86_64_exceptions_init()},
         b"IndianOS: FATAL exception table installation failed\n\0");
    log(b"IndianOS: exception handlers ready\n\0");
    need(unsafe{pmm_initialize(boot_memory_region_count(),
                      Some(boot_memory_region_at),ptr::null_mut())},
         b"IndianOS: FATAL physical memory initialization failed\n\0");
    need(unsafe{pmm_self_test()},
         b"IndianOS: FATAL physical memory self-test failed\n\0");
    log(b"IndianOS: physical memory manager ready\n\0");

    #[cfg(feature="test-ud2")]
    {
        log(b"IndianOS: triggering intentional UD2\n\0");
        unsafe{asm!("ud2",options(nomem,nostack))};
    }
    log(b"IndianOS: milestone 002 complete\n\0");

    need(unsafe{vmm_boot_self_test()},
         b"IndianOS: FATAL virtual memory self-test failed\n\0");
    log(b"IndianOS: detached virtual memory manager ready\n\0");
    log(b"IndianOS: milestone 003 complete\n\0");
    let mut paging=CpuState::default();
    need(unsafe{x86_64_paging_detect(&mut paging)},
         b"IndianOS: FATAL paging capability discovery failed\n\0");
    log(b"IndianOS: paging capabilities inspected\n\0");
    if unsafe{paging_preflight(&paging)} {
        log(b"IndianOS: shadow paging preflight passed\n\0");
    } else {
        log(b"IndianOS: shadow paging switch not permitted\n\0");
    }
    #[cfg(feature="test-shadow")]
    {
        need(unsafe{x86_64_paging_shadow_switch_test()},
             b"IndianOS: FATAL shadow CR3 switch failed\n\0");
        log(b"IndianOS: shadow CR3 switched and restored\n\0");
    }
    log(b"IndianOS: milestone 004 complete\n\0");
    need(unsafe{indianos_program_self_test() && indianos_syscall_self_test()},
         b"IndianOS: FATAL user image contract self-test failed\n\0");
    log(b"IndianOS: ELF64 user-image validation ready\n\0");
    log(b"IndianOS: syscall ABI metadata validation ready\n\0");
    log(b"IndianOS: milestone 005 foundation complete\n\0");
    let (init,init_size)=module(b"/bin/init\0").unwrap_or_else(||{
        log(b"IndianOS: FATAL initramfs /bin/init is unavailable or invalid\n\0");
        halt()
    });
    log(b"IndianOS: read-only initramfs /bin/init validated\n\0");
    need(unsafe{user_image_boot_stage_test(init,init_size)},
         b"IndianOS: FATAL detached user-image staging failed\n\0");
    log(b"IndianOS: initramfs /bin/init staged and released safely\n\0");

    #[cfg(feature="test-shell")]
    {
        // Scripted ring-3 init -> validated spawn -> distinct worker lifetime.
        log(b"IndianOS: R013 init shell begin\n\0");
        #[cfg(feature="test-irq-keyboard")]
        {
            need(crate::input::enable_irq1(),
                 b"IndianOS: FATAL R014.2 PS/2 IRQ1 initialization failed\n\0");
            log(b"IndianOS: R014.2 IRQ1 keyboard controller armed\n\0");
        }
        let mut table=ProcessTable::empty();
        need(unsafe{indianos_process_table_init(&mut table)},
             b"IndianOS: FATAL R013 process table init\n\0");
        let init_pcb=unsafe{indianos_process_create(&mut table)};
        need(!init_pcb.is_null() && unsafe{(*init_pcb).pid}==1,
             b"IndianOS: FATAL R013 PID1 allocation\n\0");
        need(unsafe{x86_64_user_process_run(init_pcb,init,init_size) &&
                    indianos_process_success(init_pcb)},
             b"IndianOS: FATAL R013 init shell failed\n\0");
        #[cfg(feature="test-irq-keyboard")]
        {
            need(crate::input::irq_count()>0,
                 b"IndianOS: FATAL R014.2 no physical PS/2 IRQ1 delivered\n\0");
            log(b"IndianOS: R014.2 PS/2 IRQ1 delivery verified\n\0");
        }
        need(unsafe{indianos_r013_take_worker_request()},
             b"IndianOS: FATAL R013 no worker spawn request\n\0");
        log(b"IndianOS: R013 launching /bin/worker\n\0");
        let (worker,worker_size)=module(b"/bin/worker\0").unwrap_or_else(||{
            log(b"IndianOS: FATAL R013 worker missing in initramfs\n\0");
            halt()
        });
        let child=unsafe{indianos_process_create(&mut table)};
        need(!child.is_null() && unsafe{(*child).pid}==2,
             b"IndianOS: FATAL R013 PID2 allocation\n\0");
        need(unsafe{x86_64_user_process_run(child,worker,worker_size) &&
                    indianos_process_success(child)},
             b"IndianOS: FATAL R013 worker failed or leaked frames\n\0");
        log(b"IndianOS: R013 worker exited and reaped\n\0");
        log(b"IndianOS: R013 shell session complete\n\0");
    }

    #[cfg(feature="test-scheduler")]
    {
        let (worker,worker_size)=module(b"/bin/worker\0").unwrap_or_else(||{
            log(b"IndianOS: FATAL M009 worker executable not found\n\0");
            halt()
        });
        log(b"IndianOS: M009 cooperative scheduler experiment begin\n\0");
        need(unsafe{x86_64_m009_schedule_run(init,init_size,worker,worker_size)},
             b"IndianOS: FATAL M009 cooperative context test failed\n\0");
        log(b"IndianOS: M009 scheduler experiment complete\n\0");
    }

    #[cfg(feature="test-processes")]
    {
        log(b"IndianOS Rust: M008 starting worker lookup\n\0");
        let (worker,worker_size)=module(b"/bin/worker\0").unwrap_or_else(||{
            log(b"IndianOS: FATAL second user executable missing\n\0");
            halt()
        });
        log(b"IndianOS Rust: M008 worker lookup succeeded; initializing PCB table\n\0");
        let mut table=ProcessTable::empty();
        need(unsafe{indianos_process_table_init(&mut table)},
             b"IndianOS: FATAL process table initialization failed\n\0");
        log(b"IndianOS Rust: M008 PCB table initialized; creating PIDs\n\0");
        let first=unsafe{indianos_process_create(&mut table)};
        log(b"IndianOS Rust: M008 first PID returned\n\0");
        let second=unsafe{indianos_process_create(&mut table)};
        log(b"IndianOS Rust: M008 second PID returned\n\0");
        if first.is_null() || second.is_null() ||
            unsafe{(*first).pid}!=1 || unsafe{(*second).pid}!=2 {
            log(b"IndianOS: FATAL process ID assignment failed\n\0");halt()
        }
        log(b"IndianOS: M008 sequential process launch begin\n\0");
        need(unsafe{x86_64_user_process_run(first,init,init_size) &&
               indianos_process_success(first)},
             b"IndianOS: FATAL first process did not exit and reap\n\0");
        need(unsafe{x86_64_user_process_run(second,worker,worker_size) &&
               indianos_process_success(second)},
             b"IndianOS: FATAL second process did not exit and reap\n\0");
        log(b"IndianOS: M008 two independent processes exited and reaped\n\0");
    }

    #[cfg(any(feature="test-ring3",feature="test-fault-guard",
              feature="test-fault-supervisor"))]
    {
        need(unsafe{x86_64_user_demo_run(init,init_size)},
             b"IndianOS: FATAL ring-3 user program experiment failed\n\0");
        #[cfg(any(feature="test-fault-guard",feature="test-fault-supervisor"))]
        log(b"IndianOS: user fault isolation experiment complete\n\0");
        #[cfg(not(any(feature="test-fault-guard",feature="test-fault-supervisor")))]
        log(b"IndianOS: ring-3 user program experiment complete\n\0");
    }
    #[cfg(feature="test-pagefault")]
    {
        log(b"IndianOS: triggering intentional page fault\n\0");
        unsafe {
            asm!("mov rax, qword ptr [rax]",
                 inout("rax")0x0000_7fff_0000_0000u64=>_,
                 options(nostack,readonly));
        }
    }
    halt()
}
