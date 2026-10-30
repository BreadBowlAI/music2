#!/usr/bin/env python3
# In short: makes the FLAC files beside it, and the WAV of the same samples for each, with
# libsndfile (a real encoder: libFLAC inside it). The tests of FMT-24 decode both and check that
# they give the same bits. Run it again only to change them: `python3 make.py` in this folder.
#
# Each file: 4,800 frames at 48 kHz (two FLAC blocks of 4,096), a sine with a little noise, and the
# loudest and quietest numbers its bits hold, so the edges of the scale are checked too.
#
# `tiny-keys/` holds the recordings of the SFZ fixture `core/session/tests/fixtures/sfz/tiny-keys`
# as FLAC, the same numbers as its WAVs: a pack made of them renders the bits of the pack of WAVs.
import ctypes, math, os, struct

sf = ctypes.CDLL("libsndfile.so.1")


class Info(ctypes.Structure):
    _fields_ = [("frames", ctypes.c_int64), ("samplerate", ctypes.c_int), ("channels", ctypes.c_int),
                ("format", ctypes.c_int), ("sections", ctypes.c_int), ("seekable", ctypes.c_int)]


sf.sf_open.restype = ctypes.c_void_p
sf.sf_open.argtypes = [ctypes.c_char_p, ctypes.c_int, ctypes.POINTER(Info)]
sf.sf_readf_int.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_int), ctypes.c_int64]
sf.sf_readf_int.restype = ctypes.c_int64
sf.sf_writef_int.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_int), ctypes.c_int64]
sf.sf_writef_int.restype = ctypes.c_int64
sf.sf_close.argtypes = [ctypes.c_void_p]
sf.sf_strerror.restype = ctypes.c_char_p
sf.sf_strerror.argtypes = [ctypes.c_void_p]

READ, WRITE = 0x10, 0x20
WAV, FLAC = 0x010000, 0x170000
PCM = {8: (0x0005, 0x0001), 16: (0x0002, 0x0002), 24: (0x0003, 0x0003)}  # (WAV's, FLAC's): WAV's 8 bits are unsigned
FRAMES, RATE = 4800, 48000


def samples(bits, channels):
    top = (1 << (bits - 1)) - 1
    seed, out = 12345, []
    for f in range(FRAMES):
        for c in range(channels):
            seed = (seed * 1103515245 + 12345) & 0x7FFFFFFF
            noise = (seed >> 8) % 33 - 16 if bits > 8 else (seed >> 8) % 3 - 1
            v = int(round(top * 0.5 * math.sin(2 * math.pi * 440 * (c + 1) * f / RATE))) + noise
            if f == 10 + c:
                v = top
            if f == 20 + c:
                v = -top - 1
            out.append(max(-top - 1, min(top, v)))
    return out


def write(path, kind, bits, channels, values, rate=RATE):
    info = Info(0, rate, channels, kind | PCM[bits][kind == FLAC], 0, 0)
    h = sf.sf_open(path.encode(), WRITE, ctypes.byref(info))
    assert h, (path, sf.sf_strerror(None))
    # libsndfile's int calls take 32-bit numbers: a 16-bit sample is its top 16 bits, exactly.
    buf = (ctypes.c_int * len(values))(*[v << (32 - bits) for v in values])
    frames = len(values) // channels
    assert sf.sf_writef_int(h, buf, frames) == frames, path
    sf.sf_close(h)


def read16(path):
    """A 16-bit WAV's numbers, its channels and its rate."""
    info = Info()
    h = sf.sf_open(path.encode(), READ, ctypes.byref(info))
    assert h and info.format & 0xFFFF == PCM[16][0], path
    buf = (ctypes.c_int * (info.frames * info.channels))()
    assert sf.sf_readf_int(h, buf, info.frames) == info.frames, path
    sf.sf_close(h)
    return [v >> 16 for v in buf], info.channels, info.samplerate


