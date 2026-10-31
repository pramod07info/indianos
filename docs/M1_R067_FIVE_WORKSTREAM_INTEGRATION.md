# IndianOS R067 — Five workstation workstreams in one integration commit

Base branch: main at `8ce1e4261810a6a17caaa39ec1bae8839ffd33f8`.

## Real changes
1. Live authenticated Desktop bridge for `DiskEditor` (insert/delete/duplicate row), project line movement and indentation, and project snippet extraction. PID, session generation, capability expiry, FILE_READ/FILE_WRITE validated before routing. **3 integration tests including remount.**
2. Durable project source snippet extraction into a non-clobbering checksummed COW new file. `ProjectTree::copy_source_lines` validates bounds, UTF-8, fresh filesystem generation, caller identity, leaf names. **3 regressions including remount.**
3. Window layout bookmarks: snapshot geometry+binding epochs, restore only when all windows are still owned and unchanged since observation; atomic batch update. **2 rollback/positive regressions.**
4. Persistent Terminal `copylines /source first count /new`: reads actual mounted Unicode source bytes and creates one durable file; permission, names, bounds and short caller response checked before any disk write. **3 regressions.**
5. Interactive Terminal autosuggestion from most recent matching successful command: read-only suffix preview plus explicit accept that never dispatches command. Fixed-capacity ASCII; no heap allocations or history/clipboard leakage. **3 regressions.**

**14 new tests across five production Rust modules, plus this document.** Five functional workstreams implemented by one GitHub coordinator; **no five parallel autonomous Codex workers were launched**. No placeholder agent files.

PR must stay Draft until exact-head host tests, no_std freestanding build, M1 combined, S0 security, real QEMU normal boot + graphical Files/Terminal reboot persistence, independent review and branch protection check. Do not infer closed M1 gates solely from Rust source/host tests.
