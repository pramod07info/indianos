"""Negative controls for actual normal desktop QEMU evidence, not a guest mock."""
from pathlib import Path
import struct
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
try:
    import test_normal_desktop_qemu as harness
finally:
    sys.path.pop(0)


class NormalDesktopControls(unittest.TestCase):
    def test_frame_settlement_requires_identical_real_samples_with_bounded_wait(self):
        partial = (640, 480, b"partial repaint")
        complete = (640, 480, b"complete repaint")
        session = Mock()
        session.shot.side_effect = [partial, complete, complete, complete]
        self.assertEqual(harness.settled_frame(session, Path("focus.ppm")), complete)
        self.assertEqual(session.shot.call_count, 4)
        self.assertEqual(session.drain_serial.call_count, 4)
        self.assertEqual(session.shot.call_args.args[0], Path("focus.settle-03.ppm"))
        session = Mock()
        session.shot.side_effect = [(640, 480, bytes([index])) for index in range(11)]
        with self.assertRaisesRegex(AssertionError, "did not settle"):
            harness.settled_frame(session, Path("focus.ppm"))
        self.assertEqual(session.shot.call_count, 11)
        self.assertEqual(session.drain_serial.call_count, 11)

    def test_minimize_requires_hidden_client_and_exact_restored_retained_pixels(self):
        frame = (640, 480, bytes(640 * 480 * 3))
        pixels = bytearray(frame[2])
        at = (320 * 640 + 500) * 3
        pixels[at:at + 3] = b"\xff\xff\xff"
        hidden = (640, 480, bytes(pixels))
        harness.verify_minimize_restore(frame, hidden, frame)
        for minimized, restored in ((frame, frame), (hidden, hidden),
                                     (hidden, (640, 480, b"truncated")),
                                     ((800, 600, hidden[2]), frame)):
            with self.subTest(minimized=minimized[:2]), self.assertRaises(AssertionError):
                harness.verify_minimize_restore(frame, minimized, restored)
        for bounds in ((-1, 0, 2, 2), (2, 2, 1, 3), (0, 0, 641, 1), (0, 0, 1, 481)):
            with self.assertRaises(AssertionError):
                harness.frame_region(frame, bounds)

    def test_suspended_editor_requires_fresh_exact_refusal_and_no_visual_change(self):
        frame = (640, 480, bytes(640 * 480 * 3))
        marker = b"IndianOS user: desktop Terminal editor suspended outside keyboard focus\n"
        stale = b"older input\n"
        harness.verify_suspended_editor(frame, frame, stale + marker, len(stale))
        harness.verify_suspended_editor(frame, frame, b"z" + marker, 0)
        for output, since in ((stale, len(stale)), (marker, len(marker)),
                              (marker[:-1], 0), (marker + marker, 0),
                              (marker + harness.ACK.encode(), 0),
                              (marker + harness.SHUTDOWN.encode(), 0)):
            with self.subTest(output=output), self.assertRaises(AssertionError):
                harness.verify_suspended_editor(frame, frame, output, since)
        with self.assertRaises(AssertionError):
            harness.verify_suspended_editor(frame, (640, 480, b"changed"), marker, 0)

    def test_surface_geometry_and_actual_debugger_guard_probes(self):
        records = [(base, base + 4096, base + 8192, base + 8192)
                   for base in (0xffff800010000000, 0xffff800010004000)]
        raw = struct.pack("<8Q", *(word for record in records for word in record))
        self.assertEqual(harness.surface_guard_records(raw), records)
        malformed = [raw[:-1], bytes(64), raw[:32] + raw[:32]]
        words = list(struct.unpack("<8Q", raw))
        words[3] += 4096
        malformed.append(struct.pack("<8Q", *words))
        for bad in malformed:
            with self.subTest(raw=bad), self.assertRaises(AssertionError):
                harness.surface_guard_records(bad)
        debugger = Mock()
        debugger.command.return_value = "E14"
        debugger.memory.return_value = b"\0"
        evidence = []
        harness.verify_debug_surface_guards(debugger, records, evidence, "PID4")
        self.assertEqual(debugger.command.call_count, 4)
        self.assertEqual(debugger.memory.call_count, 4)
        self.assertEqual(len(evidence), 2)
        debugger.command.return_value = "00"
        with self.assertRaises(AssertionError):
            harness.verify_debug_surface_guards(debugger, records, [], "PID4")
        debugger.command.return_value = "E14"
        debugger.memory.side_effect = AssertionError("payload unmapped")
        with self.assertRaises(AssertionError):
            harness.verify_debug_surface_guards(debugger, records, [], "PID4")

    def test_unready_services_require_actual_ordered_cleanup_and_bounded_recovery(self):
        verified = b"IndianOS normal desktop: retired session verified restart-safe\n"
        restart = b"IndianOS normal desktop: restarting retired service session\n"
        exhausted = b"IndianOS normal desktop: restart budget exhausted\n"
        recovery = b"IndianOS recovery: reason=desktop-session-failed\n"
        sessions = {
            "premature-compositor": (
                b"IndianOS: R031 scheduler process exit=0x0000000000000000\n"
                b"IndianOS: PID 4 EXITED\nIndianOS: PID 4 REAPED\n"
                b"IndianOS normal desktop: service exited before durable shutdown\n"),
            "unready-compositor": b"IndianOS normal desktop: startup dispatch budget exhausted\n",
        }
        for name, session in sessions.items():
            trace = session + verified + restart + session + verified + exhausted + recovery
            harness.verify_unready_recovery(trace, name)
            for bad in (trace.replace(verified, b"", 1), trace + restart,
                        restart + trace.replace(restart, b"", 1),
                        trace.replace(exhausted, b""), trace + harness.READY.encode(),
                        trace + harness.LIFECYCLE_READY.encode(),
                        trace + harness.DURABLE_SHUTDOWN.encode(), trace + b"FATAL"):
                with self.subTest(name=name, trace=bad), self.assertRaises(AssertionError):
                    harness.verify_unready_recovery(bad, name)
            if name == "premature-compositor":
                with self.assertRaises(AssertionError):
                    harness.verify_unready_recovery(trace + b"PID 4 FAULTED", name)

    def test_valid_unready_elf_fixtures_preserve_all_other_bytes(self):
        elf = bytearray(4160)
        elf[:7] = b"\x7fELF\x02\x01\x01"
        struct.pack_into("<HHIQQ", elf, 16, 2, 62, 1, 0x400000, 64)
        struct.pack_into("<HHH", elf, 52, 64, 56, 1)
        struct.pack_into("<IIQQQQQQ", elf, 64, 1, 5, 4096, 0x400000, 0x400000, 64, 64, 4096)
        name = b"/bin/compositor"
        entry = struct.pack("<HHI", len(name), 0, len(elf)) + name + elf
        archive = b"INDRAM01" + struct.pack("<II", 1, 16 + len(entry)) + entry
        for code in (harness.PREMATURE_EXIT, harness.UNREADY_YIELD):
            changed = harness.service_entry_code(archive, name, code)
            self.assertEqual(changed[:4096], elf[:4096])
            self.assertEqual(changed[4096:4096 + len(code)], code)
            self.assertEqual(changed[4096 + len(code):], elf[4096 + len(code):])
        duplicate = b"INDRAM01" + struct.pack("<II", 2, 16 + 2 * len(entry)) + entry + entry
        trailing = bytearray(archive + b"x")
        struct.pack_into("<I", trailing, 12, len(trailing))
        for source, code in ((archive, b""), (archive, bytes(33)),
                             (duplicate, harness.PREMATURE_EXIT),
                             (bytes(trailing), harness.UNREADY_YIELD)):
            with self.assertRaises(AssertionError):
                harness.service_entry_code(source, name, code)

    def test_qemu_sockets_stay_short_with_long_artifact_paths_and_cleanup_after_close(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary) / ("long-checkout-" * 8) / ("artifacts-" * 8)
            folder.mkdir(parents=True)
            process = Mock()
            process.poll.return_value = None
            process.communicate.return_value = (b"tail diagnostic\n", None)
            sock = Mock()
            session = SimpleNamespace(selector=Mock(), output=bytearray(b"boot\n"))
            with patch.object(harness.subprocess, "Popen", return_value=process) as popen, \
                    patch.object(harness, "open_monitor", return_value=sock) as monitor, \
                    patch.object(harness, "QemuSession", return_value=session):
                actual_process, actual_sock, actual_session = harness.spawn(
                    "qemu", Path("system.iso"), Path("OVMF.fd"), Path("disk.raw"),
                    folder, "partition-protective-overflow", debugger=True)
            monitor_path = monitor.call_args.args[0]
            self.assertLess(len(str(monitor_path).encode()), 108)
            self.assertLess(len(str(session.debugger_path).encode()), 108)
            self.assertEqual(monitor_path.parent, session.debugger_path.parent)
            self.assertEqual(monitor_path.parent.parent, Path("/tmp"))
            self.assertNotIn(str(folder), str(monitor_path))
            command = popen.call_args.args[0]
            self.assertIn(f"unix:{monitor_path},server=on,wait=off", command)
            self.assertIn(f"unix:{session.debugger_path},server=on,wait=off", command)
            # Only socket files are temporary; diagnostics retain their requested
            # long artifact path and the entire already-read/trailing stream.
            directory = monitor_path.parent
            output = folder / "partition-protective-overflow.serial.log"
            harness.close(actual_process, actual_sock, actual_session, output)
            self.assertFalse(directory.exists())
            self.assertEqual(output.read_bytes(), b"boot\ntail diagnostic\n")
            sock.close.assert_called_once()
            process.terminate.assert_called_once()

    def test_qemu_startup_failure_preserves_stderr_and_removes_socket_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            process = Mock()
            process.poll.return_value = 1
            diagnostic = b"qemu-system-x86_64: could not open monitor socket\n"
            process.communicate.return_value = (diagnostic, None)
            captured = []
            def refuse_monitor(path, unused_process):
                captured.append(path)
                raise AssertionError("QEMU boot exited before monitor opened")
            with patch.object(harness.subprocess, "Popen", return_value=process), \
                    patch.object(harness, "open_monitor", side_effect=refuse_monitor):
                with self.assertRaisesRegex(AssertionError, "before monitor"):
                    harness.spawn("qemu", Path("system.iso"), Path("OVMF.fd"),
                                  Path("disk.raw"), folder, "failed-start")
            self.assertEqual((folder / "failed-start.serial.log").read_bytes(), diagnostic)
            self.assertFalse(captured[0].parent.exists())
            process.kill.assert_not_called()

    def test_qemu_process_launch_error_still_releases_socket_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            directory = tempfile.TemporaryDirectory(prefix="indianos-qemu-", dir="/tmp")
            tracked = SimpleNamespace(name=directory.name, cleanup=Mock(side_effect=directory.cleanup))
            with patch.object(harness.tempfile, "TemporaryDirectory", return_value=tracked), \
                    patch.object(harness.subprocess, "Popen", side_effect=OSError("missing QEMU")):
                with self.assertRaisesRegex(OSError, "missing QEMU"):
                    harness.spawn("qemu", Path("system.iso"), Path("OVMF.fd"),
                                  Path("disk.raw"), folder, "failed-launch")
            tracked.cleanup.assert_called_once()
            self.assertFalse(Path(tracked.name).exists())
            self.assertIn(b"missing QEMU", (folder / "failed-launch.serial.log").read_bytes())

    def test_failed_write_requires_fresh_sync_refusal_and_no_success_or_shutdown(self):
        refusal = b"desktop shutdown refused: selected volume sync failed\n"
        stale = harness.NAMED_COMMIT.encode() + b"\n"
        harness.verify_failed_write(stale + refusal, len(stale))
        for bad in (b"", b"no sync proof", refusal + harness.NAMED_COMMIT.encode(),
                    refusal + harness.COMMITTED.encode(), refusal + harness.SHUTDOWN.encode(),
                    refusal + harness.STOPPED.encode()):
            with self.subTest(output=bad), self.assertRaises(AssertionError):
                harness.verify_failed_write(bad, 0)
        with self.assertRaises(AssertionError):
            harness.verify_failed_write(refusal, len(refusal))

    def graphical_field(self, text="A", field="terminal", offset=(0, 0)):
        width, height = 1024, 768
        pixels = bytearray([31, 50, 73]) * (width * height)
        frame = (width, height, pixels)
        x, y, _ = harness.text_field_bounds(frame, field)
        x, y = x + offset[0], y + offset[1]
        # Independent pinned A glyph, sufficient for the oracle controls.
        rows_by_char = {
            "A": (14, 17, 17, 31, 17, 17, 17),
            "S": (15, 16, 16, 14, 1, 1, 30),
            "V": (17, 17, 17, 17, 17, 10, 4),
            "E": (31, 16, 16, 30, 16, 16, 31),
            "D": (30, 17, 17, 17, 17, 17, 30),
        }
        for index, char in enumerate(text):
            rows = rows_by_char[char]
            for row, bits in enumerate(rows):
                for column in range(5):
                    if bits & (1 << (4 - column)):
                        for dy in range(2):
                            for dx in range(2):
                                offset = ((y + row * 2 + dy) * width + x + index * 12 + column * 2 + dx) * 3
                                pixels[offset:offset + 3] = b"\xff\xff\xff"
        return width, height, bytes(pixels)

    def test_drag_requires_exact_glyph_translation_not_cursor_or_stale_content(self):
        moved = self.graphical_field("A", "files-note", (40, 20))
        harness.verify_text_pixels(moved, "files-note", "A", (40, 20))
        for image, offset in ((moved, (0, 0)),
                              (self.graphical_field("A", "files-note"), (40, 20)),
                              (moved, (40, 21)), (moved, (2000, 0))):
            with self.subTest(offset=offset), self.assertRaises(AssertionError):
                harness.verify_text_pixels(image, "files-note", "A", offset)

    def test_graphical_readback_requires_exact_glyphs_and_cleared_trailing_cells(self):
        image = self.graphical_field()
        harness.verify_text_pixels(image, "terminal", "a")
        with self.assertRaises(AssertionError):
            harness.verify_text_pixels(image, "terminal", "B")
        with self.assertRaises(AssertionError):
            harness.verify_text_pixels(self.graphical_field("AA"), "terminal", "A")
        with self.assertRaises(AssertionError):
            harness.verify_text_pixels(self.graphical_field(""), "terminal", "A")
        # The success label cannot masquerade as the named file's content.
        # The real two-boot scenario deliberately uses VIOLET, not SAVED.
        with self.assertRaises(AssertionError):
            harness.verify_text_pixels(self.graphical_field("SAVED"), "terminal", harness.G1_PAYLOAD)
        # Even one corrupted foreground/background pixel invalidates readback;
        # a changed frame or a UART success message cannot replace the glyph.
        x, y, _ = harness.text_field_bounds(image, "terminal")
        for dx in (0, 2):
            corrupt = bytearray(image[2])
            at = (y * image[0] + x + dx) * 3
            corrupt[at:at + 3] = b"\xff\xff\xff" if dx == 0 else b"\x80\x80\x80"
            with self.subTest(dx=dx), self.assertRaises(AssertionError):
                harness.verify_text_pixels((image[0], image[1], bytes(corrupt)), "terminal", "A")

    def test_graphical_fields_use_qualified_window_geometry_and_reject_wrong_regions(self):
        image = self.graphical_field(field="files-directory")
        self.assertEqual(harness.text_field_bounds(image, "terminal"), (520, 544, 270))
        self.assertEqual(harness.text_field_bounds(image, "files-directory"), (72, 282, 264))
        self.assertEqual(harness.text_field_bounds(image, "files-note"), (72, 309, 240))
        harness.verify_text_pixels(image, "files-directory", "A")
        with self.assertRaises(AssertionError):
            harness.verify_text_pixels(image, "terminal", "A")
        for field, expected in (("unknown", "A"), ("terminal", ""),
                                ("terminal", "é"), ("terminal", "?"),
                                ("terminal", "A" * 23)):
            with self.subTest(field=field, expected=expected), self.assertRaises(AssertionError):
                harness.verify_text_pixels(image, field, expected)
        with self.assertRaises(AssertionError):
            harness.verify_text_pixels((1024, 768, image[2][:-1]), "files-directory", "A")

    def test_stopped_root_requires_one_real_physical_cr3(self):
        class Monitor:
            def __init__(self, response):
                self.response = response
            def hmp(self, command):
                assert command == "info registers"
                return self.response
        self.assertEqual(harness.stopped_root(Monitor("CR2=0 CR3=0000000000123000 CR4=20")), 0x123000)
        for response in ("", "CR3=garbage", "CR3=0", "CR3=123000 CR3=456000"):
            with self.subTest(response=response), self.assertRaises(AssertionError):
                harness.stopped_root(Monitor(response))

    def test_kernel_guard_symbols_require_aligned_nonoverlapping_complete_objects(self):
        symbols = ("ffff800000100000 0000000000005000 b indianos_kernel_core::interrupts::DF_STACK\n"
                   "ffff800000200000 0000000000084000 b indianos_kernel_core::interrupts::USER_STACKS\n")
        layout = harness.stack_guard_records(symbols)
        self.assertEqual(len(layout), 5)
        self.assertEqual(layout[0], (0xffff800000100000, 0xffff800000101000, 0xffff800000105000))
        self.assertEqual(layout[-1][2] - layout[-1][1], 128 * 1024)
        for bad in (symbols + symbols.splitlines()[0], symbols.splitlines()[0],
                    symbols.replace("00084000", "00084004"),
                    symbols.replace("ffff800000200000", "ffff800000100000"),
                    symbols.replace("ffff800000100000", "ffff800000100001"),
                    symbols.replace("ffff800000100000", "0000000000100000"),
                    symbols.replace("ffff800000100000", "fffffffffffff000")):
            with self.subTest(symbols=bad), self.assertRaises(AssertionError):
                harness.stack_guard_records(bad)

    def test_guest_guard_probe_requires_unreadable_guards_and_readable_stack_neighbors(self):
        records = [(0xffff800000100000, 0xffff800000101000, 0xffff800000105000)]
        class Debugger:
            mapped_guard = False
            missing_stack = False
            def command(self, request):
                self.request = request
                return "00" if self.mapped_guard else "E14"
            def memory(self, address, count):
                if self.missing_stack:
                    raise AssertionError("stack memory missing")
                assert count == 1 and address in (records[0][1], records[0][2] - 1)
                return b"\0"
        debugger = Debugger()
        evidence = []
        harness.verify_debug_stack_guards(debugger, records, evidence, "test")
        self.assertEqual(len(evidence), 1)
        debugger.mapped_guard = True
        with self.assertRaises(AssertionError):
            harness.verify_debug_stack_guards(debugger, records, [], "test")
        debugger.mapped_guard = False
        debugger.missing_stack = True
        with self.assertRaises(AssertionError):
            harness.verify_debug_stack_guards(debugger, records, [], "test")

    def transient_trace(self):
        old = harness.READY.encode() + b"\n"
        lines = ["IndianOS: fault vector=0x000000000000000e",
                 "IndianOS: fault CR2=0x0000000060000000",
                 "IndianOS: PID 4 FAULTED", "IndianOS: PID 4 REAPED",
                 "IndianOS normal desktop: retired session verified restart-safe",
                 "IndianOS normal desktop: restarting retired service session"]
        lines += [f"IndianOS: PID {pid} {state}" for pid in (1, 2, 3, 4)
                  for state in ("READY", "DISPATCHED")]
        lines += [harness.SURFACES_GUARDED,
                  "desktop compositor denied input and filesystem authority",
                  harness.COMPOSITOR, harness.LIFECYCLE_READY, harness.READY]
        return old + ("\n".join(lines) + "\n").encode(), len(old)

    def test_transient_restart_accepts_fresh_execution_and_later_clean_reap(self):
        trace, since = self.transient_trace()
        harness.verify_transient_restart(trace, since)
        clean = b"IndianOS: PID 4 REAPED\n" + harness.STOPPED.encode() + b"\n"
        harness.verify_transient_restart(trace + clean, since)

    def test_transient_restart_rejects_stale_missing_duplicate_and_wrong_fault_evidence(self):
        trace, since = self.transient_trace()
        for bad in (trace.replace(b"0000000060000000", b"0000000060001000"),
                    trace.replace(b"fault vector=0x000000000000000e", b"fault vector=0x0000000000000006"),
                    trace.replace(b"PID 4 FAULTED", b"PID 3 FAULTED"),
                    trace.replace(b"PID 4 REAPED", b"PID 3 REAPED"),
                    trace.replace(b"retired session verified restart-safe", b"unverified"),
                    trace.replace(b"PID 2 DISPATCHED", b"PID 2 waiting"),
                    trace.replace(harness.COMPOSITOR.encode(), b""),
                    trace + b"IndianOS: PID 4 FAULTED\n",
                    trace + harness.READY.encode(), trace + b"FATAL",
                    trace + b"restart budget exhausted", trace + b"reason=desktop-session-failed",
                    trace + b"IndianOS: PID 4 REAPED\n",
                    trace + b"IndianOS: PID 4 REAPED\nIndianOS: PID 4 REAPED\n" + harness.STOPPED.encode()):
            with self.subTest(trace=bad), self.assertRaises(AssertionError):
                harness.verify_transient_restart(bad, since)
        for stale in (0, len(trace), len(trace) + 1, -1):
            with self.subTest(since=stale), self.assertRaises(AssertionError):
                harness.verify_transient_restart(trace, stale)

    def test_transient_restart_rejects_reordered_fault_and_readiness(self):
        trace, since = self.transient_trace()
        restart = b"IndianOS normal desktop: restarting retired service session\n"
        wrong = trace[:since] + restart + trace[since:].replace(restart, b"")
        with self.assertRaises(AssertionError):
            harness.verify_transient_restart(wrong, since)
        with self.assertRaises(AssertionError):
            harness.verify_transient_restart(trace[:since] + harness.STOPPED.encode() + trace[since:], since)

    def test_debugger_inspects_only_file_backed_executable_bytes(self):
        elf = bytearray(4098)
        elf[:7] = b"\x7fELF\x02\x01\x01"
        struct.pack_into("<Q", elf, 32, 64)
        struct.pack_into("<HH", elf, 54, 56, 1)
        struct.pack_into("<IIQQQQQQ", elf, 64, 1, 5, 4096, 0x400000, 0x400000, 2, 2, 4096)
        elf[-2:] = b"\x90\xc3"
        self.assertEqual(harness.executable_bytes(elf, 0x400000, 2), b"\x90\xc3")
        for source, address, count in ((elf, 0x400001, 2), (elf, 0x400000, 0),
                                       (elf[:100], 0x400000, 1), (b"bad", 0, 1)):
            with self.assertRaises(AssertionError):
                harness.executable_bytes(source, address, count)
        struct.pack_into("<I", elf, 68, 4)
        with self.assertRaises(AssertionError):
            harness.executable_bytes(elf, 0x400000, 1)

    def test_debugger_protocol_rejects_wrong_checksums_disconnects_and_oversized_packets(self):
        class Wire:
            def __init__(self, data):
                self.data = data
                self.sent = []
            def recv(self, count):
                result, self.data = self.data[:count], self.data[count:]
                return result
            def sendall(self, data):
                self.sent.append(data)
        debugger = harness.GuestDebugger.__new__(harness.GuestDebugger)
        debugger.sock = Wire(b"+$OK#9a")
        self.assertEqual(debugger.command("?"), "OK")
        self.assertEqual(debugger.sock.sent, [b"$?#3f", b"+"])
        for invalid in (b"-$OK#9a", b"+$OK#00", b"", b"$" + b"x" * 16385):
            debugger.sock = Wire(invalid)
            with self.subTest(packet=invalid[:32]), self.assertRaises(AssertionError):
                debugger.response()

    def test_service_restart_requires_two_ordered_faults_and_exactly_one_retry(self):
        fault = (b"IndianOS: PID 4 FAULTED\nIndianOS: PID 4 REAPED\n"
                 b"IndianOS normal desktop: retired session verified restart-safe\n")
        restart = b"IndianOS normal desktop: restarting retired service session\n"
        exhausted = b"IndianOS normal desktop: restart budget exhausted\n"
        recovery = b"IndianOS recovery: reason=desktop-session-failed\n"
        trace = fault + restart + fault + exhausted + recovery
        harness.verify_bounded_restart(trace)
        for bad in (fault + recovery, trace + fault, trace + restart,
                    restart + fault + fault + exhausted + recovery,
                    trace.replace(exhausted, b""), trace + harness.READY.encode(),
                    trace + b"FATAL", trace.replace(b"PID 4 REAPED", b"PID 3 REAPED", 1)):
            with self.subTest(trace=bad), self.assertRaises(AssertionError):
                harness.verify_bounded_restart(bad)

    def test_fault_fixture_changes_only_executable_entry_bytes(self):
        elf = bytearray(4098)
        elf[:7] = b"\x7fELF\x02\x01\x01"
        struct.pack_into("<HHIQQ", elf, 16, 2, 62, 1, 0x400000, 64)
        struct.pack_into("<HHH", elf, 52, 64, 56, 1)
        struct.pack_into("<IIQQQQQQ", elf, 64, 1, 5, 4096, 0x400000, 0x400000, 2, 2, 4096)
        elf[4096:] = b"\x90\xc3"
        name = b"/bin/compositor"
        entry = struct.pack("<HHI", len(name), 0, len(elf)) + name + elf
        archive = b"INDRAM01" + struct.pack("<II", 1, 16 + len(entry)) + entry
        changed = harness.faulting_service(archive, name)
        self.assertEqual(changed[:-2], elf[:-2])
        self.assertEqual(changed[-2:], b"\x0f\x0b")
        self.assertEqual(len(changed), len(elf))
        with self.assertRaisesRegex(AssertionError, "missing"):
            harness.faulting_service(archive, b"/bin/absent")
        bad = archive.replace(b"\x7fELF", b"FAIL", 1)
        with self.assertRaisesRegex(AssertionError, "not a native ELF"):
            harness.faulting_service(bad, name)

    def test_named_listing_requires_fresh_complete_exact_records(self):
        line = b"IndianOS user: R055 Terminal directory: f:readme f:a d:docs\r\n"
        harness.verify_listing(line, 0, {"f:readme", "f:a", "d:docs"})
        for output, since in ((line, len(line)), (line.rstrip(b"\r\n"), 0),
                              (line.replace(b"f:a", b"f:b"), 0),
                              (line.replace(b"d:docs", b"f:a"), 0)):
            with self.assertRaises(AssertionError):
                harness.verify_listing(output, since, {"f:readme", "f:a", "d:docs"})

    def test_named_input_rejects_unsafe_tokens_before_any_injection(self):
        class Session:
            def hmp(self, command):
                raise AssertionError("unsafe input reached HMP")
        for command in ("", "cat ../a", "put a=x;quit", "cat a\n", "cat A"):
            with self.assertRaisesRegex(AssertionError, "unsupported physical"):
                harness.type_named_command(Session(), command)

    def trace(self):
        lines = [f"IndianOS: PID {pid} {state}" for pid in (1, 2, 3, 4)
                 for state in ("READY", "DISPATCHED")]
        prefix = ("\n".join(lines + [harness.COMPOSITOR] +
                            [marker.decode("ascii") for marker in harness.TERMINAL_AUTHORITY] +
                            [harness.READY]) + "\n").encode()
        tail = (harness.DURABLE_SHUTDOWN + "\ndesktop selected volume synchronized\n" + harness.SHUTDOWN + "\n" +
                "\n".join(f"IndianOS: PID {pid} {state}" for pid in (1, 2, 3, 4)
                          for state in ("EXITED", "REAPED")) +
                "\n" + harness.STOPPED + "\n").encode()
        return prefix + tail, len(prefix)

    def test_actual_four_process_execution_sync_and_reaping_are_required(self):
        trace, shutdown_at = self.trace()
        harness.verify_runtime(trace, shutdown_at)
        for pid in (1, 2, 3, 4):
            for state in ("READY", "DISPATCHED", "EXITED", "REAPED"):
                missing = trace.replace(f"IndianOS: PID {pid} {state}".encode(), b"absent")
                with self.subTest(pid=pid, state=state), self.assertRaises(AssertionError):
                    harness.verify_runtime(missing, shutdown_at)

    def test_healthy_lifecycle_rejects_replays_partial_records_and_premature_retirement(self):
        trace, shutdown_at = self.trace()
        harness.verify_runtime(trace.replace(harness.DURABLE_SHUTDOWN.encode(),
                                            b"q" + harness.DURABLE_SHUTDOWN.encode()), shutdown_at)
        for marker in (harness.COMPOSITOR, harness.LIFECYCLE_READY, harness.READY,
                       harness.DURABLE_SHUTDOWN, harness.STOPPED):
            for bad in (trace + marker.encode() + b"\n",
                        trace.replace(marker.encode(), b"containing " + marker.encode()),
                        trace.replace(marker.encode() + b"\n", marker.encode() + b" incomplete\n")):
                with self.subTest(marker=marker), self.assertRaises(AssertionError):
                    harness.verify_runtime(bad, shutdown_at)
        for marker in (b"IndianOS: PID 2 EXITED", b"IndianOS: PID 2 REAPED"):
            moved = marker + b"\n" + trace.replace(marker + b"\n", b"")
            with self.assertRaises(AssertionError):
                harness.verify_runtime(moved, shutdown_at + len(marker) + 1)
        for suffix in (b"IndianOS: PID 2 FAULTED\n",
                       b"IndianOS normal desktop: service exited before durable shutdown\n"):
            with self.assertRaises(AssertionError):
                harness.verify_runtime(trace + suffix, shutdown_at)

    def test_terminal_authority_controls_must_execute_before_fresh_readiness(self):
        trace, shutdown_at = self.trace()
        for marker in harness.TERMINAL_AUTHORITY:
            for bad in (trace.replace(marker, b"missing"),
                        trace.replace(marker, b"missing") + marker):
                with self.subTest(marker=marker), self.assertRaises(AssertionError):
                    harness.verify_runtime(bad, shutdown_at)

    def test_late_compositor_stale_sync_and_reordered_shutdown_are_rejected(self):
        trace, offset = self.trace()
        # Evidence received only after shutdown cannot establish startup.
        moved = trace.replace(harness.COMPOSITOR.encode(), b"") + harness.COMPOSITOR.encode()
        with self.assertRaises(AssertionError):
            harness.verify_runtime(moved, offset)
        for bad in (trace.replace(b"desktop selected volume synchronized", b"no barrier"),
                    trace.replace(harness.DURABLE_SHUTDOWN.encode(), b"not armed"),
                    trace.replace(b"desktop selected volume synchronized\n" + harness.SHUTDOWN.encode(),
                                  harness.SHUTDOWN.encode() + b"\ndesktop selected volume synchronized")):
            with self.assertRaises(AssertionError):
                harness.verify_runtime(bad, offset)
        # A valid older trace cannot authorize a later empty shutdown attempt.
        with self.assertRaises(AssertionError):
            harness.verify_runtime(trace, len(trace))

    def test_provisioning_console_and_fatal_routes_do_not_count_as_desktop(self):
        trace, offset = self.trace()
        for marker in (*harness.FORBIDDEN, b"FATAL",
                       b"restarting retired service session", b"restart budget exhausted"):
            with self.subTest(marker=marker), self.assertRaises(AssertionError):
                harness.verify_runtime(trace + marker, offset)

    def test_uart_cannot_masquerade_as_physical_input_or_visual_action(self):
        frame = (640, 480, bytes(640 * 480 * 3))
        harness.verify_uart_ignored(frame, frame, b"old physical marker", 19)
        for marker in harness.UART_ACTIONS:
            with self.assertRaises(AssertionError):
                harness.verify_uart_ignored(frame, frame, marker, 0)
        changed = bytearray(frame[2]); changed[0] = 1
        with self.assertRaises(AssertionError):
            harness.verify_uart_ignored(frame, (640, 480, bytes(changed)), b"", 0)

    def test_only_explicit_partition_bytes_may_change(self):
        original = bytes(harness.TOTAL * 512)
        changed = bytearray(original); changed[harness.START * 512] = 1
        harness.verify_guard(original, bytes(changed))
        for offset in (0, harness.START * 512 - 1,
                       (harness.START + harness.COUNT) * 512, len(original) - 1):
            bad = bytearray(original); bad[offset] = 1
            with self.subTest(offset=offset), self.assertRaises(AssertionError):
                harness.verify_guard(original, bytes(bad))
        with self.assertRaises(AssertionError):
            harness.verify_guard(original, original[:-1])

    def test_rotated_guid_fixture_has_valid_header_checksums_and_unchanged_neighbors(self):
        original = bytes(harness.TOTAL * 512)
        changed = harness.rotate_guid(original)
        for lba in (1, 255):
            header = bytearray(changed[lba * 512:lba * 512 + 92])
            self.assertEqual(header[56:72], bytes([0x99]) * 16)
            checksum = int.from_bytes(header[16:20], "little")
            header[16:20] = bytes(4)
            self.assertEqual(checksum, zlib.crc32(header))
        for lba in range(harness.TOTAL):
            if lba not in (1, 255):
                self.assertEqual(changed[lba * 512:(lba + 1) * 512], bytes(512))

    @staticmethod
    def archive(entries):
        body = b"".join(struct.pack("<HHI", len(name), 0, len(payload)) + name + payload
                        for name, payload in entries)
        return b"INDRAM01" + struct.pack("<II", len(entries), 16 + len(body)) + body

    def test_failure_fixture_changes_only_requested_archive_entry(self):
        entries = [(b"/bin/init", b"init bytes"), (b"/bin/compositor", b"ELF compositor"),
                   (b"/etc/desktop-volume", bytes(64))]
        original = self.archive(entries)
        self.assertEqual(harness.rewrite_initramfs(original, b"/bin/compositor", None),
                         self.archive([entries[0], entries[2]]))
        self.assertEqual(harness.rewrite_initramfs(original, b"/bin/compositor", b"bad"),
                         self.archive([entries[0], (b"/bin/compositor", b"bad"), entries[2]]))
        with self.assertRaises(AssertionError):
            harness.rewrite_initramfs(original, b"/bin/missing", None)

    def test_invalid_source_archive_is_not_used_to_produce_false_failure_evidence(self):
        valid = self.archive([(b"/bin/init", b"payload")])
        for bad in (b"", valid + b"trailing", valid[:-1],
                    b"BROKEN!!" + valid[8:], self.archive([(b"/bin/init", b"one"),
                                                          (b"/bin/init", b"two")])):
            with self.assertRaises(AssertionError):
                harness.rewrite_initramfs(bad, b"/bin/init", b"new")


if __name__ == "__main__":
    unittest.main()
