//! Safe, allocation-free metadata validation for IndianOS syscalls.
//!
//! This ports kernel/src/syscall_abi.c, retaining its C-visible ABI while
//! preventing a raw user pointer from being dereferenced in the validator.
//! Actual copy_from_user and privileged CPU transitions remain separate.

pub(crate) const USER_MIN: u64 = 0x0000_0000_0040_0000;
pub(crate) const USER_TOP: u64 = 0x0000_7fff_f000_0000;
pub(crate) const STACK_BYTES: u64 = 64 * 1024;
pub(crate) const PAGE_BYTES: u64 = 4096;
const MAX_WRITE: u64 = 256;
/// Must match cpu_entry's initial ring-3 RFLAGS profile. Dispatch, syscall
/// capture and resume validation share this policy, including IRQ keyboard.
pub(crate) const USER_INTERRUPTS_ENABLED:bool=
    cfg!(any(feature="preempt",feature="test-irq-keyboard"));
pub(crate) const MAX_SEGMENTS: usize = 8;

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct ProgramSegment {
    pub(crate) virtual_address: u64,
    pub(crate) file_offset: u64,
    pub(crate) file_size: u64,
    pub(crate) memory_size: u64,
    pub(crate) page_begin: u64,
    pub(crate) page_end: u64,
    pub(crate) writable: bool,
    pub(crate) executable: bool,
}
impl ProgramSegment {
    pub(crate) const fn empty() -> Self {
        Self {
            virtual_address: 0, file_offset: 0,
            file_size: 0, memory_size: 0, page_begin: 0, page_end: 0,
            writable: false, executable: false,
        }
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct ProgramPlan {
    pub(crate) entry: u64,
    pub(crate) stack_begin: u64,
    pub(crate) stack_end: u64,
    pub(crate) guard_begin: u64,
    pub(crate) segment_count: u32,
    pub(crate) segments: [ProgramSegment; MAX_SEGMENTS],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SyscallRequest {
    pub(crate) number: u64,
    pub(crate) arg0: u64,
    pub(crate) arg1: u64,
}

pub(crate) fn user_range(plan: &ProgramPlan, address: u64, length: u64, write: bool) -> bool {
    if length == 0 || address < USER_MIN || address >= USER_TOP ||
        length > USER_TOP - address ||
        plan.segment_count as usize > MAX_SEGMENTS ||
        plan.stack_begin < USER_MIN ||
        plan.stack_end > USER_TOP ||
        plan.stack_begin >= plan.stack_end {
        return false;
    }
    // A malformed overlapping segment cannot reauthorize the protected
    // user guard page, even if the segment itself is marked writable.
    let end=address+length;
    if plan.guard_begin>=USER_MIN &&
        plan.guard_begin.checked_add(PAGE_BYTES).is_some_and(|guard_end|
            address<guard_end&&plan.guard_begin<end){
        return false;
    }
    if address >= plan.stack_begin &&
        plan.stack_end.checked_sub(address).is_some_and(|available| length <= available) {
        return true;
    }
    for seg in plan.segments.iter().take((plan.segment_count as usize).min(MAX_SEGMENTS)) {
        if address >= seg.page_begin && address < seg.page_end &&
            seg.page_end.checked_sub(address).is_some_and(|available| length <= available) &&
            (!write || (seg.writable && !seg.executable)) {
            return true;
        }
    }
    false
}

/// # Safety
/// \`plan\` is null or a properly aligned, valid, readable kernel pointer.
/// The argument is *not* an arbitrary userspace pointer; this function only
/// validates its numeric range and never reads user bytes.
#[no_mangle]
pub unsafe extern "C" fn indianos_syscall_validate(
    plan: *const ProgramPlan, request: SyscallRequest,
) -> bool {
    let Some(plan) = (unsafe { plan.as_ref() }) else { return false; };
    match request.number {
        0 => request.arg0 == 0 && request.arg1 == 0, // ABI_QUERY
        1 => (1..=MAX_WRITE).contains(&request.arg1) &&
            user_range(plan, request.arg0, request.arg1, false), // WRITE
        7 => request.arg1 == 0 &&
            user_range(plan, request.arg0, 32, false), // QUANTUM descriptor
        2 => request.arg1 == 0, // EXIT
        3 => request.arg0 == 0 && request.arg1 == 0, // YIELD
        #[cfg(any(feature="test-shell",feature="test-ipc",feature="normal-init",feature="normal-desktop"))]
        4 => request.arg0 == 0 && request.arg1 == 0, // GETPID
        #[cfg(feature = "test-shell")]
        5 => request.arg0 == 1 && request.arg1 == 0, // allowlisted /bin/worker
        #[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
        18 => request.arg1==64 && user_range(plan,request.arg0,64,false),
        #[cfg(all(feature="test-r029-userspace",not(feature="normal-desktop")))]
        19 => (1..=2).contains(&request.arg0) && request.arg1==0,
        #[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
        20 => request.arg0==0 && request.arg1==0,
        #[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
        21 => request.arg1==16 && user_range(plan,request.arg0,16,true),
        #[cfg(feature="test-r031-supervisor")]
        26 => request.arg0==0 && request.arg1==0, // supervisor state PID1 only
        #[cfg(any(feature="test-r031-boot-desktop",feature="normal-desktop"))]
        25 => request.arg0==0 && request.arg1==0, // PID1 physical mouse pump
        #[cfg(all(feature="test-r029-desktop-suite",not(feature="normal-desktop")))]
        24 => (1..=6).contains(&request.arg0) && request.arg1==0,
        #[cfg(all(feature="test-r029-frame-server",not(feature="normal-desktop")))]
        22 => request.arg0==0 && request.arg1==0, // FRAME_QUERY
        #[cfg(all(feature="test-r029-frame-server",not(feature="normal-desktop")))]
        23 => (1..=2).contains(&request.arg0) && request.arg1==0, // FRAME_PRESENT
        #[cfg(feature="normal-desktop")]
        38 => request.arg0<=1 && request.arg1==0, // PID1 lifecycle; authority checked at trap
        #[cfg(feature="normal-desktop")]
        37 => request.arg0<=1 &&
              user_range(plan,request.arg1,64,request.arg0==0), // compositor state query/apply
        #[cfg(feature="test-r030-ondevice")]
        30 => (1..=4).contains(&request.arg0) &&
              user_range(plan,request.arg1,48,false), // filesystem descriptor
        #[cfg(feature="test-r030-ondevice")]
        31 => request.arg0<=4 && request.arg1==0,
        #[cfg(any(feature="test-r031-vfs",feature="normal-desktop"))]
        32 => (1..=if cfg!(feature="normal-desktop"){12}else{11}).contains(&request.arg0) &&
              user_range(plan,request.arg1,64,false), // installed native app
        #[cfg(any(feature="test-r031-vfs",feature="normal-desktop"))]
        33 => (matches!(request.arg0,1..=6|34..=38)) &&
              user_range(plan,request.arg1,64,false), // versioned handles/range locks
        #[cfg(feature="test-r031-files-gui")]
        36 => (1..=4).contains(&request.arg0) &&
              user_range(plan,request.arg1,64,false), // ring3 local UDP descriptor
        #[cfg(any(feature="test-r031-files-gui",feature="normal-desktop"))]
        34 => request.arg0==1 &&
              user_range(plan,request.arg1,64,false), // PID2 shared dir listing
        #[cfg(feature = "test-r026")]
        16 | 17 => request.arg0 == 2 && request.arg1 == 0, // worker status/reap query
        #[cfg(any(feature="test-interactive",feature="normal-init",feature="normal-desktop"))]
        6 => request.arg1 == 1 && user_range(plan,request.arg0,1,true),
        #[cfg(all(any(feature="test-ipc",feature="normal-desktop"),not(feature="test-r025")))]
        8 => (4..=68).contains(&request.arg1) &&
            user_range(plan,request.arg0,request.arg1,false), // IPC_SEND (R025.1)
        #[cfg(feature="test-r025")]
        8 => (12..=76).contains(&request.arg1) &&
            user_range(plan,request.arg0,request.arg1,false), // Cap-authorized SEND
        #[cfg(any(feature="test-ipc",feature="normal-desktop"))]
        9 => request.arg1 == 72 &&
            user_range(plan,request.arg0,72,true), // IPC_RECEIVE
        #[cfg(any(feature="test-ipc",feature="normal-desktop"))]
        35 => (1..=crate::ipc::MAX_WAIT_ROUNDS as u64).contains(&request.arg1) &&
            user_range(plan,request.arg0,crate::ipc::RECEIVE_BYTES as u64,true),
        #[cfg(feature="test-r025")]
        10 => request.arg0>0 && request.arg1==0, // request own capability
        #[cfg(feature="test-r025")]
        11..=13 => request.arg1==48 &&
            user_range(plan,request.arg0,48,false), // FILE create/read/delete
        #[cfg(feature="test-r025")]
        14 => request.arg0>0 && request.arg1==0, // CAP_REVOKE
        #[cfg(feature="test-r025")]
        15 => (1..=2).contains(&request.arg0) && request.arg1==0, // PROCESS_STATUS
        _ => false,
    }
}

/// Validate the CPU-generated ring-3 IRET frame before honoring syscalls or
/// saving user context. 15 saved GPRs are followed by RIP, CS, RFLAGS, RSP, SS.
/// This is a numeric policy check: it never dereferences user addresses.
pub(crate) fn ring3_frame_valid(
    frame: &[u64; 20], plan: &ProgramPlan, irq_enabled: bool,
) -> bool {
    const FIXED: u64 = 1 << 1;
    const IF: u64 = 1 << 9;
    const IOPL: u64 = 3 << 12;
    const NT: u64 = 1 << 14;
    const VM: u64 = 1 << 17;
    // x86-64 IRETQ must not import reserved high RFLAGS from a user
    // snapshot. Bit 1 is fixed; bits 3, 5, 15 and 22..63 are reserved.
    const RESERVED:u64=(1<<3)|(1<<5)|(1<<15)|(!0u64<<22);
    let (rip, cs, flags, rsp, ss) =
        (frame[15], frame[16], frame[17], frame[18], frame[19]);
    cs == 0x2b && ss == 0x33 &&
        flags & FIXED != 0 &&
        flags & (IOPL | NT | VM | RESERVED) == 0 &&
        (flags & IF != 0) == irq_enabled &&
        (USER_MIN..USER_TOP).contains(&rip) &&
        plan.stack_begin >= USER_MIN && plan.stack_end <= USER_TOP &&
        plan.stack_begin < plan.stack_end &&
        rsp >= plan.stack_begin && rsp <= plan.stack_end
}

#[no_mangle]
pub extern "C" fn indianos_syscall_self_test() -> bool {
    let plan = ProgramPlan {
        entry: 0,
        stack_begin: USER_TOP - STACK_BYTES,
        stack_end: USER_TOP,
        guard_begin: USER_TOP - STACK_BYTES - PAGE_BYTES,
        segment_count: 0,
        segments: [ProgramSegment::empty(); MAX_SEGMENTS],
    };
    let check = |number, arg0, arg1| unsafe {
        indianos_syscall_validate(&plan, SyscallRequest { number, arg0, arg1 })
    };
    check(0, 0, 0) &&
        check(1, plan.stack_begin, 8) &&
        !check(1, plan.guard_begin, 1) &&
        check(2, 0, 0) &&
        check(3, 0, 0) &&
        !check(3, 1, 0) &&
        !check(999, 0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, offset_of, size_of};

    #[cfg(feature="normal-desktop")]
    #[test]
    fn desktop_profile_exposes_bounded_compositor_state_without_legacy_control(){
        let mut plan=ProgramPlan{entry:USER_MIN,stack_begin:USER_TOP-STACK_BYTES,
            stack_end:USER_TOP,guard_begin:USER_TOP-STACK_BYTES-PAGE_BYTES,
            segment_count:1,segments:[ProgramSegment::empty();MAX_SEGMENTS]};
        plan.segments[0]=ProgramSegment{page_begin:USER_MIN,
            page_end:USER_MIN+PAGE_BYTES,writable:false,executable:true,
            ..ProgramSegment::empty()};
        let check=|number,arg0,arg1|unsafe{indianos_syscall_validate(&plan,
            SyscallRequest{number,arg0,arg1})};
        assert!(check(37,0,plan.stack_end-64));
        assert!(check(37,1,USER_MIN));
        assert!(!check(37,0,USER_MIN));
        assert!(!check(37,2,plan.stack_begin));
        assert!(!check(37,0,plan.stack_end-63));
        assert!(!check(37,1,plan.guard_begin));
        assert!(check(38,0,0));
        assert!(check(38,1,0));
        assert!(!check(38,2,0));
        assert!(!check(38,0,1));
        assert!(!check(38,1,u64::MAX));
        for operation in [34,35,36,37]{
            assert!(check(33,operation,plan.stack_begin));
            assert!(!check(33,operation,plan.stack_end-63));
        }
        assert!(!check(33,33,plan.stack_begin));
        assert!(check(33,38,plan.stack_begin));
        assert!(!check(33,39,plan.stack_begin));
        // Hardware input preflights every destination before the runtime can
        // consume a physical byte; guard/kernel/overflow addresses cannot
        // drain the trusted broker's input stream.
        assert!(check(6,plan.stack_end-1,1));
        for address in [0,plan.guard_begin,USER_TOP,u64::MAX]{
            assert!(!check(6,address,1));
            assert!(!check(21,address,16));
        }
        assert!(!check(6,plan.stack_begin,0));
        assert!(!check(6,plan.stack_begin,2));
        assert!(!check(21,plan.stack_end-15,16));
        assert!(!check(25,1,0));
        assert!(!check(25,0,1));
        for (number,arg0,arg1) in [(5,1,0),(19,1,0),(22,0,0),(23,1,0),
            (24,1,0),(30,1,plan.stack_begin),(31,1,0),(36,1,plan.stack_begin)]{
            assert!(!check(number,arg0,arg1),"legacy control {number}");
        }
        assert!(check(32,12,plan.stack_begin));
        assert!(!check(32,13,plan.stack_begin));
    }

    #[cfg(any(feature="test-r031-vfs",feature="normal-desktop"))]
    #[test]
    fn native_truncate_ingress_accepts_only_known_actions_and_complete_descriptors(){
        let plan=ProgramPlan{entry:USER_MIN,stack_begin:USER_TOP-STACK_BYTES,
            stack_end:USER_TOP,guard_begin:USER_TOP-STACK_BYTES-PAGE_BYTES,
            segment_count:0,segments:[ProgramSegment::empty();MAX_SEGMENTS]};
        let check=|action,address|unsafe{indianos_syscall_validate(&plan,
            SyscallRequest{number:33,arg0:action,arg1:address})};
        for action in 1..=6{assert!(check(action,plan.stack_begin));}
        for action in [0,7,u64::MAX]{assert!(!check(action,plan.stack_begin));}
        for address in [plan.guard_begin,plan.stack_end-63,USER_TOP,u64::MAX]{
            assert!(!check(6,address));
        }
    }

    #[test]
    fn ffi_layout_is_identical_to_c() {
        assert_eq!(size_of::<ProgramSegment>(), 56);
        assert_eq!(size_of::<ProgramPlan>(), 488);
        assert_eq!(offset_of!(ProgramPlan, segments), 40);
        assert_eq!(align_of::<ProgramPlan>(), 8);
        assert_eq!(size_of::<SyscallRequest>(), 24);
    }

    #[test]
    fn validates_exact_original_syscall_contract() {
        assert!(indianos_syscall_self_test());
        let mut plan = ProgramPlan {
            entry: USER_MIN, stack_begin: USER_TOP - STACK_BYTES,
            stack_end: USER_TOP, guard_begin: USER_TOP - STACK_BYTES - PAGE_BYTES,
            segment_count: 1, segments: [ProgramSegment::empty(); MAX_SEGMENTS],
        };
        plan.segments[0] = ProgramSegment {
            page_begin: USER_MIN, page_end: USER_MIN + PAGE_BYTES,
            ..ProgramSegment::empty()
        };
        let check = |request: SyscallRequest| unsafe { indianos_syscall_validate(&plan, request) };
        assert!(check(SyscallRequest { number: 1, arg0: USER_MIN, arg1: 32 }));
        assert!(!check(SyscallRequest { number: 1, arg0: USER_MIN, arg1: 257 }));
        assert!(!check(SyscallRequest { number: 1, arg0: USER_MIN - 1, arg1: 1 }));
        assert!(!check(SyscallRequest { number: 1, arg0: USER_TOP, arg1: 1 }));
        assert!(!check(SyscallRequest { number: 1, arg0: u64::MAX - 1, arg1: 64 }));
        assert!(!check(SyscallRequest { number: 1, arg0: USER_MIN + PAGE_BYTES - 1, arg1: 2 }));
        assert!(!check(SyscallRequest { number: 3, arg0: 1, arg1: 0 }));
    }

    #[cfg(any(feature="test-r029-userspace",feature="normal-desktop"))]
    #[test]
    fn ring3_graphics_syscall_rejects_bad_identity_buffer_and_lengths(){
        let plan=ProgramPlan{
            entry:USER_MIN,stack_begin:USER_TOP-STACK_BYTES,
            stack_end:USER_TOP,guard_begin:USER_TOP-STACK_BYTES-PAGE_BYTES,
            segment_count:0,segments:[ProgramSegment::empty();MAX_SEGMENTS]
        };
        let check=|number,arg0,arg1|unsafe{
            indianos_syscall_validate(&plan,SyscallRequest{number,arg0,arg1})
        };
        assert!(check(18,plan.stack_begin,64));
        assert!(!check(18,plan.stack_begin,63));
        assert!(!check(18,plan.stack_begin,65));
        assert!(!check(18,plan.guard_begin,64));
        assert!(!check(18,USER_TOP-63,64));
        assert!(!check(18,u64::MAX-10,64));
        assert_eq!(check(19,1,0),!cfg!(feature="normal-desktop"));
        assert_eq!(check(19,2,0),!cfg!(feature="normal-desktop"));
        assert!(!check(19,3,0));
        assert!(!check(19,1,1));
        assert!(check(20,0,0));
        assert!(!check(20,1,0));
        assert!(!check(20,0,1));
    }
    #[cfg(any(feature = "test-shell",feature="normal-desktop"))]
    #[test]
    fn shell_abi_rejects_invalid_program_ids_and_arguments() {
        let plan = ProgramPlan {
            entry: USER_MIN, stack_begin: USER_TOP-STACK_BYTES,
            stack_end: USER_TOP, guard_begin: USER_TOP-STACK_BYTES-PAGE_BYTES,
            segment_count: 0, segments: [ProgramSegment::empty();MAX_SEGMENTS],
        };
        let valid=|number,arg0,arg1| unsafe {
            indianos_syscall_validate(&plan,SyscallRequest{number,arg0,arg1})
        };
        assert!(valid(4,0,0));
        assert!(!valid(4,1,0));
        assert_eq!(valid(5,1,0),!cfg!(feature="normal-desktop"));
        assert!(!valid(5,2,0));
        assert!(!valid(5,1,1));
    }
    #[cfg(any(feature="test-interactive",feature="normal-desktop"))]
    #[test]
    fn read_syscall_rejects_nonwritable_ranges(){
        let mut plan=ProgramPlan{
            entry:USER_MIN,stack_begin:USER_TOP-STACK_BYTES,
            stack_end:USER_TOP,guard_begin:USER_TOP-STACK_BYTES-PAGE_BYTES,
            segment_count:1,segments:[ProgramSegment::empty();MAX_SEGMENTS]
        };
        plan.segments[0]=ProgramSegment{
            page_begin:USER_MIN,page_end:USER_MIN+PAGE_BYTES,
            writable:false,executable:true,..ProgramSegment::empty()
        };
        let check=|addr,len|unsafe{indianos_syscall_validate(&plan,
            SyscallRequest{number:6,arg0:addr,arg1:len})};
        assert!(check(plan.stack_end-1,1));
        assert!(!check(plan.stack_end,1));
        assert!(!check(plan.guard_begin,1));
        assert!(!check(USER_MIN,1));
        assert!(!check(plan.stack_end-1,2));
    }
    #[cfg(any(feature="test-ipc",feature="normal-desktop"))]
    #[test]
    fn ipc_abi_checks_exact_lengths_memory_and_readonly_pages() {
        let mut plan=ProgramPlan{
            entry:USER_MIN, stack_begin:USER_TOP-STACK_BYTES,
            stack_end:USER_TOP, guard_begin:USER_TOP-STACK_BYTES-PAGE_BYTES,
            segment_count:1, segments:[ProgramSegment::empty();MAX_SEGMENTS],
        };
        plan.segments[0]=ProgramSegment{
            page_begin:USER_MIN,page_end:USER_MIN+PAGE_BYTES,
            writable:false,executable:true,..ProgramSegment::empty()
        };
        let check=|number,address,length|unsafe{
            indianos_syscall_validate(&plan,SyscallRequest{
                number,arg0:address,arg1:length,
            })
        };
        assert!(check(4,0,0));
        assert!(!check(4,1,0));
        #[cfg(not(feature="test-r025"))]
        {
            assert!(check(8,plan.stack_begin,4));
            assert!(check(8,USER_MIN,68));
            assert!(!check(8,plan.stack_begin,3));
            assert!(!check(8,plan.stack_begin,69));
            assert!(!check(8,plan.guard_begin,4));
        }
        #[cfg(feature="test-r025")]
        {
            assert!(!check(8,plan.stack_begin,4));
            assert!(check(8,USER_MIN,76));
            assert!(!check(8,plan.stack_begin,11));
            assert!(!check(8,plan.stack_begin,77));
            assert!(!check(8,plan.guard_begin,12));
        }
        assert!(!check(8,u64::MAX-4,8));
        assert!(check(9,plan.stack_begin,72));
        assert!(!check(9,USER_MIN,72));
        assert!(!check(9,plan.stack_end-71,72));
        assert!(!check(9,plan.stack_begin,71));
        assert!(!check(9,plan.guard_begin,72));
    }
    #[cfg(feature="test-r025")]
    #[test]
    fn r025_syscalls_fail_closed_for_missing_invalid_and_oversized_buffers() {
        let mut plan=ProgramPlan{
            entry:USER_MIN, stack_begin:USER_TOP-STACK_BYTES,
            stack_end:USER_TOP, guard_begin:USER_TOP-STACK_BYTES-PAGE_BYTES,
            segment_count:1,segments:[ProgramSegment::empty();MAX_SEGMENTS],
        };
        plan.segments[0]=ProgramSegment {
            page_begin:USER_MIN,page_end:USER_MIN+PAGE_BYTES,
            writable:false,executable:true,..ProgramSegment::empty()
        };
        let check=|num,addr,length|unsafe{
            indianos_syscall_validate(&plan,SyscallRequest {
                number:num,arg0:addr,arg1:length,
            })
        };
        assert!(check(8,plan.stack_begin,12));
        assert!(check(8,plan.stack_begin,76));
        assert!(!check(8,plan.stack_begin,11));
        assert!(!check(8,plan.stack_begin,77));
        assert!(check(10,2,0));
        assert!(!check(10,0,0));
        for operation in 11..=13 {
            assert!(check(operation,plan.stack_begin,48));
            assert!(check(operation,USER_MIN,48));
            assert!(!check(operation,plan.guard_begin,48));
            assert!(!check(operation,USER_TOP-47,48));
            assert!(!check(operation,plan.stack_begin,47));
            assert!(!check(operation,plan.stack_begin,49));
        }
        assert!(check(14,1,0));
        assert!(!check(14,0,0));
        assert!(!check(14,1,1));
        assert!(check(15,1,0));
        assert!(check(15,2,0));
        assert!(!check(15,3,0));
    }
    #[cfg(feature="test-r026")]
    #[test]
    fn r026_worker_queries_reject_arbitrary_pid_and_arguments() {
        let plan=ProgramPlan{
            entry:USER_MIN,stack_begin:USER_TOP-STACK_BYTES,
            stack_end:USER_TOP,guard_begin:USER_TOP-STACK_BYTES-PAGE_BYTES,
            segment_count:0,segments:[ProgramSegment::empty();MAX_SEGMENTS],
        };
        let valid=|number,arg0,arg1|unsafe{
            indianos_syscall_validate(&plan,SyscallRequest{number,arg0,arg1})
        };
        assert!(valid(5,1,0));
        assert!(!valid(5,2,0));
        for syscall in [16,17]{
            assert!(valid(syscall,2,0));
            for pid in [0,1,3,u64::MAX]{
                assert!(!valid(syscall,pid,0));
            }
            assert!(!valid(syscall,2,1));
        }
    }
    #[cfg(any(feature="test-ipc",feature="normal-desktop"))]
    #[test]
    fn ipc_wait_validates_full_destination_and_bounded_cooperative_budget(){
        let plan=ProgramPlan{
            entry:USER_MIN,stack_begin:USER_TOP-STACK_BYTES,stack_end:USER_TOP,
            guard_begin:USER_TOP-STACK_BYTES-PAGE_BYTES,segment_count:0,
            segments:[ProgramSegment::empty();MAX_SEGMENTS],
        };
        let valid=|address,budget|unsafe{indianos_syscall_validate(&plan,
            SyscallRequest{number:35,arg0:address,arg1:budget})};
        for budget in 1..=32{assert!(valid(plan.stack_begin,budget));}
        for budget in [0,33,u64::MAX]{assert!(!valid(plan.stack_begin,budget));}
        assert!(valid(plan.stack_end-72,1));
        for address in [0,plan.guard_begin,plan.stack_end-71,plan.stack_end,u64::MAX]{
            assert!(!valid(address,1));
        }
        assert!(unsafe{indianos_syscall_validate(&plan,SyscallRequest{
            number:9,arg0:plan.stack_begin,arg1:72})});
        assert!(!unsafe{indianos_syscall_validate(&plan,SyscallRequest{
            number:9,arg0:plan.stack_begin,arg1:1})});
    }

    #[test]
    fn trap_frame_blocks_supervisor_selectors_and_guard_stack() {
        let plan = ProgramPlan {
            entry: USER_MIN, stack_begin: USER_TOP - STACK_BYTES,
            stack_end: USER_TOP, guard_begin: USER_TOP - STACK_BYTES - PAGE_BYTES,
            segment_count: 0, segments: [ProgramSegment::empty(); MAX_SEGMENTS],
        };
        let mut regs = [0u64; 20];
        regs[15] = USER_MIN;
        regs[16] = 0x2b;
        regs[17] = 0x02;
        regs[18] = plan.stack_end - 16;
        regs[19] = 0x33;
        assert!(ring3_frame_valid(&regs, &plan, false));
        regs[16] = 0x08;
        assert!(!ring3_frame_valid(&regs, &plan, false));
        regs[16] = 0x2b;
        regs[19] = 0x10;
        assert!(!ring3_frame_valid(&regs, &plan, false));
        regs[19] = 0x33;
        regs[18] = plan.guard_begin;
        assert!(!ring3_frame_valid(&regs, &plan, false));
        regs[18] = plan.stack_end - 16;
        regs[17] |= 3<<12;
        assert!(!ring3_frame_valid(&regs, &plan, false));
        regs[17] = 0x202;
        assert!(!ring3_frame_valid(&regs, &plan, false));
        assert!(ring3_frame_valid(&regs, &plan, true));
        regs[15] = 0xffff_ffff_8000_0000;
        assert!(!ring3_frame_valid(&regs, &plan, true));
        regs[15] = USER_MIN;
        regs[17] &= !2;
        assert!(!ring3_frame_valid(&regs, &plan, true));
    }
}

#[cfg(test)]
mod m1_gate20_w5_usercopy_tests{
 use super::*;
 #[test]fn malicious_segment_overlap_cannot_expose_guard_or_overrun_plan(){
  let mut p=ProgramPlan{entry:USER_MIN,stack_begin:USER_TOP-STACK_BYTES,
   stack_end:USER_TOP,guard_begin:USER_MIN+PAGE_BYTES,
   segment_count:1,segments:[ProgramSegment::empty();MAX_SEGMENTS]};
  p.segments[0]=ProgramSegment{page_begin:USER_MIN,page_end:USER_MIN+3*PAGE_BYTES,
   writable:true,executable:false,..ProgramSegment::empty()};
  assert!(user_range(&p,USER_MIN+32,16,true));
  assert!(!user_range(&p,p.guard_begin+32,16,true));
  assert!(!user_range(&p,p.guard_begin-4,16,true));
  p.segment_count=(MAX_SEGMENTS+1) as u32;
  assert!(!user_range(&p,USER_MIN+32,16,false));
 }
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) struct CopySpan{pub address:u64,pub length:u64,pub writable:bool}
/// Preflight every scatter/gather user transfer without reading any user
/// pointer; writable alias, overflow, guard and partial admission fail closed.
pub(crate) fn user_copy_batch_valid(plan:&ProgramPlan,spans:&[CopySpan])->bool{
 if spans.is_empty()||spans.len()>MAX_SEGMENTS{return false;}
 for (i,&span) in spans.iter().enumerate(){
  if !user_range(plan,span.address,span.length,span.writable){return false;}
  let Some(end)=span.address.checked_add(span.length)else{return false};
  for prior in &spans[..i]{
   let Some(other_end)=prior.address.checked_add(prior.length)
    else{return false};
   if (span.writable||prior.writable)&&span.address<other_end&&
     prior.address<end{return false;}
  }
 }
 true
}
#[cfg(test)]mod m1_batch03_w3_memory_tests{
 use super::*;
 fn plan()->ProgramPlan{
  let mut s=[ProgramSegment::empty();MAX_SEGMENTS];
  s[0]=ProgramSegment{page_begin:USER_MIN,page_end:USER_MIN+2*PAGE_BYTES,
    writable:true,..ProgramSegment::empty()};
  ProgramPlan{entry:USER_MIN,stack_begin:USER_TOP-STACK_BYTES,
   stack_end:USER_TOP,guard_begin:USER_TOP-STACK_BYTES-PAGE_BYTES,
   segment_count:1,segments:s}
 }
 #[test]fn good_scatter_copy_bad_writable_alias_and_guard(){
  let p=plan();let read=CopySpan{address:USER_MIN,length:16,writable:false};
  assert!(user_copy_batch_valid(&p,&[read,CopySpan{
    address:USER_MIN+32,length:16,writable:true}]));
  assert!(!user_copy_batch_valid(&p,&[read,CopySpan{
    address:USER_MIN+8,length:16,writable:true}]));
  assert!(!user_copy_batch_valid(&p,&[read,CopySpan{
    address:p.guard_begin,length:16,writable:true}]));
 }
 #[test]fn overflow_zero_and_oversized_batch_denied(){
  let p=plan();let bad=CopySpan{address:u64::MAX-1,length:4,writable:true};
  assert!(!user_copy_batch_valid(&p,&[bad]));
  assert!(!user_copy_batch_valid(&p,&[]));
  assert!(!user_copy_batch_valid(&p,&[CopySpan{
    address:USER_MIN,length:1,writable:false};MAX_SEGMENTS+1]));
 }
}


