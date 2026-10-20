"""Unit tests for the OFFLINE reference compatibility planner."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from app_resolution import plan_app  # noqa: E402


class AppResolutionTests(unittest.TestCase):
    def setUp(self):
        self.catalog = {"apps": [
            {"name": "Editor", "aliases": ["Text Editor"], "options": [
                {"route": "web", "architectures": ["x86_64"]},
                {"route": "native", "architectures": ["x86_64"]},
            ]},
            {"name": "Mac App", "options": [
                {"route": "remote_mac", "architectures": ["x86_64"]}
            ]},
            {"name": "Windows Tool", "options": [
                {"route": "windows_vm", "architectures": ["x86_64"]}
            ]},
        ]}
        self.device = {
            "cpu_arch": "x86_64",
            "authorized_mac": False,
            "routes": {"native": "planned", "web": "planned",
                       "remote_mac": "planned", "windows_vm": "planned"},
        }

    def test_unknown_app_does_not_guess_download(self):
        result = plan_app(self.catalog, self.device, "Unlisted Tool")
        self.assertEqual(result["status"], "unknown")
        self.assertFalse(result["installable"])
        self.assertEqual(result["choices"], [])

    def test_routes_are_planned_not_installed(self):
        result = plan_app(self.catalog, self.device, "Text Editor")
        self.assertEqual([x["route"] for x in result["choices"]], ["native", "web"])
        self.assertTrue(all(x["status"] == "planned" for x in result["choices"]))
        self.assertFalse(result["installable"])

    def test_mac_requires_authorized_host(self):
        result = plan_app(self.catalog, self.device, "Mac App")
        self.assertEqual(result["choices"][0]["status"], "blocked")
        self.assertIn("authorized Mac", result["choices"][0]["reason"])

    def test_cpu_architecture_mismatch_is_blocked(self):
        self.device["cpu_arch"] = "aarch64"
        result = plan_app(self.catalog, self.device, "Windows Tool")
        self.assertEqual(result["choices"][0]["status"], "blocked")

    def test_ready_runtime_is_not_verified_app(self):
        self.device["routes"]["native"] = "ready"
        result = plan_app(self.catalog, self.device, "Editor")
        self.assertEqual(result["choices"][0]["status"], "candidate")
        self.assertFalse(result["installable"])

    def test_verified_requires_explicit_test_evidence_flag(self):
        self.device["routes"]["native"] = "ready"
        self.catalog["apps"][0]["options"][1]["verified_on_indianos"] = True
        result = plan_app(self.catalog, self.device, "Editor")
        self.assertEqual(result["choices"][0]["status"], "verified")
        self.assertFalse(result["installable"])  # still never installs

    def test_invalid_runtime_state_is_rejected(self):
        self.device["routes"]["native"] = "magic"
        with self.assertRaises(ValueError):
            plan_app(self.catalog, self.device, "Editor")

    def test_no_installation_commands_returned(self):
        result = plan_app(self.catalog, self.device, "Editor")
        self.assertNotIn("command", str(result).lower())
        self.assertTrue(all(c["user_approval_required"] for c in result["choices"]))


if __name__ == "__main__":
    unittest.main()
