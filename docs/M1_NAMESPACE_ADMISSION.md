# Strict namespace admission on selected storage

`HierVfs::mount_healthy` now verifies the complete reserved hierarchical
namespace after COW health verification and before returning a usable mount.
Both checksummed metadata copies and valid payload CRCs are insufficient when
the stored names describe orphan objects, contradictory file/directory/link
aliases, malformed directory markers or noncanonical symlink targets.

Admission reuses the bounded `listing_integrity` verifier already required by
directory enumeration and explicit recovery mounting. It checks every reserved
entry and its complete parent chain without heap allocation. Unknown legacy
flat keys outside the VFS reserved namespaces remain opaque. Valid canonical
symlinks remain stored links; admission never follows them. Error propagation
preserves media and does not format, repair, recover or publish a degraded mount.

Selected GPT/MBR boot admission, healthy remount, restart revalidation and the
host volume-selection tool all use this same production mount path. Ordinary
`mount` remains available to existing explicit inspection/recovery callers;
normal Desktop admission does not silently fall back to it.

Tests construct actual COW transactions with valid redundancy and checksums,
then verify rejected healthy mounts without any write callback. Positive tests
preserve nested files, canonical links, opaque legacy data and generation.
A read error after COW health verification must still abort namespace admission.

The host-only disposable fixture tool supports:

```
indianos-desktop-fixture --namespace-fault orphan --output NEW.raw
```

Supported faults are `orphan`, `alias`, `marker` and `link`. The tool uses the
actual DurableFs writer, retains healthy COW redundancy, and changes no bytes
outside the explicitly bounded test partition. Output must be a new `.raw` file;
existing files, symlinks and device destinations cannot be overwritten. This
fixture mode is neither guest provisioning nor a recovery implementation.

The normal Desktop QEMU suite supplies each generated image with the unchanged
trusted boot selection. Every image must fail storage admission, start no user
service, enter read-only recovery and remain byte-for-byte unchanged. CRC-flipped
tests remain separate so a missing namespace check cannot hide behind a lower
checksum barrier. New QEMU invocations can override `--fixture-tool`; its default
is the release fixture binary already built by the combined M1 workflow.

This closes a strict-admission implementation gap. It does not implement
persistent ownership/ACLs, general locking, filesystem repair or physical-media
qualification, and does not alone complete an M1 acceptance gate.
