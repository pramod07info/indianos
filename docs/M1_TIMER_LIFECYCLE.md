# M1 masked xAPIC calibration lifecycle

The previous implementation tracked prepared and periodic-running states but
not an active masked measurement. Two consecutive successful
`x86_64_xapic_calibration_begin` calls therefore reset INITIAL while a caller
could still be timing the first interval. Pairing that reset down-counter with
the original reference interval produces a wrong periodic calibration.
Additionally, `x86_64_xapic_timer_sample_masked` accepted CURRENT after stop:
stop leaves the LVT masked and prepared, while CURRENT can retain a stale value.

An internal active-calibration flag now distinguishes prepared/stopped from
measuring. Begin rejects a duplicate measurement before any MMIO write. Stop
masks and zeros INITIAL before clearing the measurement flag. Successful
periodic start clears it as well. Sampling outside an active measurement returns
zero even when CURRENT contains a nonzero value. Invalid periodic configuration
leaves the existing measurement untouched. Callers abort a measurement with
stop before retrying begin; repeated stop remains valid. These operations retain
the existing single-BSP, IRQ-disabled and no-concurrent-user requirements.

The C ABI, dedicated vector 0x40, UC mapping validation, arithmetic calibration
bounds and QEMU reference-clock procedure are unchanged. A caller may still
supply an independently calibrated periodic count after prepare or stop.

The regression test uses the actual prepare/begin/sample/start/stop production
methods against an aligned, live local 4 KiB register array. It injects the
private virtual binding only in the test, avoiding RDMSR, port I/O and hardware
mapping. A mutex serializes tests that access shared timer state, and a drop
guard restores all modified statics even on assertion failure. The test checks
rejected duplicate begin and invalid periodic start preserve the entire register
page, stop masks/zeros hardware, stale samples are rejected, begin can retry,
and periodic-running state rejects begin. This verifies register programming
and software lifecycle, not physical countdown or genuine IRQ delivery.

Validation: locked, offline `cargo test --features timer timer::tests --
--test-threads=4` passed all 22 timer tests; locked, offline `cargo check
--no-default-features --features timer --target x86_64-unknown-none` passed.
Existing unrelated compiler warnings remain. Genuine QEMU preemption is
validated separately by the coordinator.
