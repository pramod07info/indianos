# M1/R089 — Five-stream kernel hardening (single integration batch)

Base: `b76350f7179ebc569916f94800aa754854992393`. Target: `main`. No physical-disk tests or writes are permitted.
This is a **small real-code slice**, not a claim that 100 kernel features or the eight remaining high-level M1 gates are complete.

## Owned kernel workstreams

1. **Block boundaries** (`block.rs`): reject a new partition window when parent geometry changes during constructor preflight; add a no-I/O regression.
2. **Partition selection** (`m1_partition.rs`): fail closed for MBR empty slots with nonzero stale boot/CHS/extent metadata; add a read-only corrupt-catalog regression.
3. **Copy-on-write formatting** (`durablefs.rs`): enforce stable geometry before and after each sector blankness read and both superblock writes; add a mid-format device-change fault regression.
4. **Authenticated Terminal input** (`m1_input.rs`): mutations of recalled commands cancel and scrub stale history-navigation drafts, without losing failed-edit recovery. Add five regressions.
5. **ATA PIO** (`ata_pio.rs`): respect ATA settling interval after a hardware sector read, before post-transfer status check; test rejected non-ATA and incomplete IDENTIFY payloads.

## Acceptance constraints

- Only these five code modules and this document are modified in this batch.
- Tests and CI on **the exact PR head SHA** are mandatory before merge.
- Rust host tests, no_std checks, M1 persistent desktop/reliability, security and disposable QEMU disk acceptance remain required.
- This batch does **not** demonstrate normal PID1 + separate ring3 Terminal/Files/compositor or persistent QEMU reboot readback.
- High-level M1 gates remain unaccepted until full evidence as defined in [issue #616](https://github.com/pramod07info/indianos/issues/616).
- Requested 100-task target: **not achieved** by this commit; only the implementation and tests described above may be credited after verification. Do not convert the old 100-item task scope into completed checkboxes.

## CI / merge evidence

Complete only after GitHub confirms exact-SHA checks. Do not claim passing checks from an ancestor commit.
