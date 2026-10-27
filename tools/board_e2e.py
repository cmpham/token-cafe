#!/usr/bin/env python3
"""Hardware suite. Requires diagnostic QA firmware, pyserial, built board_e2e example.
Usage: firmware Python tools/board_e2e.py --port /dev/cu.usbmodem2101
All artifacts stay local; restore production firmware after running.
"""
import argparse, json, math, queue, re, statistics, struct, subprocess, threading, time, zlib
from pathlib import Path
import serial


def png(path, raw):
    rows=[]
    for y in range(272):
        row=bytearray([0])
        for x in range(480):
            p=int.from_bytes(raw[(y*480+x)*2:(y*480+x)*2+2], 'little')
            r,g,b=p>>11,(p>>5)&63,p&31
            row.extend(((r<<3)|(r>>2),(g<<2)|(g>>4),(b<<3)|(b>>2)))
        rows.append(row)
    def chunk(kind,data):
        return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data))
    path.write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',480,272,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(b''.join(rows)))+chunk(b'IEND',b''))


class Rig:
    def __init__(self,port,out):
        self.out=out;out.mkdir(parents=True,exist_ok=True)
        self.serial=serial.Serial(port,115200,timeout=.2);self.messages=queue.Queue();self.frames=[];self.logs=[];self.seq=0
        self.closed=False
        self.serial_log=open(out/'serial.log','w',buffering=1)
        self.host_log=open(out/'host.log','w',buffering=1)
        self.host=None
        threading.Thread(target=self.reader,daemon=True).start()
        self.start_host()
    def reader(self):
        while not self.closed:
            try: line=self.serial.readline().decode(errors='replace').strip()
            except serial.SerialException: return
            if not line: continue
            self.serial_log.write(line+'\n')
            m=re.search(r'\((\d+)\) BLE: FRAME (\d+) CRC=([0-9a-f]+)',line)
            if m: self.frames.append((int(m[1]),int(m[2]),m[3]))
            if line.startswith('@QA '): self.messages.put(line)
    def start_host(self):
        self.replies=queue.Queue();self.verified=[]
        self.host=subprocess.Popen(['target/release/examples/board_e2e',str(self.out)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1)
        host,replies,verified=self.host,self.replies,self.verified
        def stdout():
            for line in host.stdout:
                try:replies.put(json.loads(line))
                except ValueError:pass
        def stderr():
            for line in host.stderr:
                self.host_log.write(line);self.logs.append(line)
                m=re.search(r'verified frame CRC=([0-9a-f]+).*input=(\d+)',line)
                if m:verified.append((m[1],int(m[2])))
        threading.Thread(target=stdout,daemon=True).start();threading.Thread(target=stderr,daemon=True).start()
        assert self.replies.get(timeout=15)['ready']
    def command(self,cmd,**kw):
        self.seq+=1
        self.host.stdin.write(json.dumps(dict(cmd=cmd,request=self.seq,**kw))+'\n');self.host.stdin.flush()
        while True:
            reply=self.replies.get(timeout=15)
            if reply.get('id')==self.seq:return reply
    def usb(self,command):
        self.seq+=1;ident=self.seq
        self.serial.write(f'QA {command.format(id=ident)}\n'.encode());self.serial.flush()
        return ident
    def ack(self,ident,timeout=10):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            line=self.messages.get(timeout=max(.1,end-time.monotonic()))
            if line.startswith('@QA ERROR'):raise AssertionError(line)
            if line.startswith(f'@QA INPUT {ident} '):return int(line.split()[3])
        raise TimeoutError('USB sample acknowledgment')
    def mode(self,on):return self.ack(self.usb('MODE {id} '+str(int(on))))
    def sample(self,count,x=0,y=0):return self.ack(self.usb(f'IN {{id}} {count} {x} {y}'))
    def point(self,x,y):return x,y
    def tap(self,x,y,hold=.08):
        x,y=self.point(x,y);down=self.sample(1,x,y);time.sleep(hold);up=self.sample(0,x,y);return down,up
    def expected(self,action,**kw):return self.command('expect',action=action,**kw)
    def wait(self,minimum=None,timeout=20):
        end=time.monotonic()+timeout
        start=len(self.verified)
        while time.monotonic()<end:
            s=self.command('state')
            if s['actual']==s['expected'] and (minimum is None or s['inputs']>=minimum) and len(self.verified)>=start+2 and self.verified[-2:]==[(s['expected'],s['inputs'])]*2:return s
            time.sleep(.05)
        raise AssertionError(f'Host/model or BLE verification did not converge: {s}')
    def capture_bytes(self):
        ident=self.usb('SNAP {id}');raw=bytearray();metadata=None;ended=False
        end=time.monotonic()+45
        while time.monotonic()<end:
            line=self.messages.get(timeout=max(.1,end-time.monotonic()))
            if line.startswith(f'@QA SNAP {ident} '):metadata=line.split()
            elif line.startswith(f'@QA DATA {ident} '):
                _,_,_,offset,data=line.split();assert int(offset)==len(raw),'Capture offset gap';raw.extend(bytes.fromhex(data))
            elif line==f'@QA END {ident}':ended=True;break
        assert ended and metadata and int(metadata[6])==261120 and len(raw)==261120,'Incomplete board snapshot'
        assert f'{zlib.crc32(raw):08x}'==metadata[5],'Capture CRC mismatch'
        return raw,metadata
    def snap(self,name,minimum=None,release=None):
        state=self.wait(minimum);expected=(self.out/'expected.rgb565').read_bytes();actual=(self.out/'host.rgb565').read_bytes()
        assert expected==actual,'Host differs from independent action model'
        raw,metadata=self.capture_bytes()
        png(self.out/(name+'.png'),raw);(self.out/(name+'.rgb565')).write_bytes(raw)
        if raw!=expected:
            png(self.out/(name+'-expected.png'),expected)
            raise AssertionError(f'Board pixels differ: {sum(a!=b for a,b in zip(raw,expected))} bytes')
        result={'case':name,'status':'PASS','crc':metadata[5],'inputs':state['inputs'],'board_frame':int(metadata[3]),'capture':name+'.png','expected_crc':state['expected'],'host_crc':state['actual'],'compared_bytes':len(raw)}
        if release is not None:
            times=[t for t,_,crc in self.frames if t>=release and crc==state['expected']]
            assert times,'No matching LCD completion timestamp';result['release_to_lcd_ms']=min(times)-release
        return result
    def reset(self,**kw):
        self.mode(True);self.command('reset',**kw);return self.wait()
    def close(self):
        try:self.mode(False)
        except Exception:pass
        if self.host and self.host.poll() is None:
            self.host.stdin.write('{"cmd":"quit"}\n');self.host.stdin.flush()
            try:self.host.wait(timeout=5)
            except subprocess.TimeoutExpired:self.host.kill();self.host.wait()
        self.closed=True;self.serial.close()


def suite(r):
    results=[]
    def case(name,fn):
        try:
            result=fn()
            if result is None:result=r.snap(name)
            result['case']=name
        except Exception as e:result={'case':name,'status':'FAIL','error':str(e)}
        results.append(result);(r.out/'results.json').write_text(json.dumps(results,indent=2));print(json.dumps(result),flush=True)
    def fresh():r.reset()
    case('01-initial-full-sync',lambda:(fresh(),r.snap('01-initial-full-sync'))[1])
    for i,(x,y) in enumerate([(60,80),(180,80),(300,80),(420,80),(60,160),(180,160),(300,160),(420,160)]):
        def selection(i=i,x=x,y=y):
            fresh();before=r.command('state')['inputs'];r.expected('click',x=x,y=y);_,up=r.tap(x,y)
            return r.snap(f'select-{i}',before+1,up)
        case(f'02-select-seat-{i}',selection)
    def toggle():
        fresh();r.expected('click',x=60,y=80);r.tap(60,80);r.wait();r.expected('click',x=60,y=80);r.tap(60,80)
        return r.snap('03-toggle-off')
    case('03-toggle-off',toggle)
    def hold():
        fresh();base=r.command('state')['inputs'];x,y=r.point(60,80);r.sample(1,x,y);time.sleep(.5)
        assert r.command('state')['inputs']==base,'Long hold activated before release'
        r.snap('04-held-no-activation');r.expected('click',x=60,y=80);up=r.sample(0,x,y)
        return r.snap('04-long-hold-release',base+1,up)
    case('04-long-hold-release',hold)
    for name,samples in [
        ('05-nonrail-drag',[(1,60,90),(1,100,130),(0,100,130)]),
        ('06-multitouch',[(1,60,90),(2,60,90),(0,60,90)]),
        ('07-out-of-range',[(1,480,272),(0,480,272)]),
        ('08-release-alone',[(0,60,90)]),
        ('09-move-outside',[(1,60,90),(1,65535,65535),(0,65535,65535)]),
    ]:
        def negative(name=name,samples=samples):
            fresh();base=r.command('state')['inputs']
            for sample in samples:r.sample(*sample)
            time.sleep(.4);assert r.command('state')['inputs']==base,'Rejected gesture emitted input'
            return r.snap(name)
        case(name,negative)
    def footer():
        fresh();r.expected('click',x=60,y=80);r.tap(60,80);r.wait();r.expected('click',x=210,y=252);_,up=r.tap(210,252)
        return r.snap('10-footer-next',release=up)
    case('10-footer-next',footer)
    def scroll():
        fresh();r.expected('click',x=60,y=80);r.tap(60,80);r.wait();r.expected('scroll',y=271)
        x,y=r.point(470,252);r.sample(1,x,y);x,y=r.point(470,271);r.sample(1,x,y);r.sample(0,x,y)
        r.snap('11-scroll-last');r.expected('scroll',y=224);x,y=r.point(470,212);r.sample(1,x,y);r.sample(0,x,y)
        return r.snap('11-scroll-first')
    case('11-scroll-endpoints',scroll)
    def open_scroll():
        fresh();r.expected('next');r.expected('scroll',y=271);x,y=r.point(470,271);r.sample(1,x,y);r.sample(0,x,y)
        return r.snap('12-scroll-opens-focus')
    case('12-scroll-opens-focus',open_scroll)
    for name,config,action in [
        ('13-empty',{'count':0},None),('14-one-long-name',{'count':1,'long':True},('focus',{'id':'qa-0'})),
        ('15-overflow',{'count':10},('focus',{'id':'qa-9'})),('16-missing-task',{},('focus',{'id':'qa-2'})),
        ('17-unicode',{},('focus',{'id':'qa-3'})),('18-attention',{'attention':True},None),
        ('19-manual-with-attention',{'attention':True},('focus',{'id':'qa-0'})),
    ]:
        def content(name=name,config=config,action=action):
            r.reset(**config)
            if action:r.command('both',action=action[0],**action[1])
            return r.snap(name)
        case(name,content)
    def replace():
        fresh();r.command('both',action='focus',id='qa-0');r.command('both',action='scroll',y=271);r.wait();r.command('both',action='replace')
        return r.snap('20-task-replaced')
    case('20-task-replaced',replace)
    def remove():
        fresh();r.command('both',action='focus',id='qa-0');r.wait();r.command('both',action='remove');return r.snap('21-selected-removed')
    case('21-selected-removed',remove)
    def burst():
        fresh();base=r.command('state')['inputs']
        for i in range(12):
            x=60+(i%4)*120;y=80 if i%8<4 else 160;r.expected('click',x=x,y=y);r.tap(x,y,hold=0)
        return r.snap('22-rapid-12-taps',base+12)
    case('22-rapid-12-taps',burst)
    def cycles():
        fresh();base=r.command('state')['inputs']
        for _ in range(10):
            r.expected('click',x=60,y=80);r.tap(60,80,hold=.02);r.wait()
        return r.snap('23-repeat-toggle-10',base+10)
    case('23-repeat-toggle-10',cycles)
    def reconnect():
        r.host.stdin.write('{"cmd":"quit"}\n');r.host.stdin.flush();r.host.wait(timeout=5);time.sleep(2);r.start_host();r.mode(True);r.wait(timeout=30)
        return r.snap('24-host-reconnect')
    case('24-host-reconnect',reconnect)
    def post():
        r.expected('click',x=60,y=80);_,up=r.tap(60,80);return r.snap('25-touch-after-reconnect',release=up)
    case('25-touch-after-reconnect',post)
    def corners():
        fresh()
        for x,y in [(0,0),(319,0),(0,271),(319,271)]:
            base=r.command('state')['inputs'];r.expected('click',x=x,y=y);r.tap(x,y);r.wait(base+1)
        return r.snap('26-four-corners')
    case('26-four-corners',corners)
    def clamp():
        fresh();r.command('both',action='focus',id='qa-0');r.command('both',action='scroll',y=271);r.wait()
        base=r.command('state')['inputs']
        for _ in range(3):
            x,y=r.point(470,271);r.sample(1,x,y);r.sample(0,x,y)
        return r.snap('27-scroll-clamped',base+3)
    case('27-scroll-clamped',clamp)
    def rail_cancel():
        fresh();r.command('both',action='focus',id='qa-0');r.wait();r.expected('scroll',y=252)
        x,y=r.point(470,252);r.sample(1,x,y);r.sample(1,65535,65535);r.sample(0,65535,65535)
        return r.snap('28-scroll-leaves-panel')
    case('28-scroll-leaves-panel',rail_cancel)
    def invalid_commands():
        fresh();base=r.command('state')['inputs']
        for command in ['IN {id} 3 20 20','IN {id} 1 999999 20','MODE {id} 2','BOGUS {id}']:
            r.usb(command)
            line=r.messages.get(timeout=5)
            assert line.startswith('@QA ERROR'),line
        assert r.command('state')['inputs']==base
        return r.snap('29-invalid-usb-commands')
    case('29-invalid-usb-commands',invalid_commands)
    def held_reconnect():
        fresh();x,y=r.point(60,80);r.sample(1,x,y)
        r.host.stdin.write('{"cmd":"quit"}\n');r.host.stdin.flush();r.host.wait(timeout=5);time.sleep(2)
        r.start_host();r.wait(timeout=30)
        # Deliberately do not reset diagnostic mode: the connection must cancel the old gesture.
        r.sample(0,x,y);time.sleep(.5)
        assert r.command('state')['inputs']==0,'Stale held gesture leaked into the reconnected session'
        return r.snap('30-held-across-reconnect')
    case('30-held-across-reconnect',held_reconnect)
    def overflow_next():
        r.reset(count=10);r.command('both',action='focus',id='qa-7');r.wait()
        r.expected('click',x=210,y=252);r.tap(210,252);return r.snap('31-footer-overflow')
    case('31-footer-overflow',overflow_next)
    def all_closed():
        fresh();r.command('both',action='remove');r.command('both',action='tick',steps=300);return r.snap('32-removal-animation-complete')
    case('32-removal-animation-complete',all_closed)
    def unicode_last():
        fresh();r.command('both',action='focus',id='qa-3');r.wait();r.expected('scroll',y=271)
        x,y=r.point(470,271);r.sample(1,x,y);r.sample(0,x,y);return r.snap('33-unicode-last-page')
    case('33-unicode-last-page',unicode_last)
    def attention_rotate():
        r.reset(attention=True);r.command('both',action='tick',steps=120);return r.snap('34-attention-rotation')
    case('34-attention-rotation',attention_rotate)
    def idle_soak():
        fresh();start=time.monotonic();initial=r.command('state')['actual'];time.sleep(60)
        assert r.command('state')['actual']==initial
        result=r.snap('35-idle-60s');result['duration_s']=round(time.monotonic()-start,2);return result
    case('35-idle-60s',idle_soak)
    def double_case(index,x,url):
        r.reset(targets=True);base=r.command('state')['inputs']
        r.expected('focus',id=f'qa-{index}')
        r.tap(x,80);time.sleep(.08);r.tap(x,80)
        result=r.snap(f'36-double-{index}',base+2)
        assert r.command('state')['activations']==[url], 'Expected exactly one desktop activation'
        return result
    case('36-double-claude',lambda:double_case(0,60,'claude://code/continue?session=local_qa'))
    case('37-double-codex',lambda:double_case(2,300,'codex://threads/qa-codex'))
    def unlinked():
        r.reset(targets=True);base=r.command('state')['inputs'];r.expected('focus',id='qa-1')
        r.tap(180,80);r.tap(180,80)
        result=r.snap('38-double-unlinked',base+2)
        assert not r.command('state')['activations']
        return result
    case('38-double-unlinked',unlinked)
    def slow():
        r.reset(targets=True);base=r.command('state')['inputs']
        r.tap(60,80);time.sleep(.55);r.tap(60,80)
        result=r.snap('39-slow-taps-toggle',base+2)
        assert not r.command('state')['activations']
        return result
    case('39-slow-taps-toggle',slow)
    def different():
        r.reset(targets=True);base=r.command('state')['inputs'];r.expected('focus',id='qa-1')
        r.tap(60,80);r.tap(180,80)
        result=r.snap('40-different-patrons',base+2)
        assert not r.command('state')['activations']
        return result
    case('40-different-patrons',different)
    def double_footer():
        r.reset(targets=True);base=r.command('state')['inputs']
        r.expected('click',x=210,y=252);r.expected('review',double=True,x=210,y=252)
        r.tap(210,252);r.tap(210,252)
        result=r.snap('41-double-footer',base+2)
        assert not r.command('state')['activations']
        return result
    case('41-double-footer',double_footer)
    def broken_pair():
        r.reset(targets=True);base=r.command('state')['inputs']
        r.tap(60,80)
        r.sample(1,60,91);r.sample(2,60,91);r.sample(0,60,91)
        r.tap(60,80)
        result=r.snap('42-multitouch-breaks-pair',base+2)
        assert not r.command('state')['activations']
        return result
    case('42-multitouch-breaks-pair',broken_pair)
    return results


def main():
    p=argparse.ArgumentParser();p.add_argument('--port',required=True);p.add_argument('--out',default='target/board-e2e');args=p.parse_args()
    r=Rig(args.port,Path(args.out))
    try:
        r.mode(True);results=suite(r)
        values=[v['release_to_lcd_ms'] for v in results if 'release_to_lcd_ms' in v]
        summary={'cases':len(results),'passed':sum(v['status']=='PASS' for v in results),'failed':sum(v['status']=='FAIL' for v in results),
                 'latency_ms':{'n':len(values),'median':statistics.median(values) if values else None,'p95':sorted(values)[math.ceil(len(values)*.95)-1] if values else None,'max':max(values) if values else None},
                 'transport_errors':[line for line in r.logs if 'mismatch' in line or 'reconnecting' in line]}
        if values:summary['latency_target_pass']=summary['latency_ms']['p95']<=1500 and max(values)<=2000
        (r.out/'summary.json').write_text(json.dumps(summary,indent=2));print(json.dumps(summary),flush=True)
    finally:r.close()
    if summary['failed'] or not summary.get('latency_target_pass',False) or summary['transport_errors']:
        raise SystemExit(1)


if __name__=='__main__':main()
