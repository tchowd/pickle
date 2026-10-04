#!/usr/bin/python3
"""Pickle-only loopback Ollama gateway. No cloud aliases, model management, or content logs."""
import hmac, http.client, json, pathlib, select, socket, threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

DIRECTORY = pathlib.Path(__file__).resolve().parent
TOKEN = (DIRECTORY/'access-token').read_text().strip()
MODEL = 'llama3.2:1b'
BUSY = threading.Lock()

def metadata(path, body=None):
    c = http.client.HTTPConnection('127.0.0.1',11434,timeout=5)
    try:
        c.request('POST' if body is not None else 'GET',path,body=json.dumps(body) if body is not None else None,headers={'Content-Type':'application/json'})
        r=c.getresponse(); data=r.read(256001)
        if r.status != 200 or len(data)>256000: raise ValueError('metadata unavailable')
        return json.loads(data)
    finally: c.close()

def verify():
    model=next((m for m in metadata('/api/tags')['models'] if m['name']==MODEL),None)
    if not model or model.get('digest') != (DIRECTORY/'model-digest').read_text().strip(): raise ValueError('model digest changed')
    info=metadata('/api/show',{'model':MODEL})
    if info.get('remote_host') or info.get('remote_model') or info.get('details',{}).get('format')!='gguf': raise ValueError('cloud models forbidden')
    return info

class Handler(BaseHTTPRequestHandler):
    protocol_version='HTTP/1.1'
    def log_message(self,*args): pass
    def reply(self,status,value):
        encoded=json.dumps(value).encode()
        self.send_response(status); self.send_header('Content-Type','application/json'); self.send_header('Content-Length',str(len(encoded))); self.end_headers(); self.wfile.write(encoded)
    def authorized(self):
        if not hmac.compare_digest(self.headers.get('Authorization',''),'Bearer '+TOKEN):
            self.reply(403,{'error':'access denied'}); return False
        return True
    def do_GET(self):
        if not self.authorized(): return
        try:
            if self.path=='/api/tags': self.reply(200,{'models':[m for m in metadata('/api/tags')['models'] if m['name']==MODEL]})
            elif self.path=='/api/ps': self.reply(200,{'models':[m for m in metadata('/api/ps')['models'] if m['name']==MODEL]})
            else: self.reply(404,{'error':'route unavailable'})
        except Exception: self.reply(503,{'error':'local metadata unavailable'})
    def do_POST(self):
        if not self.authorized(): return
        self.connection.settimeout(10)
        try:
            count=int(self.headers.get('Content-Length','0'))
            if not 0<count<=30000: self.reply(413,{'error':'request too large'}); return
            body=json.loads(self.rfile.read(count))
            if body.get('model') != MODEL: self.reply(404,{'error':'model unavailable'}); return
            if self.path=='/api/show': self.reply(200,verify()); return
            if self.path!='/api/chat': self.reply(404,{'error':'route unavailable'}); return
            options=body.get('options',{})
            if options.get('num_ctx') != 8192 or not 1<=options.get('num_predict',0)<=1024: raise ValueError()
            messages=body['messages']
            if not isinstance(messages,list) or not 1<=len(messages)<=12: raise ValueError()
            for message in messages:
                if set(message)-{'role','content'} or message.get('role') not in ['system','user','assistant'] or not isinstance(message.get('content'),str): raise ValueError()
            total=sum(len(m['content'].encode()) for m in messages)
            if total+len(json.dumps(body.get('format',{}),separators=(',',':'),ensure_ascii=False).encode())>6656: raise ValueError()
            verify()
        except Exception: self.reply(400,{'error':'invalid or nonlocal request'}); return
        if not BUSY.acquire(False): self.reply(409,{'error':'busy'}); return
        upstream=http.client.HTTPConnection('127.0.0.1',11434,timeout=90)
        finished=threading.Event()
        def watch_disconnect():
            while not finished.wait(0.1):
                try:
                    ready,_,_=select.select([self.connection],[],[],0)
                    if ready and not self.connection.recv(1,socket.MSG_PEEK):
                        if upstream.sock: upstream.sock.shutdown(socket.SHUT_RDWR)
                        upstream.close(); return
                except OSError:
                    upstream.close(); return
        try:
            payload={'model':MODEL,'messages':messages,'stream':bool(body.get('stream',True)),
                     'keep_alive':'2m','options':{'num_ctx':8192,'num_predict':options['num_predict'],'temperature':0.2}}
            if 'format' in body: payload['format']=body['format']
            upstream.request('POST','/api/chat',json.dumps(payload),{'Content-Type':'application/json'})
            threading.Thread(target=watch_disconnect,daemon=True).start()
            response=upstream.getresponse()
            self.send_response(response.status)
            self.send_header('Content-Type',response.getheader('Content-Type','application/x-ndjson'))
            self.send_header('Connection','close'); self.end_headers(); self.close_connection=True
            count=0
            while True:
                part=response.read1(4096)
                if not part: break
                count+=len(part)
                if count>256000: break
                self.wfile.write(part); self.wfile.flush()
        except Exception:
            # No request bodies, tokens, or upstream messages enter logs.
            self.close_connection=True
        finally:
            finished.set(); upstream.close(); BUSY.release()

if __name__=='__main__':
    server=ThreadingHTTPServer(('127.0.0.1',11435),Handler)
    server.daemon_threads=True
    server.serve_forever()
