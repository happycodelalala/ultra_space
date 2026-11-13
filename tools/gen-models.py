#!/usr/bin/env python3
"""Generates the low-poly part models (task 3.12) as glTF binaries.

Original, flat-shaded mechanical models built from simple solids (bevelled boxes, tapered
blocks, n-sided prisms placed in any direction) with mechanical details (clevises, bearing
caps, bolt rings, hydraulic rams, hoses, vents); a joint's actuators sit inside its own part,
since every part's model is rigid. Sized from each part's collision shape in `content/data/parts/*.json`, in part
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


# --- Placing solids: oriented boxes, tubes and mechanical details ----------------------------


def add3(a, b):
    return [a[0] + b[0], a[1] + b[1], a[2] + b[2]]


def mul3(a, k):
    return [a[0] * k, a[1] * k, a[2] * k]


def lerp(a, b, t):
    return [a[i] + (b[i] - a[i]) * t for i in range(3)]


def length(a):
    return math.sqrt(dot(a, a))


AXES = ([1, 0, 0], [0, 1, 0], [0, 0, 1])


def basis(d, hint=None):
    """Right-handed axes (u, v, w) with v along `d` and u as near `hint` as it gets."""
    v = norm(d)
    h = hint if hint is not None else ([1, 0, 0] if abs(v[0]) < 0.9 else [0, 0, 1])
    u = sub(h, mul3(v, dot(h, v)))
    if dot(u, u) < 1e-9:
        u = [0, 0, 1] if abs(v[2]) < 0.9 else [1, 0, 0]
        u = sub(u, mul3(v, dot(u, v)))
    u = norm(u)
    return u, v, cross(u, v)


def place(solid, origin, axes):
    """A solid built in local axes, moved to `origin` with its X, Y, Z along `axes`."""
    polys, centre = solid
    u, v, w = axes

    def f(p):
        return [origin[i] + p[0] * u[i] + p[1] * v[i] + p[2] * w[i] for i in range(3)]

    return [[f(q) for q in poly] for poly in polys], f(centre)


class Mech(Model):
    """A model with mechanical building blocks."""

    def block(self, mat, half, centre=(0, 0, 0), bevel=0.04, axes=AXES, taper=(1.0, 1.0),
              squash=None):
        self.add(mat, *place(bevel_box(half, bevel, taper=taper, squash=squash), centre, axes))

    def tube(self, mat, a, b, r, sides=12, taper=1.0, hint=None):
        """A prism from `a` to `b` (radius `r` at `a`, `r * taper` at `b`)."""
        d = sub(b, a)
        half = length(d) / 2
        if half < 1e-6:
            return
        solid = prism(r, half, sides, 1, taper=taper, rotate=math.pi / sides)
        self.add(mat, *place(solid, lerp(a, b, 0.5), basis(d, hint)))

    def disc(self, mat, centre, axis, r, thickness, sides=16):
        a = norm(axis)
        self.tube(mat, sub(centre, mul3(a, thickness / 2)), add3(centre, mul3(a, thickness / 2)), r,
                  sides)

    def bolts(self, centre, axis, radius, count, size=0.04, mat="trim", phase=0.0):
        """A ring of hex bolt heads on a face (centre on the face, `axis` out of it)."""
        u, v, w = basis(axis)
        for k in range(count):
            a = 2 * math.pi * (k + phase) / count
            p = add3(centre, add3(mul3(u, radius * math.cos(a)), mul3(w, radius * math.sin(a))))
            self.tube(mat, p, add3(p, mul3(v, size * 0.8)), size, 6)

    def ram(self, a, b, r, stroke=0.5):
        """A hydraulic ram from `a` (barrel end) to `b` (rod end): barrel, gland, polished rod
        and a clevis eye at each end."""
        d = norm(sub(b, a))
        split = lerp(a, b, 1 - stroke)
        self.tube("dark", a, split, r, 10)
        self.tube("trim", sub(split, mul3(d, r * 0.4)), split, r * 1.15, 10)
        self.tube("armor", split, sub(b, mul3(d, r * 0.6)), r * 0.48, 8)
        u, _, _ = basis(d)
        for p in (a, b):
            self.tube("dark", sub(p, mul3(u, r * 0.9)), add3(p, mul3(u, r * 0.9)), r * 0.75, 8)

    def hose(self, points, r=0.045, mat="dark"):
        """A hose through `points` (a smooth curve through them)."""
        curve = []
        for i in range(len(points) - 1):
            p0 = points[max(0, i - 1)]
            p1, p2 = points[i], points[i + 1]
            p3 = points[min(len(points) - 1, i + 2)]
            for k in range(4):
                t = k / 4
                curve.append([0.5 * (2 * p1[j] + (p2[j] - p0[j]) * t
                                     + (2 * p0[j] - 5 * p1[j] + 4 * p2[j] - p3[j]) * t * t
                                     + (3 * p1[j] - p0[j] - 3 * p2[j] + p3[j]) * t ** 3)
                              for j in range(3)])
        curve.append(points[-1])
        for p, q in zip(curve, curve[1:]):
            self.tube(mat, p, q, r, 6)

    def vents(self, centre, axes, width, height, slats, mat="dark"):
        """Louvred vent on a face: a frame and angled slats (axes: along, up, out)."""
        u, v, w = axes
        self.block("trim", [width / 2 + 0.04, height / 2 + 0.04, 0.02], centre, 0.01, axes)
        for k in range(slats):
            y = -height / 2 + (k + 0.5) * height / slats
            c = add3(add3(centre, mul3(v, y)), mul3(w, 0.03))
            tilt = (u, norm(add3(v, mul3(w, 0.6))), norm(sub(w, mul3(v, 0.6))))
            self.block(mat, [width / 2, height / slats * 0.32, 0.02], c, 0.005, tilt)


def nodes_of(part):
    return {n["id"]: (n["position_m"], norm(n["direction"])) for n in part.get("attach_nodes", [])}


def extent(h, d):
    """Half extent of the box with half sizes `h` along unit direction `d`."""
    return abs(d[0]) * h[0] + abs(d[1]) * h[1] + abs(d[2]) * h[2]


# --- Joints -----------------------------------------------------------------------------------


def joint(h, part):
    """A hinge: a clevis from the base flange down to the pivot (the end node, where the joint
    turns), a knuckle drum on the pivot pin with bearing caps, and on one side the rotary vane
    actuator that turns it, fed by two hoses from a valve manifold on the flange."""
    a = norm(part["joint"]["axis"])
    nodes = nodes_of(part)
    pb, db = nodes["base"]
    pivot = nodes["end"][0]
    v = sub(db, mul3(a, dot(db, a)))
    v = norm(v) if dot(v, v) > 1e-6 else basis(a)[2]
    w = cross(a, v)
    axes = (a, v, w)
    half_len = extent(h, a)
    r = max(0.16, 0.85 * extent(h, w))
    pin = 0.6 * half_len
    cheek = max(0.05, 0.16 * half_len)
    height = max(dot(sub(pb, pivot), v), r * 0.5)
    m = Mech()
    # Knuckle drum and its bearing caps.
    m.tube("dark", sub(pivot, mul3(a, pin)), add3(pivot, mul3(a, pin)), r, 16)
    for s in (-1, 1):
        m.disc("armor", add3(pivot, mul3(a, s * pin * 0.55)), a, r * 1.04, pin * 0.18, 16)
    # Clevis cheeks: a plate each side from the flange down, rounded about the pin.
    for s in (-1, 1):
        off = mul3(a, s * (pin + cheek / 2))
        mid = add3(add3(pivot, mul3(v, height / 2)), off)
        m.block("armor", [cheek / 2, height / 2, r * 0.9], mid, 0.02, axes)
        m.disc("armor", add3(pivot, off), a, r * 0.92, cheek, 16)
        cap = add3(pivot, mul3(a, s * (pin + cheek + 0.02)))
        m.disc("trim", cap, a, r * 0.5, 0.04, 12)
        m.bolts(add3(cap, mul3(a, s * 0.02)), mul3(a, s), r * 0.36, 6, size=min(0.04, r * 0.1))
    # Flange on the base node.
    top = add3(pivot, mul3(v, height))
    m.block("armor", [pin + cheek, 0.05, r * 0.95], sub(top, mul3(v, 0.05)), 0.02, axes)
    # Vane actuator on the +axis side, the manifold and hoses feeding it.
    act0 = add3(pivot, mul3(a, pin + cheek))
    act1 = add3(pivot, mul3(a, pin + cheek + max(0.1, 0.24 * half_len)))
    m.tube("dark", act0, act1, r * 0.72, 12)
    m.disc("trim", act1, a, r * 0.5, 0.03, 12)
    man = add3(add3(top, mul3(v, -0.14)), mul3(a, pin + cheek + 0.1))
    m.block("dark", [0.07, 0.08, r * 0.35], man, 0.015, axes)
    for s in (-1, 1):
        port = add3(add3(act1, mul3(w, s * r * 0.3)), mul3(v, r * 0.25))
        m.hose([add3(man, mul3(w, s * r * 0.2)),
                add3(add3(man, mul3(a, 0.08)), mul3(add3(w, mul3(v, -1.0)), s * r * 0.25)),
                port], r=min(0.04, r * 0.09))
    return m


def fold_knee(h, part):
    """A folding knee: a U-bracket under the two side-by-side segments, a lug up to the base
    segment, the knuckle and its caps on the pivot, and a ram along the bracket driving the
    crank that unfolds the knee."""
    hx, hy, hz = h
    nodes = nodes_of(part)
    pb = nodes["base"][0]
    pe = nodes["end"][0]
    m = Mech()
    m.block("armor", [hx, hy * 0.55, hz * 0.85], (0, -hy * 0.4, 0), 0.06)
    m.block("armor", [hx * 0.42, hy * 0.6, hz * 0.8], (pb[0], pb[1] - hy * 0.55, 0), 0.04)
    r = hz * 0.7
    m.block("armor", [hx * 0.42, hy * 0.35, hz * 0.7], (pe[0], pe[1] - hy * 0.6, 0), 0.04)
    m.tube("dark", (pe[0] - hx * 0.4, pe[1], 0), (pe[0] + hx * 0.4, pe[1], 0), r, 14)
    for s in (-1, 1):
        cap = (pe[0] + s * hx * 0.44, pe[1], 0)
        m.disc("trim", cap, (1, 0, 0), r * 0.55, 0.05, 12)
        m.bolts(add3(cap, (s * 0.025, 0, 0)), (s, 0, 0), r * 0.38, 5, size=0.03)
    m.ram((-hx * 0.75, -hy * 0.3, -hz * 0.95), (hx * 0.55, -hy * 0.3, -hz * 0.95), hz * 0.16)
    m.block("trim", [hx * 0.9, 0.03, 0.02], (0, -hy * 0.85, hz * 0.86), 0.005)
    return m


def rotor(h, part):
    """A rotary joint about its own length: a motor housing with cooling fins, a slewing
    bearing ring and the turning face plate, bolted."""
    hx, hy, hz = h
    r = min(hx, hz)
    m = Mech()
    m.disc("armor", (0, hy - 0.04, 0), (0, 1, 0), r * 1.0, 0.08, 16)
    m.bolts((0, hy, 0), (0, 1, 0), r * 0.8, 8, size=0.03)
    m.tube("dark", (0, hy - 0.08, 0), (0, -hy * 0.3, 0), r * 0.82, 16)
    for k in range(12):
        ang = 2 * math.pi * k / 12
        d = (math.sin(ang), 0, math.cos(ang))
        m.block("dark", [0.015, hy * 0.45, r * 0.12], (d[0] * r * 0.9, hy * 0.25, d[2] * r * 0.9),
                0.005, basis((0, 1, 0), (d[2], 0, -d[0])))
    m.disc("trim", (0, -hy * 0.38, 0), (0, 1, 0), r * 0.98, 0.07, 20)
    m.disc("armor", (0, -hy * 0.75, 0), (0, 1, 0), r * 0.88, hy * 0.5, 16)
    m.bolts((0, -hy, 0), (0, -1, 0), r * 0.62, 8, size=0.03, phase=0.5)
    return m


# --- Arm and leg segments ---------------------------------------------------------------------


def beam(h, part):
    """A heavy arm segment: a box beam of four corner longerons over a ribbed web with
    diagonal bracing, pin bosses at both ends, its own ram along the back (it drives the next
    joint through a crank) and a hose run along the front."""
    hx, hy, hz = h
    m = Mech()
    m.block("dark", [hx * 0.66, hy * 0.92, hz * 0.66], bevel=0.03)
    for sx in (-1, 1):
        for sz in (-1, 1):
            m.block("armor", [hx * 0.2, hy * 0.88, hz * 0.2], (sx * hx * 0.72, 0, sz * hz * 0.72), 0.04)
    bays = max(2, round(2 * hy / 1.2))
    ys = [-hy * 0.8 + k * 1.6 * hy / bays for k in range(bays + 1)]
    for y in ys:
        m.block("armor", [hx * 0.88, 0.06, hz * 0.88], (0, y, 0), 0.02)
    for y0, y1 in zip(ys, ys[1:]):
        for sx in (-1, 1):
            m.tube("armor", (sx * hx * 0.74, y0, -hz * 0.55), (sx * hx * 0.74, y1, hz * 0.55),
                   0.045, 4)
    r = min(hz * 0.62, hy * 0.25)
    for s in (-1, 1):
        y = s * (hy - r * 0.7)
        m.tube("dark", (-hx * 1.04, y, 0), (hx * 1.04, y, 0), r, 14)
        for sx in (-1, 1):
            m.disc("trim", (sx * hx * 1.05, y, 0), (1, 0, 0), r * 0.5, 0.04, 12)
    rr = hx * 0.15
    m.ram((0, hy * 0.72, hz * 0.98), (0, -hy * 0.7, hz * 0.98), rr, stroke=0.45)
    for y in (hy * 0.72, -hy * 0.7):
        m.block("armor", [rr * 1.2, rr * 0.9, hz * 0.12], (0, y, hz * 0.86), 0.02)
    m.hose([(hx * 0.4, hy * 0.9, -hz * 0.95), (hx * 0.4, 0, -hz * 0.98),
            (hx * 0.4, -hy * 0.9, -hz * 0.95)], r=0.045)
    for y in ys[1:-1]:
        m.block("dark", [0.07, 0.05, 0.06], (hx * 0.4, y, -hz * 0.94), 0.01)
    m.block("trim", [hx * 0.9, 0.05, hz * 0.9], (0, hy * 0.55, 0), 0.01)
    return m


def spider(h, part):
    """A folding-leg segment: a tapered armoured tube with sleeves and pin bosses at both
    ends, an outer fairing, a slim fold ram near the lower knee and a hydraulic line clamped
    along it."""
    hx, hy, hz = h
    r = hx * 0.6
    m = Mech()
    m.tube("armor", (0, -hy * 0.92, 0), (0, hy * 0.92, 0), r, 8, taper=1.08)
    for s in (-1, 1):
        y = s * hy * 0.8
        m.tube("dark", (0, y - hy * 0.1, 0), (0, y + hy * 0.1, 0), r * 1.22, 8)
        m.disc("trim", (0, y - s * hy * 0.11, 0), (0, 1, 0), r * 1.28, 0.05, 8)
        yb = s * (hy - r * 0.55)
        m.tube("dark", (-hx * 1.02, yb, 0), (hx * 1.02, yb, 0), r * 0.8, 10)
    m.block("armor", [r * 0.75, hy * 0.5, 0.05], (0, 0, r * 1.02), 0.02, taper=(0.7, 1.0))
    m.block("trim", [r * 0.3, hy * 0.08, 0.02], (0, hy * 0.3, r * 1.08), 0.005)
    m.ram((r * 1.25, hy * 0.25, 0), (r * 1.25, -hy * 0.82, 0), r * 0.24, stroke=0.4)
    m.hose([(-r * 1.15, hy * 0.85, 0), (-r * 1.12, 0, 0), (-r * 1.15, -hy * 0.85, 0)],
           r=max(0.03, r * 0.1))
    for y in (-hy * 0.45, 0.0, hy * 0.45):
        m.block("dark", [0.05, 0.04, 0.05], (-r * 1.05, y, 0), 0.01)
    return m


# --- Shell arm and leg mounts -----------------------------------------------------------------


def carrier(h, part):
    """The bracket the quarter is bolted to: a hub on the wrist, struts out to a big mounting
    pad on the shell and to the leg's deploy hinge, flanges bolted at each, and the valve
    block that feeds the leg and the quarter's engines."""
    hx, hy, hz = h
    nodes = nodes_of(part)
    m = Mech()
    m.block("armor", [hx * 0.72, hy * 0.72, hz * 0.72], bevel=0.12)
    m.block("dark", [hx * 0.8, hy * 0.3, hz * 0.6], (0, hy * 0.1, 0), 0.03)
    for name, (p, d) in nodes.items():
        m.tube("armor", mul3(d, 0.1), sub(p, mul3(d, 0.08)), 0.3, 10)
        m.disc("armor", sub(p, mul3(d, 0.04)), d, 0.48, 0.08, 16)
        m.bolts(p, d, 0.38, 8, size=0.035)
        if name == "shell":
            m.block("armor", [0.75, 0.06, 0.6], sub(p, mul3(d, 0.1)), 0.02, basis(d, (1, 0, 0)))
            for s in (-1, 1):
                gusset = (s * 0.45, 0, 0)
                m.tube("armor", gusset, add3(sub(p, mul3(d, 0.12)), (s * 0.55, 0, 0)), 0.08, 6)
    valve = (hx * 0.85, -hy * 0.2, 0)
    m.block("dark", [0.1, 0.18, 0.22], valve, 0.02)
    for name in ("shell", "leg"):
        if name in nodes:
            p, d = nodes[name]
            m.hose([add3(valve, (0.08, 0, 0.1 if name == "shell" else -0.1)),
                    add3(lerp(valve, p, 0.5), (0.25, 0, 0)), sub(p, mul3(d, 0.15))], r=0.04)
    return m


