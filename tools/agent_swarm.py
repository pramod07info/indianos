#!/usr/bin/env python3
"""Safely scope ten separate local-model code-generation tasks."""
import argparse
import os
from pathlib import Path
import re
import sys

AGENTS = {
    "01": {"branch": "agents/01-filesystem-scale", "module": "agent_01_fs_extents", "purpose": "Validate sorted, nonoverlapping, nonzero block extents with bounded checked arithmetic; reject corrupt entries and overflow."},
    "02": {"branch": "agents/02-signed-apps", "module": "agent_02_pkg_gate", "purpose": "Build signed-package policy preflight taking an ALREADY verified signature boolean, trusted signer id and version; reject unverified, untrusted and downgrade. Do not claim cryptographic verification."},
    "03": {"branch": "agents/03-security-identity", "module": "agent_03_cap_epoch", "purpose": "Validate owner, rights, revocation epoch and generation for bounded capabilities; reject stale grants and identity spoofing."},
    "04": {"branch": "agents/04-service-manager", "module": "agent_04_service_restart", "purpose": "Build finite-state service restart policy with crash detection, per-service restart budget, generation increments and explicit stop states."},
    "05": {"branch": "agents/05-userspace-compositor", "module": "agent_05_surface_rect", "purpose": "Validate safe rectangle clipping and per-process surface ownership for 1280x800 screen using checked arithmetic."},
    "06": {"branch": "agents/06-network-stack", "module": "agent_06_ipv4_route", "purpose": "Implement bounded IPv4 longest prefix match routing table with default and /32 routes, no actual packet I/O."},
    "07": {"branch": "agents/07-hardware-drivers", "module": "agent_07_pci_aperture", "purpose": "Validate numeric PCI memory apertures, alignment, allowed range and overflow, with no hardware I/O."},
    "08": {"branch": "agents/08-updates-recovery", "module": "agent_08_boot_slot", "purpose": "Choose safe validated boot slot after interrupted update using two-slot metadata and monotonic version guards."},
    "09": {"branch": "agents/09-ci-hardening", "module": "agent_09_ci_policy", "purpose": "Reject merge when mandatory CI outcomes are pending, failed or skipped, or checked SHA is stale."},
    "10": {"branch": "agents/10-app-ai-planner", "module": "agent_10_app_policy", "purpose": "Check trusted catalog, signed manifest, architecture match and explicit permission approval; never install anything."},
}
SRC = Path("rust/kernel_core/src")
START = "BEGIN_INDIANOS_RUST"
END = "END_INDIANOS_RUST"
BANNED = re.compile(r"\bunsafe\b|\bextern\b|\bstd\s*::|\bstatic\s+mut\b|"
                    r"\b(?:include|include_str|include_bytes|env|global_asm|asm|"
                    r"compile_error|cfg_attr)\s*!|#\s*\[\s*(?:path|link|no_mangle)\b", re.I)

def prompt(agent, retry=False):
    t = AGENTS[agent]
    return ("Produce ONLY fully compilable safe Rust 2021 source between "
            "BEGIN_INDIANOS_RUST and END_INDIANOS_RUST. The code is a bounded "
            "no_std-compatible helper: no heap, std, unsafe, FFI, syscalls, "
            "hardware I/O, commands, external crates, global mutable variables, "
            "build macros or side effects. Expose pub fn and explicit errors. "
            "Include at least THREE real #[test] unit tests inside #[cfg(test)] "
            "for success, denial and boundaries. Keep below 15000 bytes. "
            "Implement exactly this narrow module: " + t["module"] + ". " +
            t["purpose"] + " "
            + ("RETRY after rejection: prefer fixed arrays and simple types. " if retry else "")
            + "Your first output bytes MUST be BEGIN_INDIANOS_RUST. End output "
            "with END_INDIANOS_RUST. No markdown or explanation.\n")

def parse(raw, agent):
    a = raw.find(START)
    b = raw.find(END, a + len(START))
    if a < 0 or b < 0:
        raise ValueError("Missing markers")
    code = raw[a + len(START):b].strip()
    if not 300 <= len(code.encode("utf8")) <= 15000:
        raise ValueError("Source size outside safety bounds")
    if BANNED.search(code) or "\x00" in code:
        raise ValueError("Unsafe or privileged code rejected")
    if code.count("#[test]") < 3 or "#[cfg(test)]" not in code or "pub fn" not in code:
        raise ValueError("Missing public functions or tests")
    if re.search(r"#!\s*\[|#\s*\[\s*macro_use\b|(?:use|mod)\s+crate\s*::", code):
        raise ValueError("Disallowed compilation override")
    return "// Generated limited helper for " + AGENTS[agent]["module"] + "\n" + code + "\n"

def main():
    p = argparse.ArgumentParser()
    p.add_argument("action", choices=("prepare", "retry", "apply"))
    p.add_argument("--agent", required=True, choices=sorted(AGENTS))
    p.add_argument("--file", required=True, type=Path)
    args = p.parse_args()
    t = AGENTS[args.agent]
    path = SRC / (t["module"] + ".rs")
    marker = "// IndianOS SWARM EMPTY SLOT: " + t["module"] + "\n"
    try:
        if args.action == "prepare":
            selected = path.exists() and path.read_text(encoding="utf8") == marker
            if selected:
                args.file.write_text(prompt(args.agent), encoding="utf8")
            if os.getenv("GITHUB_OUTPUT"):
                with open(os.environ["GITHUB_OUTPUT"], "a") as out:
                    out.write("selected=" + str(selected).lower() + "\n")
            print("Selected" if selected else "Already complete", t["module"])
        elif args.action == "retry":
            args.file.write_text(prompt(args.agent, retry=True), encoding="utf8")
        else:
            if not path.exists() or path.read_text(encoding="utf8") != marker:
                raise ValueError("Not an empty, preapproved agent slot")
            content = parse(args.file.read_text(encoding="utf8"), args.agent)
            path.write_text(content, encoding="utf8")
    except (ValueError, OSError) as exc:
        print("Agent candidate rejected:", exc, file=sys.stderr)
        raise SystemExit(1)

if __name__ == "__main__":
    main()
