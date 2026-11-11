"""Mandatory QEMU guest dependency provisioning without Ubuntu apt DNS."""
from pathlib import Path
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
INSTALLER = ROOT / "tools/ci/install-qemu-deps.sh"
NIX_ACTION = "cachix/install-nix-action@75c75c31ebece974e48ebc36b4ad125bd97bd97b"
NIXPKGS_SHA = "ac588ef1c914865dad517b337ea0eb9694a4021c"
WORKFLOWS = (
    "r0314-directory-tree.yml",
    "r0315-durable-range-io.yml",
    "r0316-8-filesystem.yml",
    "r0319-files-gui.yml",
    "desktop-screenshot-history.yml",
    "m1-persistent-desktop.yml",
    "m1-reliability-integration.yml",
    "kernel-boot.yml",
    "verify-r118-ap-ack.yml",
    "m2-developer-alpha.yml",
)
FILE_SYSTEM_PR_JOBS = (
    "r0314-directory-tree.yml",
    "r0315-durable-range-io.yml",
    "r0316-8-filesystem.yml",
    "r0319-files-gui.yml",
)


class QemuDependencyCiTests(unittest.TestCase):
    def test_installer_shell_syntax(self):
        subprocess.run(["bash", "-n", str(INSTALLER)], check=True)

    def test_non_ubuntu_cache_installer_fails_closed(self):
        src = INSTALLER.read_text(encoding="utf-8")
        self.assertIn("set -euo pipefail", src)
        self.assertIn(NIXPKGS_SHA, src)
        for pkg in ("qemu", "xorriso", "OVMF.fd"):
            self.assertIn("nix_package " + pkg, src)
        for expected in ("nix build --no-link --print-out-paths",
                         "qemu-system-x86_64", "OVMF_CODE.fd",
                         "clang", "ld.lld", "exit 1", "qemu-system-x86_64 --version"):
            self.assertIn(expected, src)
        # Multi-output Nix derivations must be inspected independently.
        self.assertIn("nix_output_binary", src)
        self.assertIn("nix_output_firmware", src)
        self.assertIn("while IFS= read -r root", src)
        self.assertIn("nix_package \"$llvm_attr\"", src)
        self.assertIn("for llvm_tool in clang ld.lld", src)
        self.assertNotIn("apt-get", src)
        self.assertNotIn("|| true", src)
        self.assertNotIn("setup-qemu-action", src)  # user-mode isn't a VM

    def test_every_qemu_job_installs_nix_before_guest_tools(self):
        for name in WORKFLOWS:
            with self.subTest(workflow=name):
                src = (ROOT / ".github/workflows" / name).read_text(encoding="utf-8")
                self.assertIn("runs-on: ubuntu-24.04", src)
                self.assertIn("bash tools/ci/install-qemu-deps.sh", src)
                self.assertNotIn("sudo apt-get update", src)
                self.assertEqual(src.count(NIX_ACTION),
                                 src.count("bash tools/ci/install-qemu-deps.sh"))
                self.assertLess(src.index(NIX_ACTION),
                                src.index("bash tools/ci/install-qemu-deps.sh"))

    def test_filesystem_guest_gates_remain_mandatory_on_pr(self):
        for name in FILE_SYSTEM_PR_JOBS:
            with self.subTest(workflow=name):
                src = (ROOT / ".github/workflows" / name).read_text(encoding="utf-8")
                self.assertIn("  pull_request:\n    branches: [main]", src)
                self.assertIn("tools/ci/install-qemu-deps.sh", src)
                self.assertIn("qemu", src.lower())
        # Screenshot archiver writes back to main; untrusted PRs cannot run it.
        history = (ROOT / ".github/workflows/desktop-screenshot-history.yml").read_text()
        self.assertNotIn("  pull_request:", history)


if __name__ == "__main__":
    unittest.main()