def saddle(h, part):
    """A shoulder saddle: a base plate bolted round its edge to the shoulder block, a
    pedestal with gussets up to each shell arm's shoulder hinge, and the manifold feeding
    both arms."""
    hx, hy, hz = h
    nodes = nodes_of(part)
    m = Mech()
    m.block("armor", [hx, hy * 0.45, hz], (0, -hy * 0.55, 0), 0.08)
    m.block("dark", [hx * 0.8, hy * 0.2, hz * 0.85], (0, -hy * 0.05, 0), 0.03)
    for sx in (-1, 1):
        for k in range(6):
            z = -hz * 0.85 + k * hz * 1.7 / 5
            m.tube("trim", (sx * hx * 0.85, -hy * 0.1, z), (sx * hx * 0.85, -hy * 0.02, z), 0.045, 6)
    for name, (p, d) in nodes.items():
        foot = (p[0], -hy * 0.1, p[2])
        m.tube("armor", foot, sub(p, mul3(d, 0.06)), 0.42, 12)
        m.disc("armor", sub(p, mul3(d, 0.04)), d, 0.52, 0.08, 16)
        m.bolts(p, d, 0.43, 8, size=0.035)
        for s in (-1, 1):
            m.block("armor", [0.05, hy * 0.35, 0.3], (p[0] + s * 0.42, 0.05, p[2]), 0.01)
    m.block("dark", [hx * 0.35, hy * 0.3, 0.35], (0, hy * 0.15, 0), 0.03)
    for name, (p, d) in nodes.items():
        m.hose([(0, hy * 0.25, 0.3 if p[2] > 0 else -0.3), (hx * 0.4, hy * 0.6, p[2] * 0.6),
                add3(p, (0.45, -0.2, 0))], r=0.05)
    m.block("trim", [hx * 0.95, 0.03, 0.05], (0, -hy * 0.1, -hz * 0.98), 0.005)
    m.block("trim", [hx * 0.95, 0.03, 0.05], (0, -hy * 0.1, hz * 0.98), 0.005)
    return m


