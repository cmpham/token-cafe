#!/usr/bin/env python3
"""Bounded long-text render/heap profile in the actual Xtensa engine."""
import json,struct,sys
from test_protocol_v2_qemu import Host,fixture,emulator,pb,sha,OUT

def profile(d,qmp,process):
 results=[]
 for name,body in [('short',b'Short task notes'),('32k-ascii',b'x'*32768),('32k-controls',b'\t'*32768),('32k-nul',b'\0'*32768),('32k-newlines',b'\n'*32768)]:
  h=Host(d,244);sessions,_=fixture(5000)
  for s in sessions:
   s.ClearField('approval');s.status=1;s.waiting_reason=0
   s.task_blob.CopyFrom(pb.BlobRef(sha256=sha(body),byte_length=len(body)))
  h.snapshot(sessions,pb.View(page=1));h.advance(10000)
  h.tap(60,90);h.body(body)
  cafe=struct.unpack('<5I',d.request(11))
  h.advance(500);h.tap(200,240);h.advance(100);h.tap(200,240)
  samples=[struct.unpack('<5I',d.request(11)) for _ in range(12)]
  assert samples[0][:2]==samples[-1][:2], 'heap usage or largest free block changed across identical renders'
  assert max(s[3] for s in samples)<50000, 'review render exceeded the QEMU 50ms budget'
  results.append(dict(case=name,cafe=dict(zip(['free','largest','minimum','draw_us','pack_us'],cafe)),review_samples=[dict(zip(['free','largest','minimum','draw_us','pack_us'],s)) for s in samples]))
  h.present(f'perf-{name}',qmp,process)
 return results
if __name__=='__main__':
 with emulator() as args:results=profile(*args)
 path=sys.argv[1] if len(sys.argv)>1 else str(OUT/'performance.json')
 with open(path,'w') as f:json.dump(results,f,indent=2)
 print(json.dumps(results),flush=True)
