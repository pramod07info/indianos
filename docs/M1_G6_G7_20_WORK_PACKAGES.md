# M1 G6 + G7: 20 engineering work packages (not 20 executing agents)

Refreshed after PR #770 and #771 merged on 2026-11-12; next development base: main `b6c719e3751a3ff26af8687c1d0ee14216084aa4`.
Scope: G6 = persistent VFS / power-loss safety; G7 = live authenticated ring-3 file ABI.
The 20 entries below are independently reviewable assignments. **This
integration used one coordinator, not 20 independently launched coding agents.**
A code commit, an authored test, or a merged PR does **not** complete a gate.
Physical power-loss and multicore/hardware certification remain open.

## G6 — persistent volume, transaction durability, isolation

| Package | Proposed implementation and acceptance evidence | Status |
| --- | --- | --- |
| G6-01 | Pin immutable selected-media identity across remount; reject changed GPT token before filesystem reads | Open |
| G6-02 | Audit flush ordering for new COW data banks and both metadata copies; test dropped acknowledgements | Open |
| G6-03 | Quarantine the mounted session on any post-publication verification failure; remount before retry | Source merged via PR #770; full G6 acceptance still open |
| G6-04 | Inject torn metadata-head writes during a real QEMU guest power interruption; recover old or new valid state | Open |
| G6-05 | Inject data-write loss and stale-sector readback; require checksum rejection and no user-visible success | Open |
| G6-06 | Validate interrupted rename, unlink and directory COW transactions on two consecutive cold boots | Open |
| G6-07 | Implement actual blocking or conflict-safe concurrent file locks beyond current cooperative selected leases | Open |
| G6-08 | Finish persistent owner/group/mode permissions across create, edit, rename, remount and PID recycle | Open |
| G6-09 | Prove bounded recovery without automatic format or repair after corruption and media replacement | Open |
| G6-10 | Qualify flush/ordering and unsafe power-loss recovery on deliberately selected physical test hardware | Open — requires hardware |

## G7 — authenticated per-process syscall file service

| Package | Proposed implementation and acceptance evidence | Status |
| --- | --- | --- |
| G7-11 | Require exact kernel-authenticated process incarnation on every file ABI operation | Open |
| G7-12 | Bind path operations and descriptors to namespace, mount receipt and current revision | Open |
| G7-13 | Quarantine selected volume on storage faults during VFS authorization, not just payload I/O | Source merged via PR #771; full G7 acceptance still open |
| G7-14 | Revoke all stale descriptors on mount rollback, PID recycle and failed publication | Open |
| G7-15 | Verify foreign-reader lease prevents cross-process mutations before device probe, then releases on close | Open |
| G7-16 | Enforce write grant and quota across every descriptor edit/rename/truncate, including failures | Open |
| G7-17 | Enforce ancestor-search, target owner and destination-directory authorization without TOCTOU bypass | Private-file and directory ACL enforcement candidate in PR #773; broader acceptance open |
| G7-18 | Run independent actual ring-3 clients through syscall create/read/write/rename in QEMU (no host adapter substitute) | Open |
| G7-19 | Stress concurrent process exit, reuse, open handle close, permission revocation and namespace changes | Open |
| G7-20 | Verify two-process persistent disk round-trip across two real QEMU cold boots at the exact merged SHA | Open |

## November 12 next batch — bounded COW metadata-read uncertainty

The new G6 candidate hardens the legacy COW path against a specific
fault boundary: all four new data-bank sectors can be written successfully,
yet the following header read can fail before metadata publication.
The live mount must retire even though the old checkpoint remains
readable after a fresh verification/remount. This is a bounded
synthetic-fault regression, **not** actual QEMU power interruption,
all-stage power-loss coverage or physical storage qualification.

## Dependency / integration strategy

1. Implement error-path containment first: G6-03 in PR #770 and G7-13 in its own draft PR.
2. Next, G6-01/02/05/09 and G7-11/12/14/16/17 should be independently reviewed against the actual mounted device and authentication contracts.
3. General locks (G6-07) and cross-process stress (G7-15/19) require an explicit serialization and scheduling design. Host-only simulated gates are not real SMP qualification.
4. QEMU destructive-on-disposable-media suites (G6-04/06, G7-18/20) must run on the exact PR head; real hardware G6-10 is separately authorized and evidence-gated.
5. Keep independent PRs small enough to isolate broken behavior. Multiple commits within a PR may distinguish implementation from fault-injection regression; merge only after all required exact-head checks and independent review.

## Closure rules

For each item retain: implementation paths, regression test names, host/no_std build,
serial and disk-image QEMU artifacts where applicable, exact PR head SHA,
all required checks not skipped, independent review, protected main merge,
and post-merge boot verification. A check that was green on an older SHA
does not qualify newer code. Do not mark G6/G7 complete until the complete
acceptance contract in issue #616 and docs/DELIVERY_MILESTONES.md passes.
