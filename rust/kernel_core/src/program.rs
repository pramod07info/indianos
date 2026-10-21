//! Strict, static ELF64/x86-64 program-image planner for IndianOS.
//!
//! Ported from kernel/src/program.c. Never enters or dereferences a user VA;
//! input ELF bytes come from an already accessible kernel buffer. No heap,
//! relocations, interpreter, TLS, dynamic ELF or writable+executable pages.

use core::{ptr, slice};
use crate::syscall::{
    ProgramPlan, ProgramSegment, MAX_SEGMENTS, USER_MIN, USER_TOP,
    STACK_BYTES, PAGE_BYTES, user_range,
};

const MAX_ELF_BYTES: usize = 32 * 1024 * 1024;
const ELF_HEADER: usize = 64;
const PROGRAM_HEADER: usize = 56;
const PAGE_MASK: u64 = PAGE_BYTES - 1;

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let s = bytes.get(at..at + 2)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}
fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let s = bytes.get(at..at + 4)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn read_u64(bytes: &[u8], at: usize) -> Option<u64> {
    let s = bytes.get(at..at + 8)?;
    Some(u64::from_le_bytes([
        s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
    ]))
}
fn within(offset: u64, count: u64, total: u64) -> bool {
    offset <= total && count <= total - offset
}
fn disjoint(a0: u64, a1: u64, b0: u64, b1: u64) -> bool {
    a1 <= b0 || b1 <= a0
}

fn plan_elf64(bytes: &[u8]) -> Option<ProgramPlan> {
    if bytes.len() < ELF_HEADER || bytes.len() > MAX_ELF_BYTES ||
        bytes.get(..4)? != b"\x7fELF" ||
        bytes[4] != 2 || bytes[5] != 1 || bytes[6] != 1 || bytes[7] != 0 ||
        bytes[8..16].iter().any(|&b| b != 0) ||
        read_u16(bytes, 16)? != 2 || // ET_EXEC
        read_u16(bytes, 18)? != 62 || // EM_X86_64
        read_u32(bytes, 20)? != 1 ||
        read_u16(bytes, 52)? != ELF_HEADER as u16 ||
        read_u16(bytes, 54)? != PROGRAM_HEADER as u16 {
        return None;
    }
    let entry = read_u64(bytes, 24)?;
    let phoff = read_u64(bytes, 32)?;
    let phnum = read_u16(bytes, 56)? as u64;
    if phnum == 0 || phnum > 32 ||
        !within(phoff, phnum * PROGRAM_HEADER as u64, bytes.len() as u64) ||
        !(USER_MIN..USER_TOP).contains(&entry) {
        return None;
    }

    let stack_begin = USER_TOP - STACK_BYTES;
    let guard_begin = stack_begin - PAGE_BYTES;
    let mut p = ProgramPlan {
        entry, stack_begin, stack_end: USER_TOP, guard_begin,
        segment_count: 0,
        segments: [ProgramSegment::empty(); MAX_SEGMENTS],
    };
    let mut executable_entry = false;
    for index in 0..(phnum as usize) {
        let off = (phoff as usize) + index * PROGRAM_HEADER;
        let ph = bytes.get(off..off + PROGRAM_HEADER)?;
        let kind = read_u32(ph, 0)?;
        let flags = read_u32(ph, 4)?;
        // No PT_INTERP/DYNAMIC/TLS or executable PT_GNU_STACK.
        if kind == 3 || kind == 2 || kind == 7 ||
            (kind == 0x6474e551 && flags & 1 != 0) {
            return None;
        }
        if kind != 1 { continue; } // PT_LOAD only.
        if p.segment_count as usize >= MAX_SEGMENTS ||
            flags & !7 != 0 || flags & 4 == 0 || flags & 3 == 3 {
            return None;
        }
        let offset = read_u64(ph, 8)?;
        let va = read_u64(ph, 16)?;
        let file_size = read_u64(ph, 32)?;
        let memory_size = read_u64(ph, 40)?;
        let align = read_u64(ph, 48)?;
        if memory_size == 0 || file_size > memory_size ||
            !within(offset, file_size, bytes.len() as u64) ||
            !(USER_MIN..guard_begin).contains(&va) ||
            memory_size > guard_begin - va ||
            (offset & PAGE_MASK) != (va & PAGE_MASK) ||
            (align > 1 && (!align.is_power_of_two() ||
                           (va & (align - 1)) != (offset & (align - 1)))) {
            return None;
        }
        let end = va.checked_add(memory_size)?;
        if end > u64::MAX - PAGE_MASK { return None; }
        let page_begin = va & !PAGE_MASK;
        let page_end = (end + PAGE_MASK) & !PAGE_MASK;

        for old in &p.segments[..p.segment_count as usize] {
            if !disjoint(page_begin, page_end, old.page_begin, old.page_end) {
                return None;
            }
        }
        p.segments[p.segment_count as usize] = ProgramSegment {
            virtual_address: va, file_offset: offset,
            file_size, memory_size, page_begin, page_end,
            writable: flags & 2 != 0,
            executable: flags & 1 != 0,
        };
        p.segment_count += 1;
        if flags & 1 != 0 && entry >= va && entry - va < file_size {
            executable_entry = true;
        }
    }
    if p.segment_count == 0 || !executable_entry { return None; }
    Some(p)
}

