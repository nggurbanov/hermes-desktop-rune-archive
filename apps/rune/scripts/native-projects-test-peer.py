#!/usr/bin/env python3
"""Private synthetic Local gateway. Run: python3 fixture.py selftest | serve ROOT."""
import asyncio, hashlib, json, os, pathlib, secrets, signal, socket, stat, struct, subprocess, sys, tempfile, time
from aiohttp import web, ClientSession, WSMsgType

INSTANCE = 'private-fixture-instance'
EPOCH = 7

def dump(path, value):
    path.write_text(json.dumps(value, sort_keys=True) + '\n')
    path.chmod(0o600)

def private_root(root):
    root = pathlib.Path(root)
    if not root.is_absolute() or root.resolve() != root or root.parent != pathlib.Path('/dev/shm'):
        raise ValueError('root must be a fresh direct /dev/shm child')
    root.mkdir(mode=0o700)  # refuses existing paths including existing user homes
    return root

def proc_identity(pid):
    s = pathlib.Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
    return {'pid':pid, 'startticks':int(s[19]), 'pgid':os.getpgid(pid)}

class Fixture:
    def __init__(self, root):
        self.root = private_root(root)
        self.home = self.root / 'h'; self.home.mkdir(mode=0o700)
        self.folder = self.root / 'p'; self.folder.mkdir(mode=0o700)
        self.second = self.root / 'q'; self.second.mkdir(mode=0o700)
        self.home = str(self.home.resolve()); self.folder = str(self.folder.resolve()); self.second = str(self.second.resolve())
        self.tickets = {}; self.project = None; self.counter = 0
        self.log = open(self.root / 'transcript.jsonl', 'x', buffering=1); os.chmod(self.log.name, 0o600)
    def event(self, **kw):
        self.log.write(json.dumps(kw, sort_keys=True)+'\n')
    def mint(self, purpose):
        t = secrets.token_urlsafe(32); key = hashlib.sha256(t.encode()).hexdigest()
        self.tickets[key] = (time.monotonic()+30, purpose)
        self.event(channel='control', action='mint', purpose=purpose, ticket_sha256=key, profile_id=self.home, instance_id=INSTANCE)
        return t
    def redeem(self, token, purpose):
        key = hashlib.sha256(token.encode()).hexdigest()
        item = self.tickets.get(key); ok = bool(item and item[0] > time.monotonic() and item[1] == purpose)
        if ok: del self.tickets[key]
        elif item and item[0] <= time.monotonic(): del self.tickets[key]
        self.event(channel='auth', purpose=purpose, ticket_sha256=key, accepted=ok)
        return ok
    async def control(self, reader, writer):
        sock = writer.get_extra_info('socket'); pid,uid,gid = struct.unpack('3i', sock.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12))
        req = {}; reply = {'protocol':1,'ok':False,'error':'unsupported or invalid request'}
        try:
            raw = await asyncio.wait_for(reader.readline(), 5)
            if len(raw)>65536: raise ValueError('size')
            req = json.loads(raw); verb=req.get('verb'); p=req.get('params',{})
            self.event(channel='control', verb=verb if verb in ('identify','session-ticket') else 'unsupported', peer_pid=pid, peer_uid=uid)
            if uid != os.geteuid() or req.get('protocol') != 1 or set(req)-{'protocol','id','verb','params'}: raise ValueError('envelope')
            if verb == 'identify' and not p:
                result={'pid':os.getpid(),'runtime_protocol':1,'state':'ready','authority_epoch':EPOCH,'instance_id':INSTANCE,'capabilities':['session-authority-v1'],'api_origin':self.origin,'served_profiles':[{'home':self.home,'profile_id':self.home}]}
            elif verb == 'session-ticket' and isinstance(p,dict) and p.get('profile_id')==self.home and p.get('instance_id')==INSTANCE and p.get('purpose') in ('interactive','native-http') and set(p)<= {'profile_id','instance_id','purpose','scope'} and (p.get('scope')=='host' if p['purpose']=='interactive' else 'scope' not in p):
                result={'profile_id':self.home,'instance_id':INSTANCE,'ticket':self.mint(p['purpose']),'expires_in_seconds':30}
            else: raise ValueError('request')
            reply={'protocol':1,'ok':True,'result':result}
        except (ValueError, TypeError, json.JSONDecodeError, asyncio.TimeoutError): pass
        if isinstance(req,dict) and 'id' in req: reply['id']=req['id']
        writer.write((json.dumps(reply)+'\n').encode()); await writer.drain(); writer.close(); await writer.wait_closed()
    def rpc(self, method, p):
        if not isinstance(p,dict): raise ValueError()
        if method=='profiles.list': return {'profiles':[{'name':'default','path':self.home}]}
        if method=='runtime.describe': return {'profile_id':self.home,'instance_id':INSTANCE,'authority_epoch':EPOCH}
        if method=='session.list': return {'sessions':[]}
        if method.startswith('projects.') and 'profile' in p: raise ValueError()
        if method=='projects.create':
            folders=p.get('folders'); name=p.get('name')
            if not isinstance(name,str) or not name or not isinstance(folders,list) or not folders or p.get('use') is not True or folders != [self.folder, self.second]: raise ValueError()
            self.project={'id':'fixture-project','name':name,'label':name,'primary_path':self.folder,'path':self.folder}
            return {'project':self.project}
        if method=='projects.project_sessions' and self.project and p.get('project_id')==self.project['id']: return {'project':self.project,'sessions':[]}
        if method=='projects.tree': return {'active_id':self.project['id'] if self.project else None,'projects':[self.project] if self.project else []}
        raise ValueError()
    async def handler(self, request):
        self.counter+=1; client=self.counter
        path=request.path
        self.event(channel='http',client=client,method=request.method if request.method in ('GET','POST') else 'other',path=path if path in ('/api/status','/api/profiles','/api/ws') else 'unsupported',peer=request.remote)
        if request.method=='GET' and path in ('/api/status','/api/profiles'):
            return web.json_response({'status':'ready'} if path.endswith('status') else {'profiles':[{'name':'default','path':self.home}]})
        if path=='/api/ws' and request.method=='GET' and not request.query_string:
            offered=[x.strip() for x in request.headers.get('Sec-WebSocket-Protocol','').split(',')]
            tags=[x[len('hermes-gateway-ticket.'):] for x in offered if x.startswith('hermes-gateway-ticket.')]
            if 'hermes-gateway-v1' not in offered or len(tags)!=1 or not self.redeem(tags[0],'interactive'): return web.Response(status=401)
            ws=web.WebSocketResponse(protocols=['hermes-gateway-v1']); await ws.prepare(request)
            async for msg in ws:
                if msg.type != WSMsgType.TEXT: continue
                obj = None
                try:
                    obj=json.loads(msg.data); method=obj.get('method'); p=obj.get('params',{})
                    self.event(channel='rpc',client=client,method=method if method in ('profiles.list','runtime.describe','session.list','projects.create','projects.project_sessions','projects.tree') else 'unsupported', profile_present=isinstance(p,dict) and 'profile' in p)
                    if obj.get('jsonrpc')!='2.0' or 'id' not in obj: raise ValueError()
                    result=self.rpc(method,p); reply={'jsonrpc':'2.0','id':obj['id'],'result':result}
                except (ValueError,TypeError,AttributeError,json.JSONDecodeError): reply={'jsonrpc':'2.0','id':obj.get('id') if isinstance(obj,dict) else None,'error':{'code':-32601,'message':'Method not supported'}}
                await ws.send_json(reply)
            return ws
        token=request.headers.get('X-Hermes-Gateway-Ticket','')
        if not self.redeem(token,'native-http'): return web.Response(status=401)
        return web.json_response({'error':'unsupported route'},status=404)
    async def run(self):
        app=web.Application(); app.router.add_route('*','/{tail:.*}',self.handler)
        runner=web.AppRunner(app, access_log=None); await runner.setup(); site=web.TCPSite(runner,'127.0.0.1',0); await site.start()
        port=site._server.sockets[0].getsockname()[1]; self.origin=f'http://127.0.0.1:{port}/'
        path=self.home+'/gateway.sock'; server=await asyncio.start_unix_server(self.control,path); os.chmod(path,0o600)
        ready={'home':self.home,'folder':self.folder,'second':self.second,'origin':self.origin,**proc_identity(os.getpid()),'socket_inode':os.stat(path).st_ino}
        dump(self.root/'ready.json',ready)
        stop=asyncio.Event(); loop=asyncio.get_running_loop()
        for sig in (signal.SIGTERM,signal.SIGINT): loop.add_signal_handler(sig,stop.set)
        await stop.wait(); server.close(); await server.wait_closed(); await runner.cleanup(); os.unlink(path); self.log.close()

