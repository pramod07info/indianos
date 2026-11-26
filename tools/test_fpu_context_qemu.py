#!/usr/bin/env python3
"""Run the opt-in x87/SSE desktop fixture on disposable selected GPT media.

This proves cooperative ring-3 state retention, not multicore or AVX support.
The ordinary normal-desktop image and its full acceptance remain separate.
"""
import argparse
from pathlib import Path
import shutil
import tempfile

from test_normal_desktop_qemu import READY, spawn


def verify_context_proof(output: bytes) -> None:
    ready = output.find(READY.encode())
    if ready < 0:
        raise AssertionError("desktop never reached readiness after context probes")
    for pid in range(1, 5):
        marker = f"IndianOS user: desktop PID{pid} x87 SSE context retained across yields".encode()
        if output.count(marker) != 1 or output.find(marker) >= ready:
            raise AssertionError(f"missing, duplicate or late PID{pid} FPU proof")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("iso", "disk", "ovmf", "artifact-dir"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--qemu", default="qemu-system-x86_64")
    args = parser.parse_args()
    args.artifact_dir.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix="fpu-", dir=args.artifact_dir)).resolve()
    with tempfile.TemporaryDirectory(prefix="fpu-disposable-") as temporary:
        disk = Path(temporary) / "selected.raw"
        shutil.copyfile(args.disk, disk)
        process, sock, session = spawn(args.qemu, args.iso.resolve(), args.ovmf.resolve(),
                                       disk, folder, "fpu-context")
        try:
            session.until(READY, 60)
            verify_context_proof(bytes(session.output))
            session.shot(folder / "fpu-context-ready.ppm")
        finally:
            (folder / "fpu-context.serial.log").write_bytes(session.output)
            if process.poll() is None:
                process.kill()
            process.wait(timeout=10)
            sock.close()
    print("PASS: four distinct ring-3 x87/SSE contexts retained across four yields each")


if __name__ == "__main__":
    main()
