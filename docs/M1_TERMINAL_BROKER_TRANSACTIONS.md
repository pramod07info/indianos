# Normal Desktop filesystem broker transactions

PID1 authenticates Terminal messages through kernel IPC sender metadata. The
Terminal supplies no PID, descriptor or absolute path in its filesystem request;
the existing fixed request codec independently validates names, lengths,
reserved bytes and the `/shared` namespace. Authentication alone previously
allowed Terminal to send multiple filesystem requests during any single
keystroke exchange, even before submitting a command line.

Normal Desktop now admits filesystem work only in the reply transaction for a
submitted Enter. Beginning an editor, typing an ordinary character, cancelling
or reporting input loss grants no filesystem request. One Enter admits at most
one request, including a failed or malformed request. A second request fails
the service protocol before another filesystem call. PID1 never retries an
ambiguous mutation based on a peer's reply. `A` completes that transaction;
`C` is valid only for an explicit Tab cancellation. A completed transaction
cannot be reused.

The first ordinary editor activation performs a real negative exchange:
Terminal sends a `WUNAUTHORIZED` note mutation before a line has been submitted,
requires PID1's error reply, and then acknowledges normal editor activation.
No VFS call is made for this denied request. Its markers are:

- `desktop broker denied filesystem request outside submitted Enter`
- `desktop Terminal unsolicited filesystem mutation denied`

The existing physical Terminal workflow must still write and read back the
expected selected-volume bytes after this probe. Host state-machine tests
exercise all 256 key values with both begin states, single-request limits,
replayed completion, cancellation and a malformed request followed by an
attempted replacement. These use the actual production admission type.

The existing IPC envelope, canonical file codec, positive read/write commands
and legacy experimental profiles retain their interfaces. A protocol violation
that requires terminating PID1 is handled by the runtime's existing verified
session restart and recovery policy; this change adds no independent watchdog.
This is admission per cooperative key exchange, not a versioned public socket
or filesystem service ABI, and it does not provide persistent request IDs or
exactly-once semantics across service crashes. Those remain separate work.

No compilation, tests or QEMU were executed locally for this source-only batch.
The new host suite and real Terminal rejection/readback paths must pass required
GitHub CI before acceptance. This document does not mark cross-process
filesystem or crash-recovery milestone gates complete.
