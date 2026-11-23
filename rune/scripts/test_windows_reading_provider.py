"""Loopback HTTP contract tests only; no UI, model, credentials or downloads."""
import contextlib
import http.client
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest

SCRIPT = Path(__file__).with_name("windows-reading-provider.py")
spec = importlib.util.spec_from_file_location("reading_fixture_provider", SCRIPT)
provider = importlib.util.module_from_spec(spec)
spec.loader.exec_module(provider)
CONTRACT = {"before": "BEFORE owned fixture marker.", "selected": "Café shuttle service may help on this route in dry weather.",
            "after": "AFTER owned fixture marker.", "answer": "The Café shuttle may help on this route in dry weather."}


def chat(stream=True):
    return {"model": provider.MODEL, "messages": [
        {"role": "system", "content": "Explain the supplied text without adding facts."},
        {"role": "user", "content": json.dumps({"source": CONTRACT["selected"], "follow_up_question": ""}, ensure_ascii=False)}],
        "stream": stream, "keep_alive": "2m", "options": {"num_ctx": 8192, "num_predict": 1024, "temperature": 0.2}}


class ProviderTests(unittest.TestCase):
    @contextlib.contextmanager
    def server(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory) / "state.json"
            with provider.FixtureServer(CONTRACT, state) as server:
                worker = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.01})
                worker.start()
                try:
                    yield server, state
                finally:
                    server.shutdown()
                    worker.join(2)
                    self.assertFalse(worker.is_alive())

    def request(self, server, path, body, headers=None, method="POST"):
        connection = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=3)
        try:
            connection.request(method, path, json.dumps(body, ensure_ascii=False).encode(),
                               headers={"Content-Type": "application/json", **(headers or {})})
            result = connection.getresponse()
            return result.status, result.read(40_000)
        finally:
            connection.close()

    def test_exact_source_and_fixed_unicode_stream_and_buffered_envelopes(self):
        for streamed in (True, False):
            with self.server() as (server, state):
                status, raw = self.request(server, "/api/show", {"model": provider.MODEL})
                self.assertEqual(status, 200)
                model = json.loads(raw)
                self.assertEqual(model, {"details": {"format": "gguf"}, "capabilities": ["completion"]})
                status, raw = self.request(server, "/api/chat", chat(streamed))
                self.assertEqual(status, 200)
                chunks = [json.loads(line) for line in raw.splitlines()] if streamed else [json.loads(raw)]
                self.assertEqual("".join(c["message"]["content"] for c in chunks), CONTRACT["answer"])
                self.assertTrue(chunks[-1]["done"])
                self.assertEqual(json.loads(state.read_text()), dict(total_requests=2, show_count=1, chat_count=1, source_verified=True, rejected=False))

    def test_wrong_source_surrounding_text_auth_extra_fields_and_paths_reject_without_echo(self):
        cases = []
        for text in ("unexpected-sensitive-input", CONTRACT["before"] + CONTRACT["selected"], CONTRACT["selected"] + CONTRACT["after"]):
            body = chat()
            body["messages"][1]["content"] = json.dumps({"source": text, "follow_up_question": ""})
            cases.append(("/api/chat", body, {}))
        extra = chat(); extra["tools"] = [{"private": "unexpected-sensitive-input"}]
        cases.extend([("/api/chat", extra, {}), ("/api/chat", chat(), {"Authorization": "Bearer unexpected-sensitive-input"}),
                      ("/api/tags", {}, {}), ("/api/chat?capture=unexpected-sensitive-input", chat(), {})])
        for path, body, headers in cases:
            with self.server() as (server, state), contextlib.redirect_stderr(io.StringIO()) as errors:
                status, raw = self.request(server, path, body, headers)
                self.assertEqual(status, 400)
                self.assertNotIn(b"unexpected-sensitive-input", raw)
                current = state.read_text()
                self.assertNotIn("unexpected-sensitive-input", current)
                self.assertTrue(json.loads(current)["rejected"])
                self.assertFalse(json.loads(current)["source_verified"])
                self.assertEqual(errors.getvalue(), "")

    def test_duplicate_keys_oversized_body_and_wrong_method_are_not_accepted(self):
        with self.server() as (server, state):
            self.assertEqual(self.request(server, "/api/show", {"model": provider.MODEL}, method="GET")[0], 400)
            self.assertEqual(self.request(server, "/api/show", {"oversized": "x" * 33_000})[0], 400)
            connection = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=3)
            try:
                connection.request("POST", "/api/show", '{"model":"secret","model":"'+provider.MODEL+'"}', {"Content-Type": "application/json"})
                response = connection.getresponse()
                self.assertEqual(response.status, 400)
                response.read()
            finally:
                connection.close()
            self.assertEqual(json.loads(state.read_text())["total_requests"], 3)

    def test_no_chat_before_show_or_duplicate_metadata_generation(self):
        with self.server() as (server, state):
            self.assertEqual(self.request(server, "/api/chat", chat())[0], 400)
            self.assertEqual(self.request(server, "/api/show", {"model": provider.MODEL})[0], 200)
            self.assertEqual(self.request(server, "/api/show", {"model": provider.MODEL})[0], 400)
            self.assertEqual(json.loads(state.read_text())["total_requests"], 3)
            self.assertTrue(json.loads(state.read_text())["rejected"])

    def test_cli_ready_pid_ephemeral_port_atomic_state_and_cooperative_stop(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            contract = root / "contract.json"; contract.write_text(json.dumps(CONTRACT), encoding="utf-8")
            ready, state, stop = (root / n for n in ("ready.json", "state.json", "stop"))
            child = subprocess.Popen([sys.executable, str(SCRIPT), "--contract", str(contract), "--ready", str(ready), "--state", str(state), "--stop-file", str(stop)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                deadline = time.monotonic() + 5
                while not ready.exists() and child.poll() is None and time.monotonic() < deadline:
                    time.sleep(0.01)
                metadata = json.loads(ready.read_text())
                self.assertEqual(metadata["pid"], child.pid)
                self.assertTrue(0 < metadata["port"] <= 65535)
                self.assertEqual(json.loads(state.read_text())["total_requests"], 0)
                stop.touch()
                output, errors = child.communicate(timeout=5)
                self.assertEqual(child.returncode, 0)
                self.assertEqual((output, errors), (b"", b""))
            finally:
                if child.poll() is None:
                    child.kill(); child.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
