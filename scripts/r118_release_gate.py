#!/usr/bin/env python3
"""Fail-closed, read-only GitHub evidence report for one exact PR head.

Examples:
  GH_TOKEN=... python3 scripts/r118_release_gate.py \
    --repo pramod07info/indianos --pr 752 --expected-head FULL_SHA

This program does not merge code, approve PRs, or certify physical hardware.\nIt cannot qualify SMP, IOMMU, firmware trust or unsafe power-cut restoration.
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
    "exact-head-evidence-unit-tests",
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
        check_payload = get(prefix + "/commits/" + args.expected_head +
                            "/check-runs?per_page=100")
        checks = check_payload["check_runs"]
        # Never silently ignore mandatory checks on a later API page.
        report["check_results_complete"] = (
            isinstance(check_payload.get("total_count", len(checks)), int)
            and check_payload.get("total_count", len(checks)) == len(checks)
        )
        for name in REQUIRED:
            candidates = [check for check in checks if check.get("name") == name]
            report["required_checks"][name] = (
                "success" if report["check_results_complete"] and candidates and all(
                    check.get("head_sha") == args.expected_head
                    and check.get("status") == "completed"
                    and check.get("conclusion") == "success"
                    for check in candidates
                ) else "not-success")
        reviews = get(prefix + "/pulls/" + str(args.pr) + "/reviews?per_page=100")
        # GitHub REST reviews are paginated. Never accept an approval when
        # a first full page may hide a newer change request on later pages.
        report["review_history_complete"] = isinstance(reviews, list) and len(reviews) < 100
        latest = {}
        author = pr["user"]["login"]
        if report["review_history_complete"]:
            for review in reviews:
                user = review.get("user") or {}
                login = user.get("login", "")
                # A bot/unknown identity is not an independent human review.
                if (not login or login == author or
                        user.get("type") != "User"):
                    continue
                if review.get("state") in ("APPROVED", "CHANGES_REQUESTED", "DISMISSED"):
                    state = review["state"]
                    latest[login] = (state if review.get("commit_id") == args.expected_head
                                     else "OUTDATED")
        report["independent_approved"] = (
            report["review_history_complete"] and
            any(v == "APPROVED" for v in latest.values()) and
            not any(v == "CHANGES_REQUESTED" for v in latest.values()))
        report["review_states"] = latest
        try:
            protected = get(prefix + "/branches/" + pr["base"]["ref"] + "/protection")
            # A branch's protection endpoint existing is insufficient: it
            # must require BOTH status checks and independent PR review.
            status = protected.get("required_status_checks") or {}
            reviews_required = protected.get("required_pull_request_reviews") or {}
            required_approval_count = reviews_required.get("required_approving_review_count", 0)
            report["required_human_approvals"] = required_approval_count
            contexts = set(status.get("contexts") or [])
            contexts.update(item.get("context") for item in status.get("checks", [])
                            if isinstance(item, dict))
            report["protected_required_checks"] = sorted(contexts)
            report["branch_protection"] = (
                "enabled" if protected.get("url") and
                status.get("strict") is True and
                isinstance(required_approval_count,int) and
                required_approval_count>=1 and
                set(REQUIRED).issubset(contexts)
                else "not-confirmed")
        except urllib.error.HTTPError:
            report["branch_protection"] = "unverifiable"
        # A PR may have advanced while network calls were in flight.
        # Never use checks/reviews from an earlier head for a later one.
        current = get(prefix + "/pulls/" + str(args.pr))
        report["head_stable"] = (
            current.get("state") == "open" and
            current.get("head", {}).get("sha") == args.expected_head)
        report["ready"] = (
            report["head_stable"] and
            report["check_results_complete"] and
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
