#!/usr/bin/env python3
"""S0 defense-in-depth integration audit.

Guards against accidental removal of the actual file-handle enforcement
while refactoring. This is an assertion about source wiring, NOT a
substitute for code review, dynamic adversarial testing or a formal proof.
"""
from pathlib import Path

ROOT=Path(__file__).resolve().parent.parent

def require(ok: bool, message: str) -> None:
    if not ok:
        raise SystemExit(f"SECURITY S0 FAIL: {message}")

def check() -> None:
    source=(ROOT/"rust/kernel_core/src/fs_handles.rs").read_text()
    guard=(ROOT/"rust/kernel_core/src/fs_write_guard.rs").read_text()
    lib=(ROOT/"rust/kernel_core/src/lib.rs").read_text()
    workflow=(ROOT/".github/workflows/security-gate.yml").read_text()
    roadmap=(ROOT/"docs/DELIVERY_MILESTONES.md").read_text()

    open_fn=source.split("    pub fn open<",1)[1].split("    pub fn read<",1)[0]
    write_fn=source.split("    pub fn write<",1)[1].split("    pub fn seek(",1)[0]
    require("WriteGuard::allow_path(path)" in open_fn,
            "writable handle creation no longer checks protected paths")
    require("self.write_guard.preflight(h.pathname(),input.len())" in write_fn,
            "real file write does not enforce S0 policy")
    require(write_fn.index("self.write_guard.preflight(") <
            write_fn.index("fs.write_at("),
            "S0 policy is invoked after the file was modified")
    require(write_fn.index("self.write_guard.completed(n)") >
            write_fn.index("fs.write_at("),
            "write budget is charged before success")
    truncate=source.split("    fn truncate_for<",1)[1].split("    pub fn seek(",1)[0]
    require("self.index(owner,token)?" in truncate and "h.mode&WRITE==0" in truncate,
            "descriptor truncate lost owner or write-right authorization")
    require("self.write_guard.preflight(h.pathname(),changed)" in truncate and
            truncate.index("self.write_guard.preflight(") < truncate.index("fs.truncate("),
            "descriptor truncate bypasses protected paths or mutation budget")
    require(truncate.index("self.write_guard.completed(changed)") >
            truncate.index("fs.truncate("),
            "descriptor truncate charges mutation budget before durable success")
    require("write_guard:WriteGuard" in source and
            "write_guard:WriteGuard::new()" in source,
            "kernel write guard not stored and initialized")
    require("pub mod fs_write_guard;" in lib,
            "S0 module no longer built into kernel")
    for item in ("/boot","/system","/security","/recovery",
                 "/snapshots","/keys","/firmware"):
        require(f'b"{item}"' in guard,
                f"{item} was removed from protected namespace list")
    require("MAX_WRITE_OPERATIONS:u32=" in guard and
            "MAX_WRITE_BYTES:u32=" in guard,
            "session write budgets removed")
    require("test-r031-vfs" in workflow and
            "cargo test" in workflow and "cargo check" in workflow,
            "S0 CI lacks filesystem and no_std acceptance")
    require("continue-on-error" not in workflow,
            "security regression workflow must not tolerate failures")
    require("Security Milestone S0" in roadmap,
            "required milestone missing from roadmap")
    terminal=(ROOT/"rust/kernel_core/src/m1_persistent_terminal.rs").read_text()
    require("WriteGuard::allow_path(p)" in terminal,
            "M1 terminal no longer uses protected namespace guard")
    mkdir=terminal.split('  "mkdir"=>{',1)[1].split('  "write"=>{',1)[0]
    write=terminal.split('  "write"=>{',1)[1].split('  "mv"=>{',1)[0]
    rename=terminal.split('  "mv"=>{',1)[1].split('  _=>',1)[0]
    require("writable_path(p)?" in mkdir and
            mkdir.index("writable_path(p)?") < mkdir.index("fs.mkdir(p)"),
            "M1 mkdir bypasses protected namespace check")
    require("writable_path(p)?" in write and
            write.index("writable_path(p)?") < write.index("fs.overwrite(p"),
            "M1 write/create bypasses protected namespace check")
    require("writable_path(from)?" in rename and
            "writable_path(to)?" in rename and
            rename.index("writable_path(to)?") < rename.index("fs.rename_file("),
            "M1 rename bypasses source/destination protection")
    require("security_m1_terminal_protects_namespaces" in terminal,
            "S1.1 terminal mutation regressions are missing")
    print("S0 integration audit PASS: open/write guarded, protected paths, "
          "CI and roadmap present")

if __name__=="__main__":
    check()
