#!/usr/bin/env python3
"""Run a bounded UEFI QEMU smoke test without ever touching host disks.

Expected behaviour: kernel writes three serial milestones and halts; the
harness terminates the QEMU process once all markers have been observed.
"""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import selectors
import subprocess
import sys
import time

MILESTONES = (
    "IndianOS: kernel entered",
    "IndianOS: boot information validated",
    "IndianOS: milestone 001 complete",
    "IndianOS: exception handlers ready",
    "IndianOS: physical memory manager ready",
    "IndianOS: milestone 002 complete",
    "IndianOS: detached virtual memory manager ready",
    "IndianOS: milestone 003 complete",
    "IndianOS: paging capabilities inspected",
    "IndianOS: milestone 004 complete",
    "IndianOS: ELF64 user-image validation ready",
    "IndianOS: syscall ABI metadata validation ready",
    "IndianOS: milestone 005 foundation complete",
)


def assert_milestones(text: str) -> None:
    """Require complete ordered serial milestones with no reported fatal error."""
    if "IndianOS: FATAL" in text:
        raise AssertionError("kernel reported a fatal boot error")
    offset = 0
    for marker in MILESTONES:
        position = text.find(marker, offset)
        if position == -1:
            raise AssertionError(f"missing or out-of-order milestone: {marker}")
        offset = position + len(marker)

def assert_expected_exception(text: str) -> None:
    """A deliberate invalid-opcode trap must be caught by our own IDT."""
    offset = 0
    expected = (*MILESTONES[:3], "IndianOS: exception handlers ready",
                "IndianOS: physical memory manager ready",
                "IndianOS: triggering intentional UD2",
                "IndianOS: FATAL CPU exception vector=0x0000000000000006")
    for marker in expected:
        position = text.find(marker, offset)
        if position == -1:
            raise AssertionError(f"missing or out-of-order exception marker: {marker}")
        offset = position + len(marker)
    if "IndianOS: milestone 002 complete" in text:
        raise AssertionError("exception build should not return from #UD")


def assert_expected_pagefault(text: str) -> None:
    """Verify intentional missing-page trap records the faulting CR2."""
    expected = (*MILESTONES,
                "IndianOS: triggering intentional page fault",
                "IndianOS: FATAL CPU exception vector=0x000000000000000e",
                "IndianOS: page fault cr2=0x00007fff00000000")
    offset = 0
    for marker in expected:
        position = text.find(marker, offset)
        if position == -1:
            raise AssertionError(f"missing or out-of-order page-fault marker: {marker}")
        offset = position + len(marker)


def assert_expected_shadow(text: str) -> None:
    """Shadow CR3 must switch and restore before milestone completion."""
    expected = (*MILESTONES[:-1],
                "IndianOS: shadow CR3 switched and restored",
                MILESTONES[-1])
    offset = 0
    for marker in expected:
        position = text.find(marker, offset)
        if position == -1:
            raise AssertionError(f"missing or out-of-order shadow marker: {marker}")
        offset = position + len(marker)
    if "IndianOS: FATAL" in text:
        raise AssertionError("shadow-switch kernel reported fatal error")


def run_smoke(iso: Path, ovmf: Path, qemu: str, timeout: float,
              mode: str = "normal") -> str:
    if not iso.is_file():
        raise ValueError(f"ISO not found: {iso}")
    if not ovmf.is_file():
        raise ValueError(f"OVMF firmware missing: {ovmf} (install ovmf or set OVMF_CODE)")
    if timeout <= 0 or timeout > 180:
        raise ValueError("timeout must be between 0 and 180 seconds")
    if mode not in {"normal", "exception", "pagefault", "shadow"}:
        raise ValueError("invalid smoke-test mode")
    verifiers = {
        "normal": assert_milestones,
        "exception": assert_expected_exception,
        "pagefault": assert_expected_pagefault,
        "shadow": assert_expected_shadow,
    }
    verify = verifiers[mode]
    argv = [
        qemu, "-machine", "q35", "-m", "512M",
        "-drive", f"if=pflash,unit=0,format=raw,file={ovmf},readonly=on",
        "-cdrom", str(iso), "-boot", "d", "-display", "none",
        "-serial", "stdio", "-monitor", "none", "-no-reboot", "-no-shutdown",
    ]
    try:
        process = subprocess.Popen(argv, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
    except OSError as exc:
        raise RuntimeError(f"unable to start QEMU: {exc}") from exc
    output = bytearray()
    try:
        assert process.stdout is not None
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            until = time.monotonic() + timeout
            while time.monotonic() < until:
                ready = selector.select(min(0.5, max(0, until - time.monotonic())))
                if ready:
                    piece = os.read(process.stdout.fileno(), 4096)
                    if not piece:
                        break
                    output.extend(piece)
                    if len(output) > 1024 * 1024:
                        raise AssertionError("QEMU emitted too much output")
                    try:
                        verify(output.decode("utf-8", errors="replace"))
                        return output.decode("utf-8", errors="replace")
                    except AssertionError:
                        pass
                if process.poll() is not None:
                    break
        text = output.decode("utf-8", errors="replace")
        verify(text)
        return text
    finally:
        if process.poll() is None:
            process.terminate()
        try:
            process.communicate(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate(timeout=3)


def main() -> int:
    parser = argparse.ArgumentParser(description="Smoke-test IndianOS boot in UEFI QEMU")
    parser.add_argument("--iso", type=Path, required=True)
    parser.add_argument("--ovmf", type=Path, required=True)
    parser.add_argument("--qemu", default="qemu-system-x86_64")
    parser.add_argument("--timeout", type=float, default=25)
    parser.add_argument("--mode", choices=("normal", "exception", "pagefault", "shadow"), default="normal")
    args = parser.parse_args()
    try:
        output = run_smoke(args.iso, args.ovmf, args.qemu, args.timeout, args.mode)
    except (AssertionError, ValueError, RuntimeError, OSError) as exc:
        print(f"IndianOS QEMU smoke test FAILED: {exc}", file=sys.stderr)
        return 1
    print(output)
    print("IndianOS QEMU boot smoke test PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
