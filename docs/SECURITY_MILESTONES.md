# IndianOS Security Milestone — practical malware/ransomware defense

**Task register:** [Security Issue #136](https://github.com/pramod07info/indianos/issues/136).

## Honest security guarantee

There is no credible way to certify any general-purpose OS as **100% virus-proof
or hacker-proof**. Signing an app does not prove the app is benevolent. Hashes
like CRC32 or SHA256 detect unexpected modifications but cannot stop a process
with legitimate write privileges from encrypting its own files, and writable
snapshots on the same disk can be destroyed by a privileged attacker.

IndianOS must therefore be secure by default, minimize blast radius,
provide authenticated and independently protected recovery, test hostile
inputs, ship timely fixes and document supported threat classes without
claiming an impossible universal guarantee.

## Attack-surface inventory (current experimental OS)

| Entry point | Existing foundation | Important gap |
| --- | --- | --- |
| App packages / ELF | M2 Ed25519 and guarded runtime test flows | Production publisher root, universal sandbox and signatures for all apps |
| Per-process file handles | R031 PID-scoped read/write tokens; **S0 restricts sensitive path writes** | Proper PID-generation identity and mediation of every VFS syscall |
| Filesystem disk | R027 CRC/checksummed COW metadata and bounded partitions | Authenticated snapshots, backups and per-user ACL |
| Drivers / PCI | HD0–HD3 detection/signed policy test flows | Actual IOMMU/driver confinement, DMA attestation, trusted firmware |
| External files / USB / network | Partial bounded parsers, experimental network | Fuzzing, full protocol isolation, antivirus engine, removable-media scanning |
| Updates and boot | QEMU-simulated trust policy, signed examples | Physical hardware-rooted secure boot and anti-rollback |
| Ransomware | S0 bounds writes through one syscall entry | Broader detection, snapshot anti-delete policy, offline restore |

## Implemented in S0 (bounded security improvement)

The existing `rust/kernel_core/src/fs_handles.rs` file-handle operations are
the **only mutation surface hardened by this specific patch**. Kernel-owned
`fs_write_guard` is allocation-free, compatible with `no_std` and:

1. Denies ordinary writable handles to exact protected prefixes `/boot`,
   `/system`, `/security`, `/recovery`, `/snapshots`, `/keys` and
   `/firmware`, including descendants and normalized double-slash/dot aliases.
   Read-only handles are still available for troubleshooting and recovery.
2. Checks immediately before every file-handle write, not only during open;
   denied writes cannot enter the underlying durable COW filesystem.
3. Tracks a **kernel service-wide** cap of 32 successful write operations or
   4,096 bytes through the file handle interface. An attacker cannot reset the
   quota by closing/reopening a file handle. Successful bytes are counted
   only after a durable write; failure does not consume quota.
4. Exposes counts of successful writes, bytes and denials for kernel-side
   audit integration. The bounds are initial experimental thresholds,
   **not** production-calibrated ransomware classification.
5. Exercises hostile names, PID handle theft, protected original content,
   repeated write attempts, failed-write accounting and path aliases through
   actual host filesystem operations. Tests avoid physical/host disk writes.
6. Adds a separate PR/main security workflow that runs these host regression
   tests, existing signed package/trust tests and bare-metal `no_std` compile,
   plus a limited source-wiring consistency audit.

### Important limitations (security work is not finished)

- Existing `FileHandles` still gives PID1 broad rights and identifies callers
  by integer PID, not an independently authenticated PID+generation.
  The new guard is shared per file-handle service instance and can reset when
  that service itself is recreated. It does not yet protect against an
  attacker controlling the kernel or privileged direct VFS methods.
- Other direct filesystem mutation routes (create, delete, rename, overwrite,
  ATA raw disk, memory mapped I/O, installers) are **not** mediated here.
  A future S1 phase must replace hardcoded PIDs and cover every path.
- No immutable/durable ransomware snapshots, user encryption, offline
  backup recovery, antivirus signature database, full behavioral detector,
  TPM-backed boot keys, independent penetration testing or real hardware
  qualification is claimed.
- A new GitHub Actions workflow is not automatically a protected branch
  rule. A repository administrator must enable required status checks and
  required reviewer approval in GitHub's branch protection/rulesets.
- False positives are possible with fixed write budgets. Calibrate workloads
  after S1 per-app tokens and provide explicit trusted updater exceptions
  that cannot be controlled by arbitrary applications.

## S1.1 — M1 graphical Terminal filesystem ingress (current slice)

The feature-gated authenticated M1 Terminal/desktop command handler accesses
`HierVfs` directly rather than using S0 `FileHandles`. S1.1 applies the
shared normalized protected-namespace policy **before** all Terminal
`write` (overwrite/create), `mkdir`, and `mv` (both source and destination)
operations, returning denial without calling the persistent mutator.
It does not impose the experimental S0 service-wide write count on Terminal.
Read-only `cat` inspection is still possible where process rights allow it.

The security test suite validates forbidden system/recovery paths, duplicate
slash/dot aliases, parent traversal denial, rename source/destination checks,
unchanged filesystem generations, persistence after remount, and false-positive
safety for similarly named non-system directories. The S0 contract checker
now asserts the M1 command handler retains these mandatory checks.

This is **only one S1 ingress migration**, not universal path enforcement,
process-generation-bound ACLs, trusted update exceptions, protected raw ATA
writes, immutable snapshots, or malware immunity. More filesystem APIs and
privileged paths require dedicated mediation and independent testing.

## Security acceptance roadmap

**S1:** identity-bound capability checks for all VFS and block routes,
least-privilege directories, strong revocation, cross-process isolation and
denial telemetry.

**S2:** protected COW snapshots plus authenticated encrypted offline/remote
backups; simulated ransomware, disk failure and actual restore drills.

**S3:** independently authenticated secure boot, TPM anti-rollback,
signed-release provenance, exploit hardening and broad parser fuzz testing.

**S4:** adversarial external review, penetration testing, vulnerability
response and supported-hardware release qualification.

**Stop-ship rule:** Never declare IndianOS production-safe for irreplaceable
data while S1–S4 remain unfinished. Every release must disclose validated
defenses, known gaps and security contact/recovery policy.
