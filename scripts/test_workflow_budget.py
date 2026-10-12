import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('workflow_budget', Path(__file__).resolve().parents[1] / '.agents/skills/analyze-logs/scripts/workflow_budget.py')
budget = importlib.util.module_from_spec(spec)
spec.loader.exec_module(budget)


class WorkflowBudgetTests(unittest.TestCase):
    def test_stop_requires_observed_elapsed_time_and_cannot_reset_state(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'state.json'
            budget.start(path, 180, 1000)
            self.assertFalse(budget.check(path, 1058)['exhausted'])
            self.assertEqual(budget.check(path, 1058)['remaining_seconds'], 122)
            before = path.read_bytes()
            with self.assertRaises(FileExistsError):
                budget.start(path, 180, 1058)
            self.assertEqual(path.read_bytes(), before)
            self.assertTrue(budget.check(path, 1180)['exhausted'])
            self.assertEqual(budget.check(path, 1185)['remaining_seconds'], 0)
            with self.assertRaises(ValueError):
                budget.check(path, 999)
            self.assertFalse(budget.check(path, 1058)['exhausted'])

    def test_invalid_state_is_unavailable_not_exhausted(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'state.json'
            for text in ['{}', '{"version":1,"started_monotonic":1,"seconds":true}', '{"version":1,"started_monotonic":1,"seconds":NaN}', 'x' * 4097]:
                path.write_text(text, encoding='utf-8')
                with self.assertRaises((ValueError, KeyError)):
                    budget.check(path, 10)
            path.unlink()
            with self.assertRaises(ValueError):
                budget.start(path, float('inf'), 10)
            self.assertFalse(path.exists())
