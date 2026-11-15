"""Villagers and animals in 3.125 cm blocks: a big head on a small body (block-world.md section 4)."""
import math
from mathutils import Vector

from core import Vox, UP_Z


def villager(name, x, y, z, face, kind, top, legs, hair="hair", skin="skin", arms=(15, -10), scale=1.0, tool=None):
    v = Vox(0.03125, UP_Z)
    for sx in (-1, 1):
        v.box((sx * 0.13 - 0.08, -0.1, 0.0), (sx * 0.13 + 0.08, 0.1, 0.12), "dark")
        v.box((sx * 0.13 - 0.08, -0.14, 0.0), (sx * 0.13 + 0.08, 0.0, 0.05), "dark")
        v.box((sx * 0.13 - 0.07, -0.08, 0.12), (sx * 0.13 + 0.07, 0.08, 0.42), legs)
    if kind == "woman":
        for k in range(10):
            r = 0.34 - k * 0.018
            v.ball((0, 0, 0.12 + k * 0.035), (r, r * 0.9, 0.03), top)
        v.box((-0.14, -0.33, 0.15), (0.14, -0.27, 0.62), "cream")
    v.box((-0.2, -0.14, 0.4), (0.2, 0.14, 0.86), top)
    v.box((-0.21, -0.15, 0.48), (0.21, 0.15, 0.54), "brown2"); v.box((-0.04, -0.16, 0.47), (0.04, -0.14, 0.55), "gold")
    v.box((-0.12, -0.15, 0.8), (0.12, -0.12, 0.86), "cream" if kind != "guard" else "steel")
    if kind == "guard":
        v.box((-0.16, -0.17, 0.35), (0.16, -0.14, 0.8), "red"); v.box((-0.16, 0.14, 0.35), (0.16, 0.17, 0.8), "red")
        v.box((-0.04, -0.18, 0.62), (0.04, -0.16, 0.72), "gold"); v.box((-0.1, -0.18, 0.66), (0.1, -0.16, 0.68), "gold")
        for sx in (-1, 1):
            v.ball((sx * 0.25, 0, 0.84), (0.12, 0.13, 0.08), lambda p: "steelhi" if p.z > 0.88 else "steel")
    hands = []
    for sx, ang in zip((-1, 1), arms):
        a = math.radians(ang); sh = Vector((sx * 0.25, 0, 0.8)); d = Vector((sx * 0.08, -math.sin(a), -math.cos(a))).normalized()
        v.rod(sh, sh + d * 0.3, 0.065, top if kind != "guard" else "steel2")
        v.ball(sh + d * 0.36, (0.075, 0.075, 0.075), skin); hands.append(sh + d * 0.36)
    v.box((-0.25, -0.22, 0.88), (0.25, 0.22, 1.36), skin)
    v.cut((-0.25, -0.22, 0.88), (-0.215, -0.185, 1.36)); v.cut((0.215, -0.22, 0.88), (0.25, -0.185, 1.36))
    for sx in (-1, 1):
        v.box((sx * 0.1 - 0.035, -0.235, 1.06), (sx * 0.1 + 0.035, -0.215, 1.17), "eye")
        v.box((sx * 0.1 - 0.0, -0.24, 1.135), (sx * 0.1 + 0.03, -0.225, 1.17), "white")
        v.box((sx * 0.1 - 0.05, -0.235, 1.2), (sx * 0.1 + 0.05, -0.215, 1.235), hair if kind != "guard" else "skin2")
        v.box((sx * 0.17 - 0.03, -0.232, 1.0), (sx * 0.17 + 0.03, -0.215, 1.035), "pink")
    v.box((-0.025, -0.25, 1.03), (0.025, -0.22, 1.08), "skin2")
    v.box((-0.05, -0.235, 0.95), (0.05, -0.215, 0.97), "skin2")
    if kind == "guard":
        v.box((-0.28, -0.26, 0.9), (0.28, 0.26, 1.42), "steel")
        v.box((-0.29, -0.27, 1.38), (0.29, 0.27, 1.44), "steelhi")
        v.box((-0.21, -0.275, 1.1), (0.21, -0.25, 1.135), "eye")
        v.box((-0.017, -0.29, 0.95), (0.017, -0.26, 1.1), "steelhi")
        for zz in (0.97, 1.03):
            for xx in (-0.14, -0.07, 0.07, 0.14):
                v.box((xx - 0.017, -0.275, zz), (xx + 0.017, -0.25, zz + 0.03), "eye")
        for zz in (0.93, 1.16, 1.32):
            v.box((-0.285, -0.27, zz), (0.285, -0.26, zz + 0.035), "steelhi")
        for k in range(8):
            v.box((-0.03, -0.1 + k * 0.035, 1.44), (0.03, -0.065 + k * 0.035, 1.5 + 0.08 * math.sin(k / 7 * math.pi)), "red")
    else:
        v.box((-0.27, -0.24, 1.26), (0.27, 0.24, 1.42), hair)
        v.box((-0.27, -0.05, 0.98), (0.27, 0.24, 1.3), hair)
        shade = "hair2" if hair == "hair" else "blond2"
        for k in range(-7, 8):
            l = 0.035 * (abs(k) // 2) + (0.035 if k % 2 else 0.0)
            v.box((k * 0.035 - 0.0175, -0.255, max(1.24 - l, 1.0)), (k * 0.035 + 0.0175, -0.215, 1.3), hair if k % 3 else shade)
        for sx in (-1, 1):
            v.box((sx * 0.27 - 0.035, -0.18, 1.0), (sx * 0.27 + 0.035, 0.1, 1.3), hair)
            v.box((sx * 0.27 - 0.035, -0.18, 0.97), (sx * 0.27 + 0.035, -0.08, 1.0), shade)
        v.box((-0.03, -0.245, 1.38), (0.0, 0.2, 1.43), shade)
        for (xx, yy) in [(-0.16, -0.1), (0.12, -0.05), (-0.05, 0.12), (0.17, 0.12)]:
            v.box((xx - 0.05, yy - 0.05, 1.42), (xx + 0.05, yy + 0.05, 1.47), hair)
        if kind == "woman":
            v.ball((0, 0.27, 1.25), (0.11, 0.09, 0.11), hair); v.box((-0.04, 0.2, 1.25), (0.04, 0.24, 1.3), "red")
        if kind == "bearded":
            v.box((-0.2, -0.26, 0.86), (0.2, -0.2, 1.02), hair)
            for k in range(-4, 5):
                v.box((k * 0.04 - 0.02, -0.26, 0.8 - 0.03 * ((k * 5) % 3)), (k * 0.04 + 0.02, -0.21, 0.88), hair)
            v.box((-0.1, -0.265, 1.0), (0.1, -0.225, 1.04), hair)
    if kind == "farmer":
        v.rod((0, 0, 1.4), (0, 0, 1.45), 0.42, lambda p, t: "straw" if (int(p.x / 0.035) + int(p.y / 0.035)) % 4 else "straw2", flat=True)
        v.rod((0, 0, 1.45), (0, 0, 1.62), lambda t: 0.25 - 0.04 * t, "straw", flat=True)
        v.rod((0, 0, 1.46), (0, 0, 1.52), 0.26, "red", flat=True)
    if tool == "hoe":
        h = hands[1]
        v.rod(h + Vector((0, -0.05, 0.25)), Vector((h.x + 0.05, h.y - 0.55, 0.05)), 0.025, "brown")
        v.box((h.x - 0.02, h.y - 0.62, 0.0), (h.x + 0.12, h.y - 0.5, 0.06), "steel")
    if tool == "sword":
        v.box((0.32, -0.32, 0.05), (0.38, -0.28, 0.8), "steelhi"); v.box((0.33, -0.33, 0.05), (0.37, -0.27, 0.8), "steel")
        v.box((0.22, -0.33, 0.8), (0.48, -0.27, 0.85), "gold"); v.box((0.33, -0.32, 0.85), (0.37, -0.28, 1.0), "brown2")
        v.ball((0.35, -0.3, 1.02), 0.035, "gold")
    if tool == "sack":
        v.ball((-0.3, 0.1, 1.0), (0.2, 0.26, 0.2), lambda p: "cream" if p.z > 1.0 else "straw2")
    if tool == "basket":
        h = hands[0]
        v.box((h.x - 0.14, h.y - 0.16, h.z - 0.2), (h.x + 0.14, h.y + 0.1, h.z - 0.03), "straw2")
        v.box((h.x - 0.1, h.y - 0.12, h.z - 0.05), (h.x + 0.1, h.y + 0.06, h.z), "red")
    o = v.mesh(name, (x, y, z), face, scale=scale)
    return o


def chicken(x, y, z, face):
    v = Vox(0.03125, UP_Z)
    v.ball((0, 0.02, 0.2), (0.12, 0.16, 0.1), "white")
    v.ball((0, 0.16, 0.27), (0.06, 0.06, 0.07), "white")
    v.ball((0, -0.12, 0.33), (0.07, 0.07, 0.08), "white")
    v.box((-0.02, -0.15, 0.4), (0.02, -0.06, 0.45), "comb")
    v.box((-0.02, -0.22, 0.31), (0.02, -0.18, 0.35), "beak")
    v.box((-0.02, -0.19, 0.27), (0.02, -0.17, 0.3), "comb")
    for sx in (-1, 1):
        v.box((sx * 0.05 - 0.01, -0.17, 0.35), (sx * 0.05 + 0.01, -0.15, 0.37), "eye")
        v.box((sx * 0.05 - 0.015, -0.01, 0.0), (sx * 0.05 + 0.015, 0.02, 0.12), "beak")
        v.box((sx * 0.13 - 0.02, -0.04, 0.15), (sx * 0.13 + 0.02, 0.12, 0.26), "cream")
    v.mesh("chicken", (x, y, z), face)
