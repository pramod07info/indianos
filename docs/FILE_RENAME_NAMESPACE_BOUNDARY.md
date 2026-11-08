# Handle rename and namespace confinement

A writable source descriptor does not authorize an arbitrary destination.
Conditional rename (handle action 13) and file-name swap (action 16) now
independently canonicalize the destination and require both the current
process namespace root and the global protected-path write policy to permit it.

Previously those two operations checked the destination against protected OS
paths but omitted the process's narrower root. A PID1 incarnation restricted to
`/shared/docs` could use a valid source descriptor to rename or exchange a file
with `/shared/other` or `/shared/docs2/out`. Both requests now fail before the
filesystem operation, cursor changes or successful-write accounting. Traversal,
relative, empty and protected destinations are rejected as well. A same-root
destination still works; successful rename/swap retires the original source
token, and conditional mismatches preserve the existing no-change behavior.

Selected-volume preflight applies this destination check to conditional rename,
swap and ordinary handle rename (action 31) before ATA discovery, capacity
queries, GPT inspection or filesystem remount. Mounted execution repeats the
check for actions 13/16, so a caller cannot bypass confinement by supplying an
already mounted filesystem. Existing process-generation, source-handle rights,
foreign-reader exclusion and COW identity checks remain in effect.

Added regression source exercises narrowed-root positive and negative cases,
canonical same-root destinations, conditional mismatches, unchanged foreign
content/generation/budget, and source-token retirement. Selected-volume tests use
a device that panics even on a capacity query and also invoke the production
runtime adapter, proving that forbidden destinations must be rejected before
native ATA probing. No local build or test execution was performed at the
user's request; GitHub verification is still required. This change does not
establish general ACL, Unix ownership or multicore filesystem locking support.
