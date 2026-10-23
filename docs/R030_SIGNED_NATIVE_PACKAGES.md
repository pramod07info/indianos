# R030 — Native SDK and signed offline development packages

## Actual implementation

R030 adds a no-std Rust native application SDK, an Ed25519-signed native
package format, an offline developer-host install/update/rollback/remove
utility, and a QEMU test that boots the **exact installed verified ELF**
inside IndianOS ring 3.

The native SDK is at sdk/rust. It wraps the real IndianOS INT 0x80 syscalls
for ABI query, bounded text write, voluntary scheduling yield and exit.
The test native ELF is linked against this SDK, rather than merely
pretending that an ordinary host executable is an IndianOS program.

The package archive (.ipkg) contains exactly three uncompressed files:
manifest.json, manifest.sig, payload.elf. Its manifest has name, version,
architecture, SHA-256 of the ELF, byte length and a fixed /bin/init entry.
The Ed25519 signature covers canonical JSON manifest bytes; a separately
provided public key is the trust anchor. Private keys exist only in the
developer's environment or in a temporary CI directory.

## Transaction and defensive checks

- Developer must specify an existing absolute installation root. No default
  system path, network access or implicit privilege escalation.
- The archive rejects duplicate names, unknown files, path traversal,
  compressed payloads, symlinks, excessive sizes and malformed JSON.
- The ELF verifier accepts only static x86-64 ET_EXEC user programs and
  rejects dynamic/interpreted ELF, executable+writable segments,
  overlapping pages and invalid entry or memory ranges.
- A signature and digest must verify before install. Versions cannot
  silently downgrade or be duplicated.
- Package writes go into content-addressed, non-executable host files.
  An atomic state-file update makes a staged version active and preserves
  the previous version for rollback. Rollback checks its stored digest.
- Removal disables activation; it does not execute uninstall hooks or
  delete unknown filesystem paths.
- Fifteen host regression tests cover forged signatures, wrong trust keys,
  tampered content, state and payload symlinks, ELF denial, rollback and
  fail-closed behavior.

The package tool runs on the **development host**, not in the IndianOS
kernel. Its state file is not a tamperproof protected OS trust store; it is
not a production-grade package manager against malicious local root users.

## Reproducible QEMU acceptance

Requires Rust x86-64-unknown-none target, QEMU, OVMF, Clang/LLD, xorriso,
Python3 and OpenSSL.

~~~sh
python3 -m unittest discover -s tests -p 'test_r030_packages.py' -v
cargo test --offline --manifest-path sdk/rust/Cargo.toml
make test-r030-native-qemu OVMF_CODE=/usr/share/OVMF/OVMF_CODE_4M.fd
~~~

The acceptance test:
1. Compiles an actual SDK-dependent ring-3 ELF and generates temporary
   Ed25519 developer keys.
2. Signs a 1.0.0 package, verifies it with the corresponding public key,
   and installs its content-addressed ELF into an explicit temp root.
3. Regenerates initramfs from the **verified installed ELF** and rebuilds
   a QEMU ISO. Extracts the initramfs from that ISO and verifies exact bytes.
4. Boots IndianOS in QEMU and demands the real native SDK app's serial
   marker and clean ring-3 exit through INT 0x80.
5. Installs a separately signed 1.0.1 update, injects its exact ELF,
   and boots QEMU again.
6. Rolls back to 1.0.0, verifies that the ELF is byte-identical to its
   original signed version, and boots QEMU a third time.
7. Removes the application and verifies it is no longer activatable.

The test never boots arbitrary physical media and never puts its
private test signing key in the git repository. The build fix also ensures
a second ISO generation does not accidentally re-extract Limine sources
and delete the already-built installer.

## User-facing future work

The on-device IndianOS package service, protected OS trust store, durable
disk installation, GUI launcher, dependency resolution, app permissions,
audited update daemon, physical-PC execution and binary compatibility
with Linux, Windows, macOS or Android remain unimplemented.

R030's host-only signed package proof must **not** be described as an
already functioning IndianOS app store or general application installer.
