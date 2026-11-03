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
    return [{"user": {"login": reviewer, "type": "User"},
             "state": state, "commit_id": commit}]


def evidence(checks=None, reviews=None, protection=None, actual_head=SHA):
    checks = gate.REQUIRED if checks is None else checks
    if reviews is None:
        reviews = fixture_reviews()
    pr = {"head": {"sha": actual_head}, "state": "open", "merged": False,
          "user": {"login": "author", "type": "User"}, "base": {"ref": "main"}}
    data = [
        pr,
        {"total_count": len(checks), "check_runs": [
            {"name": name, "head_sha": SHA, "status": "completed",
             "conclusion": "success"} for name in checks]},
        reviews,
        {"url": "https://api.github.com/branches/main/protection",
         "required_status_checks": {"strict": True,
                                    "contexts": list(gate.REQUIRED)},
         "required_pull_request_reviews": {"required_approving_review_count": 1}}
        if protection is None else protection,
        {"head": {"sha": actual_head}, "state": "open"},
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

    def test_m2_and_orbit_real_guest_jobs_are_mandatory(self):
        self.assertIn("m2-native-and-guest",gate.REQUIRED)
        self.assertIn("orbit-ui-preview",gate.REQUIRED)
        for missing in ("m2-native-and-guest","orbit-ui-preview"):
            checks=[name for name in gate.REQUIRED if name!=missing]
            exit_code,result=self.invoke(evidence(checks=checks))
            self.assertEqual(exit_code,1)
            self.assertEqual(result["required_checks"][missing],"not-success")
            self.assertFalse(result["ready"])

    def test_missing_branch_protection_denies_release(self):
        exit_code, result = self.invoke(evidence(protection={}))
        self.assertEqual(exit_code, 1)
        self.assertEqual(result["branch_protection"], "not-confirmed")


    def test_untrusted_check_from_a_different_sha_fails_closed(self):
        records = list(evidence())
        records[1]["check_runs"][0]["head_sha"] = "b" * 40
        exit_code, result = self.invoke(iter(records))
        self.assertEqual(exit_code, 1)
        self.assertEqual(result["required_checks"][gate.REQUIRED[0]], "not-success")

    def test_incomplete_check_pagination_cannot_pass(self):
        records = list(evidence())
        records[1]["total_count"] += 1
        exit_code, result = self.invoke(iter(records))
        self.assertEqual(exit_code, 1)
        self.assertFalse(result["check_results_complete"])
        self.assertFalse(result["ready"])

    def test_one_failed_duplicate_check_blocks_reuse_of_an_old_success(self):
        records = list(evidence())
        original = records[1]["check_runs"][0].copy()
        original["conclusion"] = "failure"
        records[1]["check_runs"].append(original)
        records[1]["total_count"] += 1
        exit_code, result = self.invoke(iter(records))
        self.assertEqual(exit_code, 1)
        self.assertEqual(result["required_checks"][gate.REQUIRED[0]], "not-success")

    def test_protection_without_required_checks_or_reviews_is_not_enough(self):
        for protection in (
            {"url": "https://api.github.com/protected"},
            {"url": "https://api.github.com/protected",
             "required_status_checks": {"contexts": list(gate.REQUIRED)}},
            {"url": "https://api.github.com/protected",
             "required_pull_request_reviews": {},
             "required_status_checks": {"contexts": list(gate.REQUIRED[:-1])}},
        ):
            exit_code, result = self.invoke(evidence(protection=protection))
            self.assertEqual(exit_code, 1)
            self.assertEqual(result["branch_protection"], "not-confirmed")

    def test_latest_reviewer_change_request_overrides_earlier_approval(self):
        reviews = fixture_reviews() + fixture_reviews(
            state="CHANGES_REQUESTED", reviewer="independent")
        exit_code, result = self.invoke(evidence(reviews=reviews))
        self.assertEqual(exit_code, 1)
        self.assertFalse(result["independent_approved"])

    def test_pr_head_change_during_evidence_collection_fails_closed(self):
        records=list(evidence())
        records[-1]["head"]["sha"]="b"*40
        exit_code,result=self.invoke(iter(records))
        self.assertEqual(exit_code,1)
        self.assertFalse(result["head_stable"])
        self.assertFalse(result["ready"])

    def test_bot_approval_is_not_independent_human_review(self):
        records=list(evidence())
        records[2][0]["user"]["type"]="Bot"
        exit_code,result=self.invoke(iter(records))
        self.assertEqual(exit_code,1)
        self.assertFalse(result["independent_approved"])

    def test_exactly_one_hundred_reviews_requires_the_empty_next_page(self):
        records=list(evidence())
        records[2]=fixture_reviews()*100
        records.insert(3, [])
        exit_code,result=self.invoke(iter(records))
        self.assertEqual(exit_code,0)
        self.assertTrue(result["review_history_complete"])
        self.assertTrue(result["ready"])

    def test_later_review_page_change_request_overrides_first_page(self):
        records=list(evidence())
        records[2]=fixture_reviews()*100
        records.insert(3,fixture_reviews(state="CHANGES_REQUESTED"))
        exit_code,result=self.invoke(iter(records))
        self.assertEqual(exit_code,1)
        self.assertTrue(result["review_history_complete"])
        self.assertFalse(result["independent_approved"])

    def test_review_page_above_cap_or_malformed_page_denies(self):
        records=list(evidence())
        records[2]=fixture_reviews()*100
        records.insert(3,{"not":"a review list"})
        exit_code,result=self.invoke(iter(records))
        self.assertEqual(exit_code,1)
        self.assertFalse(result["review_history_complete"])

    def test_check_runs_over_100_are_collected_at_exact_head(self):
        records=list(evidence())
        required=records[1]["check_runs"]
        # One hundred successful exact-head checks plus the mandatory 9
        # on page 2; a fake count/truncated result would never pass.
        first=required + [{
            "name":"nonmandatory", "head_sha":SHA,
            "status":"completed", "conclusion":"success"
        } for _ in range(100-len(required))]
        records[1]={"total_count":101,"check_runs":first}
        records.insert(2,{"total_count":101,"check_runs":[{
            "name":"nonmandatory-2","head_sha":SHA,
            "status":"completed","conclusion":"success"
        }]})
        exit_code,result=self.invoke(iter(records))
        self.assertEqual(exit_code,0)
        self.assertTrue(result["check_results_complete"])
        self.assertTrue(result["ready"])

    def test_moving_check_count_across_pages_is_denied(self):
        records=list(evidence())
        required=records[1]["check_runs"]
        first=required + [{
            "name":"other","head_sha":SHA,
            "status":"completed","conclusion":"success"
        } for _ in range(100-len(required))]
        records[1]={"total_count":101,"check_runs":first}
        records.insert(2,{"total_count":102,"check_runs":[{
            "name":"other2","head_sha":SHA,
            "status":"completed","conclusion":"success"
        }]})
        exit_code,result=self.invoke(iter(records))
        self.assertEqual(exit_code,1)
        self.assertFalse(result["check_results_complete"])
        self.assertFalse(result["ready"])

    def test_branch_protection_requires_strict_status_and_human_count(self):
        records=list(evidence())
        records[3]["required_status_checks"]["strict"]=False
        exit_code,result=self.invoke(iter(records))
        self.assertEqual(exit_code,1)
        self.assertEqual(result["branch_protection"],"not-confirmed")
        records=list(evidence())
        records[3]["required_pull_request_reviews"]["required_approving_review_count"]=0
        exit_code,result=self.invoke(iter(records))
        self.assertEqual(exit_code,1)
        self.assertEqual(result["branch_protection"],"not-confirmed")


if __name__ == "__main__":
    unittest.main()
