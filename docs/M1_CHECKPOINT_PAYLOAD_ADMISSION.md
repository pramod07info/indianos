# Checkpoint payload admission and final durability verification

Every DurableFs metadata publication now verifies all files referenced by the
proposed checkpoint before writing the inactive metadata head and again after
its readback. This includes metadata-only rename/delete/tree operations and
ordinary HierVfs mutations, not only explicitly flush-verified writes. A mounted
metadata checksum is insufficient evidence that unchanged file data remains
intact. Publishing a renamed namespace over an already corrupt payload could
otherwise overwrite the remaining rollback head while reporting success.

Strict durable create/overwrite retains the data-before-metadata barrier order.
After the final metadata flush it also verifies the complete published payload
set and rechecks both recorded heads. A controller that corrupts a file while
flushing metadata must not receive a successful durable response simply because
its metadata sectors remain readable. Any payload verification error during
publication retires the mount. Caller output remains untouched on later reads;
explicit remount is required even if a transient transport error disappears.

An error after publishing metadata is an uncertain outcome, not a claim that
the operation rolled back. An explicit remount may select the new complete
checkpoint or the older complete checkpoint. The filesystem never repairs or
formats media implicitly. Three-bank ownership continues to protect payloads
referenced by both observed heads, including rejected recovery candidates.

Regression coverage includes unchanged-payload corruption before a rename,
payload read failure without metadata mutation, corruption caused by a metadata
write, payload corruption/read failure at the final flush, explicit final-flush
failure, permanent session retirement and remount outcomes. Existing ordered
barrier success tests continue to cover healthy creation and overwrite.
Tests were added but not executed locally in this source-only batch.

Admission also rereads both exact metadata sectors after payload verification.
If either differs from the scanned header, or the reread fails, mount and health
inspection reject the mixed snapshot without writing. Strict healthy mounting
checks the filesystem instance it actually returns, rather than trusting a
healthy preflight followed by a separate potentially degraded mount. Live health
checks compare both recorded heads before and after inspection; a changed
inactive rollback head retires the session even if its active head is intact.
Regression coverage injects header corruption and transport failure during
admission, checks strict-mount scan changes and verifies permanent retirement
after an inactive-head replacement. Read-only recovery still permits an
explicit fresh mount of a stable volume with one valid checkpoint.

This bounded filesystem still has eight objects, a 2 KiB per-file limit,
CRC-based accidental-corruption detection and a single serialized volume owner.
Verification cannot prove that a malicious controller has actually persisted
bytes it consistently fabricates, nor does it replace power-loss hardware
qualification. M1 persistence acceptance still requires exact-head CI, real
emulator reboot/torn-write scenarios and the repository's remaining exit gates.
