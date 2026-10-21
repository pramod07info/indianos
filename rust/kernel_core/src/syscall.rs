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
    number: u64,
    arg0: u64,
    arg1: u64,
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
        2 => request.arg1 == 0, // EXIT
        3 => request.arg0 == 0 && request.arg1 == 0, // YIELD
        _ => false,
    }
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
}
