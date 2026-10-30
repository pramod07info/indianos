#!/usr/bin/env python3
"""Actual normal-profile ring3 PID1, UART command integrity and cleanup.

Only disposable sentinel storage is attached. No test kernel/userland feature
flags, serial simulated by the host through QEMU's real emulated UART.
"""
from __future__ import annotations
import argparse
import hashlib
from pathlib import Path
import tempfile
from test_boot_recovery_qemu import Guest, READY, HALTED
from smoke_boot import assert_hardware_inventory, assert_milestones

PID1_READY = "IndianOS user: normal PID1 ready: ABI=1 PID=1 ring3 bootstrap console"
STATUS = "IndianOS user: bootstrap status: PID=1 ABI=1 filesystem=offline desktop=offline"
REJECTED = "IndianOS user: bootstrap: input rejected"

def verify_trace(text: str) -> None:
    expected = ["IndianOS: normal boot starting native PID1", "PID 1 RUNNING",
                PID1_READY, "bootstrap commands: help status halt", STATUS,
                REJECTED, STATUS, REJECTED, STATUS,
                "bootstrap: unsupported command", "bootstrap stopping PID1",
                "PID 1 EXITED", "ring-3 PMM frame count restored", "PID 1 REAPED",
                "normal PID1 exited and resources reclaimed",
                "IndianOS recovery: reason=pid1-stopped", READY]
    at = 0
    for marker in expected:
        found = text.find(marker, at)
        if found < 0:
            raise AssertionError(f"missing/out-of-order normal PID1 response: {marker}")
        at = found + len(marker)
    if text.count(REJECTED) != 2 or text.count(STATUS) != 3:
        raise AssertionError("invalid/overlong input changed command boundaries")
    if "FATAL" in text or "R027 blank disk" in text or "test-r031" in text:
        raise AssertionError("unexpected error or experimental storage path")

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--iso", type=Path, required=True)
    parser.add_argument("--qemu", default="qemu-system-x86_64")
    parser.add_argument("--ovmf", type=Path, default=Path("/usr/share/OVMF/OVMF_CODE.fd"))
    parser.add_argument("--timeout", type=float, default=45)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="indianos-normal-init-") as directory:
        guard = Path(directory) / "sentinel.img"
        original = bytes(range(256)) * 256
        guard.write_bytes(original)
        guest = Guest(args.iso.resolve(), args.qemu, args.ovmf, args.timeout, guard)
        try:
            guest.until(PID1_READY)
            guest.send(b"help\r\nstatus\nha\x00lt\nstatus\n" + b"x"*33 +
                       b"\x08halt\nstatus\nformat\nhalt\n")
            guest.until(READY)
            assert_milestones(guest.text)
            assert_hardware_inventory(guest.text)
            verify_trace(guest.text)
            guest.send(b"status\nhalt\n")
            guest.until(HALTED)
            if "filesystem=offline writes=disabled user-exec=disabled" not in guest.text:
                raise AssertionError("stopped PID1 did not enter read-only recovery")
            print(guest.text)
        except Exception as error:
            raise AssertionError(f"{error}\n--- serial ---\n{guest.text[-65536:]}"
                                 f"\n--- stderr ---\n{guest.errors.decode(errors='replace')}") from error
        finally:
            guest.close()
        if hashlib.sha256(guard.read_bytes()).digest() != hashlib.sha256(original).digest():
            raise AssertionError("ordinary boot modified attached sentinel disk")
    print("PASS: normal ring3 PID1, poisoned UART lines, reaping, read-only recovery and untouched disk")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
