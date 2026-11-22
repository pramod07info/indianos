"""Offline, deterministic negative and positive cases for the PR audit guard."""
import sys
from pathlib import Path
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parents[1] / "scripts"))
import m1_dedup_guard as guard

REGISTRY = {"gates": [{"id": "G1"}, {"id": "G2"}], "tasks": [{"id": "M1-P1-001"}]}
BODY = """Task ID: G1
Change class: fix-existing
Existing source inspected: rust/kernel_core/src/m1_persistent_terminal.rs
Merged PRs inspected: #618, #652, #654, #778, #790
Missing behavior: Reproduce a concrete missed receipt failure using the existing terminal entry point.
Test evidence: rust/kernel_core/src/m1_persistent_terminal.rs regression test detects the receipt drift.
Reuse decision: Extend the existing function instead of creating an alternative terminal session API.
"""


class DedupGuardTests(unittest.TestCase):
    def test_positive_m1_source_and_valid_audit(self):
        self.assertEqual(guard.evaluate("M1 G1 repair", BODY, ["rust/kernel_core/src/m1_persistent_terminal.rs"], REGISTRY), [])

    def test_m1_source_without_audit_is_rejected(self):
        errors = guard.evaluate("kernel work", "", ["rust/kernel_core/src/m1_partition.rs"], REGISTRY)
        self.assertTrue(any("Task ID" in x for x in errors))

    def test_short_placeholder_fields_are_rejected(self):
        errors = guard.evaluate("M1 G1", BODY.replace("#618, #652, #654, #778, #790", "TODO"),
                                ["rust/kernel_core/src/m1_partition.rs"], REGISTRY)
        self.assertTrue(any("Merged PRs inspected" in x for x in errors))

    def test_unknown_task_id_is_rejected(self):
        errors = guard.evaluate("M1 G1", BODY.replace("Task ID: G1", "Task ID: G99"),
                                ["rust/kernel_core/src/m1_partition.rs"], REGISTRY)
        self.assertTrue(any("not in" in x for x in errors))

    def test_infrastructure_cannot_claim_exemption_and_change_kernel(self):
        errors = guard.evaluate("M1 G1", BODY.replace("fix-existing", "ci-infrastructure"),
                                ["rust/kernel_core/src/m1_partition.rs"], REGISTRY)
        self.assertTrue(any("production" in x for x in errors))

    def test_docs_and_ci_only_changes_do_not_block(self):
        self.assertEqual(guard.evaluate("M1 G1 docs", "", ["docs/M1_GATES_6_7_ACCEPTANCE.md"], REGISTRY), [])
        self.assertEqual(guard.evaluate("M1 G2 CI", "", [".github/workflows/kernel-boot.yml"], REGISTRY), [])

    def test_non_m1_kernel_code_does_not_block(self):
        self.assertEqual(guard.evaluate("Repair SMP interrupt", "", ["rust/kernel_core/src/apic.rs"], REGISTRY), [])

    def test_generic_kernel_file_is_guarded_with_m1_context(self):
        errors = guard.evaluate("M1 G2", "", ["rust/kernel_core/src/block.rs"], REGISTRY)
        self.assertTrue(errors)

    def test_duplicate_fields_are_invalid(self):
        errors = guard.evaluate("M1 G1", BODY + "Task ID: G2\n", ["rust/kernel_core/src/m1_partition.rs"], REGISTRY)
        self.assertTrue(any("Task ID" in x for x in errors))

    def test_explicit_m1_task_id_works(self):
        self.assertEqual(guard.evaluate("M1 fix", BODY.replace("Task ID: G1", "Task ID: M1-P1-001"),
                                        ["rust/kernel_core/src/m1_persistent_terminal.rs"], REGISTRY), [])

    def test_untrusted_title_does_not_exempt_direct_m1(self):
        self.assertTrue(guard.in_scope("unrelated maintenance", ["rust/kernel_core/src/m1_partition.rs"]))

    def test_github_api_failure_does_not_produce_silent_pass(self):
        with self.assertRaises(ValueError):
            guard.fetch_pr_files("invalid", 12, "token")
        with self.assertRaises(ValueError):
            guard.fetch_pr_files("owner/repo", 12, "")


if __name__ == "__main__":
    unittest.main()
