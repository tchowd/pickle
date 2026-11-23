#!/usr/bin/env python3
"""Fixed synthetic Ollama HTTP fixture; never loads a model or logs input."""
import argparse
import json
import os
from pathlib import Path
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MODEL = "pickle-reading-fixture:local"
MAX_BODY = 32_768
MAX_REQUESTS = 16


def strict_json(raw):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("Duplicate JSON key")
            result[key] = value
        return result
    return json.loads(raw, object_pairs_hook=unique)


def atomic_json(path, value):
    path = Path(path)
    payload = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".reading-", delete=False) as stream:
        temporary = Path(stream.name)
        stream.write(payload)
    try:
        # Windows readers may briefly omit FILE_SHARE_DELETE. Retry only this
        # small owned metadata replacement, with an absolute bounded deadline.
        deadline = time.monotonic() + 1
        while True:
            try:
                os.replace(temporary, path)
                return
            except PermissionError:
                if time.monotonic() >= deadline:
                    raise
                time.sleep(0.01)
    finally:
        temporary.unlink(missing_ok=True)


def contract_file(path):
    path = Path(path)
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 4096:
        raise ValueError("Invalid fixed contract")
    value = strict_json(path.read_text(encoding="utf-8-sig"))
    if not isinstance(value, dict) or set(value) != {"before", "selected", "after", "answer"}:
        raise ValueError("Invalid fixed contract")
    for key, maximum in (("before", 300), ("selected", 200), ("after", 300), ("answer", 300)):
        text = value[key]
        if not isinstance(text, str) or not text.strip() or len(text) > maximum or any(ord(c) < 32 for c in text):
            raise ValueError("Invalid fixed contract")
    if value["before"] in value["selected"] or value["after"] in value["selected"]:
        raise ValueError("Ambiguous fixed contract")
    return value


