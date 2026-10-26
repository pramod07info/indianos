#!/usr/bin/env python3
"""HD1 QEMU proof: discover a virtual GPU behind a PCIe root port.

No physical hardware, installer writes, cloud traffic or privileged driver
activation. Uses the existing disposable QEMU ISO and private OVMF_VARS copy.
"""
from __future__ import annotations
import argparse
from pathlib import Path
import re
import sys
from smoke_boot import run_smoke, assert_hardware_inventory

def main() -> int:
    cli=argparse.ArgumentParser()
    cli.add_argument("--iso",required=True,type=Path)
    cli.add_argument("--ovmf",required=True,type=Path)
    cli.add_argument("--qemu",default="qemu-system-x86_64")
    args=cli.parse_args()
    virtual_hardware=(
       "-device","pcie-root-port,id=hd1port,bus=pcie.0,"
                 "chassis=8,slot=8,addr=0x6",
       "-device","virtio-gpu-pci,id=hd1gpu,bus=hd1port",
    )
    try:
        text=run_smoke(args.iso,args.ovmf,args.qemu,45.0,
            mode="normal",machine="q35",
            extra_devices=virtual_hardware)
        assert_hardware_inventory(text)
        match=re.search(
            r"IndianOS HD1 PCI buses=(\d+) bridges=(\d+) "
            r"secondary_GPU=(\d+) read-only",text)
        if match is None:
            raise AssertionError("no actual bridge topology boot report")
        buses,bridges,secondary_gpus=map(int,match.groups())
        if buses<2 or bridges<1 or secondary_gpus<1:
            raise AssertionError(
                f"root-port GPU not discovered: buses={buses}, "
                f"bridges={bridges}, secondary GPU={secondary_gpus}")
        if "QPU=unavailable" not in text or "CPU fallback=enabled" not in text:
            raise AssertionError("HD1 no-hardware quantum/CPU fallback lost")
        if "ECAM=not mapped" not in text or "driver=not activated" not in text:
            raise AssertionError("HD1 incorrectly activated untrusted accelerator")
    except (AssertionError,RuntimeError,ValueError,OSError) as exc:
        print(f"IndianOS HD1 bridged virtio GPU QEMU FAILED: {exc}",
              file=sys.stderr)
        return 1
    print(f"IndianOS HD1 bridge GPU QEMU PASS: buses={buses}, "
          f"bridges={bridges}, downstream GPUs={secondary_gpus}")
    return 0

if __name__=="__main__":
    raise SystemExit(main())
