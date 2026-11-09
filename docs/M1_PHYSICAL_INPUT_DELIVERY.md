# Physical input delivery for Normal Desktop

The single-BSP normal session separates hardware collection from ring-3 window
policy. Only PID1 reads physical keyboard bytes and pointer packets; it relays
pointer state to the independent compositor, which owns hit testing, focus and
title drag capture. Files and Terminal cannot read the physical devices.

## Preserve pointer edges

The normal pointer syscall now consumes at most one complete PS/2 packet per
request. Previously it consumed up to four complete packets and returned only
the final position/button state. A stationary press followed by a release
could therefore disappear completely, preventing a real click from reaching
the compositor. Moving while holding and then releasing could similarly lose
capture transitions. Every complete packet now passes through the same bounded
coordinate transformation; ordinary motion remains clipped to the framebuffer.
The existing syscall wire format and legacy experimental profiles are retained.

A state-only GUI regression test covers consecutive press/release, held-button
and idle samples, release after idle, and clipping at both framebuffer corners.
This tests the production transformation without performing PS/2 port I/O.
Actual click and title-drag behavior still needs the QEMU physical-input suite.

## Integrity loss follows editor state

The keyboard driver reports a lost transfer using byte zero and quarantines
untrusted suffix bytes according to its input recovery contract. PID1 must
forward that integrity event to an open Terminal editor even when Files is
currently focused. Treating the event as an ordinary unfocused key would leave
the old partial line live and could later execute an unintended shortened
command. PID1 now poisons the pending line and preserves current compositor
focus; the existing parser rejects the poisoned line until Enter or explicit
editor cancellation. No filesystem operation is requested by the loss event.

The marker `desktop input loss invalidated pending Terminal command` records
this broker path. Parser regression coverage checks rejection of a partial line
and an apparent replacement command after loss, followed by successful editing
after the rejected Enter. A hardware-driver change separately handles device
overrun/error bytes using this same integrity event.

## Evidence and remaining limits

The compositor policy test's presentation fixture uses binary window ID 1,
matching production `[b'P', 1]`; ASCII `b"P1"` remains invalid protocol. This
corrects the observed host-test failure without relaxing validation.

No compilation, tests or QEMU were run locally for these changes, following the
code-only request. Required CI must compile all affected profiles and run host
state tests plus actual physical keyboard/mouse acceptance. These changes do
not establish USB HID support, IRQ12 delivery, arbitrary hardware compatibility,
or a wall-clock recovery guarantee. PS/2 sampling remains polling on one BSP;
malformed/lost mouse-transfer recovery and hardware acceptance require their
own evidence. Gates G3/G4/G5 remain subject to the repository exit criteria.
