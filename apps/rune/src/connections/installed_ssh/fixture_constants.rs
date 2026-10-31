const FIXTURE_TOKEN_UPLOAD: &str = r###"import os,stat,sys
p=os.path.expanduser(sys.argv[1]);d=os.path.dirname(p);n=os.path.basename(p)
os.makedirs(d,mode=0o700,exist_ok=True)
dd=os.open(d,os.O_RDONLY|getattr(os,'O_DIRECTORY',0)|getattr(os,'O_NOFOLLOW',0))
try:
 s=os.fstat(dd)
 if not stat.S_ISDIR(s.st_mode) or (hasattr(os,'getuid') and s.st_uid!=os.getuid()):raise SystemExit(1)
 if (s.st_mode&0o777)!=0o700:os.fchmod(dd,0o700)
 fd=os.open(n,os.O_WRONLY|os.O_CREAT|os.O_EXCL|getattr(os,'O_NOFOLLOW',0),0o600,dir_fd=dd)
 try:
  data=sys.stdin.buffer.read(4097)
  if not data or len(data)>4096:raise ValueError('invalid credential')
  while data:
   count=os.write(fd,data)
   if count<=0:raise OSError('short write')
   data=data[count:]
 except BaseException:
  os.unlink(n,dir_fd=dd)
  raise
 finally:os.close(fd)
finally:os.close(dd)
"###;
const FIXTURE_DISCOVER: &str = r###"import json,os,sys
p=os.path.join(sys.argv[1],'spawn-ledger.json')
try:
 with open(p,'rb') as f: raw=f.read(1048577)
except FileNotFoundError:print('[]');sys.exit(0)
if len(raw)>1048576:raise SystemExit(2)
rows=json.loads(raw)
if not isinstance(rows,list):raise SystemExit(3)
out=[]
for r in rows:
 if not isinstance(r,dict):continue
 pid=r.get('pid');port=r.get('port');host=r.get('host','')
 if not isinstance(pid,int) or isinstance(pid,bool) or pid<=0 or not isinstance(port,int) or isinstance(port,bool) or not 0<port<65536:continue
 if r.get('purpose') not in ('serve','dashboard') or r.get('isolated') is True or not isinstance(host,str) or host.lower() not in ('','0.0.0.0','127.0.0.1','::','::1','localhost'):continue
 try:os.kill(pid,0)
 except ProcessLookupError:continue
 except PermissionError:pass
 try:
  with open('/proc/%d/stat'%pid) as f:proc=f.read()
  fields=proc[proc.rfind(')')+2:].split()
  if fields[0]=='Z':continue
  claimed=r.get('create_time')
  if isinstance(claimed,(int,float)) and not isinstance(claimed,bool):
   with open('/proc/stat') as f:btime=next(int(line.split()[1]) for line in f if line.startswith('btime '))
   actual=btime+int(fields[19])/os.sysconf('SC_CLK_TCK')
   if abs(actual-claimed)>2.0:continue
 except (OSError,ValueError,IndexError,StopIteration):pass
 out.append(port)
print(json.dumps(out))
"###;
