//! Rust-owned Limine revision-6 request table and response-pointer adapters.
//!
//! This replaces the LAST Limine-specific C request/shim translation unit
//! under RUST_BOOT_PROTOCOL=1. The public Limine ABI uses four 64-bit IDs,
//! revision u64, then a pointer, all written by Limine in RW request pages.
//!
//! SAFETY: response pointers belong to Limine and are valid only during the
//! initial single-core boot before any bootloader memory reclamation.
//! Do not call these getters from interrupts or other CPUs.
use core::{ffi::c_void,ptr};

const COMMON:[u64;2]=[0xc7b1dd30df4c8b88,0x0a82e883a194f07b];
#[repr(C)]
struct Request {
    id:[u64;4],
    revision:u64,
    response:*const c_void,
}
#[repr(C)]
struct ModuleRequest {
    id:[u64;4],
    revision:u64,
    response:*const c_void,
    internal_module_count:u64,
    internal_modules:*const c_void,
}
#[used]
#[link_section=".limine_requests_start"]
static mut START:[u64;4]=[
    0xf6b8f4b39de7d1ae,0xfab91a6940fcb9cf,
    0x785c6ed015d3e316,0x181e920a7852b9d9,
];
#[used]
#[link_section=".limine_requests"]
static mut BASE_REVISION:[u64;3]=[
    0xf9562b2d5c95a6c8,0x6a7b384944536bdc,6
];
#[used]
#[link_section=".limine_requests"]
static mut MEMORY_MAP:Request=Request{
    id:[COMMON[0],COMMON[1],0x67cf3d9d378a806f,0xe304acdfc50c3c62],
    revision:0,response:ptr::null(),
};
#[used]
#[link_section=".limine_requests"]
static mut HHDM:Request=Request{
    id:[COMMON[0],COMMON[1],0x48dcf1cb8ad2b852,0x63984e959a98244b],
    revision:0,response:ptr::null(),
};
#[used]
#[link_section=".limine_requests"]
static mut FRAMEBUFFER:Request=Request{
    id:[COMMON[0],COMMON[1],0x9d5827dcd881dd75,0xa3148604f6fab11b],
    revision:0,response:ptr::null(),
};
#[used]
#[link_section=".limine_requests"]
static mut MODULES:ModuleRequest=ModuleRequest{
    id:[COMMON[0],COMMON[1],0x3e7e279702be32af,0xca1c4f3bd1280cee],
    revision:0,response:ptr::null(),
    internal_module_count:0,internal_modules:ptr::null(),
};
// Limine MP protocol: APs remain waiting in the bootloader because the
// kernel never writes a goto_address. Reading CPU topology does NOT bring
// up an AP, install per-AP IDTs or enable remote TLB shootdowns.
#[repr(C)]
struct MpRequest {
    id:[u64;4],
    revision:u64,
    response:*const MpResponse,
    flags:u64,
}
#[repr(C)]
struct MpResponse {
    revision:u64,
    flags:u32,
    bsp_lapic_id:u32,
    cpu_count:u64,
    cpus:*const *const MpInfo,
}
#[repr(C)]
struct MpInfo {
    processor_id:u32,
    lapic_id:u32,
    reserved:u64,
    goto_address:*const c_void,
    extra_argument:u64,
}
#[used]
#[link_section=".limine_requests"]
static mut MP:MpRequest=MpRequest {
    id:[COMMON[0],COMMON[1],0x95a67b819a1b857e,0xa0b61b723b6a73e0],
    revision:0,response:ptr::null(),flags:0,
};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) struct MpInventory {
    pub cpus:u32,
    pub bsp_lapic_id:u32,
}
const MAX_BOOT_CPUS:usize=64;

