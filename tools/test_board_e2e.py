"""Regression tests for the hardware runner's acknowledgment barrier."""
import unittest
from board_e2e import Rig


class BarrierTests(unittest.TestCase):
    def test_historical_match_cannot_satisfy_capture(self):
        rig = Rig.__new__(Rig)
        rig.verified = [('target', 1), ('target', 1)]
        calls = []
        def command(_):
            calls.append(1)
            if len(calls) >= 2:
                rig.verified.append(('target', 1))
            return {'actual': 'target', 'expected': 'target', 'inputs': 1}
        rig.command = command
        rig.wait(minimum=1, timeout=1)
        self.assertEqual(len(calls), 3)

    def test_old_input_generation_cannot_satisfy_capture(self):
        rig = Rig.__new__(Rig)
        rig.verified = []
        def command(_):
            rig.verified.append(('target', 1))
            return {'actual': 'target', 'expected': 'target', 'inputs': 1}
        rig.command = command
        with self.assertRaises(AssertionError):
            rig.wait(minimum=2, timeout=.02)




class CaptureTests(unittest.TestCase):
    def rig(self, crc=None, size=261120, ended=True):
        import queue,zlib
        rig=Rig.__new__(Rig);rig.messages=queue.Queue();rig.usb=lambda _:7
        raw=bytes(261120);actual=f'{zlib.crc32(raw):08x}'
        rig.messages.put(f'@QA SNAP 7 1 100 {crc or actual} {size}')
        rig.messages.put('@QA DATA 7 0 '+raw.hex())
        if ended:rig.messages.put('@QA END 7')
        return rig
    def test_complete_capture(self):
        raw,_=self.rig().capture_bytes();self.assertEqual(len(raw),261120)
    def test_corrupt_capture(self):
        with self.assertRaises(AssertionError):self.rig(crc='deadbeef').capture_bytes()
    def test_wrong_size_header(self):
        with self.assertRaises(AssertionError):self.rig(size=2).capture_bytes()

if __name__ == '__main__':
    unittest.main()