/// P0 follow-up W2: opt-in strict ring-3 executable-page admission.
/// The live trap path still uses ring3_frame_valid and MUST be upgraded
/// before this strict policy can count as a production NX/W^X guarantee.
/// No hardware page tables are changed by this numeric check.
pub(crate) fn ring3_frame_strict_execution(
 frame:&[u64;20],plan:&ProgramPlan,irq_enabled:bool)->bool{
 if !ring3_frame_valid(frame,plan,irq_enabled)||
    plan.segment_count==0||plan.segment_count as usize>MAX_SEGMENTS||
    plan.guard_begin.checked_add(PAGE_BYTES)!=Some(plan.stack_begin)||
    frame[18]>=plan.stack_end{
  return false;
 }
 let rip=frame[15];
 let mut entry_executable=false;let mut rip_executable=false;
 let count=plan.segment_count as usize;
 for (i,segment) in plan.segments[..count].iter().enumerate(){
  if segment.page_begin<USER_MIN||segment.page_end>USER_TOP||
     segment.page_begin>=segment.page_end||
     segment.page_begin%PAGE_BYTES!=0||segment.page_end%PAGE_BYTES!=0||
     segment.virtual_address<segment.page_begin||
     segment.memory_size==0||segment.file_size>segment.memory_size||
     segment.virtual_address.checked_add(segment.memory_size)
        .is_none_or(|end|end>segment.page_end)||
     (segment.executable&&segment.writable)||
     (segment.page_begin<plan.stack_end&&plan.stack_begin<segment.page_end)||
     (segment.page_begin<plan.guard_begin+PAGE_BYTES&&
      plan.guard_begin<segment.page_end){
   return false;
  }
  for other in &plan.segments[..i]{
   if segment.page_begin<other.page_end&&other.page_begin<segment.page_end{
    return false; // shared pages cannot silently override NX/guard rights
   }
  }
  if segment.executable{
   if let Some(end)=segment.virtual_address.checked_add(segment.memory_size){
    if (segment.virtual_address..end).contains(&rip){rip_executable=true;}
    if (segment.virtual_address..end).contains(&plan.entry){
     entry_executable=true;
    }
   }
  }
 }
 entry_executable&&rip_executable
}
#[cfg(test)]
mod kernel_p0_next_w2_userexec_tests{
 use super::*;
 fn base()->ProgramPlan{
  let mut p=ProgramPlan{entry:USER_MIN,
   stack_begin:USER_TOP-STACK_BYTES,stack_end:USER_TOP,
   guard_begin:USER_TOP-STACK_BYTES-PAGE_BYTES,
   segment_count:1,segments:[ProgramSegment::empty();MAX_SEGMENTS]};
  p.segments[0]=ProgramSegment{
   virtual_address:USER_MIN,file_offset:0,file_size:16,
   memory_size:PAGE_BYTES,page_begin:USER_MIN,page_end:USER_MIN+PAGE_BYTES,
   writable:false,executable:true};
  p
 }
 fn frame(p:&ProgramPlan)->[u64;20]{
  let mut f=[0u64;20];
  f[15]=USER_MIN;f[16]=0x2b;f[17]=0x2;f[18]=p.stack_end-8;f[19]=0x33;f
 }
 #[test]fn executable_segment_admitted_but_nx_or_writable_code_denied(){
  let mut p=base();let f=frame(&p);
  assert!(ring3_frame_strict_execution(&f,&p,false));
  p.segments[0].executable=false;
  assert!(!ring3_frame_strict_execution(&f,&p,false));
  p.segments[0].executable=true;p.segments[0].writable=true;
  assert!(!ring3_frame_strict_execution(&f,&p,false));
 }
 #[test]fn overlapping_segment_guard_and_widened_rip_are_denied(){
  let mut p=base();let f=frame(&p);
  p.segment_count=2;p.segments[1]=ProgramSegment{
   virtual_address:USER_MIN+PAGE_BYTES/2,
   page_begin:USER_MIN,page_end:USER_MIN+PAGE_BYTES,
   memory_size:PAGE_BYTES/2,executable:false,writable:true,
   ..ProgramSegment::empty()};
  assert!(!ring3_frame_strict_execution(&f,&p,false));
  p=base();p.guard_begin=p.stack_begin;
  assert!(!ring3_frame_strict_execution(&f,&p,false));
  p=base();let mut wrong=f;wrong[15]=USER_MIN+PAGE_BYTES*2;
  assert!(!ring3_frame_strict_execution(&wrong,&p,false));
  wrong=f;wrong[18]=p.stack_end;
  assert!(!ring3_frame_strict_execution(&wrong,&p,false));
 }
}


