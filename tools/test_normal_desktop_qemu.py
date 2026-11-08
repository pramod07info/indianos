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
LIFECYCLE_READY = "IndianOS normal desktop: lifecycle READY verified"
DURABLE_SHUTDOWN = "IndianOS normal desktop: durable shutdown armed"
SURFACES_GUARDED = "IndianOS: desktop private surface guards verified"
RECOVERY = "IndianOS recovery: reason=desktop-storage-unavailable"
FORBIDDEN = (b"PID1 provisioned", b"R027 blank disk", b"normal PID1 ready:")
UART_ACTIONS = (PHYSICAL.encode(), SHUTDOWN.encode(), b"application launcher",
                b"user switched to", b"Terminal physical input acknowledged")
NAMED_COMMIT = "desktop broker committed named selected-volume mutation and refreshed Files"
NAMED_READBACK = "IndianOS user: R051 Terminal readback: saved"
G1_PAYLOAD = "violet"  # Visually distinct from the success status SAVED.
G1_READBACK = "IndianOS user: R051 Terminal readback: " + G1_PAYLOAD
TERMINAL_AUTHORITY = (
    b"IndianOS user: R117 QEMU PID2 stack guard and crossing descriptor pointers denied",
    b"IndianOS user: R114 PID2 ring3 invalid pointer, private path and denied write blocked",
    b"IndianOS user: R114 Terminal PID3 denied direct Files handles and directory syscall",
    b"IndianOS user: desktop PID1 denied compositor window authority",
    b"IndianOS user: desktop app denied compositor and physical input authority",
    b"IndianOS user: desktop compositor denied input and filesystem authority",
    LIFECYCLE_READY.encode(),
    SURFACES_GUARDED.encode(),
)

# Independent expected raster contract for the qualified normal desktop.
# Do not import the production renderer: a shared rendering bug must not make
# the image and its oracle agree. Lowercase intentionally uses uppercase glyphs.
GLYPHS = {
    "A": (14,17,17,31,17,17,17), "B": (30,17,17,30,17,17,30),
    "C": (14,17,16,16,16,17,14), "D": (30,17,17,17,17,17,30),
    "E": (31,16,16,30,16,16,31), "F": (31,16,16,30,16,16,16),
    "G": (14,17,16,23,17,17,14), "H": (17,17,17,31,17,17,17),
    "I": (31,4,4,4,4,4,31), "J": (7,2,2,2,18,18,12),
    "K": (17,18,20,24,20,18,17), "L": (16,16,16,16,16,16,31),
    "M": (17,27,21,21,17,17,17), "N": (17,25,21,19,17,17,17),
    "O": (14,17,17,17,17,17,14), "P": (30,17,17,30,16,16,16),
    "Q": (14,17,17,17,21,18,13), "R": (30,17,17,30,20,18,17),
    "S": (15,16,16,14,1,1,30), "T": (31,4,4,4,4,4,4),
    "U": (17,17,17,17,17,17,14), "V": (17,17,17,17,17,10,4),
    "W": (17,17,17,21,21,21,10), "X": (17,17,10,4,10,17,17),
    "Y": (17,17,10,4,4,4,4), "Z": (31,1,2,4,8,16,31),
    "0": (14,17,19,21,25,17,14), "1": (4,12,4,4,4,4,14),
    "2": (14,17,1,2,4,8,31), "3": (30,1,1,14,1,1,30),
    "4": (2,6,10,18,31,2,2), "5": (31,16,30,1,1,17,14),
    "6": (6,8,16,30,17,17,14), "7": (31,1,2,4,8,8,8),
    "8": (14,17,17,14,17,17,14), "9": (14,17,17,15,1,2,12),
    " ": (0,0,0,0,0,0,0), ":": (0,4,4,0,4,4,0),
}


