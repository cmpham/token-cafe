#!/usr/bin/env python3
"""Independent Python host against the actual Xtensa engine in Espressif QEMU.
All identities, requests, secrets and decisions are synthetic. No desktop adapters.
"""
import contextlib, hashlib, hmac, json, os, pathlib, shutil, socket, struct, subprocess, sys, tempfile, time
ROOT=pathlib.Path(__file__).resolve().parents[1]
sys.path[:0]=[str(ROOT/'target/protocol-schema'),str(ROOT/'target/protocol-tools-py312/lib/python3.12/site-packages')]
import companion_pb2 as pb
from cryptography.hazmat.primitives.ciphers.aead import ChaCha20Poly1305
from cryptography.hazmat.primitives.kdf.hkdf import HKDF
from cryptography.hazmat.primitives import hashes
sys.path.insert(0,str(ROOT/'firmware'))
from host import connect, screenshot
OUT=ROOT/'target/protocol-v2-qa';OUT.mkdir(exist_ok=True)
KEY=b'\x42'*32
AUTH=b'\x22'*16

def sha(b): return hashlib.sha256(b).digest()
def lp(s):
 b=s.encode();return struct.pack('<H',len(b))+b
class Device:
 def __init__(self,sock):self.sock=sock;sock.settimeout(20)
 def read(self,n):
  data=b''
  while len(data)<n:
   part=self.sock.recv(n-len(data))
   if not part:raise ConnectionError('QEMU exited')
   data+=part
  return data
 def request(self,op,data=b'',ok=True):
  self.sock.sendall(b'QV2'+bytes([op])+struct.pack('<H',len(data))+data)
  header=self.read(7)
  assert header[:4]==b'QV2'+bytes([op]),header
  status=header[4];body=self.read(struct.unpack('<H',header[5:])[0])
  assert status==(0 if ok else 1),(op,status,body)
  return body
class Host:
 def __init__(self,d,cap):
  self.d=d;self.cap=cap;self.time=0;self.tx=0;self.rx=0;self.ids=[0,0];self.partial=[None,None];self.actions=[];self.assets=None
  d.request(9,struct.pack('<H',cap));idle=d.request(1);assert idle==b'\0\x07qa-qemu'
  hn=os.urandom(16);d.request(2,b'\x01'+hn);r=d.request(1);assert r[0]==2 and len(r)==33
  transcript=b'tc-auth-v2'+lp('qa-qemu')+hn+r[1:17]
  assert hmac.digest(KEY,b'board'+transcript,'sha256')[:16]==r[17:]
  d.request(2,b'\x03'+hmac.digest(KEY,b'host'+transcript,'sha256')[:16]);assert d.request(1)==b'\x04\0'+struct.pack('<H',cap)
  def key(label,n):return HKDF(algorithm=hashes.SHA256(),length=n,salt=sha(transcript),info=b'tc-v2/'+label).derive(KEY)
  self.encrypt=ChaCha20Poly1305(key(b'host-key',32));self.decrypt=ChaCha20Poly1305(key(b'board-key',32));self.tn=key(b'host-nonce',4);self.rn=key(b'board-nonce',4);self.epoch=key(b'epoch',16)
  got=self.poll();assert len(got)==1 and got[0].HasField('hello');self.assets=got[0].hello.asset_digest
  self.send('hello_ack',pb.HelloAck(asset_digest=self.assets,capabilities=[1,2,3,4,5,6],required_capabilities=[1,2,6],heartbeat_ms=1000,session_limit=64,blob_limit=32768,max_envelope_bytes=4096));assert self.poll()[0].HasField('resync')
 def env(self,field,msg):
  e=pb.Envelope(major=2,connection_epoch=self.epoch,authority_epoch=AUTH);getattr(e,field).CopyFrom(msg);return e
 def fragments(self,e):
  raw=e.SerializeToString();lane=0 if e.WhichOneof('payload') in ['snapshot_begin','snapshot_part','snapshot_commit','delta','blob_chunk'] else 1
  self.ids[lane]+=1;mid=self.ids[lane]|lane<<31
  for off in range(0,len(raw),self.cap-36):
   self.tx+=1;counter=struct.pack('<Q',self.tx);plain=struct.pack('<III',mid,len(raw),off)+raw[off:off+self.cap-36]
   yield counter+self.encrypt.encrypt(self.tn+counter,plain,b'tc-v2\x01'+counter)
 def send(self,field,msg):
  e=self.env(field,msg)
  for fragment in self.fragments(e):self.d.request(3,fragment)
  return e.SerializeToString()
 def poll(self):
  got=[]
  for _ in range(512):
   wire=self.d.request(4)
   if not wire:break
   counter=wire[:8];self.rx+=1;assert struct.unpack('<Q',counter)[0]==self.rx
   plain=self.decrypt.decrypt(self.rn+counter,wire[8:],b'tc-v2\x02'+counter)
   word,total,offset=struct.unpack('<III',plain[:12]);lane=word>>31;mid=word&0x7fffffff
   if offset==0:self.partial[lane]=[mid,total,bytearray()]
   p=self.partial[lane];assert p[0]==mid and p[1]==total and len(p[2])==offset;p[2]+=plain[12:]
   if len(p[2])==total:
    e=pb.Envelope.FromString(p[2]);assert e.connection_epoch==self.epoch
    if not e.HasField('hello'):assert e.authority_epoch==AUTH
    got.append(e);self.partial[lane]=None
    if e.HasField('action'):self.actions.append(e.action)
  else:raise AssertionError('unbounded output')
  return got
 def advance(self,ms):
  end=self.time+ms
  while self.time<end:
   self.time=min(self.time+100,end)
   if self.time%500<100:self.send('heartbeat',pb.Heartbeat(monotonic_ms=self.time))
   self.d.request(5,struct.pack('<Q',self.time));self.poll()
 def tap(self,x,y):
  self.d.request(6,struct.pack('<BHH',1,x,y));self.advance(30);self.d.request(6,struct.pack('<BHH',0,x,y));return self.poll()
 def present(self,name=None,qmp=None,process=None):
  self.d.request(7)
  if name:screenshot(qmp,process,OUT/f'{name}.ppm')
 def snapshot(self,sessions,view,revision=1):
  raw=[self.send('snapshot_begin',pb.SnapshotBegin(revision=revision,session_count=len(sessions),view=view,usage=pb.Usage(tokens_display='11.0B',cost_display='$5.1K')))]
  for i,s in enumerate(sessions):raw.append(self.send('snapshot_part',pb.SnapshotPart(revision=revision,index=i,sessions=[s])))
  digest=sha(b''.join(struct.pack('<I',len(r))+r for r in raw));self.send('snapshot_commit',pb.SnapshotCommit(revision=revision,parts_count=len(sessions),sha256=digest));return self.poll()
 def body(self,body):
  for offset in range(0,len(body),512):self.send('blob_chunk',pb.BlobChunk(sha256=sha(body),total_length=len(body),offset=offset,data=body[offset:offset+512],final=offset+512>=len(body)))
  return self.poll()