# --- Hands, grapples and engines --------------------------------------------------------------


def grapple(h, part):
    """A three-claw grapple: a wrist flange and bearing, a palm hub, and three claws of two
    jointed fingers each, every claw closed by its own small ram."""
    hx, hy, hz = h
    m = Mech()
    m.disc("armor", (0, hy * 0.9, 0), (0, 1, 0), hx * 0.85, hy * 0.18, 12)
    m.bolts((0, hy, 0), (0, 1, 0), hx * 0.65, 6, size=0.03)
    m.disc("trim", (0, hy * 0.72, 0), (0, 1, 0), hx * 0.8, 0.05, 16)
    m.tube("dark", (0, hy * 0.7, 0), (0, hy * 0.2, 0), hx * 0.7, 6, taper=1.1)
    m.tube("armor", (0, hy * 0.25, 0), (0, hy * 0.05, 0), hx * 0.82, 6, taper=0.8)
    for k in range(3):
        ang = 2 * math.pi * k / 3
        rd = (math.sin(ang), 0, math.cos(ang))
        tg = (math.cos(ang), 0, -math.sin(ang))
        k0 = add3(mul3(rd, hx * 0.62), (0, hy * 0.12, 0))
        k1 = add3(mul3(rd, hx * 0.95), (0, -hy * 0.42, 0))
        tip = add3(mul3(rd, hx * 0.2), (0, -hy * 0.98, 0))
        m.tube("dark", sub(k0, mul3(tg, 0.12)), add3(k0, mul3(tg, 0.12)), 0.08, 8)
        m.tube("armor", k0, k1, hx * 0.17, 6, taper=0.85, hint=tg)
        m.tube("dark", sub(k1, mul3(tg, 0.1)), add3(k1, mul3(tg, 0.1)), 0.065, 8)
        m.tube("armor", k1, tip, hx * 0.14, 6, taper=0.3, hint=tg)
        m.ram(add3(mul3(rd, hx * 0.3), (0, hy * 0.45, 0)), lerp(k0, k1, 0.45), 0.035)
    return m