def text_field_bounds(image: tuple[int, int, bytes], field: str) -> tuple[int, int, int]:
    width, height, pixels = image
    if width < 640 or height < 480 or len(pixels) != width * height * 3:
        raise AssertionError("invalid qualified framebuffer geometry")
    window_width = min(max(width * 49 // 100, 320), width - 80)
    window_height = min(max(height * 50 // 100, 240), height - 110)
    if field == "terminal":
        x, y, count = width - window_width - 36 + 9 + 24, height - window_height - 65 + 44 + 181, 270
    elif field == "files-directory":
        x, y, count = 45 + 9 + 18, 75 + 44 + 163, 264
    elif field == "files-note":
        x, y, count = 45 + 9 + 18, 75 + 44 + 190, 240
    else:
        raise AssertionError("unknown graphical text field")
    if x < 0 or y < 0 or x + count > width or y + 14 > height:
        raise AssertionError("graphical text field exceeds framebuffer")
    return x, y, count


def verify_text_pixels(image: tuple[int, int, bytes], field: str, expected: str,
                       offset: tuple[int, int] = (0, 0)) -> None:
    """Require exact glyph pixels and cleared trailing cells in a fresh shot.

    Callers acquire the shot only after a fresh physical-key acknowledgement.
    Comparing just changed pixels or a SAVED label cannot prove file readback.
    """
    x, y, count = text_field_bounds(image, field)
    x, y = x + offset[0], y + offset[1]
    if x < 0 or y < 0 or x + count > image[0] or y + 14 > image[1]:
        raise AssertionError("translated graphical text field exceeds framebuffer")
    if not expected or not expected.isascii() or len(expected) * 12 > count:
        raise AssertionError("unsupported graphical text expectation")
    text = expected.upper()
    if any(char not in GLYPHS for char in text):
        raise AssertionError("unsupported graphical glyph expectation")
    width, _, pixels = image
    for row in range(14):
        for column in range(count):
            character, inside = divmod(column, 12)
            ink = (character < len(text) and inside < 10 and
                   GLYPHS[text[character]][row // 2] & (1 << (4 - inside // 2)) != 0)
            offset = ((y + row) * width + x + column) * 3
            white = pixels[offset:offset + 3] == b"\xff\xff\xff"
            if white != ink:
                raise AssertionError(f"{field} pixels do not display exact {expected!r}")


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
    if not 0 < shutdown_at < len(output):
        raise AssertionError("invalid normal-desktop shutdown evidence boundary")
    # Require complete records: a truncated line or a containing diagnostic is
    # not evidence that a native service actually reached the transition.
    lines = output.replace(b"\r", b"").split(b"\n")[:-1]
    # The physical console echoes Q before the shutdown syscall logs its
    # result. Accept that single known echo, never an arbitrary log prefix.
    lines = [line[1:] if line == b"q" + DURABLE_SHUTDOWN.encode() else line for line in lines]
    for marker in (COMPOSITOR, LIFECYCLE_READY, READY, DURABLE_SHUTDOWN, STOPPED):
        if lines.count(marker.encode()) != 1:
            raise AssertionError("desktop lifecycle lacks an exact one-shot transition")
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
    startup = prefix[:prefix.find(READY.encode())]
    lifecycle = startup.find(LIFECYCLE_READY.encode())
    if not 0 <= startup.find(COMPOSITOR.encode()) < lifecycle:
        raise AssertionError("kernel readiness preceded compositor readiness")
    if any(marker not in startup for marker in TERMINAL_AUTHORITY):
        raise AssertionError("Terminal startup lacks actual syscall authority negative controls")
    for pid in (1, 2, 3, 4):
        for state in ("READY", "DISPATCHED"):
            if f"IndianOS: PID {pid} {state}".encode() not in prefix:
                raise AssertionError("desktop readiness lacks actual process execution")
        if startup.find(f"IndianOS: PID {pid} DISPATCHED".encode()) > lifecycle:
            raise AssertionError("kernel readiness preceded actual service execution")
    tail = output[shutdown_at:]
    synced = tail.find(b"desktop selected volume synchronized")
    armed = tail.find(DURABLE_SHUTDOWN.encode())
    shutdown = tail.find(SHUTDOWN.encode())
    if armed < 0 or synced < 0 or shutdown < 0 or not armed < synced < shutdown:
        raise AssertionError("desktop shutdown preceded a verified storage barrier")
    for pid in (1, 2, 3, 4):
        exited = f"IndianOS: PID {pid} EXITED".encode()
        reaped = f"IndianOS: PID {pid} REAPED".encode()
        if lines.count(exited) != 1 or lines.count(reaped) != 1:
            raise AssertionError("normal desktop did not reclaim every service")
        if not armed < tail.find(exited) < tail.find(reaped) < tail.find(STOPPED.encode()):
            raise AssertionError("normal desktop exited outside its durable shutdown sequence")
    if b"FAULTED" in output or b"service exited before durable shutdown" in output:
        raise AssertionError("healthy desktop contained a critical service failure")


def verify_suspended_editor(before: tuple[int, int, bytes], after: tuple[int, int, bytes],
                            output: bytes, since: int) -> None:
    marker = b"IndianOS user: desktop Terminal editor suspended outside keyboard focus"
    if not 0 <= since < len(output):
        raise AssertionError("invalid suspended-editor evidence boundary")
    lines = output[since:].replace(b"\r", b"").split(b"\n")[:-1]
    lines = [line[1:] if line == b"z" + marker else line for line in lines]
    if lines.count(marker) != 1:
        raise AssertionError("Files focus did not suspend Terminal keyboard delivery")
    if ACK.encode() in output[since:] or SHUTDOWN.encode() in output[since:]:
        raise AssertionError("unfocused editor received a key or initiated shutdown")
    if before != after:
        raise AssertionError("unfocused editor input changed the framebuffer")


def physical_focus(session: QemuSession, previous: tuple[int, int], target: tuple[int, int],
                   app: str) -> tuple[int, int]:
    # HMP relative motion enters the emulated 8042 auxiliary port. The guest
    # must route it through PID1 IPC and PID4's hit testing before acknowledging.
    start = len(session.output)
    session.hmp(f"mouse_move {target[0] - previous[0]} {target[1] - previous[1]}")
    session.drain_serial(0.2)
    session.hmp("mouse_button 1")
    session.drain_serial(0.2)
    session.hmp("mouse_button 0")
    session.until("IndianOS user: desktop physical focus: " + app, 20, since=start)
    return target


def frame_region(image: tuple[int, int, bytes], bounds: tuple[int, int, int, int]) -> bytes:
    width, height, pixels = image
    left, top, right, bottom = bounds
    if (len(pixels) != width * height * 3 or
            not 0 <= left < right <= width or not 0 <= top < bottom <= height):
        raise AssertionError("invalid framebuffer evidence region")
    return b"".join(pixels[(y * width + left) * 3:(y * width + right) * 3]
                    for y in range(top, bottom))


def verify_minimize_restore(before: tuple[int, int, bytes], minimized: tuple[int, int, bytes],
                            restored: tuple[int, int, bytes]) -> None:
    if before[:2] != minimized[:2] or before[:2] != restored[:2]:
        raise AssertionError("window lifecycle changed framebuffer dimensions")
    width, height = before[:2]
    # This client strip is outside Files, titlebars and the stationary pointer.
    bounds = (width - 160, height - 180, width - 80, height - 100)
    original = frame_region(before, bounds)
    if frame_region(minimized, bounds) == original:
        raise AssertionError("minimized Terminal still occupies its client region")
    if frame_region(restored, bounds) != original:
        raise AssertionError("restored Terminal did not recover its retained client pixels")


def pointer_window_acceptance(session: QemuSession, folder: Path, index: int,
                              initial: tuple[int, int, bytes]) -> None:
    width, height = initial[:2]
    expected = "HELLO INDIANOS" if index == 0 else PAYLOAD
    physical_key(session, "2", "R031.1 user switched to Terminal app",
                 folder, index, 20, initial, True)
    # Capture Files' titlebar, move it, release, then move without a press.
    # Exact translated file glyphs prove window movement rather than cursor
    # repaint; their stable position after release proves capture ended.
    physical_focus(session, (width // 2, height // 2), (100, 90), "Files")
    session.hmp("mouse_button 1")
    session.drain_serial(0.2)
    session.hmp("mouse_move 40 20")
    session.drain_serial(0.3)
    session.hmp("mouse_button 0")
    session.drain_serial(0.2)
    moved = session.shot(folder / f"desktop-{index}-pointer-drag.ppm")
    verify_text_pixels(moved, "files-note", expected, (40, 20))
    session.hmp("mouse_move 40 40")
    session.drain_serial(0.3)
    released = session.shot(folder / f"desktop-{index}-pointer-release.ppm")
    verify_text_pixels(released, "files-note", expected, (40, 20))
    session.hmp("mouse_move -40 -40")
    session.drain_serial(0.2)
    session.hmp("mouse_button 1")
    session.drain_serial(0.2)
    session.hmp("mouse_move -40 -20")
    session.drain_serial(0.3)
    session.hmp("mouse_button 0")
    session.drain_serial(0.2)
    # Restore the initial pointer so subsequent focus tests have one known
    # relative-coordinate origin, without bypassing the real mouse device.
    session.hmp(f"mouse_move {width // 2 - 100} {height // 2 - 90}")
    session.drain_serial(0.3)
    restored = session.shot(folder / f"desktop-{index}-pointer-drag-restored.ppm")
    verify_text_pixels(restored, "files-note", expected)
    acknowledged_key(session, "2", "R031.1 user switched to Terminal app")
    visible = session.shot(folder / f"desktop-{index}-before-minimize.ppm")
    terminal_height = min(max(height * 50 // 100, 240), height - 110)
    title = (width - 100, height - terminal_height - 50)
    session.hmp(f"mouse_move {title[0] - width // 2} {title[1] - height // 2}")
    session.drain_serial(0.2)
    session.hmp("mouse_button 1")
    session.drain_serial(0.2)
    acknowledged_key(session, "m", "R031.1 Terminal minimized")
    # Minimize cancels the captured title drag. A still-held motion cannot
    # transfer that capture to Files or restore/move the hidden Terminal.
    session.hmp("mouse_move 40 20")
    session.drain_serial(0.3)
    minimized = session.shot(folder / f"desktop-{index}-minimized.ppm")
    verify_text_pixels(minimized, "files-note", expected)
    session.hmp("mouse_button 0")
    session.drain_serial(0.2)
    acknowledged_key(session, "r", "R031.1 Terminal restored")
    restored = session.shot(folder / f"desktop-{index}-restored.ppm")
    verify_minimize_restore(visible, minimized, restored)
    session.hmp(f"mouse_move {width // 2 - title[0] - 40} {height // 2 - title[1] - 20}")
    session.drain_serial(0.3)


def pointer_focus_acceptance(session: QemuSession, folder: Path, index: int,
                             editing: tuple[int, int, bytes]) -> tuple[int, int, bytes]:
    # Keep CAT pending while Files owns focus. An erroneously delivered Z
    # changes the command to CATZ and cannot produce the exact readback below.
    type_command(session, "cat")
    width, height = editing[:2]
    pointer = physical_focus(session, (width // 2, height // 2), (100, 135), "Files")
    focused = session.shot(folder / f"desktop-{index}-pointer-files.ppm")
    changed(editing, focused, "physical pointer Files focus")
    since = len(session.output)
    session.hmp("sendkey z 80")
    session.until("desktop Terminal editor suspended outside keyboard focus", 20, since=since)
    blocked = session.shot(folder / f"desktop-{index}-pointer-key-blocked.ppm")
    verify_suspended_editor(focused, blocked, bytes(session.output), since)
    physical_focus(session, pointer, (width - 100, height - 150), "Terminal")
    restored = session.shot(folder / f"desktop-{index}-pointer-terminal.ppm")
    changed(blocked, restored, "physical pointer Terminal focus")
    submitted = acknowledged_key(session, "ret")
    expected = "HELLO INDIANOS" if index == 0 else PAYLOAD
    readback = "IndianOS user: R051 Terminal readback: " + expected
    session.until(readback, 20, since=submitted)
    session.drain_serial(0.1)
    exact_terminal_line(bytes(session.output), submitted, readback)
    result = session.shot(folder / f"desktop-{index}-pointer-pending-readback.ppm")
    verify_text_pixels(result, "terminal", expected)
    verify_text_pixels(result, "files-note", expected)
    return result


def verify_uart_ignored(before: tuple[int, int, bytes], after: tuple[int, int, bytes],
                        output: bytes, since: int) -> None:
    if any(marker in output[since:] for marker in UART_ACTIONS):
        raise AssertionError("UART injection operated the physical-only desktop")
    if before[:2] != after[:2] or differences(before, after):
        raise AssertionError("UART injection changed the desktop framebuffer")


def spawn(qemu: str, iso: Path, ovmf: Path, disk: Path, folder: Path, name: str,
          debugger: bool = False, *, write_fault: Path | None = None):
    # AF_UNIX paths include the whole checkout/artifact prefix. GitHub's
    # longer prefix made partition-protective-overflow.sock exceed 108 bytes.
    # Keep both sockets private and short, independent of artifact names.
    socket_directory = tempfile.TemporaryDirectory(prefix="indianos-qemu-", dir="/tmp")
    monitor = Path(socket_directory.name) / "monitor.sock"
    debugger_path = Path(socket_directory.name) / "gdb.sock"
    image = f"blkdebug:{write_fault}:{disk}" if write_fault is not None else str(disk)
    command = [qemu, "-machine", "pc", "-m", "256M", "-smp", "1",
               "-display", "none", "-vga", "std", "-serial", "stdio",
               "-monitor", f"unix:{monitor},server=on,wait=off", "-no-reboot",
               "-no-shutdown", "-boot", "d",
               "-drive", f"if=pflash,format=raw,readonly=on,file={ovmf}",
               "-cdrom", str(iso),
               "-drive", f"if=ide,index=0,media=disk,format=raw,file={image}"]
    if debugger:
        command.extend(["-gdb", f"unix:{debugger_path},server=on,wait=off"])
    process = None
    sock = None
    try:
        process = subprocess.Popen(command, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT, bufsize=0)
        sock = open_monitor(monitor, process)
        session = QemuSession(process, sock)
        session.socket_directory = socket_directory
        session.debugger_path = debugger_path
        return process, sock, session
    except BaseException as error:
        output = b""
        try:
            if sock is not None:
                sock.close()
            if process is not None:
                if process.poll() is None:
                    process.kill()
                output, _ = process.communicate(timeout=5)
            # Startup diagnostics used to disappear behind the generic
            # monitor assertion. Preserve QEMU's actual error even here.
            (folder / f"{name}.serial.log").write_bytes(
                output or f"QEMU startup failed: {error}\n".encode())
        finally:
            socket_directory.cleanup()
        raise


def close(process, sock, session: QemuSession, output: Path) -> None:
    try:
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
        # Preserve trailing fault diagnostics before removing socket files.
        output.write_bytes(session.output)
    finally:
        session.socket_directory.cleanup()


def boot(qemu: str, iso: Path, ovmf: Path, disk: Path, folder: Path, index: int) -> None:
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, f"desktop-{index}")
    try:
        session.until("IndianOS normal desktop: selected storage admitted", 45)
        session.until(COMPOSITOR, 20)
        session.until(READY, 20)
        session.until("R031.9 real file and directory contents rendered into Files window", 20)
        frame = session.shot(folder / f"desktop-{index}-initial.ppm")
        check_two_windows(frame)
        verify_text_pixels(frame, "files-directory",
                           "f:readme " if index == 0 else "f:readme f:a d:docs ")
        verify_text_pixels(frame, "files-note", "HELLO INDIANOS" if index == 0 else PAYLOAD)
        if PHYSICAL.encode() in session.output:
            raise AssertionError("physical-origin evidence appeared before physical input")
        # Real UART bytes must not be accepted as physical desktop commands.
        serial_at = len(session.output)
        session.serial(b"qaa21")
        session.drain_serial(0.5)
        serial_frame = session.shot(folder / f"desktop-{index}-uart-negative.ppm")
        verify_uart_ignored(frame, serial_frame, bytes(session.output), serial_at)
        pointer_window_acceptance(session, folder, index, frame)
        # The window lifecycle scenario leaves Terminal active. Restore Files
        # so each existing keyboard transition has an observable focus change.
        acknowledged_key(session, "1", "R031.1 user switched to Files app")
        frame = session.shot(folder / f"desktop-{index}-window-lifecycle-complete.ppm")
        actions = [("a", "R031.1 application launcher OPEN"),
                   ("a", "R031.1 application launcher CLOSED"),
                   ("2", "R031.1 user switched to Terminal app"),
                   ("1", "R031.1 user switched to Files app"),
                   ("2", "R031.1 user switched to Terminal app")]
        for step, (key, marker) in enumerate(actions):
            frame = physical_key(session, key, marker, folder, index, step, frame, False)
        acknowledged_key(session, "1", "R031.1 user switched to Files app")
        frame = session.shot(folder / f"desktop-{index}-files-focus.ppm")
        frame = physical_key(session, "2", "R031.1 user switched to Terminal app",
                             folder, index, 8, frame, False)
        acknowledged_key(session, "ret")
        editing = session.shot(folder / f"desktop-{index}-editor.ppm")
        changed(frame, editing, "normal Terminal activation")
        pointer_focus_acceptance(session, folder, index, editing)
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
        verify_text_pixels(result, "terminal", "SAVED" if index == 0 else PAYLOAD)
        verify_text_pixels(result, "files-note", PAYLOAD)
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
            for step, (command, listing, structural) in enumerate((
                    ("touch tmp", "f:readme f:tmp ", True),
                    ("put tmp=" + G1_PAYLOAD, "f:readme f:tmp ", False),
                    ("mv tmp a", "f:readme f:a ", True),
                    ("touch gone", "f:readme f:a f:gone ", True),
                    ("rm gone", "f:readme f:a ", True),
                    ("mkdir docs", "f:readme f:a d:docs ", True))):
                type_named_command(session, command)
                before = session.shot(folder / f"desktop-{index}-named-{step}-typed.ppm")
                submitted = acknowledged_key(session, "ret")
                session.until(NAMED_COMMIT, 20, since=submitted)
                result = session.shot(folder / f"desktop-{index}-named-{step}-result.ppm")
                verify_text_pixels(result, "terminal", "SAVED")
                verify_text_pixels(result, "files-directory", listing)
                verify_text_pixels(result, "files-note", PAYLOAD)
                if structural:
                    files_repainted(before, result)
        type_named_command(session, "cat a")
        typed = session.shot(folder / f"desktop-{index}-named-readback-typed.ppm")
        submitted = acknowledged_key(session, "ret")
        session.until(G1_READBACK, 20, since=submitted)
        session.drain_serial(0.1)
        exact_terminal_line(bytes(session.output), submitted, G1_READBACK)
        result = session.shot(folder / f"desktop-{index}-named-readback.ppm")
        changed(typed, result, "named persistent-file graphical readback")
        verify_text_pixels(result, "terminal", G1_PAYLOAD)
        verify_text_pixels(result, "files-directory", "f:readme f:a d:docs ")
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
            frame = session.shot(folder / f"abrupt-{attempt}-terminal.ppm")
            verify_text_pixels(frame, "terminal", "SAVED" if attempt == 0 else "saved")
            verify_text_pixels(frame, "files-directory", "f:readme f:cut ")
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
                reason: str = RECOVERY, *, write_fault: Path | None = None) -> None:
    before = hashlib.sha256(disk.read_bytes()).digest()
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, name, write_fault=write_fault)
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


def write_failure_config(folder: Path) -> Path:
    path = folder / "reject-storage-writes.ini"
    path.write_text('[inject-error]\nevent = "write_aio"\nerrno = "5"\n'
                    'once = "off"\nimmediately = "on"\n')
    return path


def rejected_partitions(qemu: str, iso: Path, ovmf: Path, fixture_tool: Path,
                        folder: Path) -> None:
    fault = write_failure_config(folder)
    for kind in ("overlap", "duplicate-id", "protective-overflow"):
        disk = folder / f"partition-{kind}.raw"
        checked_run([str(fixture_tool), "--partition-fault", kind, "--output", str(disk)])
        # The protected backend prevents firmware from normalizing malformed
        # metadata before the kernel can inspect it. A valid-device positive
        # control uses the same backend in failed_write_boot below.
        denied_boot(qemu, iso, ovmf, disk, folder, f"partition-{kind}", write_fault=fault)


def verify_failed_write(output: bytes, since: int) -> None:
    if not 0 <= since < len(output):
        raise AssertionError("invalid failed-write evidence boundary")
    tail = output[since:]
    if b"desktop shutdown refused: selected volume sync failed" not in tail:
        raise AssertionError("uncertain storage did not refuse shutdown sync")
    if any(marker in tail for marker in (NAMED_COMMIT.encode(), COMMITTED.encode(),
                                         SHUTDOWN.encode(), STOPPED.encode())):
        raise AssertionError("failed storage mutation was advertised as durable")


def failed_write_boot(qemu: str, iso: Path, ovmf: Path, disk: Path, folder: Path) -> None:
    before = disk.read_bytes()
    fault = write_failure_config(folder)
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, "failed-ata-write",
                                   write_fault=fault)
    try:
        # This positive control proves the backend permits IDENTIFY, GPT/COW
        # reads and a genuine ring3 desktop; rejection is not a missing drive.
        session.until(READY, 45)
        acknowledged_key(session, "2", "R031.1 user switched to Terminal app")
        acknowledged_key(session, "ret")
        since = len(session.output)
        type_named_command(session, "put readme=blocked")
        acknowledged_key(session, "ret")
        verify_text_pixels(session.shot(folder / "failed-ata-write-error.ppm"), "terminal", "IO ERROR")
        # Reads would succeed on the underlying device. Refusing this read
        # shows the uncertain selected mount was retired rather than retried.
        type_named_command(session, "cat readme")
        acknowledged_key(session, "ret")
        verify_text_pixels(session.shot(folder / "failed-ata-read-retired.ppm"), "terminal", "IO ERROR")
        acknowledged_key(session, "tab")
        acknowledged_key(session, "q", "desktop shutdown refused: selected volume sync failed")
        session.drain_serial(0.1)
        verify_failed_write(bytes(session.output), since)
    finally:
        close(process, sock, session, folder / "failed-ata-write.serial.log")
    if disk.read_bytes() != before:
        raise AssertionError("failed ATA write or shutdown changed selected media")
    print("Normal desktop actual ATA write failure: IO pixels, retired mount, refused sync and unchanged media PASS")



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
    return service_entry_code(raw, target, b"\x0f\x0b")


def service_entry_code(raw: bytes, target: bytes, code: bytes) -> bytes:
    """Patch only executable entry bytes of an otherwise unchanged native ELF.

    These are disposable adversarial service images, not kernel test hooks.
    Every archive bound and duplicate is checked before using an entry.
    """
    if len(raw) < 16 or raw[:8] != b"INDRAM01" or not 1 <= len(code) <= 32:
        raise AssertionError("invalid executable fault source")
    count, total = struct.unpack_from("<II", raw, 8)
    if not 1 <= count <= 8 or total != len(raw):
        raise AssertionError("invalid executable fault archive bounds")
    offset = 16
    entries = {}
    for _ in range(count):
        if offset + 8 > len(raw):
            raise AssertionError("truncated executable fault archive")
        size, _, length = struct.unpack_from("<HHI", raw, offset)
        reserved = struct.unpack_from("<H", raw, offset + 2)[0]
        offset += 8
        if reserved or not 1 <= size <= 63 or offset + size + length > len(raw):
            raise AssertionError("invalid executable fault entry bounds")
        name = raw[offset:offset + size]
        if name in entries:
            raise AssertionError("duplicate executable fault entry")
        entries[name] = raw[offset + size:offset + size + length]
        offset += size + length
    if offset != len(raw) or target not in entries:
        raise AssertionError("fault service is missing or source has trailing bytes")
    data = bytearray(entries[target])
    if data[:7] != b"\x7fELF\x02\x01\x01" or len(data) < 64:
        raise AssertionError("fault source is not a native ELF")
    entry, phoff = struct.unpack_from("<QQ", data, 24)
    stride, count = struct.unpack_from("<HH", data, 54)
    if stride != 56 or not 1 <= count <= 64 or phoff + count * stride > len(data):
        raise AssertionError("fault source program headers are malformed")
    for index in range(count):
        at = phoff + index * stride
        kind, flags, file_offset, virtual = struct.unpack_from("<IIQQ", data, at)
        size = struct.unpack_from("<Q", data, at + 32)[0]
        if kind == 1 and flags & 1 and virtual <= entry and entry + len(code) <= virtual + size:
            start = file_offset + entry - virtual
            if start + len(code) > len(data):
                raise AssertionError("fault entry exceeds the ELF file")
            data[start:start + len(code)] = code
            return bytes(data)
    raise AssertionError("fault source has no file-backed executable entry")


# Syscall2 exits with status0. UD2 catches an erroneous kernel return instead
# of allowing unpatched original entry code to advertise readiness.
PREMATURE_EXIT = b"\xb8\x02\x00\x00\x00\x31\xff\x31\xf6\xcd\x80\x0f\x0b"
# Syscall3 cooperatively yields forever. Reset RAX each turn because the
# successful yield return is0; the short branch loops all13 instructions bytes.
UNREADY_YIELD = b"\xb8\x03\x00\x00\x00\x31\xff\x31\xf6\xcd\x80\xeb\xf3"


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


def surface_guard_records(raw: bytes) -> list[tuple[int, int, int, int]]:
    if len(raw) != 64:
        raise AssertionError("surface guard ownership ledger is not 64 bytes")
    words = struct.unpack("<8Q", raw)
    records = [tuple(words[start:start + 4]) for start in (0, 4)]
    for lower, begin, end, upper in records:
        if (lower < 0xffff800000000000 or lower % 4096 or
                begin != lower + 4096 or end != begin + 4096 or
                upper != end or upper + 4096 > 1 << 64):
            raise AssertionError("invalid private surface guard geometry")
    ordered = sorted(records)
    if ordered[0][3] + 4096 > ordered[1][0]:
        raise AssertionError("private surface guard objects overlap")
    return records


def verify_debug_surface_guards(debugger: GuestDebugger, records: list[tuple[int, int, int, int]],
                                evidence: list[str], phase: str) -> None:
    for index, (lower, begin, end, upper) in enumerate(records):
        for guard in (lower, upper):
            if not debugger.command(f"m{guard:x},1").startswith("E"):
                raise AssertionError("private app surface guard remains readable")
        debugger.memory(begin, 1)
        debugger.memory(end - 1, 1)
        evidence.append(f"{phase}: surface{index + 1} guards {lower:#x}/{upper:#x} unreadable; payload {begin:#x}..{end:#x} readable")


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
               SURFACES_GUARDED.encode(),
               b"desktop compositor denied input and filesystem authority",
               COMPOSITOR.encode(), LIFECYCLE_READY.encode(), READY.encode())
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


def verify_unready_recovery(output: bytes, name: str) -> None:
    if name == "premature-compositor":
        failure = b"IndianOS normal desktop: service exited before durable shutdown"
        session = (b"IndianOS: R031 scheduler process exit=0x0000000000000000",
                   b"IndianOS: PID 4 EXITED", b"IndianOS: PID 4 REAPED", failure)
        for marker in session:
            if output.count(marker) != 2:
                raise AssertionError("premature clean-exit scenario lacks exact actual lifecycle")
        if b"PID 4 FAULTED" in output:
            raise AssertionError("premature exit returned to UD2 instead of exiting")
    elif name == "unready-compositor":
        failure = b"IndianOS normal desktop: startup dispatch budget exhausted"
        session = (failure,)
        if output.count(failure) != 2:
            raise AssertionError("unready service did not exhaust both startup budgets")
    else:
        raise AssertionError("unknown unready-service scenario")
    verified = b"IndianOS normal desktop: retired session verified restart-safe"
    restart = b"IndianOS normal desktop: restarting retired service session"
    exhausted = b"IndianOS normal desktop: restart budget exhausted"
    recovery = b"IndianOS recovery: reason=desktop-session-failed"
    for marker, count in ((verified, 2), (restart, 1), (exhausted, 1), (recovery, 1)):
        if output.count(marker) != count:
            raise AssertionError("unready service bypassed bounded rollback/recovery")
    cursor = 0
    for marker in (*session, verified, restart, *session, verified, exhausted, recovery):
        position = output.find(marker, cursor)
        if position < 0:
            raise AssertionError("unready-service lifecycle is out of order")
        cursor = position + len(marker)
    if any(marker in output for marker in (READY.encode(), LIFECYCLE_READY.encode(),
                                           DURABLE_SHUTDOWN.encode(), b"FATAL")):
        raise AssertionError("unready service was accepted or escaped controlled recovery")


def unready_service_boot(qemu: str, iso: Path, ovmf: Path, disk: Path,
                         folder: Path, name: str) -> None:
    before = disk.read_bytes()
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, name)
    try:
        session.until("IndianOS recovery: reason=desktop-session-failed", 45)
        session.until("IndianOS recovery: read-only serial console ready", 10)
        verify_unready_recovery(bytes(session.output), name)
    finally:
        close(process, sock, session, folder / f"{name}.serial.log")
    if disk.read_bytes() != before:
        raise AssertionError("unready-service recovery modified selected storage")
    print(f"Normal desktop {name}: bounded actual failure, complete retirement and unchanged recovery PASS")


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
    surface_ledger = unique_symbol(nm, kernel, ("DESKTOP_SURFACE_LAYOUT",))
    symbols = subprocess.run([nm, "-S", "-n", "-C", str(kernel)], check=True,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
    guards = stack_guard_records(symbols.stdout.decode("ascii"))
    unmapped = 0x60000000
    original = disk.read_bytes()
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, "transient-compositor", debugger=True)
    path = session.debugger_path
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
        surface_layout = debugger.memory(surface_ledger, 64)
        surfaces = surface_guard_records(surface_layout)
        verify_debug_surface_guards(debugger, surfaces, evidence, "PID4 cloned root")
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
        readback = session.shot(folder / "transient-saved-readback.ppm")
        verify_text_pixels(readback, "terminal", "saved")
        verify_text_pixels(readback, "files-directory", "f:readme f:revived ")
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
        if debugger.memory(surface_ledger, 64) != surface_layout:
            raise AssertionError("desktop restart changed source-owned private surface identity")
        verify_debug_surface_guards(debugger, surfaces, evidence, "reclaimed recovery kernel")
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
              "desktop-session-failed"),
             ("premature-compositor", b"/bin/compositor", service_entry_code(original, b"/bin/compositor", PREMATURE_EXIT),
              "desktop-session-failed"),
             ("unready-compositor", b"/bin/compositor", service_entry_code(original, b"/bin/compositor", UNREADY_YIELD),
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
        rejected_partitions(args.qemu, args.iso.resolve(), args.ovmf.resolve(),
                            args.fixture_tool.resolve(), artifacts)
        disk.write_bytes(original)
        failed_write_boot(args.qemu, args.iso.resolve(), args.ovmf.resolve(), disk, artifacts)
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
            elif name in ("premature-compositor", "unready-compositor"):
                unready_service_boot(args.qemu, modified, args.ovmf.resolve(), disk, artifacts, name)
            else:
                denied_boot(args.qemu, modified, args.ovmf.resolve(), disk, artifacts, name, reason)
    print("PASS: normal four-service desktop, selected GPT persistence, physical input, guarded shutdown and corruption recovery")


if __name__ == "__main__":
    main()
