# M1 implementation deduplication contract

Canonical backlog: `docs/M1_PHASE_SCOPE.md` (99 tasks). Machine-readable index:
`docs/M1_TASK_REGISTRY.json` (99 stable IDs and G1–G8). Acceptance remains under
issue #616 and the per-gate QEMU acceptance documents. Registry `audit_required`
means **unreviewed for this registry**, not “missing” or “unimplemented”. Never
create a parallel backlog or mark a gate complete because a task ID exists.

## Stop work that is already implemented

Before any M1 production change, check the exact current `main` source,
existing tests, relevant merged PR diffs, and gate acceptance evidence.
Classify the work as: `missing-code`, `fix-existing`, `acceptance-only`,
`ci-infrastructure`, `optional`, or `out-of-scope`. Only the first two classes
are permitted to alter production source by this PR policy; the rest should
reuse current code, finish evidence, or be postponed. Never implement an
alternative Terminal session, storage API, scheduler, or file service merely
because a gate checkbox remains unchecked.

For each required production PR, the description must contain seven literal,
nonempty single-line fields (the default PR template provides them):

```
Task ID: G2
Change class: fix-existing
Existing source inspected: rust/kernel_core/src/m1_partition.rs
Merged PRs inspected: #639, #778, #790
Missing behavior: Exact input and missing acceptance condition or reproduced defect, not a general feature request.
Test evidence: rust/kernel_core/src/m1_partition.rs regression exercising the exact failure and safe positive control.
Reuse decision: Extend existing inspect_partition_receipt rather than create a new partition parser.
```

Use `M1-Pn-nnn` task IDs from the registry when a specific phase task is
identified, or G1–G8 for gate acceptance. If no prior merged PR actually
covers the requested area, explain the searches in `Merged PRs inspected`
and cite the closest merged PR reviewed. A reviewer, not this regex-based
check, determines semantic overlap. Optional work is not a gate blocker.

## Safe CI behavior

Workflow `.github/workflows/m1-duplicate-prevention.yml` uses
`pull_request_target` with **read-only token** and checks out ONLY the base
SHA from `main`. It reads the PR description and changed file names via the
GitHub API; it never executes PR-head code or obtains secrets from PR code.
It fails closed if it cannot fetch PR file inventory. A separate `pull_request`
`audit-pr` job executes the same base-SHA policy for an exact-head CI run
and tests run with read-only permissions. Production edits in
`rust/kernel_core/src/m1_*.rs`, or related M1 source under an M1/G1–G8 PR,
require the evidence fields; documentation and CI-only changes are exempt.

The check validates presence, the known task ID, source-path citation, merged
PR-number citations, classification, and test/reuse rationale. It **cannot**
detect semantic equivalence, verify that cited source was actually read, or
prove a cited PR covers the behavior. Do not interpret green as independent
code review. The workflow's separate unprivileged tests exercise negative
controls for blank evidence, forged IDs, and improperly classified code.

Repository administrators must add the `audit-pr` job of `M1 Duplicate
Prevention` to the `main` branch required status checks / ruleset. Without
branch protection, GitHub displays failures but does not inherently block a
manual merge. The existing agent coordinator will also require a successful
exact-head M1 Duplicate Prevention run before automatically merging. Privileged
workflow changes need extra scrutiny. This PR cannot configure administrator-
only rulesets.

## Coordinator acceptance

1. Discover existing source and merged PRs; reject duplicate task assignments.
2. Assign non-overlapping owners; same files require explicit sequencing.
3. Record missing behavior, test plan, and the decision to reuse existing code.
4. Make the smallest fix in existing modules; no task-count quotas.
5. Demand exact-head host/QEMU/S0 evidence and an independent review.
6. Recheck merged `main`; only then update gate acceptance.

G1/G2 example: PRs #618, #632, #639, #652, #654, #765, #778 and #790 are
merged; PR #792 was closed **without merge** as redundant optional MBR
Terminal-session expansion. The accepted GPT desktop contract and MBR host
safety must be verified, not rebuilt from scratch.
