#!/usr/bin/env python3
"""Build labeled review boards from cargo run --example patron_qa output."""
from PIL import Image,ImageDraw
from pathlib import Path
root=Path(__file__).resolve().parents[1] / 'target/patron-qa'
for roster in ['dragons','pokemon']:
 for pair in [('type','think'),('wave','sip'),('sleep','stretch')]:
  board=Image.new('RGB',(960,8*165+30),(22,19,30));d=ImageDraw.Draw(board)
  for col,(action,phase) in enumerate((a,p) for a in pair for p in ['a','b']):
   d.text((col*240+5,8),f'{roster} {action} {phase}',fill='white')
   im=Image.open(root/f'{roster}-{action}-{phase}.png')
   for seat in range(8):
    x=(seat%4)*120+20;y=48+(seat//4)*82
    k=im.width//480
    crop=im.crop((x*k,(y+7)*k,(x+80)*k,(y+62)*k)).resize((240,165),Image.Resampling.NEAREST)
    board.paste(crop,(col*240,seat*165+30))
  board.save(root/f'{roster}-{pair[0]}-{pair[1]}-board.png')
 for kind in ['arrival','departure']:
  board=Image.new('RGB',(960,960),(22,19,30));d=ImageDraw.Draw(board)
  indexes=[0,5,10,15,20,25,30,35,40,45,50,60]
  for i,n in enumerate(indexes):
   im=Image.open(root/f'{roster}-{kind}-{n:02}.png')
   x=i%3*320;y=i//3*240
   board.paste(im.resize((320,240),Image.Resampling.NEAREST),(x,y));d.text((x+3,y+225),f'{kind} {(n+1)/10:.1f}s',fill='white')
  board.save(root/f'{roster}-{kind}-board.png')