class FixtureServer(ThreadingHTTPServer):
    daemon_threads = True
    block_on_close = False
    request_queue_size = 4

    def __init__(self, contract, state_path):
        self.contract = contract
        self.state_path = Path(state_path)
        self.state = dict(total_requests=0, show_count=0, chat_count=0, source_verified=False, rejected=False)
        self.state_lock = threading.Lock()
        self.slots = threading.BoundedSemaphore(4)
        super().__init__(("127.0.0.1", 0), Handler)
        self.timeout = 0.2
        atomic_json(self.state_path, self.state)

    def process_request(self, request, client_address):
        if not self.slots.acquire(blocking=False):
            request.close()
            return
        try:
            super().process_request(request, client_address)
        except BaseException:
            self.slots.release()
            raise

    def process_request_thread(self, request, client_address):
        try:
            super().process_request_thread(request, client_address)
        finally:
            self.slots.release()

    def handle_error(self, request, client_address):
        # Never print a traceback containing any unrecognized request content.
        with self.state_lock:
            self.state["rejected"] = True
            atomic_json(self.state_path, self.state)


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"

    def setup(self):
        super().setup()
        self.connection.settimeout(2)

    def log_message(self, *args):
        pass

    def send_error(self, code, message=None, explain=None):
        self.reject()

    def response(self, status, body, streaming=False):
        encoded = (body if isinstance(body, str) else json.dumps(body, ensure_ascii=False)).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/x-ndjson" if streaming else "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(encoded)
        self.close_connection = True

    def do_GET(self):
        self.reject()

    do_PUT = do_DELETE = do_PATCH = do_OPTIONS = do_HEAD = do_GET

    def reject(self):
        with self.server.state_lock:
            self.server.state["total_requests"] += 1
            self.server.state["rejected"] = True
            atomic_json(self.server.state_path, self.server.state)
        self.response(400, {"error": "Fixture request rejected."})

    def do_POST(self):
        try:
            headers = self.headers
            allowed = {"host", "content-type", "content-length", "accept", "accept-encoding", "connection", "user-agent"}
            if len(self.path) > 64 or self.path not in ("/api/show", "/api/chat") or len(headers) > 16:
                raise ValueError()
            if any(key.lower() not in allowed for key in headers) or sum(len(k) + len(v) for k, v in headers.items()) > 8192:
                raise ValueError()
            lengths = headers.get_all("Content-Length", [])
            if len(lengths) != 1 or not lengths[0].isdigit() or len(lengths[0]) > 6:
                raise ValueError()
            length = int(lengths[0])
            if not 0 < length <= MAX_BODY or headers.get_content_type() != "application/json":
                raise ValueError()
            raw = self.rfile.read(length)
            if len(raw) != length:
                raise ValueError()
            body = strict_json(raw)
            if self.path == "/api/show":
                if body != {"model": MODEL}:
                    raise ValueError()
                kind = "show_count"
            else:
                self.validate_chat(body)
                kind = "chat_count"
            with self.server.state_lock:
                state = self.server.state
                if state["total_requests"] >= MAX_REQUESTS or state[kind] != 0 or kind == "chat_count" and state["show_count"] != 1:
                    raise ValueError()
                state["total_requests"] += 1
                state[kind] += 1
                if kind == "chat_count":
                    state["source_verified"] = True
                atomic_json(self.server.state_path, state)
        except (ValueError, TypeError, KeyError, UnicodeError, OSError):
            self.reject()
            return
        if self.path == "/api/show":
            self.response(200, {"details": {"format": "gguf"}, "capabilities": ["completion"]})
            return
        answer = self.server.contract["answer"]
        complete = {"model": MODEL, "done": True, "done_reason": "stop", "eval_count": 12,
                    "prompt_eval_count": 30, "load_duration": 0, "eval_duration": 1_000_000,
                    "message": {"role": "assistant", "content": answer}}
        if body["stream"]:
            # Two deterministic answer deltas, then a genuine done marker.
            split = len(answer) // 2
            chunks = [{"done": False, "message": {"content": part}} for part in (answer[:split], answer[split:])]
            complete["message"]["content"] = ""
            self.response(200, "".join(json.dumps(c, ensure_ascii=False) + "\n" for c in chunks + [complete]), True)
        else:
            self.response(200, complete)

    def validate_chat(self, body):
        if not isinstance(body, dict) or set(body) != {"model", "messages", "stream", "keep_alive", "options"}:
            raise ValueError()
        if body["model"] != MODEL or type(body["stream"]) is not bool or body["keep_alive"] != "2m":
            raise ValueError()
        if body["options"] != {"num_ctx": 8192, "num_predict": 1024, "temperature": 0.2}:
            raise ValueError()
        messages = body["messages"]
        if not isinstance(messages, list) or len(messages) != 2:
            raise ValueError()
        for message, role in zip(messages, ("system", "user")):
            if not isinstance(message, dict) or set(message) != {"role", "content"} or message["role"] != role:
                raise ValueError()
            content = message["content"]
            if not isinstance(content, str) or len(content.encode("utf-8")) > 8192:
                raise ValueError()
            if self.server.contract["before"].casefold() in content.casefold() or self.server.contract["after"].casefold() in content.casefold():
                raise ValueError()
        selected = strict_json(messages[1]["content"])
        if selected != {"source": self.server.contract["selected"], "follow_up_question": ""}:
            raise ValueError()


def main():
    parser = argparse.ArgumentParser(description="Fixed owned reading fixture; no model inference")
    parser.add_argument("--contract", required=True)
    parser.add_argument("--ready", required=True)
    parser.add_argument("--state", required=True)
    parser.add_argument("--stop-file", required=True)
    args = parser.parse_args()
    try:
        contract = contract_file(args.contract)
        with FixtureServer(contract, args.state) as server:
            atomic_json(args.ready, {"pid": os.getpid(), "port": server.server_port})
            deadline = time.monotonic() + 180
            while not Path(args.stop_file).exists():
                if time.monotonic() >= deadline:
                    return 1
                server.handle_request()
        return 0
    except (ValueError, OSError):
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
