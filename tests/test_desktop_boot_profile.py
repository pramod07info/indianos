"""Exercise real Make profile selection without compiling an image."""
from pathlib import Path
import subprocess
import tempfile
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
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no disk selection is invented", result.stderr)

    def test_experimental_or_reference_build_cannot_impersonate_native_desktop(self):
        for option in ("R031_FILES_GUI_TEST=1", "RUST_CORE=0", "RUST_USERLAND=0", "RUST_TOOLS=0"):
            with self.subTest(option=option):
                result = profile("DESKTOP_SESSION=1", "DESKTOP_VOLUME_CONFIG=selected-volume.bin", option)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("DESKTOP_SESSION", result.stderr)

    def test_trusted_selection_automatically_selects_desktop(self):
        result = profile("DESKTOP_VOLUME_CONFIG=selected-volume.bin")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines()[0], "desktop-init")
        self.assertIn("--features normal-desktop", result.stdout)
        rule = "edition:;@printf '%s\\n' '$(RESOLVED_EDITION)' '$(BOOT_CONFIG)'"
        result = subprocess.run(["make", "-s", "--eval", rule, "edition",
                                 "DESKTOP_VOLUME_CONFIG=selected-volume.bin"],
                                cwd=ROOT, text=True, capture_output=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), ["desktop", "boot/limine-desktop.conf"])

    def test_explicit_console_does_not_pack_an_ignored_desktop_selection(self):
        result = profile("EDITION=console", "DESKTOP_VOLUME_CONFIG=selected-volume.bin")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines()[0], "normal-init")
        self.assertNotIn("/etc/desktop-volume", result.stdout)
        self.assertNotIn("normal-desktop", result.stdout)

    def test_invalid_or_conflicting_editions_fail_closed(self):
        for options in (("EDITION=",), ("EDITION=%",), ("EDITION=server",),
                        ("EDITION=desktop console",), ("EDITION=desktop",),
                        ("EDITION=desktop", "DESKTOP_SESSION=0"),
                        ("EDITION=console", "DESKTOP_SESSION=1"),
                        ("DESKTOP_VOLUME_CONFIG=selected-volume.bin", "DESKTOP_SESSION=0"),
                        ("EDITION=desktop", "DESKTOP_VOLUME_CONFIG=path with spaces")):
            with self.subTest(options=options):
                self.assertNotEqual(profile(*options).returncode, 0)

    def test_desktop_rejects_partial_native_runtime_rollbacks(self):
        for option in ("RUST_MAIN", "RUST_USER_RUNTIME", "RUST_CPU_ENTRY", "RUST_BOOT",
                       "RUST_BOOT_PROTOCOL", "RUST_PAGING", "RUST_PAGING_CPU",
                       "RUST_INTERRUPTS", "RUST_TIMER", "RUST_PMM", "RUST_VMM",
                       "RUST_USER_IMAGE", "RUST_RUNTIME", "RUST_BOOT_STAGING"):
            with self.subTest(option=option):
                result = profile("EDITION=desktop", "DESKTOP_VOLUME_CONFIG=selected-volume.bin",
                                 option + "=0")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(option + "=1", result.stderr)

    def test_personality_changes_invalidate_artifacts_in_a_reused_build_tree(self):
        with tempfile.TemporaryDirectory() as directory:
            build = Path(directory)
            artifact, stamp = build / "chosen", build / "boot-profile.state"
            rule = f"{artifact}: {stamp};@printf '%s\\n' '$(RESOLVED_EDITION)' >> $@"
            def choose(*options):
                result = subprocess.run(["make", "-s", "--eval", rule, str(artifact),
                                         "BUILD=" + directory, *options],
                                        cwd=ROOT, text=True, capture_output=True, check=False)
                self.assertEqual(result.returncode, 0, result.stderr)
            choose()
            first = stamp.stat().st_mtime_ns
            choose()
            self.assertEqual(stamp.stat().st_mtime_ns, first)
            self.assertEqual(artifact.read_text().splitlines(), ["console"])
            # A deliberately older artifact makes this independent of clock precision.
            import os
            os.utime(artifact, (1, 1))
            choose("DESKTOP_VOLUME_CONFIG=selected-volume.bin")
            self.assertEqual(artifact.read_text().splitlines(), ["console", "desktop"])
            self.assertIn("edition=desktop", stamp.read_text())

    def test_generic_desktop_build_validates_selection_before_native_compilation(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run(["make", "-s", "kernel", "BUILD=" + directory,
                                     "DESKTOP_VOLUME_CONFIG=missing-selection.bin", "CARGO=false"],
                                    cwd=ROOT, text=True, capture_output=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("desktop-volume-preflight", result.stderr)
            self.assertFalse((Path(directory) / "indianos.elf").exists())

    def test_console_and_existing_experimental_profiles_are_preserved(self):
        for option, expected in ((None, "normal-init"), ("R031_FILES_GUI_TEST=1", "r031-files-shell"),
                                 ("R031_SUPERVISOR_TEST=1", "r031-supervisor-shell"),
                                 ("PREEMPT_SELFTEST=1", "preempt-a")):
            with self.subTest(option=option):
                result = profile(*(tuple() if option is None else (option,)))
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.splitlines()[0], expected)
                self.assertNotIn("--features normal-desktop", result.stdout)
