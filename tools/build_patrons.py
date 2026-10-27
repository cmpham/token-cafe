#!/usr/bin/env python3
"""Compile authored pose atlases / PMDCollab animations into runtime frames.
Pillow is needed only at build time; originals and attribution remain intact.
"""
from pathlib import Path
import xml.etree.ElementTree as ET
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'assets/patrons/source'
NAMES = ['clay', 'tsunami', 'glory', 'starflight', 'sunny', 'peril', 'winter', 'moonwatcher',
         'pikachu', 'eevee', 'bulbasaur', 'charmander', 'squirtle', 'jigglypuff', 'gengar', 'dragonite']
# Six two-frame seated clips, then four four-frame walking directions.
CLIPS = ['type', 'think', 'wave', 'sip', 'sleep', 'stretch', 'down', 'up', 'left', 'right']
FRAMES = 28
CELL = 48


def bounds(im):
    box = im.getchannel('A').point(lambda a: 255 if a >= 128 else 0).getbbox()
    assert box, 'empty animation frame'
    return box


def authored(name):
    atlas = Image.open(SOURCE / 'poses' / f'{name}.png').convert('RGBA')
    # Generated rows have unequal margins. Split at verified transparent
    # gutters, not image.height/4 (which can include a neighbor's horns).
    alpha = atlas.getchannel('A')
    gaps, start = [], None
    for y in range(atlas.height + 1):
        empty = y < atlas.height and sum(v >= 128 for v in alpha.crop((0, y, atlas.width, y + 1)).get_flattened_data()) < 8
        if empty and start is None:
            start = y
        elif not empty and start is not None:
            if y - start >= 4:
                gaps.append((start, y))
            start = None
    assert len(gaps) == 5, (name, 'expected four separated rows', gaps)
    images = []
    for row in range(4):
        top, bottom = gaps[row][1], gaps[row + 1][0]
        for col in range(4):
            images.append(atlas.crop((round(col * atlas.width / 4), top, round((col + 1) * atlas.width / 4), bottom)))
    if name == 'dragonite':
        # The authored front view is almost symmetrical and reads as sliding
        # sideways. Use the existing four-direction PMD gait, matching source
        # height before the shared character normalization.
        walks = pmd(name, walk_only=True)
        factor = max(bounds(im)[3] - bounds(im)[1] for im in images[12:14]) / max(bounds(im)[3] - bounds(im)[1] for im in walks)
        walks = [im.resize((round(im.width * factor), round(im.height * factor)), Image.Resampling.NEAREST) for im in walks]
        return images[:12] + walks
    # Front 3/4 walk doubles as side walk; rear art is used for upward travel.
    return images[:12] + [images[i] for i in [12, 13, 12, 13, 14, 15, 14, 15]] + [images[i].transpose(Image.Transpose.FLIP_LEFT_RIGHT) for i in [12, 13, 12, 13]] + [images[i] for i in [12, 13, 12, 13]]


