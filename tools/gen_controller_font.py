"""Rasterize the controller's portable sans-serif atlas. Requires Pillow."""
from pathlib import Path
import struct
from PIL import Image, ImageDraw, ImageFont
root=Path(__file__).resolve().parents[1]
raw=bytearray()
for size in (11,16):
    font=ImageFont.truetype(str(root/'assets/fonts/DejaVuSans.ttf'),size)
    for code in range(32,127):
        image=Image.new('L',(24,22))
        ImageDraw.Draw(image).text((0,size),chr(code),font=font,fill=255,anchor='ls')
        raw.append(round(font.getlength(chr(code))))
        raw.extend(image.tobytes())
(root/'assets/fonts/controller-alpha.bin').write_bytes(raw)
# Preserve the license notice embedded by the font author (TrueType name table).
b=(root/'assets/fonts/DejaVuSans.ttf').read_bytes()
for i in range(struct.unpack_from('>H',b,4)[0]):
    tag,_,offset,_=struct.unpack_from('>4sIII',b,12+16*i)
    if tag!=b'name':continue
    _,count,start=struct.unpack_from('>HHH',b,offset)
    texts=[]
    for j in range(count):
        platform,_,_,name,length,pos=struct.unpack_from('>HHHHHH',b,offset+6+12*j)
        if name==13:
            text=b[offset+start+pos:offset+start+pos+length].decode('utf-16-be' if platform in (0,3) else 'mac_roman')
            if text not in texts:texts.append(text)
    (root/'assets/fonts/LICENSE-DejaVu.txt').write_text('\n'.join(line.rstrip() for line in '\n\n'.join(texts).splitlines())+'\n')
print('Generated',len(raw),'bytes')
