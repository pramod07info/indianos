//! IndianOS Rust-owned x86-64 serial UART and freestanding memory runtime.
//!
//! Feature "runtime" explicitly selects these bare-metal exports. The
//! kernel's original serial.c and memory_runtime.c remain a verified
//! fallback until QEMU confirms the Rust replacements. No heap, no std.

use core::{arch::asm, ffi::{c_char, c_int, c_void}, ptr};

const COM1: u16 = 0x3f8;
#[inline(always)]
unsafe fn outb(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value,
             options(nostack, nomem, preserves_flags));
    }
}
#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!("in al, dx", out("al") value, in("dx") port,
             options(nostack, nomem, preserves_flags));
    }
    value
}
#[no_mangle]
pub extern "C" fn serial_init() {
    unsafe {
        outb(COM1 + 1, 0);
        outb(COM1 + 3, 0x80);
        outb(COM1, 0x03); // divisor 3 => 38400 baud
        outb(COM1 + 1, 0);
        outb(COM1 + 3, 3); // 8-N-1
        outb(COM1 + 2, 0xc7);
        outb(COM1 + 4, 0x0b);
    }
}
fn serial_putc(byte: u8) {
    for _ in 0..100_000 {
        if unsafe { inb(COM1 + 5) } & 0x20 != 0 {
            unsafe { outb(COM1, byte); }
            return;
        }
    }
}
/// # Safety
/// \`message\` is a trusted NUL-terminated supervisor-memory string or NULL.
/// Never pass unchecked user pointers here.
#[no_mangle]
pub unsafe extern "C" fn serial_write(message: *const c_char) {
    if message.is_null() { return; }
    let mut p = message.cast::<u8>();
    loop {
        let ch = unsafe { ptr::read(p) };
        if ch == 0 { return; }
        if ch == b'\n' { serial_putc(b'\r'); }
        serial_putc(ch);
        p = unsafe { p.add(1) };
    }
}

/// The following compiler runtime functions deliberately use explicit
/// x86-64 REP string instructions, not pointer loops that rustc could
/// optimize recursively into calls to these same exported symbols.
/// Single CPU, DF must be 0 on return per System V kernel ABI.

/// # Safety
/// Destination is valid and writable for count bytes.
#[no_mangle]
pub unsafe extern "C" fn memset(
    dest: *mut c_void, value: c_int, count: usize,
) -> *mut c_void {
    let d = dest;
    let mut output = dest;
    let mut remaining = count;
    unsafe {
        asm!("cld", "rep stosb",
             inout("rdi") output, in("al") value as u8,
             inout("rcx") remaining, options(nostack));
    }
    let _ = (output, remaining);
    d
}
/// # Safety
/// Source and destination are valid and nonoverlapping for count bytes.
#[no_mangle]
pub unsafe extern "C" fn memcpy(
    dest: *mut c_void, source: *const c_void, count: usize,
) -> *mut c_void {
    let d = dest;
    let mut output = dest;
    let mut input = source;
    let mut remaining = count;
    unsafe {
        asm!("cld", "rep movsb",
             inout("rdi") output, inout("rsi") input,
             inout("rcx") remaining, options(nostack));
    }
    let _ = (output, input, remaining);
    d
}
/// # Safety
/// Source and destination are valid for count bytes; ranges may overlap.
#[no_mangle]
pub unsafe extern "C" fn memmove(
    dest: *mut c_void, source: *const c_void, count: usize,
) -> *mut c_void {
    if count == 0 || dest.cast_const() == source { return dest; }
    let d = dest as usize;
    let s = source as usize;
    if d < s || d - s >= count {
        return unsafe { memcpy(dest, source, count) };
    }
    // Overlap and destination lies higher: copy backwards, restoring DF=0.
    let mut output = unsafe { dest.cast::<u8>().add(count - 1) };
    let mut input = unsafe { source.cast::<u8>().add(count - 1) };
    let mut remaining = count;
    unsafe {
        asm!("std", "rep movsb", "cld",
             inout("rdi") output, inout("rsi") input,
             inout("rcx") remaining, options(nostack));
    }
    let _ = (output, input, remaining);
    dest
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_memmove_and_memset() {
        let mut arr = *b"abcdefgh";
        unsafe {
            memmove(arr.as_mut_ptr().add(2).cast(), arr.as_ptr().cast(), 6);
        }
        assert_eq!(&arr, b"ababcdef");
        unsafe { memset(arr.as_mut_ptr().cast(), 0x51, 3); }
        assert_eq!(&arr, b"QQQbcdef");
        let mut copy = [0u8; 8];
        unsafe { memcpy(copy.as_mut_ptr().cast(), arr.as_ptr().cast(), 8); }
        assert_eq!(arr, copy);
    }
}
