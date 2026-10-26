"""Host-side unit tests for strictly ordered serial milestone recognition."""
import sys
import tempfile
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from smoke_boot import (assert_milestones, assert_hardware_inventory,
                        assert_expected_exception,
                        assert_expected_pagefault, assert_expected_shadow,
                        assert_expected_usermode, assert_expected_userfault,
                        assert_expected_processes, assert_expected_scheduler,
                        assert_expected_preempt,
                        matching_ovmf_vars, MILESTONES)  # noqa: E402


class SmokeBootTests(unittest.TestCase):
    def test_hd0_boot_detects_cpu_and_reports_no_fake_qpu(self):
        assert_hardware_inventory(
            "IndianOS HD0 CPU=GenuineIntel x86-64 family=6 model=58; "
            "GPU=1 detected(no driver); NPU=0 detected(no driver); "
            "QPU=unavailable; CPU fallback=enabled")
        with self.assertRaises(AssertionError):
            assert_hardware_inventory("IndianOS HD0 CPU=GenuineIntel")
        with self.assertRaises(AssertionError):
            assert_hardware_inventory("IndianOS: kernel entered")

    def test_accepts_expected_order_with_additional_messages(self):
        assert_milestones("\nextra\n" + "\r\nother\r\n".join(MILESTONES) + "\r\n")

    def test_rejects_old_m002_only_transcript(self):
        with self.assertRaises(AssertionError):
            assert_milestones("\n".join(MILESTONES[:-2]))

    def test_accepts_page_fault_reporting(self):
        transcript = "\n".join([*MILESTONES,
            "IndianOS: triggering intentional page fault",
            "IndianOS: FATAL CPU exception vector=0x000000000000000e error=0x0",
            "IndianOS: page fault cr2=0x00007fff00000000 present=0 write=0"])
        assert_expected_pagefault(transcript)

    def test_rejects_wrong_cr2(self):
        transcript = "\n".join([*MILESTONES,
            "IndianOS: triggering intentional page fault",
            "IndianOS: FATAL CPU exception vector=0x000000000000000e",
            "IndianOS: page fault cr2=0x0000000000000000"])
        with self.assertRaises(AssertionError):
            assert_expected_pagefault(transcript)

    def test_accepts_shadow_switch_with_restoration(self):
        split = MILESTONES.index("IndianOS: milestone 004 complete")
        assert_expected_shadow("\n".join([*MILESTONES[:split],
            "IndianOS: shadow CR3 switched and restored",
            *MILESTONES[split:]]))

    def test_rejects_missing_shadow_restore(self):
        with self.assertRaises(AssertionError):
            assert_expected_shadow("\n".join(MILESTONES))

    def test_rejects_missing_m005_markers(self):
        with self.assertRaises(AssertionError):
            assert_milestones("\n".join(MILESTONES[:-3]))

    def test_rejects_no_archive_module(self):
        with self.assertRaises(AssertionError):
            assert_milestones("\n".join(MILESTONES[:-2]))

    def test_rejects_missing_staging_success(self):
        with self.assertRaises(AssertionError):
            assert_milestones("\n".join([*MILESTONES[:-1],
                "IndianOS: FATAL detached user-image staging failed"]))

    def test_ovmf_vars_template_must_match_code_and_not_be_modified(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            code = root / "OVMF_CODE_4M.fd"
            vars_template = root / "OVMF_VARS_4M.fd"
            code.write_bytes(b"firmware-code")
            self.assertIsNone(matching_ovmf_vars(code))
            vars_template.write_bytes(b"unmodified-template")
            self.assertEqual(matching_ovmf_vars(code), vars_template)
            self.assertEqual(vars_template.read_bytes(), b"unmodified-template")
            other = root / "not-ovmf.bin"
            other.write_bytes(b"stub")
            self.assertIsNone(matching_ovmf_vars(other))

    def test_usermode_transcript_must_show_syscall_and_exit(self):
        output = "\n".join([*MILESTONES,
            "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
            "IndianOS user: Namaste from IndianOS user mode!",
            "IndianOS: /bin/init exited through int 0x80",
            "IndianOS: ring-3 user program experiment complete"])
        assert_expected_usermode(output)

    def test_usermode_rejects_missing_exit(self):
        output = "\n".join([*MILESTONES,
            "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
            "IndianOS user: Namaste from IndianOS user mode!"])
        with self.assertRaises(AssertionError):
            assert_expected_usermode(output)

    def test_user_guard_fault_contained_and_cleaned(self):
        output = "\n".join([*MILESTONES,
            "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
            "IndianOS: isolated ring-3 page fault",
            "IndianOS: user page fault contained; kernel resumed",
            "IndianOS: ring-3 borrowed user tables detached",
            "IndianOS: ring-3 supervisor clone released",
            "IndianOS: ring-3 user image released",
            "IndianOS: ring-3 PMM frame count restored",
            "IndianOS: user fault isolation experiment complete"])
        assert_expected_userfault(output, "fault-guard")
        assert_expected_userfault(output, "fault-supervisor")

    def test_user_fault_requires_memory_cleanup(self):
        output = "\n".join([*MILESTONES,
            "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
            "IndianOS: isolated ring-3 page fault",
            "IndianOS: user page fault contained; kernel resumed"])
        with self.assertRaises(AssertionError):
            assert_expected_userfault(output, "fault-guard")

    def test_user_fault_rejects_kernel_panic_and_successful_sys_exit(self):
        output = "\n".join([*MILESTONES,
            "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
            "IndianOS: isolated ring-3 page fault",
            "IndianOS: user page fault contained; kernel resumed",
            "IndianOS: ring-3 borrowed user tables detached",
            "IndianOS: ring-3 supervisor clone released",
            "IndianOS: ring-3 user image released",
            "IndianOS: ring-3 PMM frame count restored",
            "IndianOS: user fault isolation experiment complete"])
        with self.assertRaises(AssertionError):
            assert_expected_userfault(output + "\nIndianOS: FATAL CPU exception", "fault-guard")
        with self.assertRaises(AssertionError):
            assert_expected_userfault(output +
                "\nIndianOS: /bin/init exited through int 0x80", "fault-supervisor")

    def test_two_unique_processes_are_reaped_before_next_launch(self):
        output = "\n".join([*MILESTONES,
            "IndianOS: M008 sequential process launch begin",
            "IndianOS: PID 1 READY",
            "IndianOS: PID 1 RUNNING",
            "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
            "IndianOS user: Namaste from IndianOS user mode!",
            "IndianOS: PID 1 EXITED",
            "IndianOS: ring-3 PMM frame count restored",
            "IndianOS: PID 1 REAPED",
            "IndianOS: PID 2 READY",
            "IndianOS: PID 2 RUNNING",
            "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
            "IndianOS user: IndianOS worker process 2 completed",
            "IndianOS: PID 2 EXITED",
            "IndianOS: ring-3 PMM frame count restored",
            "IndianOS: PID 2 REAPED",
            "IndianOS: M008 two independent processes exited and reaped"])
        assert_expected_processes(output)

    def test_rejects_process_two_starting_before_process_one_cleanup(self):
        output = "\n".join([*MILESTONES,
            "IndianOS: M008 sequential process launch begin",
            "IndianOS: PID 1 READY",
            "IndianOS: PID 1 RUNNING",
            "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
            "IndianOS user: Namaste from IndianOS user mode!",
            "IndianOS: PID 2 READY",
            "IndianOS: PID 1 EXITED",
            "IndianOS: ring-3 PMM frame count restored",
            "IndianOS: PID 1 REAPED"])
        with self.assertRaises(AssertionError):
            assert_expected_processes(output)

    def test_rejects_skipped_process_or_single_reclamation(self):
        output = "\n".join([*MILESTONES,
            "IndianOS: M008 sequential process launch begin",
            "IndianOS: PID 1 READY",
            "IndianOS: PID 1 RUNNING",
            "IndianOS: entering ring-3 /bin/init (QEMU experimental)",
            "IndianOS user: Namaste from IndianOS user mode!",
            "IndianOS: PID 1 EXITED",
            "IndianOS: ring-3 PMM frame count restored",
            "IndianOS: PID 1 REAPED",
            "IndianOS: M008 two independent processes exited and reaped"])
        with self.assertRaises(AssertionError):
            assert_expected_processes(output)

    def test_scheduler_requires_interleaved_user_contexts(self):
        names = [*MILESTONES,
            "IndianOS: M009 cooperative scheduler experiment begin",
            "IndianOS: PID 1 READY", "IndianOS: PID 2 READY",
            "IndianOS: M009 two simultaneous user address spaces ready"]
        for step in (1, 2, 3):
            for pid, label in ((1, "A"), (2, "B")):
                names += [f"IndianOS: PID {pid} DISPATCHED",
                          f"IndianOS user: M009 task {label} step {step}"]
                if step < 3:
                    names.append(f"IndianOS: PID {pid} YIELDED")
                else:
                    names += [f"IndianOS: PID {pid} EXITED",
                              f"IndianOS: PID {pid} REAPED"]
        names += ["IndianOS: M009 round-robin cooperative switching PASSED",
                  "IndianOS: M009 scheduler experiment complete"]
        assert_expected_scheduler("\n".join(names))

    def test_scheduler_rejects_missing_task_yield(self):
        names = [*MILESTONES,
            "IndianOS: M009 cooperative scheduler experiment begin",
            "IndianOS: PID 1 READY", "IndianOS: PID 2 READY",
            "IndianOS: M009 two simultaneous user address spaces ready",
            "IndianOS: PID 1 DISPATCHED",
            "IndianOS user: M009 task A step 1",
            "IndianOS: PID 2 DISPATCHED"]
        with self.assertRaises(AssertionError):
            assert_expected_scheduler("\n".join(names))

    def test_hardware_timer_preempts_two_cpu_bound_users(self):
        output = "\n".join([*MILESTONES,
            "IndianOS: R024 APIC preemptive scheduler experiment begin",
            "IndianOS: PID 1 READY", "IndianOS: PID 2 READY",
            "IndianOS: R024 xAPIC supervisor UC mapping installed",
            "IndianOS: M009 two simultaneous user address spaces ready",
            "IndianOS: R024 calibrated periodic xAPIC IRQ at 100 Hz",
            "IndianOS: PID 1 DISPATCHED",
            "IndianOS user: M009 timer task A started",
            "IndianOS: PID 1 APIC_TIMER_PREEMPTED", "IndianOS: PID 1 YIELDED",
            "IndianOS: PID 2 DISPATCHED",
            "IndianOS user: M009 timer task B started",
            "IndianOS: PID 2 APIC_TIMER_PREEMPTED", "IndianOS: PID 2 YIELDED",
            "IndianOS: PID 1 DISPATCHED",
            "IndianOS user: M009 timer task A finished",
            "IndianOS: PID 1 EXITED", "IndianOS: PID 1 REAPED",
            "IndianOS: PID 2 DISPATCHED",
            "IndianOS user: M009 timer task B finished",
            "IndianOS: PID 2 EXITED", "IndianOS: PID 2 REAPED",
            "IndianOS: R024 APIC timer-preemptive round-robin PASSED",
            "IndianOS: M009 scheduler experiment complete"])
        assert_expected_preempt(output)

    def test_hardware_timer_rejects_no_preemption(self):
        output = "\n".join([*MILESTONES,
            "IndianOS: R024 APIC preemptive scheduler experiment begin",
            "IndianOS: R024 xAPIC supervisor UC mapping installed",
            "IndianOS: M009 two simultaneous user address spaces ready",
            "IndianOS: R024 calibrated periodic xAPIC IRQ at 100 Hz",
            "IndianOS user: M009 timer task A started",
            "IndianOS user: M009 timer task B started",
            "IndianOS user: M009 timer task A finished",
            "IndianOS user: M009 timer task B finished",
            "IndianOS: R024 APIC timer-preemptive round-robin PASSED",
            "IndianOS: M009 scheduler experiment complete"])
        with self.assertRaises(AssertionError):
            assert_expected_preempt(output)

    def test_rejects_out_of_order(self):
        with self.assertRaises(AssertionError):
            assert_milestones("\n".join(reversed(MILESTONES)))

    def test_rejects_missing_event(self):
        with self.assertRaises(AssertionError):
            assert_milestones(MILESTONES[0] + "\n" + MILESTONES[2])

    def test_rejects_kernel_fatal(self):
        with self.assertRaises(AssertionError):
            assert_milestones("\n".join(MILESTONES) + "\nIndianOS: FATAL boot information invalid")

    def test_accepts_deliberate_ud2_fault(self):
        output = "\n".join([*MILESTONES[:5],
                           "IndianOS: triggering intentional UD2",
                           "IndianOS: FATAL CPU exception vector=0x0000000000000006 error=0x0"])
        assert_expected_exception(output)

    def test_rejects_unexpected_cpu_exception(self):
        output = "\n".join([*MILESTONES[:5],
                           "IndianOS: triggering intentional UD2",
                           "IndianOS: FATAL CPU exception vector=0x000000000000000e"])
        with self.assertRaises(AssertionError):
            assert_expected_exception(output)

    def test_rejects_exception_handler_that_returns(self):
        output = "\n".join([*MILESTONES[:5],
                           "IndianOS: triggering intentional UD2",
                           "IndianOS: FATAL CPU exception vector=0x0000000000000006",
                           "IndianOS: milestone 002 complete"])
        with self.assertRaises(AssertionError):
            assert_expected_exception(output)


if __name__ == "__main__":
    unittest.main()
