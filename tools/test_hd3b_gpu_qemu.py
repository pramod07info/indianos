#!/usr/bin/env python3
"""HD3B.1 QEMU PCI proof for read-only modern virtio-gpu capability parsing.

Runs isolated QEMU/OVMF boots both with and without the virtual accelerator.
This does NOT claim a GPU command queue, framebuffer or device DMA is active.
"""
from __future__ import annotations
import argparse
from pathlib import Path
import sys
from smoke_boot import assert_hardware_inventory, run_smoke

def main()->int:
    parser=argparse.ArgumentParser()
    parser.add_argument("--iso",required=True,type=Path)
    parser.add_argument("--ovmf",required=True,type=Path)
    parser.add_argument("--qemu",default="qemu-system-x86_64")
    args=parser.parse_args()
    virtio_gpu=(
        "-device","pcie-root-port,id=hd3bport,bus=pcie.0,"
                  "chassis=9,slot=9,addr=0x7",
        "-device","virtio-gpu-pci,id=hd3bgpu,bus=hd3bport",
    )
    cases=(
        ("CPU-only",(),"IndianOS HD3B virtio-gpu unavailable/unsupported"),
        ("PCIe virtio-gpu",virtio_gpu,
         "IndianOS HD3B virtio-gpu modern PCI capabilities verified"),
    )
    for name,devices,expected in cases:
        try:
            log=run_smoke(args.iso,args.ovmf,args.qemu,55.0,
                          mode="normal",machine="q35",
                          extra_devices=devices)
            assert_hardware_inventory(log)
            if expected not in log:
                raise AssertionError(
                    f"{name}: native PCI capability probe did not report "
                    f"{expected!r}")
            if ("queue=disabled" not in log or
                "GPU acceleration=not active" not in log or
                "DMA=disabled" not in log or
                "CPU fallback=enabled" not in log):
                raise AssertionError(
                    f"{name}: GPU probing accidentally claimed DMA/activation")
            if devices and ("secondary_GPU=1" not in log or
                            "bridges=" not in log):
                raise AssertionError(
                    f"{name}: downstream PCI bridge GPU missing")
            print(f"IndianOS HD3B.1 {name} read-only GPU probe PASS")
        except (AssertionError,RuntimeError,ValueError,OSError) as exc:
            print(f"IndianOS HD3B.1 {name} QEMU FAILED: {exc}",
                  file=sys.stderr)
            return 1
    return 0

if __name__=="__main__":
    raise SystemExit(main())
