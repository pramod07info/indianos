# Read-only init boot recovery

Normal Rust kernel startup enters a bounded COM1 recovery console when
`/bin/init` cannot be found in a valid initramfs or its detached image staging
fails. Valid normal boot keeps its existing behavior. This does not launch a
production PID1 or complete normal desktop startup.

The console masks interrupts and polls COM1. `help` lists its commands,
`status` reports the fixed failure reason and disabled storage/user execution,
and `halt` stops the CPU. It cannot format, repair, retry staging, inspect
arbitrary memory, start programs or reboot. This matters because a failed
staging attempt can leave uncertain resources; recovery never retries it.

Input uses a 32-byte stack buffer. Backspace edits a valid line; overflow,
unsupported control bytes or a damaged UART transfer poison the entire line
until the next newline. A later command suffix cannot bypass rejection.
CRLF produces one command. Unknown commands have no side effects.

Validation commands:

```sh
cargo test --locked --offline --manifest-path rust/kernel_core/Cargo.toml --lib --features kernel-main recovery_tests
python3 -m unittest discover -s tests -p test_boot_recovery.py -v
make test-boot-recovery-qemu
```

The QEMU harness first boots a normal control, then boots disposable ISO
variants with a missing module, a valid archive missing init, a corrupt
archive, and an invalid init ELF. It injects real UART commands and checks
rejection, status/help responses and halt ordering. A writable disposable
sentinel drive must retain its hash after every boot; snapshot overlays are
disabled so they cannot hide writes. No physical host disk is opened.

The coordinator records actual QEMU results in the integration report.
Physical machines, graphical recovery, storage-failure recovery and a
separate recovery process remain unsupported by this slice.
