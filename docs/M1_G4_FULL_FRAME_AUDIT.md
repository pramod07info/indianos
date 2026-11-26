# G4 full-frame composition and recovery audit

Audit date: 25 Nov 2026. Main baseline:
`b637ce5df22df5c9e05b5d55f0d0442bcf312bbf`. PR #816 intake head:
`e7f06b750f4ccdd075bf9f778c9c11c082af02ed`. The source paths below were
inspected at that head and against the in-progress full-frame and recovery
changes. An uncommitted implementation is not a qualified candidate SHA.

The canonical outcome contracts remain [issue #616](https://github.com/pramod07info/indianos/issues/616),
[delivery milestones](DELIVERY_MILESTONES.md), the [99-task phase scope](M1_PHASE_SCOPE.md),
[task registry](M1_TASK_REGISTRY.json), [gate acceptance](M1_GATES_8_10_ACCEPTANCE.md)
and [integration evidence](M1_INTEGRATION_AUDIT.md). Registry `audit_required`
does not establish that code is missing. This audit creates no alternative
backlog and advances no checkbox. Formal M1 acceptance remains **5/13**;
the registry still records **0/99 fully verified detailed tasks**.

## Every P9 row and its relationship to G4

Symbols without another prefix are under `rust/kernel_core/src`. Existing
implementation in this table means source was found; exact-candidate runtime
acceptance is recorded separately. Broader P9 requirements remain binding for
the complete M1 ledger even when they are outside this focused G4 change.

| Task | Existing production symbols and behavior | Classification and missing behavior | Required acceptance case |
| --- | --- | --- | --- |
| M1-P9-001 · Ring-3 compositor | `gui.rs::UserSurfaceService::{paint,apply_window_frame}`, `compositor_pixels.rs::Request`, `userland/rust/src/main.rs::{desktop_compositor,compose_desktop_row}`. PID4 owns window policy and can assemble an atomic private row from generation-pinned client samples. Kernel `paint` previously assembled the visible scene. | **G4 missing code:** full-frame assembly and bounded presentation; in-progress frame transport must replace normal ring0 scene assembly, while retaining legacy entry points. | Complete background, chrome, clients, stacking, clipping, launcher and pointer from PID4; exact pixel oracle and fresh kernel presentation proof before readiness/ACK. Incomplete, invalid, stale or interrupted frames retain the previous visible frame. Record frame cycles, transport calls and separate startup duration. |
| M1-P9-002 · Private app pixels | `app_pixels.rs::PixelCache::{begin,tile,commit,abort,snapshot}`, `user_runtime.rs::desktop_pixel_request`, `user_image.rs::install_pixel_scratch_guards`, `userland/rust/src/desktop_pixels.rs::State::raster_canvas`. #797 and #812 provide retained atomic planes and guarded private app-mapped canvases. | **Implemented / acceptance pending:** reuse ownership and guard machinery; no second canvas implementation. Larger compositor reads must preserve page ownership, generation and generic user-copy limits. | Actual owner/peer/PID4 CR3 checks; malformed canvas and guard-crossing denial; failed publication retains old pixels; scoped bulk reads deny foreign callers and stale generations; recovery preserves the two client canvases. |
| M1-P9-003 · Window operations and stacking | `userland/rust/src/desktop_window.rs::Manager::{control,compose_screen_strip,compose_row}`, `gui_protocol.rs::DesktopFrame::{parse,wire}`. Focus, title drag, minimize/restore, clipping and opaque foreground occlusion exist. | **G4 integration:** preserve these semantics in the full-frame renderer. The earlier transparent-client occlusion fix must not be duplicated or lost. | Transparent foreground samples retain their own opaque chrome/body; minimizing reveals the exact rear pixels. Bounded geometry, real PS/2 focus/drag/release/minimize/restore and keyboard transitions cancel old drag anchors. |
| M1-P9-004 · Physical IRQ focus/input | `input.rs::ScanDecoder`, input IRQ entry, `ps2_mouse.rs`, PID1 routing in `userland/rust/src/main.rs`. Normal physical PS/2 ingress and authenticated broker routing already exist. | **G5 dependency:** retain delivery and input-loss policy. PID4 recovery must revoke capture without treating a held button as a new press. The registry's current surface-oriented source references are not proof of a missing input implementation. | Real Set-1/AUX events, focus/capture and input-state screenshots; held motion after replacement cannot capture until actual release, followed by a fresh press. UART injection cannot satisfy these cases. |
| M1-P9-005 · Shell/taskbar/launcher | `gui.rs::{Palette,UserSurfaceService::paint}` and `desktop_window.rs::Manager::control` contain the established background, chrome, taskbar and launcher state. | **G4 scene migration:** move the current visual behavior to userspace, rather than inventing a replacement shell or theme. | Existing exact glyphs, colors, launcher toggles, app selection and pointer readback remain correct through normal boot, interaction and replacement. |
| M1-P9-006 · Unicode/layout | `font5x7.rs::glyph`, `console.rs::glyph` and `desktop_pixels.rs::State::row` implement the current bounded glyph path. The inspected normal path does not provide a Unicode text/layout engine. | **Broader P9 requirement:** remains open. Not evidence that the explicit G4 compositor/isolation exit requires a new font engine in this PR. | A future dedicated text/layout qualification must cover encoding, missing glyphs, geometry and rendering. Preserve current ASCII pixel acceptance here; do not mark this row complete. |
| M1-P9-007 · Theme/icon/wallpaper controls | `gui.rs::Palette` and the opt-in `orbit_ui` path provide fixed colors/preview visuals. Interactive normal-desktop customization was not located in the inspected path. | **Broader P9 requirement:** remains open; fixed palette migration is required here, new customization is separate. | Future normal-session controls need persistence, bounds and visible readback. This PR must preserve the original IndianOS visual identity. |
| M1-P9-008 · Acceleration/software fallback | `gui.rs::Surface::{fill,glyph,text}` provides software raster primitives; PID4 full-row assembly and kernel hardware copy extend the software route. | **G4 software path / G8 driver dependency:** qualify the software framebuffer route now. Hardware GPU acceleration is not established by these changes. | Supported framebuffer geometry/format and clipped software presentation pass; legacy and recovery paths remain operational. Driver-specific acceleration requires its own device evidence. |
| M1-P9-009 · Multi-display/DPI | `boot.rs::console_framebuffer` and `gui.rs::Surface::from_boot` use the selected boot framebuffer. Normal multi-display selection and DPI policy were not located. | **Broader P9 requirement:** remains open. Single-display full-frame support must state its admitted bounds, without claiming this row complete. | Future display/DPI qualification covers independent geometry, scaling, layout and device changes; current tests must cover supported single-display extents and arithmetic. |
| M1-P9-010 · Clipboard/drop/notifications | `desktop_window.rs::Manager` supports window dragging; focus notifications exist. Neither is a clipboard, file drag/drop or notification UI implementation. Those normal paths were not located. | **Broader P9 requirement:** remains open; do not relabel existing pointer movement as these features or create duplicate IPC infrastructure. | Future dedicated UI/security cases must cover data ownership, cancellation and delivery. Current G4 acceptance retains pointer capture and authenticated IPC checks. |
| M1-P9-011 · Compositor restart/isolation | `user_runtime.rs::{DesktopLifecycle,discard,x86_64_m009_schedule_run}`, `gui.rs::UserSurfaceService::retire_client`, `ipc.rs::IpcRouter::unregister`. Existing fault policy retires and reloads the whole session. | **G4 missing code:** one bounded PID4-only replacement, fresh frame/IPC authority and readiness while PID1, Files and Terminal remain alive. | Controlled live PID4 fault during an active frame; unchanged client incarnations, file epochs, canvases and committed content; stale frame authority denied; no inherited capture; restored physical input and durable shutdown. Replacement load failure, withheld readiness and repeated failure follow the bounded fallback policy. |
| M1-P9-012 · Image/input/display acceptance | `tools/test_normal_desktop_qemu.py::{transient_service_boot,verify_pixel_startup,verify_text_pixels,verify_debug_stack_guards}`, host controls and `.github/workflows/m1-reliability-integration.yml` provide real graphical/input and recovery oracles. | **G4 emulator acceptance pending / G8 physical dependency:** add full-frame provenance and client-preserving replacement receipts. Preserve G1–G3 partition, reboot, startup and shutdown-exhaustion cases. Physical display qualification remains separate. | Exact PR-head host/freestanding/QEMU/security/M2 receipts, actual serial/register/pixel artifacts, independent review and critical merged-main rerun. An emulator screenshot does not establish physical-machine support. |

## Review findings and verification limits

Independent draft review identified two integration hazards: a replacement
could reinterpret an already-held pointer button as a fresh capture, and
returning BAD for PID1 input during replacement could terminate an otherwise
healthy broker. The changing implementation now includes pointer release
quarantine, a zero-result yielding input continuation, pinned button state
and presentation-bound replacement readiness. These fixes still require the
integrated candidate tests and live recovery evidence; inspection alone does
not qualify them.

The reviewer independently executed the production window-policy host suite
from the root working tree: **15 tests passed**, including full-scene opacity,
format/geometry checks, release quarantine and sequenced delivery. The reviewer
also executed all **six G4 kernel regressions**, reusing the transport worker's
compiled host test binary for clean source commit `763cb7a`. Those tests passed
geometry/wire rejection, generation-bound bulk samples, private staging until
complete publication, changed-input/client rejection, exact-state replacement
replay, and scoped copy/guard-page checks. This was not an independent kernel
rebuild or a test of the final combined scheduler/QEMU guest. No GitHub approval,
hardware result or final candidate acceptance is implied.

## Formal acceptance boundary

The existing automated evidence reviewer is not an independent source approval.
The explicit collaborator/sign-off packet in
[the G1–G3 review protocol](M1_GATES_6_7_ACCEPTANCE.md#g1g3-independent-source-review-and-explicit-sign-off--24-nov-2026)
applies to those named gates and must not be misreported as an existing G4
approval. G4 remains subject to issue #616 and the general gate acceptance
requirements: actual source/security review, passing exact-candidate checks,
retained artifacts, merged-main verification and a recorded acceptance decision
through the repository's reviewed registry/checklist process.

A coding-agent review or assignment is not a submitted approval by an eligible
independent repository collaborator. Before accepting G4, pin the final PR SHA,
record actual run/job URLs and artifact IDs, disposition every mandatory G4
criterion and dependency, and obtain the applicable independent decision.
Keep implementation status separate from `tests passed on SHA`, `independent
review` and `accepted on main`. This audit leaves the M1 gate count unchanged.
