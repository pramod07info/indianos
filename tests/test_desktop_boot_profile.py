"""Exercise real Make profile selection without compiling an image."""
from pathlib import Path
import subprocess
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
RULE = "profile:;@printf '%s\\n' '$(RUST_INIT_FEATURE)' '$(RUST_WORKER_FEATURE)' '$(RUST_THIRD_FEATURE)' '$(RUST_FEATURES)' '$(INITRAMFS_EXTRA_ARGS)'"


def profile(*options):
    return subprocess.run(
        ["make", "-s", "--no-print-directory", "--eval", RULE, "profile", *options],
        cwd=ROOT, text=True, capture_output=True, check=False,
    )


class DesktopBootProfile(unittest.TestCase):
    def test_desktop_edition_requires_explicit_selection_before_toolchain(self):
        result = subprocess.run(["make", "-s", "desktop-image", "DESKTOP_VOLUME_CONFIG=", "CARGO=false"],
                                cwd=ROOT, text=True, capture_output=True, check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("desktop-image requires DESKTOP_VOLUME_CONFIG", result.stderr)

    def test_desktop_edition_has_a_distinct_automatic_boot_entry(self):
        config = (ROOT / "boot/limine-desktop.conf").read_text()
        self.assertIn("/IndianOS Desktop (experimental)", config)
        self.assertIn("path: boot():/boot/indianos.elf", config)
        self.assertIn("module_string: indianos-initramfs-v1", config)
        self.assertNotIn("test-", config)

    def test_native_desktop_dependencies_do_not_activate_experimental_kernel_code(self):
        features = tomllib.loads((ROOT / "rust/kernel_core/Cargo.toml").read_text())["features"]
        pending = ["normal-desktop"]
        visited = set()
        while pending:
            feature = pending.pop()
            if feature in visited:
                continue
            visited.add(feature)
            self.assertFalse(feature.startswith("test-"), feature)
            self.assertNotEqual(feature, "orbit-ui-preview")
            self.assertIn(feature, features)
            pending.extend(features[feature])
        self.assertIn("user-runtime", visited)
        self.assertIn("kernel-main", visited)

    def test_native_desktop_packages_four_explicit_service_roles_without_test_features(self):
        result = profile("DESKTOP_SESSION=1", "DESKTOP_VOLUME_CONFIG=selected-volume.bin")
        self.assertEqual(result.returncode, 0, result.stderr)
        init, files, terminal, features, archive = result.stdout.splitlines()
        self.assertEqual((init, files, terminal), ("desktop-init", "desktop-files", "desktop-terminal"))
        self.assertIn("--features normal-desktop", features)
        self.assertNotIn("test-", features)
        for path in ("/bin/files", "/bin/terminal", "/bin/compositor", "/etc/desktop-volume"):
            self.assertIn(path, archive)
        self.assertIn("selected-volume.bin", archive)

    def test_no_selection_is_invented_when_configuration_is_absent(self):
        result = profile("DESKTOP_SESSION=1", "DESKTOP_VOLUME_CONFIG=")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("--extra4", result.stdout)
        self.assertNotIn("/etc/desktop-volume", result.stdout)

    def test_experimental_or_reference_build_cannot_impersonate_native_desktop(self):
        for option in ("R031_FILES_GUI_TEST=1", "RUST_CORE=0", "RUST_USERLAND=0", "RUST_TOOLS=0"):
            with self.subTest(option=option):
                result = profile("DESKTOP_SESSION=1", option)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("DESKTOP_SESSION", result.stderr)

    def test_console_and_existing_experimental_profiles_are_preserved(self):
        for option, expected in ((None, "normal-init"), ("R031_FILES_GUI_TEST=1", "r031-files-shell"),
                                 ("R031_SUPERVISOR_TEST=1", "r031-supervisor-shell"),
                                 ("PREEMPT_SELFTEST=1", "preempt-a")):
            with self.subTest(option=option):
                result = profile(*(tuple() if option is None else (option,)))
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.splitlines()[0], expected)
                self.assertNotIn("--features normal-desktop", result.stdout)
