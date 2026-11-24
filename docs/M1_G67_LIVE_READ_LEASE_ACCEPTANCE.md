# Native shared-file lease acceptance

The baseline at `bc2d296742970d287d0e8bfb1cca750d5b56433a` already tests
normal IRQ input, authenticated file operations and interrupted publication.
This extends that same desktop matrix with two boots; it does not introduce
a second filesystem, test kernel service or simulated process.

The first boot uses physical Files focus and the `l` action to acquire an
actual PID2 read-only descriptor and shared read lock on `/shared/readme`.
The visible Files state must become `READ LOCKED`. Terminal then requests
three PID1 path mutations against another file: create, write and rename.
For each mutation the original PID2 descriptor must be read and verified
before the broker acknowledges publication. A new acquisition or release
record during that sequence is a failure, not proof of token continuity.
The client's native code and kernel ownership regressions provide the
separate check that verification uses the cached opaque token rather than
reopening the pathname.

Writing the leased file must fail with the native error record and actual
`IO ERROR` pixels. The disk must remain byte-for-byte unchanged, without
quarantine, recovery or a successful-commit record. A subsequent read must
still return `HELLO INDIANOS`. Physical Files focus and `u` release the same
descriptor; the client must show `UNLOCKED`. The previously protected write
must then succeed and paint the new `permitted` contents in Files.

The second cold boot reads both `permitted` and the unrelated file's `violet`
contents through native Terminal requests, with exact serial and pixel
oracles. Shutdown must synchronize and reap all four services. Cold readback
and synchronized shutdown must not change any disk bytes, and every phase
checks the selected partition's surrounding guards.

The existing 49 boots remain intact. Two sharing boots plus an independent
healthy-preview replay bring the configured matrix to 52. The replay uses a
fresh copy and separate artifacts, and any failure remains fatal; it is not
a retry-until-success policy. An initial guest run retired a transient Files
read token during ordinary refresh despite passing host tests. Native read,
unlock and ATA stage diagnostics preserve that failure's cause if it recurs.
Four new host negative-control tests reject missing/stale/duplicate or
out-of-order lease evidence, token reacquisition, destructive contention,
session retirement and a write preceding lease release. The focused cloud
Python suite passed **53 tests**, and the full cloud Python suite passed
**196 tests**, with zero failures. The complete matrix subsequently passed on merged main
`06bed767889a8fe05aa31c9f7ee468710d027049`: [combined reliability run
45335772189](https://github.com/pramod07info/indianos/actions/runs/45335772189),
normal-desktop job 138247362816. Its actual log records all three data cuts,
all six namespace cuts with two cold reboots, native PID2 lease continuity
and the final desktop acceptance PASS. The same job passed 4,210 kernel
tests, 53 harness controls and the freestanding check. Security S0 run
45335772074 also passed on that merged SHA. These receipts qualify that
source only; subsequent positioned-I/O changes require a fresh exact-head run.

This qualifies the bounded, serialized two-process path-sharing contract.
It does not establish SMP locking, arbitrary concurrent descriptor writers,
physical power-loss guarantees or full G6/G7 milestone acceptance.
