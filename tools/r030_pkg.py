#!/usr/bin/env python3
"""IndianOS R030 native application package manager (host-side SDK).

Developer-controlled staging roots only. Signed packages are verified
with OpenSSL Ed25519 using an explicitly configured trusted public key.
This is NOT an on-device package daemon, privileged installer, or a
replacement for the kernel's ELF page/permission validation.
"""
from __future__ import annotations

import argparse
import contextlib
import fcntl
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zipfile

MAGIC = b"INDPKG01"
TARGET = "indianos-x86_64-elf-v1"
MAX_ELF = 2 * 1024 * 1024
MAX_PACKAGE = MAX_ELF + 8192
MAX_NAME = 36
MAX_MANIFEST = 1024
PARTS = {"manifest.json", "app.elf", "signature.ed25519"}
NAME = re.compile(r"[a-z][a-z0-9-]{0,35}\Z", re.ASCII)
VERSION = re.compile(r"(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})\Z", re.ASCII)


class PackageError(ValueError):
    pass


def fail(msg: str) -> None:
    raise PackageError(msg)


def canonical(value: dict) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def check_elf(elf: bytes) -> None:
    if len(elf) < 64 or len(elf) > MAX_ELF:
        fail("invalid native ELF length")
    if (elf[:7] != b"\x7fELF\x02\x01\x01" or
        elf[7:16] != bytes(9) or
        struct.unpack_from("<HHI", elf, 16) != (2, 62, 1)):
        fail("only x86-64 little-endian static ET_EXEC ELF64 is accepted")
    phoff = struct.unpack_from("<Q", elf, 32)[0]
    phentsize, count = struct.unpack_from("<HH", elf, 54)
    if phentsize != 56 or not 1 <= count <= 32:
        fail("invalid ELF program headers")
    if phoff < 64 or phoff + count * phentsize > len(elf):
        fail("out-of-bounds ELF program headers")
    loaded = False
    for offset in range(phoff, phoff + count * phentsize, phentsize):
        kind, flags, foff, _, _, filesz, memsz, _ = struct.unpack_from("<IIQQQQQQ", elf, offset)
        if kind == 3:
            fail("dynamic interpreter is unsupported")
        if kind == 1:
            loaded = True
            if filesz > memsz or foff > len(elf) or filesz > len(elf) - foff:
                fail("invalid PT_LOAD ranges")
            if flags & 2 and flags & 1:
                fail("writable and executable PT_LOAD forbidden")
    if not loaded:
        fail("ELF contains no loadable program segments")


def validate_manifest(m: dict, elf: bytes) -> None:
    if not isinstance(m, dict) or set(m) != {
        "format", "name", "version", "target", "entrypoint", "sha256", "size"
    }:
        fail("unknown or missing package manifest fields")
    if m["format"] != 1 or m["target"] != TARGET or m["entrypoint"] != "/bin/init":
        fail("unsupported package format, target, or entrypoint")
    if not isinstance(m["name"], str) or not NAME.fullmatch(m["name"]):
        fail("invalid application identifier")
    if not isinstance(m["version"], str) or not VERSION.fullmatch(m["version"]):
        fail("invalid semver version")
    if type(m["size"]) is not int or m["size"] != len(elf):
        fail("application payload length mismatch")
    if m["sha256"] != hashlib.sha256(elf).hexdigest():
        fail("application SHA256 mismatch")
    check_elf(elf)


def openssl_signature(message: bytes, key: Path, signature: bytes | None) -> bytes:
    if not key.is_file() or key.is_symlink():
        fail("trusted key is missing or symlinked")
    with tempfile.TemporaryDirectory(prefix="indianos-ed25519-") as temp:
        directory = Path(temp)
        payload = directory / "payload.bin"
        payload.write_bytes(message)
        cmd = ["openssl", "pkeyutl"]
        if signature is None:
            cmd.extend(["-sign", "-inkey", str(key)])
        else:
            sigfile = directory / "signature.bin"
            sigfile.write_bytes(signature)
            cmd.extend(["-verify", "-pubin", "-inkey", str(key),
                        "-sigfile", str(sigfile)])
        cmd.extend(["-rawin", "-in", str(payload)])
        result = subprocess.run(cmd, capture_output=True, check=False)
        if result.returncode:
            fail("Ed25519 signing/verifying failed: invalid signature or key")
        return result.stdout


