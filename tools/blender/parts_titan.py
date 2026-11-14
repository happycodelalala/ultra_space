"""The Bastion's body as a titan (about 100 m tall) for interplanetary exploration and
combat, built in its design units (1/BASTION_SCALE); its functional layout is
design/bastion.md.

Big armour forms (the fine plate seams, fasteners and wear are drawn on them at real sizes by
the hull detail shader), machinery out in the open where it moves, and only what a space
machine needs outside: airlocks and a docking port, EVA handrails (yellow, on the routes
suited crew take to the work sites) with work lights, RCS thruster quads at the extremities,
navigation lights, sensors, radiators, a comms dish. Sizes written `R(x)` are real metres.
"""

import math

import bmesh
from mathutils import Matrix

from mech import V, X, Y, Z, along, frame
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


def handrail(b, pts, normal, standoff=0.2):
    """An EVA handrail along `pts` on a surface (`normal` out of it): a yellow bar on
    stand-offs every 1.2 m, a tether ring every 4 m."""
    pts = [V(p) for p in pts]
    n = V(normal).normalized()
    off = n * R(standoff)
    for p0, p1 in zip(pts, pts[1:]):
        b.bar(p0 + off, p1 + off, R(0.08), "trim")
        length = (p1 - p0).length
        k = max(1, round(length / R(1.2)))
        for i in range(k + 1):
            p = p0.lerp(p1, i / k)
            b.bar(p, p + off, R(0.06), "dark")
        for i in range(1, max(1, round(length / R(4)))):
            p = p0.lerp(p1, i / max(1, round(length / R(4)))) + off * 0.5
            b.box((R(0.2), R(0.2), R(0.2)), frame(p), "metal")


def airlock(b, centre, normal, up):
    """An airlock door, 2.4 × 3 m: an armoured frame, the door, hazard paint round it (keep
    clear), a work light over it, handrails either side."""
    c, n, u = V(centre), V(normal).normalized(), V(up)
    u = (u - n * u.dot(n)).normalized()
    side = u.cross(n)
    place = frame(c, u=side, v=u)
    b.box((R(4.0), R(4.6), R(0.6)), place, "armor2")
    b.box((R(3.2), R(3.8), R(0.75)), place, "trim")
    b.box((R(2.6), R(3.2), R(0.85)), place, "dark")
    b.box((R(2.4), R(3.0), R(0.95)), place, "armor2")
    b.box((R(1.4), R(0.3), R(0.6)), frame(c + u * R(2.6) + n * R(0.4), u=side, v=u), "light")
    for sx in (-1, 1):
        handrail(b, [c + side * sx * R(2.3) - u * R(1.4), c + side * sx * R(2.3) + u * R(1.4)], n)


def docking_port(m, b, centre, normal):
    """A docking port: an androgynous collar on a short tunnel, hazard ring round it, six
    docking lights round the rim."""
    c, n = V(centre), V(normal).normalized()
    m.cyl(c - n * R(0.5), c + n * R(1.5), R(3.2), "armor2", 32, bevel=R(0.2))
    m.cyl(c + n * R(1.5), c + n * R(2.2), R(2.6), "metal", 32, bevel=R(0.1))
    m.cyl(c + n * R(2.2), c + n * R(2.35), R(1.9), "dark", 32)
    m.cyl(c - n * R(0.6), c + n * R(0.6), R(4.2), "trim", 32)
    u = n.orthogonal().normalized()
    w = n.cross(u)
    for k in range(6):
        t = 2 * math.pi * k / 6
        d = u * math.cos(t) + w * math.sin(t)
        b.box((R(0.5), R(0.5), R(0.4)), frame(c + d * R(3.6) + n * R(1.6), u=d, w=n), "light")


def rcs_quad(m, b, centre, normal, up):
    """An RCS thruster quad: a block with four nozzles, two along `up` and two across it (an
    extremity's pushes for attitude control)."""
    c, n, u = V(centre), V(normal).normalized(), V(up)
    u = (u - n * u.dot(n)).normalized()
    side = u.cross(n)
    b.box((R(2.6), R(2.6), R(1.6)), frame(c, u=side, v=u), "dark")
    for d in (u, -u, side, -side):
        base = c + d * R(1.3) + n * R(0.2)
        m.cyl(base, base + d * R(1.2), R(0.3), "metal", 12, r2=R(0.6))


