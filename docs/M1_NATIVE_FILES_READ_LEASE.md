# Native Files read leases

The normal Files window now offers a user-controlled read lease on its
`/shared/readme` preview. With Files focused, press **L** to acquire the shared
lease and **U** to release it. A sixth retained app text field shows `READ LOCKED`
or `UNLOCKED`; changing this state does not allocate more drawing commands.

PID1 routes those physical keys through the existing authenticated Files IPC
channel. Files opens a read-only native descriptor, acquires its shared lock,
reads the bounded preview and retains that exact opaque token. PID1 presents
the resulting state through PID4 after the Files response. Terminal focus and
pending editor input retain their existing routing rules. Legacy experimental
profiles do not gain the commands or change behavior.

Every Files refresh while the lease is active uses positioned read on the
original descriptor, compares its contents with the captured preview, and checks
that a sequential first-byte read still begins at offset zero. It then restores
the cursor and acknowledges refresh. It does not reopen a replacement descriptor to
claim continuity. The existing ordinary preview rendering may independently
use its normal short-lived reader. This lets the selected path API's qualified
unrelated-file sharing preserve a real PID2 read capability across PID1 commits.
Mutating the protected file remains a conflict until its lease is released.

Repeated acquisition, missing or denied files, and release after lost authority
return a bounded error rather than create a second handle or report success.
A refresh encountering revoked authority or changed protected content clears
the private record and returns `LEASELOST`, which fails the broker's normal
`UPDATED` contract and enters existing session recovery. EXIT attempts release;
the kernel's process retirement still owns final capability cleanup.

The existing Files update shortcut reports ordinary service refusal while a
read lease protects its target; it does not terminate PID1 on contention.
The private lease record validates the captured preview and token lifecycle;
it grants no authority. All reads, locks, unlocks and closes still use native
syscall33 and the kernel-authenticated process incarnation. The new Terminal
failure diagnostic is emitted only after an authenticated broker error reply.

The client negotiates ABI version 1, the 64-byte descriptor layout and the
unrelated-reader flag `0x08` and positioned-I/O flag `0x10` before acquiring the long-lived lease. Unsupported
kernels return a normal control failure rather than enabling a volume-wide lock.

Cloud verification for this client change: ten standalone renderer/protocol/
lease tests passed, including replayed capture, changed contents, token mismatch,
empty files, stale records after clear and ABI capability negotiation. Freestanding release checks passed
for `desktop-files`, `desktop-init` and `desktop-terminal`. Actual cross-process
lease continuity, conflict denial, graphical state and reboot acceptance require
the integrated QEMU test. This is a single-BSP file-service workflow, not SMP,
blocking POSIX locks or a physical-hardware qualification claim.