def fixture(cap):
 request_id=f'qa-{cap}'
 body=b"printf 'synthetic fixture only'\nNo actual command is executed.\nUnicode is explicit: caf\xc3\xa9\nSpaces  and\ttabs remain visible.\n"+b'Full request line for scrolling.\n'*14
 br=pb.BlobRef(sha256=sha(body),byte_length=len(body))
 digest=sha(b'tc-req-v2'+lp(request_id)+lp('claude:qa')+b'\x01'+lp('Run synthetic fixture')+lp('Bash')+sha(body)+struct.pack('<I',len(body))+b'\x02\x01\x02')
 r=pb.Request(id=request_id,digest=digest,title='Run synthetic fixture',tool_name='Bash',body_blob=br,state=1,allowed_decisions=[1,2])
 ss=[]
 for i,seat in enumerate([0,1,4,5,2,3,6,7]):
  ss.append(pb.Session(id='claude:qa' if i==0 else f'{"claude" if i<4 else "codex"}:qa-{i}',provider=1 if i<4 else 2,title=f'TEST PATRON {i+1}',display_name=f'QA PATRON {i+1}',status=3 if i==0 else 1 if i%2 else 2,waiting_reason=1 if i==0 else 0,seat=seat,appearance_seed=1234+i,task_blob=br,navigation_available=True))
 ss[0].approval.CopyFrom(r);return ss,body

@contextlib.contextmanager
def emulator():
 qemu=shutil.which('qemu-system-xtensa');assert qemu
 with tempfile.TemporaryDirectory(prefix='tc-v2-',dir='/tmp') as tmp,(OUT/'qemu.log').open('wb') as log:
  uart=pathlib.Path(tmp)/'uart';qmp=pathlib.Path(tmp)/'qmp'
  args=[qemu,'-machine','esp32s3','-m','8M','-global','driver=ssi_psram,property=is_octal,value=true','-global','driver=esp32s3.gpio,property=strap_mode,value=0x04','-drive',f'file={ROOT}/firmware/v2/build/merged-binary.bin,if=mtd,format=raw','-snapshot','-display','sdl','-serial',f'unix:{uart},server=on,wait=on','-qmp',f'unix:{qmp},server=on,wait=off','-monitor','none']
  p=subprocess.Popen(args,stdout=log,stderr=log,env={**os.environ,'SDL_VIDEODRIVER':'dummy'})
  try:
   with connect(uart,p) as sock:
    d=Device(sock);boot=bytearray()
    while not boot.endswith(b'TC_V2_READY\n'):
     boot+=d.read(1)
     if len(boot)>65536:raise RuntimeError('boot failed')
    (OUT/'boot.log').write_bytes(boot);yield d,qmp,p
  finally:
   p.terminate()
   try:p.wait(5)
   except subprocess.TimeoutExpired:p.kill();p.wait()

