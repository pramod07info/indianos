# Native desktop product boot profile

The Desktop edition now has a dedicated product entry:

```
make desktop-image DESKTOP_VOLUME_CONFIG=/absolute/path/selected-volume.bin
```

It validates the complete canonical configuration before starting compilation,
builds in `build/desktop-edition` (override `DESKTOP_BUILD`), and installs an
`IndianOS Desktop (experimental)` Limine entry that starts the four-service
desktop automatically. This target pins the native desktop personality and its
boot configuration; it cannot silently become the console edition. The generic
`make image DESKTOP_VOLUME_CONFIG=/absolute/path/selected-volume.bin` route now
also selects this normal desktop profile and its automatic boot entry. This creates bootable emulator media, not
a physical-disk installer or a signed release.

To select an **existing regular raw image** with a healthy IndianOS filesystem:

```
cargo run --locked --offline --release --manifest-path tools/rust/Cargo.toml \
  --features desktop-volume --bin indianos-desktop-volume -- \
  --image /absolute/path/existing.raw --partition 0 --output /absolute/path/NEW-selection.bin
```

The zero-based GPT entry ordinal is mandatory. The tool reuses the kernel's
full GPT receipt validation and healthy-only filesystem mount, then rechecks
the receipt. It opens the image read-only, refuses devices and symlinks, never
formats or repairs it, and refuses an existing selection destination. Input
must remain offline and unmodified while inspecting; this integrity pin is not
an exclusive host lock or authentication of a disk owner. Kernel admission
revalidates the selected medium on every boot. MBR, damaged GPT, unhealthy COW
metadata and absent entries fail before writing any selection output.

`EDITION=auto` is the default: a supplied trusted selection selects Desktop;
without one, existing console and experimental profiles retain their selection.
Use `EDITION=desktop` to require Desktop explicitly; an absent selection is an
error before compilation. `EDITION=console` explicitly retains console behavior
even when a desktop selection is supplied, and does not package that selection.
Contradictory edition/session flags and unknown editions are rejected. Selection
paths must be whitespace-free for the Make dependency graph; no image path or
partition is inferred. The host volume tool still accepts ordinary quoted paths.

`make image DESKTOP_SESSION=1 DESKTOP_VOLUME_CONFIG=selected-volume.bin` remains
a supported explicit route to
the native Rust desktop profile. Its kernel feature is `normal-desktop`; none
of the `test-*` features or disposable-media bootstrap profiles are enabled.
Mixing this profile with a test profile, C reference kernel, reference userland
or Python archive fallback is rejected before compilation. Disabling any required
native boot, paging, interrupt or runtime component is also rejected. Both image
routes validate the selection before compiling native kernel or service code.
Without a trusted selection, ordinary image builds remain console builds.

Desktop also rejects externally supplied `RUST_FEATURES`, service-role features
and initramfs extra-argument/dependency variables. GNU Make command-line and
`-e` environment overrides otherwise supersede product assignments, permitting
a Desktop-labelled image to contain console services or omit its selected-volume
configuration. Use the documented edition/native options for product builds;
advanced derived-profile overrides remain available to explicit console builds.
This protects the supported product configuration contract, not an untrusted
host build environment or arbitrary replacement compiler/Makefile.

Each build directory records its resolved edition, selection path, boot entry,
kernel feature graph, service roles and compilation/link inputs in
`boot-profile.state`. An unchanged profile preserves its timestamp; changed
inputs invalidate kernel, service, archive and ISO artifacts. Reusing a build
directory after switching editions therefore cannot silently reuse console
executables in a Desktop image. This does not permit simultaneous builds of
different personalities in the same directory; use distinct `BUILD` roots.

The boot archive carries four independently compiled services: `/bin/init`,
`/bin/files`, `/bin/terminal`, `/bin/compositor`. Boot validates the complete
four-image ELF dependency set before admitting storage, installing GUI grants
or starting a process. Missing/invalid services enter the existing read-only
recovery console. No serial command is required to start a valid session.

The trusted boot archive must also provide `/etc/desktop-volume`, exactly 64
bytes: `INDVOL01` at0–7, little-endian partition ordinal at8–11, zero reserved
bytes12–15, disk GUID16–31, partition GUID32–47, start LBA48–55 and sector count
56–63. Nil GUIDs, unsupported ordinals, zero/short/overflowed ranges and extra or
missing bytes are rejected. This is an explicit integrity selection, **not** a
firmware-rooted authentication or a substitute for secure boot.

The privileged runtime compares that selection against a fresh complete GPT
receipt and strictly mounts the existing healthy filesystem. It never chooses
the first partition or formats a missing, corrupt or foreign volume. Without a
valid selection or healthy volume, boot goes directly to read-only recovery.
The host QEMU fixture provisions only disposable media outside the guest.

After admission, the kernel initializes clipped display hardware and invokes
the four-service runtime. A clean shutdown returns to read-only recovery. An
unexpected service exit (including status zero before durable shutdown), fault
or exhausted cooperative startup budget permits **one whole-session restart
per boot**, only after rollback verifies process authority retirement and the
original free-frame count. The retry consumes a one-shot kernel permission,
revalidates the original complete GPT receipt and healthy non-quarantined
filesystem, and recreates the four services, mailboxes, GUI and physical-input
state. File generations, descriptor tokens and write budgets remain monotonic;
old incarnations do not regain authority. Failure to verify cleanup, media or
restart startup returns to recovery, and a second service failure exhausts the
budget. A deterministic faulty executable cannot cause an endless restart loop.

This adds no ACPI poweroff or retry over uncertain resource state. Full hardware
qualification and exact-SHA end-to-end evidence remain necessary. No M1 gate is
closed by the profile alone.

Tests exercise the actual Make profile matrix, reject unsafe combinations and
cover profile invalidation in a reused build directory. These added tests have
not been executed locally in this source-only batch.
The host volume-tool integration test provisions the existing disposable
fixture through the actual kernel filesystem, derives the same selection, and
checks byte-for-byte unchanged media on success, GPT corruption and damaged
filesystem rejection. It also rejects stale output paths and invalid ordinals.
The M1 combined workflow tests this tool and builds through `desktop-image`.
Feature-enabled kernel tests validate all four ELF images, W^X, missing bytes,
the complete configuration shape and GUID/range constraints. Combined build and
QEMU execution are deliberately deferred until the coordinated source batch is
complete; exact results belong to the integration report.
