"""Hermetic update-publisher regressions (no dependencies or private key generation).

Run: python3 rune/scripts/test_sign_update.py
Optional real OpenSSL 3 check:
  PICKLE_RUN_SIGN_UPDATE_INTEGRATION=1 python3 rune/scripts/test_sign_update.py
The optional check uses only RFC 8032's publicly known first Ed25519 test key.
"""
import base64
import contextlib
import importlib.util
import io
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

SCRIPT = pathlib.Path(__file__).with_name('sign-update.py')
PUBLIC_HEX = 'd75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a'
PUBLIC_DER = bytes.fromhex('302a300506032b6570032100' + PUBLIC_HEX)
# RFC 8032 §7.1 test 1: published, nonsecret fixture, never a release credential.
FIXTURE_DER = bytes.fromhex(
    '302e020100300506032b657004220420'
    '9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60'
)
FIXTURE_PEM = (b'-----BEGIN PRIVATE KEY-----\n' + base64.b64encode(FIXTURE_DER)
               + b'\n-----END PRIVATE KEY-----\n')


class PublisherChecks(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location('pickle_sign_update_fixture', SCRIPT)
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        temporary = tempfile.TemporaryDirectory(prefix='pickle-publisher-fixture-')
        self.addCleanup(temporary.cleanup)
        self.root = pathlib.Path(temporary.name)
        self.key = self.root / 'public-test-only-key.pem'
        self.key.write_bytes(FIXTURE_PEM)
        self.installer = self.root / 'PickleRune-setup.exe'
        self.installer.write_bytes(b'public fixture installer bytes')
        self.output = self.root / 'manifest.json'
        self.output.write_bytes(b'previous manifest retained')

    def arguments(self, expected=PUBLIC_HEX, output=None):
        arguments = [str(SCRIPT), '--key', str(self.key), '--version', '1.2.3',
                     '--artifact', 'x86_64-pc-windows-msvc', str(self.installer),
                     'https://example.com/PickleRune-setup.exe',
                     '--output', str(output or self.output)]
        if expected is not None:
            arguments.extend(['--expected-public-key-hex', expected])
        return arguments

    def run_main(self, arguments=None):
        with mock.patch.object(sys, 'argv', arguments or self.arguments()), contextlib.redirect_stdout(io.StringIO()):
            self.module.main()

    def assert_sources_intact(self):
        self.assertEqual(self.key.read_bytes(), FIXTURE_PEM)
        self.assertEqual(self.installer.read_bytes(), b'public fixture installer bytes')

    def assert_bounded_noninteractive(self, call):
        self.assertEqual(call.kwargs['stdin'], subprocess.DEVNULL)
        self.assertEqual(call.kwargs['stdout'], subprocess.PIPE)
        self.assertEqual(call.kwargs['stderr'], subprocess.PIPE)
        self.assertEqual(call.kwargs['timeout'], 30)
        self.assertIs(call.kwargs['check'], False)

    def test_expected_public_key_is_required_before_any_openssl_call_or_output(self):
        with mock.patch.object(self.module.subprocess, 'run') as run, contextlib.redirect_stderr(io.StringIO()) as errors:
            with self.assertRaises(SystemExit) as raised:
                self.run_main(self.arguments(expected=None))
        self.assertEqual(raised.exception.code, 2)
        self.assertIn('--expected-public-key-hex', errors.getvalue())
        run.assert_not_called()
        self.assertEqual(self.output.read_bytes(), b'previous manifest retained')
        self.assert_sources_intact()

    def test_malformed_expected_public_key_never_reaches_openssl(self):
        for invalid in ['', 'a' * 63, 'a' * 65, 'g' * 64, '../private-key.pem']:
            with self.subTest(invalid=invalid), mock.patch.object(self.module.subprocess, 'run') as run:
                with self.assertRaisesRegex(ValueError, '64 hexadecimal'):
                    self.run_main(self.arguments(expected=invalid))
                run.assert_not_called()
                self.assertEqual(self.output.read_bytes(), b'previous manifest retained')

    def test_wrong_publisher_key_preserves_manifest_and_never_signs(self):
        with mock.patch.object(self.module.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, PUBLIC_DER, b'')) as run:
            with self.assertRaisesRegex(ValueError, 'does not match'):
                self.run_main(self.arguments(expected='00' * 32))
        self.assertEqual(run.call_count, 1)
        self.assertEqual(run.call_args.args[0][1], 'pkey')
        self.assertNotIn('-sign', run.call_args.args[0])
        self.assert_bounded_noninteractive(run.call_args)
        self.assertEqual(self.output.read_bytes(), b'previous manifest retained')
        self.assert_sources_intact()

    def test_wrong_key_does_not_create_an_output_directory(self):
        output = self.root / 'not-created' / 'manifest.json'
        with mock.patch.object(self.module.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, PUBLIC_DER, b'')):
            with self.assertRaisesRegex(ValueError, 'does not match'):
                self.run_main(self.arguments(expected='00' * 32, output=output))
        self.assertFalse(output.parent.exists())

    def test_non_ed25519_key_is_rejected_without_signing(self):
        with mock.patch.object(self.module.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'not an Ed25519 public key', b'')) as run:
            with self.assertRaisesRegex(ValueError, 'not Ed25519'):
                self.run_main()
        self.assertEqual(run.call_count, 1)
        self.assertEqual(self.output.read_bytes(), b'previous manifest retained')

    def test_identical_key_or_installer_output_is_rejected_before_signing(self):
        for source in [self.key, self.installer]:
            with self.subTest(source=source.name), mock.patch.object(self.module.subprocess, 'run') as run:
                with self.assertRaisesRegex(ValueError, 'must be separate'):
                    self.run_main(self.arguments(output=source))
                run.assert_not_called()
                self.assert_sources_intact()

    def test_hardlinked_key_or_installer_output_cannot_destroy_source_bytes(self):
        for index, source in enumerate([self.key, self.installer]):
            with self.subTest(source=source.name):
                alias = self.root / f'hardlink-{index}.json'
                os.link(source, alias)
                before = source.read_bytes()
                with mock.patch.object(self.module.subprocess, 'run') as run:
                    with self.assertRaisesRegex(ValueError, 'must be separate'):
                        self.run_main(self.arguments(output=alias))
                    run.assert_not_called()
                self.assertTrue(alias.samefile(source))
                self.assertEqual(alias.read_bytes(), before)
                self.assert_sources_intact()

    def test_key_derivation_and_signing_both_have_noninteractive_deadlines(self):
        def openssl_result(command, **kwargs):
            if command[1] == 'pkey':
                return subprocess.CompletedProcess(command, 0, PUBLIC_DER, b'')
            self.assertIn('-sign', command)
            signature = pathlib.Path(command[command.index('-out') + 1])
            signature.write_bytes(b'\xa5' * 64)
            return subprocess.CompletedProcess(command, 0, b'', b'')
        with mock.patch.object(self.module.subprocess, 'run', side_effect=openssl_result) as run:
            self.run_main(self.arguments(expected=PUBLIC_HEX.upper()))
        self.assertEqual(run.call_count, 2)
        self.assertEqual([call.args[0][1] for call in run.call_args_list], ['pkey', 'pkeyutl'])
        for call in run.call_args_list:
            self.assert_bounded_noninteractive(call)
        envelope = json.loads(self.output.read_bytes())
        payload = json.loads(bytes.fromhex(envelope['payload_hex']))
        self.assertEqual(envelope['schema'], 1)
        self.assertEqual(len(bytes.fromhex(envelope['signature_hex'])), 64)
        self.assertEqual(payload['version'], '1.2.3')
        self.assertEqual(payload['artifacts'][0]['size'], self.installer.stat().st_size)
        self.assertFalse(list(self.root.glob('.pickle-manifest-*')))
        self.assert_sources_intact()

    def test_derivation_or_signing_timeout_never_replaces_the_existing_manifest(self):
        for signing in [False, True]:
            with self.subTest(signing=signing):
                results = ([subprocess.CompletedProcess([], 0, PUBLIC_DER, b'')] if signing else [])
                results.append(subprocess.TimeoutExpired('openssl', 30))
                with mock.patch.object(self.module.subprocess, 'run', side_effect=results) as run:
                    with self.assertRaises(subprocess.TimeoutExpired):
                        self.run_main()
                self.assertEqual(run.call_count, 2 if signing else 1)
                for call in run.call_args_list:
                    self.assert_bounded_noninteractive(call)
                self.assertEqual(self.output.read_bytes(), b'previous manifest retained')
                self.assert_sources_intact()

    def assert_staging_failure_preserves_files(self, phase):
        original_temporary = self.module.tempfile.NamedTemporaryFile

        class FailingStagingFile:
            def __init__(self, **kwargs):
                self.inner = original_temporary(**kwargs)

            def __enter__(self):
                self.opened = self.inner.__enter__()
                self.name = self.opened.name
                return self

            def write(self, value):
                if phase == 'write':
                    self.opened.write(value[:17])
                    self.opened.flush()
                    raise OSError('controlled staging write failure')
                return self.opened.write(value)

            def __exit__(self, exception_type, value, traceback):
                result = self.inner.__exit__(exception_type, value, traceback)
                if phase == 'close' and exception_type is None:
                    raise OSError('controlled staging close failure')
                return result

        def openssl_result(command, **kwargs):
            if command[1] == 'pkey':
                return subprocess.CompletedProcess(command, 0, PUBLIC_DER, b'')
            pathlib.Path(command[command.index('-out') + 1]).write_bytes(b'\xa5' * 64)
            return subprocess.CompletedProcess(command, 0, b'', b'')

        with mock.patch.object(self.module.subprocess, 'run', side_effect=openssl_result) as run, \
                mock.patch.object(self.module.tempfile, 'NamedTemporaryFile', side_effect=FailingStagingFile):
            with self.assertRaisesRegex(OSError, f'controlled staging {phase} failure'):
                self.run_main()
        self.assertEqual([call.args[0][1] for call in run.call_args_list], ['pkey', 'pkeyutl'])
        self.assertEqual(self.output.read_bytes(), b'previous manifest retained')
        self.assertFalse(list(self.root.glob('.pickle-manifest-*')))
        self.assert_sources_intact()

    def test_staging_write_failure_removes_partial_temporary_and_preserves_manifest(self):
        self.assert_staging_failure_preserves_files('write')

    def test_staging_close_failure_removes_temporary_and_preserves_manifest(self):
        self.assert_staging_failure_preserves_files('close')

    @unittest.skipUnless(os.environ.get('PICKLE_RUN_SIGN_UPDATE_INTEGRATION') == '1',
                         'explicit integration requires OpenSSL 3 and only the public RFC test key')
    def test_public_rfc_fixture_signs_and_verifies_with_real_openssl3(self):
        executable = shutil.which('openssl')
        if not executable:
            self.fail('The explicitly requested integration requires OpenSSL 3')
        version = subprocess.run([executable, 'version'], stdin=subprocess.DEVNULL,
                                 capture_output=True, timeout=30, check=True)
        if not version.stdout.startswith(b'OpenSSL 3.'):
            self.fail('The explicitly requested integration requires OpenSSL 3')
        arguments = self.arguments()
        arguments.extend(['--openssl', executable])
        self.run_main(arguments)
        envelope = json.loads(self.output.read_bytes())
        self.assertEqual(envelope['schema'], 1)
        payload = bytes.fromhex(envelope['payload_hex'])
        self.assertEqual(json.loads(payload)['version'], '1.2.3')
        message = self.root / 'public-fixture-message.bin'
        message.write_bytes(self.module.DOMAIN + payload)
        signature = self.root / 'public-fixture-signature.bin'
        signature.write_bytes(bytes.fromhex(envelope['signature_hex']))
        public = self.root / 'public-verification-key.der'
        public.write_bytes(PUBLIC_DER)
        verified = subprocess.run([executable, 'pkeyutl', '-verify', '-rawin', '-pubin',
                                   '-keyform', 'DER', '-inkey', str(public), '-in', str(message),
                                   '-sigfile', str(signature)], stdin=subprocess.DEVNULL,
                                  capture_output=True, timeout=30, check=False)
        self.assertEqual(verified.returncode, 0, verified.stderr.decode(errors='replace'))
        self.assert_sources_intact()


if __name__ == '__main__':
    unittest.main()
