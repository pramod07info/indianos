#!/usr/bin/env python3
"""R051: type a real ring-3 Terminal save, reboot, and read the same note.

All input uses QEMU's physical 8042 keyboard/mouse path. No COM1 commands,
host-side filesystem model, or serial-only success substitutes are accepted.
The existing experimental Files-GUI ISO and disposable qualified IDE window
are required; this does not qualify normal desktop boot or physical hardware.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys
import tempfile

from test_r029_desktop_qemu import QemuSession, open_monitor, differences, check_two_windows
from test_r031_files_gui_qemu import READY, PASS, PHYSICAL, TOTAL, guard, physical_key

ACK = "R051 Terminal physical input acknowledged"
COMMITTED = "R051 PID1 committed authenticated Terminal note and refreshed Files"
DISPLAYED = "R051 Terminal displayed brokered persistent note"
PERSISTED = "R051 Files rendered persisted Terminal content"
SHUTDOWN = "R031.1 shutdown requested: notifying Files and Terminal"
PAYLOAD = "quiet note"  # q must remain editor data, not the desktop quit shortcut.
READBACK = "IndianOS user: R051 Terminal readback: " + PAYLOAD


def acknowledged_key(session: QemuSession, key: str, marker: str = ACK) -> int:
    """Require a new guest response for this single physical key transition."""
    if not (len(key) == 1 and key in "abcdefghijklmnopqrstuvwxyz12") and key not in (
        "spc", "ret", "tab", "backspace"
    ):
        raise AssertionError("unapproved physical Terminal key")
    start = len(session.output)
    session.hmp(f"sendkey {key} 80")
    session.until(marker, 20, since=start)
    if marker == ACK and SHUTDOWN.encode() in session.output[start:]:
        raise AssertionError("Terminal editor key escaped into desktop shutdown")
    return start


def type_command(session: QemuSession, command: str) -> None:
    if not command or any(c not in "abcdefghijklmnopqrstuvwxyz " for c in command):
        raise AssertionError("unsupported physical Terminal command")
    for char in command:
        acknowledged_key(session, "spc" if char == " " else char)


def changed(before: tuple[int, int, bytes], after: tuple[int, int, bytes],
            action: str, minimum: int = 8) -> None:
    if differences(before, after) < minimum:
        raise AssertionError("real QEMU framebuffer did not change for " + action)


def files_repainted(before: tuple[int, int, bytes], after: tuple[int, int, bytes]) -> None:
    # In the qualified desktop layout the left quarter contains visible Files
    # content, outside the Terminal text field. A changed SAVED label alone
    # cannot prove that Files actually repainted the newly persisted note.
    if before[:2] != after[:2]:
        raise AssertionError("QEMU display geometry changed")
    width, height = before[:2]
    changes = sum(before[2][i:i + 3] != after[2][i:i + 3]
                  for y in range(height) for x in range(width // 4)
                  for i in [(y * width + x) * 3])
    if changes < 8:
        raise AssertionError("durable save did not repaint visible Files pixels")


def exact_readback(output: bytes, since: int) -> None:
    # A substring must not accept a stale response, longer note, or echoed
    # command. The kernel prefixes each real ring-3 write with this identity.
    lines = output[since:].replace(b"\r", b"").split(b"\n")[:-1]
    if READBACK.encode() not in lines:
        raise AssertionError("Terminal did not read back the exact persisted payload")


def exact_terminal_line(output: bytes, since: int, expected: str) -> None:
    lines = output[since:].replace(b"\r", b"").split(b"\n")[:-1]
    if expected.encode() not in lines:
        raise AssertionError("missing exact fresh filesystem response")


def boot(qemu: str, iso: Path, ovmf: Path, disk: Path, folder: Path, index: int) -> None:
    monitor = folder / f"terminal-{index}.sock"
    command = [qemu, "-machine", "pc", "-m", "256M", "-smp", "1",
               "-display", "none", "-vga", "std",
               "-monitor", f"unix:{monitor},server=on,wait=off",
               "-serial", "stdio", "-no-reboot", "-no-shutdown", "-boot", "d",
               "-drive", f"if=pflash,format=raw,readonly=on,file={ovmf}",
               "-cdrom", str(iso),
               "-drive", f"if=ide,index=0,media=disk,format=raw,file={disk}"]
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.STDOUT, bufsize=0)
    sock = None
    session = None
    try:
        sock = open_monitor(monitor, process)
        session = QemuSession(process, sock)
        provision = ("R031.9 PID1 provisioned persistent /shared/readme for GUI" if index == 0
                     else "R031.9 PID1 mounted existing persistent /shared/readme")
        session.until(provision, 45)
        session.until("R031.9 Files displayed HELLO INDIANOS from persistent volume"
                      if index == 0 else PERSISTED, 15)
        session.until("R031.9 real file and directory contents rendered into Files window", 15)
        session.until("R031.1 Terminal service registered and running", 15)
        session.until(READY, 15)
        frame = session.shot(folder / f"terminal-{index}-initial.ppm")
        check_two_windows(frame)
        if PHYSICAL.encode() in session.output:
            raise AssertionError("physical origin evidence preceded the first injected key")

        # Preserve the original desktop shutdown contract: six real actions
        # and a physical mouse event, with all retained-frame/dispatch bounds.
        actions = [("a", "R031.1 application launcher OPEN"),
                   ("a", "R031.1 application launcher CLOSED"),
                   ("2", "R031.1 user switched to Terminal app"),
                   ("1", "R031.1 user switched to Files app"),
                   ("2", "R031.1 user switched to Terminal app"),
                   ("h", "R031.1 Files window moved")]
        for step, (key, marker) in enumerate(actions):
            frame = physical_key(session, key, marker, folder, index, step, frame, step == 0)
        session.hmp("mouse_move 100 135")
        session.drain_serial(0.2)
        session.hmp("mouse_button 1")
        session.drain_serial(0.2)
        session.hmp("mouse_button 0")
        session.until("R031.1 physical PS2 mouse event reached desktop service", 15)
        # A mouse click may have changed focus. First establish Files, then
        # prove a real focus/pixel transition back to Terminal before typing.
        acknowledged_key(session, "1", "R031.1 user switched to Files app")
        frame = session.shot(folder / f"terminal-{index}-files-focus.ppm")
        frame = physical_key(session, "2", "R031.1 user switched to Terminal app",
                             folder, index, 8, frame, False)

        acknowledged_key(session, "ret")
        editing = session.shot(folder / f"terminal-{index}-editor.ppm")
        changed(frame, editing, "Terminal editor activation")
        command_text = "write " + PAYLOAD if index == 0 else "cat"
        type_command(session, command_text)
        typed = session.shot(folder / f"terminal-{index}-typed.ppm")
        changed(editing, typed, "physically typed command", 8 if index == 0 else 1)
        submitted = acknowledged_key(session, "ret")
        if index == 0:
            session.until(COMMITTED, 15, since=submitted)
            session.until(PERSISTED, 15, since=submitted)
            session.until("R031.9 Files refreshed on-disk content after desktop IPC", 15,
                          since=submitted)
        else:
            session.until(DISPLAYED, 15, since=submitted)
            session.until(READBACK, 15, since=submitted)
            # Drain to a complete serial line before its exact-value check.
            session.drain_serial(0.1)
            exact_readback(bytes(session.output), submitted)
        result = session.shot(folder / f"terminal-{index}-result.ppm")
        changed(typed, result, "durable save and Files refresh" if index == 0
                else "rebooted note displayed in Terminal")

        if index == 0:
            files_repainted(typed, result)
        # Additional real filesystem commands execute through PID1, rather
        # than formatting a client-side fixture. Repeat on the same disk reboot.
        for text, expected in (("ls", "IndianOS user: R055 Terminal directory: f:readme"),
                               ("stat", "IndianOS user: R055 Terminal stat: 10 BYTES")):
            type_command(session, text)
            typed_extra = session.shot(folder / f"terminal-{index}-{text}-typed.ppm")
            command_start = acknowledged_key(session, "ret")
            session.until(expected, 15, since=command_start)
            session.drain_serial(0.1)
            exact_terminal_line(bytes(session.output), command_start, expected)
            result = session.shot(folder / f"terminal-{index}-{text}-result.ppm")
            changed(typed_extra, result, "brokered filesystem " + text)

        acknowledged_key(session, "tab")
        left = session.shot(folder / f"terminal-{index}-left-editor.ppm")
        changed(result, left, "physical Tab leaving the editor")
        physical_key(session, "q", SHUTDOWN, folder, index, 9, left, False)
        session.until("R031.1 Files service stopped and capabilities revoked", 15)
        session.until("R031.1 session manager exiting after app services", 15)
        for pid in (1, 2, 3):
            session.until(f"IndianOS: PID {pid} REAPED", 15)
        session.until(PASS, 15)
        print(f"R051 Terminal QEMU boot {index + 1}/2 passed: physical input, real pixels, cleanup")
    except Exception:
        if session:
            print(session.output[-12000:].decode("utf-8", "replace"), file=sys.stderr)
        raise
    finally:
        if session:
            session.selector.close()
        if sock:
            sock.close()
        if process.poll() is None:
            process.terminate()
        try:
            process.communicate(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            process.communicate(timeout=3)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iso", type=Path, required=True)
    parser.add_argument("--qemu", required=True)
    parser.add_argument("--ovmf", type=Path, required=True)
    args = parser.parse_args()
    for path in (args.iso, args.ovmf):
        if not path.is_file():
            parser.error(f"missing artifact {path}")
    with tempfile.TemporaryDirectory(prefix="indianos-r051-terminal-") as tmp:
        folder = Path(tmp)
        disk = folder / "128-disposable-sectors.raw"
        disk.write_bytes(bytes(TOTAL * 512))
        for index in range(2):
            boot(args.qemu, args.iso.resolve(), args.ovmf.resolve(), disk, folder, index)
            guard(disk)
    print("R051 PASS: physical ring-3 Terminal write, durable Files refresh, same-disk reboot, "
          "exact brokered readback, real framebuffer changes and all three PCB cleanups")


if __name__ == "__main__":
    main()
