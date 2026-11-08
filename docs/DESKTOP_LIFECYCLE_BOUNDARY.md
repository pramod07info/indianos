# Normal desktop lifecycle boundary

The normal desktop runtime treats PID1, Files, Terminal and compositor as
critical services for the whole session. Syscall 38 is available only in the
normal-desktop profile. Both operations require argument 1 to be zero and
authenticate the active ring-3 PID1 process and its current filesystem identity.
Success returns zero; refusal returns `u64::MAX`.

* `38(0, 0)` publishes kernel-verified readiness once. All four current tasks
  must retain their live PCB, loaded image, attached address space and authority.
  Each peer must have actually run and saved a ring-3 context. PID1 calls this
  after the application/compositor handshakes, before its user-visible READY.
* `38(1, 0)` arms clean shutdown once, after readiness. While holding the BSP
  filesystem permit it synchronizes the authenticated selected volume. A failed
  flush leaves shutdown unarmed. PID1 sends service exit messages only after
  success.

Any critical service exit before shutdown is armed, including status zero,
aborts the session. Faults and nonzero exits always abort. Restart permission is
issued only by the existing verified authority retirement and frame rollback;
this contract does not grant retries on uncertain teardown. Startup has a budget
of 256 cooperative dispatches until verified readiness. It does not bound wall
clock time or contain a task which never enters the kernel.

The two GUI command stores expose their actual guard/payload geometry through
`gui::surface_guard_layout`. The runtime freezes those addresses in
`DESKTOP_SURFACE_LAYOUT`, removes all four guard mappings in the source CR3
before PMM accounting and task clones, and checks guard absence plus effective
supervisor RW/NX payload permissions in both source and cloned CR3s. Restarts
must retain the same geometry. Source page-table split ownership lasts until
reboot; a refused table release quarantines frames and denies further retries.

Regression source covers startup dispatch boundaries, one-shot transitions,
failed flush state, premature zero exits, fault/nonzero exits after arming,
malformed syscall arguments and overlapping/overflowed surface layouts.
These tests were added without local execution at the user's request. Passing
host tests alone would not establish working boot, flush durability, page-fault
containment or hardware support; GitHub/QEMU acceptance remains required.
