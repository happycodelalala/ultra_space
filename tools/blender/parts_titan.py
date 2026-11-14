"""The Bastion's body as a titan (about 100 m tall), built in its design units (1/BASTION_SCALE).

A machine this size keeps big armour forms (large sculpted plates; the fine plate seams,
fasteners and wear are drawn on them at real sizes by the hull detail shader), its structure
shows as girders, banks of parallel rams and pipe runs, and the crew's scale is everywhere:
catwalks with railings, caged ladders, door-sized hatches, rows of lit windows, floodlight
banks, masts with red warning beacons. Sizes written `R(x)` are real metres.
"""

import math

import bmesh
from mathutils import Matrix

from mech import V, X, Y, Z, along, frame
import parts_body
from parts import BASTION_SCALE, builder

S = BASTION_SCALE


def R(metres):
    """Real metres in design units."""
    return metres / S


class Batch:
    """Many small boxes in one mesh per material (railings, rungs, panels): one object each
    instead of hundreds."""

    def __init__(self, m):
        self.m = m
        self.meshes = {}

    def box(self, size, place, mat):
        bm = self.meshes.setdefault(mat, bmesh.new())
        scale = Matrix.Diagonal((size[0], size[1], size[2], 1.0))
        bmesh.ops.create_cube(bm, size=1.0, matrix=place @ scale)

    def bar(self, a, b, t, mat, hint=None):
        a, b = V(a), V(b)
        self.box((t, (b - a).length, t), along(a, b, hint) @ Matrix.Translation((0, (b - a).length / 2, 0)), mat)

    def done(self, bevel=0.0):
        for mat, bm in self.meshes.items():
            self.m._object(bm, mat, frame(), bevel, 1, angle=40.0)
        self.meshes = {}


# --- Crew-scale fittings -----------------------------------------------------------------


def railing(b, a, c, up, out=None, height=1.1):
    """A railing from `a` to `c`: posts every 2.5 m, top and knee rails."""
    a, c, up = V(a), V(c), V(up).normalized()
    n = max(1, round((c - a).length / R(2.5)))
    t = R(0.08)
    for k in range(n + 1):
        p = a.lerp(c, k / n)
        b.bar(p, p + up * R(height), t, "metal")
    for h in (height, height * 0.5):
        b.bar(a + up * R(h), c + up * R(h), t, "metal")


def catwalk(b, a, c, up, out, width=1.4):
    """A walkway along `a`–`c` against a wall (`out` away from it): grating deck on brackets, a
    railing along the outer edge, its edge in hazard paint."""
    a, c, up, out = V(a), V(c), V(up).normalized(), V(out).normalized()
    d = (c - a).normalized()
    length = (c - a).length
    w = R(width)
    mid = (a + c) / 2 + out * w / 2
    b.box((length, R(0.15), w), frame(mid, u=d, v=up), "dark")
    b.box((length, R(0.17), R(0.15)), frame(mid + out * (w / 2 - R(0.075)), u=d, v=up), "trim")
    n = max(1, round(length / R(4)))
    for k in range(n + 1):
        p = a.lerp(c, k / n)
        b.bar(p - up * R(0.1), p + out * w * 0.9 - up * R(0.08), R(0.12), "dark")
        b.bar(p - up * R(1.0), p + out * w * 0.6 - up * R(0.1), R(0.1), "dark")
    railing(b, a + out * (w - R(0.05)), c + out * (w - R(0.05)), up)


