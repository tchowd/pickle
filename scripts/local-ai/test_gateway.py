import importlib.util,json,pathlib,tempfile,threading,unittest,urllib.request,urllib.error
class GatewayChecks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory();root=pathlib.Path(cls.temp.name)
        (root/'access-token').write_text('fixture');(root/'model-digest').write_text('digest')
        (root/'gateway.py').write_text(pathlib.Path(__file__).with_name('gateway.py').read_text())
        spec=importlib.util.spec_from_file_location('gateway',root/'gateway.py');cls.module=importlib.util.module_from_spec(spec);spec.loader.exec_module(cls.module)
        cls.module.metadata=lambda path,body=None: {'models':[{'name':'llama3.2:1b','digest':'digest'},{'name':'preserved-large-model'}]} if path=='/api/tags' else {'details':{'format':'gguf'}}
        cls.server=cls.module.ThreadingHTTPServer(('127.0.0.1',0),cls.module.Handler)
        cls.thread=threading.Thread(target=cls.server.serve_forever,daemon=True);cls.thread.start()
    @classmethod
    def tearDownClass(cls): cls.server.shutdown();cls.server.server_close();cls.temp.cleanup()
    def call(self,path,body=None,token='fixture'):
        request=urllib.request.Request('http://127.0.0.1:'+str(self.server.server_port)+path,data=None if body is None else json.dumps(body).encode(),headers={'Authorization':'Bearer '+token})
        try:
            with urllib.request.urlopen(request,timeout=3) as response: return response.status,json.load(response)
        except urllib.error.HTTPError as error:
            with error: return error.code,json.load(error)
    def test_auth_and_metadata_filter(self):
        self.assertEqual(self.call('/api/tags',token='wrong')[0],403)
        self.assertEqual([m['name'] for m in self.call('/api/tags')[1]['models']],['llama3.2:1b'])
    def test_no_model_management_or_cloud(self):
        self.assertEqual(self.call('/api/pull',{'model':'llama3.2:1b'})[0],404)
        self.assertEqual(self.call('/api/chat',{'model':'qwen:cloud'})[0],404)
        self.assertEqual(self.call('/api/chat',{'model':'preserved-large-model'})[0],404)
    def test_bounds_and_busy(self):
        payload={'model':'llama3.2:1b','messages':[{'role':'user','content':'Hello'}],'options':{'num_ctx':8192,'num_predict':100}}
        self.module.BUSY.acquire()
        try: self.assertEqual(self.call('/api/chat',payload)[0],409)
        finally: self.module.BUSY.release()
        payload['messages'][0]['content']='x'*7000
        self.assertEqual(self.call('/api/chat',payload)[0],400)
        payload['messages'][0]={'role':'user','content':'Hello','images':['fake']}
        self.assertEqual(self.call('/api/chat',payload)[0],400)
    def test_cloud_alias_rejected_even_with_local_name(self):
        original=self.module.metadata
        self.module.metadata=lambda path,body=None: original(path,body) if path=='/api/tags' else {'remote_host':'https://ollama.com','details':{'format':'gguf'}}
        try: self.assertEqual(self.call('/api/show',{'model':'llama3.2:1b'})[0],400)
        finally: self.module.metadata=original
if __name__=='__main__': unittest.main()
