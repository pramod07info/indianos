//! IndianOS native Rust kernel core, migration slice R001.
//!
//! \`no_std\`, allocation-free process lifecycle, round-robin scheduler and
//! strict fault-origin policy. Existing C/assembly callers use a deliberate
//! \`repr(C)\` ABI. All exported raw-pointer entry points are unsafe because
//! the kernel caller must supply valid, properly aligned, exclusively
//! accessible pointers. This module is NOT a generic userspace FFI surface.
#![cfg_attr(not(any(test, feature = "std")), no_std)]
#![deny(unsafe_op_in_unsafe_fn)]

use core::ptr;

mod syscall;
mod program;
mod initramfs;
pub mod block;
pub mod tinyfs;
pub mod quantum;
pub mod quantum_circuit;
pub mod quantum_protocol;
pub mod quantum_queue;
pub mod quantum_service;
pub mod quantum_abi;
#[cfg(target_arch = "x86_64")]
pub mod ata_pio;
#[cfg(feature = "vmm")]
mod vmm;
#[cfg(feature = "pmm")]
mod pmm;
#[cfg(feature = "user-image")]
mod user_image;
#[cfg(feature = "paging")]
mod paging;
#[cfg(feature = "paging-cpu")]
mod paging_cpu;
#[cfg(feature = "user-runtime")]
mod user_runtime;
#[cfg(feature = "test-interactive")]
mod input;
#[cfg(feature = "test-irq-keyboard")]
mod console;
#[cfg(feature = "runtime")]
mod runtime;
#[cfg(feature = "boot-staging")]
mod boot_staging;
#[cfg(feature = "boot")]
mod boot;
#[cfg(feature = "interrupts")]
mod interrupts;
#[cfg(feature = "timer")]
mod timer;
#[cfg(feature = "kernel-main")]
mod kernel_main;
#[cfg(feature = "limine-requests")]
mod limine_requests;
#[cfg(all(target_arch = "x86_64", feature = "cpu-entry", not(any(test, feature = "std"))))]
mod cpu_entry;

pub const MAX_PROCESSES: usize = 4;
pub const MAX_DISPATCHES: u32 = if cfg!(feature = "preempt") { 2048 } else { 32 };

const EMPTY: i32 = 0;
const NEW: i32 = 1;
const READY: i32 = 2;
const RUNNING: i32 = 3;
const EXITED: i32 = 4;
const FAULTED: i32 = 5;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Process {
    pub pid: u32,
    pub state: i32, // C enum (int). Reject invalid values rather than UB.
    pub address_space_root: u64,
    pub exit_status: u64,
    pub fault_vector: u64,
    pub fault_error: u64,
    pub fault_address: u64,
    pub resources_reclaimed: bool,
}

