# R051 — File-object handle safety and uncertain-write revocation

## Actual M1 G6/G7 hardening

The durable VFS holds a **monotonically increasing on-disk metadata generation**. Previous kernel file handles retained only an authorized pathname, allowing a handle to open `/shared/note`, then silently address a different file if another actor deleted/renamed that path and created a new object at the same spelling.

Now every kernel `FileHandles` slot pins the on-disk metadata generation observed at open; `read_for` and `write_for` reject a stale generation *before* any filesystem read, write, security budget charge or cursor advancement. A successful write through that exact handle refreshes its generation stamp so consecutive writes still work. A retired handle must explicitly close and re-open the current file to acquire the new object. This applies to both compatibility and runtime-authenticated handle paths.

Because the current fixed-size R027 COW format has **no stable inode/object ID**, the fail-closed check is deliberately conservative: an unrelated file write changes the global volume generation and also retires old handles. We must implement durable object identities and per-object generations before relaxing this policy. `seek` is an in-memory cursor operation and does not itself require a disk read; the next read/write still performs the epoch check. This is not POSIX inode semantics.

The GPT Terminal session independently retires after a failed/ambiguous `FsError::Io` or `InvalidVolume` command, not only when the initial mount fails. A fresh session revalidates GPT identifiers and COW metadata; ordinary missing-file errors remain retryable.

## Added nontrivial regression evidence

Six file-handle tests cover unlink/recreation after open, rename/replacement, repeated COW writes through the same handle, independently authenticated PID1 writer and PID2 Files reader, conservative unrelated-file invalidation, and cursor/epoch atomicity after rejected reads. Two Terminal tests cover a failed disk write followed by permanent session retirement and a retryable missing-file error. These use the actual durable filesystem modules rather than placeholder agent modules.

## Mandatory qualification

- Exact PR SHA M1 Rust host/no_std checks, authenticated runtime file-handle tests, security S0, real disposable-disk QEMU two-boot Files and physical Terminal persistence, kernel boot, independent review and main CI.
- Never autoformat media, disable tests, fake worker count or merge while mandatory checks remain pending.
- Main M1 gates G6/G7 remain incomplete. This batch does not implement persistent inodes, production PID1, SMP device locking or native userland compositor.

## Agent execution accounting

20 independently executing coding workers were requested, but the connected GitHub action interface does not provide their launching capability. This branch contains a real coordinator implementation, **zero separately launched coding agents**, one consolidated commit and one draft PR. The standing 20-task roadmap is `docs/M1_RUNTIME_RECOVERY_BATCH.md`; no dummy `agent_XX` files were created.