def hand(h, part):
    """A heavy hand: a wrist bearing, a palm with the finger actuators inside, three
    three-jointed fingers in a row curling toward the palm side (+X) and an opposed thumb.
    Symmetric front to back, so the same part serves both arms."""
    hx, hy, hz = h
    m = Mech()
    m.disc("dark", (0, hy * 0.85, 0), (0, 1, 0), hx * 0.7, hy * 0.3, 14)
    m.disc("trim", (0, hy * 0.68, 0), (0, 1, 0), hx * 0.78, 0.06, 14)
    m.block("armor", [hx * 0.55, hy * 0.28, hz * 0.85], (0, hy * 0.38, 0), 0.08)
    m.block("armor", [0.08, hy * 0.26, hz * 0.75], (-hx * 0.6, hy * 0.38, 0), 0.04)
    m.block("trim", [0.03, hy * 0.06, hz * 0.6], (-hx * 0.69, hy * 0.5, 0), 0.01)
    m.vents((hx * 0.57, hy * 0.4, 0), ((0, 0, 1), (0, 1, 0), (1, 0, 0)), hz * 0.8, hy * 0.26, 4)
    seg = 0.19
    for z in (-hz * 0.58, 0.0, hz * 0.58):
        pts = [(-hx * 0.3, hy * 0.1, z), (-hx * 0.42, -hy * 0.35, z), (-hx * 0.12, -hy * 0.7, z),
               (hx * 0.3, -hy * 0.86, z)]
        m.tube("dark", (pts[0][0], pts[0][1], z - seg * 1.3), (pts[0][0], pts[0][1], z + seg * 1.3),
               seg * 1.1, 10)
        for i, (p, q) in enumerate(zip(pts, pts[1:])):
            m.tube("armor", p, q, seg * (1.0 - 0.12 * i), 6, taper=0.9, hint=(0, 0, 1))
            m.tube("dark", (q[0], q[1], z - seg), (q[0], q[1], z + seg), seg * 0.75, 8)
    thumb = [(hx * 0.45, hy * 0.12, 0), (hx * 0.62, -hy * 0.3, 0), (hx * 0.42, -hy * 0.6, 0)]
    m.tube("dark", (thumb[0][0], thumb[0][1], -seg * 1.3), (thumb[0][0], thumb[0][1], seg * 1.3),
           seg * 1.1, 10)
    for p, q in zip(thumb, thumb[1:]):
        m.tube("armor", p, q, seg * 1.05, 6, taper=0.85, hint=(0, 0, 1))
    return m


