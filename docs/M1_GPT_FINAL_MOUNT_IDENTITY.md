# GPT identity validation after the actual filesystem mount

`mount_gpt_selected_healthy` now revalidates the full GPT selection after its
last filesystem mount read, before returning a writable bounded volume. The
previous preflight validated disk/partition GUIDs around an initial COW health
probe, but then invoked geometry-only health checks and another actual mount.
A fully checksummed GUID rotation during those later filesystem reads could
therefore return a mount despite the caller's original GUID pin.

The final check retains exclusive ownership of the already-mounted block
window. Small crate-private forwarding hooks let it inspect the parent disk
through a `ReadOnlyDevice`; this does not consume the mounted filesystem or
start another filesystem mount/read cycle. Any identity mismatch or final
identity-read error drops the candidate mount and returns no usable volume.
Public mount signatures and the on-disk filesystem format remain unchanged.
No discovery, final validation or ordinary mount formats or writes a sector.

A regression first measures all filesystem transfers on a stable successful
mount, then rotates the disk GUID or partition GUID immediately after the final
filesystem transfer. The disk-GUID case was reproduced as accepted before the fix; both must now fail closed.
It also injects a final GPT read failure and checks that no mount is returned
and no writes occur. Existing selected remount and recovery tests still apply.

This is an observed TOCTOU-gap fix, not a hardware media lock. The caller must
still serialize/quiesce the block device for the complete operation and its
subsequent use. GUID and capacity checks cannot detect arbitrary hostile media
that changes and restores state entirely between observations, or replacement
media that deliberately clones every identifier. This does not establish live
ring-3 GPT integration or physical-hardware acceptance, and closes no M1 gate.
