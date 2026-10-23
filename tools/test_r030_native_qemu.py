#!/usr/bin/env python3
"""QEMU boot the exact ELF from an R030 signed verified native installation.

Only temporary trust keys, temp install root, disposable build output and
QEMU in-memory guest are used. No host package installation or network.
"""
from __future__ import annotations
import argparse
import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile

from build_initramfs import pack
from r030_package_tool import active


def run(*argv: str, timeout=900) -> None:
    print("R030 command:", " ".join(argv), flush=True)
    subprocess.run(argv, check=True, timeout=timeout)


def qemu_boot(root: Path, app: str, build: Path, ovmf: Path, qemu: str, phase: str) -> None:
    verified = active(root, app)
    payload = verified.read_bytes()
    original = hashlib.sha256(payload).hexdigest()
    initramfs = build / "initramfs.img"
    initramfs.write_bytes(pack([("/bin/init", payload)]))
    # ISO rebuilt with the newly installed package payload, not source ELF.
    run("make", "image", f"BUILD={build}", "RUST_CORE=1", "USERMODE_SELFTEST=1", "R030_NATIVE_TEST=1")
    iso = build / "indianos.iso"
    with tempfile.TemporaryDirectory(prefix="indianos-r030-iso-") as directory:
        dumped = Path(directory) / "actual-initramfs.img"
        run("xorriso", "-osirrox", "on", "-indev", str(iso),
            "-extract", "/boot/initramfs.img", str(dumped))
        if dumped.read_bytes() != pack([("/bin/init", payload)]):
            raise AssertionError("QEMU ISO does not contain installed signed native ELF")
    run(sys.executable, "tools/smoke_boot.py", "--iso", str(iso),
        "--qemu", qemu, "--ovmf", str(ovmf), "--mode", "r030",
        "--timeout", "50", timeout=80)
    if hashlib.sha256(verified.read_bytes()).hexdigest() != original:
        raise AssertionError("host package bytes unexpectedly mutated during launch")
    print(f"R030 {phase} signed verified native ELF launched in QEMU ring-3 PASSED", flush=True)


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--qemu", required=True)
    p.add_argument("--ovmf", type=Path, required=True)
    args = p.parse_args()
    if not args.ovmf.is_file():
        p.error("OVMF code missing")
    build = Path("build/r030-native")
    run("make", "image", f"BUILD={build}", "RUST_CORE=1", "USERMODE_SELFTEST=1", "R030_NATIVE_TEST=1")
    original = (build / "userland/demo.elf").read_bytes()
    with tempfile.TemporaryDirectory(prefix="indianos-r030-test-root-") as tmp:
        directory = Path(tmp)
        root = directory / "explicit-install-root"
        root.mkdir()
        private = directory / "temporary-private.pem"
        public = directory / "trusted-public.pem"
        run("openssl", "genpkey", "-algorithm", "ed25519", "-out", str(private))
        run("openssl", "pkey", "-in", str(private), "-pubout", "-out", str(public))
        native = directory / "native.elf"
        native.write_bytes(original)
        pkg1 = directory / "native-1.0.0.ipkg"
        pkg2 = directory / "native-1.0.1.ipkg"
        tool = [sys.executable, "tools/r030_package_tool.py"]
        run(*tool, "build", "--elf", str(native), "--output", str(pkg1),
            "--private-key", str(private), "--name", "native-demo", "--version", "1.0.0")
        run(*tool, "verify", "--package", str(pkg1), "--trust-key", str(public))
        run(*tool, "install", "--package", str(pkg1), "--trust-key", str(public), "--root", str(root))
        qemu_boot(root, "native-demo", build, args.ovmf, args.qemu, "initial install")
        # A new signed version appends nonloaded metadata. ELF PT_LOAD bits
        # stay identical, but signature and installed archive differ.
        native.write_bytes(original + b"R030-UPDATE-METADATA")
        run(*tool, "build", "--elf", str(native), "--output", str(pkg2),
            "--private-key", str(private), "--name", "native-demo", "--version", "1.0.1")
        run(*tool, "install", "--package", str(pkg2), "--trust-key", str(public), "--root", str(root))
        qemu_boot(root, "native-demo", build, args.ovmf, args.qemu, "signed update")
        run(*tool, "rollback", "--root", str(root), "--name", "native-demo")
        if active(root, "native-demo").read_bytes() != original:
            raise AssertionError("rollback did not restore exact installed binary")
        qemu_boot(root, "native-demo", build, args.ovmf, args.qemu, "signed rollback")
        run(*tool, "remove", "--root", str(root), "--name", "native-demo")
        try:
            active(root, "native-demo")
        except ValueError:
            pass
        else:
            raise AssertionError("remove left native program launchable")
        print("R030 signed native bundle: verify/install/update/rollback/remove and 3 independent QEMU ring3 launches PASSED")


if __name__ == "__main__":
    main()
