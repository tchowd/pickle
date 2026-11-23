"""Actual owned-loopback tests; never opens a browser or an upstream socket."""
import importlib.util
import json
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest

SCRIPT = Path(__file__).with_name("windows-extension-page-fixture.py")
spec = importlib.util.spec_from_file_location("extension_page", SCRIPT)
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)


class ProxyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.state = Path(self.temp.name) / "state.json"
        self.server = fixture.Server(self.state)
        self.worker = threading.Thread(target=self.server.serve_forever, kwargs={"poll_interval": 0.02})
        self.worker.start()

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.worker.join(2)
        self.assertFalse(self.worker.is_alive())
        self.temp.cleanup()

    def request(self, raw):
        with socket.create_connection(self.server.server_address, timeout=2) as peer:
            peer.settimeout(2)
            peer.sendall(raw)
            result = bytearray()
            while True:
                try:
                    chunk = peer.recv(4096)
                except ConnectionResetError:
                    break
                if not chunk:
                    break
                result.extend(chunk)
                self.assertLess(len(result), 8192)
        return bytes(result)

    def test_normal_domain_fixed_page_has_no_subresources_or_forwarding(self):
        response = self.request(b"GET http://native-extension.test/owned-extension-article HTTP/1.1\r\nHost: native-extension.test\r\n\r\n")
        self.assertTrue(response.startswith(b"HTTP/1.1 200 OK\r\n"))
        self.assertIn(fixture.PAGE, response)
        self.assertIn(b"default-src 'none'", response)
        self.assertIn("Café".encode(), response)
        self.assertNotIn(b"<script", response)
        self.assertEqual(json.loads(self.state.read_text()), dict(total_requests=1, pages_served=1, rejected_requests=0, forwarded_requests=0))

    def test_external_connect_wrong_host_auth_and_body_never_forward(self):
        cases = [
            b"CONNECT external.invalid:443 HTTP/1.1\r\nHost: external.invalid\r\n\r\n",
            b"GET http://external.invalid/private HTTP/1.1\r\nHost: external.invalid\r\n\r\n",
            b"GET /owned-extension-article HTTP/1.1\r\nHost: other.invalid\r\n\r\n",
            b"GET /owned-extension-article HTTP/1.1\r\nHost: native-extension.test\r\nAuthorization: unexpected-private-value\r\n\r\n",
            b"GET /owned-extension-article HTTP/1.1\r\nHost: native-extension.test\r\nProxy-Authorization: unexpected-private-value\r\n\r\n",
            b"POST /owned-extension-article HTTP/1.1\r\nHost: native-extension.test\r\nContent-Length: 3\r\n\r\nabc",
            b"GET /owned-extension-article HTTP/1.1\r\nHost: native-extension.test\r\nHost: native-extension.test\r\n\r\n",
        ]
        for raw in cases:
            response = self.request(raw)
            self.assertTrue(response.startswith(b"HTTP/1.1 403 Forbidden\r\n"))
            self.assertNotIn(b"unexpected-private-value", response)
            self.assertNotIn(b"external.invalid", response)
        state = self.state.read_text()
        self.assertNotIn("private", state)
        self.assertEqual(json.loads(state), dict(total_requests=len(cases), pages_served=0, rejected_requests=len(cases), forwarded_requests=0))

    def test_oversized_and_incomplete_headers_are_bounded_refusals(self):
        oversized = b"GET /owned-extension-article HTTP/1.1\r\nHost: native-extension.test\r\nX: " + b"x" * 9000 + b"\r\n\r\n"
        self.assertTrue(self.request(oversized).startswith(b"HTTP/1.1 403"))
        started = time.monotonic()
        self.assertTrue(self.request(b"GET /owned-extension-article HTTP/1.1\r\n").startswith(b"HTTP/1.1 403"))
        self.assertLess(time.monotonic() - started, 3)
        self.assertEqual(json.loads(self.state.read_text())["rejected_requests"], 2)

    def test_relative_form_same_fixed_route_only(self):
        self.assertTrue(self.request(b"GET /owned-extension-article HTTP/1.0\r\nHost: native-extension.test:80\r\n\r\n").startswith(b"HTTP/1.1 200"))
        self.assertTrue(self.request(b"GET /arbitrary-secret HTTP/1.1\r\nHost: native-extension.test\r\n\r\n").startswith(b"HTTP/1.1 403"))


class LifetimeTests(unittest.TestCase):
    def test_ready_identity_and_cooperative_stop(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            child = subprocess.Popen([sys.executable, str(SCRIPT), "--ready", str(root / "ready"), "--state", str(root / "state"), "--stop-file", str(root / "stop")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                deadline = time.monotonic() + 5
                while not (root / "ready").exists() and child.poll() is None and time.monotonic() < deadline:
                    time.sleep(0.02)
                ready = json.loads((root / "ready").read_text())
                self.assertEqual(ready["pid"], child.pid)
                self.assertTrue(0 < ready["port"] < 65536)
                (root / "stop").write_text("stop")
                self.assertEqual(child.wait(timeout=3), 0)
                self.assertEqual(json.loads((root / "state").read_text())["forwarded_requests"], 0)
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait(timeout=3)


if __name__ == "__main__":
    unittest.main()