def ctl(home, verb, params=None):
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(3); s.connect(home+'/gateway.sock'); peer=struct.unpack('3i',s.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12))
        q={'protocol':1,'id':1,'verb':verb}
        if params is not None:q['params']=params
        s.sendall((json.dumps(q)+'\n').encode()); f=s.makefile('rb'); return json.loads(f.readline()), peer

async def test(root, ready):
    n=0
    def check(x):
        nonlocal n
        assert x; n+=1
    h=ready['home']; info,peer=ctl(h,'identify'); result=info['result']
    check(peer[0]==ready['pid']==result['pid'] and peer[1]==os.geteuid()); check(stat.S_IMODE(os.stat(h).st_mode)==0o700); check(stat.S_IMODE(os.stat(h+'/gateway.sock').st_mode)==0o600); check(os.stat(h+'/gateway.sock').st_ino==ready['socket_inode'])
    check(result['served_profiles']==[{'home':h,'profile_id':h}] and result['instance_id']==INSTANCE and result['authority_epoch']==EPOCH)
    def ticket(purpose, **change):
        p={'profile_id':h,'instance_id':INSTANCE,'purpose':purpose}; p.update({'scope':'host'} if purpose=='interactive' else {}); p.update(change); return ctl(h,'session-ticket',p)[0]
    check(not ticket('interactive',instance_id='wrong')['ok']); check(not ticket('interactive',profile_id='/wrong')['ok']); check(not ticket('unknown')['ok'])
    async with ClientSession() as client:
        base=ready['origin'].rstrip('/')
        for path in ('/api/status','/api/profiles'):
            async with client.get(base+path) as res: check(res.status==200)
        async with client.get(base+'/api/unknown') as res: check(res.status==401)
        async with client.get(base+'/api/unknown',headers={'X-Hermes-Gateway-Ticket':'invalid'}) as res: check(res.status==401)
        http=ticket('native-http')['result']['ticket']
        try:
            await client.ws_connect(base+'/api/ws',protocols=['hermes-gateway-v1','hermes-gateway-ticket.'+http]); check(False)
        except Exception: check(True)
        async with client.get(base+'/api/unknown',headers={'X-Hermes-Gateway-Ticket':http}) as res: check(res.status==404)
        async with client.get(base+'/api/unknown',headers={'X-Hermes-Gateway-Ticket':http}) as res: check(res.status==401)
        ws_ticket=ticket('interactive')['result']['ticket']
        async with client.ws_connect(base+'/api/ws',protocols=['hermes-gateway-v1','hermes-gateway-ticket.'+ws_ticket]) as ws:
            check(ws.protocol=='hermes-gateway-v1')
            async def rpc(method,params):
                await ws.send_json({'jsonrpc':'2.0','id':1,'method':method,'params':params}); return await ws.receive_json()
            check((await rpc('profiles.list',{}))['result']['profiles'][0]['path']==h)
            check((await rpc('runtime.describe',{}))['result']=={'profile_id':h,'instance_id':INSTANCE,'authority_epoch':EPOCH})
            check((await rpc('session.list',{}))['result']['sessions']==[])
            check((await rpc('projects.create',{'name':'Fixture','folders':[ready['folder'],ready['second']],'use':True}))['result']['project']['primary_path']==ready['folder'])
            check((await rpc('projects.project_sessions',{'project_id':'fixture-project'}))['result']['project']['id']=='fixture-project')
            check((await rpc('projects.tree',{'preview_limit':3}))['result']['active_id']=='fixture-project')
            check((await rpc('projects.tree',{'profile':'default'}))['error']['code']==-32601)
            check((await rpc('fs.write',{}))['error']['code']==-32601)
        try:
            await client.ws_connect(base+'/api/ws',protocols=['hermes-gateway-v1','hermes-gateway-ticket.'+ws_ticket]); check(False)
        except Exception: check(True)
    return n

