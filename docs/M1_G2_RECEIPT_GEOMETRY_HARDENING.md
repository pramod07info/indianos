# M1 G2 selected-media receipt integrity — consolidated implementation batch

**Scope:** G2 (defensive MBR/GPT partition selection), with G6 selected-volume admission implications. This is a source-and-regression batch, not a completed M1 gate or an additional 99-task acceptance exit.

## Implemented items

- **G2-R01 — MBR fingerprint lifetime:** pin device sector capacity across the initial sector-zero read, full MBR catalog discovery and final sector-zero read. Reject a capacity change on the final read before returning the selected partition; never issue writes.
- **G2-R02 — Whole receipt lifetime:** hold one capacity-pinned read-only adapter across both catalog scans, both raw sector-zero fingerprints and both full GPT/MBR table digest calculations. Reject a changed-capacity device at any of those read boundaries, rather than relying on separate intermediate snapshots.
- **G2-R03 — Pre-I/O GPT receipt validation:** reject a nil disk/partition GUID, a missing protective GPT MBR entry, an invalid protective range or a nonempty hybrid MBR slot before issuing any mount or health-probe disk read. Check that a genuine GPT receipt still permits a healthy selected-volume mount and read-only probe.

## Regression checks added

1. `m1_g2_final_mbr_read_capacity_change_fails_closed` injects capacity drift only on the final MBR fingerprint read and requires `Error::Io`, no extra reads and no writes.
2. `m1_g2_gpt_receipt_forgery_rejected_without_io` checks six forged receipt variants against mount and diagnostic probe, requiring zero disk reads/writes.
3. `m1_g2_healthy_gpt_receipt_mount_and_probe` proves valid GPT receipt readback and read-only health probing without further writes.

## Acceptance and evidence — still open

- Compile and run exact-PR-head Rust host and `no_std` builds. Run the complete existing MBR/GPT, COW, secure-admission and filesystem regressions; do not drop failing tests.
- Run the normal desktop and durable selected-GPT two-boot QEMU matrix on the exact commit, with genuine disk/serial/screenshot artifacts. A test helper or a green build alone does not establish guest behavior.
- Require independent code/security review and a passing post-merge `main` verification before changing high-level milestone acceptance.
- No physical disk swap, IRQ/DMA/SMP, unsafe power-loss, TPM, boot trust or release qualification is claimed. The full G2 and G6 exit conditions in `docs/M1_GATES_6_7_ACCEPTANCE.md`, `docs/M1_GATES_8_10_ACCEPTANCE.md` and issue #616 remain open.

**Batch bookkeeping:** one production file (three targeted changes and three regression tests), this evidence note, one consolidated Git commit and one PR. No automatic gate checkbox changes.

## Follow-up batch in the SAME PR — G2-R04 through G2-R08

- **G2-R04 — Full claimed MBR layout preflight:** reuse the production read-free MBR parser before either strict mount or diagnostic probe. Reject overlaps, impossible empty slots and invalid boot indicators without issuing I/O even if a forged receipt carries a matching CRC.
- **G2-R05 — Two-pass GUID selection:** require an identical full protective-MBR, primary/secondary GPT and table receipt before returning a GUID selection. A valid second GPT epoch with reused GUIDs and bounds is not stable selection.
- **G2-R06 — Pre-repair full receipt gate:** preserve the complete selected GPT table receipt, including other partitions' metadata, across a second pre-write validation before opening the selected COW repair window.
- **G2-R07 — Post-repair original receipt gate:** admit recovery only if a healthy remount matches the original pre-repair full receipt; do not bless a freshly captured different GPT table.
- **G2-R08 — Executable regressions:** tests for forged MBR layouts with zero disk I/O; valid GPT epoch switch; changed table before writable repair; and positive stable repair/readback.

**Scope boundary:** these are software checks, not physical-disk atomicity or power-loss proof. The block driver must still be quiesced by its caller. GPT/MBR CRC32 is not authentication, and none of the 13 M1 high-level gates is automatically marked complete by this additional source batch.

**CI:** new consolidated head supersedes the prior green SHA. Exact-head host, no_std, QEMU, security and durability checks must rerun and independent review is still mandatory.

### Exact-head CI recovery: GPT epoch comparison

The first R04–R08 exact-head host suite reported one failing new test: a CRC-valid GPT attributes change occurred *after* the initial catalog read, but both table digests were captured afterward. The catalog compared selected partition geometry and GUIDs only, so an equal catalog could not detect changed GPT entry attributes. The corrective production change stores all three validated GPT metadata checksums (entry array, primary header and backup header) inside the catalog, ensuring the two discovery snapshots compare GPT table/header epochs as well as GUIDs and LBAs. The focused regression covers both same-GUID attribute drift and CRC-valid first-usable-bound drift. The initial failure is preserved in workflow 43717827155 and superseded by a new exact-head run; no gate credit is inferred before the rerun succeeds.