def engine(h, part=None):
    """A gimballed engine: mounting plate on top (bolted to the structure), turbopump housing,
    gimbal ring on two trunnions, and a bell nozzle with cooling bands flaring to the exit
    (-Y)."""
    r, hl, _ = h
    m = Mech()
    m.block("armor", [r * 0.8, 0.04, r * 0.8], (0, hl - 0.04, 0), 0.02)
    m.bolts((0, hl, 0), (0, 1, 0), r * 0.7, 8, size=0.03)
    m.tube("armor", (0, hl - 0.08, 0), (0, hl * 0.15, 0), r * 0.6, 12)
    m.block("dark", [r * 0.18, hl * 0.25, r * 0.16], (r * 0.62, hl * 0.45, 0), 0.02)
    m.disc("trim", (0, hl * 0.08, 0), (0, 1, 0), r * 0.66, hl * 0.1, 16)
    for s in (-1, 1):
        m.tube("dark", (s * r * 0.6, hl * 0.08, 0), (s * r * 0.78, hl * 0.08, 0), r * 0.08, 8)
    throat = r * 0.36
    m.tube("dark", (0, hl * 0.02, 0), (0, -hl * 0.2, 0), throat, 16, taper=0.9)
    m.tube("dark", (0, -hl * 0.2, 0), (0, -hl * 0.6, 0), throat * 0.9, 16, taper=1.6)
    m.tube("dark", (0, -hl * 0.6, 0), (0, -hl, 0), throat * 1.44, 16, taper=1.4)
    for y, rr in ((-hl * 0.45, throat * 1.25), (-hl * 0.8, throat * 1.75)):
        m.disc("trim", (0, y, 0), (0, 1, 0), rr, 0.03, 16)
    m.hose([(r * 0.62, hl * 0.25, 0.1), (r * 0.7, -hl * 0.1, 0.15), (throat * 1.2, -hl * 0.3, 0.1)],
           r=0.035)
    return m


# --- Body -------------------------------------------------------------------------------------