/// R118-014/028 foundation: only validate Limine-supplied topology on
/// the bootstrap processor. Bootloader owns every listed pointer, and all
/// CPUs except the BSP MUST remain parked. No AP is started here.
///
/// SAFETY: the caller must be the BSP during initial Limine handoff, while
/// response pages and CPU pointer arrays are valid and immutable. A corrupt
/// bootloader pointer is not recoverable by a memory-safe parser.
unsafe fn validate_mp_response(response:&MpResponse)->Option<MpInventory>{
    let count=usize::try_from(response.cpu_count).ok()?;
    if count==0 || count>MAX_BOOT_CPUS || response.cpus.is_null() ||
       (response.cpus as usize)%core::mem::align_of::<*const MpInfo>()!=0 {
        return None;
    }
    let mut lapic_ids=[0u32;MAX_BOOT_CPUS];
    let mut processor_ids=[0u32;MAX_BOOT_CPUS];
    let mut bsp_found=0usize;
    for index in 0..count {
        // Trusted Limine pointer table only: never use this on untrusted
        // firmware bytes, from ring3, or after bootloader reclamation.
        let info_ptr=unsafe{ptr::read_volatile(response.cpus.add(index))};
        if info_ptr.is_null() ||
           (info_ptr as usize)%core::mem::align_of::<MpInfo>()!=0 {
            return None;
        }
        let info=unsafe{&*info_ptr};
        if lapic_ids[..index].contains(&info.lapic_id) ||
           processor_ids[..index].contains(&info.processor_id) {
            return None;
        }
        // A bootloader-armed AP would execute without an IndianOS-owned
        // GDT/IDT/TSS/stack. Reject such a handoff before treating it as
        // a safe, parked secondary processor.
        if info.lapic_id!=response.bsp_lapic_id &&
           !unsafe{ptr::read_volatile(ptr::addr_of!(info.goto_address))}.is_null() {
            return None;
        }
        bsp_found+=usize::from(info.lapic_id==response.bsp_lapic_id);
        lapic_ids[index]=info.lapic_id;
        processor_ids[index]=info.processor_id;
    }
    if bsp_found!=1{return None;}
    Some(MpInventory{cpus:count as u32,bsp_lapic_id:response.bsp_lapic_id})
}

/// Read-only topology discovery. No AP startup and no concurrency claim.
pub(crate) fn observed_mp_inventory()->Option<MpInventory>{
    let response=unsafe{ptr::read_volatile(ptr::addr_of!(MP.response))};
    if response.is_null() ||
       (response as usize)%core::mem::align_of::<MpResponse>()!=0 {
        return None;
    }
    unsafe{validate_mp_response(&*response)}
}

#[used]
#[link_section=".limine_requests_end"]
static mut END:[u64;2]=[0xadc0e0531bb10d03,0x9572709f31764c62];

#[no_mangle]
pub extern "C" fn boot_limine_revision_supported()->bool{
    unsafe{ptr::read_volatile(ptr::addr_of!(BASE_REVISION[2]))==0}
}
#[no_mangle]
pub extern "C" fn boot_limine_memmap()->*const c_void{
    unsafe{ptr::read_volatile(ptr::addr_of!(MEMORY_MAP.response))}
}
#[no_mangle]
pub extern "C" fn boot_limine_hhdm()->*const c_void{
    unsafe{ptr::read_volatile(ptr::addr_of!(HHDM.response))}
}
#[no_mangle]
pub extern "C" fn boot_limine_modules()->*const c_void{
    unsafe{ptr::read_volatile(ptr::addr_of!(MODULES.response))}
}
#[no_mangle]
pub extern "C" fn boot_limine_framebuffers()->*const c_void{
    unsafe{ptr::read_volatile(ptr::addr_of!(FRAMEBUFFER.response))}
}
#[cfg(test)]
mod tests{
    use super::*;
    use core::mem::{offset_of,size_of};
    #[test]
    fn request_binary_layout_matches_limine(){
        assert_eq!(size_of::<Request>(),48);
        assert_eq!(size_of::<ModuleRequest>(),64);
        assert_eq!(offset_of!(Request,response),40);
        assert_eq!(offset_of!(ModuleRequest,internal_module_count),48);
        assert_eq!(BASE_REVISION_INIT,6);
    }
    const BASE_REVISION_INIT:u64=6;
}

#[cfg(test)]
mod r125_mp_inventory_tests {
    use super::*;
    use core::mem::{offset_of,size_of};