/// # Safety
/// \`elf\` must point to at least \`elf_size\` readable kernel bytes
/// (not a userspace VA). \`out\` is a valid writable kernel pointer or NULL.
/// Invalid ELF data leaves the caller's original plan untouched.
#[no_mangle]
pub unsafe extern "C" fn indianos_program_plan_elf64(
    elf: *const u8, elf_size: usize, out: *mut ProgramPlan
) -> bool {
    if elf.is_null() || out.is_null() ||
        !(ELF_HEADER..=MAX_ELF_BYTES).contains(&elf_size) {
        return false;
    }
    let bytes = unsafe { slice::from_raw_parts(elf, elf_size) };
    let Some(plan) = plan_elf64(bytes) else { return false; };
    unsafe { ptr::write(out, plan); }
    true
}

/// # Safety
/// \`plan\` is either NULL or a valid readable kernel memory address.
#[no_mangle]
pub unsafe extern "C" fn indianos_program_user_range(
    plan: *const ProgramPlan, address: u64, length: u64, writable: bool
) -> bool {
    let Some(p) = (unsafe { plan.as_ref() }) else { return false; };
    user_range(p, address, length, writable)
}

#[no_mangle]
pub extern "C" fn indianos_program_self_test() -> bool {
    let mut elf = [0u8; 0x110];
    elf[0..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2; elf[5] = 1; elf[6] = 1;
    elf[16] = 2; elf[18] = 62; elf[20] = 1;
    elf[24] = 0; elf[25] = 1; elf[26] = 0x40;
    elf[32] = 64; elf[52] = 64; elf[54] = 56; elf[56] = 1;
    elf[64] = 1; elf[68] = 5;
    elf[72] = 0; elf[73] = 1;
    elf[80] = 0; elf[81] = 1; elf[82] = 0x40;
    elf[96] = 1; elf[104] = 0; elf[105] = 0x10;
    elf[112] = 0; elf[113] = 0x10;
    elf[256] = 0xc3;
    let mut plan = ProgramPlan {
        entry: 0, stack_begin: 0, stack_end: 0, guard_begin: 0,
        segment_count: 0, segments: [ProgramSegment::empty(); MAX_SEGMENTS],
    };
    plan_elf64(&elf).is_some() &&
        unsafe { indianos_program_plan_elf64(elf.as_ptr(), elf.len(), &mut plan) } &&
        plan.entry == USER_MIN + 0x100 &&
        plan.segment_count == 1 &&
        plan.segments[0].executable && !plan.segments[0].writable &&
        user_range(&plan, plan.entry, 1, false) &&
        !user_range(&plan, plan.entry, 1, true) &&
        !user_range(&plan, plan.guard_begin, 1, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn valid_fixture_and_boundary_cases() {
        assert!(indianos_program_self_test());
        let mut elf = [0u8; 0x110];
        elf[0..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2; elf[5] = 1; elf[6] = 1; elf[16] = 2;
        elf[18] = 62; elf[20] = 1;
        elf[24] = 0; elf[25] = 1; elf[26] = 0x40;
        elf[32] = 64; elf[52] = 64; elf[54] = 56; elf[56] = 1;
        elf[64] = 1; elf[68] = 5; elf[72] = 0; elf[73] = 1;
        elf[80] = 0; elf[81] = 1; elf[82] = 0x40;
        elf[96] = 1; elf[105] = 0x10; elf[113] = 0x10; elf[256] = 0xc3;
        assert!(plan_elf64(&elf).is_some());
        let mut invalid = elf;
        invalid[18] = 3; // wrong architecture
        assert!(plan_elf64(&invalid).is_none());
        invalid = elf; invalid[68] = 7; // W+X rejected
        assert!(plan_elf64(&invalid).is_none());
        invalid = elf; invalid[56] = 33; // excessive phnum
        assert!(plan_elf64(&invalid).is_none());
        invalid = elf; invalid[96] = 0xff; // initialized bytes exceed ELF file bounds
        assert!(plan_elf64(&invalid).is_none());
    }
}
