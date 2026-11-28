#!/usr/bin/env python3
"""Verify actual timer interruption inside each ELF's seeded x87/SSE loop."""
import argparse
from pathlib import Path
import re
import subprocess
from smoke_boot import assert_expected_preempt

CONFIGURED = "IndianOS: R024 eager x87 SSE contexts configured"
BEGIN = "indianos_preempt_fpu_busy_begin"
END = "indianos_preempt_fpu_busy_end"
RIP = re.compile(r"IndianOS: R024 timer interrupted user RIP=0x([0-9a-f]{16})")


def symbol_bounds(symbols: str) -> tuple[int, int]:
    found = {}
    for line in symbols.splitlines():
        fields = line.split()
        if fields and fields[-1] in (BEGIN, END):
            name = fields[-1]
            if name in found or len(fields) < 8 or fields[6] == "UND":
                raise AssertionError("duplicate or undefined FPU loop symbol")
            try:
                found[name] = int(fields[1], 16)
            except ValueError as error:
                raise AssertionError("malformed FPU loop address") from error
    if set(found) != {BEGIN, END} or not 0x400000 <= found[BEGIN] < found[END] < (1 << 47):
        raise AssertionError("missing or invalid bounded user FPU loop")
    return found[BEGIN], found[END]


def verify_preempt_fpu(text: str, bounds: dict[int, tuple[int, int]]) -> None:
    # Preserve the existing actual APIC concurrency/exit/reclamation contract.
    assert_expected_preempt(text)
    lines = text.replace("\r", "").splitlines()
    if lines.count(CONFIGURED) != 1 or set(bounds) != {1, 2}:
        raise AssertionError("missing configured contexts or exact process bounds")
    configured = lines.index(CONFIGURED)
    for pid, label in ((1, "A"), (2, "B")):
        begin, end = bounds[pid]
        if not 0x400000 <= begin < end < (1 << 47):
            raise AssertionError("invalid process FPU loop bounds")
        proof = f"IndianOS user: R024 timer task {label} x87 SSE control and registers retained"
        if lines.count(proof) != 1:
            raise AssertionError("missing or duplicate checked FPU register receipt")
        started = lines.index(f"IndianOS user: M009 timer task {label} started")
        finished = lines.index(f"IndianOS user: M009 timer task {label} finished")
        checked = lines.index(proof)
        if not configured < started < checked < finished:
            raise AssertionError("FPU receipt is outside the live task lifetime")
        interrupted = False
        for index in range(started + 1, checked):
            if lines[index] != f"IndianOS: PID {pid} APIC_TIMER_PREEMPTED":
                continue
            sample = RIP.fullmatch(lines[index + 1])
            if sample and begin <= int(sample[1], 16) < end:
                interrupted = True
        if not interrupted:
            raise AssertionError(f"PID{pid} lacks actual interruption inside its seeded loop")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--init-elf", type=Path, required=True)
    parser.add_argument("--worker-elf", type=Path, required=True)
    args = parser.parse_args()
    bounds = {}
    for pid, path in ((1, args.init_elf), (2, args.worker_elf)):
        symbols = subprocess.check_output(["readelf", "--wide", "--symbols", str(path)], text=True)
        bounds[pid] = symbol_bounds(symbols)
    verify_preempt_fpu(args.log.read_text(errors="replace"), bounds)
    for pid,(begin,end) in bounds.items():
        print(f"PID{pid} verified ELF seeded loop: 0x{begin:x}..0x{end:x}")
    print("PASS: actual APIC interrupts inside both seeded x87/SSE loops; register/control state retained")


if __name__ == "__main__":
    main()
