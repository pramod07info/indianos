# Native positioned I/O acceptance

This extends the existing normal-profile shared-read-lease scenario in
`tools/test_normal_desktop_qemu.py`; it does not add a separate desktop, test
syscall, or virtual-machine case. The configured suite remains 52 boots.

PID2 Files holds its original read descriptor for `/shared/readme`. Physical
PS/2 input submits `touch peer`, `put peer=violet`, `patch peer 2=xy`, and
`mv peer other` through PID3 Terminal and the authenticated PID1 broker. Each
successful command requires exactly one fresh cursor-preservation record,
original-token lease verification, and commit record, in that order. The patch
additionally requires a native positioned-write completion record before the
Files verification. Missing, repeated, prefixed, stale, or reordered records
fail the oracle. The native cursor check is implemented by Files, not by this
host harness.

Both `patch readme 1=x` and `put readme=blocked` must fail while the lease is
held. The entire disk image must remain byte-for-byte unchanged, Files must
still display READ LOCKED, and the session must retain its authority without
quarantine, shutdown, or reacquisition. After physical release, the existing
replacement command must succeed. A cold reboot must display and return
`readme=permitted` and `other=vixyet`, including exact framebuffer glyphs.
Partition-neighbor guard bytes remain protected by the existing checks.

Every qualified boot also requires the native Terminal startup denial of
well-shaped direct positioned filesystem requests before desktop READY. This
prevents the broker demonstration from concealing an unintended direct grant.

Host evidence for the integrated change: 55 normal-desktop oracle tests and
202 repository Python tests pass, including compiled-production IRQ controls. These tests exercise evidence rejection, not guest behavior.
Actual QEMU execution and exact-head CI are required before accepting these
runtime claims; this document alone does not mark G5, G6, or G7 complete.

Integrated cloud workspace validation also passed 4,215 normal-desktop kernel
tests, 4,314 combined-runtime tests, 3,207 ordinary-init tests, 153 M2 tests,
28 client tests, 32 filesystem-security tests, three bare-metal profile checks
and the S0 wiring audit.
The first parallel kernel compilations exhausted the workspace disk; generated
targets were removed and the checks rerun sequentially with debug symbols
disabled. No source, security requirement or CI check was removed.
