#!/usr/bin/env python3
"""R015 destructive ATA PIO integration test: temporary raw QEMU disk only."""
import argparse
import pathlib
import subprocess
import tempfile

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--iso", type=pathlib.Path, required=True)
    parser.add_argument("--qemu", required=True)
    parser.add_argument("--ovmf", type=pathlib.Path, required=True)
    args = parser.parse_args()
    if not args.iso.is_file() or not args.ovmf.is_file():
        parser.error("ISO and OVMF firmware must exist")
    with tempfile.TemporaryDirectory(prefix="indianos-r015-ata-") as tmp:
        disk = pathlib.Path(tmp) / "disposable.raw"
        disk.write_bytes(bytes(512 * 32))
        command = [
            args.qemu, "-machine", "pc", "-m", "256M", "-smp", "1",
            "-display", "none", "-monitor", "none", "-serial", "stdio",
            "-no-reboot", "-boot", "d",
            "-drive", f"if=pflash,format=raw,readonly=on,file={args.ovmf}",
            "-cdrom", str(args.iso.resolve()),
            "-drive", f"if=ide,index=0,media=disk,format=raw,file={disk}",
        ]
        try:
            result = subprocess.run(command, capture_output=True, timeout=35, check=False)
            output = (result.stdout + result.stderr).decode("utf-8", errors="replace")
        except subprocess.TimeoutExpired as exc:
            output = ((exc.stdout or b"") + (exc.stderr or b"")).decode(
                "utf-8", errors="replace")
        required = [
            "IndianOS: R015 ATA disk detected",
            "IndianOS: R015 ATA disk read write restore PASSED",
        ]
        if any(marker not in output for marker in required) or "FATAL" in output:
            raise SystemExit("R015 ATA QEMU smoke FAILED:\n" + output[-6000:])
        if disk.read_bytes() != bytes(512 * 32):
            raise SystemExit("R015 ATA smoke FAILED: temporary disk was not restored")
        print("R015 ATA QEMU read/write/readback/bounds/restore PASSED")

if __name__ == "__main__":
    main()