def torso(h, part=None):
    """The armoured torso and cockpit: an inner frame, sloped chest plates over the cockpit
    with its hatch and viewport, overlapping abdomen plates, louvred side intakes, the
    reactor housing with radiator fins on the back, shoulder bearings and a boarding ladder."""
    hx, hy, hz = h
    m = Mech()
    m.block("dark", [hx * 0.86, hy * 0.96, hz * 0.84], bevel=0.3)

    def slope(p):
        # The chest leans back toward the top.
        if p[2] < 0:
            p[2] += (p[1] + hy * 0.45) / (hy * 0.9) * hz * 0.12
        return p

    m.block("armor", [hx * 0.93, hy * 0.42, hz * 0.55], (0, hy * 0.44, -hz * 0.4), 0.25, squash=slope)
    m.block("armor", [hx * 0.9, hy * 0.42, hz * 0.4], (0, hy * 0.44, hz * 0.5), 0.2)
    hatch = (0, hy * 0.48, -hz * 0.99)
    m.block("trim", [hx * 0.36, hy * 0.24, 0.05], hatch, 0.04)
    m.block("armor", [hx * 0.32, hy * 0.2, 0.06], add3(hatch, (0, 0, -0.04)), 0.04)
    m.block("glass", [hx * 0.26, hy * 0.05, 0.03], add3(hatch, (0, hy * 0.1, -0.1)), 0.01)
    m.tube("dark", (-hx * 0.3, hatch[1] - hy * 0.25, hatch[2] - 0.05),
           (hx * 0.3, hatch[1] - hy * 0.25, hatch[2] - 0.05), 0.07, 8)
    for k in range(4):
        y = -hy * 0.12 - k * hy * 0.2
        m.block("armor", [hx * (0.62 - 0.05 * k), hy * 0.1, hz * 0.18], (0, y, -hz * 0.86), 0.06,
                taper=(0.95, 1.0))
    for s in (-1, 1):
        m.block("armor", [0.12, hy * 0.36, hz * 0.6], (s * hx * 0.9, -hy * 0.42, 0), 0.06)
        m.vents((s * hx * 1.0, -hy * 0.4, -hz * 0.1), ((0, 0, 1), (0, 1, 0), (s, 0, 0)),
                hz * 0.7, hy * 0.35, 5)
        m.tube("dark", (s * hx * 0.75, hy * 0.57, 0), (s * hx * 1.0, hy * 0.57, 0), hy * 0.27, 16)
        m.disc("trim", (s * hx * 1.0, hy * 0.57, 0), (1, 0, 0), hy * 0.2, 0.05, 16)
        m.block("glass", [hx * 0.06, 0.06, 0.04], (s * hx * 0.75, hy * 0.82, -hz * 0.92), 0.01)
    # Reactor housing and radiator on the back.
    m.block("armor", [hx * 0.55, hy * 0.55, hz * 0.2], (0, -hy * 0.05, hz * 0.78), 0.12)
    for k in range(9):
        x = -hx * 0.45 + k * hx * 0.9 / 8
        m.block("dark", [0.025, hy * 0.42, hz * 0.08], (x, -hy * 0.05, hz * 0.98), 0.005)
    for s in (-1, 1):
        m.disc("trim", (s * hx * 0.3, -hy * 0.68, hz * 0.98), (0, 0, 1), hy * 0.09, 0.08, 12)
    # Collar around the neck, ladder rungs up the abdomen.
    m.tube("trim", (0, hy * 0.92, 0), (0, hy * 1.02, 0), hx * 0.42, 16)
    for k in range(6):
        y = -hy * 0.9 + k * hy * 0.17
        m.tube("trim", (hx * 0.62, y, -hz * 0.98), (hx * 0.8, y, -hz * 0.98), 0.035, 6)
    return m


def head(h, part=None):
    """The sensor head: an armoured housing with a wrap-around visor, the main optical sensor,
    side sensor pods, an antenna mast and the neck bearing."""
    hx, hy, hz = h
    m = Mech()
    m.disc("dark", (0, -hy * 0.88, 0), (0, 1, 0), hx * 0.6, hy * 0.24, 14)

    def chin(p):
        if p[2] < 0 and p[1] < 0:
            p[2] += (-p[1] / hy) * hz * 0.18
        return p

    m.block("armor", [hx * 0.85, hy * 0.62, hz * 0.88], (0, -hy * 0.1, 0), 0.2, taper=(1.0, 0.82),
            squash=chin)
    m.block("glass", [hx * 0.62, hy * 0.1, hz * 0.06], (0, hy * 0.12, -hz * 0.76), 0.03)
    for s in (-1, 1):
        m.block("glass", [0.04, hy * 0.08, hz * 0.3], (s * hx * 0.75, hy * 0.12, -hz * 0.42), 0.01)
    m.disc("dark", (0, -hy * 0.2, -hz * 0.9), (0, 0, 1), hx * 0.24, 0.12, 12)
    m.disc("glass", (0, -hy * 0.2, -hz * 0.97), (0, 0, 1), hx * 0.15, 0.04, 12)
    for s in (-1, 1):
        m.tube("dark", (s * hx * 0.8, -hy * 0.05, 0.1), (s * hx * 1.02, -hy * 0.05, 0.1), hy * 0.2, 10)
        m.disc("glass", (s * hx * 1.03, -hy * 0.05, 0.1), (1, 0, 0), hy * 0.12, 0.03, 10)
    m.block("armor", [hx * 0.18, hy * 0.08, hz * 0.6], (0, hy * 0.56, 0.0), 0.04)
    m.tube("dark", (hx * 0.45, hy * 0.5, hz * 0.45), (hx * 0.45, hy * 1.15, hz * 0.45), 0.035, 6)
    m.tube("trim", (hx * 0.45, hy * 1.15, hz * 0.45), (hx * 0.45, hy * 1.25, hz * 0.45), 0.06, 6)
    m.vents((0, hy * 0.3, hz * 0.88), ((1, 0, 0), (0, 1, 0), (0, 0, 1)), hx * 0.9, hy * 0.35, 4)
    return m


def waist(h, part=None):
    """The pelvis: a slewing ring under the torso, the frame carrying both hip actuators, and
    armour skirts front and back hinged to clear the thighs."""
    hx, hy, hz = h
    m = Mech()
    m.disc("trim", (0, hy * 0.85, 0), (0, 1, 0), min(hx, hz) * 0.8, hy * 0.2, 20)
    m.disc("dark", (0, hy * 0.55, 0), (0, 1, 0), min(hx, hz) * 0.72, hy * 0.4, 20)
    m.block("dark", [hx * 0.75, hy * 0.5, hz * 0.7], (0, -hy * 0.1, 0), 0.1)
    m.block("armor", [hx * 0.32, hy * 0.62, hz * 0.95], (0, -hy * 0.15, 0), 0.12)
    for s in (-1, 1):
        x = s * hx * 0.67
        m.tube("dark", (x - hx * 0.24, -hy * 0.45, 0), (x + hx * 0.24, -hy * 0.45, 0), hy * 0.45, 14)
        m.disc("armor", (x + s * hx * 0.26, -hy * 0.45, 0), (1, 0, 0), hy * 0.4, 0.08, 14)
        m.bolts((x + s * hx * 0.3, -hy * 0.45, 0), (s, 0, 0), hy * 0.28, 6, size=0.04)
        for sz in (-1, 1):
            m.block("armor", [hx * 0.25, hy * 0.5, 0.08], (x * 0.9, -hy * 0.5, sz * hz * 0.95), 0.04,
                    basis((0, 1, -sz * 0.25), (1, 0, 0)))
    for sz in (-1, 1):
        m.block("armor", [hx * 0.3, hy * 0.6, 0.1], (0, -hy * 0.6, sz * hz * 1.0), 0.05,
                basis((0, 1, -sz * 0.2), (1, 0, 0)), taper=(0.8, 1.0))
    m.block("trim", [hx * 0.18, hy * 0.15, 0.04], (0, -hy * 0.35, -hz * 1.08), 0.01)
    return m


