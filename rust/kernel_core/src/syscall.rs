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
        length > USER_TOP - address {
        return false;
    }
    if address >= plan.stack_begin &&
        plan.stack_end.checked_sub(address).is_some_and(|available| length <= available) {
        return true;
    }
    for seg in plan.segments.iter().take((plan.segment_count as usize).min(MAX_SEGMENTS)) {
        if address >= seg.page_begin && address < seg.page_end &&
            seg.page_end.checked_sub(address).is_some_and(|available| length <= available) &&
            (!write || seg.writable) {
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
        #[cfg(any(feature = "test-shell", feature = "test-ipc"))]
        4 => request.arg0 == 0 && request.arg1 == 0, // GETPID
        #[cfg(feature = "test-shell")]
        5 => request.arg0 == 1 && request.arg1 == 0, // request /bin/worker
        #[cfg(feature="test-interactive")]
        6 => request.arg1 == 1 && user_range(plan,request.arg0,1,true),
        #[cfg(all(feature="test-ipc",not(feature="test-r025")))]
        8 => (4..=68).contains(&request.arg1) &&
            user_range(plan,request.arg0,request.arg1,false), // IPC_SEND (R025.1)
        #[cfg(feature="test-r025")]
        8 => (12..=76).contains(&request.arg1) &&
            user_range(plan,request.arg0,request.arg1,false), // Cap-authorized SEND
        #[cfg(feature="test-ipc")]
        9 => request.arg1 == 72 &&
            user_range(plan,request.arg0,72,true), // IPC_RECEIVE
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
    let (rip, cs, flags, rsp, ss) =
        (frame[15], frame[16], frame[17], frame[18], frame[19]);
    cs == 0x2b && ss == 0x33 &&
        flags & FIXED != 0 &&
        flags & (IOPL | NT | VM) == 0 &&
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

    #[cfg(feature = "test-shell")]
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
        assert!(valid(5,1,0));
        assert!(!valid(5,2,0));
        assert!(!valid(5,1,1));
    }
    #[cfg(feature="test-interactive")]
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
    #[cfg(feature="test-ipc")]
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