def pmd(name, walk_only=False):
    root = SOURCE / 'pmd' / name
    animations = {a.findtext('Name'): a for a in ET.parse(root / 'AnimData.xml').findall('.//Anim')}

    def frames(action, direction=0):
        a = animations[action]
        if a.findtext('CopyOf'):
            return frames(a.findtext('CopyOf'), direction)
        w, h = int(a.findtext('FrameWidth')), int(a.findtext('FrameHeight'))
        im = Image.open(root / f'{action}-Anim.png').convert('RGBA')
        row = direction if im.height // h == 8 else 0
        return [im.crop((i * w, row * h, (i + 1) * w, (row + 1) * h)) for i in range(len(a.findall('./Durations/Duration')))]

    def pair(action, indexes):
        fs = frames(action)
        return [fs[min(i, len(fs) - 1)] for i in indexes]

    # PMD Sit is rear-only. Use front-facing leaning Nod art behind the desk,
    # with articulated keyboard paws supplied by the scene's foreground pass.
    result = [] if walk_only else (pair('Nod', [0, 1]) + pair('Nod', [1, 2]) + pair('Pose', [0, 1]) +
              pair('Eat', [1, 3]) + pair('Sleep', [0, 1]) + pair('DeepBreath', [0, 1]))
    # PMD direction rows: down, down-right, right, up-right, up, up-left, left, down-left.
    for direction in [0, 4, 6, 2]:
        fs = frames('Walk', direction)
        result += [fs[i * len(fs) // 4] for i in range(4)]
    return result


def normalize(images, resolution=1):
    boxes = [bounds(im) for im in images]
    # One scale for the entire character, never inflate sleeping frames.
    scale = resolution * min(42 / max(b[3] - b[1] for b in boxes), 45 / max(b[2] - b[0] for b in boxes))
    cells, transforms = [], []
    for i, (im, box) in enumerate(zip(images, boxes)):
        # Shared horizontal anchor for each clip; bottom contact stays on row 47.
        start, length = ((i // 2) * 2, 2) if i < 12 else (12 + ((i - 12) // 4) * 4, 4)
        group = boxes[start:start + length]
        # Input frame sizes may vary by action but are fixed within each clip.
        center = (min(b[0] for b in group) + max(b[2] for b in group)) / 2
        cropped = im.crop(box)
        size = (max(1, round(cropped.width * scale)), max(1, round(cropped.height * scale)))
        cropped = cropped.resize(size, Image.Resampling.NEAREST)
        cell = Image.new('RGBA', (CELL * resolution, CELL * resolution))
        origin = (round(CELL / 2 * resolution + (box[0] - center) * scale), CELL * resolution - size[1])
        cell.paste(cropped, origin)
        cells.append(cell)
        transforms.append((scale, box, origin))
    return cells, transforms


def main():
    all_frames = []
    hd_frames = []
    cup_anchors = bytearray()
    contact = Image.new('RGBA', (CELL * FRAMES, (CELL + 8) * len(NAMES)), (38, 29, 48, 255))
    draw = ImageDraw.Draw(contact)
    for row, name in enumerate(NAMES):
        source = authored(name) if row < 8 or name == 'dragonite' else pmd(name)
        cells, transforms = normalize(source)
        hd_frames.extend(normalize(source, 2)[0])
        if 8 <= row < 15:
            root = SOURCE / 'pmd' / name
            anim = next(a for a in ET.parse(root / 'AnimData.xml').findall('.//Anim') if a.findtext('Name') == 'Eat')
            w, h = int(anim.findtext('FrameWidth')), int(anim.findtext('FrameHeight'))
            offsets = Image.open(root / 'Eat-Offsets.png').convert('RGBA')
            for frame, source_frame in [(6, 1), (7, 3)]:
                points = [(x, y) for y in range(h) for x in range(w)
                          if offsets.getpixel((source_frame * w + x, y)) == (0, 0, 0, 255)]
                assert len(points) == 1, (name, frame, 'head anchor')
                scale, box, origin = transforms[frame]
                x, y = points[0]
                cup_anchors.extend((round(origin[0] + (x - box[0]) * scale), round(origin[1] + (y - box[1]) * scale)))
        else:
            cup_anchors.extend((0, 0, 0, 0))
        assert len(cells) == FRAMES
        for pose in range(6):
            assert cells[pose * 2].tobytes() != cells[pose * 2 + 1].tobytes(), (name, CLIPS[pose], 'identical frames')
        for i, cell in enumerate(cells):
            contact.alpha_composite(cell, (i * CELL, row * (CELL + 8) + 8))
        draw.text((0, row * (CELL + 8)), name, fill='white')
        all_frames.extend(cells)
    (ROOT / 'assets/patrons/poses-hd.rgba').write_bytes(b''.join(im.tobytes() for im in hd_frames))
    (ROOT / 'assets/patrons/cup-anchors.bin').write_bytes(cup_anchors)
    (ROOT / 'assets/patrons/poses.rgba').write_bytes(b''.join(im.tobytes() for im in all_frames))
    contact.resize((CELL * FRAMES * 2, (CELL + 8) * len(NAMES) * 2), Image.Resampling.NEAREST).save(ROOT / 'docs/patron-poses.png')
    preview = Image.new('RGBA', (CELL * 8, CELL * 2), (38, 29, 48, 255))
    for i in range(16):
        preview.alpha_composite(all_frames[i * FRAMES], ((i % 8) * CELL, (i // 8) * CELL))
    preview.resize((1024, 256), Image.Resampling.NEAREST).save(ROOT / 'docs/patrons.png')


if __name__ == '__main__':
    main()
