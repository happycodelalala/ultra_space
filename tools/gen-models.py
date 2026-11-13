#!/usr/bin/env python3
"""Generates the low-poly part models (task 3.12) as glTF binaries.

Original, blocky, flat-shaded models built from a few solids (bevelled boxes, tapered blocks,
n-sided prisms), sized from each part's collision shape in `content/data/parts/*.json`, in part
axes (metres, Y up). Each primitive's material is named by its slot: `armor`, `trim`, `dark`
or `glass`; the game maps those names to its own materials. Writes
`assets/export/models/<model>.glb` for every part whose form names a model built here; parts
without one keep their placeholder shape. Hand-made models with the same names can replace
these.

    tools/gen-models.py            # writes every model
    tools/gen-models.py --check    # fails if a written model is out of date
"""

import json
import math
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PARTS = ROOT / "content" / "data" / "parts"
OUT = ROOT / "assets" / "export" / "models"

# Colours only for other viewers (the game uses its own materials by name).
COLOURS = {
    "armor": [0.55, 0.58, 0.62, 1.0],
    "trim": [0.72, 0.56, 0.30, 1.0],
    "dark": [0.16, 0.17, 0.19, 1.0],
    "glass": [0.20, 0.45, 0.65, 1.0],
}


# --- Solids: lists of convex-ish polygons, oriented outward from the solid's centre ---------


class Model:
    def __init__(self):
        self.polys = {}  # material -> list of (points, centre)

    def add(self, material, polys, centre):
        self.polys.setdefault(material, []).extend((p, centre) for p in polys)
        return self


def sub(a, b):
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def norm(a):
    length = math.sqrt(dot(a, a)) or 1.0
    return [a[0] / length, a[1] / length, a[2] / length]


def bevel_box(half, bevel, centre=(0, 0, 0), taper=(1.0, 1.0), squash=None):
    """A box with chamfered edges and corners. `taper` scales X and Z at the bottom and top
    (a block narrowing along Y); `squash` is an optional function moving each vertex."""
    hx, hy, hz = half
    b = min(bevel, hx * 0.45, hy * 0.45, hz * 0.45)

    def vertex(sx, sy, sz, axis):
        p = [sx * (hx - b), sy * (hy - b), sz * (hz - b)]
        p[axis] = [sx * hx, sy * hy, sz * hz][axis]
        # Taper with height.
        t = (p[1] + hy) / (2 * hy)
        k = taper[0] + (taper[1] - taper[0]) * t
        p[0] *= k
        p[2] *= k
        if squash:
            p = squash(p)
        return [p[0] + centre[0], p[1] + centre[1], p[2] + centre[2]]

    signs = (-1, 1)
    polys = []
    # Faces.
    for axis in range(3):
        for s in signs:
            others = [a for a in range(3) if a != axis]
            ring = []
            for u, v in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
                sv = [0, 0, 0]
                sv[axis] = s
                sv[others[0]] = u
                sv[others[1]] = v
                ring.append(vertex(sv[0], sv[1], sv[2], axis))
            polys.append(ring)
    # Edge chamfers.
    for axis in range(3):
        a1, a2 = [a for a in range(3) if a != axis]
        for s1 in signs:
            for s2 in signs:
                quad = []
                for s in (-1, 1):
                    sv = [0, 0, 0]
                    sv[axis] = s
                    sv[a1] = s1
                    sv[a2] = s2
                    quad.append(vertex(sv[0], sv[1], sv[2], a1))
                for s in (1, -1):
                    sv = [0, 0, 0]
                    sv[axis] = s
                    sv[a1] = s1
                    sv[a2] = s2
                    quad.append(vertex(sv[0], sv[1], sv[2], a2))
                polys.append(quad)
    # Corners.
    for sx in signs:
        for sy in signs:
            for sz in signs:
                polys.append([vertex(sx, sy, sz, a) for a in range(3)])
    return polys, list(centre)


def prism(radius, half_length, sides, axis, centre=(0, 0, 0), taper=1.0, rotate=0.0):
    """An n-sided prism along `axis` (0 X, 1 Y, 2 Z); the + end scaled by `taper`."""
    rings = []
    for end, scale in ((-1, 1.0), (1, taper)):
        ring = []
        for k in range(sides):
            a = 2 * math.pi * (k + 0.5) / sides + rotate
            u, v = radius * scale * math.cos(a), radius * scale * math.sin(a)
            p = [0.0, 0.0, 0.0]
            p[axis] = end * half_length
            others = [i for i in range(3) if i != axis]
            p[others[0]] = u
            p[others[1]] = v
            ring.append([p[0] + centre[0], p[1] + centre[1], p[2] + centre[2]])
        rings.append(ring)
    polys = [rings[0], rings[1]]
    for k in range(sides):
        j = (k + 1) % sides
        polys.append([rings[0][k], rings[0][j], rings[1][j], rings[1][k]])
    return polys, list(centre)


