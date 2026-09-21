#!/usr/bin/python3
"""Chrome native messaging -> authenticated local Pickle bridge. No content files or logs."""
import json, os, pathlib, socket, struct, subprocess, sys, time

def read_exact(count):
    chunks = bytearray()
    while len(chunks) < count:
        part = sys.stdin.buffer.read(count - len(chunks))
        if not part: raise ValueError('Incomplete browser message')
        chunks.extend(part)
    return bytes(chunks)

def run():
    size = struct.unpack('=I', read_exact(4))[0]
    if size > 90000: raise ValueError('Page reference is too large')
    payload = json.loads(read_exact(size))
    payload['bundleID'] = os.environ['PICKLE_BROWSER_BUNDLE']
    config = pathlib.Path.home() / 'Library/Application Support/Pickle/BrowserBridge/connection.json'
    for attempt in range(20):
        try:
            state = json.loads(config.read_text())
            with socket.create_connection(('127.0.0.1', int(state['port'])), timeout=4) as client:
                client.sendall(json.dumps({'token':state['token'],'payload':payload}).encode() + b'\n')
                response = bytearray()
                while len(response) < 10000:
                    block = client.recv(4096)
                    if not block: break
                    response.extend(block)
                return json.loads(response)
        except (OSError, ValueError):
            if attempt == 0:
                subprocess.run(['/usr/bin/open', os.environ['PICKLE_APP']], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if attempt == 19: raise ValueError('Open Pickle and check browser context is enabled')
            time.sleep(0.2)
try:
    result = run()
except Exception as error:
    result = {'error': str(error)}
encoded = json.dumps(result).encode()
sys.stdout.buffer.write(struct.pack('=I',len(encoded)) + encoded)
sys.stdout.buffer.flush()
