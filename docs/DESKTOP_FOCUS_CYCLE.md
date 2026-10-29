# Stable visible-window cycling

Desktop focus cycling uses increasing, never-reused creation IDs and wraps to
the smallest visible ID. Raising a selected window still updates stacking order,
but no longer changes the next cycle target. Previously three or more windows
could cycle between only two windows. Minimized and closed windows are skipped;
cycling never restores them. Empty and single-window desktops are supported.

In the opt-in R029 polling desktop, the printable `n` shortcut cycles focus and
updates the session's active action target through the existing focus path.
Subsequent move/close actions therefore target the selected window. This shortcut
is scoped to that polling desktop; no new cross-process focus permission or
ring-3 compositor ABI is introduced.

Regressions cover three complete laps of eight windows, direct focus changes,
minimize/close/open interleaving, z-order hit tests, and Session keyboard-to-action
routing. Run `cargo test --manifest-path rust/kernel_core/Cargo.toml desktop::`
and `cargo test --manifest-path rust/kernel_core/Cargo.toml --features test-r029-desktop gui::tests`.
Visual acceptance still requires `make test-r029-desktop-qemu`.

## Observed validation, 2026-10-29

Host suite with `--features test-r029-desktop`: 1,850 passed. Targeted
`no_std` x86_64-unknown-none check passed. The extended
`make test-r029-desktop-qemu` passed real framebuffer comparisons for both
`n` cycle inputs, direct focus, move, minimize, restore and close. A rebuild
initially hit the existing generated-Limine dependency issue (binary removed
during re-extraction); regenerating the build-only Limine tool resolved it.
No source workaround or weakened assertion was used.

