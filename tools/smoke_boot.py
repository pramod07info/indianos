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
import shutil
import subprocess
import tempfile
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
    "IndianOS: read-only initramfs /bin/init validated",
    "IndianOS: initramfs /bin/init staged and released safely",
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
    index = MILESTONES.index("IndianOS: milestone 004 complete")
    expected = (*MILESTONES[:index],
                "IndianOS: shadow CR3 switched and restored",
                *MILESTONES[index:])
    offset = 0
    for marker in expected:
        position = text.find(marker, offset)
        if position == -1:
            raise AssertionError(f"missing or out-of-order shadow marker: {marker}")
        offset = position + len(marker)
    if "IndianOS: FATAL" in text:
        raise AssertionError("shadow-switch kernel reported fatal error")


def assert_expected_usermode(text: str) -> None:
    """Success requires proof of entering user syscall AND returning via exit."""
    if "IndianOS: FATAL" in text:
        raise AssertionError("user-mode kernel reported fatal error")
    expected = (*MILESTONES,
                "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
                "IndianOS user: Namaste from IndianOS user mode!",
                "IndianOS: /bin/init exited through int 0x80",
                "IndianOS: ring-3 user program experiment complete")
    offset = 0
    for marker in expected:
        position = text.find(marker, offset)
        if position < 0:
            raise AssertionError(f"missing or out-of-order user-mode event: {marker}")
        offset = position + len(marker)


def assert_expected_userfault(text: str, mode: str) -> None:
    """The user must fault and be reaped; ring-0 must stay alive."""
    if mode not in ("fault-guard", "fault-supervisor"):
        raise ValueError("invalid fault-isolation mode")
    if "IndianOS: FATAL" in text:
        raise AssertionError("fatal kernel error instead of user fault isolation")
    expected = (
        *MILESTONES,
        "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
        "IndianOS: isolated ring-3 page fault",
        "IndianOS: user page fault contained; kernel resumed",
        "IndianOS: ring-3 borrowed user tables detached",
        "IndianOS: ring-3 supervisor clone released",
        "IndianOS: ring-3 user image released",
        "IndianOS: ring-3 PMM frame count restored",
        "IndianOS: user fault isolation experiment complete",
    )
    offset = 0
    for marker in expected:
        position = text.find(marker, offset)
        if position < 0:
            raise AssertionError(f"missing or out-of-order {mode} marker: {marker}")
        offset = position + len(marker)
    if "IndianOS: /bin/init exited through int 0x80" in text:
        raise AssertionError("faulting process unexpectedly exited via syscall")


def assert_expected_processes(text: str) -> None:
    """Two unique PIDs, two different ELF messages, reaping before reuse."""
    if "IndianOS: FATAL" in text:
        raise AssertionError("kernel fatal during sequential process execution")
    expected = (
        *MILESTONES,
        "IndianOS: M008 sequential process launch begin",
        "IndianOS: PID 1 READY",
        "IndianOS: PID 1 RUNNING",
        "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
        "IndianOS user: Namaste from IndianOS user mode!",
        "IndianOS: PID 1 EXITED",
        "IndianOS: ring-3 PMM frame count restored",
        "IndianOS: PID 1 REAPED",
        "IndianOS: PID 2 READY",
        "IndianOS: PID 2 RUNNING",
        "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
        "IndianOS user: IndianOS worker process 2 completed",
        "IndianOS: PID 2 EXITED",
        "IndianOS: ring-3 PMM frame count restored",
        "IndianOS: PID 2 REAPED",
        "IndianOS: M008 two independent processes exited and reaped",
    )
    offset = 0
    for marker in expected:
        position = text.find(marker, offset)
        if position < 0:
            raise AssertionError(f"missing or out-of-order M008 event: {marker}")
        offset = position + len(marker)
    if text.count("IndianOS: entering ring-3 /bin/init (QEMU experimental)") != 2:
        raise AssertionError("M008 requires exactly two user-mode entries")
    if text.count("IndianOS: ring-3 PMM frame count restored") != 2:
        raise AssertionError("M008 requires physical memory reclamation after each task")
    if "IndianOS: PID 1 FAULTED" in text or "IndianOS: PID 2 FAULTED" in text:
        raise AssertionError("M008 successful task unexpectedly faulted")


