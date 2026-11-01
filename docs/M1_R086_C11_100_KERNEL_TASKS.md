# IndianOS R086 C11 — 100 additional core-kernel engineering tasks

**Status: 100 planned tasks; zero C11 tasks implemented or accepted.** This is an extension in the same draft PR #734, not 100 completed capabilities.

**Planned implementation commit**: `feat(kernel/r086-c11): harden x86-64 trap boundaries, memory, scheduler, kernel ABI and fault recovery (100 tasks)`

**Five engineering workstreams / 20 distinct task targets each.** Five workstreams describe ownership; there are no independently instantiated worker agents in this planning step.

**Priority**: These tasks are kernel-layer prerequisites for M1 acceptance. They must be dependency-reviewed against existing C01–C10 tasks before implementation, and cannot displace urgent G1–G8 end-to-end completion from [issue #616](https://github.com/pramod07info/indianos/issues/616).

**Acceptance rule**: For each line below, implement meaningful behavior on the live code path, add positive and negative regression cases, demonstrate applicable freestanding no_std and genuine QEMU execution, and link observed exact-SHA CI evidence. An existing helper, empty test or code inventory alone does not count.

## Task checklist

### Workstream 1: x86-64 CPU entry, traps and privilege transitions
**Expected existing modules:** `cpu_entry.rs, interrupts.rs, paging_cpu.rs, kernel_main.rs, user_runtime.rs`; inspect ownership to avoid overlapping other agents.

- [ ] C11-T001. Validate the CPU vendor-supported feature mask before enabling kernel features.
- [ ] C11-T002. Reject unsupported CPUID topology combinations with a bounded boot diagnostic.
- [ ] C11-T003. Validate the full canonical range of user RIP before IRET/SYSRET.
- [ ] C11-T004. Validate user RSP canonicality and privilege level on every return to ring-3.
- [ ] C11-T005. Preserve callee-saved integer registers across timer interrupt context entry.
- [ ] C11-T006. Preserve user FS-base across interrupts and task switches.
- [ ] C11-T007. Preserve user GS-base across trusted kernel-entry transitions.
- [ ] C11-T008. Sanitize user-controlled RFLAGS bits before privilege return.
- [ ] C11-T009. Select a guarded dedicated interrupt stack for double faults.
- [ ] C11-T010. Detect invalid interrupt-stack IST pointers before loading the TSS.
- [ ] C11-T011. Validate IDT descriptor type and gate privilege bits before publication.
- [ ] C11-T012. Disambiguate hardware-pushed versus synthetic exception error codes.
- [ ] C11-T013. Record exception CR2 address before another fault overwrites it.
- [ ] C11-T014. Reject nested unrecoverable kernel faults without recursive panic printing.
- [ ] C11-T015. Deliver user-mode invalid-opcode faults to the offending process supervisor.
- [ ] C11-T016. Restore the correct SIMD/FPU context on task switch where hardware support exists.
- [ ] C11-T017. Verify kernel stack alignment at syscall and interrupt handler entry.
- [ ] C11-T018. Prevent user segment selectors from being installed as kernel code segments.
- [ ] C11-T019. Validate syscall MSR setup against configured kernel entry addresses.
- [ ] C11-T020. Add real QEMU ring-3 return-path acceptance for interrupt, syscall and exception transitions.

### Workstream 2: Memory-map validation, page-fault semantics and safe reclamation
**Expected existing modules:** `boot.rs, pmm.rs, paging.rs, paging_cpu.rs, vmm.rs, user_runtime.rs`; inspect ownership to avoid overlapping other agents.

- [ ] C11-T021. Sort and merge adjacent usable boot memory ranges only when attributes agree.
- [ ] C11-T022. Reject overlapping firmware usable and reserved regions without ambiguous ownership.
- [ ] C11-T023. Reserve framebuffer and ACPI regions before physical-page allocation begins.
- [ ] C11-T024. Prevent PMM bitmap backing pages from reentering the allocatable pool.
- [ ] C11-T025. Track usable memory capacity separately from firmware-reported physical capacity.
- [ ] C11-T026. Validate page-table root alignment before loading CR3.
- [ ] C11-T027. Reject noncanonical virtual addresses during page-table walks.
- [ ] C11-T028. Enforce upper-level user/supervisor flag consistency across all page-table levels.
- [ ] C11-T029. Validate and reject a page table entry with incompatible huge-page flags.
- [ ] C11-T030. Support safe huge-page split when applying a smaller protected mapping.
- [ ] C11-T031. Prevent virtual mapping updates from partially succeeding after page-table allocation failure.
- [ ] C11-T032. Release intermediate page tables only when no child entry remains live.
- [ ] C11-T033. Implement deterministic COW page fault handling for shared read-only file pages.
- [ ] C11-T034. Abort COW allocation safely when physical frames are exhausted.
- [ ] C11-T035. Prevent mapping a user buffer over the kernel stack guard region.
- [ ] C11-T036. Zero newly allocated user-visible pages before first mapping.
- [ ] C11-T037. Zero reclaimed user page contents before mapping to a different process.
- [ ] C11-T038. Revalidate memory permissions after a COW-backed mapping changes ownership.
- [ ] C11-T039. Bound the number of recoverable user page faults within one scheduler quantum.
- [ ] C11-T040. QEMU-test stale and corrupt page-table entries with verified kernel containment.

### Workstream 3: Preemptive scheduler, wait queues and synchronization
**Expected existing modules:** `timer.rs, user_runtime.rs, process_events.rs, service_supervisor.rs, interrupts.rs`; inspect ownership to avoid overlapping other agents.

- [ ] C11-T041. Implement a scheduler invariant checker for ready/running/blocked membership.
- [ ] C11-T042. Prevent a process from appearing twice on the runnable queue after an IRQ.
- [ ] C11-T043. Publish voluntary yield and timer preemption through a common scheduling transition.
- [ ] C11-T044. Keep sleeping tasks off the runnable queue until their wake deadline.
- [ ] C11-T045. Use overflow-safe monotonic ticks when comparing timer wake deadlines.
- [ ] C11-T046. Make cancellation of an armed timer idempotent for the same process incarnation.
- [ ] C11-T047. Wake a blocked IPC receiver when the first authenticated event arrives.
- [ ] C11-T048. Ensure a spurious wakeup rechecks its original blocking predicate.
- [ ] C11-T049. Implement bounded wait-queue membership under process termination races.
- [ ] C11-T050. Prevent a waiting process from sleeping after its event has already arrived.
- [ ] C11-T051. Define lock acquisition ordering for scheduler and process-event state.
- [ ] C11-T052. Prohibit sleeping while holding interrupt-dispatch-owned locks.
- [ ] C11-T053. Implement bounded spin-lock contention diagnostics in debug QEMU builds.
- [ ] C11-T054. Restore the original interrupt-enable state after nested critical sections.
- [ ] C11-T055. Reject scheduler selection of processes already marked terminated.
- [ ] C11-T056. Ensure parent wait operations cannot reap a child more than once.
- [ ] C11-T057. Implement deterministic task timeout cleanup after canceled blocking syscalls.
- [ ] C11-T058. Keep kernel idle execution interruptible without busy-loop starving timers.
- [ ] C11-T059. Stress scheduler wait/wakeup transitions with adversarial IRQ timing in QEMU.
- [ ] C11-T060. Verify runnable queue and process totals return to baseline after crash/restart storms.

### Workstream 4: Kernel ABI boundary, object handles and secure IPC
**Expected existing modules:** `syscall.rs, ipc.rs, capability.rs, fs_handles.rs, process_events.rs, user_runtime.rs`; inspect ownership to avoid overlapping other agents.

- [ ] C11-T061. Reject syscall numbers outside the advertised kernel ABI table.
- [ ] C11-T062. Return a deterministic not-supported status for reserved syscall slots.
- [ ] C11-T063. Verify pointer argument length multiplication before mapping a user slice.
- [ ] C11-T064. Reject a user memory slice whose end crosses the canonical user address ceiling.
- [ ] C11-T065. Copy syscall input into kernel-owned buffers before mutating privileged state.
- [ ] C11-T066. Ensure failed output copy leaves sensitive kernel structs outside user memory.
- [ ] C11-T067. Define kernel-object handle kind tags for file, process, and IPC resources.
- [ ] C11-T068. Reject cross-kind handle substitution even when numeric identifiers collide.
- [ ] C11-T069. Bind every kernel handle lookup to a live process-generation token.
- [ ] C11-T070. Invalidate object handles atomically before recycling their numeric slots.
- [ ] C11-T071. Prevent capability duplication from increasing rights beyond the source handle.
- [ ] C11-T072. Restrict cross-process capability transfers to an explicitly authorized recipient.
- [ ] C11-T073. Revoke capabilities held by pending IPC messages when sender ownership expires.
- [ ] C11-T074. Preserve FIFO order among equal-priority messages to the same receiver.
- [ ] C11-T075. Report bounded mailbox capacity errors without committing partial messages.
- [ ] C11-T076. Reject replies targeting a request identifier already canceled or retired.
- [ ] C11-T077. Validate IPC envelope version and payload size before touching service tables.
- [ ] C11-T078. Prevent untrusted userland from registering privileged kernel event source types.
- [ ] C11-T079. Add a negative QEMU scenario for forged handle, stale generation and bad user-copy.
- [ ] C11-T080. Verify syscall entry/exit resource accounting is balanced after interrupted requests.

### Workstream 5: Kernel fault containment, diagnostics and recovery validation
**Expected existing modules:** `kernel_main.rs, console.rs, boot_staging.rs, service_supervisor.rs, durablefs.rs, tests/, .github/workflows/`; inspect ownership to avoid overlapping other agents.

- [ ] C11-T081. Introduce stable machine-readable kernel boot-phase identifiers in serial diagnostics.
- [ ] C11-T082. Emit a bounded nonrecursive kernel panic record when serial output is unavailable.
- [ ] C11-T083. Store crash counters with integrity protection in a recovery-safe location.
- [ ] C11-T084. Differentiate user process crash reason from a kernel-wide fatal exception.
- [ ] C11-T085. Ensure a userland crash dump excludes kernel-only page contents.
- [ ] C11-T086. Bound crash-report formatting to avoid allocating during low-memory panic.
- [ ] C11-T087. Record the exact CPU vector and process incarnation for recoverable user faults.
- [ ] C11-T088. Detect repeated PID1 crash loops and transition to an explicit recovery profile.
- [ ] C11-T089. Prevent corrupted boot recovery metadata from selecting an unsafe writable disk.
- [ ] C11-T090. Preserve last known good boot configuration when a service manifest update fails.
- [ ] C11-T091. Check log ring capacity so flood traffic cannot corrupt adjacent kernel memory.
- [ ] C11-T092. Protect kernel trace producer/consumer indices against interrupt races.
- [ ] C11-T093. Add a rate-limited diagnostic for repeated invalid guest syscalls.
- [ ] C11-T094. Make debug kernel telemetry opt-in and redact file contents and credentials.
- [ ] C11-T095. Provide a QEMU diagnostic for OOM, failed boot service, and failed disk mount.
- [ ] C11-T096. Add fault injection for a kernel memory allocation failure during PID1 launch.
- [ ] C11-T097. Add fault injection for an IRQ arriving during a process teardown.
- [ ] C11-T098. Add fault injection for a disk error while persisting a recovery record.
- [ ] C11-T099. Require QEMU watchdog failure to preserve serial and exit-code evidence.
- [ ] C11-T100. Publish a repeatable exact-SHA kernel acceptance report for boot, isolation, faults, and recovery.

## Coordinator evidence
- [ ] Map each of the 100 task IDs to actual changed production files, direct regression tests, and exact-sha CI/QEMU evidence.
- [ ] Ensure no duplicate count with C01–C10 or the merged R085 checklist; pre-existing functions are not re-credited.
- [ ] Verify all necessary host, no_std, QEMU ring-3 boot, security, and fault/recovery CI jobs for the commit before merge.
- [ ] Describe supported CPU/hardware scope and unresolved M1 gate blockers.
- [ ] Never assert five agents or workstations ran without actual execution records.
