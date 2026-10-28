#!/usr/bin/env python3
"""Archive genuine QEMU P6 framebuffer screendumps as lossless RGB PNG images.

Never fabricates a screen or uses an assistant rendering. Fails if expected
QEMU screenshot stages are absent or the framebuffer data is malformed.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path
import struct
import zlib


def ppm_to_png(raw: bytes) -> tuple[bytes, int, int, str]:
    cursor = 0

    def token() -> bytes:
        nonlocal cursor
        while cursor < len(raw):
            if raw[cursor] == 35:  # PPM comment
                end = raw.find(b"\n", cursor)
                if end < 0:
                    raise ValueError("unterminated PPM comment")
                cursor = end + 1
            elif raw[cursor] in b" \t\r\n":
                cursor += 1
            else:
                break
        start = cursor
        while cursor < len(raw) and raw[cursor] not in b" \t\r\n#":
            cursor += 1
        if start == cursor:
            raise ValueError("missing PPM header token")
        return raw[start:cursor]

    if token() != b"P6":
        raise ValueError("expected actual QEMU binary P6 framebuffer screenshot")
    width, height, max_value = map(int, (token(), token(), token()))
    if not (640 <= width <= 8192 and 480 <= height <= 8192 and max_value == 255):
        raise ValueError(f"unexpected framebuffer geometry {width}x{height}/{max_value}")
    if cursor >= len(raw) or raw[cursor] not in b" \t\r\n":
        raise ValueError("no separator following PPM maxval")
    cursor += 1
    pixels = raw[cursor:]
    if len(pixels) != width * height * 3:
        raise ValueError("PPM pixel byte count does not match expected geometry")

    def chunk(kind: bytes, data: bytes) -> bytes:
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data) & 0xffffffff))

    stride = width * 3
    scanlines = b"".join(b"\0" + pixels[y * stride:(y + 1) * stride]
                         for y in range(height))
    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(scanlines, 9))
           + chunk(b"IEND", b""))
    return png, width, height, hashlib.sha256(pixels).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--workflow-sha", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--run-url", required=True)
    args = parser.parse_args()
    files = sorted(args.input.glob("*.ppm"))
    required = {"00-kernel-owned-desktop.ppm", "01-user-surface.ppm",
                "02-user-surface.ppm", "03-user-surface.ppm"}
    if not required.issubset({f.name for f in files}):
        raise SystemExit("R029 test did not capture all four real QEMU screenshot stages")
    args.output.mkdir(parents=True, exist_ok=True)
    frames = []
    for source in files:
        png, width, height, pixels_sha256 = ppm_to_png(source.read_bytes())
        target = args.output / (source.stem + ".png")
        target.write_bytes(png)
        frames.append({"file": target.name, "width": width, "height": height,
                       "pixel_sha256": pixels_sha256})
    if len({(f["width"], f["height"]) for f in frames}) != 1:
        raise SystemExit("QEMU framebuffer geometry changed during capture")
    metadata = {
        "version": args.version,
        "source_sha": args.source_sha,
        "workflow_sha": args.workflow_sha,
        "run_url": args.run_url,
        "captured_at_utc": datetime.now(timezone.utc).isoformat(),
        "capture": "real QEMU/OVMF VGA screendump; opt-in Orbit preview",
        "frames": frames,
        "limitations": "Not proof of default desktop boot, hardware GPU, or production readiness",
    }
    (args.output / "metadata.json").write_text(
        json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    print("Captured", len(frames), "verified QEMU screenshots for", args.version,
          args.source_sha)


if __name__ == "__main__":
    main()
