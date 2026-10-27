"""Regression checks for source direction mapping and room-sized patron assets."""
import unittest
import xml.etree.ElementTree as ET
from PIL import Image
import build_patrons as build


class PatronAssets(unittest.TestCase):
    def test_room_scale_and_two_times_detail(self):
        for name in build.NAMES:
            source = build.authored(name) if name in build.NAMES[:8] or name == 'dragonite' else build.pmd(name)
            for detail in [1, 2]:
                cells, _ = build.normalize(source, detail)
                self.assertTrue(all(im.size == (48 * detail, 48 * detail) for im in cells), name)
                boxes = [build.bounds(im) for im in cells]
                self.assertGreaterEqual(max(max(b[3] - b[1] for b in boxes) / 42, max(b[2] - b[0] for b in boxes) / 45), 0.95 * detail, name)
                self.assertTrue(all(b[3] == 48 * detail for b in boxes), name)

    def test_pmd_horizontal_walks_use_the_direction_the_art_faces(self):
        # Verified visually against the original eight-row PMD sheets:
        # row 2 faces right, row 6 faces left.
        for name in build.NAMES[8:]:
            root = build.SOURCE / 'pmd' / name
            anim = next(a for a in ET.parse(root / 'AnimData.xml').findall('.//Anim') if a.findtext('Name') == 'Walk')
            w, h = int(anim.findtext('FrameWidth')), int(anim.findtext('FrameHeight'))
            count = len(anim.findall('./Durations/Duration'))
            atlas = Image.open(root / 'Walk-Anim.png').convert('RGBA')
            frames = build.pmd(name, walk_only=True)
            for start, row in [(20, 6), (24, 2)]:
                for i in range(4):
                    x = (i * count // 4) * w
                    expected = atlas.crop((x, row * h, x + w, (row + 1) * h))
                    self.assertEqual(frames[start - 12 + i].tobytes(), expected.tobytes(), (name, row, i))


if __name__ == '__main__':
    unittest.main()
