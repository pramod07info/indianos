#!/usr/bin/env python3
"""Require the user ELF fixture to contain an int 0x80 software trap opcode.

This is a *preflight* only: opcode presence does not prove reachability,
correct ABI, isolation, or successful execution. The QEMU test verifies those.
"""
from __future__ import annotations
import argparse
from pathlib import Path

INT80 = b"\xcd\x80"


def check_program(path: Path) -> None:
    data = path.read_bytes()
    if not data.startswith(b"\x7fELF") or len(data) < 64:
        raise ValueError("user fixture is not ELF")
    if INT80 not in data:
        raise ValueError(
            "QEMU ring-3 test unavailable: /bin/init still lacks an INT 0x80 "
            "syscall instruction. The current demo is an infinite-loop fixture."
        )


def main() -> int:
    parser = argparse.ArgumentParser(description="Check ring-3 ELF test fixture")
    parser.add_argument("elf", type=Path)
    args = parser.parse_args()
    try:
        check_program(args.elf)
    except (OSError, ValueError) as exc:
        parser.error(str(exc))
    print("Experimental ring-3 ELF includes INT 0x80; QEMU verification is still required.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