def box(half, centre=(0, 0, 0)):
    return bevel_box(half, 0.0001, centre)


# --- Models (original designs) ----------------------------------------------------------------


def size_of(part):
    shape = part["shape"]
    if shape["type"] == "shell":
        return [1, 1, 1]
    if shape["type"] == "box":
        return [s / 2 for s in shape["size_m"]]
    if shape["type"] == "cylinder":
        return [shape["radius_m"], shape["length_m"] / 2, shape["radius_m"]]
    r = shape["radius_m"]
    return [r, r, r]


def torso(h):
    hx, hy, hz = h
    m = Model()
    # Core: a bevelled block, narrower at the waist.
    m.add("armor", *bevel_box([hx * 0.92, hy, hz * 0.88], 0.45, taper=(0.82, 1.0)))
    # Chest plates over the upper front, a glass visor strip between them.
    for s in (-1, 1):
        m.add("armor", *bevel_box([hx * 0.36, hy * 0.28, 0.18], 0.12,
                                  centre=(s * hx * 0.46, hy * 0.42, -hz + 0.18)))
    m.add("glass", *bevel_box([hx * 0.42, hy * 0.08, 0.12], 0.05, centre=(0, hy * 0.05, -hz + 0.12)))
    # Abdomen ribs.
    for k in range(3):
        m.add("dark", *box([hx * 0.5, 0.12, 0.1], centre=(0, -hy * 0.3 - k * 0.42, -hz * 0.86)))
    # Collar ring and trim stripes.
    m.add("trim", *bevel_box([hx * 0.6, 0.18, hz * 0.6], 0.08, centre=(0, hy - 0.2, 0)))
    for s in (-1, 1):
        m.add("trim", *box([0.08, hy * 0.6, 0.06], centre=(s * hx * 0.88, hy * 0.1, -hz * 0.84)))
    return m


def head(h):
    hx, hy, hz = h
    m = Model()
    m.add("armor", *bevel_box([hx * 0.9, hy * 0.85, hz * 0.9], 0.22, centre=(0, -hy * 0.1, 0),
                              taper=(1.0, 0.8)))
    m.add("glass", *bevel_box([hx * 0.7, hy * 0.12, 0.08], 0.03, centre=(0, hy * 0.05, -hz * 0.86)))
    m.add("trim", *bevel_box([0.08, hy * 0.25, hz * 0.6], 0.03, centre=(0, hy * 0.75, -hz * 0.1)))
    for s in (-1, 1):
        m.add("dark", *box([0.06, hy * 0.3, hz * 0.35], centre=(s * hx * 0.9, -hy * 0.15, 0)))
    return m


def waist(h):
    hx, hy, hz = h
    m = Model()
    m.add("armor", *bevel_box([hx, hy * 0.7, hz], 0.3, centre=(0, -hy * 0.3, 0)))
    m.add("dark", *bevel_box([hx * 0.7, hy * 0.35, hz * 0.8], 0.1, centre=(0, hy * 0.6, 0)))
    m.add("trim", *bevel_box([hx * 0.35, hy * 0.4, 0.12], 0.06, centre=(0, -hy * 0.35, -hz)))
    return m


def shoulder(h):
    hx, hy, hz = h
    m = Model()
    m.add("dark", *bevel_box([hx * 0.75, hy * 0.75, hz * 0.75], 0.15))
    m.add("armor", *bevel_box([hx, hy * 0.45, hz], 0.25, centre=(0, hy * 0.55, 0), taper=(1.0, 0.85)))
    m.add("trim", *box([hx * 0.98, 0.06, 0.1], centre=(0, hy * 0.35, -hz * 0.95)))
    return m


