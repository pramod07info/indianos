# Native app-owned pixel rows

Normal Files and Terminal now rasterize their app content in their own ring-3
processes. The existing command parser and bounded retained-command contract
are reused. The console and app renderer import the same `font5x7.rs`; the
alphabet is not copied into another implementation. Legacy experimental client
profiles keep their existing command submission path.

Each native app owns a three-page aligned ELF data allocation. The runtime
registers its one-page pixel payload and removes the lower/upper guard mappings
from that app's address space. The app never accesses the guards again; guard
copy attempts must fail. This private scratch allocation remains within the
existing bounded user-image page budget. It is not framebuffer MMIO and does
not expose the compositor's memory.

The app retains at most eight validated commands, independently of the kernel's
metadata admission. For each new command it reconstructs the dirty rectangle
from those retained commands into rows of at most256 pixels. Canonical zero
pixels mean uncovered; `ffRRGGBB` means app content. Replacing shorter text
replays underlying fills so old glyphs and spacing cannot survive. Native
syscall39 transfers those rows through REGISTER/BEGIN/TILE/COMMIT/ABORT, with
the shared wire parser and constants used by both sides of the boundary.

A successful commit publishes the app's private model. A rejected row or commit
aborts the pending transaction and exits the failing service; the established
verified session-recovery policy handles it. No app chooses another process's
window, a framebuffer address or an unchecked kernel pointer. PID1 and PID4
also probe that well-formed app-pixel mutation requests are forbidden.

Before application startup handshakes, each app probes foreign-window access,
guard-source copying, incomplete commit, abort and stale-token replay. Distinct
Files/Terminal diagnostic markers report those actual native calls. These
markers alone are not graphical or page-fault proof: the existing screenshot,
copy-boundary, mapping and restart checks remain required.

The host renderer tests cover private models, transparency, exact replacement
clearing and invalid batches. They were authored without local execution, as
requested. Final-head GitHub compilation and actual QEMU acceptance remain
required. The scratch page is private app pixel storage; retained presentation
planes still live behind the kernel's checked copy boundary. This does not add
general shared-memory windows, GPU acceleration or physical-device support.
