"""Legacy host protocol checks; no browser or application is launched."""
import json
import os
import pathlib
import queue
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
HOST = ROOT / 'scripts/browser-host.py'
PROCESS_TIMEOUT = 5


def invoke(message, environment=None):
    # Use the interpreter running this suite, including setup-python in CI.
    # /usr/bin/python3 is a separate Apple developer-tools installation whose
    # startup/provisioning is not controlled by the configured test runtime.
    return subprocess.run(
        [sys.executable, str(HOST)], input=message, capture_output=True,
        env=environment, timeout=PROCESS_TIMEOUT,
    )


def decode(result):
    if result.returncode != 0:
        raise AssertionError(f'Host exited {result.returncode}: {result.stderr!r}')
    if len(result.stdout) < 4:
        raise AssertionError(f'Host returned an incomplete frame: {result.stderr!r}')
    size = struct.unpack('=I', result.stdout[:4])[0]
    if len(result.stdout[4:]) != size:
        raise AssertionError('Host response did not match its native-messaging frame length')
    return json.loads(result.stdout[4:])


class RelayFixture:
    """Bounded one-request server; propagate failures instead of losing a thread."""
    def __init__(self):
        self.server = socket.socket()
        self.server.bind(('127.0.0.1', 0))
        self.server.listen(1)
        self.server.settimeout(0.1)
        self.port = self.server.getsockname()[1]
        self.stopped = threading.Event()
        self.result = queue.Queue(maxsize=1)
        self.worker = threading.Thread(target=self.respond, daemon=True)

    def __enter__(self):
        self.worker.start()
        return self

    def __exit__(self, *_):
        self.stopped.set()
        self.server.close()
        self.worker.join(timeout=3)
        if self.worker.is_alive():
            raise AssertionError('Relay fixture did not stop within its deadline')

    def respond(self):
        try:
            while not self.stopped.is_set():
                try:
                    client, _ = self.server.accept()
                    break
                except socket.timeout:
                    continue
                except OSError:
                    if self.stopped.is_set():
                        return  # Listener closed during teardown, not a protocol failure.
                    raise
            else:
                return
            with client:
                client.settimeout(2)
                raw = bytearray()
                while not raw.endswith(b'\n'):
                    block = client.recv(4096)
                    if not block:
                        raise AssertionError('Host closed before sending a complete relay request')
                    raw.extend(block)
                    if len(raw) > 100_000:
                        raise AssertionError('Host exceeded the relay fixture request bound')
                request = json.loads(raw)
                client.sendall(b'{"ok":"true"}')
            self.result.put(request)
        except Exception as error:
            self.result.put(error)

    def request(self):
        try:
            result = self.result.get(timeout=1)
        except queue.Empty as error:
            raise AssertionError('Host never completed its authenticated relay request') from error
        if isinstance(result, Exception):
            raise AssertionError('Relay fixture rejected the host request') from result
        return result


class HostChecks(unittest.TestCase):
    def test_native_framing_authenticated_relay_and_browser_identity(self):
        with tempfile.TemporaryDirectory() as directory, RelayFixture() as relay:
            config = pathlib.Path(directory) / 'Library/Application Support/Pickle/BrowserBridge'
            config.mkdir(parents=True)
            (config / 'connection.json').write_text(json.dumps({
                'port': relay.port, 'token': 'fixture-token',
            }))
            payload = json.dumps({'selection': 'Hello', 'reference': {'text': 'Fixture'}}).encode()
            result = invoke(struct.pack('=I', len(payload)) + payload, {
                **os.environ, 'HOME': directory,
                'PICKLE_BROWSER_BUNDLE': 'com.google.Chrome', 'PICKLE_APP': '/unused',
            })
            self.assertEqual(decode(result), {'ok': 'true'})
            received = relay.request()
            self.assertEqual(received['token'], 'fixture-token')
            self.assertEqual(received['payload']['bundleID'], 'com.google.Chrome')
            self.assertEqual(received['payload']['selection'], 'Hello')

    def test_oversized_message_is_rejected_before_relay_or_application_launch(self):
        self.assertIn('too large', decode(invoke(struct.pack('=I', 90001)))['error'])

    def test_truncated_frame_is_rejected_before_relay_or_application_launch(self):
        self.assertIn('Incomplete browser message', decode(invoke(struct.pack('=I', 20) + b'{}'))['error'])

    def test_relay_disconnect_is_a_failure_not_an_ignored_accept_error(self):
        with RelayFixture() as relay:
            with socket.create_connection(('127.0.0.1', relay.port), timeout=1):
                pass
            with self.assertRaisesRegex(AssertionError, 'rejected the host request') as raised:
                relay.request()
            self.assertIn('closed before sending', str(raised.exception.__cause__))


if __name__ == '__main__':
    unittest.main()