def signed_payload(manifest: bytes, elf: bytes) -> bytes:
    return MAGIC + struct.pack("<I", len(manifest)) + manifest + elf


def build(elf: bytes, name: str, version: str, private: Path) -> bytes:
    check_elf(elf)
    manifest = {
        "format": 1, "name": name, "version": version,
        "target": TARGET, "entrypoint": "/bin/init",
        "size": len(elf), "sha256": hashlib.sha256(elf).hexdigest()
    }
    validate_manifest(manifest, elf)
    metadata = canonical(manifest)
    signature = openssl_signature(signed_payload(metadata, elf), private, None)
    if len(signature) != 64:
        fail("unexpected Ed25519 signature size")
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as archive:
        for filename, content in [
            ("manifest.json", metadata), ("app.elf", elf),
            ("signature.ed25519", signature)
        ]:
            entry = zipfile.ZipInfo(filename, (1980, 1, 1, 0, 0, 0))
            entry.compress_type = zipfile.ZIP_STORED
            entry.external_attr = 0o600 << 16
            archive.writestr(entry, content)
    if len(buf.getvalue()) > MAX_PACKAGE:
        fail("package exceeds bounded size")
    return buf.getvalue()


def verify(raw: bytes, trusted_public: Path) -> tuple[dict, bytes]:
    if len(raw) > MAX_PACKAGE or len(raw) < 100:
        fail("invalid package archive size")
    try:
        with zipfile.ZipFile(io.BytesIO(raw)) as archive:
            infos = archive.infolist()
            if len(infos) != len(PARTS) or {p.filename for p in infos} != PARTS:
                fail("package must have exactly three distinct flat members")
            for entry in infos:
                if (entry.compress_type != zipfile.ZIP_STORED or
                    entry.is_dir() or entry.file_size > MAX_ELF or
                    (entry.external_attr >> 16) & 0o170000 == 0o120000):
                    fail("compressed, symlink or oversized package member")
            metadata = archive.read("manifest.json")
            elf = archive.read("app.elf")
            signature = archive.read("signature.ed25519")
    except (zipfile.BadZipFile, EOFError, OSError) as err:
        raise PackageError("malformed ZIP package") from err
    if len(metadata) > MAX_MANIFEST or len(signature) != 64:
        fail("manifest or signature has invalid size")
    try:
        manifest = json.loads(metadata.decode("ascii"))
    except (ValueError, UnicodeError) as err:
        raise PackageError("invalid manifest encoding") from err
    if canonical(manifest) != metadata:
        fail("manifest must be canonical and deterministic")
    validate_manifest(manifest, elf)
    openssl_signature(signed_payload(metadata, elf), trusted_public, signature)
    return manifest, elf


def version_tuple(version: str) -> tuple[int, int, int]:
    if not VERSION.fullmatch(version):
        fail("invalid semver version")
    return tuple(int(part) for part in version.split("."))


def guarded(path: Path) -> Path:
    # Refuse symlink redirection within the selected SDK staging root.
    absolute = path.absolute()
    for segment in [absolute, *absolute.parents]:
        if segment.is_symlink():
            fail("symlinked installation root or component forbidden")
    return absolute


@contextlib.contextmanager
def locked(root: Path):
    root = guarded(root)
    root.mkdir(parents=True, exist_ok=True)
    apps = root / "apps"
    if apps.is_symlink():
        fail("symlinked application directory forbidden")
    apps.mkdir(exist_ok=True)
    fd = os.open(root / ".lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        yield apps
    finally:
        fcntl.flock(fd, fcntl.LOCK_UN)
        os.close(fd)


def read_checked(path: Path, pubkey: Path) -> tuple[dict, bytes]:
    if path.is_symlink() or not path.is_file():
        fail("package not found or symlink")
    if path.stat().st_size > MAX_PACKAGE:
        fail("oversized package")
    return verify(path.read_bytes(), pubkey)


def atomic_write(path: Path, data: bytes) -> None:
    if path.exists() and path.is_symlink():
        fail("refusing symlinked output")
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".r030-", delete=False) as tmp:
        tmp.write(data)
        tmp.flush()
        os.fsync(tmp.fileno())
        temp = Path(tmp.name)
    try:
        os.replace(temp, path)
        dfd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(dfd)
        finally:
            os.close(dfd)
    finally:
        temp.unlink(missing_ok=True)


