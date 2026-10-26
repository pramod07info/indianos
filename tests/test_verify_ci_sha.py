"""Regression tests for fail-closed CI commit identity checks."""
import importlib.util
from pathlib import Path
import unittest

_MODULE_PATH = Path(__file__).resolve().parents[1] / "tools" / "verify_ci_sha.py"
_SPEC = importlib.util.spec_from_file_location("verify_ci_sha", _MODULE_PATH)
assert _SPEC is not None and _SPEC.loader is not None
_module = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_module)


class VerifyCiShaTests(unittest.TestCase):
    def test_exact_sha(self):
        self.assertTrue(_module.verify_sha("a" * 40, "a" * 40))

    def test_case_insensitive_hex(self):
        self.assertTrue(_module.verify_sha("A" * 40, "a" * 40))

    def test_stale_sha_rejected(self):
        self.assertFalse(_module.verify_sha("a" * 40, "b" * 40))

    def test_short_sha_rejected(self):
        self.assertFalse(_module.verify_sha("a" * 7, "a" * 7))

    def test_invalid_and_empty_sha_rejected(self):
        for value in ("", "z" * 40, "a" * 39 + " ", "a" * 40 + "\n"):
            self.assertFalse(_module.verify_sha(value, value))


if __name__ == "__main__":
    unittest.main()
