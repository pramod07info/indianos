# R026: 20 major developer workstation workstreams

Source: `f48c161d8a84d83a3631861249242832f62aeae2`. Integration: `integration/workstation-r026-twenty`.

1. Real file-tree indexing, 2. Cross-project file transfer, 3. Atomic batch copy,
4. Source restore checkpoints, 5. File filters and sort,
6. UTF-8 multi-cursor edits, 7. Multi-file atomic find/replace,
8. Source snippet insertions, 9. TODO/FIXME source issue explorer,
10. Terminal env variables, 11. Actual file output redirection,
12. Typed terminal job scheduler, 13. Actual-source fingerprint incremental build planner,
14. Compiler diagnostic aggregation, 15. Test retry/quarantine,
16. Trusted debugger stepping state, 17. Typed watch variable diffs,
18. Split-pane editor tabs, 19. Workspace recovery snapshots,
20. Combined project/editor/terminal/build/debug QEMU guest acceptance.

Each one of 20 Git branches includes a substantive Rust module and three module tests.
There are also two integration unit tests and a QEMU guest smoke.
The target is one combined CI build and one squash commit on main **only after all mandatory checks pass**.
Validate Roadmap SHA, S0 security, Orbit UI QEMU, M1 persistent disk, M2 package/network,
independent review and full kernel x86_64 QEMU at the exact head.

**Important:** RamFs is volatile in-memory storage; these real file edits do not imply reboot durability.
Job build/test/debugger registries require trusted native engines and are not complete compilers,
hardware debuggers or a fully functioning graphical IDE. Workspace recovery is a volatile checkpoint.
Twenty Git workstreams are not twenty concurrently executing autonomous AI coding agents.