def nav_light(b, p, normal, mat):
    """A navigation light (`beacon` red left, `nav_green` right, `strobe` white) in a guard."""
    p, n = V(p), V(normal).normalized()
    o = n.orthogonal()
    b.box((R(0.8), R(0.8), R(0.6)), frame(p + n * R(0.3), w=n, v=o), mat)
    b.box((R(1.2), R(1.2), R(0.3)), frame(p, w=n, v=o), "dark")


def dish(m, centre, normal, r):
    """A high-gain antenna stowed flat: the dish facing out, its feed on struts."""
    c, n = V(centre), V(normal).normalized()
    m.lathe([(0, 0), (r * 0.5, r * 0.06), (r, r * 0.24), (r * 1.02, r * 0.3), (r * 0.98, r * 0.31),
             (r * 0.5, r * 0.12), (0, r * 0.06)], along(c, c + n), "armor2", 32, closed=True, angle=40.0)
    f = c + n * r * 0.55
    m.cyl(f, f + n * r * 0.12, r * 0.08, "dark", 12)
    u = n.orthogonal().normalized()
    w = n.cross(u)
    for k in range(3):
        t = 2 * math.pi * k / 3
        d = u * math.cos(t) + w * math.sin(t)
        m.cyl(c + d * r * 0.7 + n * r * 0.2, f, r * 0.015, "metal", 6)


def coolant(b, pts, r=0.45, count=2, spread=1.2):
    """Coolant lines (`count` side by side, `spread` m apart) along `pts`, clamped every 4 m."""
    pts = [V(p) for p in pts]
    for p0, p1 in zip(pts, pts[1:]):
        d = (p1 - p0).normalized()
        across = d.cross(Z if abs(d.z) < 0.9 else X).normalized()
        for k in range(count):
            o = across * R(spread) * (k - (count - 1) / 2)
            b.bar(p0 + o, p1 + o, R(2 * r), "metal")
        n = max(1, int((p1 - p0).length / R(4)))
        for i in range(n + 1):
            p = p0.lerp(p1, i / n)
            b.box((R(spread * count + 0.6), R(0.5), R(2 * r + 0.4)), frame(p, u=across, v=d), "dark")