def ladder(b, top, bottom, normal, width=0.5, cage=True):
    """A ladder up a surface: stiles on stand-offs, rungs every 0.3 m, a safety cage."""
    top, bottom, n = V(top), V(bottom), V(normal).normalized()
    d = (top - bottom)
    length = d.length
    up = d.normalized()
    side = up.cross(n).normalized()
    off = n * R(0.25)
    w = R(width) / 2
    t = R(0.07)
    for s in (-1, 1):
        b.bar(bottom + off + side * s * w, top + off + side * s * w, t, "metal")
    count = int(length / R(0.3))
    for k in range(count + 1):
        p = bottom.lerp(top, k / max(count, 1)) + off
        b.bar(p - side * w, p + side * w, t * 0.8, "metal")
    for k in range(int(length / R(3)) + 1):
        p = bottom.lerp(top, k / max(1, int(length / R(3))))
        b.bar(p, p + off, t, "dark")
    if cage:
        r = R(0.4)
        for k in range(1, int(length / R(1.5))):
            p = bottom.lerp(top, k * R(1.5) / length) + off
            pts = [p + side * w + n * 0, p + side * (w + r * 0.3) + n * r, p + n * (r * 1.8),
                   p - side * (w + r * 0.3) + n * r, p - side * w]
            for q0, q1 in zip(pts, pts[1:]):
                b.bar(q0, q1, t * 0.7, "metal")


def hatch(b, centre, normal, up, lamp=True):
    """A crew door, 1.2 × 2.2 m: a frame, the door, a lamp above it."""
    c, n, u = V(centre), V(normal).normalized(), V(up)
    u = (u - n * u.dot(n)).normalized()
    side = u.cross(n)
    place = frame(c, u=side, v=u)
    b.box((R(1.6), R(2.6), R(0.2)), place, "dark")
    b.box((R(1.2), R(2.2), R(0.3)), place, "armor2")
    b.box((R(0.15), R(0.4), R(0.4)), frame(c + side * R(0.4) + n * R(0.15), u=side, v=u), "metal")
    if lamp:
        b.box((R(0.5), R(0.18), R(0.35)), frame(c + u * R(1.5) + n * R(0.15), u=side, v=u), "light")


def windows(b, a, c, normal, up, count, w=1.4, h=0.9):
    """A row of lit windows (a crew deck behind them) from `a` to `c`."""
    a, c, n, u = V(a), V(c), V(normal).normalized(), V(up)
    u = (u - n * u.dot(n)).normalized()
    side = u.cross(n)
    for k in range(count):
        p = a.lerp(c, (k + 0.5) / count)
        b.box((R(w), R(h), R(0.2)), frame(p, u=side, v=u), "light")
        b.box((R(w + 0.4), R(0.25), R(0.5)), frame(p + u * R(h / 2 + 0.15), u=side, v=u), "dark")


def beacon(b, p, size=0.6):
    """A red warning beacon."""
    p = V(p)
    b.box((R(size), R(size), R(size)), frame(p), "beacon")
    b.box((R(size * 1.4), R(0.2), R(size * 1.4)), frame(p - Y * R(size * 0.6)), "dark")


def floods(b, centre, normal, up, cols=3, rows=2):
    """A floodlight bank: lamps in a frame."""
    c, n, u = V(centre), V(normal).normalized(), V(up)
    u = (u - n * u.dot(n)).normalized()
    side = u.cross(n)
    b.box((R(cols * 1.0 + 0.4), R(rows * 0.9 + 0.4), R(0.5)), frame(c, u=side, v=u), "dark")
    for i in range(cols):
        for j in range(rows):
            p = c + side * R((i - (cols - 1) / 2) * 1.0) + u * R((j - (rows - 1) / 2) * 0.9) + n * R(0.25)
            b.box((R(0.75), R(0.65), R(0.12)), frame(p, u=side, v=u), "light")


def mast(b, base, height, beacon_top=True):
    """A lattice mast with antenna spars, a beacon on top."""
    base = V(base)
    w = R(0.9)
    corners = [V((sx * w / 2, 0, sz * w / 2)) for sx, sz in ((-1, -1), (1, -1), (1, 1), (-1, 1))]
    top = base + Y * height
    for c in corners:
        b.bar(base + c, top + c * 0.4, R(0.14), "dark")
    n = max(2, int(height / R(2.5)))
    for k in range(n):
        y0, y1 = height * k / n, height * (k + 1) / n
        f0, f1 = 1 - 0.6 * k / n, 1 - 0.6 * (k + 1) / n
        for c0, c1 in zip(corners, corners[1:] + corners[:1]):
            b.bar(base + Y * y0 + c0 * f0, base + Y * y1 + c1 * f1, R(0.07), "metal")
    for k, (y, l) in enumerate(((0.55, 3.0), (0.8, 2.0))):
        p = base + Y * height * y
        b.bar(p - X * R(l), p + X * R(l), R(0.1), "metal")
    if beacon_top:
        beacon(b, top + Y * R(0.4))