def selftest():
    root='/dev/shm/hrg.'+secrets.token_hex(5)
    out=subprocess.DEVNULL
    proc=subprocess.Popen([sys.executable,__file__,'serve',root],stdout=out,stderr=out,start_new_session=True)
    try:
        deadline=time.monotonic()+10
        while not pathlib.Path(root,'ready.json').exists():
            if proc.poll() is not None or time.monotonic()>deadline: raise RuntimeError('fixture failed readiness')
            time.sleep(.02)
        ready=json.loads(pathlib.Path(root,'ready.json').read_text()); count=asyncio.run(test(root,ready))
        dump(pathlib.Path(root,'selftest.json'),{'checks':count,'status':'passed','identity':ready})
        print(json.dumps({'checks':count,'root':root,'status':'passed'}))
    finally:
        proc.terminate(); proc.wait(timeout=10)
        if proc.returncode != 0: raise RuntimeError(f'fixture exit {proc.returncode}')
        if pathlib.Path(root,'h','gateway.sock').exists(): raise RuntimeError('socket survived shutdown')
        dump(pathlib.Path(root,'retirement.json'),{'pid':proc.pid,'returncode':proc.returncode,'waited':True,'socket_closed':True})

if __name__=='__main__':
    if sys.argv[1:] == ['selftest']: selftest()
    elif len(sys.argv)==3 and sys.argv[1]=='serve': asyncio.run(Fixture(sys.argv[2]).run())
    else: raise SystemExit('usage: fixture.py selftest | serve /dev/shm/FRESH_ROOT')
