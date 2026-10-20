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


def run_smoke(iso: Path, ovmf: Path, qemu: str, timeout: float) -> str:
    if not iso.is_file():
        raise ValueError(f"ISO not found: {iso}")
    if not ovmf.is_file():
        raise ValueError(f"OVMF firmware missing: {ovmf} (install ovmf or set OVMF_CODE)")
    if timeout <= 0 or timeout > 180:
        raise ValueError("timeout must be between 0 and 180 seconds")
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
                        assert_milestones(output.decode("utf-8", errors="replace"))
                        return output.decode("utf-8", errors="replace")
                    except AssertionError:
                        pass
                if process.poll() is not None:
                    break
        text = output.decode("utf-8", errors="replace")
        assert_milestones(text)
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
    args = parser.parse_args()
    try:
        output = run_smoke(args.iso, args.ovmf, args.qemu, args.timeout)
    except (AssertionError, ValueError, RuntimeError, OSError) as exc:
        print(f"IndianOS QEMU smoke test FAILED: {exc}", file=sys.stderr)
        return 1
    print(output)
    print("IndianOS QEMU boot smoke test PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