def matching_ovmf_vars(code: Path) -> Path | None:
    """Find the matching OVMF_VARS template without altering that template."""
    if not code.name.startswith("OVMF_CODE"):
        return None
    template = code.with_name("OVMF_VARS" + code.name[len("OVMF_CODE"):])
    if template.is_file():
        return template
    return None


def run_smoke(iso: Path, ovmf: Path, qemu: str, timeout: float,
              mode: str = "normal") -> str:
    if not iso.is_file():
        raise ValueError(f"ISO not found: {iso}")
    if not ovmf.is_file():
        raise ValueError(f"OVMF firmware missing: {ovmf} (install ovmf or set OVMF_CODE)")
    if timeout <= 0 or timeout > 180:
        raise ValueError("timeout must be between 0 and 180 seconds")
    if mode not in {"normal", "exception", "pagefault", "shadow", "usermode",
                    "fault-guard", "fault-supervisor", "processes"}:
        raise ValueError("invalid smoke-test mode")
    verifiers = {
        "normal": assert_milestones,
        "exception": assert_expected_exception,
        "pagefault": assert_expected_pagefault,
        "shadow": assert_expected_shadow,
        "usermode": assert_expected_usermode,
        "fault-guard": lambda log: assert_expected_userfault(log, "fault-guard"),
        "fault-supervisor": lambda log: assert_expected_userfault(log, "fault-supervisor"),
        "processes": assert_expected_processes,
    }
    verify = verifiers[mode]
    argv = [
        qemu, "-machine", "q35", "-m", "512M",
        "-drive", f"if=pflash,unit=0,format=raw,file={ovmf},readonly=on",
        "-cdrom", str(iso), "-boot", "d", "-display", "none",
        "-serial", "stdio", "-monitor", "none", "-no-reboot", "-no-shutdown",
    ]
    tempvars = None
    vars_template = matching_ovmf_vars(ovmf)
    if vars_template is not None:
        tempvars = tempfile.TemporaryDirectory(prefix="indianos-ovmf-")
        vars_copy = Path(tempvars.name) / vars_template.name
        try:
            shutil.copyfile(vars_template, vars_copy)
        except OSError as exc:
            tempvars.cleanup()
            raise RuntimeError(f"cannot copy OVMF variable template: {exc}") from exc
        argv.extend(["-drive", f"if=pflash,unit=1,format=raw,file={vars_copy}"])
    try:
        process = subprocess.Popen(argv, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
    except OSError as exc:
        if tempvars is not None:
            tempvars.cleanup()
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
        try:
            verify(text)
        except AssertionError as exc:
            # CI previously hid the useful serial transcript on an incomplete
            # boot; append a bounded diagnostic tail without dumping firmware
            # megabytes or requiring successful milestone verification.
            tail = text[-8192:] if text else "<no serial data>"
            raise AssertionError(f"{exc}\n--- QEMU serial output (last 8192 characters) ---\n{tail}") from exc
        return text
    finally:
        if process.poll() is None:
            process.terminate()
        try:
            process.communicate(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate(timeout=3)
        if tempvars is not None:
            tempvars.cleanup()


def main() -> int:
    parser = argparse.ArgumentParser(description="Smoke-test IndianOS boot in UEFI QEMU")
    parser.add_argument("--iso", type=Path, required=True)
    parser.add_argument("--ovmf", type=Path, required=True)
    parser.add_argument("--qemu", default="qemu-system-x86_64")
    parser.add_argument("--timeout", type=float, default=25)
    parser.add_argument("--mode", choices=("normal", "exception", "pagefault", "shadow", "usermode", "fault-guard", "fault-supervisor", "processes"), default="normal")
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