def run_action(action: str, root: Path, name: str, source: Path | None,
               key: Path, output: Path | None) -> None:
    if not NAME.fullmatch(name):
        fail("invalid application name")
    with locked(root) as apps:
        appdir = apps / name
        if appdir.is_symlink():
            fail("symlinked app directory forbidden")
        appdir.mkdir(exist_ok=True)
        current = appdir / "current.pkg"
        previous = appdir / "previous.pkg"
        if action in ("install", "update"):
            if source is None:
                fail("missing signed package")
            metadata, _ = read_checked(source, key)
            if metadata["name"] != name:
                fail("signed package identity mismatch")
            exists = current.exists()
            if (action == "install" and exists) or (action == "update" and not exists):
                fail("invalid package install/update state")
            new_bytes = source.read_bytes()
            if exists:
                old, _ = read_checked(current, key)
                if old["name"] != name or version_tuple(metadata["version"]) <= version_tuple(old["version"]):
                    fail("unsigned, substituted or non-upgrade version")
                atomic_write(previous, current.read_bytes())
            atomic_write(current, new_bytes)
            print(f"R030 {action} {name} {metadata['version']} VERIFIED")
        elif action == "rollback":
            if not current.exists() or not previous.exists():
                fail("no current and previous versions for rollback")
            old, _ = read_checked(previous, key)
            now, _ = read_checked(current, key)
            if old["name"] != name or now["name"] != name or old["version"] == now["version"]:
                fail("invalid prior revision")
            # Write both destinations independently and atomically;
            # current always exists, even on unexpected interruption.
            before = current.read_bytes()
            atomic_write(current, previous.read_bytes())
            atomic_write(previous, before)
            print(f"R030 rollback {name} to {old['version']} VERIFIED")
        elif action == "remove":
            if not current.exists():
                fail("package not installed")
            read_checked(current, key)
            if previous.exists():
                read_checked(previous, key)
            current.unlink()
            previous.unlink(missing_ok=True)
            appdir.rmdir()
            print(f"R030 remove {name} VERIFIED")
        elif action == "export":
            if not current.exists() or output is None:
                fail("missing installed package or export output")
            m, elf = read_checked(current, key)
            guarded(output.parent).mkdir(parents=True, exist_ok=True)
            atomic_write(output, elf)
            print(f"R030 export {name} {m['version']} signed ELF VERIFIED")
        elif action == "status":
            if not current.exists():
                fail("package not installed")
            m, _ = read_checked(current, key)
            print(f"R030 status {name} version={m['version']} signature=VERIFIED")
        else:
            fail("unknown package command")


def main() -> int:
    parser = argparse.ArgumentParser(description="R030 verified native package staging tool")
    commands = parser.add_subparsers(dest="action", required=True)
    pkg = commands.add_parser("build", help="sign a freestanding ELF64")
    pkg.add_argument("--elf", type=Path, required=True)
    pkg.add_argument("--name", required=True)
    pkg.add_argument("--version", required=True)
    pkg.add_argument("--private-key", type=Path, required=True)
    pkg.add_argument("--output", type=Path, required=True)
    test = commands.add_parser("verify", help="verify signature, ELF and target")
    test.add_argument("--package", type=Path, required=True)
    test.add_argument("--public-key", type=Path, required=True)
    for action in ("install", "update", "rollback", "remove", "export", "status"):
        sub = commands.add_parser(action)
        sub.add_argument("--root", type=Path, required=True)
        sub.add_argument("--name", required=True)
        sub.add_argument("--public-key", type=Path, required=True)
        if action in ("install", "update"):
            sub.add_argument("--package", type=Path, required=True)
        if action == "export":
            sub.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.action == "build":
            elf = args.elf.read_bytes()
            out = build(elf, args.name, args.version, args.private_key)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            atomic_write(args.output, out)
            print(f"R030 built deterministic signed {args.name} {args.version}")
        elif args.action == "verify":
            m, _ = read_checked(args.package, args.public_key)
            print(f"R030 signature VERIFIED {m['name']} {m['version']} {m['target']}")
        else:
            run_action(args.action, args.root, args.name,
                       getattr(args, "package", None), args.public_key,
                       getattr(args, "output", None))
    except (PackageError, OSError, subprocess.SubprocessError) as error:
        print(f"R030 DENIED: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
