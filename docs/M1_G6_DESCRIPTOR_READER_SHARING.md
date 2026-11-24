# G6 bounded descriptor writes with independent readers

This change extends the selected-volume witness introduced for syscall32 path
mutations. It does not introduce another filesystem, lock table or on-disk format.
Native descriptor operations WRITE (3), TRUNCATE (6) and PWRITE (26) can publish a
new filesystem generation while another process retains a READ descriptor for an
unrelated file. Structural descriptor edits still require exclusive ownership of
the volume's descriptor leases.

Before device discovery, the authenticated writer's canonical path is compared
with every foreign descriptor's path using component boundaries in both ancestor
directions. Same-file or subtree overlap is denied even without an explicit range
lock. The ordinary range-lock checks remain in force. After mounting, every
foreign descriptor must be an unchanged, live READ capability with a current
namespace receipt and exactly the mounted generation. A stale generation from an
external commit is rejected; reopening the writer cannot refresh a stale reader.

After successful mutation, the same proof and selected-volume guard are checked
again. Only those foreign READ descriptors advance their filesystem generation.
Their token, cursor, access mode and lock range remain unchanged. Existing writer
methods retain their own cursor semantics: WRITE advances it; PWRITE and TRUNCATE
preserve it. Quota and permission denial leave the descriptors and disk unchanged.
An uncertain mutating I/O or filesystem-integrity error quarantines the selected
volume and revokes all descriptors, including on the already-mounted dispatch
route. An unsuccessful operation cannot refresh a foreign descriptor.

ABI query operation37 retains version1 and the 64-byte descriptor. Feature flag `0x10`
announces positioned native I/O and `0x20` announces unrelated descriptor reader
sharing; the integrated native ABI implementation must accompany those feature
bits. Existing flag `0x08` continues to announce selected-path sharing.

Regression coverage lives in `rust/kernel_core/src/fs_handles.rs`: all three
mutators with a foreign locked reader, unchanged token/cursor/lock range, durable
readback, stale and writable foreign descriptor rejection, nondestructive quota
denial, same-file pre-device denial, and mounted-route write-failure quarantine.
Run with `cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml
--features normal-desktop --lib fs_handles`. These host tests do not establish
SMP synchronization, physical storage power-loss recovery, or complete G6 signoff.
The service remains serialized on the BSP; existing actual QEMU acceptance must
verify the integrated native client and reboot behavior.

Cloud workspace validation for this subsystem change: the full `normal-desktop`
kernel host suite passed 4,214 tests with zero failures. This result includes the
four new regression functions and the updated unrelated-truncation control.
Integrated exact-head GitHub/QEMU results must be recorded by the coordinator.
The `--no-default-features --target x86_64-unknown-none --features normal-desktop`
compile check also passed; existing repository warnings remain visible.
