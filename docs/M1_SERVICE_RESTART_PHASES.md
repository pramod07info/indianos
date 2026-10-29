# M1 service restart phases

`ServiceTable` is a bounded policy object for trusted supervisor/BSP code. It does
not authenticate callers, create process mappings, install grants, or implement
SMP synchronization. The trusted scheduler must validate the faulted PCB and the
supervisor must finish replacement mappings, mailbox and ownership preparation
before calling `prepared`. These checks do not establish a userspace control API.

Private per-slot phases preserve the public `ServiceSlot` fields and layout:

| Phase | Allowed transition |
| --- | --- |
| Configured | `start` increments generation and activates |
| Active | Successful `crash` reserves a pending replacement; `stop` or `stop_exact` stops |
| Pending | Only matching service, PID and generation `prepared` activates |
| Stopped | Explicit `start` increments generation and activates |
| Exhausted | No activation permitted |

Stop methods intentionally apply only to active services. They do not cancel a
pending reservation. Successful stop invalidates any previous preparation token;
repeated preparation cannot reactivate it. A later explicit start is permitted
and retains the restart budget already consumed. Retry or generation exhaustion
fails closed; a failed replacement reservation never changes the PID or counters.
Invalid crash identity/replacement arguments leave state unchanged.

`start_batch` runs against a shadow of both slots and phases and commits both only
when every requested start succeeds. Pending and exhausted services therefore
reject an entire batch without activating earlier entries. `TerminalSupervisor`
retains its independent one-shot behavior.

Focused tests exercise production transitions, pending start bypass, stale and
repeated preparation, both stop methods, exhausted retries, batch rollback, peer
isolation and checked generation overflow. Overflow tests seed the otherwise
unreachable counter boundary through module-private test access.

Validation (2026-10-29): `cargo test --offline --locked --manifest-path
rust/kernel_core/Cargo.toml --features test-r031-supervisor
service_supervisor::` passed all 15 focused tests. `cargo check --offline --locked
--manifest-path rust/kernel_core/Cargo.toml --no-default-features --target
x86_64-unknown-none --features test-r031-supervisor` passed. Both used the shared
`/workspace/.cargo` and `/workspace/.rustup` toolchain. Existing unrelated compiler
warnings remain. Runtime QEMU restart validation belongs to the integration run;
these checks establish policy transitions and no_std compilation only.
