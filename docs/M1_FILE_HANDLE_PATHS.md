# M1 canonical file handle paths

`FileHandles::open` now normalizes the caller's absolute path into its existing
fixed storage before checking process rights and protected namespaces. The same
canonical bytes are used for the validating durable VFS read and retained by the
handle. Repeated separators, dot segments and trailing separators therefore have
identical authorization and storage behavior, including raw aliases longer than
23 bytes whose canonical path fits. The handle bound remains 23 canonical bytes;
HierVfs additionally enforces its existing logical path and catalog constraints.

The existing normalizer rejects every `..` component, embedded NUL, relative or
empty path, and overflowing canonical output. Errors map to `InvalidName`.
PID2 remains read-only under the exact `/shared/` subtree; `/sharedx/` and the
`/shared` directory do not qualify. PID1 write opens still deny canonical boot,
system and recovery paths. No ACL, ABI, transfer bound or write budget changes
are introduced. Protected denials continue to increment guard statistics.

Path, policy, capacity and durable validation failures install no handle and
consume no opaque token. Exhaustion fails without wrapping or installing a slot.
Closing a slot does not allow stale-token reuse.

Tests in `fs_handles` use HierVfs over the real DurableFs COW implementation and
an in-memory block device. They exercise canonical shared aliases and persisted
writes after remount, malformed and oversized paths, shared boundary and mode
checks, failed-open slot/token atomicity, ownership and stale tokens, protected
aliases and token exhaustion. Existing protected namespace and service budget
tests remain part of the focused suite.

Validation commands (from repository root):

```sh
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml --features test-r031-vfs fs_handles::
cargo check --locked --offline --manifest-path rust/kernel_core/Cargo.toml --no-default-features --target x86_64-unknown-none --features test-r031-vfs
```
