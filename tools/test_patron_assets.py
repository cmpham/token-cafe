"""Regression checks for source direction mapping and room-sized patron assets."""
import unittest
import xml.etree.ElementTree as ET
from PIL import Image
import build_patrons as build


class PatronAssets(unittest.TestCase):
    def test_board_coverage_uses_hd_samples_without_transparent_color_halos(self):
        hd = Image.new('RGBA', (144, 144), (255, 0, 255, 0))
        hd.putpixel((0, 0), (255, 0, 0, 255))
        hd.putpixel((1, 0), (0, 255, 0, 127))  # Below the desktop visibility cutoff.
        hd.paste((255, 255, 255, 255), (3, 0, 6, 3))
        board = build.native_frame(hd)
        self.assertEqual(board.size, (48, 48))
        self.assertEqual(board.getpixel((0, 0)), (255, 0, 0, 28))
        self.assertEqual(board.getpixel((1, 0)), (255, 255, 255, 255))
        self.assertEqual(board.getpixel((2, 0)), (0, 0, 0, 0))

    def test_board_and_portrait_atlases_resolve_every_three_times_frame(self):
        board = (build.ROOT / 'assets/patrons/poses.rgba').read_bytes()
        portraits = (build.ROOT / 'assets/patrons/portraits.rgba').read_bytes()
        self.assertEqual(len(board), 16 * 28 * 48 * 48 * 4)
        self.assertEqual(len(portraits), 16 * 2 * 32 * 32 * 4)
        antialiased = 0
        for character, name in enumerate(build.NAMES):
            source = build.authored(name) if character < 8 or name == 'dragonite' else build.pmd(name)
            hd, _ = build.normalize(source, build.SOURCE_DETAIL, pixel_art=8 <= character < 15)
            for pose, image in enumerate(hd):
                frame = character * 28 + pose
                offset = frame * 48 * 48 * 4
                resolved = build.native_frame(image)
                self.assertEqual(board[offset:offset + 48 * 48 * 4], resolved.tobytes(), frame)
                antialiased += any(0 < a < 255 for a in resolved.getchannel('A').get_flattened_data())
                if pose in [4, 5]:
                    offset = (character * 2 + pose - 4) * 32 * 32 * 4
                    self.assertEqual(portraits[offset:offset + 32 * 32 * 4], build.native_frame(image, 32).tobytes(), frame)

        # A frame aligned exactly to the native grid legitimately needs no partial coverage.
        self.assertGreater(antialiased, 16 * 12)

    def test_scale2x_refines_diagonals_without_blur_or_palette_changes(self):
        dark, light = (0, 0, 0, 255), (255, 255, 255, 255)
        image = Image.new('RGBA', (3, 3), dark)
        image.putpixel((1, 0), light)
        image.putpixel((0, 1), light)
        scaled = build.scale2x(image)
        self.assertEqual(scaled.size, (6, 6))
        self.assertEqual(scaled.getpixel((2, 2)), light)
        self.assertEqual(scaled.getpixel((3, 3)), dark)
        self.assertEqual(set(scaled.get_flattened_data()), {dark, light})

    def test_pokemon_hd_cells_gain_detail_without_changing_anchors(self):
        for name in build.NAMES[8:15]:
            source = build.pmd(name)
            before, anchors = build.normalize(source, build.SOURCE_DETAIL)
            after, hd_anchors = build.normalize(source, build.SOURCE_DETAIL, pixel_art=True)
            self.assertEqual(anchors, hd_anchors, name)
            for a, b in zip(before, after):
                self.assertEqual(a.size, b.size)
                self.assertNotEqual(a.tobytes(), b.tobytes(), name)
                self.assertEqual(build.bounds(a), build.bounds(b), name)

    def test_room_scale_and_three_times_detail(self):
        for name in build.NAMES:
            source = build.authored(name) if name in build.NAMES[:8] or name == 'dragonite' else build.pmd(name)
            for detail in [1, 2, build.SOURCE_DETAIL]:
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
