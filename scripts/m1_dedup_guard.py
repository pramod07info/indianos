#!/usr/bin/env python3
"""Fail closed on missing M1 existing-code audit evidence for feature PRs.

Runs only from the trusted base branch on pull_request_target, never from a
pull-request head with its write-capable token. Does not claim to detect
semantic duplicates: a reviewer must decide whether reuse is sufficient.
"""

import json
import os
from pathlib import Path
import re
import sys
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


DIRECT_M1 = re.compile(r"^rust/kernel_core/src/m1_[a-z0-9_]+\.rs$")
SHARED_M1 = frozenset({
    "rust/kernel_core/src/block.rs",
    "rust/kernel_core/src/durablefs.rs",
    "rust/kernel_core/src/hier_vfs.rs",
    "rust/kernel_core/src/ata_pio.rs",
    "rust/kernel_core/src/ps2_mouse.rs",
    "rust/kernel_core/src/batch5_session.rs",
})
CONTEXT = re.compile(r"(?<![a-zA-Z0-9])(?:M1|G[1-8])(?![a-zA-Z0-9])", re.I)
SOURCE_PREFIXES = ("rust/kernel_core/src/", "kernel/", "userland/", "boot/", "tools/rust/src/")
FIELD_NAMES = (
    "Task ID", "Change class", "Existing source inspected",
    "Merged PRs inspected", "Missing behavior", "Test evidence", "Reuse decision",
)
EMPTY = re.compile(r"^(?:n/?a|none|not applicable|tbd|todo|pending|unknown|[.\-\s]*)$", re.I)
REVIEW_CLASSES = frozenset({"missing-code", "fix-existing"})
PATH_RE = re.compile(r"\b(?:rust|kernel|userland|boot|tools)/[a-zA-Z0-9_./-]+\.[a-zA-Z0-9]+\b")
PR_RE = re.compile(r"(?<![a-zA-Z0-9])#([1-9]\d*)\b")


def in_scope(title: str, changed_paths: list[str]) -> bool:
    """Only M1 kernel feature work; docs/tests/workflow-only PRs are exempt."""
    if any(DIRECT_M1.fullmatch(path) for path in changed_paths):
        return True
    has_m1_context = bool(CONTEXT.search(title))
    return has_m1_context and any(
        path in SHARED_M1 or path.startswith(SOURCE_PREFIXES)
        for path in changed_paths
    )


def fields_from_body(body: str) -> dict[str, str]:
    """Parse exact one-line template keys; reject duplicates and empty text."""
    result: dict[str, str] = {}
    for line in body.splitlines():
        match = re.match(r"^\s*(?:[-*]\s*)?(?:\*\*)?([A-Za-z ]+):(?:\*\*)?\s*(.*?)\s*$", line)
        if match and match.group(1) in FIELD_NAMES:
            key = match.group(1)
            result[key] = "" if key in result else match.group(2).strip()
    return result


def evaluate(title: str, body: str, changed_paths: list[str], registry: dict) -> list[str]:
    if not in_scope(title, changed_paths):
        return []
    problems = []
    fields = fields_from_body(body)
    for name in FIELD_NAMES:
        value = fields.get(name, "").strip()
        if not value or EMPTY.fullmatch(value):
            problems.append(f"Missing/placeholder PR audit field: {name}")
    if problems:
        return problems

    allowed = {gate["id"] for gate in registry.get("gates", [])}
    allowed.update(task["id"] for task in registry.get("tasks", []))
    task = fields["Task ID"]
    if task not in allowed:
        problems.append(f"Task ID {task!r} is not in docs/M1_TASK_REGISTRY.json")
    if fields["Change class"].lower() not in REVIEW_CLASSES:
        problems.append("M1 production changes must be 'missing-code' or 'fix-existing'; validation-only/infra-only must not change production files")
    sources = PATH_RE.findall(fields["Existing source inspected"])
    if not sources or not any(p.startswith(SOURCE_PREFIXES) for p in sources):
        problems.append("Existing source inspected must cite actual existing production source path(s)")
    refs = PR_RE.findall(fields["Merged PRs inspected"])
    if not refs:
        problems.append("Merged PRs inspected must cite merged pull request IDs (e.g. #790)")
    for name in ("Missing behavior", "Test evidence", "Reuse decision"):
        value = fields[name]
        if len(value) < 25:
            problems.append(f"{name} must provide concrete evidence, not a short assertion")
    evidence = fields["Test evidence"].lower()
    if not any(word in evidence for word in ("test", "qemu", "failure", "repro", "assert", "ci run")):
        problems.append("Test evidence must name a test, failing reproduction, or CI run")
    return problems


def fetch_pr_files(repository: str, number: int, token: str) -> list[str]:
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("Invalid GitHub repository identifier")
    if number < 1 or not token:
        raise ValueError("Missing pull request number or GitHub token")
    paths = []
    for page in range(1, 31):
        url = (f"https://api.github.com/repos/{repository}/pulls/{number}/files"
               f"?per_page=100&page={page}")
        req = Request(url, headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "indianos-m1-dedup-guard",
        })
        with urlopen(req, timeout=20) as response:
            page_files = json.load(response)
        if not isinstance(page_files, list):
            raise ValueError("Unexpected GitHub pull request files response")
        paths.extend(item["filename"] for item in page_files)
        if len(page_files) < 100:
            return paths
    raise ValueError("PR exceeds 3000 files; manual review required")


def main() -> int:
    try:
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
        pr = event["pull_request"]
        repository = os.environ["GITHUB_REPOSITORY"]
        token = os.environ["GITHUB_TOKEN"]
        registry = json.loads(Path("docs/M1_TASK_REGISTRY.json").read_text())
        paths = fetch_pr_files(repository, int(pr["number"]), token)
        problems = evaluate(pr.get("title") or "", pr.get("body") or "", paths, registry)
    except (KeyError, ValueError, OSError, HTTPError, URLError, json.JSONDecodeError) as error:
        print(f"::error::M1 dedup guard could not verify PR evidence: {error}")
        return 1
    if problems:
        for problem in problems:
            print(f"::error::{problem}")
        print("See docs/M1_DUPLICATE_PREVENTION.md and the PR template.")
        return 1
    print("M1 duplicate-prevention audit metadata: PASS (semantic reviewer still required)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
