import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from check_commits import valid_header

SCRIPT = Path(__file__).with_name("check_commits.py")


class CommitHeaderTests(unittest.TestCase):
    def test_valid_headers(self):
        for message in (
            "fix(parser): preserve offsets",
            "feat!: change schema\n\nBREAKING CHANGE: migrate reports",
            "docs: explain usage",
            "revert: undo parser change",
            "fix: handle 日本語",
        ):
            with self.subTest(message=message):
                self.assertTrue(valid_header(message))

    def test_rejects_invalid_headers(self):
        for message in (
            "", "\nfix: hidden header", "Fix: title", "fix:no space",
            "fix: ", "fix:  title", "fix(): title", "fix(bad scope): title",
            "update: parser", "Merge branch main", "fix: \t",
        ):
            with self.subTest(message=message):
                self.assertFalse(valid_header(message))


class CommitCommandTests(unittest.TestCase):
    def test_message_file_exit_status(self):
        with tempfile.TemporaryDirectory() as directory:
            message = Path(directory) / "message"
            for text, expected in (("fix: valid", 0), ("bad title", 1)):
                message.write_text(text, encoding="utf-8")
                result = subprocess.run(
                    [sys.executable, str(SCRIPT), "--message-file", str(message)],
                    capture_output=True,
                )
                self.assertEqual(result.returncode, expected, result.stdout)

    def test_pr_range_checks_all_new_commits_not_old_history(self):
        with tempfile.TemporaryDirectory() as directory:
            def git(*args):
                return subprocess.check_output(
                    ["git", "-c", "commit.gpgSign=false", *args], cwd=directory, text=True,
                    stderr=subprocess.DEVNULL,
                ).strip()

            git("init")
            git("config", "user.name", "Test")
            git("config", "user.email", "test@example.invalid")
            git("-c", "core.hooksPath=/dev/null", "commit", "--allow-empty", "-m", "Old history")
            base = git("rev-parse", "HEAD")
            git("-c", "core.hooksPath=/dev/null", "commit", "--allow-empty", "-m", "fix: valid change")
            good = git("rev-parse", "HEAD")
            git("-c", "core.hooksPath=/dev/null", "commit", "--allow-empty", "-m", "Invalid new commit")
            bad = git("rev-parse", "HEAD")
            git("-c", "core.hooksPath=/dev/null", "commit", "--allow-empty", "-m", "docs: valid last commit")
            last = git("rev-parse", "HEAD")
            event = Path(directory) / "event.json"
            for title, head, expected in (
                ("fix: valid title", good, 0),
                ("Invalid title", good, 1),
                ("fix: valid title", bad, 1),
                ("fix: valid title", last, 1),
            ):
                event.write_text(json.dumps({"pull_request": {
                    "title": title, "base": {"sha": base}, "head": {"sha": head},
                }}), encoding="utf-8")
                result = subprocess.run(
                    [sys.executable, str(SCRIPT), "--event-file", str(event)],
                    cwd=directory, capture_output=True,
                )
                self.assertEqual(result.returncode, expected, result.stdout)


if __name__ == "__main__":
    unittest.main()
