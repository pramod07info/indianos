# desktop/

R029 now includes a real opt-in, single-BSP graphical QEMU demo implemented
in `rust/kernel_core/src/gui.rs`, with bounded window state in `desktop.rs`
and real PS/2 auxiliary mouse decoding in `ps2_mouse.rs`. The live QEMU test
captures actual VGA screenshots and checks mouse/keyboard-driven pixel updates.
The compositor is still a **privileged kernel experiment**, not yet a
privilege-separated user-space window server. This directory remains the
planned home for ring-3 compositor, launcher, settings and app surfaces.

See [R029 QEMU graphics evidence](../docs/R029_GRAPHICAL_DESKTOP.md)
and [desktop design](../docs/DESKTOP.md).
