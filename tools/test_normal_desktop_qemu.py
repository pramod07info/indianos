#!/usr/bin/env python3
"""Normal-profile four-service desktop on explicitly provisioned GPT storage.

Requires the production DESKTOP_SESSION image, not the opt-in R031 ISO. All
desktop input uses emulated physical PS/2 devices. UART input is explicitly
injected as a negative control and must not operate the desktop.
The supplied fixture is copied before use and never modified in place.
"""
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import re
import socket
import shutil
import subprocess
import struct
import tempfile
import zlib

from test_r029_desktop_qemu import QemuSession, open_monitor, check_two_windows, differences
from test_boot_recovery_qemu import checked_run
from test_r031_files_gui_qemu import physical_key, PHYSICAL
from test_m1_terminal_persistence_qemu import (
    acknowledged_key, type_command, changed, files_repainted, exact_terminal_line,
    ACK, COMMITTED, DISPLAYED, PERSISTED, READBACK, PAYLOAD, SHUTDOWN,
)

START = 40
COUNT = 100
TOTAL = 256
READY = "IndianOS user: normal desktop READY: PID1 broker PID2 Files PID3 Terminal PID4 compositor"
COMPOSITOR = "IndianOS user: desktop compositor PID4 ready"
STOPPED = "IndianOS normal desktop: services stopped and resources reclaimed"
RECOVERY = "IndianOS recovery: reason=desktop-storage-unavailable"
FORBIDDEN = (b"PID1 provisioned", b"R027 blank disk", b"normal PID1 ready:")
UART_ACTIONS = (PHYSICAL.encode(), SHUTDOWN.encode(), b"application launcher",
                b"user switched to", b"Terminal physical input acknowledged")
NAMED_COMMIT = "desktop broker committed named selected-volume mutation and refreshed Files"
NAMED_READBACK = "IndianOS user: R051 Terminal readback: saved"


def type_named_command(session: QemuSession, command: str) -> None:
    # Every HMP token comes from this fixed map; reject the entire string
    # before injecting anything. '=' is needed by the bounded put command.
    if not command or any(c not in "abcdefghijklmnopqrstuvwxyz =" for c in command):
        raise AssertionError("unsupported physical named-file command")
    for char in command:
        if char == "=":
            start = len(session.output)
            session.hmp("sendkey equal 80")
            session.until(ACK, 20, since=start)
            if SHUTDOWN.encode() in session.output[start:]:
                raise AssertionError("named editor input escaped into shutdown")
        else:
            acknowledged_key(session, "spc" if char == " " else char)


def verify_listing(output: bytes, since: int, expected: set[str]) -> None:
    prefix = b"IndianOS user: R055 Terminal directory: "
    lines = output[since:].replace(b"\r", b"").split(b"\n")[:-1]
    for line in lines:
        if line.startswith(prefix):
            tokens = line[len(prefix):].decode("ascii").split(" ")
            if len(tokens) == len(expected) and set(tokens) == expected:
                return
    raise AssertionError("no fresh complete selected-directory result")


def verify_guard(original: bytes, actual: bytes) -> None:
    if len(original) != TOTAL * 512 or len(actual) != len(original):
        raise AssertionError("GPT desktop fixture geometry changed")
    begin, end = START * 512, (START + COUNT) * 512
    if actual[:begin] != original[:begin] or actual[end:] != original[end:]:
        raise AssertionError("desktop wrote outside the explicitly selected partition")


def verify_runtime(output: bytes, shutdown_at: int) -> None:
    if b"FATAL" in output or any(marker in output for marker in FORBIDDEN):
        raise AssertionError("normal desktop used a provisioning or console profile")
    if (b"restarting retired service session" in output or
            b"restart budget exhausted" in output):
        raise AssertionError("healthy desktop unexpectedly restarted")
    if COMPOSITOR.encode() not in output or READY.encode() not in output:
        raise AssertionError("normal desktop did not start four independent services")
    prefix = output[:shutdown_at]
    if (COMPOSITOR.encode() not in prefix or READY.encode() not in prefix or
            prefix.find(COMPOSITOR.encode()) > prefix.find(READY.encode())):
        raise AssertionError("desktop advertised ready before compositor service")
    for pid in (1, 2, 3, 4):
        for state in ("READY", "DISPATCHED"):
            if f"IndianOS: PID {pid} {state}".encode() not in prefix:
                raise AssertionError("desktop readiness lacks actual process execution")
    tail = output[shutdown_at:]
    synced = tail.find(b"desktop selected volume synchronized")
    shutdown = tail.find(SHUTDOWN.encode())
    if synced < 0 or shutdown < 0 or synced > shutdown:
        raise AssertionError("desktop shutdown preceded a verified storage barrier")
    for pid in (1, 2, 3, 4):
        if f"IndianOS: PID {pid} REAPED".encode() not in tail:
            raise AssertionError("normal desktop did not reclaim every service")
    if STOPPED.encode() not in tail:
        raise AssertionError("normal desktop did not return cleanly to recovery")


def verify_uart_ignored(before: tuple[int, int, bytes], after: tuple[int, int, bytes],
                        output: bytes, since: int) -> None:
    if any(marker in output[since:] for marker in UART_ACTIONS):
        raise AssertionError("UART injection operated the physical-only desktop")
    if before[:2] != after[:2] or differences(before, after):
        raise AssertionError("UART injection changed the desktop framebuffer")


