# IndianOS native SDK — experimental x86-64 ring-3

The `indianos-native-sdk` Rust crate provides a tiny no-std wrapper over
the IndianOS native INT 0x80 ABI: `abi_supported`, `write`, `yield_now`,
and `exit`. It depends on no external registry packages.

Build apps as static x86-64 ET_EXEC with the linker script at
`userland/demo/linker.ld`. The kernel rejects dynamically linked,
relocated, overlapping or writable+executable ELF segments.
The QEMU R030 signed package demonstration links this actual crate into
a Rust ring-3 application, then installs the signed app and boots it.

This is NOT an independent software development kit for Linux/macOS/Windows,
and it is not a complete OS API: memory management, files, sockets,
threading, and per-application entitlements remain future work.
