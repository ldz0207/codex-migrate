"""Windows adapter integration tests using an isolated JSON-RPC subprocess.

Run: python -m unittest discover -s tests -p test_native_restore.py -v
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
POWERSHELL = shutil.which('powershell.exe')


@unittest.skipUnless(POWERSHELL, 'Windows PowerShell 5.1 is required')
class NativeRestoreTests(unittest.TestCase):
    def invoke(self, mode='normal', cwd='', unarchive=False, thread_id='11111111-1111-1111-1111-111111111111', name='Restored title', unsafe_home=False):
        with tempfile.TemporaryDirectory(prefix='native restore ') as folder:
            home = Path(folder)
            log = home / 'rpc.jsonl'
            env = os.environ | {
                'TEST_MODE': mode, 'TEST_CODEX_HOME': str(home), 'TEST_RPC_LOG': str(log),
                'TEST_THREAD_ID': thread_id, 'TEST_THREAD_NAME': name,
                'TEST_CWD': str(home) if cwd == 'valid' else cwd,
                'TEST_ALLOW_UNARCHIVE': '1' if unarchive else '0',
                'TEST_DIAGNOSTIC_PATH': str(home / 'fixture.stderr.log'),
            }
            if unsafe_home:
                env['TEST_CODEX_HOME'] = os.environ['USERPROFILE']
            proc = subprocess.run([
                POWERSHELL, '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
                '-File', str(ROOT / 'tests/native_restore_runner.ps1'),
                '-ModulePath', str(ROOT / 'scripts/CodexNativeRestore.psm1'),
                '-PythonExecutable', sys.executable,
                '-ServerScript', str(ROOT / 'tests/fake_native_app_server.py'),
            ], env=env, capture_output=True, timeout=20)
            requests = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
            diagnostics = home / 'fixture.stderr.log'
            if proc.returncode != 0 and diagnostics.exists():
                proc.stderr += diagnostics.read_bytes()
            text = proc.stdout.decode('utf-8-sig', errors='replace')
            result = json.loads(text) if proc.returncode == 0 else None
            methods = [r['method'] for r in requests]
            self.assertNotIn('turn/start', methods)
            self.assertNotIn('thread/start', methods)
            return proc, result, requests

    def test_restores_name_and_checks_history(self):
        proc, result, requests = self.invoke()
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(result['status'], 'ready')
        self.assertTrue(result['historyVerified'])
        self.assertTrue(result['nameRestored'])
        self.assertEqual(result['name'], 'Restored title')
        self.assertTrue(result['hasMore'])
        self.assertFalse(result['desktopDisplayVerified'])
        self.assertEqual(next(r for r in requests if r['method'] == 'thread/resume')['params']['excludeTurns'], True)

    def test_existing_local_name_wins(self):
        proc, result, requests = self.invoke('preserve')
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(result['name'], 'Existing local title')
        self.assertNotIn('thread/name/set', [r['method'] for r in requests])

    def test_unicode_title_survives_utf8_rpc(self):
        proc, result, _ = self.invoke(name='搬家后的科研对话')
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(result['name'], '搬家后的科研对话')

    def test_archive_is_preserved_by_default(self):
        proc, result, requests = self.invoke('archived')
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(result['status'], 'archived')
        self.assertTrue(result['archived'])
        self.assertNotIn('thread/unarchive', [r['method'] for r in requests])
        self.assertNotIn('thread/name/set', [r['method'] for r in requests])
        self.assertEqual(result['pendingName'], 'Restored title')

    def test_unarchive_requires_explicit_option(self):
        proc, result, requests = self.invoke('unarchive', unarchive=True)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(result['status'], 'ready')
        self.assertIn('thread/unarchive', [r['method'] for r in requests])

    def test_archived_history_can_be_deferred_without_unarchive(self):
        proc, result, requests = self.invoke('archived_empty')
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(result['status'], 'archived')
        self.assertFalse(result['historyVerified'])
        self.assertEqual(result['historyVerification'], 'deferredUntilExplicitReopen')
        self.assertNotIn('thread/unarchive', [r['method'] for r in requests])

    def test_user_root_is_rejected_before_start(self):
        proc, _, requests = self.invoke(unsafe_home=True)
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn(b'dedicated Codex home', proc.stderr)
        self.assertEqual(requests, [])

    def test_active_threads_are_not_resumed(self):
        proc, _, requests = self.invoke('active')
        self.assertNotEqual(proc.returncode, 0)
        self.assertNotIn('thread/resume', [r['method'] for r in requests])

    def test_wrong_thread_id_fails(self):
        proc, _, _ = self.invoke('wrong_id')
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn(b'different thread UUID', proc.stderr)

    def test_name_readback_mismatch_fails(self):
        proc, _, _ = self.invoke('bad_name')
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn(b'name readback', proc.stderr)

    def test_empty_history_is_not_reported_ready(self):
        proc, result, _ = self.invoke('empty')
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(result['status'], 'empty')
        self.assertFalse(result['historyVerified'])

    def test_selected_working_directory_is_explicit(self):
        proc, result, requests = self.invoke(cwd='valid')
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertTrue(result['cwdExists'])
        self.assertIn('cwd', next(r for r in requests if r['method'] == 'thread/resume')['params'])

    def test_missing_working_directory_fails_before_start(self):
        proc, _, requests = self.invoke(cwd='Z:\\does-not-exist\\native-restore')
        self.assertNotEqual(proc.returncode, 0)
        self.assertEqual(requests, [])

    def test_invalid_uuid_fails_before_start(self):
        proc, _, requests = self.invoke(thread_id='not-a-uuid')
        self.assertNotEqual(proc.returncode, 0)
        self.assertEqual(requests, [])

    def test_silent_backend_times_out(self):
        proc, _, _ = self.invoke('timeout')
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn(b'timeout', proc.stderr)


if __name__ == '__main__':
    unittest.main()
