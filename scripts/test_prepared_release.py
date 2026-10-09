import importlib.util
import json
import os
import shutil
import subprocess
import textwrap
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('prepared_release', Path(__file__).with_name('check-prepared-release.py'))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class PreparedReleaseTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / '.claude-plugin').mkdir()
        (self.root / '.changeset').mkdir()
        self.write('Cargo.toml', '[package]\nname = "log-analyzer"\nversion = "0.3.0"\n\n[dependencies]\nother = "1"\n')
        self.write('Cargo.lock', 'version = 4\n[[package]]\nname = "other"\nversion = "1.0.0"\n[[package]]\nname = "log-analyzer"\nversion = "0.3.0"\n')
        self.write('.claude-plugin/plugin.json', json.dumps({'name': 'log-analyzer', 'version': '0.3.0'}))
        self.write('.claude-plugin/marketplace.json', json.dumps({'plugins': [{'name': 'log-analyzer', 'version': '0.3.0'}]}))
        self.write('CHANGELOG.md', '# Changelog\n\n## Unreleased\n\n## 0.3.0 (2026-10-09)\n\n- Evidence engine.\n\n## 0.2.0 (2026-02-25)\n\n- Old.\n')

    def write(self, name, text):
        (self.root / name).write_text(text, encoding='utf-8')

    def test_synchronized_release_passes(self):
        self.assertEqual(release.validate(self.root, '0.3.0'), '0.3.0')

    def test_each_mismatched_version_fails(self):
        for name in ('Cargo.toml', 'Cargo.lock', '.claude-plugin/plugin.json', '.claude-plugin/marketplace.json'):
            path = self.root / name
            original = path.read_text(encoding='utf-8')
            with self.subTest(file=name):
                self.write(name, original.replace('0.3.0', '0.2.0'))
                with self.assertRaises(ValueError): release.validate(self.root, '0.3.0')
                self.write(name, original)

    def test_unsafe_or_nonstable_version_fails(self):
        for version in ('0.3.0; touch /tmp/nope', '0.3.0-rc.1', 'v0.3.0', '00.3.0', '0.2.0'):
            with self.subTest(version=version):
                with self.assertRaises(ValueError): release.validate(self.root, version)

    def test_wrong_missing_empty_or_duplicate_release_notes_fail(self):
        for text in ('## 0.2.0 (2026-10-09)\n- Old.', '## 0.3.0 (2026-99-09)\n- Bad date.', '## 0.3.0 (2026-10-09)\n', '## 0.3.0 (2026-10-09)\n- New.\n## 0.3.0 (2026-10-09)\n- Duplicate.'):
            with self.subTest(text=text):
                self.write('CHANGELOG.md', text)
                with self.assertRaises(ValueError): release.validate(self.root, '0.3.0')

    def test_pending_changesets_fail(self):
        self.write('.changeset/pending.md', '---\ndefault: minor\n---\nFuture feature.')
        with self.assertRaises(ValueError): release.validate(self.root, '0.3.0')


@unittest.skipIf(os.name == 'nt' or not shutil.which('bash'), 'Release preparation runs on Ubuntu with Bash')
class ReleasePreparationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        fake_bin = self.root / 'bin'
        fake_bin.mkdir()
        knope = fake_bin / 'knope'
        knope.write_text('#!/usr/bin/env bash\ncat "$KNOPE_STDOUT"\ncat "$KNOPE_STDERR" >&2\nexit 7\n', encoding='utf-8')
        knope.chmod(0o755)
        self.stdout = self.root / 'stdout'
        self.stderr = self.root / 'stderr'
        self.output = self.root / 'output'
        self.stdout.write_text('', encoding='utf-8')
        self.stderr.write_text('', encoding='utf-8')
        self.env = dict(os.environ, PATH=str(fake_bin) + os.pathsep + os.environ['PATH'],
                        KNOPE_STDOUT=str(self.stdout), KNOPE_STDERR=str(self.stderr),
                        PREPARED_VERSION='', GITHUB_OUTPUT=str(self.output))
        workflow = (Path(__file__).resolve().parents[1] / '.github/workflows/release.yml').read_text(encoding='utf-8')
        step = workflow.split('      - name: Select or prepare release\n', 1)[1]
        self.script = textwrap.dedent(step.split('        run: |\n', 1)[1].split('\n  # Run tests', 1)[0])
        self.no_release = 'Error: releases::no_release (https://knope.tech/reference/config-file/steps/prepare-release/#errors)\n'

    def run_preparation(self, dry_run='false'):
        return subprocess.run(['bash', '-c', self.script], cwd=self.root,
                              env=dict(self.env, DRY_RUN=dry_run), capture_output=True, text=True)

    def test_no_release_stops_cleanly_in_normal_and_dry_runs(self):
        self.stderr.write_text(self.no_release, encoding='utf-8')
        for dry_run in ('false', 'true'):
            with self.subTest(dry_run=dry_run):
                if self.output.exists(): self.output.unlink()
                result = self.run_preparation(dry_run)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(self.output.read_text(encoding='utf-8'), 'created=false\n')

    def test_unexpected_failure_preserves_exit_status(self):
        self.stderr.write_text('Error: malformed changeset\n', encoding='utf-8')
        self.assertEqual(self.run_preparation().returncode, 7)
        self.assertFalse(self.output.exists())

    def test_no_release_with_staged_changes_remains_failure(self):
        self.stderr.write_text(self.no_release, encoding='utf-8')
        (self.root / 'partial').write_text('partial preparation', encoding='utf-8')
        subprocess.run(['git', 'add', 'partial'], cwd=self.root, check=True)
        self.assertEqual(self.run_preparation().returncode, 7)
        self.assertFalse(self.output.exists())

    def test_stdout_release_note_cannot_mask_unexpected_failure(self):
        self.stdout.write_text(self.no_release, encoding='utf-8')
        self.stderr.write_text('Error: malformed changeset\n', encoding='utf-8')
        self.assertEqual(self.run_preparation().returncode, 7)
        self.assertFalse(self.output.exists())