# --- Plating -----------------------------------------------------------------------------


def ring_bearing(m, b, centre, axis, r, width=None, mat="dark"):
    """A slewing-ring joint seen from outside: a heavy ring, its drive motors round it, a
    cover in the middle."""
    c, ax = V(centre), V(axis).normalized()
    w = width or r * 0.35
    m.cyl(c - ax * w / 2, c + ax * w / 2, r, mat, 32, bevel=R(0.3), segments=2)
    m.cyl(c + ax * w / 2, c + ax * (w / 2 + R(0.6)), r * 0.82, "metal", 32, bevel=R(0.15), segments=1)
    m.cyl(c + ax * (w / 2 + R(0.6)), c + ax * (w / 2 + R(1.2)), r * 0.45, "armor2", 24, bevel=R(0.2), segments=2)
    u = ax.orthogonal().normalized()
    wv = ax.cross(u)
    for k in range(8):
        t = 2 * math.pi * (k + 0.5) / 8
        d = u * math.cos(t) + wv * math.sin(t)
        p = c + ax * (w / 2 + R(0.3)) + d * r * 0.64
        b.box((R(1.4), R(1.4), R(1.8)), frame(p, u=d, w=ax), "dark")


def ram_bank(m, a, c, r, count, spread, hint):
    """A bank of `count` parallel hydraulic rams from `a` to `c`, `spread` apart across
    `hint`."""
    a, c, h = V(a), V(c), V(hint).normalized()
    for k in range(count):
        o = h * spread * (k - (count - 1) / 2)
        m.ram(a + o, c + o, r, extension=0.5, hint=h, booted=False)


def pipes(b, pts, r, count, spread, across):
    """A run of `count` parallel pipes along `pts`, `spread` apart across `across`, with
    brackets."""
    across = V(across).normalized()
    pts = [V(p) for p in pts]
    for k in range(count):
        o = across * spread * (k - (count - 1) / 2)
        for p0, p1 in zip(pts, pts[1:]):
            b.bar(p0 + o, p1 + o, r * 2, "metal")
    for p0, p1 in zip(pts, pts[1:]):
        n = max(1, int((p1 - p0).length / R(4)))
        for k in range(n + 1):
            p = p0.lerp(p1, k / n)
            b.bar(p - across * spread * count / 2, p + across * spread * count / 2, r * 2.6, "dark")


# --- Body --------------------------------------------------------------------------------


