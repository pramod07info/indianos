# IndianOS R031.4 — Atomic directory-tree rename

R031.4 extends the experimental QEMU-only R031.3 hierarchical VFS with
directory rename/move (syscall 32, action 9). A move rewrites a directory's
encoded name and all nested directory/file names in one copy-on-write metadata
generation. File contents and on-disk R027 format remain unchanged.

## Guarantees

- Reject moving a directory into itself or any descendant.
- Reject missing source or destination parents, collisions with existing files
  and directories, and collisions with orphaned backing metadata.
- Check every affected name against the 24-byte backing slot *before writing*,
  including deep descendants becoming too long after relocation.
- A torn metadata publication leaves the previous complete tree mountable;
  no valid metadata generation contains a partly relocated directory.
- Maintain the restricted PID1-only ring-3 descriptor and existing user-buffer
  protection; no unsafe general-purpose kernel syscall is exposed.

## Evidence and limitations

Run the Rust tests with
\`cargo test --offline --manifest-path rust/kernel_core/Cargo.toml --features test-r031-vfs\`
and the four-boot QEMU test with \`make test-r031-vfs-qemu\`.
The final QEMU boot performs an actual ring-3 directory relocation and verifies
that the old path disappears and the destination directory lists properly.
Host tests cover nested moves, remount persistence, collisions, cycle rejection,
name-length overflow, generation atomicity and torn-metadata recovery.

This is not a production-ready VFS: eight total objects (files and directories),
no generalized allocation, journal, permission model, concurrent writers, or
physical-disk support.
