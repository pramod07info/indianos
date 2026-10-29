"""Guard the kernel-main SHA contract across screenshot archive commits.

The screenshot bot updates main after the source PR merge. Its new commit must
trigger full kernel CI or the independent reviewer/coordinator cannot approve
the next integration batch: both require an exact latest-main green run.
This test also prevents a recursive screenshot archiver CI loop.
"""
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]


def workflow(name):
    return (ROOT / ".github" / "workflows" / name).read_text("utf-8")


def section(document, start, next_start):
    return document.split(start, 1)[1].split(next_start, 1)[0]


class ScreenshotMainShaGateTests(unittest.TestCase):
    def test_screenshot_archive_push_triggers_full_kernel_boot(self):
        kernel = workflow("kernel-boot.yml")
        push = section(kernel, "  push:", "  pull_request:")
        self.assertIn('branches: [main]', push)
        self.assertIn('"development/screenshots/**"', push)
        self.assertIn('"tools/test_*.py"', push)
        self.assertIn('"rust/**"', push)
        self.assertIn("  workflow_dispatch:", kernel)

    def test_screenshot_bot_cannot_trigger_itself_on_archival_commit(self):
        history = workflow("desktop-screenshot-history.yml")
        push = section(history, "  push:", "  workflow_dispatch:")
        self.assertIn("branches:", push)
        self.assertNotIn("development/screenshots/**", push)
        self.assertIn("git add development/screenshots", history)
        self.assertIn("git push origin HEAD:main", history)
        # GitHub suppresses on.push workflows for GITHUB_TOKEN commits.
        # Explicit dispatch is necessary; the path filter alone cannot
        # satisfy the coordinator's exact-main-SHA CI requirement.
        self.assertIn("actions: write", history)
        self.assertIn("gh workflow run kernel-boot.yml --ref main", history)
        self.assertIn("GH_TOKEN:", history)

    def test_main_shas_remain_required_by_both_automated_reviewers(self):
        reviewer = workflow("agent-independent-reviewer.yml")
        coordinator = workflow("agent-coordinator.yml")
        self.assertIn("Current main full QEMU not green", reviewer)
        self.assertIn("Current main has no verified full-kernel green run", coordinator)
        self.assertIn("head_sha: mainSHA", coordinator)
        self.assertIn("head_sha:sha", reviewer)

    def test_kernel_screenshot_gate_is_a_real_job_not_noop(self):
        kernel = workflow("kernel-boot.yml")
        self.assertIn("Verify exact-main screenshot SHA gate cannot be orphaned", kernel)
        self.assertIn("test_ci_screenshot_sha_gate.py", kernel)
        self.assertIn("Build ELF + ISO and verify serial boot", kernel)
        self.assertIn("qemu-system-x86", kernel)


if __name__ == "__main__":
    unittest.main()
