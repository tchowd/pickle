#!/usr/bin/env python3
"""Owned non-forwarding browser proxy. No DNS, upstream sockets, cookies or logs."""
import argparse
import json
import os
from pathlib import Path
import socket
import socketserver
import tempfile
import threading
import time

ORIGIN = "http://native-extension.test"
PAGE_PATH = "/owned-extension-article"
PAGE = ("<!doctype html><meta charset=utf-8><title>Owned extension article</title>"
        "<main><article><p>Café shuttle service may reduce waiting on this route in dry weather. "
        "Evidence from one route does not establish reliability in rain.</p></article></main>").encode("utf-8")


def atomic_json(path, value):
    path = Path(path)
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".extension-", delete=False) as stream:
        temporary = Path(stream.name)
        stream.write(json.dumps(value, separators=(",", ":")).encode())
    try:
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


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = False
    daemon_threads = False
    block_on_close = True
    request_queue_size = 4

    def __init__(self, state_path):
        self.state_path = Path(state_path)
        self.state = dict(total_requests=0, pages_served=0, rejected_requests=0, forwarded_requests=0)
        self.lock = threading.Lock()
        self.slots = threading.BoundedSemaphore(4)
        super().__init__(("127.0.0.1", 0), Handler)
        self.timeout = 0.2
        atomic_json(self.state_path, self.state)

    def process_request(self, request, address):
        if not self.slots.acquire(False):
            request.close()
            return
        try:
            super().process_request(request, address)
        except BaseException:
            self.slots.release()
            raise

    def process_request_thread(self, request, address):
        try:
            super().process_request_thread(request, address)
        finally:
            self.slots.release()

    def handle_error(self, request, address):
        # Never serialize unexpected HTTP input, paths, headers or exceptions.
        pass


class Handler(socketserver.BaseRequestHandler):
    def handle(self):
        raw = bytearray()
        end = time.monotonic() + 1
        self.request.settimeout(0.1)
        while b"\r\n\r\n" not in raw and len(raw) < 8192 and time.monotonic() < end:
            try:
                chunk = self.request.recv(min(1024, 8192 - len(raw)))
            except socket.timeout:
                continue
            if not chunk:
                break
            raw.extend(chunk)
        accepted = False
        try:
            if not raw.endswith(b"\r\n\r\n") or len(raw) > 8192:
                raise ValueError()
            lines = bytes(raw).decode("ascii").split("\r\n")
            if len(lines) > 35 or len(lines[0]) > 512:
                raise ValueError()
            method, target, protocol = lines[0].split(" ")
            hosts = []
            for line in lines[1:-2]:
                key, value = line.split(":", 1)
                if key.lower() == "host":
                    hosts.append(value.strip().lower())
                if key.lower() in ("authorization", "proxy-authorization", "transfer-encoding"):
                    raise ValueError()
                if key.lower() == "content-length" and value.strip() != "0":
                    raise ValueError()
            accepted = (method == "GET" and protocol in ("HTTP/1.0", "HTTP/1.1") and
                        hosts in (["native-extension.test"], ["native-extension.test:80"]) and
                        target in (PAGE_PATH, ORIGIN + PAGE_PATH))
        except (ValueError, UnicodeError):
            accepted = False
        with self.server.lock:
            self.server.state["total_requests"] += 1
            self.server.state["pages_served" if accepted else "rejected_requests"] += 1
            atomic_json(self.server.state_path, self.server.state)
        body = PAGE if accepted else b"Owned fixture proxy does not forward requests."
        status = b"200 OK" if accepted else b"403 Forbidden"
        response = (b"HTTP/1.1 " + status + b"\r\nContent-Type: text/html; charset=utf-8\r\n"
                    b"Content-Security-Policy: default-src 'none'\r\nCache-Control: no-store\r\n"
                    b"Connection: close\r\nContent-Length: " + str(len(body)).encode() + b"\r\n\r\n" + body)
        self.request.settimeout(0.5)
        try:
            self.request.sendall(response)
        except OSError:
            pass


def main():
    parser = argparse.ArgumentParser(description="Owned browser extension setup proxy; no upstream access")
    parser.add_argument("--ready", required=True)
    parser.add_argument("--state", required=True)
    parser.add_argument("--stop-file", required=True)
    args = parser.parse_args()
    try:
        with Server(args.state) as server:
            atomic_json(args.ready, dict(pid=os.getpid(), port=server.server_address[1]))
            end = time.monotonic() + 300
            while not Path(args.stop_file).exists():
                if time.monotonic() >= end:
                    return 1
                server.handle_request()
        return 0
    except OSError:
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