def spawn(qemu: str, iso: Path, ovmf: Path, disk: Path, folder: Path, name: str,
          debugger: Path | None = None):
    monitor = folder / f"{name}.sock"
    command = [qemu, "-machine", "pc", "-m", "256M", "-smp", "1",
               "-display", "none", "-vga", "std", "-serial", "stdio",
               "-monitor", f"unix:{monitor},server=on,wait=off", "-no-reboot",
               "-no-shutdown", "-boot", "d",
               "-drive", f"if=pflash,format=raw,readonly=on,file={ovmf}",
               "-cdrom", str(iso),
               "-drive", f"if=ide,index=0,media=disk,format=raw,file={disk}"]
    if debugger is not None:
        command.extend(["-gdb", f"unix:{debugger},server=on,wait=off"])
    process = subprocess.Popen(command, stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT, bufsize=0)
    try:
        sock = open_monitor(monitor, process)
        return process, sock, QemuSession(process, sock)
    except BaseException:
        process.kill()
        process.communicate(timeout=5)
        raise


def close(process, sock, session: QemuSession, output: Path) -> None:
    session.selector.close()
    sock.close()
    if process.poll() is None:
        process.terminate()
    try:
        remaining, _ = process.communicate(timeout=3)
    except subprocess.TimeoutExpired:
        process.kill()
        remaining, _ = process.communicate(timeout=3)
    if remaining:
        session.output.extend(remaining)
    # Preserve the rest of a fault diagnostic even when its first UART chunk
    # caused the acceptance loop to reject the guest immediately.
    output.write_bytes(session.output)


def boot(qemu: str, iso: Path, ovmf: Path, disk: Path, folder: Path, index: int) -> None:
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, f"desktop-{index}")
    try:
        session.until("IndianOS normal desktop: selected storage admitted", 45)
        session.until(COMPOSITOR, 20)
        session.until(READY, 20)
        session.until("R031.9 real file and directory contents rendered into Files window", 20)
        frame = session.shot(folder / f"desktop-{index}-initial.ppm")
        check_two_windows(frame)
        if PHYSICAL.encode() in session.output:
            raise AssertionError("physical-origin evidence appeared before physical input")
        # Real UART bytes must not be accepted as physical desktop commands.
        serial_at = len(session.output)
        session.serial(b"qaa21")
        session.drain_serial(0.5)
        serial_frame = session.shot(folder / f"desktop-{index}-uart-negative.ppm")
        verify_uart_ignored(frame, serial_frame, bytes(session.output), serial_at)
        actions = [("a", "R031.1 application launcher OPEN"),
                   ("a", "R031.1 application launcher CLOSED"),
                   ("2", "R031.1 user switched to Terminal app"),
                   ("1", "R031.1 user switched to Files app"),
                   ("2", "R031.1 user switched to Terminal app")]
        for step, (key, marker) in enumerate(actions):
            frame = physical_key(session, key, marker, folder, index, step, frame, step == 0)
        pointer_at = len(session.output)
        session.hmp("mouse_move 100 135")
        session.drain_serial(0.2)
        session.hmp("mouse_button 1")
        session.drain_serial(0.2)
        session.hmp("mouse_button 0")
        session.until("R031.1 physical PS2 mouse event reached desktop service", 20, since=pointer_at)
        acknowledged_key(session, "1", "R031.1 user switched to Files app")
        frame = session.shot(folder / f"desktop-{index}-files-focus.ppm")
        frame = physical_key(session, "2", "R031.1 user switched to Terminal app",
                             folder, index, 8, frame, False)
        acknowledged_key(session, "ret")
        editing = session.shot(folder / f"desktop-{index}-editor.ppm")
        changed(frame, editing, "normal Terminal activation")
        type_command(session, "write " + PAYLOAD if index == 0 else "cat")
        typed = session.shot(folder / f"desktop-{index}-typed.ppm")
        changed(editing, typed, "physical normal Terminal command", 1)
        start = acknowledged_key(session, "ret")
        if index == 0:
            session.until(COMMITTED, 20, since=start)
            session.until(PERSISTED, 20, since=start)
        else:
            session.until(DISPLAYED, 20, since=start)
            session.until(READBACK, 20, since=start)
            session.drain_serial(0.1)
            exact_terminal_line(bytes(session.output), start, READBACK)
        result = session.shot(folder / f"desktop-{index}-result.ppm")
        changed(typed, result, "selected-volume Terminal result")
        if index == 0:
            files_repainted(typed, result)
        for text, expected in (("ls", "IndianOS user: R055 Terminal directory: "),
                               ("stat", "IndianOS user: R055 Terminal stat: 10 BYTES")):
            type_command(session, text)
            typed = session.shot(folder / f"desktop-{index}-{text}-typed.ppm")
            start = acknowledged_key(session, "ret")
            session.until(expected, 20, since=start)
            session.drain_serial(0.1)
            if text == "ls":
                verify_listing(bytes(session.output), start,
                               {"f:readme"} if index == 0 else {"f:readme", "f:a", "d:docs"})
            else:
                exact_terminal_line(bytes(session.output), start, expected)
            result = session.shot(folder / f"desktop-{index}-{text}-result.ppm")
            changed(typed, result, "selected-volume " + text)
        if index == 0:
            for command in ("touch tmp", "put tmp=saved", "mv tmp a",
                            "touch gone", "rm gone", "mkdir docs"):
                type_named_command(session, command)
                submitted = acknowledged_key(session, "ret")
                session.until(NAMED_COMMIT, 20, since=submitted)
        type_named_command(session, "cat a")
        submitted = acknowledged_key(session, "ret")
        session.until(NAMED_READBACK, 20, since=submitted)
        session.drain_serial(0.1)
        exact_terminal_line(bytes(session.output), submitted, NAMED_READBACK)
        result = session.shot(folder / f"desktop-{index}-named-readback.ppm")
        acknowledged_key(session, "tab")
        shutdown_at = len(session.output)
        physical_key(session, "q", SHUTDOWN, folder, index, 9, result, False)
        session.until(STOPPED, 30, since=shutdown_at)
        session.drain_serial(0.1)
        verify_runtime(bytes(session.output), shutdown_at)
    finally:
        close(process, sock, session, folder / f"desktop-{index}.serial.log")
    print(f"Normal desktop boot {index + 1}/2: four ring3 services, physical input, selected GPT readback and cleanup PASS")