    fn info(processor_id:u32,lapic_id:u32)->MpInfo{
        MpInfo{processor_id,lapic_id,reserved:0,
               goto_address:ptr::null(),extra_argument:0}
    }
    fn response(table:&[*const MpInfo],count:u64,bsp:u32)->MpResponse{
        MpResponse{revision:0,flags:0,bsp_lapic_id:bsp,
                   cpu_count:count,cpus:table.as_ptr()}
    }
    #[test]
    fn r125_mp_wire_layout_exact_limine_revision_six(){
        assert_eq!(size_of::<MpRequest>(),56);
        assert_eq!(size_of::<MpResponse>(),32);
        assert_eq!(size_of::<MpInfo>(),32);
        assert_eq!(offset_of!(MpRequest,response),40);
        assert_eq!(offset_of!(MpRequest,flags),48);
        assert_eq!(offset_of!(MpResponse,cpus),24);
        assert_eq!(offset_of!(MpInfo,goto_address),16);
    }
    #[test]
    fn r125_two_parked_cpus_have_one_authoritative_bsp(){
        let cpus=[info(0,0),info(1,1)];
        let table=[&cpus[0] as *const MpInfo,&cpus[1] as *const MpInfo];
        let data=response(&table,2,0);
        assert_eq!(unsafe{validate_mp_response(&data)},
                   Some(MpInventory{cpus:2,bsp_lapic_id:0}));
    }
    #[test]
    fn r125_duplicate_missing_bsp_and_invalid_count_fail_closed(){
        let cpus=[info(0,0),info(1,1)];
        let table=[&cpus[0] as *const MpInfo,&cpus[1] as *const MpInfo];
        for (count,bsp) in [(0,0),(65,0),(u64::MAX,0),(2,3)] {
            assert_eq!(unsafe{validate_mp_response(&response(&table,count,bsp))},None);
        }
        let duplicate=[&cpus[0] as *const MpInfo,&cpus[0] as *const MpInfo];
        assert_eq!(unsafe{validate_mp_response(&response(&duplicate,2,0))},None);
        let no_table=MpResponse{cpus:ptr::null(),..response(&table,2,0)};
        assert_eq!(unsafe{validate_mp_response(&no_table)},None);
        let same_id=[info(0,0),info(0,1)];
        let same_ptr=[&same_id[0] as *const MpInfo,&same_id[1] as *const MpInfo];
        assert_eq!(unsafe{validate_mp_response(&response(&same_ptr,2,0))},None);
    }
    #[test]
    fn r125_prestarted_ap_is_not_qualified_as_parked(){
        let bsp=info(0,0);
        let mut ap=info(1,1);
        ap.goto_address=1usize as *const c_void;
        let table=[&bsp as *const MpInfo,&ap as *const MpInfo];
        assert_eq!(unsafe{validate_mp_response(&response(&table,2,0))},None);
    }
}

/// R126 QEMU-only experimental AP execution. Limine supplies the temporary
/// AP stack/mappings. The BSP halts before bootloader memory reclamation, so
/// no kernel task, interrupt, filesystem, timer or DMA work runs concurrently.
/// Never call this from the normal production boot path.
#[cfg(feature="test-r118-ap-ack")]
static R126_AP_ACK:core::sync::atomic::AtomicU32=
    core::sync::atomic::AtomicU32::new(0);
#[cfg(feature="test-r118-ap-ack")]
static R126_AP_CPUID:core::sync::atomic::AtomicU32=
    core::sync::atomic::AtomicU32::new(u32::MAX);

#[cfg(feature="test-r118-ap-ack")]
static R127_GATE:crate::retirement_gate::RetirementGate=
    crate::retirement_gate::RetirementGate::new();
#[cfg(feature="test-r118-ap-ack")]
static R127_AP_DENIALS:core::sync::atomic::AtomicU32=
    core::sync::atomic::AtomicU32::new(0);
#[cfg(feature="test-r118-ap-ack")]
static R127_BSP_ADMISSIONS:core::sync::atomic::AtomicU32=
    core::sync::atomic::AtomicU32::new(0);
#[cfg(feature="test-r118-ap-ack")]
static R131_FENCE:crate::smp_lifecycle_fence::LifecycleFence=
    crate::smp_lifecycle_fence::LifecycleFence::new(0,1);
#[cfg(feature="test-r118-ap-ack")]
static R131_AP_DONE:core::sync::atomic::AtomicU32=
    core::sync::atomic::AtomicU32::new(0);
#[cfg(all(feature="test-r118-ap-ack",target_arch="x86_64"))]
#[no_mangle]
pub static R132_AP_ISR:core::sync::atomic::AtomicU32=
    core::sync::atomic::AtomicU32::new(0);
