# R019 workstation disk-backed editor / integration slice

## Implementation status
**WS04 — in progress** (service-level multi-operation source editing, not a visual IDE)
**WS02 — in progress** (tested COW durable writes and fault/reboot recovery on a mock sector device; not new power-loss hardware certification)

### Concrete behavior
- Open verified source bytes from mounted, checksummed \`HierVfs\` through authenticated desktop PID/generation and read capability.
- Read exact source bytes without leaking partial output when caller's buffer is too short.
- Insert, delete, replace, and append actual document bytes in a bounded editor buffer.
- Track unsaved state and refuse accidental close without an explicit discard.
- Save through \`HierVfs::overwrite\` (existing copy-on-write sector backend); reject saves when mounted filesystem generation changed.
- Reload committed bytes on explicit conflict resolution, preserving the old in-memory edit if reload fails.
- Deny reuse of stale PID generations and unauthorized writers; desktop wrapper separately checks active \`FILE_READ\` or \`FILE_WRITE\` grants.
- Host regression tests use an actual in-memory sector \`BlockDevice\`, then remount a fresh \`HierVfs\` to demonstrate persistence independently of editor object lifetime.
- Inject mid-write I/O failure and verify old committed data remains readable on remount.
- Change the real M1 persistent Desktop to expose owner-authorized editor_open/editor_replace/editor_save/editor_reload/editor_read/editor_close operations.

### Changed production code
- \`rust/kernel_core/src/m1_disk_editor.rs\` new bounded stateful editor plus seven adversarial regression tests.
- \`rust/kernel_core/src/m1_persistent_desktop.rs\` session- and capability-guarded integration endpoints.
- \`rust/kernel_core/src/lib.rs\` no_std feature-gated module registration.

### Build and verification
This one integrated source commit is submitted on \`integration/workstation-r019-disk-editor\` for the normal existing M1, S0, SHA, and full kernel QEMU pipeline. No individual 40-agent builds are requested. The checks and evidence must be inspected on the **exact new SHA**, and no automatic merge is authorized while draft.

### Not yet implemented
A graphical on-screen text editor, user-process syscall/IPC connection, full project tree, per-file revision stamps, simultaneous multi-buffer GUI editing, arbitrary filesystem capacity, hardware driver verification, or 40 launched independent AI developers. A volume-wide generation check safely rejects even unrelated concurrent writes. These require follow-up work.

The remaining thirty-eight-plus substantial tasks in issue #544 remain pending; a source commit is not counted as a full WS task until the corresponding end-to-end acceptance is demonstrated.
