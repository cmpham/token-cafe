#!/usr/bin/env python3
"""Verify live native-app hit areas. Requires QA firmware and TC_TOUCH_QA=1 app.
Runs on actual sessions; double taps open their desktop sessions on macOS.
"""
import argparse,json,time
from pathlib import Path
from board_e2e import Rig,png
class LiveRig(Rig):
    def start_host(self):pass
    def capture(self,name):
        raw,_=self.capture_bytes()
        png(self.out/(name+'.png'),raw)

def main():
    p=argparse.ArgumentParser();p.add_argument('--port',required=True);p.add_argument('--log',required=True);p.add_argument('--out',required=True);args=p.parse_args()
    out=Path(args.out);r=LiveRig(args.port,out);log=Path(args.log);results=[]
    def events():return [json.loads(s.removeprefix('TOUCH_QA ')) for s in log.read_text().splitlines() if s.startswith('TOUCH_QA ')]
    def wait_event(n):
        end=time.monotonic()+10
        while time.monotonic()<end:
            e=events()
            if len(e)>n:return e[n]
            time.sleep(.05)
        raise AssertionError('Touch did not reach native app')
    def touch(x,y,double=False):
        r.mode(True);n=len(events())
        for _ in range(2 if double else 1):
            r.sample(1,x,y);time.sleep(.08);r.sample(0,x,y);time.sleep(.08)
        e=wait_event(n+(1 if double else 0))
        assert e['kind']==('double' if double else ('scroll' if x>=436 and y>=224 else 'tap'))
        return e
    try:
        initial=touch(60,91);seats=initial['seats'];(out/'seats.json').write_text(json.dumps(seats,indent=2))
        for row,(top,bottom) in enumerate([(48,135),(136,223)]):
            for col in range(4):
                seat=row*4+col;left=col*120;right=left+119
                for label,(x,y) in {'center':(left+60,(top+bottom)//2),'top-left':(left,top),'top-right':(right,top),'bottom-left':(left,bottom),'bottom-right':(right,bottom)}.items():
                    e=touch(x,y)
                    assert e['seats']==seats,'Live seating changed during test; rerun from fresh snapshot'
                    assert e['hit']==seats[seat]['id'],(seat,label,e)
                    assert (e['x'],e['y'])==(x,y)
                    results.append(dict(seat=seat,point=label,panel=[x,y],hit=e['hit'],target=e['target'],status='PASS'))
                # Capture a selected occupied seat, regardless of toggle parity.
                if seats[seat]['id']:
                    log_start=len(log.read_text())
                    e=touch(left+60,(top+bottom)//2,double=True)
                    assert e['seats']==seats,'Live seating changed before activation'
                    assert e['target']==seats[seat]['target'] and e['hit']==seats[seat]['id']
                    time.sleep(1.5)
                    if e['target']:
                        deadline=time.monotonic()+10
                        marker=f"opened session {e['target']}"
                        while marker not in log.read_text()[log_start:] and time.monotonic()<deadline:time.sleep(.05)
                        assert marker in log.read_text()[log_start:],'No fresh native session launch acknowledgment'
                    r.capture(f'seat-{seat+1}')
                    results.append(dict(seat=seat,point='double-open',panel=[left+60,(top+bottom)//2],hit=e['hit'],target=e['target'],status='PASS'))
                (out/'results.json').write_text(json.dumps(results,indent=2))
                print(f'PASS seat {seat+1}: {seats[seat]}',flush=True)
        for x,y in [(0,0),(479,0),(60,47),(180,47),(300,47),(420,47),(60,224),(180,224),(300,224),(479,271)]:
            e=touch(x,y);assert e['hit'] is None and e['target'] is None
            results.append(dict(point='outside-patrons',panel=[x,y],status='PASS'))
        r.capture('final-board')
        (out/'results.json').write_text(json.dumps(results,indent=2))
        print(json.dumps({'passed':len(results),'occupied':sum(s['id'] is not None for s in seats)}))
    finally:r.close()
if __name__=='__main__':main()
