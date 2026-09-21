import json, pathlib, socket, sys, time, uuid
config = json.loads(pathlib.Path(sys.argv[1]).read_text())
payload={'id':str(uuid.uuid4()),'capturedAt':time.time(),'selection':'Fixture selection','bundleID':'com.google.Chrome',
         'reference':{'url':'https://example.com/article','title':'Fixture','text':'Fixture article','kind':'article'}}
def send(token, message):
    with socket.create_connection(('127.0.0.1',config['port']),timeout=3) as client:
        client.sendall(json.dumps({'token':token,'payload':message}).encode()+b'\n')
        result=bytearray()
        while True:
            part=client.recv(4096)
            if not part: break
            result.extend(part)
        return json.loads(result)
assert 'error' in send('incorrect',payload)
assert 'error' in send(config['token'],{**payload,'capturedAt':0})
assert send(config['token'],payload)=={'ok':'true'}
assert 'error' in send(config['token'],payload)
assert 'error' in send(config['token'],{**payload,'id':str(uuid.uuid4()),'reference':{**payload['reference'],'url':'file:///etc/passwd'}})
print('PASS: native bridge authentication, expiry, accepted reference, replay, URL validation')
