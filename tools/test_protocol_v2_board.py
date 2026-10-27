#!/usr/bin/env python3
"""Physical injected-touch QA with the isolated Rust v2 fixture host."""
import argparse,json,os,time,subprocess,re,statistics
from pathlib import Path
from board_e2e import Rig,png
ROOT=Path(__file__).resolve().parents[1]
class V2Rig(Rig):
 def start_host(self):
  self.action_log=self.out/'actions.jsonl'
  self.action_log.unlink(missing_ok=True)
  self.host=subprocess.Popen([str(ROOT/'target/release/examples/protocol_board_qa'),str(self.action_log),'900'],stdout=self.host_log,stderr=self.host_log,env={**os.environ,'TC_BLE_REGISTRY':str(ROOT/'target/esp/v2-pairing/qa.json')})
 def events(self):
  if not self.action_log.exists():return []
  lines=self.action_log.read_text().splitlines();result=[]
  for line in lines:
   try:result.append(json.loads(line))
   except json.JSONDecodeError:pass
  return result
 def await_event(self,predicate,timeout=30):
  end=time.monotonic()+timeout
  while time.monotonic()<end:
   if self.host.poll() is not None:raise RuntimeError('Fixture host exited; inspect host.log')
   found=[e for e in self.events() if predicate(e)]
   if found:return found[-1]
   time.sleep(.02)
  raise TimeoutError('No expected semantic response')
 def capture(self,name):
  raw,meta=self.capture_bytes();(self.out/f'{name}.rgb565').write_bytes(raw);png(self.out/f'{name}.png',raw);return meta
 def action_count(self):return len([e for e in self.events() if 'kind' in e])
 def tap_expect(self,x,y,kind,session=None):
  old=self.events();last=max([e.get('id',0) for e in old] or [0]);down,up=self.tap(x,y)
  e=self.await_event(lambda e:e.get('id',0)>last and e.get('kind')==kind)
  assert e['status'] in [1,2],e
  if session is not None:assert e['session']==session,e
  self.await_event(lambda x:x.get('applied',0)>=e['revision']);return down,up,e

def main():
 p=argparse.ArgumentParser();p.add_argument('--port',default='/dev/cu.usbmodem2101');p.add_argument('--out',default='target/protocol-v2-board-qa');p.add_argument('--count',type=int,default=100);a=p.parse_args();out=Path(a.out).resolve();r=V2Rig(a.port,out)
 try:
  r.await_event(lambda e:e.get('applied')==1);r.mode(True);time.sleep(10);r.capture('01-cafe')
  seats=[(0,'qa-claude:0'),(1,'qa-claude:1'),(4,'qa-claude:2'),(5,'qa-claude:3'),(2,'qa-codex:4'),(3,'qa-codex:5'),(6,'qa-codex:6'),(7,'qa-codex:7')]
  for seat,id in seats:r.tap_expect(60+seat%4*120,90+seat//4*82,1,id);time.sleep(.5)
  r.capture('02-all-seats')
  r.tap_expect(60,90,1,'qa-claude:0');time.sleep(1)
  r.tap(200,240);time.sleep(.1);r.tap_expect(200,240,2,'qa-claude:0');time.sleep(1);r.capture('03-review')
  r.tap_expect(450,185,5);r.capture('04-scroll')
  time.sleep(.7);r.tap_expect(250,240,7,'qa-claude:0');time.sleep(1);r.capture('05-sent')
  r.tap_expect(30,20,3);time.sleep(.5)
  # Both provider identities: handoff, reopen before the synthetic OS opener
  # completes, then go back. Check the real LCD framebuffer after acknowledgments.
  for seat,ident in [(1,'qa-claude:1'),(2,'qa-codex:4')]:
   r.tap_expect(60+seat*120,90,1,ident);time.sleep(.5)
   r.tap(200,240);time.sleep(.1);r.tap_expect(200,240,2,ident)
   r.tap_expect(410,22,6,ident)
   r.tap(200,240);time.sleep(.1);r.tap_expect(200,240,2,ident)
   raw,_=r.capture_bytes()
   pixel=int.from_bytes(raw[(480+2)*2:(480+2)*2+2],'little')
   assert pixel==((0xef>>3)<<11|(0xeb>>2)<<5|(0xf7>>3)), 'pending handoff replaced the newer review'
   r.tap_expect(30,20,3)
   time.sleep(2.1)
  timings=[]
  for i in range(a.count):
   seat,id=seats[(i%7)+1];t=time.monotonic();down,up,e=r.tap_expect(60+seat%4*120,90+seat//4*82,1,id)
   timings.append({'down_ms':down,'up_ms':up,'host':e,'wall_ms':round((time.monotonic()-t)*1000,2)})
   time.sleep(.05)
  r.capture('06-after-stress');(out/'timings.json').write_text(json.dumps(timings,indent=2));(out/'results.json').write_text(json.dumps({'result':'PASS','seats':8,'selection_cycles':a.count,'synthetic_decisions':1,'captures':6,'provider_transition_cycles':2},indent=2));samples={'press_feedback_ms':[],'release_to_lcd_ms':[]}
  serial_text=(out/'serial.log').read_text()
  for a,b in re.findall(r'FEEDBACK input=(\d+) lcd=(\d+)',serial_text):samples['press_feedback_ms'].append(int(b)-int(a))
  for a,b in re.findall(r'LOCAL kind=0 input=(\d+) commit=\d+ lcd=(\d+)',serial_text):samples['release_to_lcd_ms'].append(int(b)-int(a))
  metrics={}
  for name,values in samples.items():
   if values:
    limit=150 if name=='press_feedback_ms' else 250
    assert min(values)>=0 and max(values)<=limit,(name,values)
    values.sort();metrics[name]={'samples':len(values),'median':statistics.median(values),'p95':values[int(.95*(len(values)-1))],'max':max(values)}
  (out/'latency.json').write_text(json.dumps(metrics,indent=2))
  print('PASS physical v2 fixture tests',metrics,flush=True)
 finally:
  try:r.mode(False)
  except Exception:pass
  if r.host:r.host.terminate();r.host.wait(timeout=5)
  r.closed=True;r.serial.close()
if __name__=='__main__':main()
