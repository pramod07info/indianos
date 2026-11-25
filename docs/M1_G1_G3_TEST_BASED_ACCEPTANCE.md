# G1–G3 test-based acceptance proposal — 24 Nov 2026

The project owner explicitly authorized replacing independent source review with test-based acceptance for G1–G3 only. This is a proposed scoped policy exception, not an independent approval. Existing branch protections and required CI remain unchanged. This proposal does not activate the exception or change the canonical gate count until reconciled with the existing acceptance policy and merged.

Production evidence baseline: `5b6ace8a6e5d91c217744e5ad0b8ca7f02ace1d7` (PR #806). See [existing evidence packet](M1_GATES_6_7_ACCEPTANCE.md).

| Workflow | Verified job result |
|---|---|
| [Combined reliability](https://github.com/pramod07info/indianos/actions/runs/45395190978) | All three jobs successful |
| [Durable VFS](https://github.com/pramod07info/indianos/actions/runs/45395191443) | Success |
| [Kernel boot](https://github.com/pramod07info/indianos/actions/runs/45395191063) | Success |
| [Security](https://github.com/pramod07info/indianos/actions/runs/45395191360) | Success |
| [SHA verification](https://github.com/pramod07info/indianos/actions/runs/45395191179) | Success |

G1 and G2: technical PASS candidates. G3: conditional pending disposition of historical recovery timeout and mandatory evidence completeness. Do not waive failures, skipped required tests, or missing evidence. Shutdown exhaustion has host evidence only; do not claim emulator coverage. Reconcile canonical registry and issue #616 only after applicable requirements are satisfied. Current formal count 5/13; target 8/13. No duplicate production implementation is proposed.