#[cfg(feature="test-r118-ap-ack")]
static R132_AP_READY:core::sync::atomic::AtomicU32=
    core::sync::atomic::AtomicU32::new(0);
#[cfg(feature="test-r118-ap-ack")]
static R132_AP_DONE:core::sync::atomic::AtomicU32=
    core::sync::atomic::AtomicU32::new(0);


// Isolated R132 x2APIC fixed-vector IPI. This entry is never reached from
// ordinary kernel boot: test-r118-ap-ack halts before PMM or user programs.
// The handler is completely assembly-owned: save its clobbered registers,
// atomically record actual interrupt execution, EOI to x2APIC, then IRETQ.
// A shared-memory mailbox marker alone CANNOT make this handler run.
#[cfg(all(feature="test-r118-ap-ack",target_arch="x86_64"))]
core::arch::global_asm!(
    ".global r132_ap_ipi_entry",
    "r132_ap_ipi_entry:",
    "push rax",
    "push rcx",
    "push rdx",
    "lock inc dword ptr [rip + R132_AP_ISR]",
    "mov ecx, 0x80b", // IA32_X2APIC_EOI
    "xor eax, eax",
    "xor edx, edx",
    "wrmsr",
    "pop rdx",
    "pop rcx",
    "pop rax",
    "iretq",
);
#[cfg(feature="test-r118-ap-ack")]
unsafe extern "C" {fn r132_ap_ipi_entry();}

#[cfg(feature="test-r118-ap-ack")]
#[repr(C,packed)]
struct R132Idtr { limit:u16,base:u64 }
#[cfg(feature="test-r118-ap-ack")]
#[repr(C)]
#[derive(Clone,Copy)]
struct R132Gate {
    offset_low:u16,selector:u16,ist:u8,attributes:u8,
    offset_mid:u16,offset_high:u32,reserved:u32,
}
#[cfg(feature="test-r118-ap-ack")]
impl R132Gate{
    const EMPTY:Self=Self{offset_low:0,selector:0,ist:0,attributes:0,
                           offset_mid:0,offset_high:0,reserved:0};
    fn new(handler:u64,selector:u16)->Self{
        Self{offset_low:handler as u16,selector,ist:0,attributes:0x8e,
             offset_mid:(handler>>16) as u16,
             offset_high:(handler>>32) as u32,reserved:0}
    }
}
#[cfg(feature="test-r118-ap-ack")]
#[repr(C,align(16))]
struct R132Idt([R132Gate;256]);
#[cfg(feature="test-r118-ap-ack")]
static mut R132_AP_IDT:R132Idt=R132Idt([R132Gate::EMPTY;256]);

