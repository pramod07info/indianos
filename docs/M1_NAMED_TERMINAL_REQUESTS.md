# Named requests from the live Terminal

The bounded Terminal editor additionally parses these requests:

| Command | Request |
|---|---|
| `touch NAME` | Create an empty file |
| `cat NAME` | Read a named file |
| `rm NAME` | Delete a named file |
| `mkdir NAME` | Create a directory |
| `mv FROM TO` | Rename a file |
| `put NAME=VALUE` | Overwrite a named file with literal printable bytes |

Names have 1–15 ASCII letters, digits, underscores, hyphens or dots. Exact `.`
and `..`, slashes, controls and whitespace are rejected. Every name becomes a
zero-padded descriptor path beneath `/shared`; no command chooses another root,
PID, pointer, file descriptor, device or partition. The existing 32-byte editor
bound still applies to the entire command. Put payloads contain 1–20 printable
ASCII bytes, preserving extra equals signs and trailing spaces. Legacy
`write DATA`, bare `cat`, `ls` and `stat` retain their existing parse behavior.

`terminal_fs` provides the typed request, parser and canonical 64-byte IPC codec.
The sender encodes a request; the PID1 broker must independently decode it before
calling the authenticated selected-volume APIs. The decoder requires the exact
message size, valid opcode and lengths, canonical zero padding, valid names and
valid payload. Merely receiving the prefix `F` is not authorization. Existing
IPC sender authentication, process generation, namespace rights, quotas and disk
health checks remain required at the broker and kernel ingress.

The wire is `F`, opcode, first-name length, second-name length, payload length,
three reserved zero bytes, first name[15], second name[15], payload[20], six
reserved zeros. A non-applicable name or payload must be entirely zero.
`Name::path` produces at most 23 bytes followed by a NUL in a 24-byte buffer.

Six new tests cover command/codec roundtrips, exact legacy payload preservation,
name/root escape rejection, every length-byte value, noncanonical wire padding,
forged payload controls, overflow and transport-loss poisoning. Together with
the existing editor tests, the actual parser/codec module suite passed 13 tests.

The native Desktop broker connects these commands to the live authenticated
selected-volume APIs. A successful mutation finishes before Files is asked to
refresh; the broker waits for its reply before sending the Terminal success
result. Failed operations report an error without retrying an uncertain write or
substituting a different operation. The Terminal handles that error and remains
interactive. Sender/namespace/process-generation authority remains kernel-owned.

The G1 acceptance scenario types real PS/2 commands to create a file, overwrite
it with `violet`, rename it, create/delete a second file and create a directory.
It checks the exact Files directory glyphs after each operation, and requires
actual visible Files damage for structural changes. A fresh `cat` must display
the exact `violet` glyphs; the success label `SAVED` cannot satisfy that check.
After cold QEMU reboot, Files must display the committed directory and note,
and a newly typed `cat` must display the renamed file's exact content. Bytewise
serial readback is checked separately from the framebuffer proof. Glyph
expectations are independently pinned in the harness and include blank trailing
cells so stale suffixes, longer notes, wrong fields and mere changed pixels fail.

Every normal startup additionally requires actual pre-readiness negative syscall
controls from Files, Terminal, broker and compositor. Removing a permission probe
or moving its evidence after readiness fails acceptance. Exact-SHA CI and the
gate ledger determine completion; parsing or this document alone does not.
Subdirectories, navigation, multiword filenames, general shells and POSIX file
semantics are outside this bounded command interface.