def shoulder(h, part=None):
    """A shoulder block: the bearing turning on the torso, layered pauldron armour over the
    top and the housing of the arm's swing actuator."""
    hx, hy, hz = h
    m = Mech()
    m.block("dark", [hx * 0.7, hy * 0.7, hz * 0.7], bevel=0.12)
    m.tube("dark", (-hx * 1.0, 0, 0), (hx * 1.0, 0, 0), hy * 0.55, 16)
    for s in (-1, 1):
        m.disc("trim", (s * hx * 0.98, 0, 0), (1, 0, 0), hy * 0.4, 0.05, 16)
    for k, (y, sc) in enumerate(((hy * 0.72, 1.0), (hy * 0.42, 1.06), (hy * 0.1, 1.1))):
        m.block("armor", [hx * 0.95 * sc, hy * 0.17, hz * 0.95 * sc], (0, y, 0), 0.08,
                taper=(1.0, 0.9))
    m.tube("dark", (0, -hy * 0.5, 0), (0, -hy * 1.0, 0), hx * 0.5, 14)
    m.block("trim", [hx * 0.98, 0.04, 0.06], (0, hy * 0.25, -hz * 1.04), 0.01)
    return m


def limb(h, part=None, knee=False):
    """An arm or leg segment: a structural spine with pin bosses at both ends, armour shells
    front and outside, and an exposed pair of hydraulic rams down the back that work the
    joint below through a crank."""
    hx, hy, hz = h
    m = Mech()
    m.block("dark", [hx * 0.6, hy * 0.95, hz * 0.6], bevel=0.08)
    r = min(hx, hz) * 0.5
    for s in (-1, 1):
        m.tube("dark", (-hx * 0.85, s * hy * 0.86, 0), (hx * 0.85, s * hy * 0.86, 0), r, 14)
        for sx in (-1, 1):
            m.disc("trim", (sx * hx * 0.86, s * hy * 0.86, 0), (1, 0, 0), r * 0.55, 0.04, 12)
    m.block("armor", [hx * 0.85, hy * 0.68, hz * 0.32], (0, hy * 0.05, -hz * 0.55), 0.15,
            taper=(0.85, 1.0))
    for s in (-1, 1):
        m.block("armor", [hx * 0.2, hy * 0.6, hz * 0.6], (s * hx * 0.72, hy * 0.08, -hz * 0.05), 0.08,
                taper=(0.85, 1.0))
        for k in range(3):
            m.block("dark", [0.03, 0.04, hz * 0.45], (s * hx * 0.9, hy * (0.45 - 0.15 * k), -hz * 0.05),
                    0.01)
        m.vents((s * hx * 0.92, -hy * 0.3, -hz * 0.05), ((0, 0, 1), (0, 1, 0), (s, 0, 0)),
                hz * 0.6, hy * 0.25, 3)
    m.block("armor", [hx * 0.12, hy * 0.6, 0.06], (0, hy * 0.05, -hz * 0.9), 0.04)
    for s in (-1, 1):
        x = s * hx * 0.38
        m.ram((x, hy * 0.62, hz * 0.72), (x, -hy * 0.72, hz * 0.72), min(hx, hz) * 0.13, stroke=0.42)
    m.block("armor", [hx * 0.6, 0.08, hz * 0.2], (0, hy * 0.62, hz * 0.62), 0.03)
    m.hose([(-hx * 0.62, hy * 0.75, hz * 0.3), (-hx * 0.66, 0, hz * 0.4),
            (-hx * 0.62, -hy * 0.75, hz * 0.3)], r=0.05)
    m.block("trim", [hx * 0.32, hy * 0.38, 0.04], (0, hy * 0.08, -hz * 0.88), 0.02)
    if knee:
        def cap(p):
            if p[1] < 0:
                p[2] += p[1] * 0.25
            return p
        m.block("armor", [hx * 0.7, hy * 0.18, hz * 0.3], (0, -hy * 0.72, -hz * 0.75), 0.14,
                squash=cap)
    return m