// P0-495–501: numeric trap admission must reject reserved CPU flags
// before a later IRET, even without touching user virtual-memory bytes.
#[cfg(test)]
mod kernel_p0_batch20_w3_tests{
    use super::*;
    fn state()->(ProgramPlan,[u64;20]){
        let plan=ProgramPlan{
            entry:USER_MIN,stack_begin:USER_TOP-STACK_BYTES,stack_end:USER_TOP,
            guard_begin:USER_TOP-STACK_BYTES-PAGE_BYTES,segment_count:0,
            segments:[ProgramSegment::empty();MAX_SEGMENTS],
        };
        let mut regs=[0u64;20];
        regs[15]=USER_MIN;regs[16]=0x2b;regs[17]=0x2;
        regs[18]=plan.stack_end-8;regs[19]=0x33;
        (plan,regs)
    }
    #[test]
    fn kernel_p0_batch20_w3_blocks_reserved_iretq_flags(){
        let (p,mut f)=state();
        assert!(ring3_frame_valid(&f,&p,false));
        for bit in [3,5,15,22,31,63]{
            f[17]=2u64|(1u64<<bit);
            assert!(!ring3_frame_valid(&f,&p,false));
        }
        f[17]=2;
        assert!(ring3_frame_valid(&f,&p,false));
    }
    #[test]
    fn kernel_p0_batch20_w3_preserves_interrupt_profile_and_stack(){
        let (p,mut f)=state();
        f[17]=0x202;
        assert!(ring3_frame_valid(&f,&p,true));
        assert!(!ring3_frame_valid(&f,&p,false));
        f[18]=p.guard_begin;
        assert!(!ring3_frame_valid(&f,&p,true));
        f[18]=p.stack_end-8;
        assert!(ring3_frame_valid(&f,&p,true));
    }
}


