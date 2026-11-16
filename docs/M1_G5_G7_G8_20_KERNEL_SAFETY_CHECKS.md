# M1 G5/G7/G8: twenty kernel safety acceptance checks

Base SHA: c665cc2edf72085d354c1328d23f6f818c744ba2 (main after PR #778).
Four production modules changed with regression tests. Twenty numbered checks
are assertions within those tests, NOT twenty independently launched agents.

## Acceptance checklist

1. G7: reject persisted sticky-bit ACL modes.
2. G7: reject setuid/setgid-like invalid mode bits.
3. G7: reject all-bits-set ACL values.
4. G7: prevent root bypass of corrupted ACL metadata.
5. G7: preserve validated owner/group/other grants.
6. G7: deny unknown access flags even to root.
7. G8: reject zero ECAM base from public Window.
8. G8: reject misaligned ECAM base.
9. G8: reject inverted start/end buses.
10. G8: reject complete ECAM aperture overflow.
11. G8: preserve valid byte 4095 of the last function.
12. G8: reject invalid bus/device/function/offset and segment.
13. G8: reject null and low-half HHDM values.
14. G8: reject noncanonical higher-half HHDM values.
15. G8: reject unaligned HHDM values before shadow CR3.
16. G5: do not consume pending primary keyboard bytes.
17. G5: preserve partial AUX packet across primary-keyboard yield.
18. G5: drop parity/timeout AUX data and reset partial packet.
19. G5: preserve valid three-byte packet delivery.
20. G5: keep bounded three-status poll work.

## Production changes

* fs_permissions: never grant from a publicly constructed invalid mode.
* hardware_mcfg: validate complete public Window geometry on address query.
* paging_cpu: guard HHDM canonical high-half and page alignment before CR3.
* ps2_mouse: shared testable AUX callbacks with real poll path.

## Mandatory exact-PR-head evidence (none claimed at submission)

- [ ] Rust host suite (cargo test --manifest-path rust/kernel_core/Cargo.toml).
- [ ] Freestanding no_std build, S0 security and other impacted CI.
- [ ] Disposable-disk QEMU two-cold-boot data retention and GUI input.
- [ ] Independent review, correct protected merge, post-merge main tests.

Gate status remains 5/13 high-level M1 acceptance pending real evidence;
the eight open M1 gates are not closed by host tests or source changes.
