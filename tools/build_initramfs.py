#!/usr/bin/env python3
"""Build a deterministic, read-only IndianOS initramfs v1 archive.

No compression, signatures, privilege escalation or host-disk operations.
Build outputs are explicitly chosen by the invoking developer/Makefile.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import struct

MAGIC = b"INDRAM01"
MAX_FILES = 16
MAX_NAME = 128
MAX_BYTES = 32 * 1024 * 1024


def validate_name(path: str) -> bytes:
    try:
        raw = path.encode("ascii")
    except UnicodeEncodeError as exc:
        raise ValueError("initramfs v1 filenames must be ASCII") from exc
    if len(raw) < 2 or len(raw) > MAX_NAME or not raw.startswith(b"/"):
        raise ValueError("invalid archive absolute path")
    components = raw.split(b"/")[1:]
    for component in components:
        if component in (b"", b".", b".."):
            raise ValueError("unsafe archive path")
        for byte in component:
            if not (65 <= byte <= 90 or 97 <= byte <= 122 or
                    48 <= byte <= 57 or byte in b"-_."):
                raise ValueError("unsafe archive path")
    return raw


def pack(entries: list[tuple[str, bytes]]) -> bytes:
    if not 1 <= len(entries) <= MAX_FILES:
        raise ValueError("invalid initramfs file count")
    payload = bytearray()
    seen: set[bytes] = set()
    for path, data in sorted(entries, key=lambda item: item[0]):
        name = validate_name(path)
        if name in seen:
            raise ValueError("duplicate filename")
        if not isinstance(data, bytes):
            raise ValueError("file content must be bytes")
        seen.add(name)
        payload.extend(struct.pack("<HHI", len(name), 0, len(data)))
        payload.extend(name)
        payload.extend(data)
        if 16 + len(payload) > MAX_BYTES:
            raise ValueError("archive exceeds maximum size")
    return MAGIC + struct.pack("<II", len(entries), 16 + len(payload)) + bytes(payload)


def main() -> None:
    parser = argparse.ArgumentParser(description="Create an IndianOS initramfs v1")
    parser.add_argument("--elf", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--name", default="/bin/init")
    parser.add_argument("--extra-elf", type=Path,
                        help="Optional second ELF (M008 sequential process test)")
    parser.add_argument("--extra-name", default="/bin/worker")
    args = parser.parse_args()
    content = args.elf.read_bytes()
    entries = [(args.name, content)]
    if args.extra_elf is not None:
        entries.append((args.extra_name, args.extra_elf.read_bytes()))
    archive = pack(entries)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(archive)
    print(f"IndianOS read-only initramfs: {len(archive)} bytes")


if __name__ == "__main__":
    main()
