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
import ctypes, math, os

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
