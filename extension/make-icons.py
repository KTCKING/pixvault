"""生成 PixVault 扩展图标（纯标准库手写 PNG，不依赖 Pillow）。

设计：深色圆角底 + 白色相框 + 右下角暖橙徽章里的白色下箭头 —— 与桌面端
的黑白主色 + 暖橙点缀保持一致。小尺寸走简化版本（去掉框内的山与太阳），
保证 16px 下仍然能认出来。
"""

import math
import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "icons")
SS = 5  # 超采样倍数

BG_TOP = (38, 38, 44)
BG_BOTTOM = (16, 16, 19)
WHITE = (245, 245, 247)
ORANGE = (255, 176, 102)
DARK = (24, 24, 28)


def sd_round_rect(px, py, cx, cy, hw, hh, r):
    dx = abs(px - cx) - (hw - r)
    dy = abs(py - cy) - (hh - r)
    ax, ay = max(dx, 0.0), max(dy, 0.0)
    return math.hypot(ax, ay) + min(max(dx, dy), 0.0) - r


def sd_circle(px, py, cx, cy, r):
    return math.hypot(px - cx, py - cy) - r


def sd_triangle(px, py, ax, ay, bx, by, cx, cy):
    """等边指向下的三角形（重心坐标法算 SDF）。"""
    def edge(x1, y1, x2, y2):
        return (px - x1) * (y2 - y1) - (py - y1) * (x2 - x1)
    e0, e1, e2 = edge(ax, ay, bx, by), edge(bx, by, cx, cy), edge(cx, cy, ax, ay)
    if (e0 >= 0 and e1 >= 0 and e2 >= 0) or (e0 <= 0 and e1 <= 0 and e2 <= 0):
        return -1.0
    inside = 1.0
    return inside


def blend(dst, src, alpha):
    if alpha <= 0:
        return dst
    a = min(1.0, alpha)
    return tuple(int(round(dst[i] * (1 - a) + src[i] * a)) for i in range(3))


def render(size):
    """在设计空间 128x128 下取点，再按 size 缩放。"""
    design = 128.0
    big = size * SS
    scale = design / big

    rows = []
    simple = size <= 24

    for by in range(big):
        row = []
        for bx in range(big):
            x = (bx + 0.5) * scale
            y = (by + 0.5) * scale

            # 底：圆角矩形 + 垂直渐变
            d_bg = sd_round_rect(x, y, 64, 64, 60, 60, 27)
            if d_bg > 0:
                row.append((0, 0, 0, 0))
                continue
            t = y / design
            col = tuple(int(BG_TOP[i] + (BG_BOTTOM[i] - BG_TOP[i]) * t) for i in range(3))

            # 顶部一道极淡的高光，模拟玻璃质感（不做模糊，纯线性渐变）
            gloss = max(0.0, 1.0 - y / (design * 0.55)) * 0.10
            col = blend(col, (255, 255, 255), gloss)

            # 相框：描边圆角矩形
            d_outer = sd_round_rect(x, y, 60, 54, 32, 26, 9)
            d_inner = sd_round_rect(x, y, 60, 54, 32 - 7, 26 - 7, 6)
            frame = max(d_outer, -d_inner)
            if frame <= 0:
                col = blend(col, WHITE, 1.0)
            elif not simple:
                # 框内的太阳
                if sd_circle(x, y, 48, 42, 5.5) <= 0:
                    col = blend(col, WHITE, 1.0)
                # 框内的山：两个交叠的圆角三角，读作「照片」
                if sd_triangle(x, y, 36, 68, 54, 44, 66, 68) <= 0:
                    col = blend(col, WHITE, 1.0)
                if sd_triangle(x, y, 52, 68, 68, 50, 82, 68) <= 0:
                    col = blend(col, WHITE, 1.0)

            # 右下角暖橙徽章 + 白色下箭头
            if sd_circle(x, y, 92, 90, 26) <= 0:
                col = blend(col, ORANGE, 1.0)
                # 箭头竖杆
                if sd_round_rect(x, y, 92, 84, 4, 11, 2) <= 0:
                    col = blend(col, DARK, 1.0)
                # 箭头三角
                if sd_triangle(x, y, 82, 88, 102, 88, 92, 103) <= 0:
                    col = blend(col, DARK, 1.0)

            row.append((col[0], col[1], col[2], 255))
        rows.append(row)

    # 超采样降采样（按 alpha 加权，避免透明边缘发黑）
    out = []
    for y in range(size):
        row = []
        for x in range(size):
            r = g = b = a = 0
            for dy in range(SS):
                for dx in range(SS):
                    pr, pg, pb, pa = rows[y * SS + dy][x * SS + dx]
                    r += pr
                    g += pg
                    b += pb
                    a += pa
            n = SS * SS
            row.append((r // n, g // n, b // n, a // n))
        out.append(row)
    return out


def write_png(path, w, h, pixels):
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        for x in range(w):
            raw.extend(pixels[y][x])

    def chunk(tag, data):
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


def main():
    os.makedirs(OUT, exist_ok=True)
    for size in (16, 32, 48, 128):
        pixels = render(size)
        path = os.path.join(OUT, f"icon{size}.png")
        write_png(path, size, size, pixels)
        print(f"{path}  {os.path.getsize(path)} bytes")


if __name__ == "__main__":
    main()
