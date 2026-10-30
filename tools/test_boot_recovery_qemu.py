#!/usr/bin/env python3
"""Verify default-kernel init failure recovery using disposable UEFI ISOs.

Only attaches a disposable sentinel drive; its hash must remain unchanged.
The input ISO must be an ordinary valid
IndianOS boot image built without self-test flags; it remains unchanged.
"""
from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import selectors
import struct
import subprocess
import tempfile
import time

from smoke_boot import assert_hardware_inventory, assert_milestones

READY = "IndianOS recovery: read-only serial console ready"
STATUS = "IndianOS recovery: filesystem=offline writes=disabled user-exec=disabled"
HALTED = "IndianOS recovery: halted"
REJECTED = "IndianOS recovery: input rejected"
HELP = "IndianOS recovery: help=status and command list"
UNAVAILABLE = "IndianOS recovery: reason=init-unavailable-or-invalid"
STAGING = "IndianOS recovery: reason=init-staging-failed"


def archive(name: bytes, content: bytes) -> bytes:
    entry = struct.pack("<HHI", len(name), 0, len(content)) + name + content
    return b"INDRAM01" + struct.pack("<II", 1, 16 + len(entry)) + entry


def without_module(config: str) -> str:
    return "\n".join(line for line in config.splitlines()
                     if not line.strip().startswith(("module_path:", "module_string:"))) + "\n"


def verify_trace(text: str, reason: str) -> None:
    """Require actual input-dependent responses, not a ready banner alone."""
    expected = [reason, READY, REJECTED, REJECTED,
                "IndianOS recovery: unsupported command", reason, STATUS,
                HELP, HALTED]
    at = 0
    for marker in expected:
        found = text.find(marker, at)
        if found < 0:
            raise AssertionError(f"missing/out-of-order recovery response: {marker}")
        at = found + len(marker)
    if text.count(REJECTED) != 2 or text.count(STATUS) != 1 or text.count(HALTED) != 1:
        raise AssertionError("invalid or overlong input executed a recovery command")
    for forbidden in ("entering ring-3", "PID 1 RUNNING", "R027 blank disk",
                      "R031.1 boot service starting"):
        if forbidden in text:
            raise AssertionError(f"recovery started an unsafe boot route: {forbidden}")


class Guest:
    def __init__(self, iso: Path, qemu: str, ovmf: Path, timeout: float, guard: Path):
        self.process = subprocess.Popen([
            qemu, "-machine", "q35", "-m", "256M", "-display", "none",
            "-drive", f"if=pflash,unit=0,format=raw,file={ovmf},readonly=on",
            "-cdrom", str(iso), "-boot", "d", "-serial", "stdio", "-monitor", "none",
            "-nic", "none", "-no-reboot",
            "-drive", f"if=ide,index=0,format=raw,file={guard}",
        ], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ, "stdout")
        self.selector.register(self.process.stderr, selectors.EVENT_READ, "stderr")
        self.output = bytearray()
        self.errors = bytearray()
        self.deadline = time.monotonic() + timeout

    @property
    def text(self) -> str:
        return self.output.decode("utf-8", errors="replace")

    def until(self, marker: str) -> None:
        while marker not in self.text:
            if time.monotonic() >= self.deadline:
                raise AssertionError(f"QEMU timeout waiting for {marker}")
            events = self.selector.select(timeout=0.1)
            for key, _ in events:
                data = os.read(key.fileobj.fileno(), 4096)
                if not data:
                    self.selector.unregister(key.fileobj)
                    continue
                if key.data == "stdout":
                    self.output.extend(data)
                    if len(self.output) > 1024 * 1024:
                        raise AssertionError("unexpected excessive guest output")
                else:
                    self.errors.extend(data)
                    del self.errors[:-8192]
            if not self.selector.get_map() and self.process.poll() is not None:
                raise AssertionError(f"QEMU exited {self.process.returncode} before {marker}")

    def send(self, data: bytes) -> None:
        self.process.stdin.write(data)
        self.process.stdin.flush()

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=5)
        self.selector.close()
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            stream.close()