def limb(h, knee=False):
    hx, hy, hz = h
    m = Model()
    m.add("armor", *bevel_box([hx * 0.85, hy * 0.92, hz * 0.85], 0.2, taper=(0.82, 1.0)))
    for y in (hy * 0.82, -hy * 0.82):
        m.add("dark", *bevel_box([hx * 0.72, 0.12, hz * 0.72], 0.04, centre=(0, y, 0)))
    m.add("trim", *bevel_box([hx * 0.4, hy * 0.5, 0.1], 0.05, centre=(0, hy * 0.1, -hz * 0.82)))
    if knee:
        m.add("armor", *bevel_box([hx * 0.75, hy * 0.18, hz * 0.3], 0.15,
                                  centre=(0, -hy * 0.7, -hz * 0.75)))
    return m


def foot(h):
    hx, hy, hz = h

    def toe(p):
        # The front top slopes down to the toe.
        if p[2] < 0 and p[1] > -hy * 0.2:
            p[1] -= (-p[2] / hz) * hy * 0.9
        return p

    m = Model()
    m.add("armor", *bevel_box([hx, hy, hz], 0.18, squash=toe))
    m.add("dark", *bevel_box([hx * 0.5, hy * 0.35, hz * 0.4], 0.08, centre=(0, hy * 0.65, hz * 0.2)))
    m.add("trim", *box([hx * 0.98, 0.06, 0.4], centre=(0, -hy * 0.75, -hz * 0.85)))
    return m


def backpack(h):
    hx, hy, hz = h
    m = Model()
    m.add("armor", *bevel_box([hx * 0.95, hy * 0.95, hz], 0.35))
    for k in range(4):
        m.add("dark", *box([hx * 0.55, 0.08, 0.06], centre=(0, hy * 0.45 - k * 0.32, hz)))
    m.add("trim", *bevel_box([hx * 0.3, hy * 0.3, 0.2], 0.08, centre=(0, -hy * 0.55, hz)))
    return m


def binder(h):
    hx, hy, hz = h
    m = Model()
    m.add("armor", *bevel_box([hx, hy, hz], 0.3))
    for k, y in enumerate((hy * 0.45, -hy * 0.05, -hy * 0.55)):
        m.add("armor" if k % 2 else "trim",
              *bevel_box([0.08, hy * 0.2, hz * 0.8], 0.04, centre=(hx, y, 0)))
        m.add("armor" if k % 2 else "trim",
              *bevel_box([0.08, hy * 0.2, hz * 0.8], 0.04, centre=(-hx, y, 0)))
    return m


def drone_pod(h):
    hx, hy, hz = h
    m = Model()
    m.add("armor", *prism(min(hx, hy), hz * 0.7, 8, 2, centre=(0, 0, hz * 0.3), rotate=math.pi / 8))
    m.add("dark", *prism(min(hx, hy) * 0.95, hz * 0.3, 8, 2, centre=(0, 0, -hz * 0.7), taper=0.5,
                         rotate=math.pi / 8))
    return m


def gyro(h):
    m = Model()
    m.add("dark", *prism(h[0] * 0.9, h[1] * 0.6, 8, 1))
    return m


def hinge_x(h, sides=8):
    """A joint turning about X: a drum along X with a trim hub."""
    hx, hy, hz = h
    r = min(hy, hz)
    m = Model()
    m.add("dark", *prism(r, hx * 0.95, sides, 0, rotate=math.pi / sides))
    for s in (-1, 1):
        m.add("trim", *prism(r * 0.6, hx * 0.05, sides, 0, centre=(s * hx * 0.98, 0, 0),
                             rotate=math.pi / sides))
    return m


def rotor_y(h):
    hx, hy, hz = h
    m = Model()
    m.add("dark", *prism(min(hx, hz), hy, 8, 1, rotate=math.pi / 8))
    m.add("trim", *prism(min(hx, hz) * 1.02, hy * 0.25, 8, 1, rotate=math.pi / 8))
    return m


def arm_segment(h):
    hx, hy, hz = h
    m = Model()
    # Full length, so it meets the joints at both ends.
    m.add("armor", *bevel_box([hx * 0.8, hy * 0.8, hz * 0.8], 0.12, taper=(0.75, 1.0)))
    for y in (hy * 0.9, -hy * 0.9):
        m.add("dark", *prism(hx * 0.72, hy * 0.1, 8, 1, centre=(0, y, 0), rotate=math.pi / 8))
    for y in (hy * 0.62, -hy * 0.62):
        m.add("dark", *prism(hx * 0.95, 0.08, 8, 1, centre=(0, y, 0), rotate=math.pi / 8))
    m.add("trim", *box([0.06, hy * 0.5, hz * 0.4], centre=(hx * 0.78, 0, 0)))
    m.add("trim", *box([0.06, hy * 0.5, hz * 0.4], centre=(-hx * 0.78, 0, 0)))
    return m