// P0-535: user memory transfers reject malformed stack extents and
// writable+executable segment aliases at actual syscall admission.
#[cfg(test)]
mod kernel_p0_batch21_w3_tests{
    use super::*;
    fn plan()->ProgramPlan{
        let mut segments=[ProgramSegment::empty();MAX_SEGMENTS];
        segments[0]=ProgramSegment{
            page_begin:USER_MIN,page_end:USER_MIN+PAGE_BYTES,
            writable:true,executable:false,..ProgramSegment::empty()};
        ProgramPlan{entry:USER_MIN,stack_begin:USER_TOP-STACK_BYTES,
            stack_end:USER_TOP,guard_begin:USER_TOP-STACK_BYTES-PAGE_BYTES,
            segment_count:1,segments}
    }
    #[test]fn kernel_p0_batch21_w3_malformed_stack_cannot_authorize_segment(){
        let mut p=plan();
        assert!(user_range(&p,USER_MIN,1,true));
        p.stack_end=USER_TOP+PAGE_BYTES;
        assert!(!user_range(&p,USER_MIN,1,false));
        p=plan();p.stack_begin=USER_MIN-1;
        assert!(!user_range(&p,USER_MIN,1,true));
        p=plan();p.stack_end=p.stack_begin;
        assert!(!user_range(&p,USER_MIN,1,true));
    }
    #[test]fn kernel_p0_batch21_w3_reject_writable_executable_alias(){
        let mut p=plan();
        p.segments[0].executable=true;
        assert!(!user_range(&p,USER_MIN,16,true));
        assert!(user_range(&p,USER_MIN,16,false));
        p.segments[0].executable=false;
        assert!(user_range(&p,USER_MIN,16,true));
    }
}
