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
    #[cfg(feature="test-r029-desktop-suite")]
    fn x86_64_r029_desktop_suite_run(a:*const u8,al:usize,b:*const u8,bl:usize,c:*const u8,cl:usize)->bool;
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
    // Issue #124 HD1: each x86-64 boot enumerates CPU and PCI-PCI bridge
    // topology before optional device drivers. No PCI command writes,
    // BAR mapping, device init or implicit quantum/cloud connection.
    #[cfg(all(target_arch="x86_64",target_os="none"))]
    {
        use crate::{hardware_inventory as hw,hardware_pcie as topo};
        need(hw::inspect_cpu(&hw::native::BootCpu).is_ok(),
            b"IndianOS: FATAL unsupported x86-64 CPU capabilities\n\0");
        match topo::native::detect_boot(){
            Ok(profile)=>{
                let mut report=[0u8;320];
                need(topo::format_report(&profile,&mut report).is_ok(),
                    b"IndianOS: FATAL HD1 hardware report buffer\n\0");
                unsafe{serial_write(report.as_ptr().cast())};
                // HD2 re-probes immutable PCI identity fields, WITHOUT
                // loading unsigned drivers or enabling MMIO/bus mastering.
                let mut ids=crate::hardware_driver_policy::native::ReadOnlyIds;
                let preflight=crate::hardware_driver_policy::preflight(
                    &profile.inventory,&mut ids);
                let mut hd2=[0u8;160];
                need(crate::hardware_driver_policy::format_report(
                    &preflight,&mut hd2).is_ok(),
                    b"IndianOS: FATAL HD2 driver preflight report\n\0");
                unsafe{serial_write(hd2.as_ptr().cast())};
            },
            Err(_)=>{
                // CPU-only boot continues when an optional PCI bridge is
                // absent, malformed or cannot be probed safely.
                log(b"IndianOS HD0 CPU supported; GPU=unknown; NPU=unknown; QPU=unavailable; CPU fallback=enabled\n\0");
                log(b"IndianOS HD1 PCI topology unavailable; no optional accelerator driver activated\n\0");
                log(b"IndianOS HD2 driver catalog=unavailable; signed candidates=0; GPU/NPU drivers=not activated; QPU=unavailable; CPU fallback=enabled\n\0");
            }
        }
    }
    // HD3A: no driver activation without authenticated firmware roots,
    // real IOMMU isolation, and a tested user-mode driver runtime.
    log(crate::hardware_driver_stage::boot_driver_status());
    #[cfg(feature="test-r041-workspace")]
    {
        need(crate::batch7_workspace::bounded_workspace_smoke(),
             b"IndianOS: FATAL R041 workspace read/write/revocation smoke\n\0");
        log(b"IndianOS: R041 RAM workspace terminal write/read/revocation PASSED\n\0");
    }
    unsafe{boot_draw_status();}
    #[cfg(feature="test-r029-userspace")]
    {
        need(crate::gui::user_surface_start(),
             b"IndianOS: FATAL R029 ring3 surface server unavailable\n\0");
    }
    #[cfg(all(feature="test-r029-desktop",not(feature="test-r029-userspace")))]
    {
        log(b"IndianOS: R029 QEMU graphical desktop start\n\0");
        need(crate::gui::run(),
            b"IndianOS: FATAL R029 compositor or interactive input failed\n\0");
        log(b"IndianOS: R029 desktop shutdown completed\n\0");
    }
    #[cfg(feature="test-irq-keyboard")]
    {
        need(crate::console::init(),
             b"IndianOS: FATAL R014.2 framebuffer text console unavailable\n\0");
        log(b"IndianOS: R014.2 framebuffer text console ready\n\0");
    }
    #[cfg(feature="test-ata-disk")]
    {
        use crate::block::{BlockDevice, BlockError, SECTOR_BYTES};
        log(b"IndianOS: R015 ATA disk validation begin\n\0");
        // Only enabled for an isolated QEMU guest with a disposable test disk.
        let mut disk=unsafe{crate::ata_pio::AtaPio::probe()}.unwrap_or_else(|_|{
            log(b"IndianOS: FATAL R015 ATA probe failed\n\0");halt()
        });
        need(disk.sector_count()>=16,b"IndianOS: FATAL R015 ATA capacity\n\0");
        log(b"IndianOS: R015 ATA disk detected\n\0");
        let mut original=[0u8;SECTOR_BYTES];
        need(disk.read_sector(8,&mut original).is_ok(),
             b"IndianOS: FATAL R015 ATA initial read\n\0");
        let mut pattern=[0u8;SECTOR_BYTES];
        for (i,byte) in pattern.iter_mut().enumerate(){
            *byte=(i as u8).wrapping_mul(37).wrapping_add(0x5a);
        }
        need(disk.write_sector(8,&pattern).is_ok(),
             b"IndianOS: FATAL R015 ATA write\n\0");
        let mut actual=[0u8;SECTOR_BYTES];
        need(disk.read_sector(8,&mut actual).is_ok() && actual==pattern,
             b"IndianOS: FATAL R015 ATA readback mismatch\n\0");
        need(disk.read_sector(disk.sector_count(),&mut actual)==Err(BlockError::OutOfRange),
             b"IndianOS: FATAL R015 ATA bounds check\n\0");
        need(disk.write_sector(8,&original).is_ok(),
             b"IndianOS: FATAL R015 ATA restore\n\0");
        log(b"IndianOS: R015 ATA disk read write restore PASSED\n\0");
        // R027: QEMU disposable IDE disk ONLY. The test runner creates
        // a 32-sector temporary file and checks every byte after this test.
        // No normal boot or physical drive can enter this feature path.
        use crate::block::BlockWindow;
        use crate::tinyfs::{TinyFs, FsError};
        const VFS_START: u64 = 16;
        const VFS_SIZE: usize = 5;
        need(disk.sector_count() >= VFS_START + VFS_SIZE as u64,
             b"IndianOS: FATAL R027 disk too small\n\0");
        let mut saved = [[0u8; SECTOR_BYTES]; VFS_SIZE];
        for i in 0..VFS_SIZE {
            need(disk.read_sector(VFS_START + i as u64, &mut saved[i]).is_ok(),
                 b"IndianOS: FATAL R027 snapshot read failed\n\0");
        }
        let mut neighbor = [0u8; SECTOR_BYTES];
        need(disk.read_sector(VFS_START - 1, &mut neighbor).is_ok(),
             b"IndianOS: FATAL R027 neighbor read failed\n\0");
        {
            let window = BlockWindow::new(&mut disk, VFS_START, VFS_SIZE as u64)
                .unwrap_or_else(|_| { log(b"IndianOS: FATAL R027 window\n\0"); halt() });
            let mut fs = TinyFs::format(window)
                .unwrap_or_else(|_| { log(b"IndianOS: FATAL R027 format\n\0"); halt() });
            need(fs.create_path("//kernel/./", b"IndianOS persistent QEMU file").is_ok(),
                 b"IndianOS: FATAL R027 create\n\0");
            need(fs.create_path("/second", b"separate file").is_ok(),
                 b"IndianOS: FATAL R027 second create\n\0");
            need(fs.create_path("/../escape", b"bad") == Err(FsError::InvalidName),
                 b"IndianOS: FATAL R027 traversal\n\0");
            let mut readback = [0u8; SECTOR_BYTES];
            need(fs.read_path("/kernel", &mut readback) == Ok(29) &&
                 &readback[..29] == &b"IndianOS persistent QEMU file"[..],
                 b"IndianOS: FATAL R027 readback\n\0");
            let mut fs = TinyFs::mount(fs.into_device())
                .unwrap_or_else(|_| { log(b"IndianOS: FATAL R027 remount\n\0"); halt() });
            readback.fill(0);
            need(fs.read_path("/./second", &mut readback) == Ok(13) &&
                 &readback[..13] == &b"separate file"[..],
                 b"IndianOS: FATAL R027 remount readback\n\0");
            need(fs.delete_path("/kernel").is_ok() &&
                 fs.read_path("/kernel", &mut readback) == Err(FsError::NotFound),
                 b"IndianOS: FATAL R027 delete\n\0");
            need(fs.create_path("/replacement", b"reused").is_ok(),
                 b"IndianOS: FATAL R027 reuse\n\0");
        }
        for i in 0..VFS_SIZE {
            need(disk.write_sector(VFS_START + i as u64, &saved[i]).is_ok(),
                 b"IndianOS: FATAL R027 snapshot restore\n\0");
        }
        let mut after = [0u8; SECTOR_BYTES];
        need(disk.read_sector(VFS_START - 1, &mut after).is_ok() && after == neighbor,
             b"IndianOS: FATAL R027 neighbor corruption\n\0");
        for i in 0..VFS_SIZE {
            need(disk.read_sector(VFS_START + i as u64, &mut after).is_ok() &&
                 after == saved[i],
                 b"IndianOS: FATAL R027 restore mismatch\n\0");
        }
        log(b"IndianOS: R027 TinyFs ATA QEMU create remount delete restore PASSED\n\0");

    }
    #[cfg(feature="test-r027-durable")]
    {
        use crate::block::{BlockDevice,BlockWindow,SECTOR_BYTES};
        use crate::durablefs::{DurableFs,FsError,MAX_FILE_BYTES,MIN_SECTORS};
        // The host test creates an EXCLUSIVE 128-sector disposable raw disk.
        // There is no automatic filesystem mounting in ordinary boots.
        const FIRST:u64=24;
        const COUNT:u64=100;
        log(b"IndianOS: R027 persistent QEMU ATA disk probe begin\n\0");
        let mut disk=unsafe{crate::ata_pio::AtaPio::probe()}.unwrap_or_else(|_|{
            log(b"IndianOS: FATAL R027 ATA IDENTIFY failed\n\0");halt()
        });
        need(disk.sector_count()==128 && COUNT>=MIN_SECTORS,
             b"IndianOS: FATAL R027 unexpected disposable disk capacity\n\0");
        log(b"IndianOS: R027 ATA LBA28 primary-master discovered, 128 sectors\n\0");
        let mut left=[0u8;SECTOR_BYTES];
        let mut right=[0u8;SECTOR_BYTES];
        need(disk.read_sector(FIRST-1,&mut left).is_ok() &&
             disk.read_sector(FIRST+COUNT,&mut right).is_ok(),
             b"IndianOS: FATAL R027 neighbor snapshot failed\n\0");
        let mut meta_a=[0u8;SECTOR_BYTES];
        let mut meta_b=[0u8;SECTOR_BYTES];
        need(disk.read_sector(FIRST,&mut meta_a).is_ok() &&
             disk.read_sector(FIRST+1,&mut meta_b).is_ok(),
             b"IndianOS: FATAL R027 initial superblock read failed\n\0");
        let is_blank=meta_a.iter().all(|&b|b==0) && meta_b.iter().all(|&b|b==0);
        {
            let window=BlockWindow::new(&mut disk,FIRST,COUNT).unwrap_or_else(|_|{
                log(b"IndianOS: FATAL R027 invalid block partition\n\0");halt()
            });
            let mut fs=if is_blank {
                DurableFs::format_blank(window).unwrap_or_else(|_|{
                    log(b"IndianOS: FATAL R027 format of blank test disk failed\n\0");halt()
                })
            }else{
                DurableFs::mount(window).unwrap_or_else(|_|{
                    // Important: never format if an existing disk is corrupt.
                    log(b"IndianOS: FATAL R027 existing filesystem invalid; no reformat\n\0");halt()
                })
            };
            let mut large=[0u8;MAX_FILE_BYTES];
            let original_len=1701usize;
            let updated_len=1923usize;
            if is_blank {
                for (i,b) in large[..original_len].iter_mut().enumerate(){
                    *b=(i%251) as u8;
                }
                need(fs.create_path("/large",&large[..original_len]).is_ok() &&
                     fs.create_path("/note",b"keep across boots").is_ok() &&
                     fs.create_path("/phase",b"phase1").is_ok(),
                     b"IndianOS: FATAL R027 first boot create failed\n\0");
                log(b"IndianOS: R027 disk phase1 formatted and multi-sector file committed\n\0");
            }else{
                let mut phase=[0u8;16];
                let phase_len=fs.read_path("/phase",&mut phase).unwrap_or_else(|_|{
                    log(b"IndianOS: FATAL R027 phase marker unreadable\n\0");halt()
                });
                let read_len=fs.read_path("/large",&mut large).unwrap_or_else(|_|{
                    log(b"IndianOS: FATAL R027 multi-sector file unreadable\n\0");halt()
                });
                let mut note=[0u8;32];
                need(fs.read_path("/note",&mut note)==Ok(17) &&
                     &note[..17]==b"keep across boots",
                     b"IndianOS: FATAL R027 unrelated file changed\n\0");
                let original=read_len==original_len &&
                    large[..read_len].iter().enumerate().all(|(i,&b)|b==(i%251) as u8);
                let updated=read_len==updated_len &&
                    large[..read_len].iter().enumerate().all(|(i,&b)|b==((i*7)%253) as u8);
                if phase_len==6 && &phase[..6]==b"phase1" && original{
                    // Boot 2: checksum-verified data survived a genuine reboot.
                    for (i,b) in large[..updated_len].iter_mut().enumerate(){
                        *b=((i*7)%253) as u8;
                    }
                    need(fs.overwrite_path("/large",&large[..updated_len]).is_ok() &&
                         fs.overwrite_path("/phase",b"phase2").is_ok(),
                         b"IndianOS: FATAL R027 boot2 overwrite failed\n\0");
                    log(b"IndianOS: R027 disk phase2 reboot persistence and multi-sector overwrite PASSED\n\0");
                }else if phase_len==6 && &phase[..6]==b"phase1" && updated {
                    // A deliberately torn newest metadata copy exposes the
                    // previous complete commit; never silently format.
                    need(fs.overwrite_path("/phase",b"phase2").is_ok(),
                         b"IndianOS: FATAL R027 metadata recovery commit failed\n\0");
                    log(b"IndianOS: R027 disk torn metadata fallback recovered in QEMU\n\0");
                }else if phase_len==6 && &phase[..6]==b"phase2" && updated{
                    log(b"IndianOS: R027 disk phase3 reboot-verified durable overwrite PASSED\n\0");
                }else{
                    log(b"IndianOS: FATAL R027 inconsistent metadata and payload phase\n\0");halt()
                }
            }
        }
        let mut after=[0u8;SECTOR_BYTES];
        need(disk.read_sector(FIRST-1,&mut after).is_ok() && after==left &&
             disk.read_sector(FIRST+COUNT,&mut after).is_ok() && after==right,
             b"IndianOS: FATAL R027 wrote outside isolated disk window\n\0");
        log(b"IndianOS: R027 isolated ATA block window boundaries PASSED\n\0");
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
    #[cfg(feature="test-r028-network")]
    {
        log(b"IndianOS: R028 legacy virtio-net PCI probe begin\n\0");
        let mut nic=unsafe{crate::virtio_net::VirtioNet::probe()}
            .unwrap_or_else(|_|{
                log(b"IndianOS: FATAL R028 PCI virtio-net probe or DMA setup\n\0");
                halt()
            });
        log(b"IndianOS: R028 legacy virtio-net DMA queues initialized\n\0");
        let result=crate::network::run_demo(&mut nic,log);
        nic.close();
        need(result.is_ok(),b"IndianOS: FATAL R028 ARP ICMP UDP DNS or TCP exchange\n\0");
        log(b"IndianOS: R028 QEMU virtio-net end-to-end network client PASSED\n\0");
    }

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
    #[cfg(all(feature="test-m2-disk-exec",target_arch="x86_64",target_os="none"))]
    {
        let stage=crate::m2_guest_qemu::run().unwrap_or_else(|_|{
            log(b"IndianOS: FATAL M2 disk native ring3 lifecycle\n\0");halt()
        });
        log(b"IndianOS: M2 Phase 4D QEMU simulated monotonic boot gate VERIFIED\n\0");
        log(b"IndianOS: M2 real Ed25519 boot policy and package signature VERIFIED\n\0");
        match stage{
          1=>log(b"IndianOS: M2 disk boot1 installed ELF v1 executed\n\0"),
          2=>log(b"IndianOS: M2 disk boot2 upgraded ELF v2 executed\n\0"),
          3=>log(b"IndianOS: M2 disk boot3 rolled-back ELF v1 executed\n\0"),
          4=>log(b"IndianOS: M2 disk boot4 removed app and rejected exec\n\0"),
          _=>{log(b"IndianOS: FATAL M2 unexpected boot phase\n\0");halt()}
        }
        log(b"IndianOS: M2 persisted ring3 package lifecycle PASSED\n\0");
        halt()
    }
    let (init,init_size)=module(b"/bin/init\0").unwrap_or_else(||{
        log(b"IndianOS: FATAL initramfs /bin/init is unavailable or invalid\n\0");
        halt()
    });
    log(b"IndianOS: read-only initramfs /bin/init validated\n\0");
    need(unsafe{user_image_boot_stage_test(init,init_size)},
         b"IndianOS: FATAL detached user-image staging failed\n\0");
    log(b"IndianOS: initramfs /bin/init staged and released safely\n\0");

    #[cfg(all(feature="test-shell",not(feature="test-r026"),not(feature="test-r029-userspace")))]
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
        #[cfg(feature="preempt")]
        log(b"IndianOS: R024 APIC preemptive scheduler experiment begin\n\0");
        #[cfg(feature="test-r030-ondevice")]
        need(crate::r030_ondevice::bootstrap(),
             b"IndianOS: FATAL R030.1 blank or existing bounded drive failed\n\0");
        #[cfg(feature="test-r030-ondevice")]
        log(b"IndianOS: R030.1 verified durable filesystem mounted in QEMU\n\0");
        #[cfg(feature="test-r026")]
        log(b"IndianOS: R026 persistent init shell and on-demand worker begin\n\0");
        #[cfg(all(not(feature="preempt"),feature="test-r025"))]
        log(b"IndianOS: R025 complete syscall services test begin\n\0");
        #[cfg(all(not(feature="preempt"),feature="test-ipc",not(feature="test-r025")))]
        log(b"IndianOS: R025 secure syscall IPC test begin\n\0");
        #[cfg(feature="test-r031-boot-desktop")]
        log(b"IndianOS: R031.1 boot service starting graphical session automatically\n\0");
        #[cfg(feature="test-r029-multiprocess")]
        log(b"IndianOS: R029.3 two isolated graphical processes begin\n\0");
        #[cfg(all(not(feature="preempt"),not(feature="test-ipc"),not(feature="test-r026"),not(feature="test-r029-multiprocess")))]
        log(b"IndianOS: M009 cooperative scheduler experiment begin\n\0");
        #[cfg(feature="test-r029-desktop-suite")]
        {
            let (terminal,terminal_size)=module(b"/bin/terminal\0").unwrap_or_else(||{
                log(b"IndianOS: FATAL /bin/terminal missing\n\0");halt()
            });
            need(unsafe{x86_64_r029_desktop_suite_run(init,init_size,worker,worker_size,terminal,terminal_size)},
                 b"IndianOS: FATAL R029.5-7 desktop integration failed\n\0");
        }
        #[cfg(not(feature="test-r029-desktop-suite"))]
        need(unsafe{x86_64_m009_schedule_run(init,init_size,worker,worker_size)},
             b"IndianOS: FATAL M009 cooperative context test failed\n\0");
        #[cfg(feature="test-r030-ondevice")]
        log(b"IndianOS: R030.1-2 persistent files and native app lifecycle PASSED\n\0");
        #[cfg(feature="test-r031-vfs")]
        log(b"IndianOS: R031.3 HIERARCHICAL VFS, ATOMIC RENAME AND REBOOT RECOVERY PASSED\n\0");
        #[cfg(feature="test-r026")]
        log(b"IndianOS: R026 persistent interactive shell session complete\n\0");
        #[cfg(feature="test-r029-multiprocess")]
        {
            need(crate::gui::user_surface_verified(),
                 b"IndianOS: FATAL R029.3 window ownership or teardown\n\0");
            #[cfg(feature="test-r031-supervisor")]
            log(b"IndianOS: R031.2 SERVICE RESTART AND CAPABILITY REVOCATION PASSED\n\0");
            #[cfg(all(feature="test-r031-boot-desktop",not(feature="test-r031-supervisor")))]
            log(b"IndianOS: R031.1 automatic boot-to-desktop services and shutdown PASSED\n\0");
            #[cfg(all(feature="test-r029-desktop-suite",not(feature="test-r031-boot-desktop")))]
            log(b"IndianOS: R029.5-7 integrated three-process GUI, IPC and launcher PASSED\n\0");
            #[cfg(not(feature="test-r029-desktop-suite"))]
            log(b"IndianOS: R029.3 independent PID1 Files and PID2 Terminal GUI PASSED\n\0");
        }
        #[cfg(all(not(feature="test-r026"),not(feature="test-r029-multiprocess")))]
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

    #[cfg(all(not(feature="test-r029-multiprocess"),
              any(feature="test-ring3",feature="test-fault-guard",
                  feature="test-fault-supervisor")))]
    {
        need(unsafe{x86_64_user_demo_run(init,init_size)},
             b"IndianOS: FATAL ring-3 user program experiment failed\n\0");
        #[cfg(any(feature="test-fault-guard",feature="test-fault-supervisor"))]
        log(b"IndianOS: user fault isolation experiment complete\n\0");
        #[cfg(not(any(feature="test-fault-guard",feature="test-fault-supervisor")))]
        log(b"IndianOS: ring-3 user program experiment complete\n\0");
        #[cfg(feature="test-r029-userspace")]
        {
            need(crate::gui::user_surface_verified(),
                b"IndianOS: FATAL R029 ring3 surface lifecycle incomplete\n\0");
            log(b"IndianOS: R029 ring3 command-owned two-window surfaces PASSED\n\0");
        }
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
