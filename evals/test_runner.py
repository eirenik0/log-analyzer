import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import run


def check(id, target, value, **kwargs):
    return {"id": id, "target": target, "op": "eq", "value": value, **kwargs}


class EvaluationSafetyTests(unittest.TestCase):
    def setUp(self):
        self.case = {
            "id": "sample", "description": "sample", "tags": ["test"], "args": ["info"],
            "expect": [check("exit", "exit", 0), check("rows", "json", 2, path="/rows")],
            "known_failure": {
                "issue": "https://github.com/example/repo/issues/1", "failed_checks": ["rows"],
                "signature": [check("exit", "exit", 0), check("old", "json", 1, path="/rows")],
            },
        }

    def test_only_the_exact_known_failure_is_tolerated(self):
        self.assertEqual(run.classify(self.case, {"exit": 0, "stdout": '{"rows":1}'})[0], "XFAIL")
        self.assertEqual(run.classify(self.case, {"exit": 0, "stdout": '{"rows":0}'})[0], "FAIL")
        self.assertEqual(run.classify(self.case, {"exit": 101, "stdout": ""})[0], "FAIL")

    def test_fix_requires_removing_stale_failure_marker(self):
        status, _ = run.classify(self.case, {"exit": 0, "stdout": '{"rows":2}'})
        self.assertEqual(status, "XPASS")
        self.assertTrue(run.failed_run([{"status": status}]))
        self.assertFalse(run.failed_run([{"status": "XFAIL"}]))
        self.assertTrue(run.failed_run([{"status": "XFAIL"}], strict=True))

    def test_invalid_json_or_missing_field_does_not_pass(self):
        for text in ("", "not json", '{"different":2}'):
            self.assertEqual(run.classify(self.case, {"exit": 0, "stdout": text})[0], "FAIL")

    def test_boolean_is_not_a_numeric_count(self):
        self.assertFalse(run.check_result(check("count", "json", 1, path="/count"),
                                          {"stdout": '{"count":true}'})["passed"])

    def test_pointer_supports_arrays_and_escaped_keys(self):
        self.assertEqual(run.pointer({"a/b": [{"x~y": 7}]}, "/a~1b/0/x~0y"), 7)

    def test_scope_selection_preserves_duration_facts_under_sorting(self):
        assertion = check('duration-a', 'json', 2000, path='/operations', select={'/scope': ['a']}, item_path='/duration_ms')
        output = {'stdout': '{"operations":[{"scope":["b"],"duration_ms":3000},{"scope":["a"],"duration_ms":2000}]}'}
        self.assertTrue(run.check_result(assertion, output)['passed'])
        output['stdout'] = '{"operations":[{"scope":["b"],"duration_ms":2000}]}'
        self.assertFalse(run.check_result(assertion, output)['passed'])

    def test_invalid_manifest_is_an_error_not_an_expected_failure(self):
        bad = copy.deepcopy(self.case)
        bad["expect"][0]["op"] = "typo"
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "cases.json"
            path.write_text(json.dumps({"version": 1, "cases": [bad]}))
            with self.assertRaises(ValueError):
                run.load_cases(path)

    @patch.dict("os.environ", {"LOG_ANALYZER_FILTER": "poison", "LOG_ANALYZER_PRESET": "poison"})
    @patch("run.subprocess.run")
    def test_environment_isolation_and_literal_arguments(self, process):
        process.return_value = subprocess.CompletedProcess([], 0, '{"rows":1}', "")
        case = copy.deepcopy(self.case)
        case["args"] = ["info", "file with spaces;echo injected"]
        result = run.run_case(case, Path("/fake/binary"), 1)
        self.assertEqual(result["status"], "XFAIL")
        args, kwargs = process.call_args
        self.assertEqual(args[0][-1], "file with spaces;echo injected")
        self.assertNotIn("LOG_ANALYZER_FILTER", kwargs["env"])
        self.assertNotIn("LOG_ANALYZER_PRESET", kwargs["env"])
        self.assertEqual(kwargs["env"]["TZ"], "UTC")
        self.assertFalse(kwargs.get("shell", False))

    @patch("run.subprocess.run", side_effect=subprocess.TimeoutExpired("fake", 1))
    def test_timeout_never_becomes_expected_failure(self, process):
        self.assertEqual(run.run_case(self.case, Path("/fake/binary"), 1)["status"], "ERROR")


if __name__ == "__main__":
    unittest.main()
