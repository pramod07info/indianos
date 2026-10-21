#!/usr/bin/env python3
"""Statically inspect the QEMU-only IndianOS user-mode ELF fixture.

This is a preflight, not proof that CPU privilege transitions or syscalls work.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import struct

EXPECTED_MESSAGE = b"Namaste from IndianOS user mode!"
INT80 = b"\xcd\x80"
MAX_IMAGE = 32 * 1024 * 1024


def check_program(path: Path) -> None:
    data = path.read_bytes()
    if not (64 <= len(data) <= MAX_IMAGE and data[:4] == b"\x7fELF" and
            data[4:7] == b"\x02\x01\x01" and
            struct.unpack_from("<HH", data, 16) == (2, 62) and
            struct.unpack_from("<H", data, 52)[0] == 64):
        raise ValueError("test program must be an ELF64 x86-64 ET_EXEC")
    entry, phoff = struct.unpack_from("<QQ", data, 24)
    phentsize, phnum = struct.unpack_from("<HH", data, 54)
    if (phentsize != 56 or not 1 <= phnum <= 32 or
            phoff > len(data) or phnum * phentsize > len(data) - phoff):
        raise ValueError("malformed program header table")
    executable_entry = False
    interrupts = 0
    message_found = False
    for i in range(phnum):
        offset = phoff + i * phentsize
        kind, flags = struct.unpack_from("<II", data, offset)
        if kind != 1:  # PT_LOAD
            continue
        file_offset, virt, _, file_size, mem_size, align = struct.unpack_from(
            "<QQQQQQ", data, offset + 8
        )
        if (not (flags & 4) or (flags & 3) == 3 or file_size > mem_size or
                file_offset > len(data) or file_size > len(data) - file_offset or
                file_offset % 4096 != virt % 4096 or
                (align > 1 and (align & (align - 1)))):
            raise ValueError("invalid or writable-and-executable load segment")
        content = data[file_offset:file_offset + file_size]
        message_found |= EXPECTED_MESSAGE in content
        if flags & 1:  # executable
            executable_entry |= virt <= entry < virt + file_size
            interrupts += content.count(INT80)
    if not executable_entry:
        raise ValueError("entry is outside initialized executable segment")
    if interrupts < 2:
        raise ValueError("test ELF lacks expected write/exit INT 0x80 instructions")
    if not message_found:
        raise ValueError("test ELF lacks expected greeting in a load segment")


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate the ring-3 demo ELF")
    parser.add_argument("elf", type=Path)
    args = parser.parse_args()
    try:
        check_program(args.elf)
    except (OSError, ValueError, struct.error) as exc:
        parser.error(str(exc))
    print("Experimental ring-3 demo ELF layout and syscall markers verified (not executed).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
