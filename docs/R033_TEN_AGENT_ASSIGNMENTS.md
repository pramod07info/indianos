# R033 — ten-agent runtime integration assignment

**Baseline:** `main` commit `79f42facc7637c03613ca0ac8427c025b001a058` (QEMU and unified verification green on 2026-10-24).

**Integration branch:** `integration/r033-ten-agents`

**Status: ASSIGNED ONLY.** These branches were created from main to provide isolated workspaces; there are no new agent-written implementations or successful R033 CI results at this checkpoint. Do not merge this assignment manifest as proof of completed functionality.

| Agent | Priority | Issue | One assigned task | Isolated branch |
|---|---|---|---|---|
| 01 | P0 | #71 | Durable VFS extent-to-disk read path | `agents/r033-01-durable-extents` |
| 02 | P0 | #72 | Guest package signature/trust boundary | `agents/r033-02-guest-package-trust` |
| 03 | P0 | #73 | Process generation-bound IPC capability revocation | `agents/r033-03-capability-generation` |
| 04 | P0 | #74 | Bounded two-service supervisor lifecycle | `agents/r033-04-service-lifecycle` |
| 05 | P0 | #75 | Ring-3 desktop surface ownership/event broker | `agents/r033-05-user-compositor` |
| 06 | P1 | #76 | Bounded UDP endpoint queues and ownership | `agents/r033-06-udp-endpoints` |
| 07 | P1 | #77 | Read-only PCI device discovery and storage boundaries | `agents/r033-07-pci-discovery` |
| 08 | P1 | #78 | Transactional A/B boot recovery metadata on QEMU disk | `agents/r033-08-ab-disk-recovery` |
| 09 | P0 | #79 | Exact-commit required CI checks and coordinator gating | `agents/r033-09-ci-sha-gates` |
| 10 | P1 | #80 | Offline app dependency graph and permissions planner | `agents/r033-10-dependency-resolver` |

## Coordinator acceptance and merge sequence

1. **Each agent owns one issue and one branch**, and must produce a new source change with focused tests. Existing paths are not placeholders for invented capabilities.
2. Agent contributions may run cheap local static checks. No agent can merge directly into `main`, bypass review or modify real disks/firmware.
3. Collect **ten real branch contributions**. Reject a branch with no new code, an unexpected path, a non-fast-forward or unsafe change, or a failed target test.
4. Merge the ten validated branch heads into `integration/r033-ten-agents` (one integrated candidate). Review conflicts and dependencies; do not rewrite unrelated kernel code.
5. Open **one PR to main** from that exact integration SHA, then run the existing full `IndianOS kernel boot (x86-64)` workflow, including all QEMU, Rust, storage, networking, GUI and fault isolation regressions.
6. For failures, fix the integration branch and restart the **entire** required PR CI. Do not count a build from a prior SHA.
7. Merge the integration PR into `main` **only if all required checks are successful on the exact candidate**, the main base has not changed, and coordinator review and repo authorization rules are met.
8. Verify post-merge `main` kernel CI. If red, stop subsequent integration and create an explicit remediation issue.

## Release and security caveats

The R033 tickets are implementation goals, not claims of shipped features. Passing host unit tests does not demonstrate ring-3 ownership, guest crypto, real QEMU disk recovery, general networking, or compatibility with third-party OS applications. The previously unreliable ten-model generation loop is replaced with manual/scoped contribution plus gated integration; this manifest by itself does not start ten autonomous coding processes.
