"""The selected-input probe must reject absolute and input-dependent RSS growth."""
import importlib.util
import os
from pathlib import Path
import unittest


@unittest.skipIf(os.name == "nt", "Unix child RSS probe")
class SelectedInvestigationMemoryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location(
            "selected_memory_probe", Path(__file__).with_name("measure-selected-investigation-memory.py")
        )
        cls.probe = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.probe)

    def test_accepts_exact_limits(self):
        self.probe.check_rss([72, 96, 88], 96, 24)

    def test_rejects_absolute_peak_even_when_growth_is_flat(self):
        with self.assertRaisesRegex(AssertionError, "absolute RSS ceiling"):
            self.probe.check_rss([97, 97, 97], 96, 24)

    def test_rejects_growth_even_below_absolute_ceiling(self):
        with self.assertRaisesRegex(AssertionError, "RSS growth tolerance"):
            self.probe.check_rss([60, 85, 84], 96, 24)


if __name__ == "__main__":
    unittest.main()
