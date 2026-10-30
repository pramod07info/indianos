# M1 R053 — six kernel reliability workstreams, single consolidated commit

This M1-only integration addresses six real production-module concerns, not six independently running coding sessions.

| Subsystem | Production change | Two regressions per module |
| --- | --- | --- |
| IPC | Bounded atomic fan-out to several registered receivers; validates every recipient and capacity before publication | Full mailbox rollback, sender/recipient identity and duplicate validation |
| GUI compositor | Revoke/transfer window grants with removal of corresponding stale frame damage | Wrong-owner atomicity, window transfer without old-owner frame replay |
| Service manager | Generation-bound transactional stop of multiple running services | Valid shutdown/restart and stale/duplicate PID rollback |
| File handles | Retire an opaque read handle after uncertain filesystem I/O; disallow stale retry until explicit re-open | Fault-injected read and benign out-of-range read |
| GPT | Recheck disk/partition GUID and health after volume inspection during strict selected-partition mount | Healthy COW remount and mid-probe media GUID replacement |
| Physical mouse | Require AUX-owned 8042 mouse ACK and separate controller-config reply ownership; protect pending primary keyboard data | ACK ownership and exhaustive status-channel disjointness |

The six existing production Rust modules each add two focused tests: **12 new Rust tests**. No synthetic agent source files, external dependencies, new disk formats or physical-disk writes.

**Scope limitations:** this improves single-BSP primitives, not general multi-core locking, independently isolated ring-3 compositor, physical-install support, comprehensive persistent ACLs, flush/fsync qualification, or full M1 milestone completion. The existing G1–G8 gates remain accepted only with exact-SHA integration evidence under issue #616.

**Actual agents:** requested 6 coding agents, but the available GitHub connector cannot launch autonomous Codex coding sessions. **0 independent coding workers launched; one coordinator-authored integration of six functional workstreams**. CI reviewer/coordinator workflows do not count as coding agents.

**Merge gates:** require exact PR-head host/no_std, M1 persistent Files/Terminal QEMU reboot, security S0, full kernel boot, independent review, all required CI and branch protection. Do not count an in-progress job as passed or merge before compliance. New base SHA is documentation-only screenshot archive `fc3faa4c9d23bdea86e3f2d2bc660557ded09e55`.