def rotate_guid(raw: bytes) -> bytes:
    result = bytearray(raw)
    for lba in (1, 255):
        offset = lba * 512
        result[offset + 56:offset + 72] = bytes([0x99]) * 16
        result[offset + 16:offset + 20] = bytes(4)
        checksum = zlib.crc32(result[offset:offset + 92])
        result[offset + 16:offset + 20] = checksum.to_bytes(4, "little")
    return bytes(result)


def abrupt_reboot(qemu: str, iso: Path, ovmf: Path, disk: Path, folder: Path) -> None:
    """Kill QEMU after a committed save, then read through fresh ring3 services.

    This tests unclean guest loss with host storage intact, not a physical
    power cut or a write torn inside an ATA transaction.
    """
    original = disk.read_bytes()
    for attempt in (0, 1):
        process, sock, session = spawn(qemu, iso, ovmf, disk, folder, f"abrupt-{attempt}")
        before = disk.read_bytes()
        try:
            session.until(READY, 45)
            acknowledged_key(session, "2", "R031.1 user switched to Terminal app")
            acknowledged_key(session, "ret")
            commands = ("touch cut", "put cut=saved") if attempt == 0 else ("cat cut",)
            for command in commands:
                type_named_command(session, command)
                submitted = acknowledged_key(session, "ret")
                session.until(NAMED_COMMIT if attempt == 0 else NAMED_READBACK,
                              20, since=submitted)
                if attempt == 1:
                    session.drain_serial(0.1)
                    exact_terminal_line(bytes(session.output), submitted, NAMED_READBACK)
            session.shot(folder / f"abrupt-{attempt}-terminal.ppm")
            if SHUTDOWN.encode() in session.output or STOPPED.encode() in session.output:
                raise AssertionError("unclean reboot accidentally used clean session shutdown")
            # SIGKILL prevents QEMU's orderly shutdown handler from providing
            # the very durability barrier this scenario deliberately omits.
            process.kill()
            process.wait(timeout=5)
        finally:
            close(process, sock, session, folder / f"abrupt-{attempt}.serial.log")
        verify_guard(original, disk.read_bytes())
        if attempt == 1 and disk.read_bytes() != before:
            raise AssertionError("unclean reboot readback changed persistent storage")
    print("Normal desktop abrupt guest loss: committed named file survives SIGKILL/reboot PASS")


def denied_boot(qemu: str, iso: Path, ovmf: Path, disk: Path, folder: Path, name: str,
                reason: str = RECOVERY) -> None:
    before = hashlib.sha256(disk.read_bytes()).digest()
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, name)
    try:
        session.until(reason, 45)
        session.until("IndianOS recovery: read-only serial console ready", 10)
        if b"starting four ring3 services" in session.output or READY.encode() in session.output:
            raise AssertionError("invalid/unselected storage started desktop processes")
        if any(marker in session.output for marker in FORBIDDEN):
            raise AssertionError("storage failure attempted provisioning or console fallback")
    finally:
        close(process, sock, session, folder / f"{name}.serial.log")
    if hashlib.sha256(disk.read_bytes()).digest() != before:
        raise AssertionError("failed storage admission modified the rejected disk")
    print(f"Normal desktop {name}: read-only recovery, no service startup and unchanged media PASS")


def rejected_namespaces(qemu: str, iso: Path, ovmf: Path, fixture_tool: Path,
                        original: bytes, folder: Path) -> None:
    # Generate semantic corruption through the real COW writer. GPT and both
    # metadata/payload checksums remain valid; flipping a checksum would test
    # a different admission barrier and hide a missing namespace validation.
    for kind in ("orphan", "alias", "marker", "link"):
        disk = folder / f"namespace-{kind}.raw"
        checked_run([str(fixture_tool), "--namespace-fault", kind, "--output", str(disk)])
        verify_guard(original, disk.read_bytes())
        denied_boot(qemu, iso, ovmf, disk, folder, f"namespace-{kind}")