def searchlight(m, p, normal, r):
    """A searchlight: a lamp in a short barrel aimed along `normal`."""
    p, n = V(p), V(normal).normalized()
    m.cyl(p - n * r, p + n * r * 0.4, r * 1.2, "dark", 16, bevel=r * 0.1)
    m.cyl(p + n * r * 0.38, p + n * r * 0.45, r, "light", 16)


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
    `hint`, their rods under dust boots."""
    a, c, h = V(a), V(c), V(hint).normalized()
    for k in range(count):
        o = h * spread * (k - (count - 1) / 2)
        m.ram(a + o, c + o, r, extension=0.5, hint=h)


# --- Body --------------------------------------------------------------------------------


@builder("bastion.torso")
def torso(m, h, part):
    """The torso (design/bastion.md): the crew module behind the chest armour, the reactor
    below in the waist. A dark frame under big sculpted plates: chest plates either side of a
    thick central shield over the crew module; abdomen bands with two booted flex rams, hazard
    stripes at the pinch point; shoulder slabs with RCS quads at their outer corners and the
    navigation lights (red left, green right); servo drums for the shoulders (EVA work sites:
    handrails lead to them, work lights over them); flank plates, the surface airlock in the
    left one with its stowed hoist; terrain lights under the chest aimed down and ahead. On the
    back the radiator fin bank between armoured blades, and on the lower back plate the
    docking port and the stowed comms dish; a white stern light."""
    hx, hy, hz = h

    def P(x, y, z):
        return V((x * hx, y * hy, z * hz))

    big = dict(bevel=R(0.9), segments=3, angle=35.0)
    b = Batch(m)
    m.hull([P(sx * wx, y, sz * wz) for sx in (-1, 1) for sz in (-1, 1)
            for y, wx, wz in ((0.9, 0.8, 0.55), (0.1, 0.74, 0.7), (-0.94, 0.45, 0.6))], "dark", bevel=R(0.3))
    # Chest plates and the central shield over the crew module.
    for s_ in (-1, 1):
        m.hull([P(s_ * x, y, z) for x, y, z in (
            (0.25, 0.94, -0.62), (0.92, 0.94, -0.5), (0.98, 0.5, -0.7), (0.85, 0.12, -0.72), (0.28, 0.08, -0.86),
            (0.55, 0.55, -1.0), (0.28, 0.94, -0.2), (0.92, 0.94, -0.2), (0.95, 0.12, -0.3), (0.28, 0.08, -0.3))],
            "armor", **big)
    m.hull([P(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
        (0.3, 0.9, -0.75), (0.3, 0.1, -0.88), (0.18, 0.85, -0.98), (0.16, 0.16, -1.04), (0.3, 0.9, -0.4),
        (0.3, 0.1, -0.4))] + [P(0, 0.5, -1.1)], "armor2", **big)
    m.hazard(P(-0.5, 0.06, -0.84), P(0.5, 0.06, -0.84), V((0, -0.2, -1)).normalized(), R(1.0), R(2.0), R(0.1))
    # Terrain lights under the chest plates, aimed down and ahead.
    for s_ in (-1, 1):
        floods(b, P(s_ * 0.55, 0.1, -0.78), V((0, -1, -0.7)), -Z, 3, 1)
    # Abdomen bands and the two flex rams (booted against dust).
    for k in range(3):
        y0 = -0.14 - k * 0.27
        w = 0.42 - 0.05 * k
        m.box((2 * hx * w, hy * 0.2, hz * 0.5), at=P(0, y0 - 0.06, -0.55 + 0.04 * k),
              mat="armor" if k != 1 else "armor2", bevel=R(0.9), segments=3)
    for s_ in (-1, 1):
        m.ram(P(s_ * 0.56, 0.05, -0.62), P(s_ * 0.44, -0.86, -0.58), hx * 0.07, extension=0.35, hint=(0, 0, 1))
    # Shoulder slabs, RCS quads at their outer corners, navigation lights, servos.
    for s_ in (-1, 1):
        m.hull([P(s_ * x, y, z) for x, y, z in ((0.24, 0.9, -0.62), (0.9, 0.9, -0.55), (0.24, 0.9, 0.62),
                                                 (0.9, 0.9, 0.6), (0.3, 1.04, -0.5), (0.7, 1.0, -0.45),
                                                 (0.3, 1.04, 0.5), (0.7, 1.0, 0.48))], "armor2", **big)
        m.servo(P(s_ * 0.8, 0.57, 0), P(s_ * 1.0, 0.57, 0), hy * 0.32, "dark", 12)
        for z in (-0.5, 0.5):
            rcs_quad(m, b, P(s_ * 0.86, 0.97, z), V((s_ * 0.4, 1, 0)), V((0, 0, 1)))
        nav_light(b, P(s_ * 0.9, 0.92, 0.0), V((s_, 0.3, 0)), "beacon" if s_ < 0 else "nav_green")
        # Work lights on the slab edge over the servo (an EVA work site).
        floods(b, P(s_ * 0.86, 0.86, -0.2), V((s_ * 0.6, -1, 0)), Z, 2, 1)
    for s_ in (-1, 1):
        m.hull([P(s_ * x, y, z) for x, y, z in ((0.8, 0.3, -0.5), (0.8, 0.3, 0.5), (0.66, -0.6, -0.5),
                                                 (0.66, -0.6, 0.5), (0.9, 0.25, -0.4), (0.9, 0.25, 0.4),
                                                 (0.74, -0.55, -0.4), (0.74, -0.55, 0.4))], "armor", **big)
    # Surface airlock in the left flank plate, its hoist stowed under it; handrails up to the
    # left shoulder servo.
    al = P(-0.84, -0.12, 0.0)
    an = V((-1, -0.16, 0)).normalized()
    airlock(b, al, an, Y)
    hoist = al + an * R(0.8) - Y * R(4.4)
    b.box((R(4.2), R(1.2), R(2.8)), frame(hoist, u=Z, v=Y), "dark")
    b.box((R(3.6), R(0.3), R(3.2)), frame(hoist - Y * R(0.75), u=Z, v=Y), "armor2")
    handrail(b, [al + Y * R(2.5) + Z * R(2.3), P(-0.88, 0.32, R(2.3) / hz), P(-0.88, 0.4, -0.2)], V((-1, -0.1, 0)))
    # Back: blades, the radiator fin bank, the lower back plate with the docking port and dish.
    for s_ in (-1, 1):
        m.hull([P(s_ * x, y, z) for x, y, z in ((0.3, 0.9, 0.62), (0.92, 0.9, 0.52), (0.95, 0.15, 0.62),
                                                 (0.3, 0.05, 0.7), (0.6, 0.5, 0.86), (0.3, 0.9, 0.3),
                                                 (0.92, 0.9, 0.2), (0.95, 0.15, 0.3), (0.3, 0.05, 0.3))],
               "armor", **big)
    sink = P(0, 0.47, 0.74)
    m.box((hx * 0.56, hy * 0.8, hz * 0.14), at=sink, mat="dark", bevel=R(0.3), segments=2)
    for k in range(15):
        x = (-0.5 + k / 14) * hx * 0.5
        b.box((R(0.3), hy * 0.74, hz * 0.34), frame(sink + V((x, 0, hz * 0.14))), "metal")
    lb = P(0, -0.45, 0.66)
    m.box((hx * 1.05, hy * 0.75, hz * 0.3), at=lb, mat="armor2", bevel=R(0.9), segments=3)
    port = lb + V((-0.3 * hx, 0, hz * 0.15))
    docking_port(m, b, port, Z)
    dish(m, lb + V((0.3 * hx, 0, hz * 0.15)), Z, R(4.2))
    nav_light(b, P(0, 0.9, 0.62), V((0, 0.4, 1)), "strobe")
    # Reactor coolant up from the waist (it continues there) beside the lower back plate into
    # the radiator.
    for s_ in (-1, 1):
        x = s_ * 0.55 * hx
        coolant(b, [V((x, -hy * 0.95, R(7.2))), V((x, 0.08 * hy, R(7.2))), V((s_ * 0.3 * hx, 0.12 * hy, 0.83 * hz))])
    # Handrails from the docking port up the back to each shoulder servo.
    for s_ in (-1, 1):
        handrail(b, [port + V((s_ * R(4.6), 0, 0)), P(s_ * 0.32, 0.05, 0.84), P(s_ * 0.32, 0.88, 0.7),
                     P(s_ * 0.6, 1.04, 0.3), P(s_ * 0.86, 0.95, 0.1)], V((0, 0.3, 1)))
    b.done()


@builder("bastion.head")
def head(m, h, part):
    """The sensor head (design/bastion.md; no crew in it): an armoured house on the neck
    bearing; across the front under the brow a recessed sensor band (optical and infrared
    lenses of several sizes) with a searchlight either side, aimed where the sensors look; a
    sensor turret either side, the radar dome and a lattice mast with whip antennas on top (its
    red beacon the highest point standing on a planet, a white strobe under it); an EVA
    handrail up the back to the turrets and mast (a work site), a work light over it."""
    hx, hy, hz = h
    b = Batch(m)
    k = 1.15

    def P(x, y, z):
        return V((x * hx * k, (-1 + (y + 1) * k) * hy, z * hz * k))

    ring_bearing(m, b, P(0, -1.0, 0), Y, hx * 0.62)
    m.hull([P(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
        (0.8, -0.75, 0.75), (0.85, -0.75, -0.6), (0.6, -0.75, -0.9), (0.75, 0.5, 0.7), (0.7, 0.55, -0.5),
        (0.45, 0.4, -0.95), (0.4, 0.75, 0.3))], "dark", bevel=R(0.4), segments=2)
    m.hull([P(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
        (0.85, 0.1, 0.8), (0.9, 0.1, -0.62), (0.62, 0.08, -1.0), (0.78, 0.55, 0.72), (0.72, 0.62, -0.52),
        (0.46, 0.45, -0.98), (0.42, 0.8, 0.3))], "armor", bevel=R(0.9), segments=3)
    m.hull([P(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
        (0.88, -0.25, 0.75), (0.9, -0.25, -0.6), (0.6, -0.25, -0.95), (0.8, -0.8, 0.7), (0.82, -0.8, -0.55),
        (0.5, -0.78, -0.85))], "armor2", bevel=R(0.9), segments=3)
    # Sensor band: lenses in housings across the front.
    n = V((0, 0, -1))
    for x, r in ((-0.42, 0.9), (-0.2, 0.55), (0.0, 1.3), (0.2, 0.55), (0.42, 0.9)):
        c = P(x, -0.08, -0.92)
        m.cyl(c + n * R(-0.4), c + n * R(0.3), R(r * 1.35), "dark", 20, bevel=R(0.1))
        m.lathe([(0, 0), (R(r), 0), (R(r * 0.9), R(r * 0.3)), (0, R(r * 0.42))], along(c + n * R(0.3), c + n * R(1.0)),
                "glass", 20)
    for sx in (-1, 1):
        searchlight(m, P(sx * 0.66, -0.1, -0.8), V((sx * 0.15, -0.1, -1)), R(0.7))
        m.cyl(P(sx * 0.9, -0.1, 0.4), P(sx * 1.02, -0.1, 0.4), hy * 0.42, "armor2", 24, bevel=R(0.3))
        m.lathe([(0, 0), (hy * 0.32, 0), (hy * 0.3, hy * 0.18), (0, hy * 0.3)],
                along(P(sx * 1.02, -0.1, 0.4), P(sx * 1.2, -0.1, 0.4)), "glass", 24)
    m.lathe([(0, 0), (hx * 0.32, 0), (hx * 0.3, hx * 0.16), (hx * 0.2, hx * 0.28), (0, hx * 0.32)],
            along(P(-0.3, 0.75, 0.25), P(-0.3, 1.0, 0.25)), "armor2", 32)
    top = P(0.35, 0.72, 0.3)
    mast(b, top, R(12))
    nav_light(b, top + Y * R(9.5) + X * R(0.8), X, "strobe")
    # EVA: a handrail up the back of the head to the turrets and the mast, a work light.
    handrail(b, [P(0, -0.95, 0.88), P(0, 0.3, 0.9), P(0.3, 0.72, 0.6)], Z)
    for sx in (-1, 1):
        handrail(b, [P(0, -0.2, 0.9), P(sx * 0.7, -0.2, 0.8), P(sx * 0.88, -0.1, 0.55)], V((sx * 0.3, 0, 1)))
    floods(b, P(0, 0.62, 0.72), V((0, -0.6, 1)), Y, 2, 1)
    b.done()


def titan_limb(m, h, kind):
    """An arm or leg segment as a titan's: a box girder of dark structure between slewing-ring
    joints, big curved armour plates front and back, a bank of three rams down each side (they
    work the joint below) fed from a manifold under the joint above, bolted service panels on
    the back (no ladders or doors: no one climbs a limb while it moves; design/bastion.md)."""
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
    # Rams down the sides (they work the joint below), fed from a manifold under the joint
    # above: one line in across the joint, a short line to each ram.
    for sx in (-1, 1):
        x = sx * hx * 0.72
        top_end, bottom_end = V((x, hy * 0.6, -hz * 0.35)), V((x, -hy * 0.72, -hz * 0.3))
        ram_bank(m, top_end, bottom_end, R(0.6), 3, R(1.9), Z)
        man = V((x, hy * 0.72, -hz * 0.35 + R(3.2)))
        b.box((R(1.2), R(1.6), R(4.6)), frame(man), "dark")
        b.bar(man + Z * R(1.8), V((sx * hx * 0.5, yb, R(0))), R(0.35), "metal")
        for k in range(3):
            port = top_end + Z * R(1.9) * (k - 1) + V((0, -R(1.4), 0))
            b.bar(man + Z * (R(1.6) * (k - 1)) - Y * R(0.6), port + X * sx * R(0.5), R(0.22), "metal")
    # Bolted service panels on the back plate (worked from gantries when parked).
    zb = F(0, 0, 0.93).z
    for y in (0.25, -0.2):
        b.box((R(3.6), R(4.2), R(0.25)), frame(V((0, y * hy, zb + R(0.1)))), "armor2")
        b.box((R(3.9), R(4.5), R(0.15)), frame(V((0, y * hy, zb))), "dark")
    if kind == "shin":
        # Knee cap: a big plated wedge over the joint above.
        m.hull([F(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
            (0.8, 0.35, -0.9), (0.6, 1.02, -0.85), (0.5, 0.85, -1.1), (0.5, 0.4, -1.08), (0.75, 0.4, -0.5),
            (0.6, 0.98, -0.5))], "armor", bevel=R(0.6), segments=2)
        # Terrain lights low on the shin, aimed down and ahead at the ground it steps on.
        for sx in (-1, 1):
            floods(b, F(sx * 0.45, -0.7, -0.98), V((0, -1, -0.8)), -Z, 2, 1)
    if kind == "forearm":
        m.hull([F(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
            (1.0, -0.2, -0.75), (1.0, -0.2, 0.7), (1.0, -0.92, -0.75), (1.0, -0.92, 0.7))], "armor",
            bevel=R(0.6), segments=2)
        # Work lights at the wrist end, aimed down along the forearm at the hand.
        floods(b, F(0, -0.88, -0.8), V((0, -1, -0.35)), -Z, 3, 1)
    if kind == "thigh":
        m.hull([F(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
            (1.02, 0.2, -0.75), (1.02, 0.2, 0.6), (1.02, 0.78, -0.75), (1.02, 0.78, 0.6), (0.85, 0.98, -0.6),
            (0.85, 0.98, 0.45))], "armor", bevel=R(0.6), segments=2)
    b.done()


@builder("bastion.limb")
def upper_arm(m, h, part):
    """An upper arm (titan): girder, big plates, ram banks and their manifolds, service panels."""
    titan_limb(m, h, "upper_arm")


@builder("bastion.forearm")
def forearm(m, h, part):
    """A forearm (titan): as an upper arm, an armoured gauntlet, work lights aimed at the hand."""
    titan_limb(m, h, "forearm")


@builder("bastion.leg")
def thigh(m, h, part):
    """A thigh (titan): as an upper arm, the hip armour flared over the joint."""
    titan_limb(m, h, "thigh")


@builder("bastion.shin")
def shin(m, h, part):
    """A shin (titan): as an upper arm, a big knee cap over the joint above, terrain lights low
    on the front."""
    titan_limb(m, h, "shin")


@builder("bastion.foot")
def foot(m, h, part):
    """A foot (titan): the talon foot (toes, heel, grip pads, ankle shroud, shock struts), and
    four retractable anchor spikes in housings at the sole's corners (driven into the ground
    in low gravity, where a step could push it off), retracted flush with the sole."""
    import parts_body
    hx, hy, hz = h
    parts_body.foot(m, h, part)
    for sx in (-1, 1):
        for sz in (-0.6, 0.62):
            c = V((sx * hx * 0.78, -hy * 0.55, sz * hz))
            m.cyl(c - Y * hy * 0.4, c + Y * hy * 0.45, R(1.3), "dark", 16, bevel=R(0.15))
            m.cyl(c + Y * hy * 0.45, c + Y * hy * 0.55, R(1.0), "metal", 16)
            m.cyl(c - Y * hy * 0.44, c - Y * hy * 0.4, R(0.7), "metal", 12, r2=R(0.25))


@builder("bastion.waist")
def waist(m, h, part):
    """The waist (design/bastion.md): the pelvis, round the reactor (armoured, nothing of it
    shows) and its shadow shield; the hip actuators, skirt armour and tassets of the frame's
    pelvis; at the back the reactor's coolant manifold, its lines rising into the torso to the
    radiator."""
    import parts_body
    hx, hy, hz = h
    parts_body.waist(m, h, part)
    b = Batch(m)
    man = V((0, -0.25 * hy, 0.9 * hz))
    b.box((R(10), R(4), R(2.5)), frame(man), "dark")
    for s_ in (-1, 1):
        x = s_ * R(7.4)
        coolant(b, [man + V((s_ * R(4), 0, 0)), V((x, -0.25 * hy, R(7.2))), V((x, hy * 1.05, R(7.2)))])
    b.done()
