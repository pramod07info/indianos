#!/usr/bin/env python3
"""Fail-closed, read-only GitHub evidence report for one exact PR head.

Examples:
  GH_TOKEN=... python3 scripts/r118_release_gate.py \
    --repo pramod07info/indianos --pr 752 --expected-head FULL_SHA

This program does not merge code, approve PRs, or certify physical hardware.
It never reports OK if a required check has not concluded successfully.
"""
import argparse
import json
import os
import sys
import urllib.error
import urllib.request

REQUIRED = (
    "verify-sha",
    "network",
    "qemu-epoch",
    "runtime-qemu",
    "combined-regression",
    "durable-files-desktop",
    "Adversarial FS, signatures and no_std security regression",
    "Build ELF + ISO and verify serial boot",
)
BASE = "https://api.github.com"


def get(path):
    token = os.environ.get("GH_TOKEN", "").strip()
    headers = {"Accept": "application/vnd.github+json", "User-Agent": "IndianOS-R118-gate",
               "X-GitHub-Api-Version": "2022-11-28"}
    if token:
        headers["Authorization"] = "Bearer " + token
    request = urllib.request.Request(BASE + path, headers=headers)
    with urllib.request.urlopen(request, timeout=15) as response:
        return json.load(response)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="pramod07info/indianos")
    parser.add_argument("--pr", type=int, default=752)
    parser.add_argument("--expected-head", required=True, help="full 40-character commit SHA")
    args = parser.parse_args()
    if (len(args.expected_head) != 40 or
            any(ch not in "0123456789abcdef" for ch in args.expected_head) or
            len(args.repo.split("/")) != 2 or not all(args.repo.split("/")) or
            args.pr < 1):
        parser.error("expected full lowercase SHA, owner/repo and positive PR")
    prefix = "/repos/" + args.repo
    report = {"repository": args.repo, "pr": args.pr,
              "expected_head": args.expected_head, "ready": False,
              "required_checks": {}, "independent_approved": False,
              "branch_protection": "unknown", "hardware": "not-qualified"}
    try:
        pr = get(prefix + "/pulls/" + str(args.pr))
        report["actual_head"] = pr["head"]["sha"]
        report["state"] = pr["state"]
        report["merged"] = pr["merged"]
        if pr["head"]["sha"] != args.expected_head or pr["state"] != "open":
            report["reason"] = "PR head mismatch or PR is not open"
            print(json.dumps(report, indent=2))
            return 1
        checks = get(prefix + "/commits/" + args.expected_head +
                     "/check-runs?per_page=100")["check_runs"]
        for name in REQUIRED:
            candidates = [check for check in checks if check["name"] == name]
            report["required_checks"][name] = (
                "success" if candidates and all(
                    c["status"] == "completed" and c["conclusion"] == "success"
                    for c in candidates) else "not-success")
        reviews = get(prefix + "/pulls/" + str(args.pr) + "/reviews?per_page=100")
        latest = {}
        author = pr["user"]["login"]
        for review in reviews:
            login = review["user"]["login"]
            if login != author and review["state"] in ("APPROVED", "CHANGES_REQUESTED",
                                                       "DISMISSED"):
                latest[login] = review["state"]
        report["independent_approved"] = any(v == "APPROVED" for v in latest.values())
        try:
            protected = get(prefix + "/branches/" + pr["base"]["ref"] + "/protection")
            report["branch_protection"] = (
                "enabled" if protected.get("url") else "not-confirmed")
        except urllib.error.HTTPError:
            report["branch_protection"] = "unverifiable"
        report["ready"] = (
            all(value == "success" for value in report["required_checks"].values())
            and report["independent_approved"]
            and report["branch_protection"] == "enabled")
        print(json.dumps(report, indent=2))
        return 0 if report["ready"] else 1
    except (urllib.error.URLError, ValueError, KeyError, TypeError) as error:
        report["reason"] = "Unable to verify required GitHub evidence: " + str(error)
        print(json.dumps(report, indent=2))
        return 1


if __name__ == "__main__":
    sys.exit(main())