#[cfg(feature="test-r118-ap-ack")]
fn r132_has_x2apic()->bool{
    (unsafe{core::arch::x86_64::__cpuid(1)}.ecx&(1<<21))!=0
}
#[cfg(feature="test-r118-ap-ack")]
fn r132_rdmsr(reg:u32)->u64{
    use core::arch::asm;
    let lo:u32;let hi:u32;
    unsafe{asm!("rdmsr",in("ecx")reg,out("eax")lo,out("edx")hi,
                options(nostack,preserves_flags));}
    ((hi as u64)<<32)|lo as u64
}
#[cfg(feature="test-r118-ap-ack")]
fn r132_wrmsr(reg:u32,value:u64){
    use core::arch::asm;
    unsafe{asm!("wrmsr",in("ecx")reg,
                in("eax")value as u32,in("edx")(value>>32) as u32,
                options(nostack,preserves_flags));}
}
#[cfg(feature="test-r118-ap-ack")]
fn r132_enable_x2apic()->bool{
    // IA32_APIC_BASE global-enable is mandatory. Only the isolated
    // QEMU opt-in test changes CPU-local APIC mode. Production does not.
    if !r132_has_x2apic(){return false;}
    let base=r132_rdmsr(0x1b);
    if base&(1<<11)==0{return false;}
    if base&(1<<10)==0{r132_wrmsr(0x1b,base|(1<<10));}
    if r132_rdmsr(0x1b)&(1<<10)==0{return false;}
    // Enable local APIC software mode with a legal spurious IRQ vector.
    let svr=r132_rdmsr(0x80f);
    r132_wrmsr(0x80f,(svr&!0xff)|0x1ff);
    true
}
#[cfg(feature="test-r118-ap-ack")]
fn r132_ap_listen_for_ipi()->bool{
    use core::{arch::asm,mem::size_of,sync::atomic::Ordering};
    if !r132_enable_x2apic(){return false;}
    let cs:u16;
    unsafe{asm!("mov {sel:x}, cs",sel=out(reg)cs,
                options(nomem,nostack,preserves_flags));}
    if cs==0{return false;}
    let entry=r132_ap_ipi_entry as usize as u64;
    unsafe{ptr::write_volatile(
        ptr::addr_of_mut!(R132_AP_IDT.0).cast::<R132Gate>().add(0xf1),
        R132Gate::new(entry,cs)
    );}
    let idtr=R132Idtr{
        limit:(256*size_of::<R132Gate>()-1) as u16,
        base:unsafe{ptr::addr_of!(R132_AP_IDT.0)} as usize as u64,
    };
    unsafe{asm!("lidt [{p}]",p=in(reg)&idtr,
                options(readonly,nostack,preserves_flags));}
    R132_AP_READY.store(1,Ordering::Release);
    for _ in 0..10_000_000u32{
        if R132_AP_ISR.load(Ordering::Acquire)==1{
            unsafe{asm!("cli",options(nomem,nostack));}
            R132_AP_DONE.store(1,Ordering::Release);
            return true;
        }
        // STI + HLT has no lost wake when BSP sends before STI: x86
        // interrupt shadow makes pending IPIs wake the subsequent HLT.
        unsafe{asm!("sti; hlt",options(nomem,nostack));}
    }
    false
}
#[cfg(feature="test-r118-ap-ack")]
fn r132_bsp_send_and_confirm()->bool{
    use core::sync::atomic::Ordering;
    for _ in 0..50_000_000u32{
        if R132_AP_READY.load(Ordering::Acquire)==1{break;}
        core::hint::spin_loop();
    }
    if R132_AP_READY.load(Ordering::Acquire)!=1 ||
       !r132_enable_x2apic(){return false;}
    // ICR[63:32] is destination x2APIC ID; low byte is fixed vector.
    // Only the validated QEMU APIC ID 1 is addressed, never broadcasts.
    r132_wrmsr(0x830,(1u64<<32)|0xf1);
    for _ in 0..50_000_000u32{
        if R132_AP_DONE.load(Ordering::Acquire)==1{
            return R132_AP_ISR.load(Ordering::Acquire)==1;
        }
        core::hint::spin_loop();
    }
    false
}

#[cfg(feature="test-r118-ap-ack")]
extern "C" fn r126_secondary_cpu_entry(info:*mut MpInfo)->!{
    use core::sync::atomic::Ordering;
    unsafe{core::arch::asm!("cli",options(nomem,nostack));}
    // This entry point is AP-only, authenticated by Limine. The AP reads
    // its own real CPUID, not just the BSP-provided protocol record.
    if !info.is_null(){
        let expected=unsafe{ptr::read_volatile(ptr::addr_of!((*info).lapic_id))};
        let actual=(unsafe{core::arch::x86_64::__cpuid(1)}.ebx>>24)&255;
        if expected==actual && actual!=0 {
            // R127: real AP repeatedly attempts kernel BSP admission while
            // the BSP concurrently probes the same atomic gate. Every
            // attempt MUST deny the foreign LAPIC ID without spinning.
            let mut denied=0u32;
            for _ in 0..8192 {
                if !matches!(
                    R127_GATE.try_enter(actual,true),
                    Err(crate::retirement_gate::GateError::WrongCpu)
                ){break;}
                denied+=1;
            }
            R127_AP_DENIALS.store(denied,Ordering::Release);
            if denied==8192 {
                R126_AP_CPUID.store(actual,Ordering::Release);
                R126_AP_ACK.store(1,Ordering::Release);
                // Exact APID1 cross-CPU lifecycle protocol: no interrupt
                // routing is claimed, and the AP never touches the live
                // BSP-owned process or filesystem service tables.
                let mut seen=0u64;
                for seq in 1..=64u64{
                    let mut got=false;
                    for _ in 0..50_000_000u32{
                        match R131_FENCE.observe_and_ack(actual,seen){
                            Ok(Some(receipt)) if receipt.sequence==seq &&
                                receipt.generation==seq*2 => {
                                seen=seq;got=true;break;
                            }
                            Ok(None)=>core::hint::spin_loop(),
                            _=>break,
                        }
                    }
                    if !got{break;}
                }
                R131_AP_DONE.store(seen as u32,Ordering::Release);
                if seen==64 {
                    let _=r132_ap_listen_for_ipi();
                }
            }
        }
    }
    // No IDT/TSS, scheduler, PMM, IPC or IPI is installed on this AP.
    // It must not outlive the isolated QEMU test profile.
    loop{unsafe{core::arch::asm!("cli; hlt",options(nostack,nomem));}}
}

