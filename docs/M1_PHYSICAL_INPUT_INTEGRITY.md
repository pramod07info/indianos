# Physical desktop input and loss recovery

The normal desktop syscall ingress uses `input::poll_physical_byte`, which never
reads COM1. The existing experimental `poll_byte` keeps UART input for its
separate console fixtures. Actual 8042 primary-channel bytes remain owned by
the keyboard path; AUX bytes remain for the mouse driver. If IRQ1 is armed,
the polling path consumes its bounded queue and never reads the data register
behind the IRQ handler's back. This remains a serialized single-BSP driver.

Parity/timeout errors now produce an explicit nonprinting INPUT_LOST byte,
reset untrusted modifier/prefix state and quarantine text through the next
unmodified physical Enter. AUX/empty transfers cannot clear or cause this
keyboard quarantine. Loss notification does not increment genuine physical-key
evidence. An ordinary valid make code still does.

Trusted session startup calls `reset_physical_session` with interrupts masked.
It discards old software text, modifier/prefix state and loss quarantine before
new service authority is published, without draining the mouse channel or
changing IRQ configuration. It does not reset physical-provenance counters.

The IRQ queue previously silently dropped input when full. Losing a suffix or
delimiter could turn a partially delivered command into a different request.
It now discards pending text, emits INPUT_LOST and drops further characters
through the next Enter, which is delivered to delimit the rejected line. This
works even if Enter itself caused overflow or the ring has wrapped. Consumed
bytes and discarded queue contents are scrubbed.

The live Terminal parser now poisons an invalid/control-containing line until
Enter or explicit Tab cancellation. Backspace cannot turn loss into a valid
write. These changes protect the actual existing physical ingress and editor,
not an unused input-policy model. No wire format, public syscall number or
input shortcut is changed. Mouse packets, focus grants and existing broker
epochs retain their current ownership rules.

The PS/2 mouse decoder also accepts the full signed nine-bit motion encoding.
The header sign bits are ninth counter bits, independent of payload bit 7.
The former redundant-sign check incorrectly discarded valid +128..255 and
-256..-129 deltas and their button transitions. Decoding now uses payload minus
256 when its sign flag is set. The mandatory header synchronization bit,
primary/AUX channel checks, parity/timeout rejection and axis-overflow rejection
remain. Desktop coordinate clipping remains the hardware-independent boundary.
Tests exercise every one of the 512 encodings on both axes, both button states,
rejected overflowing packets and clean resynchronization without stale motion.

Focused tests cover valid FIFO delivery/wrap, overflow and repeated overflow,
partial-consumption loss, parity/timeout, modifier quarantine, primary/AUX
ownership and all invalid Terminal bytes. Coordinator integration must still
exercise physical keyboard and mouse, negative COM1 injection, focus/capture,
live file operations and reboot readback on the exact combined revision.
These source/tests alone do not qualify IRQ-driven production input, independent
ring-3 input-service isolation, SMP or physical hardware, and close no M1 gate.
