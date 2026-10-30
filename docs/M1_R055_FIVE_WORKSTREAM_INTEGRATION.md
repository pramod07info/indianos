# IndianOS M1/R055 — five real reliability workstreams, one commit

Starting main: `416b543dc7eae5223fbf3a25481070062539ee16`. Functional source files were updated directly; no synthetic agent files or placeholders.

| Workstream | Production change | Newly added regression tests |
| --- | --- | --- |
| Developer workstation scripts | `execute_script_atomic` stages terminal output and publishes only after all commands and frame submission succeed. Failed commands and short output roll back the RAM-backed file state without exposing earlier command text. | Failed later command, short destination |
| Persistent project editor | Unicode-safe source navigation and source-line reads no longer expose an extra phantom row after a final newline. Line counts and cursor locations agree, including empty input. | Trailing newline, empty source |
| Virtio-GPU queue isolation | Publicly constructible staged queues are revalidated for owner, domain, mandatory features, power-of-two queue length, DMA bounds and ring non-overlap before numeric descriptor accounting. It still never activates a device. | Forged ring layouts, forged privilege/isolation |
| M2 DNS outbound admission | Reject unspecified, multicast, broadcast and reserved-class IPv4 destinations in validated single-record DNS A results; rejects before adding to DNS cache. | Invalid destination classes / valid unicast control |
| Block-device read integrity | Multi-sector reads use per-sector scratch and zero the entire requested output on an I/O failure, rather than exposing a partially completed prefix or device-dirtied bytes. Bounds errors leave original output unchanged. Writes are unchanged. | Mid-read injected I/O failure, invalid range preflight |

**Validation and scope:** 9 focused Rust tests added across five existing source modules. This repository uses Rust/no_std and requires exact-commit GitHub CI, security gate S0, QEMU persistent graphical Files/Terminal reboot, kernel boot, independent review and branch protection. The connected environment cannot run a Rust compiler locally; **not yet compiled/tested** at the time the commit was authored. Keep PR draft until GitHub checks confirm success.

**Workers:** 5 functional workstreams designed and implemented through one connected GitHub coordinator, **0 independently launched Codex coding agents** from this interface. CI jobs or coordinators do not constitute five coding agents. Do not inflate M1/M2 percentages or claim physical hardware support.

**Hardware policy:** no production disk writes or native hardware activation in this integration; numeric virtio queue models are not DMA/IOMMU evidence. RAM workstation smoke test is not equivalent to the actual persistent boot acceptance gate.