def rewrite_initramfs(raw: bytes, target: bytes, payload: bytes | None) -> bytes:
    """Fault fixture only: preserve every unrelated entry and header invariant."""
    if len(raw) < 16 or raw[:8] != b"INDRAM01":
        raise AssertionError("invalid source initramfs")
    count, total = struct.unpack_from("<II", raw, 8)
    if not 1 <= count <= 8 or total != len(raw):
        raise AssertionError("invalid source archive bounds")
    offset, found = 16, False
    entries = []
    seen = set()
    for _ in range(count):
        if offset + 8 > len(raw):
            raise AssertionError("truncated source archive entry")
        size, reserved, length = struct.unpack_from("<HHI", raw, offset)
        offset += 8
        if reserved or not 1 <= size <= 63 or offset + size + length > len(raw):
            raise AssertionError("invalid source archive entry")
        name = raw[offset:offset + size]
        data = raw[offset + size:offset + size + length]
        offset += size + length
        if name in seen:
            raise AssertionError("duplicate source archive entry")
        seen.add(name)
        if name == target:
            found = True
            if payload is None:
                continue
            data = payload
        entries.append((name, data))
    if not found or offset != len(raw) or not entries:
        raise AssertionError("missing fault target or trailing source bytes")
    body = b"".join(struct.pack("<HHI", len(name), 0, len(data)) + name + data
                    for name, data in entries)
    return b"INDRAM01" + struct.pack("<II", len(entries), 16 + len(body)) + body


def faulting_service(raw: bytes, target: bytes) -> bytes:
    """Keep a valid ELF and replace its actual entry instructions with UD2."""
    offset = 16
    for _ in range(struct.unpack_from("<I", raw, 8)[0]):
        size, _, length = struct.unpack_from("<HHI", raw, offset)
        offset += 8
        name = raw[offset:offset + size]
        data = bytearray(raw[offset + size:offset + size + length])
        offset += size + length
        if name != target:
            continue
        if data[:7] != b"\x7fELF\x02\x01\x01" or len(data) < 64:
            raise AssertionError("fault source is not a native ELF")
        entry, phoff = struct.unpack_from("<QQ", data, 24)
        stride, count = struct.unpack_from("<HH", data, 54)
        if stride != 56 or phoff + count * stride > len(data):
            raise AssertionError("fault source program headers are malformed")
        for index in range(count):
            at = phoff + index * stride
            kind, flags, file_offset, virtual = struct.unpack_from("<IIQQ", data, at)
            size = struct.unpack_from("<Q", data, at + 32)[0]
            if kind == 1 and flags & 1 and virtual <= entry and entry + 2 <= virtual + size:
                start = file_offset + entry - virtual
                if start + 2 > len(data):
                    raise AssertionError("fault entry exceeds the ELF file")
                data[start:start + 2] = b"\x0f\x0b"
                return bytes(data)
        raise AssertionError("fault source has no file-backed executable entry")
    raise AssertionError("fault service is missing")


class GuestDebugger:
    """Small bounded ASCII subset of QEMU's local GDB remote protocol.

    This is host fault injection, never a production kernel test hook. The
    caller verifies the actual current process and privilege level before
    changing a register. No code, disk, page table or kernel memory is written.
    """
    def __init__(self, path: Path):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(15)
        try:
            self.sock.connect(str(path))
        except BaseException:
            self.sock.close()
            raise

    def close(self) -> None:
        self.sock.close()

    def byte(self) -> bytes:
        value = self.sock.recv(1)
        if not value:
            raise AssertionError("QEMU debugger disconnected")
        return value

    def send(self, value: str) -> None:
        raw = value.encode("ascii")
        self.sock.sendall(b"$" + raw + b"#" + f"{sum(raw) % 256:02x}".encode())

    def response(self) -> str:
        # Only acknowledgements can precede a response packet. Never accept a
        # checksum failure, unbounded packet or unsupported empty response.
        lead = self.byte()
        if lead == b"+":
            lead = self.byte()
        if lead != b"$":
            raise AssertionError("invalid QEMU debugger acknowledgement")
        payload = bytearray()
        while True:
            char = self.byte()
            if char == b"#":
                break
            payload.extend(char)
            if len(payload) > 16384:
                raise AssertionError("QEMU debugger response exceeded bound")
        checksum = self.byte() + self.byte()
        if checksum != f"{sum(payload) % 256:02x}".encode():
            raise AssertionError("invalid QEMU debugger checksum")
        self.sock.sendall(b"+")
        return payload.decode("ascii")

    def command(self, value: str) -> str:
        self.send(value)
        return self.response()

    def memory(self, address: int, count: int) -> bytes:
        if count <= 0 or count > 64:
            raise AssertionError("debugger read outside bounded inspection")
        result = self.command(f"m{address:x},{count:x}")
        if len(result) != count * 2:
            raise AssertionError("QEMU debugger memory read failed")
        try:
            return bytes.fromhex(result)
        except ValueError as error:
            raise AssertionError("malformed debugger memory") from error

    def register(self, number: int) -> int:
        result = self.command(f"p{number:x}")
        if len(result) not in (8, 16):
            raise AssertionError("unsupported x86-64 debugger register")
        try:
            return int.from_bytes(bytes.fromhex(result), "little")
        except ValueError as error:
            raise AssertionError("malformed debugger register") from error