@builder("bastion.torso")
def torso(m, h, part):
    """The torso: the big sculpted armour of the industrial frame (chest plates either side of
    the reactor, abdomen bands, shoulder slabs, back blades round the heat sink), with what a
    100 m machine carries for its crew and its size: a deck with railings on each shoulder
    slab, hatches, a lattice mast, floodlight banks under the slabs, a catwalk gallery round
    the waist, caged ladders up the flanks, two exhaust stacks on the back, red beacons."""
    hx, hy, hz = h
    parts_body.torso(m, h, part)
    b = Batch(m)
    top = 1.0 * hy
    for s_ in (-1, 1):
        # Shoulder slab deck: railing round its outer edges, a hatch, floodlights below.
        x0, x1 = s_ * 0.3 * hx, s_ * 0.86 * hx
        z0, z1 = -0.5 * hz, 0.5 * hz
        for a, c in ((V((x0, top, z0)), V((x1, top, z0))), (V((x1, top, z0)), V((x1, top, z1))),
                     (V((x1, top, z1)), V((x0, top, z1)))):
            railing(b, a, c, Y)
        hatch(b, V((s_ * 0.55 * hx, top + R(0.2), 0.2 * hz)), Y, Z, lamp=False)
        floods(b, V((s_ * 0.6 * hx, 0.88 * hy, -0.62 * hz)), V((0, -0.3, -1)), Y, 3, 2)
        beacon(b, V((s_ * 0.85 * hx, top + R(0.4), -0.48 * hz)))
    mast(b, V((0.62 * hx, top, 0.35 * hz)), R(16))
    # Waist gallery round the abdomen.
    yg = -0.28 * hy
    ring = [V((sx * 0.78 * hx, yg, sz * 0.8 * hz)) for sx, sz in ((-1, -1), (1, -1), (1, 1), (-1, 1))]
    outs = [V((0, 0, -1)), V((1, 0, 0)), V((0, 0, 1)), V((-1, 0, 0))]
    for (a, c), o in zip(zip(ring, ring[1:] + ring[:1]), outs):
        catwalk(b, a + o * R(0.4), c + o * R(0.4), Y, o)
    for s_ in (-1, 1):
        x = s_ * 0.9 * hx
        ladder(b, V((x, 0.25 * hy, 0.55 * hz)), V((x, yg, 0.55 * hz)), V((s_, 0, 0)))
        hatch(b, V((s_ * 0.88 * hx, 0.0, -0.1 * hz)), V((s_, 0, 0)), Y)
    # Exhaust stacks on the back, a ladder up one.
    for s_ in (-1, 1):
        e = V((s_ * 0.62 * hx, 0.1 * hy, 0.98 * hz))
        m.cyl(e, e + Y * R(16), R(1.6), "dark", 24, bevel=R(0.2))
        for y in (R(6), R(11)):
            m.cyl(e + Y * (y - R(0.3)), e + Y * (y + R(0.3)), R(1.8), "metal", 24)
        m.cyl(e + Y * R(16), e + Y * R(17), R(1.9), "metal", 24, bevel=R(0.2))
        m.cyl(e + Y * R(16.5), e + Y * R(17.1), R(1.45), "rubber", 24)
        beacon(b, e + Y * R(17.6) + X * s_ * R(1.6))
    ladder(b, V((0.62 * hx, 0.1 * hy, 0.98 * hz)) + Y * R(15.5) + Z * R(1.7),
           V((0.62 * hx, 0.1 * hy, 0.98 * hz)) + Y * R(2) + Z * R(1.7), Z)
    b.done()