#[cfg(feature="test-r118-ap-ack")]
pub(crate) fn launch_one_parked_qemu_ap()->bool{
    use core::sync::atomic::{fence,Ordering};
    let Some(topology)=observed_mp_inventory() else{return false;};
    if topology.cpus!=2 || topology.bsp_lapic_id!=0 {return false;}
    let response=unsafe{ptr::read_volatile(ptr::addr_of!(MP.response))};
    if response.is_null(){return false;}
    let raw=unsafe{&*response};
    let mut secondary=ptr::null_mut();
    for index in 0..2{
        let info=unsafe{ptr::read_volatile(raw.cpus.add(index))};
        if info.is_null(){return false;}
        let apic=unsafe{ptr::read_volatile(ptr::addr_of!((*info).lapic_id))};
        if apic!=topology.bsp_lapic_id {
            secondary=info as *mut MpInfo;
        }
    }
    if secondary.is_null(){return false;}
    if R127_GATE.register_bsp(0,true).is_err(){return false;}
    R126_AP_ACK.store(0,Ordering::Release);
    R126_AP_CPUID.store(u32::MAX,Ordering::Release);
    R127_AP_DENIALS.store(0,Ordering::Release);
    R127_BSP_ADMISSIONS.store(0,Ordering::Release);
    R131_AP_DONE.store(0,Ordering::Release);
    R132_AP_READY.store(0,Ordering::Release);
    R132_AP_DONE.store(0,Ordering::Release);
    R132_AP_ISR.store(0,Ordering::Release);
    fence(Ordering::Release);
    // The canonical Limine protocol wakes the AP only AFTER the BSP
    // publishes goto_address. No normal boot, userland or IRQ is enabled.
    unsafe{ptr::write_volatile(
        ptr::addr_of_mut!((*secondary).goto_address),
        r126_secondary_cpu_entry as *const c_void
    );}
    let mut arrived=false;
    for _ in 0..50_000_000u32{
        // The BSP alone owns the gate. This loop executes concurrently
        // with the real AP's 8192 denied entry attempts. The AP may never
        // acquire, mutate or retain any BSP process authority.
        let Ok(permit)=R127_GATE.try_enter(0,true)else{return false;};
        R127_BSP_ADMISSIONS.fetch_add(1,Ordering::Relaxed);
        drop(permit);
        if R126_AP_ACK.load(Ordering::Acquire)==1 {
            arrived=R126_AP_CPUID.load(Ordering::Acquire)==1 &&
                R127_AP_DENIALS.load(Ordering::Acquire)==8192 &&
                R127_BSP_ADMISSIONS.load(Ordering::Acquire)>0;
            break;
        }
        core::hint::spin_loop();
    }
    if !arrived{return false;}
    for seq in 1..=64u64{
        let receipt=match R131_FENCE.publish(0,seq*2){
            Ok(receipt)=>receipt,
            Err(_)=>return false,
        };
        let mut acknowledged=false;
        for _ in 0..50_000_000u32{
            match R131_FENCE.acknowledged(0,receipt){
                Ok(true)=>{acknowledged=true;break;},
                Ok(false)=>core::hint::spin_loop(),
                Err(_)=>return false,
            }
        }
        if !acknowledged{return false;}
    }
    for _ in 0..50_000_000u32{
        if R131_AP_DONE.load(Ordering::Acquire)==64{
            return r132_bsp_send_and_confirm();
        }
        core::hint::spin_loop();
    }
    false
}
