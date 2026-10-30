# Runtime file-handle process identity

The existing experimental R031 file syscall (33) now authenticates an actual
prepared process incarnation, not just its numeric PID. The kernel's persistent
FileHandles table issues a checked, monotonic generation when a PID1/PID2 Task
has completed image, mapping and PCB preparation. A private zero-initializable
Task field carries that generation. The public Process structure, syscall
numbers, descriptors and opaque handle representation are unchanged.

A new incarnation retires all old handles for its PID. Every authenticated
open/read/write/seek/close checks the currently bound identity. A delayed or
repeated retirement cannot revoke a replacement's identity or peer handles.
Teardown retires file authority before fallible page-table/frame reclamation,
so retries cannot preserve authorization merely because memory release failed.
Generation exhaustion fails before changing an existing binding or its handles.

PID-only compatibility APIs use generation zero and cannot read, write, seek,
close or terminate handles issued through the authenticated runtime route.
The runtime never resets its table between scheduler invocations; generations
and the service-wide write budget survive process restart. This is a single-BSP,
serialized kernel service. It does not implement SMP locking, general ACLs,
new PID permissions or handles transferable across independently recreated tables
or boots. PID1's existing protected-namespace/write-budget policy and PID2's
shared-subtree read-only policy remain enforced.

Focused regression command:

```
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml --lib --features test-r031-files-gui fs_handles::tests
```

The regression group covers aborted-run rebinding, every stale operation,
output/volume generation preservation, delayed teardown, peer isolation,
legacy/authenticated separation, forged-owner denial before hardware probing,
generation exhaustion, protected paths and restart-resistant write budgets.
The coordinator must additionally run the genuine disposable-disk R031
filesystem/graphical Files and supervisor QEMU suites against the combined
kernel; host identity tests alone do not prove those live execution paths.

Task validation: all 13 focused file-handle tests passed. Freestanding
`x86_64-unknown-none` compilation passed with
`pmm,vmm,test-r031-files-gui,test-r031-supervisor`. An initial test compilation
caught an old unit fixture calling the private PID-only index helper; the fixture
now supplies its explicit legacy identity, retaining its original assertions.
