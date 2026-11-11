# Explicit normal-boot stack admission

PR767 head `d9af9c6` passed its host/freestanding checks, both healthy graphical
boots, abrupt reboot, transient compositor/guard recovery and namespace
rejection scenarios. Its actual normal Desktop job failed during malformed
partition admission with repeated terminal exception prefixes. The diagnostic
was interrupted before a complete vector/RIP was captured; no particular CPU
exception vector is claimed here.

The exact retained kernel ELF from artifact13700562241 exposes a definite
stack-budget violation. Static disassembly shows approximately 36 KiB reserved
by `kmain`, 16.6 KiB by `inspect_partition_receipt`, 10.1 KiB each by `discover`
and `inspect`, and 12.1 KiB by `gpt`, excluding saved registers, call return
addresses, ATA helpers and fault diagnostics. These frames are nested on the
boot path. Their total already exceeds Limine's default64 KiB stack.

The Rust request table now explicitly asks Limine for256 KiB using the standard
stack-size request. Normal boot checks its response before deep firmware,
filesystem or service work; absence enters existing read-only recovery rather
than silently using the undersized default. The acknowledgment marker is
required by the existing normal QEMU acceptance. This does not alter the
separate per-process kernel trap-stack guards or their allocation contract.

Host tests cover the protocol layout and missing-response refusal. The original
malformed-partition cases and strict failure expectations remain enabled.
Only fresh GitHub QEMU evidence can establish whether this correction resolves
the observed failure; static stack analysis is not a substitute for that run.
No local build, test or emulator was executed.
