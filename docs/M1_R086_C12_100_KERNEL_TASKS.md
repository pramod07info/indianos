# IndianOS R086 C12 — 100 additional remaining core kernel tasks

**Status:** 100 planned; 0 implemented or accepted. Additional scope after C11, not a claim of 100 working features.

**Proposed implementation commit:** `feat(kernel/r086-c12): harden firmware, SMP, heap, timekeeping and device lifecycle (100 tasks)`

**Acceptance:** Every task requires production integration, positive/negative tests, exact-SHA host/no_std checks and QEMU evidence where applicable. Compare existing code and C01–C11 to avoid double-counting.

## Workstream 1: Boot and firmware isolation
- [ ] C12-T001. Validate UEFI memory map descriptor version.
- [ ] C12-T002. Reject truncated UEFI configuration tables.
- [ ] C12-T003. Check boot services exit map key consistency.
- [ ] C12-T004. Reserve runtime service regions before paging.
- [ ] C12-T005. Protect EFI variable writes behind a trusted service.
- [ ] C12-T006. Verify initramfs size against available physical memory.
- [ ] C12-T007. Reject overlapping initramfs and kernel image regions.
- [ ] C12-T008. Validate kernel relocation target alignment.
- [ ] C12-T009. Check KASLR entropy initialization and fallback.
- [ ] C12-T010. Protect boot arguments from untrusted pointer references.
- [ ] C12-T011. Handle absent ACPI root table without undefined reads.
- [ ] C12-T012. Validate RSDP revision and checksum.
- [ ] C12-T013. Reject malformed MADT interrupt controller entries.
- [ ] C12-T014. Validate HPET capabilities before enabling timer.
- [ ] C12-T015. Cross-check PIT fallback when HPET unavailable.
- [ ] C12-T016. Preserve firmware framebuffer pitch and pixel format.
- [ ] C12-T017. Avoid remapping firmware runtime memory as user-accessible.
- [ ] C12-T018. Record measured boot artifacts without inventing trust.
- [ ] C12-T019. Detect conflicting firmware-provided CPU counts.
- [ ] C12-T020. Verify cold-boot and warm-boot firmware paths.

## Workstream 2: SMP and interprocessor coordination
- [ ] C12-T021. Enumerate application processors from validated topology.
- [ ] C12-T022. Allocate isolated per-CPU kernel stacks.
- [ ] C12-T023. Initialize per-CPU GDT and TSS.
- [ ] C12-T024. Install per-CPU interrupt descriptor tables.
- [ ] C12-T025. Bring APs online with bounded startup acknowledgement.
- [ ] C12-T026. Handle an AP that fails to start without global hang.
- [ ] C12-T027. Keep per-CPU current task pointers coherent.
- [ ] C12-T028. Protect cross-CPU runqueue ownership transitions.
- [ ] C12-T029. Implement targeted interprocessor reschedule signals.
- [ ] C12-T030. Acknowledge interprocessor interrupts safely.
- [ ] C12-T031. Implement TLB shootdown request tracking.
- [ ] C12-T032. Wait for bounded remote TLB invalidation completion.
- [ ] C12-T033. Prevent stale TLB entries after cross-CPU unmap.
- [ ] C12-T034. Support per-CPU timer calibration.
- [ ] C12-T035. Handle CPU affinity changes for blocked tasks.
- [ ] C12-T036. Prevent task migration during FPU state save.
- [ ] C12-T037. Synchronize process termination across active CPUs.
- [ ] C12-T038. Implement per-CPU idle states without lost wakeups.
- [ ] C12-T039. Collect per-CPU interrupt and scheduler counters.
- [ ] C12-T040. Test two-vCPU and four-vCPU QEMU boot paths.

