"""Offline tests for the Agent X model-output safety boundary."""
from __future__ import annotations

import importlib.util
from pathlib import Path
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "tools" / "agent_x_autodev.py"
SPEC = importlib.util.spec_from_file_location("agent_x_autodev", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
agent = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(agent)


def valid_example() -> str:
    code = """pub struct Registry { count: usize }
impl Registry {
    pub fn new() -> Self { Self { count: 0 } }
    pub fn grant(&mut self) { self.count += 1; }
    pub fn revoke(&mut self) { self.count -= 1; }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn grant_works() { let mut r = Registry::new(); r.grant(); assert_eq!(r.count, 1); }
    #[test] fn revoke_works() { let mut r = Registry::new(); r.grant(); r.revoke(); assert_eq!(r.count, 0); }
}
"""
    return agent.START + "\n" + code + "\n" + agent.END + "\n"


class GuardedCodegenTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        source = self.root / agent.MODULE_ROOT
        source.mkdir(parents=True)
        (source / "lib.rs").write_text("pub mod ipc;\n", encoding="utf-8")
        (source / "ipc.rs").write_text("pub const IN_USE: bool = true;\n", encoding="utf-8")

    def test_selects_first_missing_allowlisted_milestone(self) -> None:
        selected = agent.select_task(self.root)
        self.assertIsNotNone(selected)
        self.assertEqual(selected["name"], "capability")
        prompt = self.root / "prompt.txt"
        agent.prepare(self.root, prompt, None)
        self.assertIn(agent.START, prompt.read_text())
        self.assertIn("capability", prompt.read_text())

    def test_prepared_candidate_is_written_to_one_known_path(self) -> None:
        candidate = self.root / "raw.txt"
        candidate.write_text(valid_example(), encoding="utf-8")
        agent.apply(self.root, "capability", candidate)
        path = self.root / agent.MODULE_ROOT / "capability.rs"
        self.assertIn("pub struct Registry", path.read_text())
        self.assertIn("pub mod capability;", (self.root / agent.LIB).read_text())
        with self.assertRaisesRegex(ValueError, "replace"):
            agent.apply(self.root, "capability", candidate)

    def test_model_can_never_select_arbitrary_file_paths(self) -> None:
        candidate = self.root / "raw.txt"
        candidate.write_text(valid_example(), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "unknown"):
            agent.apply(self.root, "../../.github/workflows/kernel-boot.yml", candidate)
        self.assertFalse((self.root / agent.MODULE_ROOT / "capability.rs").exists())

    def test_missing_markers_and_unsafe_constructs_are_rejected(self) -> None:
        task = agent.TASKS[0]
        with self.assertRaisesRegex(ValueError, "delimiters"):
            agent.parse_generated("pub struct Registry;", task)
        with self.assertRaisesRegex(ValueError, "unsafe"):
            agent.parse_generated(valid_example().replace("pub struct", "unsafe pub struct"), task)
        with self.assertRaisesRegex(ValueError, "unsafe"):
            agent.parse_generated(valid_example().replace("pub struct", "std:: pub struct"), task)

    def test_no_source_change_on_rejected_candidate(self) -> None:
        candidate = self.root / "bad.txt"
        candidate.write_text(agent.START + "\n" + "short" + "\n" + agent.END)
        original = (self.root / agent.LIB).read_text()
        with self.assertRaises(ValueError):
            agent.apply(self.root, "capability", candidate)
        self.assertEqual((self.root / agent.LIB).read_text(), original)
        self.assertFalse((self.root / agent.MODULE_ROOT / "capability.rs").exists())

    def test_empty_backlog_is_non_destructive(self) -> None:
        for task in agent.TASKS:
            (self.root / agent.MODULE_ROOT / (task["name"] + ".rs")).write_text("// existing\n")
        self.assertIsNone(agent.select_task(self.root))
        prompt = self.root / "prompt.txt"
        agent.prepare(self.root, prompt, None)
        self.assertFalse(prompt.exists())


if __name__ == "__main__":
    unittest.main()
