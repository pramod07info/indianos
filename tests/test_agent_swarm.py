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

if __name__ == "__main__":
    unittest.main()
