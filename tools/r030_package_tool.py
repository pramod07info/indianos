#!/usr/bin/env python3
"""IndianOS R030 offline developer package manager.

Host reference tool, NOT an IndianOS kernel package daemon. No network,
post-install scripts, or host execution of package bytes. Ed25519 verification
uses OpenSSL with an explicitly chosen trust anchor; no private key in repo.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import struct
import subprocess
import sys
import tempfile
import zipfile

NAME = re.compile(r"[a-z][a-z0-9-]{1,47}\Z", re.ASCII)
VERSION = re.compile(r"(0|[1-9][0-9]{0,6})\.(0|[1-9][0-9]{0,6})\.(0|[1-9][0-9]{0,6})\Z", re.ASCII)
FILES = frozenset({"manifest.json", "manifest.sig", "payload.elf"})
MAX_ELF = 4 * 1024 * 1024
MAX_ARCHIVE = MAX_ELF + 65536
USER_MIN, GUARD_BEGIN = 0x400000, 0x7FFFF0000000 - 0x10000 - 0x1000


class PackageError(ValueError):
    pass


def canonical(obj: dict) -> bytes:
    return (json.dumps(obj, sort_keys=True, ensure_ascii=True, separators=(",", ":")) + "\n").encode("ascii")


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def version(s: str) -> tuple[int, int, int]:
    if not isinstance(s, str) or VERSION.fullmatch(s) is None:
        raise PackageError("version must be numeric major.minor.patch")
    return tuple(int(p) for p in s.split("."))


def check_manifest(meta: object, binary: bytes) -> dict:
    if not isinstance(meta, dict) or set(meta) != {
        "format", "name", "version", "architecture", "entry", "size", "sha256"
    }:
        raise PackageError("unsupported manifest keys")
    if meta["format"] != 1 or meta["architecture"] != "x86_64-indianos" or meta["entry"] != "/bin/init":
        raise PackageError("unsupported package format, architecture, or entry")
    if not isinstance(meta["name"], str) or not NAME.fullmatch(meta["name"]):
        raise PackageError("invalid package name")
    version(meta["version"])
    if type(meta["size"]) is not int or meta["size"] != len(binary) or not 64 <= len(binary) <= MAX_ELF:
        raise PackageError("package ELF length mismatch")
    if not isinstance(meta["sha256"], str) or not re.fullmatch(r"[a-f0-9]{64}", meta["sha256"]) or sha(binary) != meta["sha256"]:
        raise PackageError("ELF digest mismatch")
    validate_elf(binary)
    return meta


def validate_elf(data: bytes) -> None:
    """Mirror essential static ET_EXEC protections of kernel program planner."""
    if (len(data) < 64 or data[:9] != b"\x7fELF\x02\x01\x01\x00\x00" or any(data[9:16]) or
            struct.unpack_from("<HHI", data, 16) != (2, 62, 1) or
            struct.unpack_from("<HH", data, 52) != (64, 56)):
        raise PackageError("not a static IndianOS x86_64 ELF64 ET_EXEC")
    entry = struct.unpack_from("<Q", data, 24)[0]
    phoff = struct.unpack_from("<Q", data, 32)[0]
    count = struct.unpack_from("<H", data, 56)[0]
    if count < 1 or count > 32 or phoff > len(data) or count * 56 > len(data) - phoff or not USER_MIN <= entry < GUARD_BEGIN:
        raise PackageError("invalid ELF header or entry")
    spans, executable = [], False
    for n in range(count):
        off = phoff + n * 56
        kind, flags = struct.unpack_from("<II", data, off)
        if kind in (2, 3, 7) or kind == 0x6474E551 and flags & 1:
            raise PackageError("ELF interpreter/dynamic/TLS/exec stack disallowed")
        if kind != 1:
            continue
        _, _, offset, va, _, file_len, mem_len, align = struct.unpack_from("<IIQQQQQQ", data, off)
        if (len(spans) >= 8 or flags & ~7 or flags & 4 == 0 or flags & 3 == 3 or
                mem_len < 1 or file_len > mem_len or offset > len(data) or file_len > len(data) - offset or
                not USER_MIN <= va < GUARD_BEGIN or mem_len > GUARD_BEGIN - va or
                offset % 4096 != va % 4096 or
                align > 1 and (align & (align-1) or va % align != offset % align)):
            raise PackageError("ELF segment invalid, writable+executable or out of range")
        begin = va & ~4095
        end = (va + mem_len + 4095) & ~4095
        if any(not (end <= old_a or begin >= old_b) for old_a, old_b in spans):
            raise PackageError("ELF overlapping mapped pages")
        spans.append((begin, end))
        if flags & 1 and va <= entry < va + file_len:
            executable = True
    if not spans or not executable:
        raise PackageError("no executable PT_LOAD entry")


def _openssl(args: list[str]) -> bytes:
    try:
        process = subprocess.run(["openssl", *args], stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, timeout=15, check=False)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise PackageError("OpenSSL unavailable or timed out") from exc
    if process.returncode:
        raise PackageError("Ed25519 signature operation failed")
    return process.stdout


def sign(data: bytes, private_key: Path) -> bytes:
    # Ed25519 requires a seekable message input on supported OpenSSL builds.
    with tempfile.TemporaryDirectory(prefix="indianos-sign-") as tmp:
        message = Path(tmp) / "manifest"
        message.write_bytes(data)
        return _openssl(["pkeyutl", "-sign", "-rawin", "-inkey", str(private_key),
                         "-in", str(message)])


def check_signature(raw: bytes, signature: bytes, public_key: Path) -> None:
    if len(signature) != 64 or not public_key.is_file():
        raise PackageError("Ed25519 signature or trusted public key missing")
    with tempfile.TemporaryDirectory(prefix="indianos-verify-") as tmp:
        sig = Path(tmp) / "sig"
        message = Path(tmp) / "manifest"
        sig.write_bytes(signature)
        message.write_bytes(raw)
        _openssl(["pkeyutl", "-verify", "-rawin", "-pubin", "-inkey",
                  str(public_key), "-sigfile", str(sig), "-in", str(message)])


def parse_archive(package: Path, public_key: Path) -> tuple[dict, bytes]:
    if not package.is_file() or package.stat().st_size > MAX_ARCHIVE:
        raise PackageError("package absent or exceeds bound")
    try:
        with zipfile.ZipFile(package) as z:
            members = z.infolist()
            if len(members) != 3 or set(m.filename for m in members) != FILES:
                raise PackageError("unexpected, duplicate, or missing archive entries")
            for member in members:
                if (member.flag_bits & 1 or member.compress_type != zipfile.ZIP_STORED or
                        member.file_size > MAX_ELF or member.compress_size != member.file_size or
                        (member.create_system == 3 and stat.S_IFMT(member.external_attr >> 16) not in (0, stat.S_IFREG))):
                    raise PackageError("only uncompressed regular archive files accepted")
            raw = z.read("manifest.json")
            signature = z.read("manifest.sig")
            binary = z.read("payload.elf")
    except (zipfile.BadZipFile, KeyError, OSError) as exc:
        raise PackageError("invalid package archive") from exc
    if len(raw) > 4096:
        raise PackageError("manifest too large")
    try:
        obj = json.loads(raw.decode("ascii"))
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise PackageError("manifest is not valid ASCII JSON") from exc
    if not isinstance(obj, dict) or canonical(obj) != raw:
        raise PackageError("manifest is not canonical")
    check_manifest(obj, binary)
    check_signature(raw, signature, public_key)
    return obj, binary


def build(binary_path: Path, out: Path, name: str, release: str, private_key: Path) -> dict:
    binary = binary_path.read_bytes()
    manifest = {"format": 1, "name": name, "version": release,
                "architecture": "x86_64-indianos", "entry": "/bin/init",
                "size": len(binary), "sha256": sha(binary)}
    check_manifest(manifest, binary)
    raw = canonical(manifest)
    signature = sign(raw, private_key)
    if len(signature) != 64:
        raise PackageError("invalid Ed25519 signature length")
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(prefix=".r030-bundle-", suffix=".tmp", dir=out.parent, delete=False) as tmp:
        tmp_name = Path(tmp.name)
    try:
        with zipfile.ZipFile(tmp_name, "w", compression=zipfile.ZIP_STORED) as z:
            for filename, bytes_ in [("manifest.json", raw), ("manifest.sig", signature), ("payload.elf", binary)]:
                z.writestr(filename, bytes_)
        os.replace(tmp_name, out)
    finally:
        tmp_name.unlink(missing_ok=True)
    return manifest


def ensure_root(root: Path) -> Path:
    if not root.is_absolute() or not root.is_dir() or root.is_symlink():
        raise PackageError("requires an existing absolute, nonsymlink disposable --root")
    state = root / ".indianos-apps"
    if state.is_symlink():
        raise PackageError("managed application state is a symlink")
    state.mkdir(exist_ok=True, mode=0o700)
    if not state.is_dir() or state.is_symlink():
        raise PackageError("managed application state not a safe directory")
    return state


def app_dir(state: Path, name: str) -> Path:
    if not NAME.fullmatch(name):
        raise PackageError("invalid name")
    p = state / name
    if p.is_symlink():
        raise PackageError("app state symlink rejected")
    p.mkdir(exist_ok=True, mode=0o700)
    if p.is_symlink() or not p.is_dir():
        raise PackageError("invalid app directory")
    return p


def read_state(base: Path) -> dict:
    path = base / "state.json"
    if not path.exists():
        return {"current": None, "previous": None}
    if path.is_symlink() or path.stat().st_size > 4096:
        raise PackageError("unsafe package state")
    try:
        obj = json.loads(path.read_text(encoding="ascii"))
    except (OSError, ValueError) as exc:
        raise PackageError("corrupt transaction state") from exc
    if not isinstance(obj, dict) or set(obj) != {"current", "previous"}:
        raise PackageError("invalid state structure")
    for item in obj.values():
        if item is not None and (not isinstance(item, dict) or
                set(item) != {"version", "sha256"} or
                not isinstance(item["sha256"], str) or
                not re.fullmatch("[a-f0-9]{64}", item["sha256"])):
            raise PackageError("invalid state record")
        if item is not None:
            version(item["version"])
    return obj


def commit_state(base: Path, state: dict) -> None:
    with tempfile.NamedTemporaryFile(dir=base, prefix=".state-", mode="w", encoding="ascii", delete=False) as tmp:
        tmp_name = Path(tmp.name)
        tmp.write(canonical(state).decode("ascii"))
        tmp.flush()
        os.fsync(tmp.fileno())
    try:
        os.replace(tmp_name, base / "state.json")
        fd = os.open(base, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        tmp_name.unlink(missing_ok=True)


def write_payload(base: Path, payload: bytes, digest: str) -> None:
    dest = base / (digest + ".elf")
    if dest.is_symlink():
        raise PackageError("payload destination is a symlink")
    if dest.exists():
        if not dest.is_file() or sha(dest.read_bytes()) != digest:
            raise PackageError("preexisting payload hash conflict")
        return
    with tempfile.NamedTemporaryFile(dir=base, prefix=".payload-", delete=False) as tmp:
        tmp_name = Path(tmp.name)
        tmp.write(payload)
        tmp.flush()
        os.fsync(tmp.fileno())
    try:
        os.chmod(tmp_name, 0o600)  # intentionally NOT executable on host
        os.replace(tmp_name, dest)
    finally:
        tmp_name.unlink(missing_ok=True)


def verify_local(base: Path, item: dict) -> Path:
    target = base / (item["sha256"] + ".elf")
    if target.is_symlink() or not target.is_file() or target.stat().st_size > MAX_ELF:
        raise PackageError("missing or unsafe installed payload")
    binary = target.read_bytes()
    if sha(binary) != item["sha256"]:
        raise PackageError("installed ELF digest mismatch")
    validate_elf(binary)
    return target


def install(package: Path, public_key: Path, root: Path) -> dict:
    manifest, binary = parse_archive(package, public_key)
    base = app_dir(ensure_root(root), manifest["name"])
    state = read_state(base)
    current = state["current"]
    if current is not None:
        verify_local(base, current)
        if version(manifest["version"]) <= version(current["version"]):
            raise PackageError("downgrade or duplicate install rejected")
    record = {"version": manifest["version"], "sha256": manifest["sha256"]}
    write_payload(base, binary, manifest["sha256"])
    next_state = {"current": record, "previous": current}
    commit_state(base, next_state)
    return next_state


def rollback(root: Path, name: str) -> dict:
    base = app_dir(ensure_root(root), name)
    state = read_state(base)
    if state["current"] is None or state["previous"] is None:
        raise PackageError("no rollback version available")
    verify_local(base, state["current"])
    verify_local(base, state["previous"])
    newer = {"current": state["previous"], "previous": state["current"]}
    commit_state(base, newer)
    return newer


def remove(root: Path, name: str) -> dict:
    base = app_dir(ensure_root(root), name)
    state = read_state(base)
    if state["current"] is None:
        raise PackageError("not installed")
    verify_local(base, state["current"])
    commit_state(base, {"current": None, "previous": None})
    # Retired blobs remain for manual cleanup; never delete unknown paths.
    return {"removed": name}


def active(root: Path, name: str) -> Path:
    base = app_dir(ensure_root(root), name)
    item = read_state(base)["current"]
    if item is None:
        raise PackageError("application is not installed")
    return verify_local(base, item)


def main() -> None:
    parser = argparse.ArgumentParser(description="Offline signed IndianOS native bundle manager (developer host)")
    commands = parser.add_subparsers(dest="cmd", required=True)
    p = commands.add_parser("build")
    for flag in ("elf", "output", "private-key", "name", "version"):
        p.add_argument("--" + flag, required=True)
    p = commands.add_parser("verify")
    p.add_argument("--package", required=True)
    p.add_argument("--trust-key", required=True)
    p = commands.add_parser("install")
    p.add_argument("--package", required=True)
    p.add_argument("--trust-key", required=True)
    p.add_argument("--root", required=True)
    for name in ("rollback", "remove", "active"):
        p = commands.add_parser(name)
        p.add_argument("--root", required=True)
        p.add_argument("--name", required=True)
    args = parser.parse_args()
    try:
        if args.cmd == "build":
            result = build(Path(args.elf), Path(args.output), args.name, args.version, Path(args.private_key))
        elif args.cmd == "verify":
            result, _ = parse_archive(Path(args.package), Path(args.trust_key))
        elif args.cmd == "install":
            result = install(Path(args.package), Path(args.trust_key), Path(args.root))
        elif args.cmd == "rollback":
            result = rollback(Path(args.root), args.name)
        elif args.cmd == "remove":
            result = remove(Path(args.root), args.name)
        else:
            result = {"elf": str(active(Path(args.root), args.name))}
        print(json.dumps(result, sort_keys=True))
    except (PackageError, FileNotFoundError, PermissionError) as exc:
        print("IndianOS R030 rejected: " + str(exc), file=sys.stderr)
        raise SystemExit(1) from exc


if __name__ == "__main__":
    main()