here = os.path.dirname(os.path.abspath(__file__))
for bits, channels in [(8, 1), (16, 1), (16, 2), (24, 1), (24, 2), (16, 3)]:
    name = f"{bits}bit-{ {1: 'mono', 2: 'stereo'}.get(channels, f'{channels}ch') }"
    values = samples(bits, channels)
    write(os.path.join(here, name + ".flac"), FLAC, bits, channels, values)
    write(os.path.join(here, name + ".wav"), WAV, bits, channels, values)

keys = os.path.join(here, "../../../../session/tests/fixtures/sfz/tiny-keys/samples")
os.makedirs(os.path.join(here, "tiny-keys"), exist_ok=True)
for wav in sorted(os.listdir(keys)):
    values, channels, rate = read16(os.path.join(keys, wav))
    write(os.path.join(here, "tiny-keys", wav[:-4] + ".flac"), FLAC, 16, channels, values, rate)


# Files libsndfile can't make, written here by hand, byte by byte, each frame with its checks
# (CRC-8 and CRC-16) right, so a decoder reads as far as the case needs:
# - `bomb.flac`: 2^27 + 1 frames of silence, 8-bit mono, in about 29 KB (each 65,535-frame block a
#   CONSTANT subframe): one more sample than a whole decode may make (FMT-24's budget).
# - `silence-64mib.flac`: 64 MiB frames of silence, 8-bit mono: as a WAV, just over a preview's 64 MB.
# - `at-the-most.flac`: a header that says 2^27 frames, 8-bit mono, the most a whole decode may
#   make, then a single block: its header passes the budget, its audio is cut short.
# - `latin1-tag.flac`, `id3v1-end.flac`, `id3v2-start.flac`: 16bit-stereo.flac with a Vorbis comment
#   that isn't UTF-8 (older taggers), an ID3v1 tag after its last frame, an ID3v2 tag before it.
# - `escape.flac`: 16-bit mono, one FIXED subframe whose residual is "escaped" (its numbers written
#   plainly), and `bits-from-header.flac`: a frame that takes its bits from STREAMINFO. Both are
#   FLAC as the format allows; claxon 0.4.3 doesn't decode either.
WHOLE = 1 << 27
BPS_CODE = {8: 0b001, 16: 0b100, 24: 0b110}


def crc8(data):
    c = 0
    for b in data:
        c ^= b
        for _ in range(8):
            c = ((c << 1) ^ 0x07) & 0xFF if c & 0x80 else (c << 1) & 0xFF
    return c


def crc16(data):
    c = 0
    for b in data:
        c ^= b << 8
        for _ in range(8):
            c = ((c << 1) ^ 0x8005) & 0xFFFF if c & 0x8000 else (c << 1) & 0xFFFF
    return c


class Bits:
    def __init__(self):
        self.v, self.n, self.out = 0, 0, bytearray()

    def put(self, value, bits):
        for i in range(bits - 1, -1, -1):
            self.v = (self.v << 1) | ((value >> i) & 1)
            self.n += 1
            if self.n == 8:
                self.out.append(self.v)
                self.v, self.n = 0, 0

    def done(self):
        while self.n:
            self.put(0, 1)
        return bytes(self.out)


def utf8_number(n):
    """A frame's number as FLAC writes it: UTF-8's way, up to 36 bits."""
    if n < 0x80:
        return bytes([n])
    for length, top in [(2, 0x7FF), (3, 0xFFFF), (4, 0x1FFFFF), (5, 0x3FFFFFF), (6, 0x7FFFFFFF)]:
        if n <= top:
            tail = []
            for _ in range(length - 1):
                tail.append(0x80 | (n & 0x3F))
                n >>= 6
            return bytes([((0xFF << (8 - length)) & 0xFF) | n] + list(reversed(tail)))
    raise ValueError(n)


def streaminfo(rate, channels, bits, frames, most=65535):
    b = Bits()
    for v, n in [(16, 16), (most, 16), (0, 24), (0, 24), (rate, 20), (channels - 1, 3), (bits - 1, 5), (frames, 36)]:
        b.put(v, n)
    body = b.done() + bytes(16)
    return b"fLaC" + bytes([0x80, 0, 0, len(body)]) + body


