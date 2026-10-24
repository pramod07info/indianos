import importlib.util
import unittest

spec = importlib.util.spec_from_file_location("agent_swarm", "tools/agent_swarm.py")
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

class SwarmContractTests(unittest.TestCase):
    def test_ten_separate_runners(self):
        self.assertEqual(len(m.AGENTS), 10)
        self.assertEqual(len({v["branch"] for v in m.AGENTS.values()}), 10)
        self.assertEqual(len({v["module"] for v in m.AGENTS.values()}), 10)

    def test_prompts_have_distinct_purposes(self):
        for agent, task in m.AGENTS.items():
            p = m.prompt(agent)
            self.assertIn(task["purpose"], p)
            self.assertIn(m.START, p)
            self.assertIn("no_std", p)

    def test_unsafe_or_unmarked_model_reply_denied(self):
        bad = m.START + "\nunsafe fn evil() {}" + "\n" + m.END
        with self.assertRaises(ValueError):
            m.parse(bad, "01")
        with self.assertRaises(ValueError):
            m.parse("pub fn example() {}", "01")

    def test_valid_source_with_three_tests_accepted(self):
        code = """
#[derive(Debug, PartialEq, Eq)]
pub enum Validation { Accepted, Denied }
pub fn validate(value: usize) -> Validation {
    if value <= 5 { Validation::Accepted } else { Validation::Denied }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn accepts_zero() { assert_eq!(validate(0), Validation::Accepted); }
    #[test] fn denies_large() { assert_eq!(validate(6), Validation::Denied); }
    #[test] fn accepts_five() { assert_eq!(validate(5), Validation::Accepted); }
}
"""
        parsed = m.parse(m.START + code + m.END, "01")
        self.assertIn("pub fn validate", parsed)

    def test_preapproved_slots_are_ten(self):
        self.assertEqual(set(m.AGENTS), {f"{i:02d}" for i in range(1, 11)})

class SwarmRepairTests(unittest.TestCase):
    def test_retry_includes_bounded_diagnostics(self):
        bad = "error: wrong type\nFAILED unexpected assertion\n" + ("noise\n" * 2000)
        txt = m.prompt("01", retry=True, feedback=bad)
        self.assertIn("error: wrong type", txt)
        self.assertIn("UNTRUSTED", txt)
        self.assertLess(len(m.summarize_feedback(bad)), 2600)

    def test_parser_rejects_wrong_marker_or_privileged_module(self):
        good = m.START + "\n" + (
            "pub fn value()->u8{0}\n#[cfg(test)] mod tests {\n" +
            "#[test] fn a() {}\n#[test] fn b() {}\n#[test] fn c() {}\n"
            "}"
        ) + "\n" + m.END
        with self.assertRaises(ValueError):
            m.parse(good.replace("pub fn", "unsafe pub fn"), "02")
        with self.assertRaises(ValueError):
            m.parse(good.replace(m.END, ""), "02")


if __name__ == "__main__":
    unittest.main()
