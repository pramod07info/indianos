#!/usr/bin/env python3
"""Bounded Agent X Rust code-generation handoff.

Accepts *only* new, allowlisted, self-contained no_std Rust modules. It never
runs model-authored shell commands, accepts model-authored file paths, or grants
the model Git credentials. Tests and push gates live in the GitHub workflow.
"""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
import sys

MODULE_ROOT = Path("rust/kernel_core/src")
LIB = MODULE_ROOT / "lib.rs"
MAX_GENERATED_BYTES = 16000

# Extending this list requires a reviewed repository change. Each new item is
# a small, isolated kernel library milestone rather than an arbitrary prompt.
TASKS = (
    {
        "name": "capability",
        "purpose": (
            "Implement a no_std fixed-capacity capability registry. "
            "Use u32 owner IDs and u64 capability tokens, where zero is invalid. "
            "Each entry binds owner, resource ID, and bitmask rights. "
            "Support grant, lookup/authorize (all requested rights required), "
            "revoke and revoke_all(owner). Reject duplicate tokens, invalid "
            "owners, zero tokens, zero rights, unknown and revoked tokens, "
            "and capacity exhaustion. Never allow rights escalation. "
            "Use no heap, syscalls, global mutable state, or external crates."
        ),
        "required": ("pub struct", "grant", "revoke", "#[cfg(test)]"),
    },
    {
        "name": "vfs_path",
        "purpose": (
            "Implement a no_std, no-heap absolute-path normalizer for a future "
            "read-only VFS. Reject empty and relative paths, NUL bytes, parent "
            "traversal ('..'), path components longer than 63 bytes, and "
            "output overflow. Canonicalize repeated '/' and '.' components "
            "into a caller-supplied fixed byte buffer. Preserve '/' root. "
            "Return explicit errors rather than truncating, and include "
            "tests for boundaries and traversal attempts."
        ),
        "required": ("pub fn", "enum", "#[cfg(test)]"),
    },
    {
        "name": "process_events",
        "purpose": (
            "Implement a no_std, no-allocation bounded process event queue. "
            "Each event identifies a nonzero PID, event kind, and u64 value. "
            "Support FIFO enqueue/dequeue with explicit overflow and empty "
            "errors, safe ring-index wraparound, and clearing stale events "
            "for a terminated PID without changing other PIDs' ordering. "
            "Include unit tests for capacity one, wraparound, and teardown."
        ),
        "required": ("pub struct", "pub enum", "#[cfg(test)]"),
    },
)

START = "BEGIN_INDIANOS_RUST"
END = "END_INDIANOS_RUST"
FORBIDDEN = re.compile(
    r"\bunsafe\b|\bextern\b|\bstd\s*::|\bstatic\s+mut\b|"
    r"\b(?:include|include_str|include_bytes|env|option_env|global_asm|"
    r"asm|compile_error|concat_bytes)\s*!|#\s*\[\s*path\s*=",
    re.I,
)


def select_task(root: Path) -> dict[str, object] | None:
    for task in TASKS:
        if not (root / MODULE_ROOT / (str(task["name"]) + ".rs")).exists():
            return task
    return None


def prompt_for(task: dict[str, object]) -> str:
    return (
        "You are implementing one real no_std Rust kernel library module.\n"
        "Return compilable Rust 2021 source for the MODULE ONLY.\n"
        "Do not include a package manifest, another file, prose or markdown.\n"
        "The reply MUST start with " + START + " and end with " + END + ".\n"
        "Use only core, safe Rust, fixed-capacity storage, and no heap or "
        "global mutable state. You must provide meaningful unit tests under "
        "#[cfg(test)] that exercise both failure and success paths.\n"
        "No unsafe, inline assembly, FFI, external crates, environment macros, "
        "file access, privileged hardware access, or side effects.\n"
        "Task " + str(task["name"]) + ": " + str(task["purpose"]) + "\n"
        "All exported types/functions should have clear public APIs.\n"
        "Your Rust output MUST contain these literal API/test fragments: "
        + ", ".join(str(item) for item in task["required"]) + ".\n"
        + (
            "For capability, declare `pub struct Registry` with public grant, "
            "lookup, authorize, revoke and revoke_all methods. Prefer simple "
            "fixed arrays and a u64 token counter; do not use atomics, raw "
            "pointers, or undefined names. Do not mix Option and Result with ?.\n"
            if task["name"] == "capability" else ""
        )
        + "Keep the implementation concise enough to complete every test "
        "and the required END marker.\n"
        "Begin your answer now:\n"
    )


