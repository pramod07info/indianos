# G7 native positioned file IO

This phase extends the existing version-1, 64-byte syscall-33 descriptor. It
exposes existing descriptor operations 25 (PREAD) and 26 (PWRITE); it does not
introduce another filesystem, on-disk format, process identity, or handle table.
The descriptor argument at bytes 32–39 is the absolute byte offset. PREAD copies
at most 72 bytes to a fully validated writable user range; PWRITE copies at most
256 bytes from a validated readable user range. Offsets are bounded by the
existing 2048-byte file limit and writes reject offset-plus-length overflow or
out-of-file bounds before handle or device mutation. Both preserve the descriptor
cursor. Existing permissions, authenticated process generations, namespace
ownership, file locks, and selected-volume recovery policy still apply.

ABI feature flag `0x10` advertises positioned IO; flag `0x20` advertises coordinated
unrelated descriptor readers. Existing features remain additive and the layout
and version stay unchanged. Files requires flags `0x08` and `0x10` before offering a held
preview. It rereads the original descriptor with PREAD at offset zero, then proves
that a sequential one-byte read still sees the first original byte, and seeks
back to zero. Empty files require a zero-length cursor probe. No reopened token
can substitute for the held descriptor.

The normal desktop Terminal accepts `patch peer 2=xy`. Its offset is a canonical
unsigned decimal value; payload bytes are preserved, including spaces and equals.
The existing 20-byte Terminal file limit applies to offset plus payload length.
The existing 32-byte editor limit still bounds the entire command. Names remain
single validated components under `/shared`; no user identity or absolute path
is supplied through IPC. Opcode 7 uses the existing fixed 64-byte request, with a
little-endian u16 offset at bytes 5–6 and byte 7 reserved zero. Other opcodes
continue to require all three bytes zero. PID1 negotiates feature flags `0x10` and `0x20`,
opens the existing file, issues PWRITE, and closes on success or rejection. It
reports success only for an exact write count and successful close, then waits
for Files refresh before replying. The existing atomic `put name=value` operation
is unchanged. A denied patch is never retried through another operation.

The actual Terminal PID3 attempts a valid handle-free ABI query, plus well-shaped
native PREAD and PWRITE descriptors at startup, and requires all to be rejected.
The query proves absence of file-service authority independently of token validity. Terminal IPC grants no direct file
capability. Cloud QEMU acceptance must separately verify physical input, retained
reader/cursor evidence, denied same-file patches, actual disk bytes, and cold
reboot readback. Host tests alone do not accept G7 or establish physical hardware
support.
