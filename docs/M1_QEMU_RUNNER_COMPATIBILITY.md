# M1 QEMU runner compatibility

The Python and Rust production smoke runners use `-machine pc,hpet=off`
or `-machine q35,hpet=off` for `preempt` mode. This preserves deliberate
HPET disablement for PIT calibration while supporting QEMU versions that
removed `-no-hpet`, including the Debian trixie runner. Other modes keep
their original machine arguments. Acceptance markers and timeouts are unchanged.

The Rust runner drains QEMU stderr concurrently, retaining its last 8192 bytes
for failure diagnostics together with exit status and serial output. Draining
continues after the buffer fills so QEMU cannot block on a full stderr pipe.
A child guard kills and reaps QEMU on early monitor or console errors as well
as normal completion. Commands use argument arrays without a shell.

Validation on this branch:

- `cargo test --manifest-path tools/rust/Cargo.toml --offline`: 11 passed.
- `python3 -m unittest discover -s tests -p test_smoke_boot.py`: 31 passed.
- Regression coverage checks both machine properties, unchanged ordinary modes,
  subprocess startup errors, and bounded Rust diagnostics after a 256 KiB
  stderr write, exceeding pipe capacity.

These are host runner regression results. The coordinator runs the actual
preemption acceptance gate against the combined kernel ISO; no hardware boot
or preemption acceptance is claimed by this branch alone.