@builder("bastion.head")
def head(m, h, part):
    """The head, a sensor citadel: an armoured house with the bridge's lit windows in a band
    round its front, a radar dome and a sensor ball either side, a lattice mast with
    beacons, a floodlight bank under the brow, a hatch onto the torso's deck behind."""
    hx, hy, hz = h
    b = Batch(m)
    k = 1.15

    def P(x, y, z):
        return V((x * hx * k, (-1 + (y + 1) * k) * hy, z * hz * k))

    ring_bearing(m, b, P(0, -1.0, 0), Y, hx * 0.62)
    m.hull([P(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
        (0.8, -0.75, 0.75), (0.85, -0.75, -0.6), (0.6, -0.75, -0.9), (0.75, 0.5, 0.7), (0.7, 0.55, -0.5),
        (0.45, 0.4, -0.95), (0.4, 0.75, 0.3))], "dark", bevel=R(0.4), segments=2)
    # Armoured brow and cheeks over the house, the window band between them.
    m.hull([P(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
        (0.85, 0.1, 0.8), (0.9, 0.1, -0.62), (0.62, 0.08, -1.0), (0.78, 0.55, 0.72), (0.72, 0.62, -0.52),
        (0.46, 0.45, -0.98), (0.42, 0.8, 0.3))], "armor", bevel=R(0.5), segments=2)
    m.hull([P(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
        (0.88, -0.25, 0.75), (0.9, -0.25, -0.6), (0.6, -0.25, -0.95), (0.8, -0.8, 0.7), (0.82, -0.8, -0.55),
        (0.5, -0.78, -0.85))], "armor2", bevel=R(0.5), segments=2)
    for sx in (-1, 1):
        windows(b, P(sx * 0.12, -0.08, -0.99), P(sx * 0.6, -0.08, -0.9), V((sx * 0.25, 0, -1)), Y, 4)
        windows(b, P(sx * 0.85, -0.08, -0.5), P(sx * 0.88, -0.08, 0.5), V((sx, 0, 0)), Y, 5)
        m.cyl(P(sx * 0.9, -0.1, 0.1), P(sx * 1.02, -0.1, 0.1), hy * 0.42, "armor2", 24, bevel=R(0.3))
        m.lathe([(0, 0), (hy * 0.32, 0), (hy * 0.3, hy * 0.18), (0, hy * 0.3)],
                along(P(sx * 1.02, -0.1, 0.1), P(sx * 1.2, -0.1, 0.1)), "glass", 24)
    floods(b, P(0, 0.05, -1.02), V((0, -0.2, -1)), Y, 4, 1)
    m.lathe([(0, 0), (hx * 0.32, 0), (hx * 0.3, hx * 0.16), (hx * 0.2, hx * 0.28), (0, hx * 0.32)],
            along(P(-0.3, 0.75, 0.25), P(-0.3, 1.0, 0.25)), "armor2", 32)
    mast(b, P(0.35, 0.72, 0.3), R(12))
    hatch(b, P(0, -0.4, 0.82), Z, Y)
    railing(b, P(-0.7, 0.78, 0.6), P(0.7, 0.78, 0.6), Y)
    b.done()


