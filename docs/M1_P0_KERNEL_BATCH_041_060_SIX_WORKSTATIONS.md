# IndianOS P0 kernel batch 3 — additional tasks P0-41–P0-60

Parent integration SHA: `7735cd34228a85fda39aa9158a2941d30359886b`; existing [draft PR #786](https://github.com/pramod07info/indianos/pull/786).
These are 20 distinct development obligations after previous P0-01–P0-40;
they are not completion of the original 90 M1 acceptance criteria.

| ID | Workstation | Change and test obligation |
| --- | --- | --- |
| P0-41 | W1 | Bind a pending AP lifecycle receipt to trusted IPI sender and exact CPU identifiers |
| P0-42 | W1 | Publish release-ordered receipt before IPI transport dispatch |
| P0-43 | W1 | On IPI send failure, keep the pending receipt and refuse memory reclaim |
| P0-44 | W1 | Acknowledge AP work only after exact local invalidation callback completes |
| P0-45 | W2 | Stage runqueue eviction and generation-exact exit notices together |
| P0-46 | W2 | Reject process teardown if any AP acknowledgement remains incomplete |
| P0-47 | W2 | Do not retire runnable process if the exit notification queue is full |
| P0-48 | W2 | Reject duplicate exit publication and preserve the live process |
| P0-49 | W3 | Preflight source and distinct recipient exact IPC identities for fanout |
| P0-50 | W3 | Stage different payloads to different receivers without partial publication |
| P0-51 | W3 | Fail atomically on later mailbox capacity or payload violations |
| P0-52 | W4 | Open a geometry-bound, read-only disk metadata inspection view |
| P0-53 | W4 | Require identical two-pass sector bytes before output visibility |
| P0-54 | W4 | Permanently retire metadata view after late I/O fault or media geometry change |
| P0-55 | W5 | Revalidate exact NIC ring quiescence receipt before hardware retry |
| P0-56 | W5 | Require real driver IRQ/DMA stop callback and zero software in-flight receives |
| P0-57 | W5 | Reject stale or foreign ring receipt when re-enabling admission |
| P0-58 | W6 | Provide trusted PID1 launch callback consuming authenticated boot admission |
| P0-59 | W6 | Prevent repeated launch of same boot epoch after successful callback |
| P0-60 | W6 | On failed or partially effective launch keep one-shot receipt consumed |

## Actual implementation scope

- **W1, SMP/CPU** — `smp_lifecycle_fence.rs`: order AP lifecycle
  publication before a trusted IPI delivery callback; failed delivery
  leaves a pending generation; AP-side callback acknowledgement occurs only
  after local invalidation success. No physical local-APIC IPI or TLB
  invalidation driver implemented by this trait.
- **W2, scheduler/notifications** — `process_events.rs`: atomically
  stage exact-generation runqueue eviction and exit queue publication behind
  checked AP receipts; fail closed on full notice queue, duplicate notice
  or stale PID. Actual scheduler SMP locking and downstream PCB/file descriptor
  retirement are still separate integration requirements.
- **W3, authenticated IPC** — `ipc.rs`: transactional fanout of distinct
  messages to distinct exact-owner mailboxes, with late-capacity and
  invalid-payload rollback. Live cross-BSP IPC serialization pending.
- **W4, storage discovery** — `block.rs`: exclusive, read-only,
  geometry-pinned two-pass metadata view; permanent retirement on fault
  or byte drift, with no output modification. Hardware media identity,
  journaling and physical power loss must be separately tested.
- **W5, NIC teardown** — `nic_rx_fence.rs`: permit a trusted control-plane
  recovery from quiescing only with its exact receipt, a fresh actual NIC
  hardware stop callback and zero in-flight receive permits. Real DMA/IOMMU
  retirement remains unqualified.
- **W6, verified PID1 launch** — `r114_boot_epoch.rs`: one-use boot
  admission consumed before the trusted launch callback, including failed
  launch prevention against duplicate PID1. This has not been wired into
  a production TPM/UEFI provider or actual boot launcher.

## Acceptance and integration policy

- One new integration commit added to **existing draft PR #786**.
- Six **GitHub Actions verification workstations** execute previous P0
  and new `kernel_p0_batch3_w1..w6` host regression families, plus
  freestanding x86_64 no_std compile at exact new SHA.
- Real kernel boot, QEMU guest reboot/storage, two-vCPU, S0 security and
  combined reliability workflows are separate required gates; do not
  infer pass from local code or a prior commit's CI.
- Six verification jobs are not six separately launched coding agents.
- Keep original M1/R123 acceptance and hardware obligations OPEN until
  tested/reviewed; no premature merge.
