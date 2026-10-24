# IndianOS 10+1 multi-agent development workflow

Baseline: main commit 255e95c (R031.9 graphical Files integration), 2026-10-24.
R031.9 has reached main, but complete main full-kernel CI may still be running.
Avoid assigning R031.9 as a new unfinished task.

## Ten queued specialized workstreams

| Agent | Priority | Issue | Branch | First bounded feature |
|---|---|---|---|---|
| 01 Filesystem | P0 | #46 | agents/01-filesystem-scale | Recoverable metadata growth, no data loss |
| 02 Packages | P0 | #47 | agents/02-signed-apps | Guest-side signature acceptance/rejection |
| 03 Security | P0 | #48 | agents/03-security-identity | Non-hardcoded identity, entropy and revocation |
| 04 Services | P0 | #49 | agents/04-service-manager | Supervised service lifecycle |
| 05 Desktop | P0 | #50 | agents/05-userspace-compositor | Userspace surface ownership and IPC |
| 06 Network | P1 | #51 | agents/06-network-stack | QEMU DHCP or bounded socket API |
| 07 Drivers | P1 | #52 | agents/07-hardware-drivers | PCI discovery and safe block interface |
| 08 Updates | P1 | #53 | agents/08-updates-recovery | Signed rollback/recovery on virtual disk |
| 09 Testing | P0 | #54 | agents/09-ci-hardening | PR kernel CI and adversarial gates |
| 10 App AI | P2 | #55 | agents/10-app-ai-planner | Trusted app catalog and typed planner |

Agent assignments designate branches and issue contracts, NOT running LLM
processes. GitHub Actions does not generate ten capable coding agents on its
own. Each development runner must be configured separately, granted only
its branch write scope, and tested on a single bounded task before scale-out.

## Coordinator rules

Coordinator lives in agents/coordinator. PRs from agents/* target main;
none of them push directly to main. Pull requests are the handoff channel.

Automated coordinator gate runs on PR changes, completion of kernel CI,
manual dispatch or every 30 minutes (GitHub schedules are best-effort).

- Work only with same-repository agents/* PRs; never checkout or execute
  untrusted PR content using coordinator write privileges.
- Require at least one changed file, size bounds, non-draft PR, successful
  exact-head full kernel QEMU CI, non-stale current main and a clean merge.
- Require the coordinator-approved label AND an approving review of the exact PR head SHA by someone
  other than the PR author. Automated policy is not a semantic security review.
- Squash merge ONE approved PR at a time, then dispatch mandatory QEMU
  kernel CI for the new main SHA; do not merge further work until main green.
- Privileged changes (.github, boot, credentials) require extra scrutiny.
  Do not treat the label as a substitute for a substantive independent review.
- Never force push, silently disable required tests, format real disks,
  or claim a device/compatibility feature without target-level evidence.
- Agents rerun CI and refresh their branches after each main merge.
- Issue closes only when reviewed PR merges and main CI evidence is recorded.

The legacy Agent X autodev runner is modified in this proposal to create an
isolated agents/legacy-x-* PR and dispatch branch CI instead of committing
directly to main. Existing Agent Y remains CI diagnostics, not a merge actor.

## Other currently uncompleted backlog groups (not allocated this first batch)

Production hardware entropy and credentials; POSIX-like filesystem behavior;
full DHCP/routing/TLS and real sockets; dynamic boot installer and signed
firmware-independent recovery; hardened secure boot and disk encryption;
SMP, memory pressure, limits and performance instrumentation; ACPI/AHCI/NVMe/
USB/Wi-Fi/audio/GPU acceleration, real machine qualification and ARM64 port;
native browser/PWA support and an app ecosystem; measured Linux/Windows/
Android compatibility routes and lawful macOS access; AI assistant runtime,
privacy and verified installers; quantum Q001 queue integration and purely
software quantum simulator tests. These require subsequent, separately
verified milestones, not universal-compatibility claims.

## Dependency order

Filesystem, security and CI gates first. Packages/updates depend on both
filesystem and security. Service manager and compositor agree on capability
IPC and lifecycle. Networking/TLS uses verified entropy and clock contracts.
AI planner never executes privilege-bearing commands; it requests a
deterministic policy decision and explicit user approval.


## Ten parallel coding runners and controlled automatic review

The GitHub Actions workflow agent-swarm.yml uses a matrix of ten distinct
runners. Each runner uses a separate agents/NN-* branch, a narrowly defined
coding-model prompt, a preapproved Rust source slot, compilation, unit tests,
kernel source audits, and a freestanding kernel build before opening a PR.
The model never receives a GitHub write token. Failed candidates never reach
the main branch.

Each first task produces a bounded helper with unit tests, not a complete
production feature. Tasks #46-#55 remain open until full integration with
independent kernel and QEMU acceptance evidence.

The Coordinator auto-reviews only a single changed preapproved agent module,
runs structural checks and requires successful full QEMU CI on the exact PR
SHA. It merges one safe PR, dispatches main CI, and waits for green status
before integrating another. Other changes require explicit reviewer approval.
Structural validation is not a substitute for human code or security review.

When a safe PR is stale, the Coordinator attempts to update its branch and
rerun exact-head CI without force pushing. Any conflicts stop that PR.

Runners can execute on workflow updates, successful kernel CI on main,
a GitHub-scheduled six-hour cadence, or manual workflow dispatch.
GitHub Actions repository settings may block bots from opening PRs;
branch protection must still be configured by a repository administrator.
