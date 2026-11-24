# G5–G7: interrupt ingress and interrupted guest publication

The source audit at `7b2184749db056bf4d19e3f1c3d162179fc71440`
found the normal physical-input and durable-file GUI path already implemented.
The existing acceptance suite covered 25 boots, including a SIGKILL **after**
file commit acknowledgement. It did not stop a live guest during publication.
The syscall ABI version query, owned range locks and permission-denial probes
also already existed and are retained rather than recreated.

The normal-desktop harness now keeps all those cases and adds three actual
interrupted-publication cases, each followed by a fresh native desktop boot:

| Cut | Actual stopped execution | Required fresh GUI readback |
| --- | --- | --- |
| Data | Before ATA data-sector write 2; write 1 has completed and flushed | Old `SAVED` file |
| Before metadata | Before the alternating metadata-sector write; all four new data sectors have completed | Old `SAVED` file |
| After metadata | Before the metadata verification read; the new metadata write and its driver flush have completed | New `VIOLET` file |

The host types every command through emulated PS/2 input. An external QEMU
execution breakpoint identifies the existing `AtaPio::select` boundary, exported
as `indianos_ata_select_command` for stable native driver diagnostics. That
function remains non-inlined for native diagnostic traces; it has no injection
mode, callback or test-dependent branch. The debugger requires the exact ELF
instruction bytes, kernel privilege and current PID1 before inspecting the
qualified x86-64 native entry arguments. A different compiler ABI, unexpected
command sequence or missing symbol fails closed. Four sequential data writes
must precede one selected-volume metadata write. No kernel instruction, page
table, register or disk byte is modified by the debugger.

QEMU is killed while still stopped, before the selected next ATA command.
Serial commit and input-completion acknowledgements must be absent after the
submission boundary. A cut with no actual media mutation is rejected. The
first two cuts require both metadata sectors unchanged; the third requires
exactly one valid next-generation checkpoint. Every scenario checks the
surrounding GPT/media bytes, reboots the same disposable disk, reads the exact
old/new contents through fresh independent native processes and verifies glyphs
in a real screenshot. Normal synchronized shutdown and all four service reaps
must follow. Recovery/readback must not change any disk bytes.

G5 checks now require IRQ1/IRQ12 delivery and valid decoded queue markers after
the first physical-input boundary in both standard desktop boots. UART remains
a negative control. Existing focus, drag, release, minimize/restore and exact
Files/Terminal screenshot checks remain mandatory. Host negative controls
reject missing, stale, duplicated and out-of-order IRQ records and impossible
publication command/checkpoint evidence.

No local project code, builds, tests or QEMU ran while authoring the initial
31-boot batch. The follow-up below has separate host-test evidence.
These are interruptions between actual sector commands, **not** torn-sector,
volatile drive-cache loss or physical power-cut qualification. Physical-device
flush guarantees, broader concurrency and independent release review remain
separate acceptance obligations. This document does not mark a full gate 100%.

## Namespace-publication extension

The follow-up reuses the same ATA debugger/cut helper for rename, unlink and
directory creation, stopping before and after their metadata publication.
Rename and unlink must issue **zero** data-sector writes. Directory creation
must issue exactly four data-sector writes before publishing its marker and
permissions. The three original replacement cuts retain their four-sector
contract and their requirement that a cut actually changed media.

A metadata-only cut before its first metadata write legitimately leaves the
image unchanged. That exception is enabled only by an explicit zero-data
contract in the pre-metadata phase; all other unchanged-image cuts fail.
Metadata-only operations also require every data-sector byte unchanged,
including orphaned/unreferenced banks, before or after publication.

Each of the six namespace cuts is followed by **two consecutive cold QEMU
boots** from the same image, without resetting it between those boots. Each
fresh desktop must produce exactly one complete native `ls` result and an
exact Files directory screenshot. Rename must contain exclusively the old or
new name, unlink must retain/remove exactly its target, and mkdir must expose
no marker as a regular file. Readback through native Terminal syscalls and
exact glyph checks must preserve `HELLO INDIANOS` in the unrelated readme and
`SAVED` in the retained/renamed cut file. Both reboots require synchronized
shutdown, all four reaps and byte-for-byte unchanged disk contents.

All original 31 boots remain, with 18 additional boots: six interrupted guest
executions and twelve recovery/readback boots, for **49 configured VM boots**.
The cloud host suite `test_normal_desktop_acceptance.py` ran after explicit
authorization and passed all **49 tests**, including negative controls for
dual/leaked names, stale/duplicate listings, unsupported cuts, incomplete or
unexpected data writes and modified data during metadata-only operations.
Rust/QEMU tools are unavailable in this workspace; the 49-boot runtime matrix
still requires execution by exact-head GitHub Actions. Host harness controls
do not establish guest crash consistency on their own.