def boot(iso: Path, qemu: str, ovmf: Path, timeout: float, reason: str | None, guard: Path) -> None:
    before = hashlib.sha256(guard.read_bytes()).digest()
    guest = Guest(iso, qemu, ovmf, timeout, guard)
    try:
        if reason is None:
            guest.until("IndianOS: initramfs /bin/init staged and released safely")
            assert_milestones(guest.text)
            assert_hardware_inventory(guest.text)
            if READY in guest.text:
                raise AssertionError("valid normal boot entered recovery")
            return
        guest.until(READY)
        # A complete bad line is rejected atomically, even with a command suffix.
        guest.send(b" " * 33 + b"halt\n")
        guest.until(REJECTED)
        # Control bytes poison the whole line despite later editing characters.
        guest.send(b"\x1b\x08halt\n")
        # Unsupported commands cannot activate disk, process or reset paths.
        guest.send(b"reboot\nstatus\r\nhelp\nhalt\n")
        guest.until(HALTED)
        verify_trace(guest.text, reason)
    except Exception as error:
        raise AssertionError(f"{error}\n--- serial ---\n{guest.text[-65536:]}"
                             f"\n--- stderr ---\n{guest.errors.decode(errors='replace')}") from error
    finally:
        guest.close()
        if hashlib.sha256(guard.read_bytes()).digest() != before:
            raise AssertionError("boot/recovery modified the disposable sentinel drive")


def checked_run(command: list[str]) -> None:
    result = subprocess.run(command, capture_output=True, timeout=60)
    if result.returncode:
        raise RuntimeError(f"command failed: {command!r}\n"
                           f"{result.stderr.decode(errors='replace')[-8192:]}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iso", required=True, type=Path)
    parser.add_argument("--qemu", default="qemu-system-x86_64")
    parser.add_argument("--ovmf", required=True, type=Path)
    parser.add_argument("--xorriso", default="xorriso")
    parser.add_argument("--timeout", type=float, default=35)
    args = parser.parse_args()
    if not args.iso.is_file() or not args.ovmf.is_file() or not 0 < args.timeout <= 180:
        parser.error("existing ISO/OVMF and timeout in (0, 180] required")
    iso = args.iso.resolve()
    ovmf = args.ovmf.resolve()
    with tempfile.TemporaryDirectory(prefix="indianos-boot-recovery-") as directory:
        root = Path(directory)
        guard = root / "sentinel.img"
        guard.write_bytes(bytes(range(256)) * 256)
        boot(iso, args.qemu, ovmf, args.timeout, None, guard)
        print("Boot recovery normal control PASS (sentinel unchanged)", flush=True)
        original_config = root / "original-limine.conf"
        checked_run([args.xorriso, "-osirrox", "on", "-indev", str(iso),
                     "-extract", "/boot/limine/limine.conf", str(original_config)])
        # ISO extraction preserves a read-only mode. Write a separate fixture
        # instead of trying to modify the extracted boot configuration.
        config = root / "limine.conf"
        config.write_text(without_module(original_config.read_text()))
        tree = root / "iso-tree"
        checked_run([args.xorriso, "-osirrox", "on", "-indev", str(iso),
                     "-extract", "/", str(tree)])
        for directory in [tree, *(p for p in tree.rglob("*") if p.is_dir())]:
            directory.chmod(directory.stat().st_mode | 0o700)
        cases = (
            ("missing-module", config, "/boot/limine/limine.conf", UNAVAILABLE),
            ("missing-init", archive(b"/bin/other", b"unused"), "/boot/initramfs.img", UNAVAILABLE),
            ("corrupt-archive", b"BROKEN!!" + archive(b"/bin/init", b"unused")[8:],
             "/boot/initramfs.img", UNAVAILABLE),
            ("invalid-init", archive(b"/bin/init", b"not an ELF image"),
             "/boot/initramfs.img", STAGING),
        )
        for name, payload, target, reason in cases:
            source = payload if isinstance(payload, Path) else root / f"{name}.img"
            if isinstance(payload, bytes):
                source.write_bytes(payload)
            modified = root / f"{name}.iso"
            destination = tree / target.lstrip("/")
            original = destination.read_bytes()
            destination.chmod(destination.stat().st_mode | 0o200)
            try:
                destination.write_bytes(source.read_bytes())
                # Rebuild the disposable El Torito image. Replaying boot
                # metadata after Limine's BIOS MBR installation is ambiguous
                # to xorriso; these fresh images are tested only with OVMF.
                checked_run([args.xorriso, "-as", "mkisofs", "-R", "-r", "-J",
                             "-b", "boot/limine/limine-bios-cd.bin", "-no-emul-boot",
                             "-boot-load-size", "4", "-boot-info-table", "--efi-boot",
                             "boot/limine/limine-uefi-cd.bin", "-efi-boot-part",
                             "--efi-boot-image", "--protective-msdos-label",
                             str(tree), "-o", str(modified)])
            finally:
                destination.write_bytes(original)
            boot(modified, args.qemu, ovmf, args.timeout, reason, guard)
            print(f"Boot recovery {name} PASS (sentinel unchanged)", flush=True)
    print("IndianOS read-only boot recovery QEMU PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