def frame(number, size, channels, bits_code, subframes):
    """One frame: 48 kHz, its block size in 16 bits after the header, then `subframes` (bits)."""
    h = Bits()
    for v, n in [(0b11111111111110, 14), (0, 2), (0b0111, 4), (0b1010, 4), (channels - 1, 4), (bits_code, 3), (0, 1)]:
        h.put(v, n)
    head = h.done() + utf8_number(number) + (size - 1).to_bytes(2, "big")
    head += bytes([crc8(head)])
    body = head + subframes
    return body + crc16(body).to_bytes(2, "big")


def constant(channels, bits, value=0):
    s = Bits()
    for _ in range(channels):
        for v, n in [(0, 1), (0, 6), (0, 1), (value & ((1 << bits) - 1), bits)]:
            s.put(v, n)
    return s.done()


with open(os.path.join(here, "bomb.flac"), "wb") as f:
    f.write(streaminfo(RATE, 1, 8, WHOLE + 1))
    number, left = 0, WHOLE + 1
    while left:
        size = min(65535, left)
        f.write(frame(number, size, 1, BPS_CODE[8], constant(1, 8)))
        left -= size
        number += 1
# 64 MiB frames of silence, 8-bit mono (14 KB): as a WAV, 44 bytes more than a sample's preview
# may be (LIB-6), and well under a whole decode's budget.
with open(os.path.join(here, "silence-64mib.flac"), "wb") as f:
    f.write(streaminfo(RATE, 1, 8, 64 << 20))
    number, left = 0, 64 << 20
    while left:
        size = min(65535, left)
        f.write(frame(number, size, 1, BPS_CODE[8], constant(1, 8)))
        left -= size
        number += 1
with open(os.path.join(here, "at-the-most.flac"), "wb") as f:
    f.write(streaminfo(RATE, 1, 8, WHOLE) + frame(0, 65535, 1, BPS_CODE[8], constant(1, 8)))

stereo = open(os.path.join(here, "16bit-stereo.flac"), "rb").read()
assert stereo[4] & 0x7F == 0 and int.from_bytes(stereo[5:8], "big") == 34, "STREAMINFO first"
# Its blocks: STREAMINFO (bytes 4 to 42), libsndfile's Vorbis comment (the last), the audio at 86.
assert stereo[42] == 0x84 and 46 + int.from_bytes(stereo[43:46], "big") == 86
vendor, tag = b"an old tagger", "ARTIST=Beyonc\u00e9".encode("latin-1")
comment = struct.pack("<I", len(vendor)) + vendor + struct.pack("<II", 1, len(tag)) + tag
with open(os.path.join(here, "latin1-tag.flac"), "wb") as f:
    f.write(stereo[:42] + bytes([0x84]) + len(comment).to_bytes(3, "big") + comment + stereo[86:])
with open(os.path.join(here, "id3v1-end.flac"), "wb") as f:
    f.write(stereo + b"TAG" + b"Title".ljust(30, b"\0") + bytes(95))
with open(os.path.join(here, "id3v2-start.flac"), "wb") as f:
    # ID3v2.4, no flags, a body of 20 bytes (its size in 7-bit bytes: a title, padded), then the FLAC.
    f.write(b"ID3\x04\x00\x00\x00\x00\x00\x14" + b"TIT2\x00\x00\x00\x06\x00\x00\x03Title" + bytes(4) + stereo)

escaped = Bits()
for v, n in [(0, 1), (0b001000, 6), (0, 1), (0, 2), (0, 4), (0b1111, 4), (16, 5)]:
    escaped.put(v, n)
for i in range(32):
    escaped.put((i * 997) & 0xFFFF, 16)
with open(os.path.join(here, "escape.flac"), "wb") as f:
    f.write(streaminfo(RATE, 1, 16, 32) + frame(0, 32, 1, BPS_CODE[16], escaped.done()))
with open(os.path.join(here, "bits-from-header.flac"), "wb") as f:
    f.write(streaminfo(RATE, 1, 16, 32) + frame(0, 32, 1, 0b000, constant(1, 16)))