def executable_bytes(elf: bytes, address: int, length: int) -> bytes:
    if len(elf) < 64 or elf[:7] != b"\x7fELF\x02\x01\x01" or length < 1:
        raise AssertionError("invalid executable inspection source")
    phoff = struct.unpack_from("<Q", elf, 32)[0]
    stride, count = struct.unpack_from("<HH", elf, 54)
    if stride != 56 or count > 64 or phoff + stride * count > len(elf):
        raise AssertionError("invalid executable program headers")
    for index in range(count):
        at = phoff + index * stride
        kind, flags, offset, virtual = struct.unpack_from("<IIQQ", elf, at)
        size = struct.unpack_from("<Q", elf, at + 32)[0]
        if (kind == 1 and flags & 1 and virtual <= address and
                address + length <= virtual + size):
            begin = offset + address - virtual
            if begin + length <= len(elf):
                return elf[begin:begin + length]
    raise AssertionError("debugger address is not file-backed executable code")


def unique_symbol(nm: str, elf: Path, parts: tuple[str, ...]) -> int:
    result = subprocess.run([nm, "-n", "-C", str(elf)], check=True,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
    matches = [line.split()[0] for line in result.stdout.decode("ascii").splitlines()
               if all(part in line for part in parts)]
    if len(matches) != 1:
        raise AssertionError("fault target lacks one unambiguous ELF symbol")
    return int(matches[0], 16)


def stack_guard_records(symbols: str) -> list[tuple[int, int, int]]:
    records = []
    for name, count in (("interrupts::DF_STACK", 1), ("interrupts::USER_STACKS", 4)):
        matches = [line.split(maxsplit=3) for line in symbols.splitlines() if name in line]
        if len(matches) != 1 or len(matches[0]) != 4:
            raise AssertionError("kernel lacks unique sized trap-stack symbols")
        base, size = (int(value, 16) for value in matches[0][:2])
        if (base < 0xffff800000000000 or base + size > 1 << 64 or
                size % count or base % 4096):
            raise AssertionError("invalid kernel stack object bounds")
        stride = size // count
        if stride < 8192 or stride > 1024 * 1024 or stride % 4096:
            raise AssertionError("invalid kernel guarded stack size")
        records += [(base + i * stride, base + i * stride + 4096,
                     base + (i + 1) * stride) for i in range(count)]
    ordered = sorted(records)
    if any(left[2] > right[0] for left, right in zip(ordered, ordered[1:])):
        raise AssertionError("kernel stack objects overlap")
    return records


def verify_debug_stack_guards(debugger: GuestDebugger, records: list[tuple[int, int, int]],
                              evidence: list[str], phase: str) -> None:
    for guard, begin, end in records:
        if not debugger.command(f"m{guard:x},1").startswith("E"):
            raise AssertionError("kernel trap-stack guard remains readable")
        debugger.memory(begin, 1)
        debugger.memory(end - 1, 1)
        evidence.append(f"{phase}: guard {guard:#x} unreadable; usable {begin:#x}..{end:#x} readable")


def stopped_root(session: QemuSession) -> int:
    roots = re.findall(r"\bCR3=([0-9a-fA-F]{1,16})\b", session.hmp("info registers"))
    if len(roots) != 1:
        raise AssertionError("missing or ambiguous stopped-guest CR3")
    root = int(roots[0], 16) & 0x000ffffffffff000
    if root == 0:
        raise AssertionError("stopped guest has no physical page root")
    return root


def verify_transient_restart(output: bytes, injection_at: int) -> None:
    if not 0 <= injection_at <= len(output):
        raise AssertionError("invalid transient injection evidence boundary")
    tail = output[injection_at:]
    markers = (b"IndianOS: fault vector=0x000000000000000e",
               b"IndianOS: fault CR2=0x0000000060000000",
               b"IndianOS: PID 4 FAULTED", b"IndianOS: PID 4 REAPED",
               b"IndianOS normal desktop: retired session verified restart-safe",
               b"IndianOS normal desktop: restarting retired service session",
               b"desktop compositor denied input and filesystem authority",
               COMPOSITOR.encode(), READY.encode())
    ready = tail.find(READY.encode())
    if ready < 0:
        raise AssertionError("transient restart lacks fresh desktop readiness")
    # The first reap retires the faulted incarnation. A later normal shutdown
    # legitimately reaps its replacement; do not confuse these two lifetimes.
    fault_phase = tail[:ready + len(READY.encode())]
    cursor = 0
    for marker in markers:
        evidence = fault_phase if marker == b"IndianOS: PID 4 REAPED" else tail
        if evidence.count(marker) != 1:
            raise AssertionError("transient restart lacks fresh exact lifecycle evidence")
        position = fault_phase.find(marker, cursor)
        if position < 0:
            raise AssertionError("transient restart lifecycle is out of order")
        cursor = position + len(marker)
    stopped = tail.find(STOPPED.encode())
    if stopped >= 0 and stopped < ready:
        raise AssertionError("transient session stopped before becoming ready")
    if tail.count(b"IndianOS: PID 4 REAPED") != (2 if stopped >= 0 else 1):
        raise AssertionError("transient restart has unexpected process retirement")
    if (b"FATAL" in output or b"restart budget exhausted" in output or
            b"reason=desktop-session-failed" in output):
        raise AssertionError("transient fault did not recover an interactive session")
    # A fresh four-process session must actually execute after the retry, not
    # reuse the first desktop's logs or advertise a kernel-generated success.
    restart_at = tail.find(markers[5])
    for pid in (1, 2, 3, 4):
        for state in ("READY", "DISPATCHED"):
            if f"IndianOS: PID {pid} {state}".encode() not in tail[restart_at:]:
                raise AssertionError("transient restart lacks fresh process execution")


def verify_bounded_restart(output: bytes) -> None:
    """Require two real faulted sessions, with one fully bounded retry."""
    fault = b"IndianOS: PID 4 FAULTED"
    reap = b"IndianOS: PID 4 REAPED"
    restart = b"IndianOS normal desktop: restarting retired service session"
    verified = b"IndianOS normal desktop: retired session verified restart-safe"
    exhausted = b"IndianOS normal desktop: restart budget exhausted"
    recovery = b"IndianOS recovery: reason=desktop-session-failed"
    for marker, count in ((fault, 2), (reap, 2), (verified, 2), (restart, 1), (exhausted, 1)):
        if output.count(marker) != count:
            raise AssertionError("service restart lacked exact bounded lifecycle evidence")
    cursor = 0
    for marker in (fault, reap, verified, restart, fault, reap, verified, exhausted, recovery):
        position = output.find(marker, cursor)
        if position < 0:
            raise AssertionError("service restart lifecycle evidence is out of order")
        cursor = position + len(marker)
    if READY.encode() in output or b"FATAL" in output:
        raise AssertionError("faulted service became ready or escaped recovery")


def crashed_service_boot(qemu: str, iso: Path, ovmf: Path, disk: Path,
                         folder: Path) -> None:
    before = disk.read_bytes()
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, "crashed-compositor")
    try:
        session.until("IndianOS: PID 4 FAULTED", 45)
        session.until("IndianOS: PID 4 REAPED", 10)
        session.until("IndianOS recovery: reason=desktop-session-failed", 30)
        session.until("IndianOS recovery: read-only serial console ready", 10)
        verify_bounded_restart(bytes(session.output))
    finally:
        close(process, sock, session, folder / "crashed-compositor.serial.log")
    if disk.read_bytes() != before:
        raise AssertionError("service crash recovery modified selected storage")
    print("Normal desktop compositor #UD: two actual faults, one bounded restart and read-only recovery PASS")


