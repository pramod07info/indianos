"""Host-only unit tests of deterministic read-only archive packing."""
import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "tools"))
from build_initramfs import pack


class InitramfsPackerTests(unittest.TestCase):
    def test_deterministic_and_sorted(self):
        content = [('/bin/z', b'Z'), ('/bin/a', b'A')]
        first = pack(content)
        self.assertEqual(first, pack(list(reversed(content))))
        self.assertEqual(first[:8], b'INDRAM01')
        self.assertEqual(int.from_bytes(first[12:16], 'little'), len(first))

    def test_blocks_traversal_and_duplicates(self):
        for name in ('bin/a', '/../a', '/a/./b', '/a//b', '/a/', '/a\\b', '/a b', '/'):
            with self.subTest(name=name):
                with self.assertRaises(ValueError):
                    pack([(name, b'')])
        with self.assertRaises(ValueError):
            pack([('/bin/a', b'A'), ('/bin/a', b'B')])
        with self.assertRaises(ValueError):
            pack([])
        with self.assertRaises(ValueError):
            pack([('/bin/a', b'A')] * 17)


if __name__ == "__main__":
    unittest.main()
