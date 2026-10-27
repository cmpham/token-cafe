#!/usr/bin/env python3
"""Build labeled review boards from cargo run --example patron_qa output."""
from PIL import Image,ImageDraw
from pathlib import Path
root=Path(__file__).resolve().parents[1] / 'target/patron-qa'
for roster in ['dragons','pokemon']:
 for pair in [('type','think'),('wave','sip'),('sleep','stretch')]:
  board=Image.new('RGB',(960,8*201+30),(22,19,30));d=ImageDraw.Draw(board)
  for col,(action,phase) in enumerate((a,p) for a in pair for p in ['a','b']):
   d.text((col*240+5,8),f'{roster} {action} {phase}',fill='white')
   im=Image.open(root/f'{roster}-{action}-{phase}.png')
   for seat in range(8):
    x=(seat%4)*120+20;y=48+(seat//4)*82
    k=im.width//480
    crop=im.crop((x*k,(y-5)*k,(x+80)*k,(y+62)*k)).resize((240,201),Image.Resampling.NEAREST)
    board.paste(crop,(col*240,seat*201+30))
  board.save(root/f'{roster}-{pair[0]}-{pair[1]}-board.png')
 for kind in ['arrival','departure']:
  board=Image.new('RGB',(960,960),(22,19,30));d=ImageDraw.Draw(board)
  indexes=[0,5,10,15,20,25,30,35,40,45,50,60]
  for i,n in enumerate(indexes):
   im=Image.open(root/f'{roster}-{kind}-{n:02}.png')
   x=i%3*320;y=i//3*240
   board.paste(im.resize((320,240),Image.Resampling.NEAREST),(x,y));d.text((x+3,y+225),f'{kind} {(n+1)/10:.1f}s',fill='white')
  board.save(root/f'{roster}-{kind}-board.png')

# Both gait phases in every direction, reviewed against the motion heading.
names=['clay','tsunami','glory','starflight','sunny','peril','winter','moonwatcher',
       'pikachu','eevee','bulbasaur','charmander','squirtle','jigglypuff','gengar','dragonite']
data=(root.parents[1]/'assets/patrons/poses-hd.rgba').read_bytes()
for half in [0,1]:
 board=Image.new('RGB',(768,960),(38,29,48));d=ImageDraw.Draw(board)
 for row in range(8):
  i=row+half*8;d.text((0,row*120),names[i],fill='white')
  for col,f in enumerate([12,13,16,17,20,21,24,25]):
   off=(i*28+f)*96*96*4
   im=Image.frombytes('RGBA',(96,96),data[off:off+96*96*4])
   board.paste(im,(96*col,120*row+24),im)
   d.text((96*col,120*row+12),['down A','down B','up A','up B','left A','left B','right A','right B'][col],fill='white')
 board.save(root/f'walk-directions-{half}.png')
