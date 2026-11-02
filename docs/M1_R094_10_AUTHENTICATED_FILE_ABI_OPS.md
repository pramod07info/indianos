# IndianOS R094 — 10 authenticated FileHandles ABI operations

R094 exposes the validated R092-R093 COW operations through the existing kernel `FileHandles::dispatch_authenticated` path and its legacy generation-zero compatibility route.

## New actually implemented dispatch operations
| Code | Behavior | Rights & limits |
|---|---|---|
| 7 | append bytes to an open file | WRITE, 1–256 bytes, write budget, update cursor to EOF |
| 8 | insert bytes at handle offset | WRITE, 1–256 bytes, budget, update cursor |
| 9 | delete byte range at current offset | WRITE, 1–256 bytes, budget |
| 10 | fill byte range from one supplied value | WRITE, 1–256 bytes, budget |
| 11 | compare-exchange content (`argument=expected.len`, buffer = expected || replacement) | WRITE, buffer <=256, mismatch returns 0/no quota consumed |
| 12 | conditional unlink based on verified file content | WRITE, buffer <=256, old token retired after success |
| 13 | conditional rename (`path=destination`, buffer = expected) | WRITE, protected destination denied, old token retired |
| 14 | verified CRC32 of current file | READ, empty input buffer, no writes |
| 15 | compare complete contents to supplied bytes | READ, <=256 bytes, no writes |
| 16 | swap current file name with supplied existing destination | WRITE, protected target denied, old token retired |

Each operation checks an opaque handle token, authenticated process incarnation, read/write rights and current filesystem generation. Transport or volume-integrity failures retire the active handle. Mutations check the service-scoped write guard before I/O and charge it only after success. Shared PID2 handles have READ rights only; they cannot mutate files. R094 does NOT bypass the existing privileged storage path.

10 new integration tests cover operation semantics, permissions, PID incarnation changes, no-write mismatches, security policy, stale handles and COW persistence.

**Accounting for the 90 additional tasks requested**:
- 17 new lower-layer DurableFs APIs (R092)
- 14 bounded, path-validated HierVfs integrations (R093)
- 10 real authenticated file-handle dispatch operations (R094)
- **41 separate implemented code deliverables, 49 tasks remain unimplemented**
- 26+9+10 = **45 new Rust tests**, not counted as production tasks.
- End-to-end independently authenticated user-process QEMU acceptance still requires exact-SHA live ring3 tests; no high-level M1 gate marked closed from host tests.