def foot(h, part=None):
    """A foot: a treaded sole, an ankle shroud over the joint, two toe plates and a heel spur,
    with shock struts between the sole and the upper."""
    hx, hy, hz = h
    m = Mech()
    m.block("dark", [hx, hy * 0.2, hz], (0, -hy * 0.8, 0), 0.06)
    for k in range(7):
        z = -hz * 0.85 + k * hz * 1.7 / 6
        m.block("dark", [hx * 0.92, 0.05, hz * 0.06], (0, -hy * 1.0, z), 0.01)

    def toe(p):
        if p[2] < 0 and p[1] > -hy * 0.1:
            p[1] -= (-p[2] / (hz * 0.45)) * hy * 0.35
        return p

    for s in (-1, 1):
        m.block("armor", [hx * 0.46, hy * 0.35, hz * 0.45], (s * hx * 0.5, -hy * 0.35, -hz * 0.52),
                0.12, squash=toe)
    m.block("armor", [hx * 0.85, hy * 0.45, hz * 0.45], (0, -hy * 0.25, hz * 0.4), 0.15)
    m.block("armor", [hx * 0.4, hy * 0.25, hz * 0.18], (0, -hy * 0.55, hz * 0.95), 0.06)
    m.block("armor", [hx * 0.55, hy * 0.45, hz * 0.4], (0, hy * 0.45, 0), 0.15, taper=(1.0, 0.8))
    m.disc("dark", (0, hy * 0.9, 0), (0, 1, 0), hx * 0.4, hy * 0.15, 14)
    for s in (-1, 1):
        m.ram((s * hx * 0.92, hy * 0.35, hz * 0.05), (s * hx * 0.92, -hy * 0.5, hz * 0.3), 0.09,
              stroke=0.45)
    m.block("trim", [hx * 0.96, 0.04, 0.35], (0, -hy * 0.6, -hz * 0.9), 0.01)
    return m


# --- Other frame parts ------------------------------------------------------------------------


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


# --- Shells -----------------------------------------------------------------------------------


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

    def merge(self, model):
        """Adds a flat-shaded model's faces."""
        for material, (p, n, i) in triangles(model).items():
            pos, nrm, idx = self.parts.setdefault(material, ([], [], []))
            base = len(pos)
            pos.extend(p)
            nrm.extend(n)
            idx.extend(k + base for k in i)


def shell_quarter(h, part):
    """A smooth curved quarter: outer face (armor), inner face (dark), edges (trim); inside,
    stiffening ribs down the curve and two ring frames across it (the engines and the folded
    leg mount on them) and the pad the carrier bolts to; outside, a raised spine down the
    middle and a band at the shoulder line."""
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
    d = Mech()
    coarse = shell_profile(shape["profile_m"], 4)
    for frac in (0.12, 0.5, 0.88):
        deg = a0 + (a1 - a0) * frac
        pts = [point(deg, y, r - t / 2 - 0.1) for y, r in coarse]
        pts[0], pts[-1] = lerp(pts[0], pts[1], 0.2), lerp(pts[-1], pts[-2], 0.2)
        for p, q in zip(pts, pts[1:]):
            d.tube("armor", p, q, 0.11, 4)
    radius_at = lambda yy: next(r0 + (r1 - r0) * (yy - y0) / (y1 - y0)
                                for (y0, r0), (y1, r1) in zip(prof, prof[1:]) if y1 <= yy <= y0)
    for yy in (1.5, -6.0):
        rr = radius_at(yy) - t / 2 - 0.1
        pts = [point(a0 + (a1 - a0) * (0.02 + 0.96 * k / 12), yy, rr) for k in range(13)]
        for p, q in zip(pts, pts[1:]):
            d.tube("armor", p, q, 0.1, 4)
    mid = (a0 + a1) / 2
    spine = [point(mid, y, r + t / 2 + 0.02) for y, r in shell_profile(shape["profile_m"], 6)]
    spine[0], spine[-1] = lerp(spine[0], spine[1], 0.3), lerp(spine[-1], spine[-2], 0.3)
    for p, q in zip(spine, spine[1:]):
        d.tube("armor", p, q, 0.13, 4)
    band = [point(a0 + (a1 - a0) * k / 16, 2.4, radius_at(2.4) + t / 2 + 0.01) for k in range(17)]
    for p, q in zip(band, band[1:]):
        d.tube("trim", p, q, 0.06, 4)
    for node in part.get("attach_nodes", []):
        p, dn = node["position_m"], norm(node["direction"])
        d.block("armor", [0.8, 0.08, 0.7], add3(p, mul3(dn, -0.02)), 0.03, basis(dn, (1, 0, 0)))
        d.bolts(add3(p, mul3(dn, 0.06)), dn, 0.55, 10, size=0.04)
    m.merge(d)
    return m


MODELS = {
    "frame.torso": torso,
    "frame.head": head,
    "frame.waist": waist,
    "frame.shoulder": shoulder,
    "frame.limb": limb,
    "frame.leg": lambda h, part: limb(h, part, knee=True),
    "frame.foot": foot,
    "frame.backpack": lambda h, part: backpack(h),
    "frame.binder": lambda h, part: binder(h),
    "frame.drone_pod": lambda h, part: drone_pod(h),
    "frame.gyro": lambda h, part: gyro(h),
    "frame.hinge": joint,
    "bastion.arm_rotor": rotor,
    "bastion.arm_hinge": joint,
    "bastion.torso": torso,
    "bastion.head": head,
    "bastion.waist": waist,
    "bastion.shoulder": shoulder,
    "bastion.limb": limb,
    "bastion.leg": lambda h, part: limb(h, part, knee=True),
    "bastion.foot": foot,
    "bastion.hand": hand,
    "bastion.link_front": beam,
    "bastion.link_back": beam,
    "bastion.forearm_front": beam,
    "bastion.forearm_back": beam,
    "bastion.elbow": joint,
    "bastion.carrier": carrier,
    "bastion.drop_front": beam,
    "bastion.drop_back": beam,
    "bastion.bend": joint,
    "bastion.fold_knee": fold_knee,
    "bastion.leg_1": spider,
    "bastion.leg_2": spider,
    "bastion.leg_3": spider,
    "bastion.leg_4": spider,
    "bastion.leg_5": spider,
    "bastion.leg_6": spider,
    "bastion.grapple": grapple,
    "bastion.lift_engine": engine,
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
        built = builder(size_of(part), part)
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