def engine(h):
    r, hl, _ = h
    m = Model()
    # Housing at the top, bell flaring to the nozzle at the bottom (-Y).
    m.add("armor", *prism(r * 0.85, hl * 0.35, 8, 1, centre=(0, hl * 0.65, 0), rotate=math.pi / 8))
    m.add("dark", *prism(r * 0.55, hl * 0.65, 8, 1, centre=(0, -hl * 0.35, 0), taper=1 / 1.75,
                         rotate=math.pi / 8))
    m.add("trim", *prism(r * 0.9, hl * 0.06, 8, 1, centre=(0, hl * 0.3, 0), rotate=math.pi / 8))
    return m


def boom(h):
    hx, hy, hz = h
    m = Model()
    m.add("armor", *bevel_box([hx * 0.85, hy, hz * 0.85], 0.12, taper=(0.85, 1.0)))
    for y in (hy * 0.85, -hy * 0.85):
        m.add("dark", *prism(hx * 0.98, min(0.1, hy * 0.12), 8, 1, centre=(0, y, 0),
                             rotate=math.pi / 8))
    if hy > 0.4:
        m.add("trim", *box([0.06, hy * 0.55, hz * 0.4], centre=(hx * 0.84, 0, 0)))
        m.add("trim", *box([0.06, hy * 0.55, hz * 0.4], centre=(-hx * 0.84, 0, 0)))
    return m


def shell_profile(points, steps):
    """Catmull-Rom curve through the four (height, radius) points, ends repeated: the game's
    shell_profile."""
    p = lambda i: points[max(0, min(3, i))]
    out = []
    for span in range(3):
        p0, p1, p2, p3 = p(span - 1), p(span), p(span + 1), p(span + 2)
        for k in range(steps):
            t = k / steps
            def c(a, b, c_, d):
                return 0.5 * (2 * b + (c_ - a) * t + (2 * a - 5 * b + 4 * c_ - d) * t * t
                              + (3 * b - a - 3 * c_ + d) * t * t * t)
            out.append((c(p0[0], p1[0], p2[0], p3[0]), c(p0[1], p1[1], p2[1], p3[1])))
    out.append(tuple(points[3]))
    return out


class Smooth:
    """Indexed, smooth-shaded triangles per material (for curved surfaces)."""

    def __init__(self):
        self.parts = {}

    def grid(self, material, rows, flip=False):
        """A quad grid from rows of (position, normal); rows run along the surface."""
        pos, nrm, idx = self.parts.setdefault(material, ([], [], []))
        base = len(pos)
        width = len(rows[0])
        for row in rows:
            for p, n in row:
                pos.append(p)
                nrm.append(n)
        for i in range(len(rows) - 1):
            for j in range(width - 1):
                a = base + i * width + j
                b, c, d = a + 1, a + width, a + width + 1
                idx.extend([a, c, b, b, c, d] if not flip else [a, b, c, b, d, c])


def shell_quarter(part):
    """A smooth curved quarter: outer face (armor), inner face (dark), edges (trim)."""
    shape = part["shape"]
    t = shape["thickness_m"]
    a0, a1 = shape["arc_deg"]
    prof = shell_profile(shape["profile_m"], 16)
    arcs = [a0 + (a1 - a0) * k / 48 for k in range(49)]

    def point(deg, y, r):
        phi = math.radians(deg)
        return [-r * math.sin(phi), y, r * math.cos(phi)]

    # Profile normals in the (r, y) plane from neighbouring samples.
    normals2 = []
    for i, (y, r) in enumerate(prof):
        y0, r0 = prof[max(0, i - 1)]
        y1, r1 = prof[min(len(prof) - 1, i + 1)]
        dy, dr = y1 - y0, r1 - r0
        l = math.hypot(dy, dr) or 1
        normals2.append((-dy / l, dr / l))  # (outward radial, up)

    def surface(side):
        rows = []
        for (y, r), (nr, ny) in zip(prof, normals2):
            row = []
            for deg in arcs:
                phi = math.radians(deg)
                radial = [-math.sin(phi), 0, math.cos(phi)]
                rr = r + side * nr * t / 2
                yy = y + side * ny * t / 2
                n = [radial[0] * nr * side, ny * side, radial[2] * nr * side]
                row.append((point(deg, yy, rr), n))
            rows.append(row)
        return rows

    m = Smooth()
    m.grid("armor", surface(1), flip=True)
    m.grid("dark", surface(-1))
    # Edges: the rim (top), the hem (bottom), and the two seams.
    outer, inner = surface(1), surface(-1)
    for rows, mat in (([outer[0], inner[0]], "trim"), ([inner[-1], outer[-1]], "trim")):
        up = [0, 1 if rows[0] is outer[0] else -1, 0]
        m.grid(mat, [[(p, up) for p, _ in row] for row in rows])
    for j, sign in ((0, -1), (len(arcs) - 1, 1)):
        phi = math.radians(arcs[j])
        side_n = [-math.cos(phi) * sign, 0, -math.sin(phi) * sign]
        rows = [[(outer[i][j][0], side_n), (inner[i][j][0], side_n)] for i in range(len(prof))]
        m.grid("trim", rows, flip=(sign > 0))
    return m