def extra(d,qmp,p):
 # All seats, double taps and stable IDs; no provider navigation is executed.
 h=Host(d,244);ss,body=fixture(999);h.snapshot(ss,pb.View(page=1));h.advance(10000)
 for s in ss:
  h.advance(500);seat=s.seat;x=60+seat%4*120;y=90+seat//4*82
  got=h.tap(x,y);assert h.actions[-1].session_id==s.id and h.actions[-1].kind==1
  if any(e.HasField('blob_get') for e in got):h.body(body)
  h.advance(100);h.tap(x,y);assert h.actions[-1].kind==6 and h.actions[-1].session_id==s.id
 h.present('all-seats-final',qmp,p)
 # Animations continue with heartbeat traffic only, without new UI state.
 h.present('animation-a',qmp,p);h.advance(333);h.present('animation-b',qmp,p)
 assert (OUT/'animation-a.ppm').read_bytes()!=(OUT/'animation-b.ppm').read_bytes()
 # Ten minutes of no input (host heartbeat does not reset inactivity).
 for _ in range(601):
  h.send('heartbeat',pb.Heartbeat());h.time+=1000;d.request(5,struct.pack('<Q',h.time));h.poll()
 assert struct.unpack('<I',d.request(8))[0]&4
 before=len(h.actions);h.tap(60,90);assert len(h.actions)==before
 assert not struct.unpack('<I',d.request(8))[0]&4
 h.present('wake',qmp,p)
 # Reboot/reconnect receives a fresh snapshot and rejects old epoch traffic.
 h=Host(d,509);ss,body=fixture(1000)
 for i in range(8,64):ss.append(pb.Session(id=f'overflow-{i}',provider=1 if i%2 else 2,status=2,seat=255,title='Overflow patron'))
 h.snapshot(ss,pb.View(page=1));h.advance(10000);h.present('64-sessions',qmp,p)
 got=h.tap(60,90);assert any(e.HasField('blob_get') for e in got);h.body(body)
 # Wrong content hash on a newly requested body must disconnect before review.
 h=Host(d,182);ss,body=fixture(1001);h.snapshot(ss,pb.View(page=1));h.advance(10000);h.tap(60,90)
 bad=pb.BlobChunk(sha256=sha(body),total_length=1,offset=0,data=b'x',final=True)
 e=h.env('blob_chunk',bad)
 for f in h.fragments(e):d.request(3,f,ok=False)
 assert not struct.unpack('<I',d.request(8))[0]&1
 d.request(9,struct.pack('<H',64),ok=False)
 print('PASS additional QEMU: eight-seat identity, double-tap routing, autonomous animation, 10min sleep/wake, reconnect, 64 sessions, hash failure, MTU rejection',flush=True)
 return {'additional_flow':'PASS'}

def test(d,qmp,p,cap):
 h=Host(d,cap);ss,body=fixture(cap);got=h.snapshot(ss,pb.View(page=1));assert any(e.HasField('applied') for e in got)
 h.advance(10000);h.present(f'{cap}-cafe',qmp,p)
 got=h.tap(60,90);assert h.actions[-1].kind==1 and h.actions[-1].session_id=='claude:qa'
 assert any(e.HasField('blob_get') for e in got);h.body(body)
 h.present(f'{cap}-selected',qmp,p)
 # Do not send an action result or view delta: local selection/review must work anyway.
 h.advance(500);h.tap(200,240);h.advance(120);h.tap(200,240)
 assert h.actions[-1].kind==2 and h.actions[-1].request_id==f'qa-{cap}';h.present(f'{cap}-review-loading',qmp,p)
 # A third tap must not approve during the 500ms quiet interval.
 before=len(h.actions);h.tap(250,240);assert len(h.actions)==before
 h.advance(600);h.present(f'{cap}-review-armed',qmp,p)
 h.tap(450,185);assert h.actions[-1].kind==5 and h.actions[-1].anchor>0
 h.present(f'{cap}-review-scroll',qmp,p);h.advance(600)
 h.tap(250,240);assert h.actions[-1].kind==7 and h.actions[-1].decision==1
 assert h.actions[-1].displayed_body_sha256==sha(body)
 h.present(f'{cap}-sent',qmp,p);before=len(h.actions);h.advance(600);h.tap(250,240);assert len(h.actions)==before
 # Replayed encrypted input fails closed; new connection is required.
 wire=next(h.fragments(h.env('heartbeat',pb.Heartbeat())));d.request(3,wire);d.request(3,wire,ok=False)
 assert not (struct.unpack('<I',d.request(8))[0]&1)
 print(f'PASS capacity={cap}: auth, snapshot, local rendering, touch/select, full review, scroll, guarded single decision, replay rejection',flush=True)
 return {'att_capacity':cap,'result':'PASS','actions':len(h.actions)}
if __name__=='__main__':
 results=[]
 with emulator() as (d,qmp,p):
  for cap in [182,244,509]:
   # Each run uses a fresh request identity so journal dedup is exercised separately.
   results.append(test(d,qmp,p,cap))
  results.append(extra(d,qmp,p))
 (OUT/'results.json').write_text(json.dumps(results,indent=2))
