#!/usr/bin/env python3
"""Negative and positive tests for the exact-head, fail-closed R118 release verifier."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import unittest
from unittest import mock

MODULE = Path(__file__).with_name("r118_release_gate.py")
spec = importlib.util.spec_from_file_location("r118_release_gate", MODULE)
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)

SHA = "a" * 40


def fixture_reviews(state="APPROVED", commit=SHA, reviewer="independent"):
    return [{"user": {"login": reviewer}, "state": state, "commit_id": commit}]


def evidence(checks=None, reviews=None, protection=None, actual_head=SHA):
    checks = gate.REQUIRED if checks is None else checks
    if reviews is None:
        reviews = fixture_reviews()
    data = [
        {"head": {"sha": actual_head}, "state": "open", "merged": False,
         "user": {"login": "author"}, "base": {"ref": "main"}},
        {"check_runs": [{"name": name, "status": "completed",
                         "conclusion": "success"} for name in checks]},
        reviews,
        {"url": "https://api.github.com/branches/main/protection"}
        if protection is None else protection,
    ]
    return iter(data)


class ReleaseGateTests(unittest.TestCase):
    def invoke(self, records):
        stdout = io.StringIO()
        with mock.patch.object(gate, "get", side_effect=lambda path: next(records)), \
             mock.patch.object(sys, "argv", ["test", "--expected-head", SHA]), \
             contextlib.redirect_stdout(stdout):
            result = gate.main()
        return result, json.loads(stdout.getvalue())

    def test_same_sha_all_checks_approved_protected_is_ready(self):
        exit_code, result = self.invoke(evidence())
        self.assertEqual(exit_code, 0)
        self.assertTrue(result["ready"])

    def test_old_pr_head_denied_before_check_queries(self):
        exit_code, result = self.invoke(evidence(actual_head="b" * 40))
        self.assertEqual(exit_code, 1)
        self.assertFalse(result["ready"])

    def test_missing_required_check_fails_closed(self):
        exit_code, result = self.invoke(evidence(checks=gate.REQUIRED[:-1]))
        self.assertEqual(exit_code, 1)
        self.assertEqual(result["required_checks"][gate.REQUIRED[-1]], "not-success")

    def test_same_author_approval_not_independent(self):
        exit_code, result = self.invoke(evidence(reviews=fixture_reviews(reviewer="author")))
        self.assertEqual(exit_code, 1)
        self.assertFalse(result["independent_approved"])

    def test_old_commit_review_is_not_accepted(self):
        exit_code, result = self.invoke(evidence(reviews=fixture_reviews(commit="b" * 40)))
        self.assertEqual(exit_code, 1)
        self.assertFalse(result["independent_approved"])

    def test_active_change_request_blocks_approval(self):
        reviews = fixture_reviews() + fixture_reviews(state="CHANGES_REQUESTED",
                                                     reviewer="second-reviewer")
        exit_code, result = self.invoke(evidence(reviews=reviews))
        self.assertEqual(exit_code, 1)
        self.assertFalse(result["independent_approved"])

    def test_missing_branch_protection_denies_release(self):
        exit_code, result = self.invoke(evidence(protection={}))
        self.assertEqual(exit_code, 1)
        self.assertEqual(result["branch_protection"], "not-confirmed")


if __name__ == "__main__":
    unittest.main()