def retry_prompt_for(task: dict[str, object]) -> str:
    """Provide stricter guidance on the single permitted regeneration attempt."""
    reminder = (
        "FINAL RETRY: The preceding model output was rejected.\n"
        "Produce new, compilable safe Rust; do not repeat the failed draft.\n"
        "Required literal fragments: "
        + ", ".join(str(item) for item in task["required"]) + ".\n"
        "Check all imports and identifiers, publicly expose required types, "
        "include at least two #[test] tests, and output both delimiters.\n"
        "No markdown, explanation, or shell commands.\n"
    )
    return prompt_for(task).replace("Begin your answer now:\n", reminder + "Begin your answer now:\n")


def write_retry_prompt(name: str, output: Path) -> None:
    task = next((item for item in TASKS if item["name"] == name), None)
    if task is None:
        raise ValueError("unknown or unapproved kernel milestone")
    output.write_text(retry_prompt_for(task), encoding="utf-8")


def parse_generated(raw: str, task: dict[str, object]) -> str:
    start = raw.find(START)
    end = raw.find(END, start + len(START)) if start >= 0 else -1
    if start < 0 or end < 0 or end <= start:
        raise ValueError("missing or out-of-order model output delimiters")
    code = raw[start + len(START):end].strip()
    if code.startswith("```"):
        code = re.sub(r"^\s*```(?:rust)?\s*\n", "", code)
        code = re.sub(r"\n```\s*$", "", code)
    if not 250 <= len(code.encode("utf-8")) <= MAX_GENERATED_BYTES:
        raise ValueError("generated module size outside permitted bounds")
    if FORBIDDEN.search(code):
        raise ValueError("unsafe, external, or environment-dependent Rust rejected")
    if code.count("#[test]") < 2:
        raise ValueError("require at least two Rust regression tests")
    for required in task["required"]:
        if required not in code:
            raise ValueError("missing required task API/test: " + str(required))
    if "\x00" in code:
        raise ValueError("NUL in source")
    return "// Agent X generated, safety-gated module: " + str(task["name"]) + "\n" + code + "\n"


def prepare(root: Path, output: Path, github_output: Path | None) -> None:
    task = select_task(root)
    if task is None:
        print("No pending allowlisted development milestone.")
        if github_output:
            with github_output.open("a") as handle:
                handle.write("selected=false\n")
        return
    output.write_text(prompt_for(task), encoding="utf-8")
    if github_output:
        with github_output.open("a") as handle:
            handle.write("selected=true\nname=" + str(task["name"]) + "\n")
    print("Selected allowlisted kernel milestone:", task["name"])


def apply(root: Path, name: str, raw_path: Path) -> None:
    task = next((t for t in TASKS if t["name"] == name), None)
    if task is None:
        raise ValueError("unknown or unapproved kernel milestone")
    target = root / MODULE_ROOT / (name + ".rs")
    lib = root / LIB
    if target.exists():
        raise ValueError("refusing to replace an existing kernel file")
    code = parse_generated(raw_path.read_text(encoding="utf-8"), task)
    before = lib.read_text(encoding="utf-8")
    anchor = "pub mod ipc;"
    entry = "pub mod " + name + ";"
    if anchor not in before or entry in before:
        raise ValueError("unexpected kernel module registry state")
    # All validation happens before writing either file.
    target.write_text(code, encoding="utf-8")
    lib.write_text(before.replace(anchor, anchor + "\n" + entry, 1), encoding="utf-8")
    print("Applied validated model output to:", target.relative_to(root))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["prepare", "apply", "retry-prompt"])
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--prompt", type=Path)
    parser.add_argument("--generated", type=Path)
    parser.add_argument("--name", type=str)
    args = parser.parse_args()
    try:
        if args.command == "prepare":
            if args.prompt is None:
                parser.error("prepare requires --prompt")
            prepare(args.root, args.prompt,
                    Path(os.environ["GITHUB_OUTPUT"]) if "GITHUB_OUTPUT" in os.environ else None)
        elif args.command == "retry-prompt":
            if args.name is None or args.prompt is None:
                parser.error("retry-prompt requires --name and --prompt")
            write_retry_prompt(args.name, args.prompt)
        else:
            if args.name is None or args.generated is None:
                parser.error("apply requires --name and --generated")
            apply(args.root, args.name, args.generated)
    except (ValueError, OSError, UnicodeError) as exc:
        print("Agent X guarded code generation rejected:", exc, file=sys.stderr)
        raise SystemExit(1)


if __name__ == "__main__":
    main()
