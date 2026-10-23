# R030 — Ed25519-signed native application packages and QEMU SDK

## Scope actually implemented

R030 provides a **developer-host application packaging tool** for
IndianOS-native x86-64 freestanding Rust executables. Developers can
compile an x86-64 static ET_EXEC ELF, build a deterministic signed archive,
verify an explicit trusted Ed25519 public key, stage packages in a dedicated
directory, install, update, rollback, remove and export a verified executable.

It is not yet an on-device IndianOS package installer, persistent package
daemon, app store, or user-mode application launcher. The sample ELFs
are run in a dedicated QEMU test boot after verification and initramfs
staging, not dynamically installed by a running guest.

The workflow is completely offline and deliberately does not download
code or attempt to install packages on the CI runner's host OS.

## CLI and signed format

```sh
# Test/example keys only; use a separately provisioned protected key in production.
openssl genpkey -algorithm Ed25519 -out /tmp/r030-dev-private.pem
openssl pkey -in /tmp/r030-dev-private.pem -pubout -out /tmp/r030-trusted-public.pem

make user-elf BUILD=build/r030-v1 R030_PACKAGE_V1=1
python3 tools/r030_pkg.py build \
  --elf build/r030-v1/userland/demo.elf \
  --name hello --version 1.0.0 \
  --private-key /tmp/r030-dev-private.pem --output /tmp/hello-v1.indpkg
python3 tools/r030_pkg.py verify \
  --package /tmp/hello-v1.indpkg --public-key /tmp/r030-trusted-public.pem

python3 tools/r030_pkg.py install --root /tmp/r030-stage \
  --name hello --package /tmp/hello-v1.indpkg \
  --public-key /tmp/r030-trusted-public.pem
python3 tools/r030_pkg.py status --root /tmp/r030-stage \
  --name hello --public-key /tmp/r030-trusted-public.pem
# update needs a separately signed higher version; rollback needs a previous version
python3 tools/r030_pkg.py export --root /tmp/r030-stage --name hello \
  --public-key /tmp/r030-trusted-public.pem --output /tmp/verified.elf
```

Archive members are exactly `manifest.json`, `app.elf`, and
`signature.ed25519` with uncompressed bounded ZIP storage and fixed
ordering/timestamps. The manifest is a canonical versioned JSON object
that pins package name, strict numeric semantic version, target
`indianos-x86_64-elf-v1`, entrypoint `/bin/init`, payload length
and SHA-256. Ed25519 signs the domain-separated manifest and ELF bytes
together. The trust public key is supplied explicitly; it cannot be
overridden by an arbitrary key embedded in the archive.

The verifier refuses dynamic-interpreter ELFs, invalid endianness/
architecture, unbounded or malformed program headers, missing PT_LOAD,
writable-and-executable PT_LOAD, path traversal and symlink members,
ZIP compression, duplicate/unknown archive members, unknown manifest
fields, invalid digital signatures, wrong package identity and
non-increasing update versions. The kernel still performs its own
ELF stage and page-ownership checks at runtime.

The isolated staging store serializes updates via a file lock and
atomically replaces package files with fsync. It retains one previous
version, verifies signatures again on rollback/export, and refuses
symlinked staging directories. Installation is not crash-safe at the
whole-volume level and does not confer process-level permissions.

## Acceptance test

```sh
python3 -m unittest discover -s tests -p 'test_r030_pkg.py' -v
make test-r030-package-qemu
```

The test generates **ephemeral test keys**, builds two distinct native
Rust ELF64 apps, signs and installs version 1, upgrades to version 2,
rolls back to version 1, rejects a damaged package, then removes it.
Each live lifecycle phase exports a freshly signature-verified ELF,
assembles a QEMU-only initramfs and boots it under OVMF. A real ring-3
program must emit the correct version-specific serial marker and exit
through the existing `int 0x80` kernel ABI.

This test is a required R030 gate in full kernel CI. All temporary key
files, staging directories and test ISOs are **development-only**.
Never ship the generated test private key.

## Outstanding on-device goals

Native persistent package management inside the guest, a trusted-key
provisioning mechanism, user approvals, package dependency resolution,
atomic metadata and disk recovery, sandboxed third-party process
launching, per-app capabilities, a package GUI, a public repository and
the AI installation assistant are **not implemented here**.

Independent Windows, Linux and macOS application binaries are not
compatible with this format. R030 is a verified SDK foundation, not
general OS application compatibility.

### GitHub Actions verification

[Passing R030 live QEMU SDK verification #39959376254](https://github.com/pramod07info/indianos/actions/runs/39959376254)
confirmed six host security/lifecycle tests, Rust host ABI regression,
QEMU ring-3 execution of native signed v1 and signed v2 after update,
QEMU execution of signed v1 again after rollback, plus tamper denial and
removal. This does not turn host staging into a guest package daemon.
