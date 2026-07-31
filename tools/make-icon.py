#!/usr/bin/env python3
"""Generate own-brew's app icon.

Written by hand rather than pulled from a design tool so the icon lives in the
repository as source: the mark is the same ◑ the app's rail shows, in the same
copper, on the same slate.

Pure standard library — no image dependency to install. Anti-aliasing comes
from rendering at 2x and averaging down.
"""

import struct
import zlib
from pathlib import Path

SIZE = 1024
SS = 2  # supersampling factor
N = SIZE * SS

# Straight from src/styles/tokens.css.
SLATE_TOP = (0x1B, 0x20, 0x1E)
SLATE_BOTTOM = (0x0D, 0x0F, 0x0E)
COPPER = (0xE0, 0x97, 0x5A)

CORNER = int(0.225 * N)  # macOS icons are a rounded square, not a circle
MARGIN = int(0.10 * N)
RING_R = int(0.30 * N)
RING_W = int(0.052 * N)


def rounded_square_alpha(x: float, y: float) -> float:
    """Coverage of the rounded-square mask at a point, 0..1."""
    lo, hi = MARGIN, N - MARGIN
    r = CORNER

    # Distance outside the rounded rectangle, negative inside.
    dx = max(lo + r - x, 0.0, x - (hi - r))
    dy = max(lo + r - y, 0.0, y - (hi - r))

    if x < lo or x > hi or y < lo or y > hi:
        return 0.0
    if dx > 0 and dy > 0:
        return 1.0 if (dx * dx + dy * dy) <= r * r else 0.0
    return 1.0


def blend(under, over, alpha):
    return tuple(int(u + (o - u) * alpha) for u, o in zip(under, over))


def render() -> bytearray:
    cx = cy = N / 2.0
    ring_outer = RING_R
    ring_inner = RING_R - RING_W

    rows = bytearray()
    for y in range(N):
        # Vertical gradient keeps the large dark field from looking flat.
        t = y / (N - 1)
        base = blend(SLATE_TOP, SLATE_BOTTOM, t)

        for x in range(N):
            a = rounded_square_alpha(x + 0.5, y + 0.5)
            if a == 0.0:
                rows += bytes((0, 0, 0, 0))
                continue

            dx, dy = x + 0.5 - cx, y + 0.5 - cy
            d = (dx * dx + dy * dy) ** 0.5

            colour = base
            if d <= ring_outer:
                if dx >= 0:
                    # The filled half of ◑.
                    colour = COPPER
                elif d >= ring_inner:
                    # The outlined half.
                    colour = COPPER

            rows += bytes((*colour, int(255 * a)))
    return rows


def downsample(src: bytearray) -> bytearray:
    """Average SS x SS blocks — this is what softens the edges."""
    out = bytearray()
    for y in range(SIZE):
        for x in range(SIZE):
            r = g = b = a = 0
            for oy in range(SS):
                for ox in range(SS):
                    i = (((y * SS + oy) * N) + (x * SS + ox)) * 4
                    r += src[i]
                    g += src[i + 1]
                    b += src[i + 2]
                    a += src[i + 3]
            n = SS * SS
            out += bytes((r // n, g // n, b // n, a // n))
    return out


def write_png(path: Path, pixels: bytearray) -> None:
    raw = bytearray()
    for y in range(SIZE):
        raw.append(0)  # filter type 0
        raw += pixels[y * SIZE * 4 : (y + 1) * SIZE * 4]

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    path.write_bytes(png)


if __name__ == "__main__":
    target = Path(__file__).resolve().parent / "icon-source.png"
    write_png(target, downsample(render()))
    print(f"wrote {target} ({target.stat().st_size // 1024} kB)")
