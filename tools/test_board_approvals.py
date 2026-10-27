#!/usr/bin/env python3
"""Injected-touch approval UI tests; never decides a real provider request."""
import json, time
from pathlib import Path
from board_e2e import Rig

r=Rig('/dev/cu.usbmodem2101',Path('target/board-approvals'))
results=[]
try:
    r.reset(approval=True,targets=True)
    r.expected('click',x=60,y=80);r.tap(60,80)
    results.append(r.snap('01-preview'))
    r.expected('review',double=True,x=200,y=240)
    r.tap(200,240);time.sleep(.08);r.tap(200,240)
    results.append(r.snap('02-full-question'))
    r.expected('review',x=450,y=194);r.tap(450,194)
    results.append(r.snap('03-scroll-question'))
    r.expected('review',x=250,y=240,trusted=True);r.tap(250,240)
    results.append(r.snap('04-approve-once'))
    # Repeated taps must not change the frozen sending state.
    r.tap(250,240)
    results.append(r.snap('05-duplicate-ignored'))
    r.reset(approval=True,targets=True)
    r.command('both',action='focus',id='qa-0')
    r.command('both',action='review',double=True,x=200,y=240);r.wait()
    r.expected('review',x=50,y=240,trusted=True);r.tap(50,240)
    results.append(r.snap('06-deny'))
    r.reset(approval=True,targets=True)
    r.command('both',action='focus',id='qa-0')
    r.command('both',action='review',double=True,x=200,y=240);r.wait()
    r.command('both',action='expire');r.wait();r.tap(250,240)
    results.append(r.snap('07-expired-no-decision'))
    r.expected('review',double=True,x=200,y=130)
    r.tap(200,130);time.sleep(.08);r.tap(200,130)
    results.append(r.snap('08-desktop-handoff'))
    assert r.command('state')['activations']==['claude://code/continue?session=local_qa']
    r.reset(approval=True)
    r.command('both',action='focus',id='qa-0')
    r.command('both',action='review',double=True,x=200,y=240);r.wait()
    r.expected('review',x=40,y=20);r.tap(40,20)
    results.append(r.snap('09-back-to-cafe'))
    r.reset(targets=True)
    r.expected('click',x=210,y=252);r.expected('review',double=True,x=210,y=252)
    r.tap(210,252);r.tap(210,252)
    results.append(r.snap('10-double-footer-opens-review'))
    assert not r.command('state')['activations']
    (r.out/'results.json').write_text(json.dumps(results,indent=2))
    print(json.dumps({'passed':len(results)}))
finally:
    r.close()