def grapple(h):
    """A three-claw grapple: a palm block, three claws tapering to points."""
    hx, hy, hz = h
    m = Model()
    m.add("dark", *prism(hx * 0.9, hy * 0.25, 6, 1, centre=(0, hy * 0.7, 0)))
    m.add("armor", *prism(hx * 0.75, hy * 0.2, 6, 1, centre=(0, hy * 0.3, 0), taper=0.8))
    for k in range(3):
        a = 2 * math.pi * k / 3
        cx, cz = math.sin(a) * hx * 0.5, math.cos(a) * hz * 0.5
        m.add("trim", *bevel_box([hx * 0.16, hy * 0.42, hz * 0.16], 0.03,
                                  centre=(cx, -hy * 0.45, cz), taper=(0.3, 1.0)))
    return m


def carrier(h):
    hx, hy, hz = h
    m = Model()
    m.add("armor", *bevel_box([hx, hy, hz], 0.2))
    m.add("dark", *prism(hx * 0.5, hy * 0.1, 8, 1, centre=(0, hy, 0), rotate=math.pi / 8))
    m.add("trim", *box([hx * 0.9, 0.05, hz * 0.3], centre=(0, -hy * 0.6, hz)))
    return m


def saddle(h):
    """A shoulder saddle: an armoured block with a socket at each end for a shell arm."""
    hx, hy, hz = h
    m = Model()
    m.add("armor", *bevel_box([hx, hy, hz], 0.15, taper=(1.0, 0.9)))
    for s_ in (-1, 1):
        m.add("dark", *prism(hx * 0.45, hy * 0.6, 8, 1, centre=(0, hy * 0.7, s_ * hz * 0.85),
                             rotate=math.pi / 8))
    m.add("trim", *box([hx * 0.9, 0.05, hz * 0.5], centre=(0, hy * 0.98, 0)))
    return m


MODELS = {
    "frame.torso": torso,
    "frame.head": head,
    "frame.waist": waist,
    "frame.shoulder": shoulder,
    "frame.limb": limb,
    "frame.leg": lambda h: limb(h, knee=True),
    "frame.foot": foot,
    "frame.backpack": backpack,
    "frame.binder": binder,
    "frame.drone_pod": drone_pod,
    "frame.gyro": gyro,
    "frame.hinge": hinge_x,
    "bastion.arm_rotor": rotor_y,
    "bastion.arm_hinge": hinge_x,
    "bastion.torso": torso,
    "bastion.head": head,
    "bastion.waist": waist,
    "bastion.shoulder": shoulder,
    "bastion.limb": limb,
    "bastion.leg": lambda h: limb(h, knee=True),
    "bastion.foot": foot,
    "bastion.link_front": boom,
    "bastion.link_back": boom,
    "bastion.carrier": carrier,
    "bastion.drop_front": boom,
    "bastion.drop_back": boom,
    "bastion.bend": hinge_x,
    "bastion.fold_knee": hinge_x,
    "bastion.leg_1": boom,
    "bastion.leg_2": boom,
    "bastion.leg_3": boom,
    "bastion.leg_4": boom,
    "bastion.leg_5": boom,
    "bastion.leg_6": boom,
    "bastion.leg_7": boom,
    "bastion.grapple": grapple,
    "bastion.saddle": saddle,
    "bastion.shell_quarter": shell_quarter,
    "thruster.main.medium": engine,
}


