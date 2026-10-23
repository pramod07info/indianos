#!/usr/bin/env python3
"""R030 QEMU acceptance: boot a signature-verified native install, update,
rollback and refuse tampering before every guest execution.

Creates test-only Ed25519 keys and files strictly in a TemporaryDirectory;
nothing touches an installed host application or actual host disk.
"""
from __future__ import annotations
import argparse
from pathlib import Path
import os
import subprocess
import sys
import tempfile
import time
import selectors

sys.path.insert(0, str(Path(__file__).resolve().parent))
from r030_pkg import PackageError, build, verify, run_action


def call(argv: list[str], timeout: int = 180) -> None:
    subprocess.run(argv, check=True, timeout=timeout)


def boot(qemu: str, iso: Path, ovmf: Path, expected: str) -> None:
    cmd = [
        qemu, "-machine", "pc", "-m", "256M", "-smp", "1",
        "-display", "none", "-monitor", "none", "-serial", "stdio",
        "-no-reboot", "-no-shutdown", "-boot", "d",
        "-drive", f"if=pflash,format=raw,readonly=on,file={ovmf}",
        "-cdrom", str(iso)
    ]
    process = subprocess.Popen(cmd, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                               bufsize=0)
    selector = selectors.DefaultSelector()
    output = bytearray()
    try:
        assert process.stdout is not None
        selector.register(process.stdout, selectors.EVENT_READ)
        deadline = time.monotonic() + 35
        while time.monotonic() < deadline:
            for key, _ in selector.select(0.25):
                chunk = os.read(key.fileobj.fileno(), 8192)
                if chunk:
                    output.extend(chunk)
                else:
                    selector.unregister(key.fileobj)
            text = output.decode("utf8", "replace")
            if "IndianOS: FATAL" in text:
                raise AssertionError("QEMU kernel fatal:\n" + text[-5000:])
            markers = (
                "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
                f"IndianOS user: {expected}",
                "IndianOS: ring-3 user program experiment complete"
            )
            if all(m in text for m in markers):
                pos = 0
                for marker in markers:
                    at = text.find(marker, pos)
                    if at < 0:
                        raise AssertionError("out-of-order QEMU guest serial marker")
                    pos = at + len(marker)
                return
            if process.poll() is not None:
                break
        raise AssertionError("R030 signed package failed to boot in QEMU:\n"
                             + output[-5000:].decode("utf8", "replace"))
    finally:
        selector.close()
        if process.poll() is None:
            process.terminate()
        try:
            process.communicate(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate(timeout=3)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--qemu", required=True)
    parser.add_argument("--ovmf", required=True, type=Path)
    args = parser.parse_args()
    if not args.ovmf.is_file():
        parser.error("OVMF firmware file missing")
    with tempfile.TemporaryDirectory(prefix="indianos-r030-sdk-only-") as temp:
        base = Path(temp)
        private, public = base / "dev-private.pem", base / "dev-public.pem"
        call(["openssl", "genpkey", "-algorithm", "Ed25519", "-out", str(private)])
        call(["openssl", "pkey", "-in", str(private), "-pubout", "-out", str(public)])
        # Two real native apps use IndianOS's existing Rust x86_64 freestanding
        # SDK linker/ELF64 process ABI, not Linux binaries or synthetic fixtures.
        for n in (1, 2):
            call(["make", "user-elf", f"BUILD=build/r030-v{n}",
                  f"R030_PACKAGE_V{n}=1"])
        elf1 = Path("build/r030-v1/userland/demo.elf").read_bytes()
        elf2 = Path("build/r030-v2/userland/demo.elf").read_bytes()
        if elf1 == elf2:
            raise AssertionError("R030 v1/v2 app ELFs unexpectedly identical")
        first, second = base / "v1.indpkg", base / "v2.indpkg"
        first.write_bytes(build(elf1, "hello", "1.0.0", private))
        second.write_bytes(build(elf2, "hello", "1.1.0", private))
        store = base / "staging"
        verified = base / "verified-init.elf"
        # Exercise signed install -> run, signed update -> run, rollback -> run.
        for action, source, expected, serial in [
            ("install", first, elf1, "R030 signed native package version 1 executed in ring-3"),
            ("update", second, elf2, "R030 signed native package version 2 executed in ring-3"),
            ("rollback", None, elf1, "R030 signed native package version 1 executed in ring-3")
        ]:
            run_action(action, store, "hello", source, public, None)
            run_action("export", store, "hello", None, public, verified)
            if verified.read_bytes() != expected:
                raise AssertionError("exported ELF bytes differ from trusted signed package")
            # Start with the normal R030 opt-in kernel and replace ONLY its
            # ephemeral initramfs image with the verified native ET_EXEC.
            builddir = Path("build/r030-qemu")
            call(["make", "image", f"BUILD={builddir}", "R030_PACKAGE_V1=1"], 360)
            call(["python3", "tools/build_initramfs.py", "--elf", str(verified),
                  "--output", str(builddir/"initramfs.img")])
            # The base ISO has been built once using the verified kernel.
            # Rebuild its disposable UEFI media directly so the test never
            # invokes Limine's BIOS installer on a second image invocation.
            # This replaces ONLY the initramfs with a signature-verified ELF.
            iso_root = builddir / "iso_root"
            image_payload = iso_root / "boot" / "initramfs.img"
            image_payload.write_bytes((builddir / "initramfs.img").read_bytes())
            iso = builddir / "indianos.iso"
            iso.unlink(missing_ok=True)
            call(["xorriso", "-as", "mkisofs", "-R", "-r", "-J",
                  "-b", "boot/limine/limine-bios-cd.bin",
                  "-no-emul-boot", "-boot-load-size", "4",
                  "-boot-info-table", "--efi-boot",
                  "boot/limine/limine-uefi-cd.bin", "-efi-boot-part",
                  "--efi-boot-image", "--protective-msdos-label",
                  str(iso_root), "-o", str(iso)], 180)
            boot(args.qemu, iso, args.ovmf, serial)
            print(f"R030 QEMU signed {action}: {serial} PASSED")
        # Tampering must not replace a previously verified installation.
        damaged = base / "tampered.indpkg"
        raw = bytearray(second.read_bytes())
        raw[-30] ^= 0x4e
        damaged.write_bytes(raw)
        try:
            run_action("update", store, "hello", damaged, public, None)
        except PackageError:
            pass
        else:
            raise AssertionError("tampered signed package accepted")
        run_action("export", store, "hello", None, public, verified)
        if verified.read_bytes() != elf1:
            raise AssertionError("rejected update unexpectedly modified current app")
        run_action("remove", store, "hello", None, public, None)
        if (store/"apps"/"hello").exists():
            raise AssertionError("application removal incomplete")
        print("R030 signed install, update, rollback, QEMU execution, "
              "tamper rejection and remove PASSED")


if __name__ == "__main__":
    main()
