#!/usr/bin/env python3
"""Offline source-install orchestration; build commands are isolated fixtures."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parent


class SourceInstallTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="zkapi-source-install-test-")
        self.root = Path(self.temporary.name)
        self.client = self.root / "checkout with spaces/zkapi-clientd"
        scripts = self.client / "scripts"
        scripts.mkdir(parents=True)
        shutil.copyfile(SCRIPTS / "install-source.sh", scripts / "install-source.sh")
        self.script = scripts / "install-source.sh"
        self.log = self.root / "calls.jsonl"
        self.tmp = self.root / "builds"
        self.tmp.mkdir()
        self.fakebin = self.root / "bin"
        self.fakebin.mkdir()
        for name, body in {
            "go": 'case "$*" in "env GOOS") echo linux;; "env GOARCH") echo amd64;; *) exit 7;; esac\n',
            "uname": 'echo Linux\n',
            "id": 'echo 1000\n',
            **{name: 'exit 0\n' for name in ("cargo", "rustc", "cmake", "pkg-config")},
        }.items():
            path = self.fakebin / name
            path.write_text('#!/bin/sh\n' + body)
            path.chmod(0o755)
        for path, kind in ((scripts / "prepare-zkapi.sh", "prepare"),
                           (scripts / "build-native.sh", "build"),
                           (self.client / "install.sh", "install")):
            path.write_text('''#!/bin/bash
set -eu
python3 - "$@" <<'PYCODE'
import json, os, sys
from pathlib import Path
kind = KIND
arguments = sys.argv[1:]
with open(os.environ['TEST_CALLS'], 'a') as out:
    out.write(json.dumps([kind, arguments]) + '\\n')
if os.environ.get('TEST_FAIL') == kind:
    sys.exit(23)
if kind == 'prepare':
    Path(arguments[0]).mkdir()
elif kind == 'build':
    assert arguments[3] == '--archive-only'
    assert Path(arguments[2]).is_dir()
    output = Path(arguments[1])
    output.mkdir()
    (output / ('zkapi-clientd_' + arguments[0] + '_linux_amd64.tar.gz')).write_bytes(b'complete fixture bundle')
elif kind == 'install':
    assert Path(arguments[arguments.index('--archive') + 1]).read_bytes() == b'complete fixture bundle'
PYCODE
'''.replace('KIND', repr(kind)))
        self.env = dict(os.environ, PATH=str(self.fakebin) + os.pathsep + os.environ['PATH'],
                        TMPDIR=str(self.tmp), TEST_CALLS=str(self.log))

    def tearDown(self):
        self.assertEqual(list(self.tmp.iterdir()), [], 'Source install left its build directory behind')
        self.temporary.cleanup()

    def run_install(self, *arguments):
        # Exercise macOS's shipped Bash 3.2 even when Homebrew Bash is on PATH.
        return subprocess.run(['/bin/bash', str(self.script), *arguments], env=self.env,
                              text=True, capture_output=True, timeout=30)

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def test_builds_complete_bundle_and_hands_off_explicit_digest(self):
        prefix = str(self.root / 'prefix with spaces')
        result = self.run_install('--prefix', prefix, '--setup', '--network', 'sepolia')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        calls = self.calls()
        self.assertEqual([call[0] for call in calls], ['prepare', 'build', 'install'])
        self.assertEqual(calls[1][1][0], '0.0.0')
        install = calls[2][1]
        self.assertEqual(install[:2], ['--version', '0.0.0'])
        self.assertEqual(install[install.index('--sha256') + 1],
                         hashlib.sha256(b'complete fixture bundle').hexdigest())
        self.assertEqual(install[-5:], ['--prefix', prefix, '--setup', '--network', 'sepolia'])

    def test_selected_version_and_no_implicit_setup(self):
        result = self.run_install('--version', '1.2.3')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        calls = self.calls()
        self.assertEqual([call[0] for call in calls], ['prepare', 'build', 'install'])
        self.assertNotIn('unbound variable', result.stderr)
        self.assertEqual(calls[1][1][0], '1.2.3')
        self.assertNotIn('--setup', calls[-1][1])

    def test_default_no_arguments_completes_installation(self):
        result = self.run_install()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn('unbound variable', result.stderr)
        self.assertEqual([call[0] for call in self.calls()], ['prepare', 'build', 'install'])
        self.assertEqual(self.calls()[-1][1][:2], ['--version', '0.0.0'])

    def test_failures_stop_before_next_stage_and_clean_up(self):
        for index, kind in enumerate(('prepare', 'build', 'install')):
            with self.subTest(stage=kind):
                self.log.unlink(missing_ok=True)
                self.env['TEST_FAIL'] = kind
                result = self.run_install()
                self.assertEqual(result.returncode, 23, result.stdout + result.stderr)
                self.assertEqual([call[0] for call in self.calls()], ['prepare', 'build', 'install'][:index + 1])
                self.assertEqual(list(self.tmp.iterdir()), [])

    def test_invalid_options_fail_before_building(self):
        for arguments in (('--version', 'dev'), ('--network', 'sepolia'),
                          ('--prefix', 'relative'), ('--setup', '--network', 'invalid'), ('--unknown',)):
            with self.subTest(arguments=arguments):
                result = self.run_install(*arguments)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.calls(), [])


if __name__ == '__main__':
    unittest.main(verbosity=2)