# --- glTF binary ------------------------------------------------------------------------------


def triangles(model):
    """Flat-shaded triangles per material: positions, normals, indices."""
    out = {}
    for material, polys in model.polys.items():
        pos, nrm, idx = [], [], []
        for points, centre in polys:
            n = [0.0, 0.0, 0.0]
            for k in range(1, len(points) - 1):
                c = cross(sub(points[k], points[0]), sub(points[k + 1], points[0]))
                n = [n[0] + c[0], n[1] + c[1], n[2] + c[2]]
            if dot(n, n) < 1e-18:
                continue  # degenerate
            mid = [sum(p[i] for p in points) / len(points) for i in range(3)]
            if dot(n, sub(mid, centre)) < 0:
                points = points[::-1]
                n = [-n[0], -n[1], -n[2]]
            n = norm(n)
            base = len(pos)
            pos.extend(points)
            nrm.extend([n] * len(points))
            for k in range(1, len(points) - 1):
                idx.extend([base, base + k, base + k + 1])
        out[material] = (pos, nrm, idx)
    return out


def glb_smooth(model):
    return glb(None, {m: v for m, v in model.parts.items()})


def glb(model, tris=None):
    tris = tris if tris is not None else triangles(model)
    binary = bytearray()
    views, accessors, primitives, materials = [], [], [], []
    for material, (pos, nrm, idx) in sorted(tris.items()):
        def view(data, target):
            while len(binary) % 4:
                binary.append(0)
            views.append({"buffer": 0, "byteOffset": len(binary), "byteLength": len(data),
                          "target": target})
            binary.extend(data)
            return len(views) - 1

        p = view(struct.pack(f"<{len(pos) * 3}f", *[c for v in pos for c in v]), 34962)
        lo = [min(v[i] for v in pos) for i in range(3)]
        hi = [max(v[i] for v in pos) for i in range(3)]
        accessors.append({"bufferView": p, "componentType": 5126, "count": len(pos),
                          "type": "VEC3", "min": lo, "max": hi})
        n = view(struct.pack(f"<{len(nrm) * 3}f", *[c for v in nrm for c in v]), 34962)
        accessors.append({"bufferView": n, "componentType": 5126, "count": len(nrm),
                          "type": "VEC3"})
        i = view(struct.pack(f"<{len(idx)}I", *idx), 34963)
        accessors.append({"bufferView": i, "componentType": 5125, "count": len(idx),
                          "type": "SCALAR"})
        materials.append({"name": material, "pbrMetallicRoughness": {
            "baseColorFactor": COLOURS[material], "metallicFactor": 0.3,
            "roughnessFactor": 0.6}})
        primitives.append({"attributes": {"POSITION": len(accessors) - 3,
                                          "NORMAL": len(accessors) - 2},
                           "indices": len(accessors) - 1, "material": len(materials) - 1})
    while len(binary) % 4:
        binary.append(0)
    doc = {
        "asset": {"version": "2.0", "generator": "tools/gen-models.py"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": primitives}],
        "materials": materials,
        "buffers": [{"byteLength": len(binary)}],
        "bufferViews": views,
        "accessors": accessors,
    }
    text = json.dumps(doc, separators=(",", ":")).encode()
    while len(text) % 4:
        text += b" "
    chunks = struct.pack("<II", len(text), 0x4E4F534A) + text
    chunks += struct.pack("<II", len(binary), 0x004E4942) + bytes(binary)
    return struct.pack("<III", 0x46546C67, 2, 12 + len(chunks)) + chunks


def main():
    check = "--check" in sys.argv[1:]
    parts = {}
    for path in sorted(PARTS.glob("*.json")):
        for part in json.loads(path.read_text())["parts"]:
            form = part.get("form") or {}
            if form.get("model") in MODELS:
                parts[form["model"]] = part
    OUT.mkdir(parents=True, exist_ok=True)
    stale = []
    for name, part in sorted(parts.items()):
        builder = MODELS[name]
        built = builder(part) if builder is shell_quarter else builder(size_of(part))
        data = glb_smooth(built) if isinstance(built, Smooth) else glb(built)
        path = OUT / f"{name}.glb"
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    if check:
        if stale:
            print("gen-models: out of date (run tools/gen-models.py):", ", ".join(stale))
            sys.exit(1)
        print(f"gen-models: ok ({len(parts)} models)")
    else:
        print(f"gen-models: wrote {len(parts)} models to {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