def titan_limb(m, h, kind):
    """An arm or leg segment as a titan's: a box girder of dark structure between slewing-ring
    joints, plated front and back in panels, a bank of three rams down each side (they work
    the joint below), pipe runs in brackets, a ladder and a maintenance hatch on the back."""
    hx, hy, hz = h
    b = Batch(m)
    yb = hy * 0.84
    wt, wb = {"upper_arm": (1.0, 0.95), "forearm": (1.05, 1.1), "thigh": (1.04, 1.0),
              "shin": (1.0, 1.12)}[kind]

    def F(x, y, z):
        w = wb + (wt - wb) * (y + 1) / 2
        return V((x * hx * w, y * hy, z * hz))

    # Girder: four corner chords, cross bracing between them on the sides.
    cx, cz = hx * 0.42, hz * 0.45
    for sx in (-1, 1):
        for sz in (-1, 1):
            b.box((R(1.6), 2 * yb, R(1.6)), frame(V((sx * cx, 0, sz * cz))), "dark")
    n = 5
    for k in range(n):
        y0, y1 = -yb + 2 * yb * k / n, -yb + 2 * yb * (k + 1) / n
        for sx in (-1, 1):
            b.bar(V((sx * cx, y0, -cz)), V((sx * cx, y1, cz)), R(0.7), "dark")
            b.bar(V((sx * cx, y0, cz)), V((sx * cx, y1, -cz)), R(0.7), "dark")
    m.box((2 * cx, 2 * yb * 0.92, 2 * cz * 0.7), at=(0, 0, 0), mat="dark", bevel=R(0.2), segments=1)
    # Joints.
    for s in (-1, 1):
        y = s * yb
        m.cyl(V((-hx * 0.5, y, 0)), V((hx * 0.5, y, 0)), min(hy, hz) * 0.42, "metal", 32, bevel=R(0.3))
        for sx in (-1, 1):
            ring_bearing(m, b, V((sx * hx * 0.5, y, 0)), V((sx, 0, 0)), min(hy, hz) * 0.4, width=R(2.0))
    # Big armour plates front and back, curved across, the lower lapped over the upper.
    top_y = 0.3 if kind == "shin" else 0.62

    def curved(y0, y1, z_in, z_out, w0, w1, front=-1):
        pts = []
        for y, w in ((y0, w0), (y1, w1)):
            for x, f in ((0.0, 1.0), (0.4, 0.96), (0.68, 0.82), (0.86, 0.55)):
                for sx in ((1,) if x == 0 else (-1, 1)):
                    pts.append(F(sx * x * w / 0.86, y, front * (z_in + (z_out - z_in) * f)))
            for sx in (-1, 1):
                pts.append(F(sx * w, y, front * z_in))
        return pts

    plate = dict(bevel=R(0.8), segments=3, angle=35.0)
    m.hull(curved(top_y, -0.1, 0.42, 1.0, 0.82, 0.76), "armor", **plate)
    m.hull(curved(0.0, -0.6, 0.45, 1.06, 0.78, 0.7), "armor2", **plate)
    m.hull(curved(0.55, -0.5, 0.42, 0.93, 0.7, 0.64, front=1), "armor", **plate)
    # Rams down the sides, pipes beside them.
    for sx in (-1, 1):
        x = sx * hx * 0.72
        ram_bank(m, V((x, hy * 0.62, -hz * 0.35)), V((x, -hy * 0.72, -hz * 0.3)), R(0.6), 3, R(1.9), Z)
        pipes(b, [V((sx * hx * 0.62, hy * 0.7, hz * 0.55)), V((sx * hx * 0.62, -hy * 0.7, hz * 0.55))], R(0.25), 3,
              R(0.8), Z)
    # Ladder and hatch on the back.
    zb = F(0, 0, 0.93).z + R(0.4)
    ladder(b, V((R(0), hy * 0.5, zb)), V((R(0), -hy * 0.5, zb)), Z)
    hatch(b, V((R(2.6), -hy * 0.55, zb - R(0.2))), Z, Y)
    if kind == "shin":
        # Knee cap: a big plated wedge over the joint above.
        m.hull([F(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
            (0.8, 0.35, -0.9), (0.6, 1.02, -0.85), (0.5, 0.85, -1.1), (0.5, 0.4, -1.08), (0.75, 0.4, -0.5),
            (0.6, 0.98, -0.5))], "armor", bevel=R(0.6), segments=2)
        b.box((F(0.6, 0.3, 0).x * 2, R(0.8), R(0.3)), frame(F(0, 0.36, -1.06), u=X, v=Y), "trim")
        beacon(b, F(0.62, 0.95, -0.9))
    if kind == "forearm":
        m.hull([F(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
            (1.0, -0.2, -0.75), (1.0, -0.2, 0.7), (1.0, -0.92, -0.75), (1.0, -0.92, 0.7))], "armor",
            bevel=R(0.6), segments=2)
        floods(b, F(0, -0.6, -0.78), -Z, Y, 3, 1)
    if kind == "thigh":
        m.hull([F(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
            (1.02, 0.2, -0.75), (1.02, 0.2, 0.6), (1.02, 0.78, -0.75), (1.02, 0.78, 0.6), (0.85, 0.98, -0.6),
            (0.85, 0.98, 0.45))], "armor", bevel=R(0.6), segments=2)
    b.done()


@builder("bastion.limb")
def upper_arm(m, h, part):
    """An upper arm (titan): girder, plating, ram banks, pipes, ladder."""
    titan_limb(m, h, "upper_arm")


@builder("bastion.forearm")
def forearm(m, h, part):
    """A forearm (titan): as an upper arm, with an armoured gauntlet and floodlights."""
    titan_limb(m, h, "forearm")


@builder("bastion.leg")
def thigh(m, h, part):
    """A thigh (titan): as an upper arm, the hip armour flared over the joint."""
    titan_limb(m, h, "thigh")


@builder("bastion.shin")
def shin(m, h, part):
    """A shin (titan): as an upper arm, a plated knee cap with a beacon."""
    titan_limb(m, h, "shin")
