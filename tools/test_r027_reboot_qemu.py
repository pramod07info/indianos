#!/usr/bin/env python3
"""R027 reboot and torn-metadata recovery proof using a disposable raw disk.

The runner creates a fixed, temporary 128-sector image, boots one QEMU ISO
five times, and never accepts any caller-supplied host disk device path.
Each boot must observe the real kernel's serial stage and bounds evidence.
"""
from __future__ import annotations
import argparse
import os
import pathlib
import selectors
import subprocess
import tempfile
import time

SECTOR = 512
TOTAL = 128
START = 24
COUNT = 100
GUARD = "IndianOS: R027 isolated ATA block window boundaries PASSED"
STAGES = [
    "IndianOS: R027 disk phase1 formatted and multi-sector file committed",
    "IndianOS: R027 disk phase2 reboot persistence and multi-sector overwrite PASSED",
    "IndianOS: R027 disk phase3 reboot-verified durable overwrite PASSED",
    "IndianOS: R027 disk torn metadata fallback recovered in QEMU",
    "IndianOS: R027 disk phase3 reboot-verified durable overwrite PASSED",
]

def one_boot(qemu: str, iso: pathlib.Path, ovmf: pathlib.Path,
             disk: pathlib.Path, required: str) -> str:
    argv = [
        qemu, "-machine", "pc", "-m", "256M", "-smp", "1",
        "-display", "none", "-monitor", "none", "-serial", "stdio",
        "-no-reboot", "-no-shutdown", "-boot", "d",
        "-drive", f"if=pflash,format=raw,readonly=on,file={ovmf}",
        "-cdrom", str(iso),
        "-drive", f"if=ide,index=0,media=disk,format=raw,file={disk}",
    ]
    process = subprocess.Popen(
        argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT, bufsize=0
    )
    selector = selectors.DefaultSelector()
    output = bytearray()
    try:
        assert process.stdout is not None
        selector.register(process.stdout, selectors.EVENT_READ)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            ready = selector.select(0.5)
            for key, _ in ready:
                chunk = os.read(key.fileobj.fileno(), 8192)
                if chunk:
                    output.extend(chunk)
                    if len(output) > 1024*1024:
                        raise AssertionError("QEMU serial output exceeded 1 MiB")
                else:
                    selector.unregister(key.fileobj)
            text = output.decode("utf-8", "replace")
            if "IndianOS: FATAL" in text:
                raise AssertionError("QEMU guest reported fatal error\n" + text[-5000:])
            if required in text and GUARD in text and text.index(required) < text.index(GUARD):
                return text
            if process.poll() is not None:
                break
        raise AssertionError(
            "QEMU missed required stage " + required + "\n" +
            output[-5000:].decode("utf-8", "replace")
        )
    finally:
        selector.close()
        if process.poll() is None:
            process.terminate()
        try:
            process.communicate(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate(timeout=3)

def validate_window(disk: pathlib.Path) -> bytes:
    data = disk.read_bytes()
    assert len(data) == TOTAL*SECTOR, "temporary disk unexpectedly resized"
    assert data[:START*SECTOR] == bytes(START*SECTOR), "wrote before volume"
    assert data[(START+COUNT)*SECTOR:] == bytes((TOTAL-START-COUNT)*SECTOR), (
        "wrote past bounded volume"
    )
    assert data[START*SECTOR:START*SECTOR+8] == b"INDR027!", (
        "expected R027 metadata header"
    )
    return data

def corrupt_latest_metadata(disk: pathlib.Path) -> None:
    raw = bytearray(validate_window(disk))
    offsets = [START*SECTOR, (START+1)*SECTOR]
    headers = [raw[o:o+SECTOR] for o in offsets]
    assert all(h[:8] == b"INDR027!" for h in headers), "missing mirrored metadata"
    generations = [int.from_bytes(h[16:24], "little") for h in headers]
    assert generations[0] != generations[1], "expected alternating COW generations"
    latest = max(range(2), key=lambda n: generations[n])
    # Corrupt CRC of exactly one metadata sector. Old copy remains intact.
    raw[offsets[latest]+24] ^= 0x5a
    disk.write_bytes(raw)

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--iso", type=pathlib.Path, required=True)
    parser.add_argument("--qemu", required=True)
    parser.add_argument("--ovmf", type=pathlib.Path, required=True)
    args = parser.parse_args()
    iso = args.iso.resolve()
    ovmf = args.ovmf.resolve()
    if not iso.is_file() or not ovmf.is_file():
        parser.error("QEMU ISO and OVMF code file must exist")
    with tempfile.TemporaryDirectory(prefix="indianos-r027-only-disposable-") as temp:
        disk = pathlib.Path(temp)/"r027-disposable.raw"
        disk.write_bytes(bytes(TOTAL*SECTOR))
        for i, marker in enumerate(STAGES):
            if i == 3:
                corrupt_latest_metadata(disk)
            text = one_boot(args.qemu, iso, ovmf, disk, marker)
            validate_window(disk)
            print(f"R027 QEMU boot {i+1}/5 passed: {marker}")
            if "IndianOS: FATAL" in text:
                raise AssertionError("QEMU kernel fatal")
        print("R027 durable filesystem: multi-sector + three reboots + "
              "torn-metadata fallback + subsequent reboot PASSED")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