impl Process {
    pub const fn empty() -> Self {
        Self {
            pid: 0, state: EMPTY, address_space_root: 0, exit_status: 0,
            fault_vector: 0, fault_error: 0, fault_address: 0,
            resources_reclaimed: false,
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct ProcessTable {
    pub slots: [Process; MAX_PROCESSES],
    pub count: u32,
    pub next_pid: u32,
}

impl ProcessTable {
    pub const fn empty() -> Self {
        Self { slots: [Process::empty(); MAX_PROCESSES], count: 0, next_pid: 1 }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct Scheduler {
    pub table: *mut ProcessTable,
    pub cursor: u32,
    pub dispatch_count: u32,
    pub yield_count: u32,
    pub tick_count: u64,
}

impl Scheduler {
    pub const fn empty() -> Self {
        Self {
            table: ptr::null_mut(), cursor: 0, dispatch_count: 0,
            yield_count: 0, tick_count: 0,
        }
    }
}

/// # Safety
/// Caller passes either NULL or a writable pointer to an aligned ProcessTable.
/// The table need not be initialized before this call.
#[no_mangle]
pub unsafe extern "C" fn indianos_process_table_init(table: *mut ProcessTable) -> bool {
    if table.is_null() { return false; }
    unsafe { ptr::write(table, ProcessTable::empty()); }
    true
}

/// # Safety
/// \`table\` is either NULL or points to a valid, exclusively borrowed table.
/// Returned process pointers are invalid after table reinitialization.
#[no_mangle]
pub unsafe extern "C" fn indianos_process_create(table: *mut ProcessTable) -> *mut Process {
    let Some(t) = (unsafe { table.as_mut() }) else { return ptr::null_mut(); };
    if t.next_pid == 0 || t.count as usize >= MAX_PROCESSES {
        return ptr::null_mut();
    }
    let index = t.count as usize;
    t.slots[index] = Process {
        pid: t.next_pid, state: NEW, ..Process::empty()
    };
    t.count += 1;
    t.next_pid = t.next_pid.wrapping_add(1);
    &mut t.slots[index]
}

/// # Safety
/// \`p\` is NULL or an exclusive, aligned pointer to a live PCB.
#[no_mangle]
pub unsafe extern "C" fn indianos_process_ready(p: *mut Process) -> bool {
    let Some(p) = (unsafe { p.as_mut() }) else { return false; };
    if p.pid == 0 || p.state != NEW { return false; }
    p.state = READY;
    true
}

/// # Safety
/// \`p\` is NULL or an exclusive, aligned pointer to a live PCB.
#[no_mangle]
pub unsafe extern "C" fn indianos_process_bind(p: *mut Process, root: u64) -> bool {
    let Some(p) = (unsafe { p.as_mut() }) else { return false; };
    if p.pid == 0 || p.state != READY || p.address_space_root != 0 ||
        p.resources_reclaimed || root == 0 || (root & 4095) != 0 {
        return false;
    }
    p.address_space_root = root;
    true
}

/// # Safety
/// \`p\` is NULL or an exclusive, aligned pointer to a live PCB.
#[no_mangle]
pub unsafe extern "C" fn indianos_process_start(p: *mut Process, root: u64) -> bool {
    let Some(p) = (unsafe { p.as_mut() }) else { return false; };
    if p.pid == 0 || p.state != READY || root == 0 ||
        root & 4095 != 0 || p.resources_reclaimed ||
        (p.address_space_root != 0 && p.address_space_root != root) {
        return false;
    }
    p.state = RUNNING;
    p.address_space_root = root;
    true
}

/// # Safety
/// \`p\` is NULL or an exclusive, aligned pointer to a live PCB.
#[no_mangle]
pub unsafe extern "C" fn indianos_process_pause(p: *mut Process) -> bool {
    let Some(p) = (unsafe { p.as_mut() }) else { return false; };
    if p.pid == 0 || p.state != RUNNING || p.address_space_root == 0 ||
        p.resources_reclaimed { return false; }
    p.state = READY;
    true
}

/// # Safety
/// \`p\` is NULL or an exclusive, aligned pointer to a live PCB.
#[no_mangle]
pub unsafe extern "C" fn indianos_process_resume(p: *mut Process) -> bool {
    let Some(p) = (unsafe { p.as_mut() }) else { return false; };
    if p.pid == 0 || p.state != READY || p.address_space_root == 0 ||
        p.resources_reclaimed { return false; }
    p.state = RUNNING;
    true
}

/// # Safety
/// \`p\` is NULL or an exclusive, aligned pointer to a live PCB.
#[no_mangle]
pub unsafe extern "C" fn indianos_process_exit(p: *mut Process, status: u64) -> bool {
    let Some(p) = (unsafe { p.as_mut() }) else { return false; };
    if p.state != RUNNING || p.address_space_root == 0 { return false; }
    p.state = EXITED;
    p.exit_status = status;
    true
}

/// # Safety
/// \`p\` is NULL or an exclusive, aligned pointer to a live PCB.
#[no_mangle]
pub unsafe extern "C" fn indianos_process_fault(
    p: *mut Process, status: u64, vector: u64, error: u64, address: u64
) -> bool {
    let Some(p) = (unsafe { p.as_mut() }) else { return false; };
    if p.state != RUNNING || p.address_space_root == 0 || vector == 0 { return false; }
    p.state = FAULTED;
    p.exit_status = status;
    p.fault_vector = vector;
    p.fault_error = error;
    p.fault_address = address;
    true
}

/// # Safety
/// The caller has ALREADY restored kernel CR3 and freed the process's
/// physical allocations. This records ownership release; it never frees.
#[no_mangle]
pub unsafe extern "C" fn indianos_process_reap(p: *mut Process) -> bool {
    let Some(p) = (unsafe { p.as_mut() }) else { return false; };
    if (p.state != EXITED && p.state != FAULTED) ||
        p.resources_reclaimed || p.address_space_root == 0 { return false; }
    p.address_space_root = 0;
    p.resources_reclaimed = true;
    true
}

/// # Safety
/// \`p\` is NULL or a valid, readable aligned PCB.
#[no_mangle]
pub unsafe extern "C" fn indianos_process_success(p: *const Process) -> bool {
    let Some(p) = (unsafe { p.as_ref() }) else { return false; };
    p.pid != 0 && p.state == EXITED && p.exit_status == 0 &&
        p.fault_vector == 0 && p.address_space_root == 0 &&
        p.resources_reclaimed
}

/// # Safety
/// The table is exclusively borrowed through \`scheduler\` or NULL; the
/// argument may be an uninitialized writable Scheduler.
#[no_mangle]
pub unsafe extern "C" fn indianos_scheduler_init(
    scheduler: *mut Scheduler, table: *mut ProcessTable
) -> bool {
    if scheduler.is_null() || table.is_null() { return false; }
    let t = unsafe { &*table };
    if t.count == 0 || t.count as usize > MAX_PROCESSES { return false; }
    unsafe { ptr::write(scheduler, Scheduler { table, ..Scheduler::empty() }); }
    true
}

/// # Safety
/// \`s\` and its embedded table pointer must refer to valid, distinct kernel
/// objects that are not being concurrently mutated on another CPU.
#[no_mangle]
pub unsafe extern "C" fn indianos_scheduler_next(s: *mut Scheduler) -> *mut Process {
    let Some(s) = (unsafe { s.as_mut() }) else { return ptr::null_mut(); };
    let Some(table) = (unsafe { s.table.as_mut() }) else { return ptr::null_mut(); };
    let count = table.count as usize;
    if count == 0 || count > MAX_PROCESSES || s.dispatch_count >= MAX_DISPATCHES {
        return ptr::null_mut();
    }
    for offset in 0..count {
        let index = (s.cursor as usize + offset) % count;
        let p = &mut table.slots[index];
        if p.pid != 0 && p.state == READY &&
            p.address_space_root != 0 && !p.resources_reclaimed {
            s.cursor = ((index + 1) % count) as u32;
            s.dispatch_count += 1;
            return p;
        }
    }
    ptr::null_mut()
}

/// # Safety
/// \`p\` must refer to an actual PCB slot in \`s.table\`; the kernel provides
/// exclusive mutable access to both objects for this operation.
#[no_mangle]
pub unsafe extern "C" fn indianos_scheduler_yield(
    s: *mut Scheduler, p: *mut Process
) -> bool {
    if p.is_null() { return false; }
    let Some(s) = (unsafe { s.as_mut() }) else { return false; };
    let Some(table) = (unsafe { s.table.as_mut() }) else { return false; };
    if table.count as usize > MAX_PROCESSES { return false; }
    // Validate slot membership BEFORE dereferencing an untrusted address.
    let found = table.slots[..table.count as usize].iter().any(
        |slot| ptr::eq(slot as *const Process, p as *const Process)
    );
    if !found { return false; }
    let success = unsafe { indianos_process_pause(p) };
    if success { s.yield_count = s.yield_count.wrapping_add(1); }
    success
}

/// # Safety
/// \`s\` is NULL or a valid, exclusively writable Scheduler pointer.
#[no_mangle]
pub unsafe extern "C" fn indianos_scheduler_account_tick(s: *mut Scheduler) {
    if let Some(s) = unsafe { s.as_mut() } {
        s.tick_count = s.tick_count.saturating_add(1);
    }
}

/// # Safety
/// \`s\` and its table pointer must be valid and readable (or NULL).
#[no_mangle]
pub unsafe extern "C" fn indianos_scheduler_all_terminal(s: *const Scheduler) -> bool {
    let Some(s) = (unsafe { s.as_ref() }) else { return false; };
    let Some(t) = (unsafe { s.table.as_ref() }) else { return false; };
    if t.count == 0 || t.count as usize > MAX_PROCESSES { return false; }
    t.slots[..t.count as usize].iter().all(|p| {
        p.pid != 0 && (p.state == EXITED || p.state == FAULTED) &&
            p.resources_reclaimed
    })
}

/// The IDT dispatcher calls this as a last-chance policy check.
#[no_mangle]
pub extern "C" fn indianos_user_fault_eligible(
    vector: u64, cs: u64, error_code: u64, active: bool
) -> bool {
    if !active || cs != 0x2b { return false; }
    match vector {
        14 => (error_code & 0x8) == 0,
        13 | 6 => true,
        _ => false,
    }
}

#[cfg(all(not(test), not(feature = "std")))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop { core::hint::spin_loop(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of, offset_of};

    #[test]
    fn stable_c_layout() {
        assert_eq!(size_of::<Process>(), 56);
        assert_eq!(align_of::<Process>(), 8);
        assert_eq!(offset_of!(Process, address_space_root), 8);
        assert_eq!(offset_of!(Process, resources_reclaimed), 48);
        assert_eq!(size_of::<ProcessTable>(), 232);
        assert_eq!(size_of::<Scheduler>(), 32);
        assert_eq!(offset_of!(Scheduler, tick_count), 24);
    }

    #[test]
    fn fault_policy_never_recovers_kernel() {
        assert!(indianos_user_fault_eligible(14, 0x2b, 4, true));
        assert!(!indianos_user_fault_eligible(14, 0x08, 4, true));
        assert!(!indianos_user_fault_eligible(14, 0x2b, 12, true));
        assert!(!indianos_user_fault_eligible(14, 0x2b, 4, false));
        assert!(indianos_user_fault_eligible(13, 0x2b, 0, true));
    }

    #[test]
    fn alternating_processes_cannot_reuse_roots_or_ids() {
        let mut table = ProcessTable::empty();
        unsafe {
            assert!(indianos_process_table_init(&mut table));
            let a = indianos_process_create(&mut table);
            let b = indianos_process_create(&mut table);
            assert_eq!((*a).pid, 1);
            assert_eq!((*b).pid, 2);
            assert!(!indianos_process_start(a, 0));
            assert!(indianos_process_ready(a));
            assert!(indianos_process_ready(b));
            assert!(indianos_process_bind(a, 0x1000));
            assert!(indianos_process_bind(b, 0x2000));
            let mut scheduler = Scheduler::empty();
            assert!(indianos_scheduler_init(&mut scheduler, &mut table));
            assert_eq!(indianos_scheduler_next(&mut scheduler), a);
            assert!(indianos_process_start(a, 0x1000));
            assert!(indianos_scheduler_yield(&mut scheduler, a));
            assert_eq!(indianos_scheduler_next(&mut scheduler), b);
            assert!(indianos_process_start(b, 0x2000));
            assert!(indianos_scheduler_yield(&mut scheduler, b));
            assert_eq!(indianos_scheduler_next(&mut scheduler), a);
            assert!(indianos_process_resume(a));
            assert!(indianos_process_exit(a, 0));
            assert!(!indianos_process_success(a));
            assert!(indianos_process_reap(a));
            assert!(indianos_process_success(a));
            assert_eq!(indianos_scheduler_next(&mut scheduler), b);
            assert!(indianos_process_resume(b));
            assert!(indianos_process_fault(b, 0x10e, 14, 4, 0x4000));
            assert!(indianos_process_reap(b));
            assert!(indianos_scheduler_all_terminal(&scheduler));
            assert!(!indianos_process_success(b));
            assert_eq!(scheduler.yield_count, 2);
            assert_eq!(scheduler.dispatch_count, 4);
        }
    }

    #[test]
    fn table_capacity_and_tick_saturation() {
        let mut table = ProcessTable::empty();
        unsafe {
            assert!(indianos_process_table_init(&mut table));
            for expected in 1..=4 {
                let p = indianos_process_create(&mut table);
                assert!(!p.is_null());
                assert_eq!((*p).pid, expected);
            }
            assert!(indianos_process_create(&mut table).is_null());
            let mut scheduler = Scheduler {
                table: &mut table, tick_count: u64::MAX,
                ..Scheduler::empty()
            };
            indianos_scheduler_account_tick(&mut scheduler);
            assert_eq!(scheduler.tick_count, u64::MAX);
        }
    }
}