def transient_service_boot(qemu: str, iso: Path, ovmf: Path, disk: Path,
                           folder: Path, xorriso: str, nm: str = "nm") -> None:
    sources = folder / "transient-sources"
    sources.mkdir()
    kernel = sources / "kernel.elf"
    archive = sources / "initramfs.img"
    checked_run([xorriso, "-osirrox", "on", "-indev", str(iso),
                 "-extract", "/boot/indianos.elf", str(kernel),
                 "-extract", "/boot/initramfs.img", str(archive)])
    raw = archive.read_bytes()
    # Validate the complete archive using the existing strict fault builder
    # before looking up the service used for debugger address resolution.
    rewrite_initramfs(raw, b"/bin/compositor", None)
    offset = 16
    compositor = None
    for _ in range(struct.unpack_from("<I", raw, 8)[0]):
        size, _, length = struct.unpack_from("<HHI", raw, offset)
        offset += 8
        if raw[offset:offset + size] == b"/bin/compositor":
            compositor = raw[offset + size:offset + size + length]
        offset += size + length
    if compositor is None:
        raise AssertionError("transient source lacks compositor")
    service = sources / "compositor.elf"
    service.write_bytes(compositor)
    active = unique_symbol(nm, kernel, ("user_runtime::ACTIVE_PROCESS",))
    target = unique_symbol(nm, service, ("desktop_window::Manager", "::wire"))
    expected = executable_bytes(compositor, target, 8)
    symbols = subprocess.run([nm, "-S", "-n", "-C", str(kernel)], check=True,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
    guards = stack_guard_records(symbols.stdout.decode("ascii"))
    unmapped = 0x60000000
    original = disk.read_bytes()
    path = folder / "transient-gdb.sock"
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, "transient-compositor", path)
    debugger = None
    evidence = [f"kernel ACTIVE_PROCESS={active:#x}; compositor wire={target:#x}",
                f"expected code={expected.hex()}; injection RIP={unmapped:#x}"]
    try:
        session.until(READY, 45)
        initial = session.shot(folder / "transient-initial.ppm")
        check_two_windows(initial)
        injection_at = len(session.output)
        session.hmp("stop")
        debugger = GuestDebugger(path)
        if not debugger.command("?").startswith(("S", "T")):
            raise AssertionError("debugger did not attach to stopped guest")
        debugger.command("qSupported:xmlRegisters=i386")
        if not debugger.command("qXfer:features:read:target.xml:0,fff").startswith(("l", "m")):
            raise AssertionError("QEMU lacks x86-64 target description")
        if debugger.command(f"Z1,{target:x},1") != "OK":
            raise AssertionError("QEMU refused temporary compositor execution breakpoint")
        # Input still enters through the emulated physical keyboard. Its
        # compositor reply is interrupted by a real page fault, not faked IPC.
        for attempt in range(24):
            debugger.send("c")
            if attempt == 0:
                status = session.hmp("info status")
                if "running" not in status:
                    session.hmp("cont")
                    status = session.hmp("info status")
                evidence.append("guest resume status: " + status.strip())
                if "running" not in status:
                    raise AssertionError("guest did not resume for physical fault trigger")
                session.hmp("sendkey a 80")
            if not debugger.response().startswith(("S", "T")):
                raise AssertionError("debugger did not stop at compositor execution")
            rip, cs = debugger.register(16), debugger.register(18)
            current = int.from_bytes(debugger.memory(active, 8), "little")
            pid = int.from_bytes(debugger.memory(current, 4), "little") if current else 0
            evidence.append(f"break PID={pid}; CS={cs:#x}; RIP={rip:#x}")
            if pid == 4 and cs & 3 == 3 and rip == target:
                if debugger.memory(target, 8) != expected:
                    raise AssertionError("current compositor code differs from source ISO")
                break
            # A virtual breakpoint may also match another process's code.
            # Advance that instruction unchanged, never inject into its context.
            if debugger.command(f"z1,{target:x},1") != "OK":
                raise AssertionError("failed to remove non-target breakpoint")
            if not debugger.command("s").startswith(("S", "T")):
                raise AssertionError("failed to advance non-target guest instruction")
            if debugger.command(f"Z1,{target:x},1") != "OK":
                raise AssertionError("failed to restore target breakpoint")
        else:
            raise AssertionError("bounded debugger search did not find ring3 PID4")
        verify_debug_stack_guards(debugger, guards, evidence, "PID4 cloned root")
        cloned_root = stopped_root(session)
        evidence.append(f"PID4 cloned CR3={cloned_root:#x}")
        if not debugger.command(f"m{unmapped:x},1").startswith("E"):
            raise AssertionError("fault injection address is unexpectedly mapped")
        if debugger.command(f"z1,{target:x},1") != "OK":
            raise AssertionError("failed to remove compositor breakpoint")
        if debugger.command("P10=" + struct.pack("<Q", unmapped).hex()) != "OK":
            raise AssertionError("QEMU refused transient user RIP injection")
        if debugger.register(16) != unmapped:
            raise AssertionError("QEMU did not install requested user RIP")
        evidence.append("verified PID4 ring3 only; temporary breakpoint removed; RIP injected")
        if debugger.command("D") != "OK":
            raise AssertionError("QEMU debugger failed to detach")
        debugger.close()
        debugger = None
        session.hmp("cont")
        session.until("IndianOS: PID 4 FAULTED", 20, since=injection_at)
        session.until(READY, 30, since=injection_at)
        verify_transient_restart(bytes(session.output), injection_at)
        fresh = session.shot(folder / "transient-restarted.ppm")
        check_two_windows(fresh)
        # Exercise fresh app authority, IPC and physical input. No old launcher
        # request may complete the restarted session's command handshake.
        acknowledged_key(session, "a", "R031.1 application launcher OPEN")
        acknowledged_key(session, "a", "R031.1 application launcher CLOSED")
        acknowledged_key(session, "2", "R031.1 user switched to Terminal app")
        acknowledged_key(session, "ret")
        for command in ("touch revived", "put revived=saved", "cat revived"):
            type_named_command(session, command)
            submitted = acknowledged_key(session, "ret")
            marker = NAMED_READBACK if command.startswith("cat ") else NAMED_COMMIT
            session.until(marker, 20, since=submitted)
            if command.startswith("cat "):
                session.drain_serial(0.1)
                exact_terminal_line(bytes(session.output), submitted, marker)
        session.shot(folder / "transient-saved-readback.ppm")
        acknowledged_key(session, "tab")
        shutdown_at = len(session.output)
        acknowledged_key(session, "q", SHUTDOWN)
        session.until(STOPPED, 30, since=shutdown_at)
        session.drain_serial(0.1)
        # The generic healthy-session verifier deliberately rejects restarts.
        # Apply it only to the fresh session beginning at its new READY records.
        restart = bytes(session.output).find(
            b"IndianOS normal desktop: restarting retired service session", injection_at)
        verify_runtime(bytes(session.output[restart + len(
            b"IndianOS normal desktop: restarting retired service session"):]),
                       shutdown_at - restart - len(
                           b"IndianOS normal desktop: restarting retired service session"))
        verify_transient_restart(bytes(session.output), injection_at)
        # After all four mappings/PCBs were reclaimed, inspect the running
        # recovery kernel as well as the earlier actual user-process root.
        session.hmp("stop")
        debugger = GuestDebugger(path)
        if not debugger.command("?").startswith(("S", "T")):
            raise AssertionError("kernel guard inspection did not stop the guest")
        debugger.command("qSupported:xmlRegisters=i386")
        debugger.command("qXfer:features:read:target.xml:0,fff")
        if debugger.register(18) & 3:
            raise AssertionError("guard inspection did not return to kernel privilege")
        kernel_root = stopped_root(session)
        if kernel_root == cloned_root:
            raise AssertionError("reclaimed kernel still uses the retired user root")
        evidence.append(f"reclaimed kernel CR3={kernel_root:#x}")
        verify_debug_stack_guards(debugger, guards, evidence, "reclaimed recovery kernel")
        if debugger.command("D") != "OK":
            raise AssertionError("kernel guard debugger failed to detach")
        debugger.close()
        debugger = None
        session.hmp("cont")
    finally:
        if debugger is not None:
            debugger.close()
        close(process, sock, session, folder / "transient-compositor.serial.log")
        (folder / "transient-debugger.log").write_text("\n".join(evidence) + "\n")
    verify_guard(original, disk.read_bytes())
    print("Normal desktop transient PID4 page fault: fresh ring3 desktop, physical input, durable file and clean shutdown PASS")


