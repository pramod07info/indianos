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
import json
from pathlib import Path
import re
import socket
import shutil
import subprocess
import struct
import tempfile
import time
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
BOOT_STACK = b"IndianOS normal desktop: explicit 256 KiB boot stack acknowledged"
IRQ_INGRESS = (b"IndianOS: G5 normal IRQ1 delivered",
               b"IndianOS: G5 normal IRQ12 delivered",
               b"IndianOS: G5 normal IRQ keyboard data queued",
               b"IndianOS: G5 normal IRQ mouse packet queued")
RECOVERY = "IndianOS recovery: reason=desktop-storage-unavailable"
FORBIDDEN = (b"PID1 provisioned", b"R027 blank disk", b"normal PID1 ready:")
UART_ACTIONS = (PHYSICAL.encode(), SHUTDOWN.encode(), b"application launcher",
                b"user switched to", b"Terminal physical input acknowledged")
NAMED_COMMIT = "desktop broker committed named selected-volume mutation and refreshed Files"
LEASE_ACQUIRED = b"IndianOS user: desktop Files read lease acquired"
LEASE_REFRESH = b"IndianOS user: desktop Files retained read lease verified after refresh"
LEASE_CURSOR = b"IndianOS user: desktop Files positioned read preserved cursor"
POSITIONED_WRITE = b"IndianOS user: desktop broker positioned write committed"
POSITIONED_DENIED = b"IndianOS user: desktop Terminal denied direct positioned filesystem IO"
LEASE_RELEASED = b"IndianOS user: desktop Files read lease released"
LEASE_DENIED = b"IndianOS user: desktop Terminal filesystem request failed"
LEASE_CONTROL_ACK = "desktop Files read lease control acknowledged"
NAMED_READBACK = "IndianOS user: R051 Terminal readback: saved"
G1_PAYLOAD = "violet"  # Visually distinct from the success status SAVED.
G1_READBACK = "IndianOS user: R051 Terminal readback: " + G1_PAYLOAD
COMPOSITOR_SNAPSHOT = b"IndianOS user: desktop PID4 composed a private row from committed client snapshots"
FRAME_ALLOCATED = b"IndianOS: kernel frame buffer allocated"
FRAME_RELEASED = b"IndianOS: kernel frame buffer released"
FRAME_PRESENTED = b"IndianOS: G4 complete PID4 frame presented"
FRAME_PUBLISHED = b"IndianOS user: desktop PID4 full frame published"
FRAME_METRICS = b"IndianOS user: desktop PID4 frame metrics "
COMPOSITOR_PREPARED = b"IndianOS: G4 compositor-only replacement prepared"
COMPOSITOR_REPLACED = b"IndianOS: G4 compositor-only replacement ready"
STALE_COMPOSITOR_DENIED = b"IndianOS user: desktop PID4 stale frame authority denied"
PIXEL_APP_EVIDENCE = tuple(
    marker
    for pid, app in ((2, "Files"), (3, "Terminal"))
    for marker in (
        f"IndianOS: PID {pid} PIXEL_GUARDS_READY".encode(),
        f"IndianOS user: desktop {app} private pixel scratch guards verified".encode(),
        f"IndianOS user: desktop {app} pixel authority and transaction denials verified".encode(),
        f"IndianOS user: desktop {app} ring3 raster pixels committed".encode(),
    )
) + (
    b"IndianOS user: desktop broker denied app pixel authority",
    b"IndianOS user: desktop compositor denied app pixel authority",
    COMPOSITOR_SNAPSHOT,
    FRAME_ALLOCATED,
    FRAME_PRESENTED, FRAME_PUBLISHED,
)
TERMINAL_AUTHORITY = PIXEL_APP_EVIDENCE + (
    POSITIONED_DENIED,
    b"IndianOS user: R117 QEMU PID2 stack guard and crossing descriptor pointers denied",
    b"IndianOS user: R114 PID2 ring3 invalid pointer, private path and denied write blocked",
    b"IndianOS user: R114 Terminal PID3 denied direct Files handles and directory syscall",
    b"IndianOS user: desktop PID1 denied compositor window authority",
    b"IndianOS user: desktop app denied compositor and physical input authority",
    b"IndianOS user: desktop compositor denied input and filesystem authority",
    LIFECYCLE_READY.encode(),
    SURFACES_GUARDED.encode(),
    b"IndianOS: desktop retained pixel surface guards verified",
    b"IndianOS user: desktop Files verified filesystem ABI and owned range-lock lifecycle",
    b"IndianOS user: desktop Files denied unauthorized permission mutation",
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
    window_width = min(max(width * 49 // 100, 338), 786, width - 80)
    window_height = min(max(height * 50 // 100, 294), 822, height - 110)
    if field == "terminal":
        x, y, count = width - window_width - 36 + 9 + 24, height - window_height - 65 + 44 + 181, 270
    elif field == "files-directory":
        x, y, count = 45 + 9 + 18, 75 + 44 + 163, 264
    elif field == "files-note":
        x, y, count = 45 + 9 + 18, 75 + 44 + 190, 240
    elif field == "files-lock":
        x, y, count = 45 + 9 + 18, 75 + 44 + 211, 240
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
    if not command or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789 =" for c in command):
        raise AssertionError("unsupported physical named-file command")
    for char in command:
        if char in "=0123456789":
            start = len(session.output)
            session.hmp(f"sendkey {'equal' if char == '=' else char} 80")
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


def verify_boot_stack(output: bytes) -> None:
    lines = output.replace(b"\r", b"").split(b"\n")[:-1]
    if lines.count(BOOT_STACK) != 1:
        raise AssertionError("boot lacks one complete stack-request acknowledgment")


def verify_irq_ingress(output: bytes, since: int) -> None:
    if not 0 <= since < len(output):
        raise AssertionError("invalid fresh physical ingress boundary")
    lines = output[since:].replace(b"\r", b"").split(b"\n")[:-1]
    if any(lines.count(marker) != 1 for marker in IRQ_INGRESS):
        raise AssertionError("physical desktop lacks fresh IRQ1/IRQ12 queued-input evidence")
    for delivered, queued in ((IRQ_INGRESS[0], IRQ_INGRESS[2]),
                              (IRQ_INGRESS[1], IRQ_INGRESS[3])):
        if lines.index(delivered) >= lines.index(queued):
            raise AssertionError("decoded physical input preceded its real interrupt")


def verify_frame_publication(output: bytes, since: int = 0,
                             geometry: tuple[int, int] | None = None) -> list[dict[str, int]]:
    """Require ordered native commit, measured work and ring-3 publication.

    These records are mandatory alongside independent screenshot oracles. They
    do not substitute for pixels, and a containing diagnostic is never evidence.
    """
    if not 0 <= since < len(output):
        raise AssertionError("invalid frame publication boundary")
    lines = output[since:].replace(b"\r", b"").split(b"\n")[:-1]
    state = 0
    measurements = []
    metric = re.compile(rb"IndianOS user: desktop PID4 frame metrics cycles=0x([0-9a-f]{16}) "
                        rb"calls=0x([0-9a-f]{16}) pixels=0x([0-9a-f]{16})")
    for line in lines:
        if FRAME_PRESENTED in line:
            if line != FRAME_PRESENTED or state != 0:
                raise AssertionError("frame presentation is prefixed, duplicated or out of order")
            state = 1
        elif FRAME_METRICS in line:
            match = metric.fullmatch(line)
            if match is None or state != 1:
                raise AssertionError("frame cost lacks exact fresh native measurements")
            cycles, calls, pixels = (int(value, 16) for value in match.groups())
            if (cycles == 0 or calls == 0 or not 640 * 480 <= pixels <= 1600 * 1600
                    or calls > 1600 * 32 + 128):
                raise AssertionError("frame cost has impossible work or unbounded call count")
            if geometry is not None:
                width, height = geometry
                if not 640 <= width <= 1600 or not 480 <= height <= 1600 or pixels != width * height:
                    raise AssertionError("frame cost does not cover the actual framebuffer")
                if calls > height * 32 + 128:
                    raise AssertionError("frame transport exceeded its bounded per-row cost")
            measurements.append(dict(cycles=cycles, calls=calls, pixels=pixels))
            state = 2
        elif FRAME_PUBLISHED in line:
            if line != FRAME_PUBLISHED or state != 2:
                raise AssertionError("ring-3 publication preceded complete measured presentation")
            state = 0
    if not measurements or state != 0:
        raise AssertionError("frame publication is missing, stale or incomplete")
    return measurements


def retain_frame_cost(session: QemuSession, path: Path,
                      geometry: tuple[int, int]) -> None:
    records = verify_frame_publication(bytes(session.output), 0, geometry)
    seconds = session.ready_observed - session.boot_started
    if seconds <= 0:
        raise AssertionError("startup wall-clock sample is invalid")
    path.write_text(json.dumps(dict(startup_seconds=seconds, frames=records), indent=2) + "\n")


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
    verify_frame_publication(prefix)
    if (COMPOSITOR.encode() not in prefix or READY.encode() not in prefix or
            prefix.find(COMPOSITOR.encode()) > prefix.find(READY.encode())):
        raise AssertionError("desktop advertised ready before compositor service")
    startup = prefix[:prefix.find(READY.encode())]
    lifecycle = startup.find(LIFECYCLE_READY.encode())
    if not 0 <= startup.find(COMPOSITOR.encode()) < lifecycle:
        raise AssertionError("kernel readiness preceded compositor readiness")
    if any(marker not in startup for marker in TERMINAL_AUTHORITY):
        raise AssertionError("Terminal startup lacks actual syscall authority negative controls")
    verify_pixel_startup(startup)
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


def verify_editor_authority(output: bytes, since: int) -> None:
    if not 0 <= since < len(output):
        raise AssertionError("invalid editor-authority evidence boundary")
    tail = output[since:]
    lines = tail.replace(b"\r", b"").split(b"\n")[:-1]
    for marker in (b"IndianOS user: desktop Terminal unsolicited filesystem mutation denied",
                   b"IndianOS user: desktop broker denied filesystem request outside submitted Enter"):
        if lines.count(marker) != 1:
            raise AssertionError("editor startup lacks fresh unsolicited-mutation denial")
    if any(marker in tail for marker in (COMMITTED.encode(), NAMED_COMMIT.encode(), PERSISTED.encode())):
        raise AssertionError("unsolicited editor startup mutation was committed")


def verify_pointer_ack(output: bytes, since: int, x: int, y: int, buttons: int) -> None:
    if (not all(isinstance(value, int) for value in (x, y, buttons)) or
            not 0 <= x < 1600 or not 0 <= y < 1600 or buttons not in range(4) or
            not 0 <= since < len(output)):
        raise AssertionError("invalid physical pointer acknowledgment boundary")
    expected = (f"IndianOS user: desktop physical pointer x=0x{x:08x} "
                f"y=0x{y:08x} buttons=0x{buttons:08x}").encode()
    lines = output[since:].replace(b"\r", b"").split(b"\n")[:-1]
    if lines.count(expected) != 1 or any(expected in line and line != expected for line in lines):
        raise AssertionError("physical pointer lacks one fresh exact presented state")
    before = b"\n".join(lines[:lines.index(expected)]) + b"\n"
    verify_frame_publication(before)


def await_pointer(session: QemuSession, x: int, y: int, buttons: int, since: int) -> None:
    # This is the production broker's post-presentation input observation,
    # not a timing estimate. Wait for the exact final packet before another
    # button transition or screendump can race it.
    expected = (f"IndianOS user: desktop physical pointer x=0x{x:08x} "
                f"y=0x{y:08x} buttons=0x{buttons:08x}")
    session.until(expected, 20, since=since)
    end = bytes(session.output).find(expected.encode(), since) + len(expected)
    session.until("\n", 20, since=end)
    verify_pointer_ack(bytes(session.output), since, x, y, buttons)


def move_pointer(session: QemuSession, previous: tuple[int, int], target: tuple[int, int],
                 buttons: int = 0) -> None:
    # Keep each emulated physical transfer within a signed Set-1 mouse
    # packet's range. Large HMP batches can exhaust the device queue before
    # the guest has presented all intermediate packets.
    x, y = previous
    while (x, y) != target:
        dx = max(-100, min(100, target[0] - x))
        dy = max(-100, min(100, target[1] - y))
        since = len(session.output)
        session.hmp(f"mouse_move {dx} {dy}")
        x, y = x + dx, y + dy
        await_pointer(session, x, y, buttons, since)


def pointer_button(session: QemuSession, position: tuple[int, int], buttons: int) -> None:
    since = len(session.output)
    session.hmp(f"mouse_button {buttons}")
    await_pointer(session, position[0], position[1], buttons, since)


def physical_focus(session: QemuSession, previous: tuple[int, int], target: tuple[int, int],
                   app: str) -> tuple[int, int]:
    # HMP relative motion enters the emulated 8042 auxiliary port. The guest
    # must route it through PID1 IPC and PID4's hit testing before acknowledging.
    start = len(session.output)
    move_pointer(session, previous, target)
    pointer_button(session, target, 1)
    pointer_button(session, target, 0)
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
    pointer_button(session, (100, 90), 1)
    move_pointer(session, (100, 90), (140, 110), 1)
    pointer_button(session, (140, 110), 0)
    moved = session.shot(folder / f"desktop-{index}-pointer-drag.ppm")
    verify_text_pixels(moved, "files-note", expected, (40, 20))
    move_pointer(session, (140, 110), (180, 150))
    released = session.shot(folder / f"desktop-{index}-pointer-release.ppm")
    verify_text_pixels(released, "files-note", expected, (40, 20))
    move_pointer(session, (180, 150), (140, 110))
    pointer_button(session, (140, 110), 1)
    move_pointer(session, (140, 110), (100, 90), 1)
    pointer_button(session, (100, 90), 0)
    center = (width // 2, height // 2)
    move_pointer(session, (100, 90), center)
    restored = session.shot(folder / f"desktop-{index}-pointer-drag-restored.ppm")
    verify_text_pixels(restored, "files-note", expected)
    acknowledged_key(session, "2", "R031.1 user switched to Terminal app")
    visible = session.shot(folder / f"desktop-{index}-before-minimize.ppm")
    terminal_height = min(max(height * 50 // 100, 294), 822, height - 110)
    title = (width - 100, height - terminal_height - 50)
    move_pointer(session, center, title)
    pointer_button(session, title, 1)
    acknowledged_key(session, "m", "R031.1 Terminal minimized")
    # Minimize cancels the captured title drag, even with the button held.
    displaced = (title[0] + 40, title[1] + 20)
    move_pointer(session, title, displaced, 1)
    minimized = session.shot(folder / f"desktop-{index}-minimized.ppm")
    verify_text_pixels(minimized, "files-note", expected)
    pointer_button(session, displaced, 0)
    acknowledged_key(session, "r", "R031.1 Terminal restored")
    restored = session.shot(folder / f"desktop-{index}-restored.ppm")
    verify_minimize_restore(visible, minimized, restored)
    move_pointer(session, displaced, center)


def pointer_focus_acceptance(session: QemuSession, folder: Path, index: int,
                             editing: tuple[int, int, bytes]) -> tuple[int, int, bytes]:
    # Keep CAT pending while Files owns focus. An erroneously delivered Z
    # changes the command to CATZ and cannot produce the exact readback below.
    type_command(session, "cat")
    width, height = editing[:2]
    pointer = physical_focus(session, (width // 2, height // 2), (100, 135), "Files")
    focused = settled_frame(session, folder / f"desktop-{index}-pointer-files.ppm")
    changed(editing, focused, "physical pointer Files focus")
    since = len(session.output)
    session.hmp("sendkey z 80")
    session.until("desktop Terminal editor suspended outside keyboard focus", 20, since=since)
    blocked = settled_frame(session, folder / f"desktop-{index}-pointer-key-blocked.ppm")
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


def settled_frame(session: QemuSession, path: Path) -> tuple[int, int, bytes]:
    """Capture identical consecutive real frames before a strict identity check.

    PS/2 press and release are distinct packets. The focus acknowledgment can
    precede repaint for the queued release; QEMU screendump does not fence the
    guest's CPU stores. Retain every sample so a timeout remains diagnosable.
    This never permits changed pixels in verify_suspended_editor.
    """
    session.drain_serial(0.2)
    previous = session.shot(path)
    equal = 0
    for attempt in range(1, 11):
        session.drain_serial(0.15)
        sample = path.with_name(f"{path.stem}.settle-{attempt:02d}{path.suffix}")
        current = session.shot(sample)
        equal = equal + 1 if current == previous else 0
        if equal == 2:
            print(f"Stable framebuffer {path.name}: {attempt + 1} retained samples", flush=True)
            return current
        previous = current
    raise AssertionError("framebuffer did not settle before strict editor isolation comparison")


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
        started = time.monotonic()
        process = subprocess.Popen(command, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.STDOUT, bufsize=0)
        sock = open_monitor(monitor, process)
        session = QemuSession(process, sock)
        session.socket_directory = socket_directory
        session.boot_started = started
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
        session.ready_observed = time.monotonic()
        verify_boot_stack(bytes(session.output))
        session.until("R031.9 real file and directory contents rendered into Files window", 20)
        frame = session.shot(folder / f"desktop-{index}-initial.ppm")
        check_two_windows(frame)
        retain_frame_cost(session, folder / f"desktop-{index}-startup-cost.json", frame[:2])
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
        input_at = len(session.output)
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
        verify_irq_ingress(bytes(session.output), input_at)
        acknowledged_key(session, "1", "R031.1 user switched to Files app")
        frame = session.shot(folder / f"desktop-{index}-files-focus.ppm")
        frame = physical_key(session, "2", "R031.1 user switched to Terminal app",
                             folder, index, 8, frame, False)
        editing_at = acknowledged_key(session, "ret")
        verify_editor_authority(bytes(session.output), editing_at)
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
    except Exception as error:
        capture_boot_failure(session, folder, f"desktop-{index}", error)
        raise
    finally:
        close(process, sock, session, folder / f"desktop-{index}.serial.log")
    print(f"Normal desktop boot {index + 1}/2: four ring3 services, physical input, selected GPT readback and cleanup PASS")


def lease_records(output: bytes, since: int) -> list[bytes]:
    if not 0 <= since < len(output):
        raise AssertionError("invalid fresh native lease evidence boundary")
    lines = output[since:].replace(b"\r", b"").split(b"\n")[:-1]
    # The physical f shortcut is denied synchronously, so its single UART
    # echo precedes PID1's record without an intervening scheduler log.
    # Normalize only that exact known record, never arbitrary log prefixes.
    refused = b"IndianOS user: desktop Files update refused by file service"
    return [line[1:] if line == b"f" + refused else line for line in lines]


def verify_lease_commit(output: bytes, since: int, positioned: bool = False) -> None:
    lines = lease_records(output, since)
    commit = b"IndianOS user: " + NAMED_COMMIT.encode()
    if (lines.count(LEASE_CURSOR) != 1 or lines.count(LEASE_REFRESH) != 1 or
            lines.count(POSITIONED_WRITE) != int(positioned) or lines.count(commit) != 1 or
            lines.index(LEASE_CURSOR) >= lines.index(LEASE_REFRESH) or
            lines.index(LEASE_REFRESH) >= lines.index(commit) or
            (positioned and lines.index(POSITIONED_WRITE) >= lines.index(LEASE_CURSOR)) or
            any(marker in lines for marker in (LEASE_ACQUIRED, LEASE_RELEASED, LEASE_DENIED))):
        raise AssertionError("unrelated write lacks fresh original-token verification before commit")


def verify_lease_denial(before: bytes, after: bytes, output: bytes, since: int,
                       marker: bytes = LEASE_DENIED) -> None:
    lines = lease_records(output, since)
    denials = (LEASE_DENIED, b"IndianOS user: desktop Files update refused by file service")
    if marker not in denials:
        raise AssertionError("unknown native read-lease denial evidence")
    if before != after:
        raise AssertionError("protected-file denial changed persistent media")
    if lines.count(marker) != 1 or sum(lines.count(x) for x in denials) != 1:
        raise AssertionError("protected-file denial lacks one exact native error record")
    forbidden = (NAMED_COMMIT.encode(), COMMITTED.encode(), PERSISTED.encode(),
                 LEASE_ACQUIRED, LEASE_CURSOR, LEASE_REFRESH, LEASE_RELEASED, POSITIONED_WRITE,
                 SHUTDOWN.encode(), STOPPED.encode(),
                 b"selected volume sync failed", b"IndianOS recovery:", b"FATAL",
                 b"restarting retired service session")
    if any(marker in output[since:] for marker in forbidden):
        raise AssertionError("read-lease contention committed, replaced authority or retired the session")


def verify_released_lease_write(output: bytes, since: int) -> None:
    lines = lease_records(output, since)
    commit = b"IndianOS user: " + NAMED_COMMIT.encode()
    if (lines.count(LEASE_RELEASED) != 1 or lines.count(commit) != 1 or
            lines.index(LEASE_RELEASED) >= lines.index(commit) or
            any(marker in lines for marker in (LEASE_ACQUIRED, LEASE_CURSOR, LEASE_REFRESH, POSITIONED_WRITE, LEASE_DENIED))):
        raise AssertionError("protected-file write did not follow release of the original lease")


def shared_read_lease_boots(qemu: str, iso: Path, ovmf: Path, disk: Path, folder: Path) -> None:
    """Actual PID2 open/read lock survives unrelated PID1 native mutations.

    User input selects the Files lock action; it never mints a test handle.
    Same-file contention is nondestructive, and release permits a durable edit.
    """
    original = disk.read_bytes()
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, "shared-read-lease")
    try:
        session.until(READY, 45)
        acknowledged_key(session, "1", "R031.1 user switched to Files app")
        acquired = acknowledged_key(session, "l", LEASE_ACQUIRED.decode())
        session.until(LEASE_CONTROL_ACK, 20, since=acquired)
        session.drain_serial(0.1)
        if lease_records(bytes(session.output), acquired).count(LEASE_ACQUIRED) != 1:
            raise AssertionError("Files did not acquire exactly one native read lease")
        verify_text_pixels(settled_frame(session, folder / "lease-acquired.ppm"),
                           "files-lock", "READ LOCKED")
        # The Files shortcut uses the same protected path API. Ordinary
        # contention must not terminate PID1 or silently release the reader.
        before = disk.read_bytes()
        refused = acknowledged_key(session, "f", "desktop Files update refused by file service")
        session.drain_serial(0.1)
        verify_lease_denial(before, disk.read_bytes(), bytes(session.output), refused,
                           b"IndianOS user: desktop Files update refused by file service")
        verify_text_pixels(session.shot(folder / "lease-files-update-denied.ppm"),
                           "files-lock", "READ LOCKED")
        acknowledged_key(session, "2", "R031.1 user switched to Terminal app")
        acknowledged_key(session, "ret")
        for step, (command, listing) in enumerate((
                ("touch peer", "f:readme f:peer "),
                ("put peer=violet", "f:readme f:peer "),
                ("patch peer 2=xy", "f:readme f:peer "),
                ("mv peer other", "f:readme f:other "))):
            type_named_command(session, command)
            submitted = acknowledged_key(session, "ret")
            session.until(NAMED_COMMIT, 20, since=submitted)
            session.drain_serial(0.1)
            verify_lease_commit(bytes(session.output), submitted, positioned=command.startswith("patch "))
            frame = session.shot(folder / f"lease-unrelated-{step}.ppm")
            verify_text_pixels(frame, "terminal", "SAVED")
            verify_text_pixels(frame, "files-directory", listing)
            verify_text_pixels(frame, "files-note", "HELLO INDIANOS")
            verify_text_pixels(frame, "files-lock", "READ LOCKED")
        for command, label in (("patch readme 1=x", "positioned"), ("put readme=blocked", "replace")):
            type_named_command(session, command)
            before = disk.read_bytes()
            denied = acknowledged_key(session, "ret")
            session.until(LEASE_DENIED.decode(), 20, since=denied)
            session.drain_serial(0.1)
            verify_lease_denial(before, disk.read_bytes(), bytes(session.output), denied)
            frame = session.shot(folder / f"lease-same-file-{label}-denied.ppm")
            verify_text_pixels(frame, "terminal", "IO ERROR")
            verify_text_pixels(frame, "files-lock", "READ LOCKED")
        type_named_command(session, "cat readme")
        read = acknowledged_key(session, "ret")
        marker = "IndianOS user: R051 Terminal readback: HELLO INDIANOS"
        session.until(marker, 20, since=read)
        session.drain_serial(0.1)
        exact_terminal_line(bytes(session.output), read, marker)
        verify_text_pixels(session.shot(folder / "lease-protected-readback.ppm"), "terminal", "HELLO INDIANOS")
        if disk.read_bytes() != before:
            raise AssertionError("readback after lease contention modified persistent media")
        acknowledged_key(session, "tab")
        acknowledged_key(session, "1", "R031.1 user switched to Files app")
        released = acknowledged_key(session, "u", LEASE_RELEASED.decode())
        session.until(LEASE_CONTROL_ACK, 20, since=released)
        verify_text_pixels(settled_frame(session, folder / "lease-released.ppm"), "files-lock", "UNLOCKED")
        acknowledged_key(session, "2", "R031.1 user switched to Terminal app")
        acknowledged_key(session, "ret")
        type_named_command(session, "put readme=permitted")
        submitted = acknowledged_key(session, "ret")
        session.until(NAMED_COMMIT, 20, since=submitted)
        session.drain_serial(0.1)
        verify_released_lease_write(bytes(session.output), released)
        frame = session.shot(folder / "lease-released-write.ppm")
        verify_text_pixels(frame, "files-note", "permitted")
        verify_text_pixels(frame, "terminal", "SAVED")
        acknowledged_key(session, "tab")
        shutdown_at = acknowledged_key(session, "q", SHUTDOWN)
        session.until(STOPPED, 20, since=shutdown_at)
        session.drain_serial(0.1)
        verify_runtime(bytes(session.output), shutdown_at)
    finally:
        close(process, sock, session, folder / "shared-read-lease.serial.log")
    verify_guard(original, disk.read_bytes())
    persisted = disk.read_bytes()
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, "shared-read-lease-reboot")
    try:
        session.until(READY, 45)
        verify_text_pixels(session.shot(folder / "lease-reboot-initial.ppm"), "files-note", "permitted")
        acknowledged_key(session, "2", "R031.1 user switched to Terminal app")
        acknowledged_key(session, "ret")
        for name, value in (("readme", "permitted"), ("other", "vixyet")):
            type_named_command(session, "cat " + name)
            submitted = acknowledged_key(session, "ret")
            marker = "IndianOS user: R051 Terminal readback: " + value
            session.until(marker, 20, since=submitted)
            session.drain_serial(0.1)
            exact_terminal_line(bytes(session.output), submitted, marker)
            verify_text_pixels(session.shot(folder / f"lease-reboot-{name}.ppm"), "terminal", value)
        acknowledged_key(session, "tab")
        shutdown_at = acknowledged_key(session, "q", SHUTDOWN)
        session.until(STOPPED, 20, since=shutdown_at)
        session.drain_serial(0.1)
        verify_runtime(bytes(session.output), shutdown_at)
    finally:
        close(process, sock, session, folder / "shared-read-lease-reboot.serial.log")
    if disk.read_bytes() != persisted:
        raise AssertionError("shared-file cold readback or synchronized shutdown changed media")
    verify_guard(original, persisted)
    print("Normal desktop native PID2 read lease: unrelated PID1 commits, same-file denial, release and cold reboot PASS")


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


class PublicationCut:
    """Fail-closed oracle for one four-sector COW replacement, not a guest mock.

    Observations come from actual native ATA command-entry stops. A cut before
    the second data command leaves one flushed orphan sector; the two metadata
    cuts bracket checkpoint publication, before any application acknowledgement.
    """
    def __init__(self, phase: str, data_sectors: int = 4):
        if phase not in ("data", "metadata-before", "metadata-after"):
            raise AssertionError("unknown guest publication cut")
        if data_sectors not in (0, 4) or (phase == "data" and data_sectors == 0):
            raise AssertionError("unsupported publication data geometry")
        self.phase = phase
        self.data_sectors = data_sectors
        self.data: list[int] = []
        self.head: int | None = None

    def observe(self, command: int, lba: int) -> bool:
        if command not in (0x20, 0x30) or not 0 <= lba < TOTAL:
            raise AssertionError("native ATA arguments do not match qualified command ABI")
        if command == 0x20:
            return self.phase == "metadata-after" and self.head == lba
        if not START <= lba < START + COUNT:
            raise AssertionError("publication attempted a write outside selected volume")
        if lba < START + 2:
            if len(self.data) != self.data_sectors or self.head is not None:
                raise AssertionError("checkpoint preceded one complete new data bank")
            self.head = lba
            return self.phase == "metadata-before"
        if self.head is not None or len(self.data) >= self.data_sectors or (
                self.data and lba != self.data[-1] + 1):
            raise AssertionError("replacement is not one bounded sequential COW bank")
        self.data.append(lba)
        return self.phase == "data" and len(self.data) == 2


def verify_publication_cut(before: bytes, after: bytes, phase: str,
                           output: bytes, submitted: int, *, data_sectors: int = 4) -> None:
    PublicationCut(phase, data_sectors)  # Reject unsupported evidence contracts.
    verify_guard(before, after)
    if not 0 < submitted <= len(output) or any(marker in output[submitted:] for marker in
            (NAMED_COMMIT.encode(), ACK.encode(), DURABLE_SHUTDOWN.encode(), STOPPED.encode())):
        raise AssertionError("publication cut lacks a fresh pre-acknowledgement boundary")
    old_heads = before[START * 512:(START + 2) * 512]
    new_heads = after[START * 512:(START + 2) * 512]
    if data_sectors == 0 and before[(START + 2) * 512:] != after[(START + 2) * 512:]:
        raise AssertionError("metadata-only operation changed a data sector")
    if before == after and not (data_sectors == 0 and phase == "metadata-before"):
        raise AssertionError("guest was killed before any actual storage mutation")
    if phase in ("data", "metadata-before"):
        if old_heads != new_heads:
            raise AssertionError("pre-publication cut changed a committed metadata head")
    elif phase == "metadata-after":
        changed = [i for i in (0, 1) if old_heads[i*512:(i+1)*512] != new_heads[i*512:(i+1)*512]]
        if len(changed) != 1:
            raise AssertionError("post-publication cut did not replace exactly one checkpoint")
        old_generation = max(struct.unpack_from("<Q", old_heads, i*512+16)[0] for i in (0, 1))
        head = bytearray(new_heads[changed[0]*512:(changed[0]+1)*512])
        saved = struct.unpack_from("<I", head, 24)[0]
        head[24:28] = bytes(4)
        if (head[:8] != b"INDR027!" or zlib.crc32(head) != saved or
                struct.unpack_from("<Q", head, 16)[0] != old_generation + 1):
            raise AssertionError("post-publication cut lacks a valid next COW checkpoint")
    else:
        raise AssertionError("unknown publication cut evidence")


def cut_submitted_publication(process, session: QemuSession, disk: Path, folder: Path,
                              name: str, target: int, active: int, expected: bytes,
                              phase: str, data_sectors: int = 4) -> None:
    """Submit the already typed physical command and stop its native I/O.

    File replacement and namespace cases use the same debugger contract.
    No caller may manufacture a cut by modifying the disk or guest state.
    """
    before = disk.read_bytes()
    submitted = len(session.output)
    debugger = None
    evidence = []
    try:
        session.hmp("stop")
        debugger = GuestDebugger(session.debugger_path)
        if not debugger.command("?").startswith(("S", "T")):
            raise AssertionError("publication debugger did not attach to stopped guest")
        debugger.command("qSupported:xmlRegisters=i386")
        debugger.command("qXfer:features:read:target.xml:0,fff")
        if debugger.memory(target, 8) != expected:
            raise AssertionError("ATA command code differs from this exact source ISO")
        if debugger.command(f"Z1,{target:x},1") != "OK":
            raise AssertionError("cannot identify real ATA command execution")
        cut = PublicationCut(phase, data_sectors)
        for attempt in range(1024):
            debugger.send("c")
            if attempt == 0:
                if "running" not in session.hmp("info status"):
                    session.hmp("cont")
                session.hmp("sendkey ret 80")
            if not debugger.response().startswith(("S", "T")):
                raise AssertionError("guest did not reach a native ATA command")
            current = int.from_bytes(debugger.memory(active, 8), "little")
            pid = int.from_bytes(debugger.memory(current, 4), "little") if current else 0
            if debugger.register(16) != target or debugger.register(18) & 3 or pid != 1:
                raise AssertionError("publication breakpoint lacks native kernel/PID1 ownership")
            # Qualified native Rust x86-64 select entry argument registers.
            # The diagnostic export stabilizes its name, not the Rust ABI.
            command, lba = debugger.register(3) & 255, debugger.register(4)
            evidence.append(f"PID={pid} RIP={target:#x} command={command:#x} LBA={lba}")
            if cut.observe(command, lba):
                session.drain_serial(0.1)
                # Remain stopped before this command; no detach/resume race.
                process.kill()
                process.wait(timeout=5)
                verify_publication_cut(before, disk.read_bytes(), phase,
                                       bytes(session.output), submitted,
                                       data_sectors=data_sectors)
                return
            if debugger.command(f"z1,{target:x},1") != "OK":
                raise AssertionError("failed to advance unchanged ATA instruction")
            if not debugger.command("s").startswith(("S", "T")):
                raise AssertionError("failed to single-step unchanged ATA code")
            if debugger.command(f"Z1,{target:x},1") != "OK":
                raise AssertionError("failed to restore ATA execution breakpoint")
        raise AssertionError("publication cut was not reached within command budget")
    finally:
        if debugger is not None:
            debugger.close()
        (folder / f"{name}.debugger.log").write_text("\n".join(evidence) + "\n")


def interrupted_publication(qemu: str, iso: Path, ovmf: Path, disk: Path,
                            folder: Path, xorriso: str, nm: str) -> None:
    """Cut actual guest writes between sectors and before their native reply.

    This uses only external execution breakpoints and SIGKILL. It does not
    synthesize torn sectors, patch kernel code or claim physical power safety.
    """
    sources = folder / "publication-sources"
    sources.mkdir()
    kernel = sources / "kernel.elf"
    checked_run([xorriso, "-osirrox", "on", "-indev", str(iso),
                 "-extract", "/boot/indianos.elf", str(kernel)])
    target = unique_symbol(nm, kernel, ("indianos_ata_select_command",))
    active = unique_symbol(nm, kernel, ("user_runtime::ACTIVE_PROCESS",))
    expected = executable_bytes(kernel.read_bytes(), target, 8)
    golden = disk.read_bytes()
    for phase in ("data", "metadata-before", "metadata-after"):
        disk.write_bytes(golden)
        name = "publication-" + phase
        process, sock, session = spawn(qemu, iso, ovmf, disk, folder, name, debugger=True)
        try:
            session.until(READY, 45)
            acknowledged_key(session, "2", "R031.1 user switched to Terminal app")
            acknowledged_key(session, "ret")
            for command in ("touch cut", "put cut=saved"):
                type_named_command(session, command)
                sent = acknowledged_key(session, "ret")
                session.until(NAMED_COMMIT, 20, since=sent)
            type_named_command(session, "put cut=violet")
            cut_submitted_publication(process, session, disk, folder, name,
                                      target, active, expected, phase)
        finally:
            close(process, sock, session, folder / f"{name}.serial.log")
        recovered = disk.read_bytes()
        process, sock, session = spawn(qemu, iso, ovmf, disk, folder, name + "-reboot")
        try:
            session.until(READY, 45)
            acknowledged_key(session, "2", "R031.1 user switched to Terminal app")
            acknowledged_key(session, "ret")
            type_named_command(session, "cat cut")
            sent = acknowledged_key(session, "ret")
            value = "violet" if phase == "metadata-after" else "saved"
            marker = "IndianOS user: R051 Terminal readback: " + value
            session.until(marker, 20, since=sent)
            session.drain_serial(0.1)
            exact_terminal_line(bytes(session.output), sent, marker)
            verify_text_pixels(session.shot(folder / f"{name}-reboot.ppm"), "terminal", value)
            acknowledged_key(session, "tab")
            shutdown_at = acknowledged_key(session, "q", SHUTDOWN)
            session.until(STOPPED, 20, since=shutdown_at)
            session.drain_serial(0.1)
            verify_runtime(bytes(session.output), shutdown_at)
        finally:
            close(process, sock, session, folder / f"{name}-reboot.serial.log")
        if disk.read_bytes() != recovered:
            raise AssertionError("publication recovery/readback changed persistent media")
        verify_guard(golden, recovered)
        print(f"Normal desktop pre-ack {phase} guest cut: exact {value} survives real reboot PASS")
    interrupted_namespace_publication(qemu, iso, ovmf, disk, folder,
                                      golden, target, active, expected)


def namespace_cut_contract(operation: str, phase: str) -> tuple[str, int, tuple[str, ...], tuple[tuple[str, str], ...]]:
    """Exact old/new namespace and unchanged-file oracle for each operation."""
    if phase not in ("metadata-before", "metadata-after"):
        raise AssertionError("namespace cuts must bracket metadata publication")
    committed = phase == "metadata-after"
    labels = ("f:readme", "f:cut")
    reads = (("readme", "HELLO INDIANOS"), ("cut", "saved"))
    if operation == "rename":
        if committed:
            labels = ("f:readme", "f:moved")
            reads = (("readme", "HELLO INDIANOS"), ("moved", "saved"))
        return "mv cut moved", 0, labels, reads
    if operation == "unlink":
        if committed:
            labels, reads = ("f:readme",), (("readme", "HELLO INDIANOS"),)
        return "rm cut", 0, labels, reads
    if operation == "mkdir":
        if committed:
            labels += ("d:d",)
        return "mkdir d", 4, labels, reads
    raise AssertionError("unknown namespace publication operation")


def verify_namespace_listing(output: bytes, since: int, expected: tuple[str, ...]) -> None:
    prefix = b"IndianOS user: R055 Terminal directory: "
    if not 0 <= since < len(output) or not expected or len(set(expected)) != len(expected):
        raise AssertionError("invalid exact namespace evidence contract")
    lines = output[since:].replace(b"\r", b"").split(b"\n")[:-1]
    listings = [line[len(prefix):] for line in lines if line.startswith(prefix)]
    if len(listings) != 1 or listings[0] != " ".join(expected).encode("ascii"):
        raise AssertionError("fresh namespace is missing, duplicated or differs from exact old/new state")


def interrupted_namespace_publication(qemu: str, iso: Path, ovmf: Path, disk: Path,
                                      folder: Path, golden: bytes, target: int,
                                      active: int, expected_code: bytes) -> None:
    """Six live namespace cuts, each verified on two consecutive cold boots."""
    for operation in ("rename", "unlink", "mkdir"):
        for phase in ("metadata-before", "metadata-after"):
            command, data_sectors, labels, reads = namespace_cut_contract(operation, phase)
            disk.write_bytes(golden)
            name = f"namespace-publication-{operation}-{phase}"
            process, sock, session = spawn(qemu, iso, ovmf, disk, folder, name, debugger=True)
            try:
                session.until(READY, 45)
                acknowledged_key(session, "2", "R031.1 user switched to Terminal app")
                acknowledged_key(session, "ret")
                for seed in ("touch cut", "put cut=saved"):
                    type_named_command(session, seed)
                    sent = acknowledged_key(session, "ret")
                    session.until(NAMED_COMMIT, 20, since=sent)
                type_named_command(session, command)
                cut_submitted_publication(process, session, disk, folder, name,
                                          target, active, expected_code, phase, data_sectors)
            finally:
                close(process, sock, session, folder / f"{name}.serial.log")
            recovered = disk.read_bytes()
            verify_guard(golden, recovered)
            for reboot in (1, 2):
                boot_name = f"{name}-reboot-{reboot}"
                process, sock, session = spawn(qemu, iso, ovmf, disk, folder, boot_name)
                try:
                    session.until(READY, 45)
                    acknowledged_key(session, "2", "R031.1 user switched to Terminal app")
                    acknowledged_key(session, "ret")
                    type_named_command(session, "ls")
                    sent = acknowledged_key(session, "ret")
                    session.until("IndianOS user: R055 Terminal directory: ", 20, since=sent)
                    session.drain_serial(0.1)
                    verify_namespace_listing(bytes(session.output), sent, labels)
                    verify_text_pixels(session.shot(folder / f"{boot_name}-listing.ppm"),
                                       "files-directory", " ".join(labels) + " ")
                    for path, value in reads:
                        type_named_command(session, "cat " + path)
                        sent = acknowledged_key(session, "ret")
                        marker = "IndianOS user: R051 Terminal readback: " + value
                        session.until(marker, 20, since=sent)
                        session.drain_serial(0.1)
                        exact_terminal_line(bytes(session.output), sent, marker)
                        verify_text_pixels(session.shot(folder / f"{boot_name}-{path}.ppm"),
                                           "terminal", value)
                    acknowledged_key(session, "tab")
                    shutdown_at = acknowledged_key(session, "q", SHUTDOWN)
                    session.until(STOPPED, 20, since=shutdown_at)
                    session.drain_serial(0.1)
                    verify_runtime(bytes(session.output), shutdown_at)
                finally:
                    close(process, sock, session, folder / f"{boot_name}.serial.log")
                if disk.read_bytes() != recovered:
                    raise AssertionError("namespace recovery/readback or shutdown modified persistent media")
                verify_guard(golden, disk.read_bytes())
            print(f"Normal desktop pre-ack {operation} {phase}: exact namespace and payloads survive TWO cold reboots PASS")


def denied_boot(qemu: str, iso: Path, ovmf: Path, disk: Path, folder: Path, name: str,
                reason: str = RECOVERY, *, write_fault: Path | None = None) -> None:
    before = hashlib.sha256(disk.read_bytes()).digest()
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, name, write_fault=write_fault)
    try:
        session.until(reason, 45)
        session.until("IndianOS recovery: read-only serial console ready", 10)
        verify_boot_stack(bytes(session.output))
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
    for kind in ("overlap", "duplicate-id", "protective-overflow",
                 "nil-disk-guid", "backup-disk-guid", "backup-table-mismatch"):
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
    changing a register. Replacement-negative cases explicitly mutate bounded
    future ELF input; the staging-privacy case changes one PID4-private pixel.
    No disk or page table is written and production code remains unchanged.
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


def compositor_source_words(symbols: str) -> tuple[int, int]:
    # LLVM may split the private Rust tuple into two scalar globals with
    # the same demangled name. Inspect actual symbol sizes, never choose
    # an arbitrary match or assume which scalar contains the pointer.
    matches = []
    for line in symbols.splitlines():
        if "user_runtime::COMPOSITOR_ELF" in line:
            fields = line.split()
            if len(fields) < 4:
                raise AssertionError("malformed compositor source symbol")
            matches.append((int(fields[0], 16), int(fields[1], 16)))
    if len(matches) == 1 and matches[0][1] == 16:
        return matches[0][0], matches[0][0] + 8
    if len(matches) == 2 and all(size == 8 for _, size in matches) and matches[0][0] != matches[1][0]:
        return tuple(address for address, _ in matches)
    raise AssertionError("ambiguous compositor source storage")


def compositor_source_extent(debugger: GuestDebugger, words: tuple[int, int], length: int) -> tuple[int, int]:
    values = [struct.unpack("<Q", debugger.memory(address, 8))[0] for address in words]
    pointers = [value for value in values if value != length]
    if values.count(length) != 1 or len(pointers) != 1 or not 0xffff800000000000 <= pointers[0] <= (1 << 64) - length:
        raise AssertionError("invalid compositor source extent")
    return pointers[0], length


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


def surface_guard_records(raw: bytes, payload_pages: int = 1) -> list[tuple[int, int, int, int]]:
    if len(raw) != 64 or payload_pages not in (1, 1188):
        raise AssertionError("surface guard ownership ledger is not 64 bytes")
    words = struct.unpack("<8Q", raw)
    records = [tuple(words[start:start + 4]) for start in (0, 4)]
    for lower, begin, end, upper in records:
        if (lower < 0xffff800000000000 or lower % 4096 or
                begin != lower + 4096 or end != begin + payload_pages * 4096 or
                upper != end or upper + 4096 > 1 << 64):
            raise AssertionError("invalid private surface guard geometry")
    ordered = sorted(records)
    if ordered[0][3] + 4096 > ordered[1][0]:
        raise AssertionError("private surface guard objects overlap")
    return records


def verify_pixel_startup(startup: bytes) -> None:
    """Fresh native pixel execution must precede each incarnation's readiness.

    These records complement actual glyph/pixel comparisons and mapping probes;
    serial assertions alone do not establish framebuffer ownership or rollback.
    """
    lines = startup.replace(b"\r", b"").split(b"\n")[:-1]
    for marker in PIXEL_APP_EVIDENCE:
        if lines.count(marker) != 1:
            raise AssertionError("pixel startup lacks fresh complete native app evidence")
    if lines.count(LIFECYCLE_READY.encode()) != 1:
        raise AssertionError("pixel startup lacks one complete lifecycle acknowledgment")
    ready = startup.find(LIFECYCLE_READY.encode())
    if ready < 0 or any(startup.find(marker) >= ready for marker in PIXEL_APP_EVIDENCE):
        raise AssertionError("pixel startup was not verified before lifecycle readiness")
    verify_frame_publication(startup)
    snapshot = startup.find(COMPOSITOR_SNAPSHOT)
    for pid, app in ((2, "Files"), (3, "Terminal")):
        guard = startup.find(f"IndianOS: PID {pid} PIXEL_GUARDS_READY".encode())
        scratch = startup.find(f"IndianOS user: desktop {app} private pixel scratch guards verified".encode())
        committed = startup.find(f"IndianOS user: desktop {app} ring3 raster pixels committed".encode())
        if not guard < scratch < committed < snapshot < ready:
            raise AssertionError("native app guards, pixel commits and compositor snapshots are out of order")


def app_scratch_records(raw: bytes, compositor_root: int, pages: int = 1) -> list[tuple[int, int, int, int]]:
    """Validate recorded owners, without reading app VAs through PID4's CR3."""
    if len(raw) != 64:
        raise AssertionError("app scratch ownership ledger is not 64 bytes")
    words = struct.unpack("<8Q", raw)
    records = [tuple(words[start:start + 4]) for start in (0, 4)]
    for root, lower, payload, upper in records:
        if (not 0 < root < 1 << 52 or root % 4096 or root == compositor_root or
                not 0x400000 <= lower < 0x00007ffff0000000 or lower % 4096 or
                payload != lower + 4096 or upper != payload + pages * 4096 or
                upper + 4096 > 0x00007ffff0000000):
            raise AssertionError("invalid app-owned scratch geometry or root")
    if records[0][0] == records[1][0]:
        raise AssertionError("independent apps share a scratch address-space root")
    return records


def physical_qword(session: QemuSession, address: int) -> int:
    """Read one actual guest-physical page-table entry without changing CR3."""
    if not 0 <= address < 1 << 52 or address % 8:
        raise AssertionError("invalid physical page-table address")
    response = session.hmp(f"xp /1gx 0x{address:x}")
    response = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", response)
    words = re.findall(r"(?m)^\s*(?:0x)?([0-9a-fA-F]{1,16}):\s+0x([0-9a-fA-F]{16})(?:\s|$)", response)
    if len(words) != 1 or int(words[0][0], 16) != address:
        raise AssertionError("physical page-table read lacks one exact-address word")
    return int(words[0][1], 16)


def physical_mapping(session: QemuSession, root: int, address: int) -> tuple[int, bool, bool, bool] | None:
    """Bounded four-level walk of the owner's real tables, including ancestors."""
    if not 0 < root < 1 << 52 or root % 4096 or not 0 <= address < 0x00007ffff0000000:
        raise AssertionError("invalid app page-table walk")
    table, writable, user, nx = root, True, True, False
    for level, shift in enumerate((39, 30, 21, 12)):
        entry = physical_qword(session, table + ((address >> shift) & 511) * 8)
        if entry & 1 == 0:
            return None
        writable = writable and bool(entry & 2)
        user = user and bool(entry & 4)
        nx = nx or bool(entry & (1 << 63))
        if level < 3 and entry & 128:
            raise AssertionError("app scratch unexpectedly uses a huge/reserved mapping")
        table = entry & 0x000ffffffffff000
        if table == 0:
            raise AssertionError("app scratch walk references physical frame zero")
    return table + (address & 4095), writable, user, nx


def verify_app_scratch_mappings(session: QemuSession, records: list[tuple[int, int, int, int]],
                                compositor_root: int, evidence: list[str]) -> None:
    payload_frames = set()
    for pid, (root, lower, payload, upper) in zip((2, 3), records):
        for guard in (lower, upper):
            if physical_mapping(session, root, guard) is not None:
                raise AssertionError("app canvas guard remains mapped in its own root")
        foreign_roots = (compositor_root, *(record[0] for record in records if record[0] != root))
        for address in range(payload, upper, 4096):
            mapping = physical_mapping(session, root, address)
            if mapping is None or mapping[1:] != (True, True, True):
                raise AssertionError("app canvas page lacks effective user RW/NX rights")
            frame = mapping[0]
            if frame in payload_frames:
                raise AssertionError("app canvas pages share a physical frame")
            payload_frames.add(frame)
            for foreign in foreign_roots:
                other = physical_mapping(session, foreign, address)
                if other is not None and other[0] == frame:
                    raise AssertionError("another service maps an app's private canvas frame")
        evidence.append(f"PID{pid} CR3={root:#x}: {(upper-payload)//4096} private canvas pages "
                        "are user RW/NX, with absent guards and no peer/compositor alias at canvas VAs")


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


def client_incarnation_records(raw: bytes) -> tuple[tuple[int, int, int], ...]:
    if len(raw) != 72:
        raise AssertionError("client incarnation receipt is not 72 bytes")
    words = struct.unpack("<9Q", raw)
    records = tuple(tuple(words[at:at + 3]) for at in (0, 3, 6))
    for index, (generation, root, files) in enumerate(records):
        if (generation == 0 or root == 0 or root >= 1 << 52 or root % 4096
                or (index < 2 and files == 0) or (index == 2 and files != 0)):
            raise AssertionError("invalid live client incarnation receipt")
    if len({record[1] for record in records}) != 3:
        raise AssertionError("independent clients share an address space")
    return records


def verify_preserved_clients(before: bytes, after: bytes, old_roles: bytes, new_roles: bytes) -> None:
    client_incarnation_records(before)
    client_incarnation_records(after)
    if before != after or len(old_roles) != 32 or len(new_roles) != 32:
        raise AssertionError("PID4 recovery replaced or changed a live client")
    old = struct.unpack("<4Q", old_roles)
    new = struct.unpack("<4Q", new_roles)
    if any(value == 0 for value in old) or old[:3] != new[:3] or new[3] <= old[3]:
        raise AssertionError("PID4 recovery lacks one fresh compositor incarnation")
    if tuple(record[0] for record in client_incarnation_records(before)) != old[:3]:
        raise AssertionError("client and role generation receipts disagree")


def verify_transient_restart(output: bytes, injection_at: int) -> None:
    """Only PID4 may be replaced; original clients and readiness must survive."""
    if not 0 < injection_at < len(output) or output[:injection_at].count(READY.encode()) != 1:
        raise AssertionError("invalid live compositor injection boundary")
    tail = output[injection_at:]
    markers = (b"IndianOS: fault vector=0x000000000000000e",
               b"IndianOS: fault CR2=0x0000000060000000",
               b"IndianOS: PID 4 FAULTED", b"IndianOS: PID 4 REAPED",
               COMPOSITOR_PREPARED, COMPOSITOR.encode(), STALE_COMPOSITOR_DENIED,
               FRAME_ALLOCATED, FRAME_PRESENTED, FRAME_PUBLISHED, COMPOSITOR_REPLACED)
    complete = tail.replace(b"\r", b"").split(b"\n")[:-1]
    if any(marker in line and line != marker for marker in markers for line in complete):
        raise AssertionError("PID4 recovery record has an untrusted prefix")
    restored = tail.find(COMPOSITOR_REPLACED)
    if restored < 0 or complete.count(COMPOSITOR_REPLACED) != 1:
        raise AssertionError("live compositor replacement did not become ready")
    phase = tail[:restored + len(COMPOSITOR_REPLACED)]
    phase_records = (phase + b"\n").replace(b"\r", b"").split(b"\n")[:-1]
    cursor = 0
    for marker in markers:
        if phase.count(marker) != 1 or phase_records.count(marker) != 1:
            raise AssertionError("PID4 recovery lacks exact fresh replacement evidence")
        position = phase.find(marker, cursor)
        if position < 0:
            raise AssertionError("PID4 recovery evidence is out of order")
        cursor = position + len(marker)
    if phase_records.count(FRAME_RELEASED) != 1 or phase.find(FRAME_RELEASED) > phase.find(COMPOSITOR_PREPARED):
        raise AssertionError("PID4 replacement lacks prior staging buffer release")
    verify_frame_publication(phase + b"\n")
    forbidden = (READY.encode(), LIFECYCLE_READY.encode(), SURFACES_GUARDED.encode(),
                 b"restarting retired service session", b"retired session verified restart-safe",
                 b"restart budget exhausted", b"reason=desktop-session-failed", b"FATAL",
                 LEASE_ACQUIRED)
    if any(marker in tail for marker in forbidden):
        raise AssertionError("PID4-only recovery restarted clients or fell back to full-session recovery")
    for pid in (1, 2, 3):
        for state in ("READY", "EXITED", "FAULTED", "REAPED"):
            marker = f"IndianOS: PID {pid} {state}".encode()
            if marker in phase:
                raise AssertionError("live client was retired or recreated during PID4 recovery")
    if tail.count(b"IndianOS: PID 4 FAULTED") != 1:
        raise AssertionError("unexpected repeated compositor fault")
    stopped = tail.find(STOPPED.encode())
    if stopped >= 0 and stopped < restored:
        raise AssertionError("recovered session stopped before compositor readiness")
    if tail.count(b"IndianOS: PID 4 REAPED") != (2 if stopped >= 0 else 1):
        raise AssertionError("PID4 recovery has unexpected process retirements")


def verify_recovered_shutdown(output: bytes, injection_at: int, shutdown_at: int) -> None:
    verify_transient_restart(output, injection_at)
    if not injection_at < shutdown_at < len(output):
        raise AssertionError("invalid recovered shutdown boundary")
    startup = output[:output.find(READY.encode())]
    verify_pixel_startup(startup)
    tail = output[shutdown_at:]
    markers = (DURABLE_SHUTDOWN.encode(), b"desktop selected volume synchronized", SHUTDOWN.encode())
    at = [tail.find(marker) for marker in markers]
    if min(at) < 0 or at != sorted(at) or tail.count(STOPPED.encode()) != 1:
        raise AssertionError("recovered session lacks one durable shutdown barrier")
    for pid in (1, 2, 3, 4):
        exited = f"IndianOS: PID {pid} EXITED".encode()
        reaped = f"IndianOS: PID {pid} REAPED".encode()
        if (tail.count(exited) != 1 or tail.count(reaped) != 1 or
                not at[0] < tail.find(exited) < tail.find(reaped) < tail.find(STOPPED.encode())):
            raise AssertionError("recovered session did not retire all original clients cleanly")


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


DESKTOP_SERVICE_PIDS = {"init": 1, "files": 2, "terminal": 3, "compositor": 4}


def verify_unready_recovery(output: bytes, name: str) -> None:
    premature_pid = DESKTOP_SERVICE_PIDS.get(name.removeprefix("premature-")) if name.startswith("premature-") else None
    if premature_pid is not None:
        failure = b"IndianOS normal desktop: service exited before durable shutdown"
        session = (b"IndianOS: R031 scheduler process exit=0x0000000000000000",
                   f"IndianOS: PID {premature_pid} EXITED".encode(),
                   f"IndianOS: PID {premature_pid} REAPED".encode(), failure)
        for marker in session:
            if output.count(marker) != 2:
                raise AssertionError("premature clean-exit scenario lacks exact actual lifecycle")
        if any(f"PID {pid} FAULTED".encode() in output for pid in DESKTOP_SERVICE_PIDS.values()):
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


def capture_boot_failure(session: QemuSession, folder: Path, name: str,
                            error: Exception) -> None:
    # Failure-only evidence: a missing serial marker cannot distinguish a
    # stalled guest, an emulator pause or a slow runner. Query the live HMP
    # before close() terminates QEMU; never resume, pause or mutate the guest.
    # These observations are diagnostic only and never enter acceptance output.
    records = [f"Original failure: {error}", f"Serial bytes: {len(session.output)}"]
    for command in ("info status", "info registers", "info block"):
        try:
            result = session.hmp(command)
        except Exception as diagnostic_error:
            result = f"Diagnostic unavailable: {diagnostic_error}"
        records.append(f"\n--- {command} ---\n{result}")
    try:
        (folder / f"{name}.failure-monitor.log").write_text(
            "\n".join(records) + "\n", encoding="utf-8", errors="replace")
    except OSError as diagnostic_error:
        # Do not replace the actual failed acceptance assertion.
        print(f"Could not retain {name} monitor diagnostics: {diagnostic_error}", flush=True)


def unready_service_boot(qemu: str, iso: Path, ovmf: Path, disk: Path,
                         folder: Path, name: str) -> None:
    before = disk.read_bytes()
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, name)
    try:
        session.until("IndianOS recovery: reason=desktop-session-failed", 45)
        session.until("IndianOS recovery: read-only serial console ready", 10)
        verify_unready_recovery(bytes(session.output), name)
    except Exception as error:
        capture_boot_failure(session, folder, name, error)
        raise
    finally:
        close(process, sock, session, folder / f"{name}.serial.log")
    if disk.read_bytes() != before:
        raise AssertionError("unready-service recovery modified selected storage")
    print(f"Normal desktop {name}: bounded actual failure, complete retirement and unchanged recovery PASS")


def bounded_debugger_read(debugger: GuestDebugger, address: int, count: int) -> bytes:
    if address <= 0 or not 1 <= count <= 128:
        raise AssertionError("debugger receipt exceeds bounded source inventory")
    return b"".join(debugger.memory(address + offset, min(64, count - offset))
                    for offset in range(0, count, 64))


def private_stage_pixel(value: int) -> int:
    # Clearing a set color bit yields a different pixel that remains within
    # the same validated native RGB mask, regardless of channel ordering.
    if not 0 < value <= 0xffffffff:
        raise AssertionError("private row fixture lacks a nonblack native pixel")
    return value & (value - 1)


def pending_frame_record(packet: bytes, geometry: tuple[int, int]) -> tuple[int, int, int, int]:
    if len(packet) != 128 or packet[:4] != b"IFR1":
        raise AssertionError("stopped compositor is not uploading a frame row")
    width, height, y, count, pixel_format = struct.unpack_from("<4HI", packet, 4)
    token, epoch, pointer, size, sequence = struct.unpack_from("<5Q", packet, 16)
    if ((width, height) != geometry or count != width or not 0 <= y < height
            or pixel_format != 1 or not token or not epoch or not sequence
            or pointer < 0x400000 or pointer >= 0x00007ffff0000000
            or size != width * 4 or packet[56:] != bytes(72)):
        raise AssertionError("stopped compositor lacks a genuine bounded pending frame")
    return y, token, epoch, sequence


def replacement_patch(original: bytes, replacement: bytes, failure: str) -> tuple[int, bytes]:
    """Only mutate future ELF input in an isolated debugger-controlled guest."""
    if len(original) < 64 or len(replacement) != len(original):
        raise AssertionError("replacement fixture changed ELF size")
    if failure == "load":
        if original[:4] != b"\x7fELF":
            raise AssertionError("replacement load fixture lacks a genuine ELF")
        return 0, b"\x00"
    if failure not in ("unready", "repeat"):
        raise AssertionError("invalid replacement source fixture")
    changed = [index for index, (old, new) in enumerate(zip(original, replacement)) if old != new]
    if not changed or changed[0] < 64 or changed[-1] - changed[0] >= 32:
        raise AssertionError("replacement fixture changed more than bounded entry instructions")
    return changed[0], replacement[changed[0]:changed[-1] + 1]


def verify_replacement_failure(output: bytes, injection_at: int, failure: str) -> None:
    if failure not in ("load", "unready", "repeat") or not 0 < injection_at < len(output):
        raise AssertionError("invalid failed replacement evidence boundary")
    if output[:injection_at].count(READY.encode()) != 1:
        raise AssertionError("replacement failure was not injected into a live session")
    tail = output[injection_at:]
    fault = b"IndianOS: PID 4 FAULTED"
    reap = b"IndianOS: PID 4 REAPED"
    recovery = b"IndianOS recovery: read-only serial console ready"
    ordered = (b"IndianOS: fault vector=0x000000000000000e",
               b"IndianOS: fault CR2=0x0000000060000000", fault, reap)
    complete = tail.replace(b"\r", b"").split(b"\n")[:-1]
    if any(marker in line and line != marker for marker in ordered + (recovery,) for line in complete):
        raise AssertionError("replacement failure record has an untrusted prefix")
    cursor = 0
    for marker in ordered:
        cursor = tail.find(marker, cursor)
        if cursor < 0 or complete.count(marker) < 1:
            raise AssertionError("replacement failure lacks the original controlled compositor fault")
        cursor += len(marker)
    if (READY.encode() in tail or COMPOSITOR_REPLACED in tail or
            complete.count(recovery) != 1 or not cursor < tail.find(recovery) or
            tail.count(RESTARTING_SESSION.encode()) > 1 or tail.count(fault) > 3):
        raise AssertionError("replacement failure escaped bounded read-only fallback")
    if any(f"IndianOS: PID {pid} FAULTED".encode() in tail for pid in (1, 2, 3)):
        raise AssertionError("compositor failure faulted an original client")
    prepared = tail.count(COMPOSITOR_PREPARED)
    if prepared != (0 if failure == "load" else 1):
        raise AssertionError("replacement failure used an unexpected local retry count")
    if failure == "load" and b"G4 compositor-only replacement failed; full-session recovery required" not in tail:
        raise AssertionError("replacement load failure did not enter the documented fallback")
    if failure == "unready" and b"compositor readiness budget exhausted" not in tail:
        raise AssertionError("withheld compositor readiness was not bounded")
    if failure == "repeat" and tail.count(fault) < 2:
        raise AssertionError("replacement fault was not actually exercised")


def transient_service_boot(qemu: str, iso: Path, ovmf: Path, disk: Path,
                           folder: Path, xorriso: str, nm: str = "nm",
                           failure: str | None = None) -> None:
    if failure not in (None, "load", "unready", "repeat"):
        raise AssertionError("unknown compositor replacement failure")
    name = "transient-compositor" + ("-" + failure if failure else "")
    sources = folder / (name + "-sources")
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
    target = unique_symbol(nm, service, ("indianos_desktop_frame_row",))
    expected = executable_bytes(compositor, target, 8)
    surface_ledger = unique_symbol(nm, kernel, ("DESKTOP_SURFACE_LAYOUT",))
    pixel_ledger = unique_symbol(nm, kernel, ("DESKTOP_PIXEL_SURFACE_LAYOUT",))
    scratch_ledger = unique_symbol(nm, kernel, ("DESKTOP_APP_PIXEL_GUARDS",))
    clients_ledger = unique_symbol(nm, kernel, ("DESKTOP_CLIENT_INCARNATIONS",))
    roles_ledger = unique_symbol(nm, kernel, ("DESKTOP_PROCESS_GENERATIONS",))
    symbols = subprocess.run([nm, "-S", "-n", "-C", str(kernel)], check=True,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
    guards = stack_guard_records(symbols.stdout.decode("ascii"))
    source_words = compositor_source_words(symbols.stdout.decode("ascii")) if failure else None
    unmapped = 0x60000000
    original = disk.read_bytes()
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, name, debugger=True)
    path = session.debugger_path
    debugger = None
    evidence = [f"kernel ACTIVE_PROCESS={active:#x}; compositor ROW={target:#x}",
                f"expected code={expected.hex()}; injection RIP={unmapped:#x}"]
    try:
        session.until(READY, 45)
        verify_boot_stack(bytes(session.output))
        initial = session.shot(folder / (name + "-initial.ppm"))
        check_two_windows(initial)
        # Keep a genuine PID2 read capability and PID3 committed state alive
        # across replacement. A new Files process cannot fake this token.
        held = len(session.output)
        acknowledged_key(session, "l", LEASE_CONTROL_ACK)
        session.until(LEASE_ACQUIRED.decode(), 20, since=held)
        acknowledged_key(session, "2", "R031.1 user switched to Terminal app")
        acknowledged_key(session, "ret")
        for command in ("touch revived", "put revived=saved", "cat revived"):
            type_named_command(session, command)
            submitted = acknowledged_key(session, "ret")
            marker = NAMED_READBACK if command.startswith("cat ") else NAMED_COMMIT
            session.until(marker, 20, since=submitted)
        acknowledged_key(session, "tab")
        width, height = initial[:2]
        physical_focus(session, (width // 2, height // 2), (100, 90), "Files")
        pointer_button(session, (100, 90), 1)
        before_fault = settled_frame(session, folder / (name + "-before-fault.ppm"))
        verify_text_pixels(before_fault, "files-lock", "READ LOCKED")
        fault_media = disk.read_bytes()
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
        staged_identity = None
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
                packet = bounded_debugger_read(debugger, debugger.register(5), 128)
                y, token, epoch, sequence = pending_frame_record(packet, initial[:2])
                evidence.append(f"pending y={y} token={token:#x} epoch={epoch:#x} sequence={sequence:#x}")
                if y == 0:
                    pointer = struct.unpack_from("<Q", packet, 32)[0]
                    owner_root = stopped_root(session)
                    mapping = physical_mapping(session, owner_root, pointer)
                    if mapping is None or mapping[1:] != (True, True, True):
                        raise AssertionError("staging fixture is not a PID4 user RW/NX row")
                    for _, root, _ in client_incarnation_records(bounded_debugger_read(debugger, clients_ledger, 72)):
                        peer = physical_mapping(session, root, pointer)
                        if peer is not None and peer[0] == mapping[0]:
                            raise AssertionError("staging fixture aliases another client's page")
                    pixel = int.from_bytes(debugger.memory(pointer, 4), "little")
                    fixture = struct.pack("<I", private_stage_pixel(pixel))
                    if debugger.command(f"M{pointer:x},4:{fixture.hex()}") != "OK" or debugger.memory(pointer, 4) != fixture:
                        raise AssertionError("debugger could not alter the isolated private row fixture")
                    staged_identity = token, epoch, sequence
                    evidence.append(f"private row0 native pixel {pixel:#x}->{int.from_bytes(fixture, 'little'):#x}")
                if y == 1:
                    if staged_identity != (token, epoch, sequence):
                        raise AssertionError("row1 does not follow the altered private row0 transaction")
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
        staged = session.shot(folder / (name + "-private-staging.ppm"))
        if staged != before_fault:
            raise AssertionError("incomplete PID4 frame changed the visible framebuffer")
        clients_before = bounded_debugger_read(debugger, clients_ledger, 72)
        roles_before = debugger.memory(roles_ledger, 32)
        client_incarnation_records(clients_before)
        if failure is not None:
            pointer, length = compositor_source_extent(debugger, source_words, len(compositor))
            if length != len(compositor) or debugger.memory(pointer, 64) != compositor[:64]:
                raise AssertionError("replacement source is not the current compositor ELF")
            replacement = (bytes(len(compositor)) if failure == "load" else
                           service_entry_code(raw, b"/bin/compositor",
                                              UNREADY_YIELD if failure == "unready" else b"\x0f\x0b"))
            start, payload = replacement_patch(compositor, replacement, failure)
            if debugger.memory(pointer + start, len(payload)) != compositor[start:start + len(payload)]:
                raise AssertionError("replacement mutation source changed before injection")
            if debugger.command(f"M{pointer + start:x},{len(payload):x}:{payload.hex()}") != "OK":
                raise AssertionError("debugger refused isolated replacement source mutation")
            if debugger.memory(pointer + start, len(payload)) != payload:
                raise AssertionError("replacement source mutation was not installed")
            evidence.append(f"future replacement {failure}: source offset {start:#x}, bytes {payload.hex()}")
        verify_debug_stack_guards(debugger, guards, evidence, "PID4 cloned root")
        surface_layout = debugger.memory(surface_ledger, 64)
        surfaces = surface_guard_records(surface_layout)
        verify_debug_surface_guards(debugger, surfaces, evidence, "PID4 cloned root")
        pixel_layout = debugger.memory(pixel_ledger, 64)
        pixel_surfaces = surface_guard_records(pixel_layout, 1188)
        verify_debug_surface_guards(debugger, pixel_surfaces, evidence, "PID4 retained pixels")
        cloned_root = stopped_root(session)
        scratch = app_scratch_records(debugger.memory(scratch_ledger, 64), cloned_root, pages=75)
        verify_app_scratch_mappings(session, scratch, cloned_root, evidence)
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
        if failure is not None:
            session.until("IndianOS recovery: read-only serial console ready", 45, since=injection_at)
            verify_replacement_failure(bytes(session.output), injection_at, failure)
            if disk.read_bytes() != fault_media:
                raise AssertionError("failed compositor replacement modified committed storage")
            verify_guard(original, disk.read_bytes())
            print(f"Normal desktop PID4-only replacement {failure}: controlled live fault, bounded fallback and unchanged media PASS")
            return
        session.until(COMPOSITOR_REPLACED.decode(), 30, since=injection_at)
        session.until("R031.1 application launcher OPEN", 20, since=injection_at)
        verify_transient_restart(bytes(session.output), injection_at)
        # Check genuine privileged generation/root receipts, not just a READY
        # line or an unchanged application VA (which a reload could reuse).
        session.hmp("stop")
        debugger = GuestDebugger(path)
        if not debugger.command("?").startswith(("S", "T")):
            raise AssertionError("replacement receipt inspection did not stop the guest")
        debugger.command("qSupported:xmlRegisters=i386")
        debugger.command("qXfer:features:read:target.xml:0,fff")
        verify_preserved_clients(clients_before, bounded_debugger_read(debugger, clients_ledger, 72),
                                 roles_before, debugger.memory(roles_ledger, 32))
        if debugger.memory(scratch_ledger, 64) != struct.pack("<8Q", *(word for row in scratch for word in row)):
            raise AssertionError("PID4-only restart recreated a client canvas")
        if debugger.command("D") != "OK":
            raise AssertionError("replacement receipt debugger failed to detach")
        debugger.close(); debugger = None
        session.hmp("cont")
        fresh = session.shot(folder / (name + "-restarted.ppm"))
        check_two_windows(fresh)
        # The interrupted toggle completes once. Close its popup before exact
        # glyph comparisons, including at the supported 640x480 geometry.
        acknowledged_key(session, "a", "R031.1 application launcher CLOSED")
        # The titlebar was captured with the button held when PID4 faulted.
        # Two held motions distinguish latch quarantine from a spurious new
        # capture at the first motion. Only a release can rearm capture.
        move_pointer(session, (100, 90), (120, 95), 1)
        move_pointer(session, (120, 95), (140, 100), 1)
        verify_text_pixels(session.shot(folder / (name + "-capture-cancelled.ppm")),
                           "files-note", "HELLO INDIANOS")
        pointer_button(session, (140, 100), 0)
        move_pointer(session, (140, 100), (width // 2, height // 2))
        # Continue normal physical launcher control after capture quarantine.
        acknowledged_key(session, "a", "R031.1 application launcher OPEN")
        acknowledged_key(session, "a", "R031.1 application launcher CLOSED")
        acknowledged_key(session, "2", "R031.1 user switched to Terminal app")
        acknowledged_key(session, "ret")
        for command in ("put revived=saved", "cat revived"):
            type_named_command(session, command)
            submitted = acknowledged_key(session, "ret")
            marker = NAMED_READBACK if command.startswith("cat ") else NAMED_COMMIT
            session.until(marker, 20, since=submitted)
            if command.startswith("cat "):
                session.drain_serial(0.1)
                exact_terminal_line(bytes(session.output), submitted, marker)
            else:
                verify_lease_commit(bytes(session.output), submitted)
        readback = session.shot(folder / (name + "-saved-readback.ppm"))
        verify_text_pixels(readback, "terminal", "saved")
        verify_text_pixels(readback, "files-directory", "f:readme f:revived ")
        acknowledged_key(session, "tab")
        acknowledged_key(session, "1", "R031.1 user switched to Files app")
        released = len(session.output)
        acknowledged_key(session, "u", LEASE_CONTROL_ACK)
        session.until(LEASE_RELEASED.decode(), 20, since=released)
        shutdown_at = len(session.output)
        acknowledged_key(session, "q", SHUTDOWN)
        session.until(STOPPED, 30, since=shutdown_at)
        session.drain_serial(0.1)
        verify_recovered_shutdown(bytes(session.output), injection_at, shutdown_at)
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
        if debugger.memory(pixel_ledger, 64) != pixel_layout:
            raise AssertionError("desktop restart relocated private pixel backing")
        verify_debug_surface_guards(debugger, pixel_surfaces, evidence, "reclaimed retained pixels")
        if debugger.memory(scratch_ledger, 64) != bytes(64):
            raise AssertionError("retired app scratch authority survived final cleanup")
        if debugger.command("D") != "OK":
            raise AssertionError("kernel guard debugger failed to detach")
        debugger.close()
        debugger = None
        session.hmp("cont")
    finally:
        if debugger is not None:
            debugger.close()
        close(process, sock, session, folder / (name + ".serial.log"))
        (folder / (name + "-debugger.log")).write_text("\n".join(evidence) + "\n")
    verify_guard(original, disk.read_bytes())
    print("Normal desktop PID4-only page fault: original client incarnations/canvases/read lease, private staging, fresh compositor, physical input and durable shutdown PASS")


SHUTDOWN_STALL = "IndianOS user: desktop shutdown fixture withholding STOPPED"
SHUTDOWN_EXHAUSTED = "IndianOS normal desktop: shutdown dispatch budget exhausted"
RESTART_SAFE = "IndianOS normal desktop: retired session verified restart-safe"
RESTARTING_SESSION = "IndianOS normal desktop: restarting retired service session"
RESTART_EXHAUSTED = "IndianOS normal desktop: restart budget exhausted"
FAILED_SESSION = "IndianOS recovery: reason=desktop-session-failed"


def verify_shutdown_exhaustion(output: bytes) -> None:
    attempt = (READY, DURABLE_SHUTDOWN, SHUTDOWN_STALL, SHUTDOWN_EXHAUSTED, RESTART_SAFE)
    sequence = (*attempt, RESTARTING_SESSION, *attempt, RESTART_EXHAUSTED, FAILED_SESSION)
    for marker in set(sequence):
        if output.count(marker.encode()) != sequence.count(marker):
            raise AssertionError(f"unexpected shutdown exhaustion marker count: {marker}")
    cursor = 0
    for marker in sequence:
        position = output.find(marker.encode(), cursor)
        if position < 0:
            raise AssertionError(f"shutdown exhaustion markers out of order: {marker}")
        cursor = position + len(marker)
    for forbidden in (STOPPED.encode(), b"startup dispatch budget exhausted",
                      b"desktop compositor PID4 stopped", b"FAULTED", b"FATAL"):
        if forbidden in output:
            raise AssertionError(f"shutdown exhaustion took a different path: {forbidden!r}")


def shutdown_exhaustion_boot(qemu: str, iso: Path, ovmf: Path, disk: Path,
                             folder: Path) -> None:
    name = "shutdown-exhaustion"
    before = disk.read_bytes()
    process, sock, session = spawn(qemu, iso, ovmf, disk, folder, name)
    try:
        session.until(READY, 45)
        first_shutdown = len(session.output)
        session.hmp("sendkey q 80")
        session.until(RESTARTING_SESSION, 45, since=first_shutdown)
        restart_at = session.output.index(RESTARTING_SESSION.encode(), first_shutdown)
        session.until(READY, 45, since=restart_at)
        # Allow the first emulated key release to reach the fresh session.
        session.drain_serial(0.15)
        second_shutdown = len(session.output)
        session.hmp("sendkey q 80")
        session.until(FAILED_SESSION, 45, since=second_shutdown)
        session.until("IndianOS recovery: read-only serial console ready", 10, since=second_shutdown)
        verify_shutdown_exhaustion(bytes(session.output))
    except Exception as error:
        capture_boot_failure(session, folder, name, error)
        raise
    finally:
        close(process, sock, session, folder / f"{name}.serial.log")
    if disk.read_bytes() != before:
        raise AssertionError("shutdown exhaustion modified the selected storage")
    print("Normal desktop shutdown exhaustion: two durable shutdown attempts, one restart and read-only recovery PASS")


def bad_service_images(iso: Path, folder: Path, xorriso: str,
                       shutdown_fixture: Path | None = None):
    tree = folder / "fault-iso-tree"
    checked_run([xorriso, "-osirrox", "on", "-indev", str(iso), "-extract", "/", str(tree)])
    for directory in [tree, *(p for p in tree.rglob("*") if p.is_dir())]:
        directory.chmod(directory.stat().st_mode | 0o700)
    destination = tree / "boot/initramfs.img"
    original = destination.read_bytes()
    destination.chmod(destination.stat().st_mode | 0o200)
    cases = []
    # Init admission has its own normal-init recovery matrix. The other
    # three native Desktop roles must each fail closed before service launch.
    for role in ("files", "terminal", "compositor"):
        target = f"/bin/{role}".encode()
        cases.extend(((f"missing-{role}", target, None, "desktop-service-unavailable"),
                      (f"invalid-{role}", target, b"not an ELF", "desktop-service-invalid")))
    cases.extend((("invalid-selection", b"/etc/desktop-volume", bytes(64), "desktop-volume-selection-invalid"),
                  ("crashed-compositor", b"/bin/compositor", faulting_service(original, b"/bin/compositor"),
                   "desktop-session-failed")))
    # Every indispensable service exiting cleanly before readiness is a
    # failure, including PID1. Use the same actual ELF mutation and oracle.
    for role in DESKTOP_SERVICE_PIDS:
        target = f"/bin/{role}".encode()
        cases.append((f"premature-{role}", target, service_entry_code(original, target, PREMATURE_EXIT),
                      "desktop-session-failed"))
    cases.append(("unready-compositor", b"/bin/compositor",
                  service_entry_code(original, b"/bin/compositor", UNREADY_YIELD), "desktop-session-failed"))
    if shutdown_fixture is not None:
        cases.append(("shutdown-exhaustion", b"/bin/compositor",
                      shutdown_fixture.read_bytes(), "desktop-session-failed"))
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
    parser.add_argument("--shutdown-fixture", type=Path,
                        help="compositor ELF that withholds STOPPED after a durable shutdown")
    args = parser.parse_args()
    if args.shutdown_fixture is not None and not args.shutdown_fixture.is_file():
        parser.error(f"missing artifact {args.shutdown_fixture}")
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
        # Replay the healthy native preview/update sequence on a fresh copy.
        # A guest-only read/unlock retirement escaped host model tests; keep
        # this independent repeat and its artifacts rather than retry failures.
        disk.write_bytes(original)
        repeated = artifacts / "native-preview-repeat"
        repeated.mkdir()
        boot(args.qemu, args.iso.resolve(), args.ovmf.resolve(), disk, repeated, 0)
        verify_guard(original, disk.read_bytes())
        disk.write_bytes(original)
        abrupt_reboot(args.qemu, args.iso.resolve(), args.ovmf.resolve(), disk, artifacts)
        disk.write_bytes(original)
        interrupted_publication(args.qemu, args.iso.resolve(), args.ovmf.resolve(), disk,
                                artifacts, args.xorriso, args.nm)
        disk.write_bytes(original)
        shared_read_lease_boots(args.qemu, args.iso.resolve(), args.ovmf.resolve(), disk, artifacts)
        disk.write_bytes(original)
        transient_service_boot(args.qemu, args.iso.resolve(), args.ovmf.resolve(), disk,
                               artifacts, args.xorriso, args.nm)
        for failure in ("load", "unready", "repeat"):
            disk.write_bytes(original)
            transient_service_boot(args.qemu, args.iso.resolve(), args.ovmf.resolve(), disk,
                                   artifacts, args.xorriso, args.nm, failure)
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
        for name, modified, reason in bad_service_images(
            args.iso.resolve(), Path(temporary), args.xorriso, args.shutdown_fixture
        ):
            if name == "shutdown-exhaustion":
                shutdown_exhaustion_boot(args.qemu, modified, args.ovmf.resolve(), disk, artifacts)
            elif name == "crashed-compositor":
                crashed_service_boot(args.qemu, modified, args.ovmf.resolve(), disk, artifacts)
            elif name.startswith("premature-") or name == "unready-compositor":
                unready_service_boot(args.qemu, modified, args.ovmf.resolve(), disk, artifacts, name)
            else:
                denied_boot(args.qemu, modified, args.ovmf.resolve(), disk, artifacts, name, reason)
    print("PASS: normal four-service desktop, selected GPT persistence, physical input, guarded shutdown and corruption recovery")


if __name__ == "__main__":
    main()
