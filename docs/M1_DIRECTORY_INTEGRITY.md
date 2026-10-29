# M1 directory integrity

HierVfs previously special-cased only literal `/` in listing and checked only
its selected directory marker. A valid `/~da~b` could consequently expose `/a/b`
without `/a`. Listing also trusted encoded metadata names, allowing orphan
entries, noncanonical names, or simultaneous file/directory aliases to appear
as children.

Directory lookup now checks every canonical prefix iteratively, including the
exact marker payload and absence of the opposite file alias. Listing normalizes
its input first, so repeated slash and dot aliases of root work consistently.
Before publishing output it snapshots the fixed durable name table and validates
all reserved hierarchical keys: decoding and re-encoding must match exactly,
parent directories must exist and verify, directory payloads must verify, and
opposite aliases must be absent. Missing parents in this metadata preflight are
reported as InvalidVolume; a directly requested missing path retains NotFound.
File reads and range reads reject an opposite directory alias before copying.

No disk layout, allocator, recursive traversal, or storage bounds change. The
snapshot uses MAX_FILES fixed keys; metadata validation and directory reads do
not write media. Legacy unreserved durable keys remain outside hierarchical
listing. Listing deliberately fails closed for any invalid reserved namespace
entry, including one outside the selected directory. Creation retains Exists
for ordinary destination collisions. This is not mount-time fsck or repair.

Tests cover canonical root/nonroot aliases across remount, orphan directory and
file keys, malformed encoded paths, invalid marker data, contradictory aliases,
missing ancestors during listing/read/mkdir, and injected directory payload I/O
failure. Failure assertions check untouched output, generation, write count, and
full media snapshots. Existing hierarchy and recovery tests remain applicable.

Validation: `cargo test --features test-r031-vfs hier_vfs --quiet` passed all
44 focused tests. `cargo check --no-default-features --features test-r031-vfs
--target x86_64-unknown-none` passed. Both used the workspace Rust toolchain;
existing unrelated compiler warnings remain. QEMU integration is coordinated
separately.

Follow-up review correction: alias corruption requires both the requested-kind
key and its opposite, rather than mere presence of the opposite. Valid directory
paths passed to file reads and valid files used as directory prefixes retain
NotFound. Existing-file overwrite, range write, unlink, rename source, and copy
source now reject genuine contradictory aliases before issuing writes. Ordinary
create and destination collisions retain Exists. Two additional tests cover
wrong-type lookup and every guarded file read/mutation with unchanged media.
Follow-up validation: all 46 focused HierVfs tests passed, the same bare-metal
no_std check passed, and git diff --check passed.