def bad_service_images(iso: Path, folder: Path, xorriso: str):
    tree = folder / "fault-iso-tree"
    checked_run([xorriso, "-osirrox", "on", "-indev", str(iso), "-extract", "/", str(tree)])
    for directory in [tree, *(p for p in tree.rglob("*") if p.is_dir())]:
        directory.chmod(directory.stat().st_mode | 0o700)
    destination = tree / "boot/initramfs.img"
    original = destination.read_bytes()
    destination.chmod(destination.stat().st_mode | 0o200)
    cases = (("missing-compositor", b"/bin/compositor", None, "desktop-service-unavailable"),
             ("invalid-compositor", b"/bin/compositor", b"not an ELF", "desktop-service-invalid"),
             ("invalid-selection", b"/etc/desktop-volume", bytes(64), "desktop-volume-selection-invalid"),
             ("crashed-compositor", b"/bin/compositor", faulting_service(original, b"/bin/compositor"),
              "desktop-session-failed"))
    for name, target, payload, reason in cases:
        modified = folder / f"{name}.iso"
        try:
            destination.write_bytes(rewrite_initramfs(original, target, payload))
            # Fresh El Torito metadata avoids replaying Limine's hybrid BIOS MBR.
            checked_run([xorriso, "-as", "mkisofs", "-R", "-r", "-J",
                         "-b", "boot/limine/limine-bios-cd.bin", "-no-emul-boot",
                         "-boot-load-size", "4", "-boot-info-table", "--efi-boot",
                         "boot/limine/limine-uefi-cd.bin", "-efi-boot-part",
                         "--efi-boot-image", "--protective-msdos-label",
                         str(tree), "-o", str(modified)])
        finally:
            destination.write_bytes(original)
        yield name, modified, "IndianOS recovery: reason=" + reason


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iso", type=Path, required=True)
    parser.add_argument("--disk", type=Path, required=True, help="preprovisioned golden fixture; never modified")
    parser.add_argument("--qemu", required=True)
    parser.add_argument("--ovmf", type=Path, required=True)
    parser.add_argument("--artifact-dir", type=Path, required=True)
    parser.add_argument("--xorriso", default="xorriso")
    parser.add_argument("--nm", default="nm")
    parser.add_argument("--fixture-tool", type=Path,
                        default=Path("tools/rust/target/release/indianos-desktop-fixture"),
                        help="actual COW writer for checksummed namespace rejection media")
    args = parser.parse_args()
    for path in (args.iso, args.disk, args.ovmf, args.fixture_tool):
        if not path.is_file():
            parser.error(f"missing artifact {path}")
    args.artifact_dir.mkdir(parents=True, exist_ok=True)
    # New artifact paths prevent prior runs from satisfying screenshot checks.
    artifacts = Path(tempfile.mkdtemp(prefix="run-", dir=args.artifact_dir)).resolve()
    print(f"Retaining normal desktop evidence in {artifacts}", flush=True)
    original = args.disk.read_bytes()
    verify_guard(original, original)
    # Keep logs and screenshots, but temporary mutable storage stays disposable.
    with tempfile.TemporaryDirectory(prefix="normal-desktop-disk-") as temporary:
        disk = Path(temporary) / "selected.raw"
        shutil.copyfile(args.disk, disk)
        for index in (0, 1):
            before = disk.read_bytes()
            boot(args.qemu, args.iso.resolve(), args.ovmf.resolve(), disk, artifacts, index)
            verify_guard(original, disk.read_bytes())
            if index == 1 and disk.read_bytes() != before:
                raise AssertionError("read-only reboot commands changed persistent storage")
        disk.write_bytes(original)
        abrupt_reboot(args.qemu, args.iso.resolve(), args.ovmf.resolve(), disk, artifacts)
        disk.write_bytes(original)
        transient_service_boot(args.qemu, args.iso.resolve(), args.ovmf.resolve(), disk,
                               artifacts, args.xorriso, args.nm)
        rejected_namespaces(args.qemu, args.iso.resolve(), args.ovmf.resolve(),
                            args.fixture_tool.resolve(), original, artifacts)
        corrupt = bytearray(original)
        # OVMF can repair one bad GPT header from its valid mirror before the
        # kernel starts. Corrupt both CRCs to test kernel fail-closed admission
        # rather than accidentally exercising firmware's automatic repair.
        for lba in (1, TOTAL - 1):
            corrupt[lba * 512 + 16] ^= 1
        cow = bytearray(original)
        for lba in (START, START + 1):
            cow[lba * 512 + 24] ^= 0x5a
        for name, payload in (("corrupt-gpt", bytes(corrupt)),
                              ("changed-guid", rotate_guid(original)),
                              ("corrupt-cow", bytes(cow))):
            disk.write_bytes(payload)
            denied_boot(args.qemu, args.iso.resolve(), args.ovmf.resolve(), disk,
                        artifacts, name)
        disk.write_bytes(original)
        for name, modified, reason in bad_service_images(args.iso.resolve(), Path(temporary), args.xorriso):
            if name == "crashed-compositor":
                crashed_service_boot(args.qemu, modified, args.ovmf.resolve(), disk, artifacts)
            else:
                denied_boot(args.qemu, modified, args.ovmf.resolve(), disk, artifacts, name, reason)
    print("PASS: normal four-service desktop, selected GPT persistence, physical input, guarded shutdown and corruption recovery")


if __name__ == "__main__":
    main()
