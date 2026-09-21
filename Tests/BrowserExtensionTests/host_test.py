import json, os, pathlib, socket, struct, subprocess, tempfile, threading
root = pathlib.Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory() as directory:
    config = pathlib.Path(directory)/'Library/Application Support/Pickle/BrowserBridge'
    config.mkdir(parents=True)
    with socket.socket() as server:
        server.bind(('127.0.0.1',0)); server.listen(1)
        (config/'connection.json').write_text(json.dumps({'port':server.getsockname()[1],'token':'fixture-token'}))
        received=[]
        def respond():
            client,_=server.accept()
            with client:
                raw=bytearray()
                while not raw.endswith(b'\n'): raw.extend(client.recv(4096))
                received.append(json.loads(raw))
                client.sendall(b'{"ok":"true"}')
        worker=threading.Thread(target=respond);worker.start()
        payload=json.dumps({'selection':'Hello','reference':{'text':'Fixture'}}).encode()
        result=subprocess.run(['/usr/bin/python3',str(root/'scripts/browser-host.py')],input=struct.pack('=I',len(payload))+payload,
            capture_output=True,env={**os.environ,'HOME':directory,'PICKLE_BROWSER_BUNDLE':'com.google.Chrome','PICKLE_APP':'/unused'},timeout=5)
        worker.join(timeout=5)
        size=struct.unpack('=I',result.stdout[:4])[0]
        assert len(result.stdout[4:])==size
        assert json.loads(result.stdout[4:])=={'ok':'true'}
        assert received[0]['token']=='fixture-token'
        assert received[0]['payload']['bundleID']=='com.google.Chrome'
        assert received[0]['payload']['selection']=='Hello'
    oversized=subprocess.run(['/usr/bin/python3',str(root/'scripts/browser-host.py')],input=struct.pack('=I',90001),capture_output=True,timeout=5)
    assert 'too large' in json.loads(oversized.stdout[4:])['error']
print('PASS: native framing, local authenticated relay, browser identity, payload bounds')
