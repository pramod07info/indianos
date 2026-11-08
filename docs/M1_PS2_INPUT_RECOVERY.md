# Physical keyboard loss recovery

Normal Desktop uses the primary 8042 Set-1 keyboard decoder. Controller
parity/timeout errors, software queue overflow and keyboard-reported overrun
bytes `00`/`FF` cancel the current command using the existing `INPUT_LOST`
event. Device overrun reports must be handled even when the controller status
has no error bits: silently discarding them can join a command prefix to a
different suffix after a lost scan code.

On loss, the driver forgets held modifiers, incomplete extended/Pause sequences
and held CapsLock state while retaining the committed CapsLock setting. It
emits one cancellation marker per loss episode and discards command characters
until a valid unmodified Enter. A later error after that delimiter starts a new
loss episode. The Terminal must consume cancellation even when temporarily
unfocused so old command text cannot survive a focus round trip.

Empty controller status and AUX mouse transfers cannot create or end keyboard
quarantine, reset its modifiers, or count as keyboard input. Set-1 `AA` remains
the Shift-release code; it cannot unconditionally be treated as a device reset
because it also denotes keyboard BAT success outside the scanning protocol.
The mouse retains its independent AUX ownership, bounded packet decoder and
overflow rejection. No USB HID or interrupt-driven mouse support is implied.

Unit coverage exercises clean-status device overruns, command-suffix rejection,
modified Enter, repeated losses, modifier/prefix cleanup, preserved CapsLock,
all controller-status ownership combinations and ordinary Shift release.
These source additions have not been run locally in this source-only batch.
CI and actual QEMU/physical-device evidence remain necessary for gate G5;
these decoder tests alone do not establish real hardware qualification.