## Workstream 3: Kernel heap and memory pressure
- [ ] C12-T041. Validate heap slab alignment classes.
- [ ] C12-T042. Detect allocator metadata corruption before reuse.
- [ ] C12-T043. Bound heap allocation arithmetic overflow.
- [ ] C12-T044. Reject allocation from interrupt-unsafe heap context.
- [ ] C12-T045. Provide emergency reserve for fault diagnostics.
- [ ] C12-T046. Prevent double free in kernel heap paths.
- [ ] C12-T047. Detect use-after-free with debug quarantine.
- [ ] C12-T048. Zero sensitive kernel buffers before recycling.
- [ ] C12-T049. Separate DMA-safe allocations from general heap.
- [ ] C12-T050. Validate DMA buffer physical continuity.
- [ ] C12-T051. Bound contiguous allocation retry loops.
- [ ] C12-T052. Report allocator fragmentation metrics.
- [ ] C12-T053. Implement reclaimable page-cache accounting.
- [ ] C12-T054. Enforce per-process memory budget at kernel boundary.
- [ ] C12-T055. Notify user process on recoverable allocation failure.
- [ ] C12-T056. Avoid recursive allocations in OOM handler.
- [ ] C12-T057. Select OOM victim without killing critical PID1 blindly.
- [ ] C12-T058. Release page-cache references after process exit.
- [ ] C12-T059. Verify no leaked frames after repeated ELF load failures.
- [ ] C12-T060. Run QEMU memory-pressure scenarios with bounded RAM.

## Workstream 4: Timekeeping and kernel event delivery
- [ ] C12-T061. Initialize monotonic clock independently of wall time.
- [ ] C12-T062. Validate timer frequency against calibrated limits.
- [ ] C12-T063. Prevent nanosecond conversion overflow.
- [ ] C12-T064. Guarantee monotonic timestamps across CPU migration.
- [ ] C12-T065. Handle counter wraparound in deadline calculations.
- [ ] C12-T066. Reject negative or unrepresentable user timeout requests.
- [ ] C12-T067. Provide absolute deadline timer API.
- [ ] C12-T068. Provide relative timer API with overflow checks.
- [ ] C12-T069. Cancel timer before process-generation retirement.
- [ ] C12-T070. Prevent duplicate timeout callback delivery.
- [ ] C12-T071. Maintain deterministic ordering for equal deadlines.
- [ ] C12-T072. Bound pending timer count per process.
- [ ] C12-T073. Prevent timer starvation under interrupt storms.
- [ ] C12-T074. Expose trusted clock source metadata to userland.
- [ ] C12-T075. Isolate wall-clock adjustments from scheduler time.
- [ ] C12-T076. Validate RTC date components before publication.
- [ ] C12-T077. Handle RTC read failure with explicit unknown time.
- [ ] C12-T078. Ensure kernel watchdog heartbeat cannot be forged by userland.
- [ ] C12-T079. Report late timer interrupts without resetting deadlines.
- [ ] C12-T080. QEMU-test sleep, timeout, cancellation and reboot timing.

## Workstream 5: Kernel device lifecycle and DMA protection
- [ ] C12-T081. Assign unique ownership tokens to detected devices.
- [ ] C12-T082. Validate device BAR size arithmetic before mapping.
- [ ] C12-T083. Prevent duplicate driver binding to one device.
- [ ] C12-T084. Reject user mapping of unrestricted MMIO regions.
- [ ] C12-T085. Validate DMA descriptor address and length.
- [ ] C12-T086. Check DMA ring indices before dereference.
- [ ] C12-T087. Quarantine device after repeated invalid completions.
- [ ] C12-T088. Handle device reset while I/O requests are pending.
- [ ] C12-T089. Cancel device requests on process exit.
- [ ] C12-T090. Reject stale completion tokens after driver restart.
- [ ] C12-T091. Validate MSI vector assignment conflicts.
- [ ] C12-T092. Mask device interrupts during teardown.
- [ ] C12-T093. Unmask interrupts only after handler registration.
- [ ] C12-T094. Bound interrupt storm mitigation per device.
- [ ] C12-T095. Validate IOMMU presence before assuming DMA isolation.
- [ ] C12-T096. Document safe fallback when IOMMU is unavailable.
- [ ] C12-T097. Avoid freeing DMA buffers before device quiescence.
- [ ] C12-T098. Preserve storage write ordering during driver restart.
- [ ] C12-T099. Verify device hot-remove does not dereference stale state.
- [ ] C12-T100. QEMU-test device reset, timeout and teardown recovery.
