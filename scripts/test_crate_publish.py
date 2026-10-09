"""Exercise the publishing workflow's release gate without network or registry writes."""
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipIf(os.name == 'nt' or sys.version_info < (3, 11) or not shutil.which('bash'),
                 'Publishing validation runs on Ubuntu with Python 3.12 and Bash')
class CrateReleaseGateTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='crate publisher ')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.repo = self.root / 'repository'
        self.repo.mkdir()
        self.git('init', '-q')
        self.git('config', 'user.name', 'Publishing fixture')
        self.git('config', 'user.email', 'fixture@example.invalid')
        self.output = self.root / 'output'
        self.release = self.root / 'crate-release.json'
        self.release_data = self.root / 'release-data.json'
        self.release_data.write_text(json.dumps({'tagName': 'v0.3.0', 'isDraft': False, 'isPrerelease': False}))
        fake_bin = self.root / 'bin'
        fake_bin.mkdir()
        for name, script in {
            'gh': '#!/bin/sh\ncat "$TEST_RELEASE_JSON"\n',
            'python': '#!/bin/sh\nexec ' + shlex.quote(sys.executable) + ' "$@"\n',
        }.items():
            path = fake_bin / name
            path.write_text(script)
            path.chmod(0o755)
        self.env = dict(os.environ, PATH=str(fake_bin) + os.pathsep + os.environ['PATH'],
                        RELEASE_VERSION='0.3.0', GITHUB_REF='refs/heads/main',
                        GITHUB_OUTPUT=str(self.output), RUNNER_TEMP=str(self.root),
                        TEST_RELEASE_JSON=str(self.release_data))
        workflow = (ROOT / '.github/workflows/publish.yml').read_text()
        step = workflow.split('      - name: Select and validate the release\n', 1)[1]
        self.script = textwrap.dedent(step.split('        run: |\n', 1)[1].split('\n      - uses:', 1)[0])

    def git(self, *args):
        return subprocess.run(['git', *args], cwd=self.repo, check=True, capture_output=True, text=True).stdout.strip()

    def prepare_tag(self, version='0.3.0', license='MIT', description='Synthetic package', publish=None):
        package = '[package]\nname = "log-analyzer"\nversion = ' + json.dumps(version) + '\n'
        if license is not None:
            package += 'license = ' + json.dumps(license) + '\n'
        if description is not None:
            package += 'description = ' + json.dumps(description) + '\n'
        if publish is not None:
            package += 'publish = ' + json.dumps(publish) + '\n'
        (self.repo / 'Cargo.toml').write_text(package)
        self.git('add', 'Cargo.toml')
        self.git('-c', 'commit.gpgsign=false', '-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'test: release fixture')
        self.git('tag', 'v0.3.0')
        return self.git('rev-parse', 'HEAD')

    def invoke(self, **env):
        return subprocess.run(['bash', '-c', self.script], cwd=self.repo,
                              env=dict(self.env, **env), capture_output=True, text=True)

    def assert_rejected(self, result):
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse(self.output.exists())

    def test_valid_release_exports_its_exact_revision(self):
        revision = self.prepare_tag()
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.output.read_text(), f'revision={revision}\n')

    def test_invalid_version_and_non_main_dispatch_are_rejected_before_lookup(self):
        for version in ('v0.3.0', '0.3.0-rc.1', '00.3.0', '0.3.0; touch injected'):
            with self.subTest(version=version):
                self.assert_rejected(self.invoke(RELEASE_VERSION=version))
                self.assertFalse(self.release.exists())
        self.assert_rejected(self.invoke(GITHUB_REF='refs/heads/feature'))
        self.assertFalse(self.release.exists())

    def test_missing_tag_and_mismatched_manifest_version_fail(self):
        self.assert_rejected(self.invoke())
        self.prepare_tag(version='0.4.0')
        self.assert_rejected(self.invoke())

    def test_draft_prerelease_and_wrong_tag_fail(self):
        self.prepare_tag()
        for key, value in (('isDraft', True), ('isPrerelease', True), ('tagName', 'v0.4.0')):
            release = {'tagName': 'v0.3.0', 'isDraft': False, 'isPrerelease': False, key: value}
            self.release_data.write_text(json.dumps(release))
            with self.subTest(field=key):
                self.assert_rejected(self.invoke())

    def test_missing_license_fails(self):
        self.prepare_tag(license=None)
        self.assert_rejected(self.invoke())

    def test_missing_description_fails(self):
        self.prepare_tag(description=None)
        self.assert_rejected(self.invoke())

    def test_disallowed_registry_fails(self):
        self.prepare_tag(publish=['private-registry'])
        self.assert_rejected(self.invoke())

    def test_disabled_publishing_fails(self):
        self.prepare_tag(publish=False)
        self.assert_rejected(self.invoke())


if __name__ == '__main__':
    unittest.main()
