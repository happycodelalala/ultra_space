"""The Bastion's body as a titan (about 100 m tall) for interstellar exploration and combat
(interstellar-capable, plausible advanced technology), built in its design units
(1/BASTION_SCALE); its functional layout is design/bastion.md.

Big armour forms (the fine plate seams, fasteners and wear are drawn on them at real sizes by
the hull detail shader), machinery out in the open where it moves, and only what a space
machine needs outside: doors, hatches and a docking port, rail channels and EVA handrails where
suited crew hold on, lamps (lens strips set into the armour), RCS thruster quads at the
extremities, navigation lights, sensors, radiators, optical comms terminals. Sizes written
`R(x)` are real metres.
"""

import math

import bmesh
from mathutils import Matrix
from mathutils.geometry import intersect_ray_tri

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

    def box(self, size, place, mat, split=1):
        """A box; `split` > 1 cuts it into that many lengths along its Y (a long bar then
        needs far fewer triangles once the export splits long edges for the baked occlusion)."""
        bm = self.meshes.setdefault(mat, bmesh.new())
        scale = Matrix.Diagonal((size[0], size[1], size[2], 1.0))
        if split <= 1:
            bmesh.ops.create_cube(bm, size=1.0, matrix=place @ scale)
            return
        for v in bm.verts:
            v.tag = True
        verts = bmesh.ops.create_cube(bm, size=1.0)["verts"]
        edges = {e for v in verts for e in v.link_edges
                 if abs((e.verts[0].co - e.verts[1].co).y) > 0.5}
        bmesh.ops.subdivide_edges(bm, edges=list(edges), cuts=split - 1, use_grid_fill=True)
        bmesh.ops.transform(bm, matrix=place @ scale, verts=[v for v in bm.verts if not v.tag])

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


def rail_channel(m, b, target, p, normal, up, length=3.0):
    """A recessed rail channel at `p` on `target`'s face (`normal` out of it): a dark slot
    0.4 m wide and 0.25 m deep, the hand and tether bar recessed in it."""
    p, n, u = V(p), V(normal).normalized(), V(up)
    u = (u - n * u.dot(n)).normalized()
    side = u.cross(n)
    m.cut_box(target, (R(0.4), R(length), R(0.5)), place=frame(p, u=side, v=u))
    k = max(1, math.ceil(length / 1.8))
    b.box((R(0.4), R(length), R(0.04)), frame(p - n * R(0.23), u=side, v=u), "dark", k)
    b.box((R(0.1), R(length - 0.04), R(0.1)), frame(p - n * R(0.12), u=side, v=u), "metal", k)


def lens_strip(m, b, target, p, normal, up, w, h, aim=None, depth=0.3, mat="light"):
    """A lamp as a lens strip (`w` × `h` m) recessed `depth` into `target` at `p` (`normal`
    out of the face), the lens turned toward `aim`."""
    p, n, u = V(p), V(normal).normalized(), V(up)
    u = (u - n * u.dot(n)).normalized()
    side = u.cross(n)
    m.cut_box(target, (R(w + 0.2), R(h + 0.2), R(2 * depth)), place=frame(p, u=side, v=u))
    a = V(aim).normalized() if aim is not None else n
    b.box((R(w), R(h), R(0.1)), frame(p - n * R(depth - 0.12), u=side, w=a), mat)


def hull_hit(points, origin, direction):
    """Where a ray from `origin` (outside) along `direction` first meets the convex hull of
    `points`."""
    bm = bmesh.new()
    for q in points:
        bm.verts.new(V(q))
    bmesh.ops.convex_hull(bm, input=bm.verts[:])
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    o, d = V(origin), V(direction).normalized()
    hits = [hit for f in bm.faces if (hit := intersect_ray_tri(*(v.co for v in f.verts), d, o, True)) is not None]
    bm.free()
    return min(hits, key=lambda q: (q - o).length)


def docking_port(m, b, centre, normal, lamp="light"):
    """A docking port recessed into the armour at `centre` (on the plate's face): the tunnel's
    armour shroud rising from a dark well, its 0.3 m lip round the collar, the androgynous
    collar ring and its face, six docking-light lenses (`lamp`) set into the collar face, and a narrow
    striped keep-clear band on the armour round it."""
    c, n = V(centre), V(normal).normalized()
    m.cyl(c - n * R(0.1), c + n * R(0.03), R(3.25), "dark", 32)
    m.cyl(c - n * R(1.1), c + n * R(1.44), R(2.9), "armor2", 32, bevel=R(0.15))
    m.cyl(c + n * R(1.3), c + n * R(1.66), R(2.6), "metal", 32, bevel=R(0.08))
    m.cyl(c + n * R(1.66), c + n * R(1.74), R(1.9), "dark", 32)
    u = n.orthogonal().normalized()
    w = n.cross(u)
    for k in range(6):
        t = 2 * math.pi * k / 6
        d = u * math.cos(t) + w * math.sin(t)
        m.cyl(c + d * R(2.25) + n * R(1.6), c + d * R(2.25) + n * R(1.675), R(0.2), lamp, 8)
    # Keep-clear band: 0.5 m wide at 3.6-4.1 m, alternate yellow and dark segments.
    count = 20
    for k in range(count):
        t = 2 * math.pi * (k + 0.5) / count
        d = u * math.cos(t) + w * math.sin(t)
        b.box((R(0.5), R(2 * math.pi * 3.85 / count * 1.02), R(0.1)),
              frame(c + d * R(3.85) + n * R(0.05), u=d, w=n), "trim" if k % 2 else "dark")


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
    """A navigation light (`beacon` red left, `nav_green` (cyan) right, `strobe` white) in a guard."""
    p, n = V(p), V(normal).normalized()
    o = n.orthogonal()
    b.box((R(0.8), R(0.8), R(0.6)), frame(p + n * R(0.3), w=n, v=o), mat)
    b.box((R(1.2), R(1.2), R(0.3)), frame(p, w=n, v=o), "dark")


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


def surface_z(points, x, y, side=-1):
    """Where a vertical line at (x, y) meets the convex hull of `points`: its front (-Z,
    `side` -1) or back (+Z, `side` 1) face. Lets fittings follow a faceted plate."""
    bm = bmesh.new()
    for p in points:
        bm.verts.new(V(p))
    bmesh.ops.convex_hull(bm, input=bm.verts[:])
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    o = V((x, y, -side * 1e3))
    hits = [hit.z for f in bm.faces
            if (hit := intersect_ray_tri(*(v.co for v in f.verts), Z * side, o, True)) is not None]
    bm.free()
    return min(hits) if side < 0 else max(hits)


def seven_seg(b, origin, right, up, text, h, mat):
    """Painted digits (stencil numerals of `h` m height) from `origin` along `right`."""
    seg = {"0": "abcdef", "1": "bc", "2": "abged", "3": "abgcd", "4": "fgbc", "5": "afgcd",
           "6": "afgedc", "7": "abc", "8": "abcdefg", "9": "abcfgd"}
    o, r, u = V(origin), V(right).normalized(), V(up).normalized()
    w, t = h * 0.55, h * 0.13
    bars = {"a": ((0, 1), True), "g": ((0, 0), True), "d": ((0, -1), True),
            "f": ((-1, 0.5), False), "b": ((1, 0.5), False), "e": ((-1, -0.5), False), "c": ((1, -0.5), False)}
    for i, ch in enumerate(text):
        c = o + r * R(i * w * 1.6)
        for s_ in seg[ch]:
            (sx, sy), across = bars[s_]
            p = c + r * R(sx * w / 2) + u * R(sy * h / 2)
            size = (R(w), R(t), R(0.06)) if across else (R(t), R(h / 2), R(0.06))
            b.box(size, frame(p, u=r, v=u), mat)


def stencil(b, centre, normal, up, lines=(1.4, 1.0)):
    """A small stencilled notice (keep clear, no step): lines of lettering at human size."""
    c, n, u = V(centre), V(normal).normalized(), V(up)
    u = (u - n * u.dot(n)).normalized()
    for k, length in enumerate(lines):
        b.box((R(length), R(0.2), R(0.06)), frame(c - u * R(0.4 * k) + n * R(0.03), u=u.cross(n), v=u), "dark")



@builder("bastion.torso")
def torso(m, h, part):
    """The torso (design/bastion.md): the pilot's capsule and a few cabins for crew behind
    the chest armour, the reactor below in the waist. All its edges are rigid: the shoulders
    and the waist bolt on. A dark frame under big sculpted plates, its main load path shown
    by steel longerons front and back (two a side, converging into the waist), tied on the
    back by a frame rib at each lower band line; chest plates either side of a thick central shield over
    the capsule. The large boarding door is a stepped plug in the shield (two seams), set
    back in a dark door frame (jambs and a lintel 0.35 m proud, the lintel trimmed to the
    jambs' width, the lintel light in the lintel aimed at the open leaf) and hinged at its foot on a hinge line across the sill (two
    leaf knuckles between the frame's): it opens down and out into a landing that the mech's
    own hand meets (a mooring socket under each jamb's foot for the hand or a platform to lock
    on); the yellow external release in a well in the right jamb, the rescue arrow over it.
    Hard lapped abdomen bands (each lower band's top under the one above); shoulder
    slabs with the hull number on the left one and wraparound navigation lenses in their
    outer front corners (red left, cyan right: the `nav_green` slot); a mounting flange ring
    for each shoulder (its face hidden under the shoulder); flank plates, each hung from a head
    under the chest plate (a seam between them) and carried by the frame on an armoured lip
    down to its foot and a tapered gusset under each edge at the foot; in each an armoured
    reactor-bay service hatch (hinged at its head, a release in a well at its foot; no
    dedicated airlock: the docking port or any hatch works as one); terrain-light lens
    strips under the chest aimed down and ahead. The big hulls (chest, shield, door leaf,
    shoulder slabs, flank plates, back blades, bands) shade with hard facets; the floods and
    work lights (terrain, lintel, docking-port work light and docking lights) are white
    (`strobe`), the orange `light` is left to the reactor and visor. On the back a flat radiator panel with fine
    ribs (the heat-pipe fins) between the armoured blades, the louvre box over it (the supply
    header inside) and the louvres' guide tracks down both sides, the armoured return header
    across its foot; on the lower back plate the docking collar recessed in its armour lip
    with lenses in its face, a keep-clear band, a work-light lens and two rail channels, and
    the bolted hatches where the coolant crosses into the waist; on the top back ledge the
    comms deck: two optical terminals in armoured wells and a conformal phased array."""
    hx, hy, hz = h

    def P(x, y, z):
        return V((x * hx, y * hy, z * hz))

    big = dict(bevel=R(0.9), segments=3, angle=35.0)
    hard = R(1) ** 2 * 6.0  # facets of 6 m² and more shade flat with hard edges (big hulls)
    b = Batch(m)
    frame_pts = [P(sx * wx, y, sz * wz) for sx in (-1, 1) for sz in (-1, 1)
                 for y, wx, wz in ((0.9, 0.8, 0.55), (0.1, 0.74, 0.7), (-0.94, 0.45, 0.6))]
    m.hull(frame_pts, "dark", bevel=R(0.3))
    # Chest plates and the central shield over the pilot's capsule.
    chest, chest_ob = {}, {}
    for s_ in (-1, 1):
        chest[s_] = [P(s_ * x, y, z) for x, y, z in (
            (0.25, 0.94, -0.62), (0.92, 0.94, -0.5), (0.98, 0.5, -0.7), (0.85, 0.12, -0.72), (0.28, 0.08, -0.86),
            (0.55, 0.55, -1.0), (0.28, 0.94, -0.2), (0.92, 0.94, -0.2), (0.95, 0.12, -0.3), (0.28, 0.08, -0.3))]
        chest_ob[s_] = m.hull(chest[s_], "armor", hard_facets=hard, **big)
    shield = [P(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
        (0.3, 0.9, -0.75), (0.3, 0.1, -0.88), (0.18, 0.85, -0.98), (0.16, 0.16, -1.04), (0.3, 0.9, -0.4),
        (0.3, 0.1, -0.4))] + [P(0, 0.5, -1.1)]
    shield_ob = m.hull(shield, "armor2", hard_facets=hard, **big)

    # Frame longerons, front and back: the load path from the chest, arms and shells down into
    # the waist. Two a side on the frame's faces (x 6 and 9 m under the chest, converging to
    # the waist flange), 1.2 m wide and 0.5 m proud; on the back a frame rib at each lower band
    # line ties them to the frame's corner (in front the bands cover the frame).
    def frame_z(y):
        return R(6.62) - (R(1.26) - y) / R(13.1) * R(0.95)

    def frame_edge(y):
        return R(9.99) - (R(1.26) - y) / R(13.1) * R(3.91)

    def longeron_x(y, x0, x1):
        return R(x0) + (R(x1) - R(x0)) * (R(1.0) - y) / R(12.7)

    def pieces(length):
        return max(1, math.ceil(length / R(1.8)))

    for sz in (-1, 1):
        for sx in (-1, 1):
            for x0, x1 in ((6.0, 4.4), (9.0, 5.4)):
                # The back's inner pair runs under the lower back plate: shown only to there.
                yt, yb = R(1.0), (R(-2.0) if sz > 0 and x0 < 7 else R(-11.5))
                t, u = [V((sx * longeron_x(y, x0, x1), y, sz * (frame_z(y) + R(0.1)))) for y in (yt, yb)]
                d = u - t
                across = (X - d.normalized() * X.dot(d.normalized())).normalized()
                b.box((R(1.2), d.length, R(0.8)), frame((t + u) / 2, u=across, v=d), "metal", pieces(d.length))

    # Boarding door in the shield, 5.2 × 7.2 m (people and small cargo), hinged at its foot
    # so it opens down and out into a landing: a stepped plug, the leaf's rim proud of a dark
    # split line and the inner plug proud of a second seam 0.25 m inside it.
    def on_shield(x, y, off, pts=shield):
        z = surface_z(pts, R(x), R(y))
        dz_x = (surface_z(pts, R(x + 0.2), R(y)) - surface_z(pts, R(x - 0.2), R(y))) / R(0.4)
        dz_y = (surface_z(pts, R(x), R(y + 0.2)) - surface_z(pts, R(x), R(y - 0.2))) / R(0.4)
        n = V((dz_x, dz_y, -1)).normalized()
        return V((R(x), R(y), z)) + n * R(off), n

    def leaf(xs, ys, off, depth, mat, bevel, hard_facets=None):
        pts = []
        for x in xs:
            for y in ys:
                p, n = on_shield(x, y, off)
                pts += [p, p - n * R(depth)]
        return m.hull(pts, mat, bevel=R(bevel), segments=1, hard_facets=hard_facets)

    dw, d0, d1 = 2.6, 2.4, 9.6
    split = leaf((-dw - 0.3, -2.2, 0, 2.2, dw + 0.3), (d0 - 0.3, 6.3, d1 + 0.3), 0.03, 0.4, "dark", 0.05)
    leaf((-dw, -2.2, 0, 2.2, dw), (d0, 6.3, d1), 0.08, 0.5, "armor2", 0.06, hard)
    i = dw - 0.25
    leaf((-i, -2.0, 0, 2.0, i), (d0 + 0.25, 6.3, d1 - 0.25), 0.1, 0.5, "dark", 0.02)
    leaf((-i + 0.1, -2.0, 0, 2.0, i - 0.1), (d0 + 0.35, 6.3, d1 - 0.35), 0.12, 0.5, "armor2", 0.06, hard)
    # Door frame (dark structure, the load ring round the opening): a jamb up each side and a
    # lintel across the head, 0.6 m wide and 0.35 m proud of the shield, so the leaf sits back
    # in it behind the dark seam. The lintel light is set into the lintel, aimed down at the
    # open leaf; the jambs' feet stand on the mooring sockets at the ends of the hinge line.
    jx, jw, jo = dw + 0.3, 0.6, 0.35
    jamb = {}
    for sx in (-1, 1):
        p, n = on_shield(sx * (jx + jw / 2), (2.85 + d1 + 0.3) / 2, 0.0)
        v = (Y - n * Y.dot(n)).normalized()
        jamb[sx] = m.box((R(jw), R(d1 + 0.3 - 2.85), R(jo + 0.5)), place=frame(p + n * R(jo / 2 - 0.25), u=v.cross(n), v=v),
                         mat="dark", bevel=0.0)
    lw = jx + jw - 0.15  # the lintel ends over the jambs' outer faces (its ends follow the shield normal)
    lintel = leaf((-lw, -2.2, 2.2, lw), (d1 + 0.3, d1 + 0.9), jo, jo + 0.5, "dark", 0.0)
    p, n = on_shield(0.0, d1 + 0.6, jo)
    lens_strip(m, b, lintel, p, n, Y, 4.0, 0.4, aim=n - Y * 0.6, depth=0.25, mat="strobe")
    # Hinge line along the door's foot (r 0.75 m), in one housing recessed across the sill
    # (the opening actuators are inside): two knuckles on the leaf, the frame's knuckles
    # between and beside them.
    p, n = on_shield(0.0, d0 - 0.15, 0.0)
    c = p - n * R(0.25)
    cutter = m.box((R(2 * jx), R(1.9), R(1.4)), place=frame(c, u=X, w=n), bevel=0, cutter=True)
    m.cut(shield_ob, cutter)
    m.cut(split, cutter)
    for sx in (-1, 1):
        m.cyl(c + X * R(sx * 0.35), c + X * R(sx * 2.15), R(0.75), "metal", 16, bevel=R(0.06), segments=1)
        m.cyl(c + X * R(sx * 2.2), c + X * R(sx * 2.75), R(0.8), "dark", 16)
    m.cyl(c - X * R(0.3), c + X * R(0.3), R(0.8), "dark", 16)
    # Landing fittings: a mooring socket under each jamb's foot, at the ends of the sill (a hand
    # or a hand-held platform locks on there; dark metal, recessed 0.5 m).
    for sx in (-1, 1):
        p, n = on_shield(sx * (jx + jw / 2), d0 - 0.1, 0.0)
        cutter = m.cyl(p + n * R(0.4), p - n * R(0.5), R(0.45), sides=16, cutter=True)
        m.cut(shield_ob, cutter)
        m.cut(split, cutter)
        m.cyl(p - n * R(0.5), p - n * R(0.42), R(0.46), "dark", 16)
        m.cyl(p - n * R(0.45), p - n * R(0.15), R(0.16), "dark", 8)
    # External release on the right jamb, low: a yellow pull bar on a dark plate let into the
    # frame's face, the rescue arrow above it pointing down at it.
    p, n = on_shield(jx + jw / 2, 3.9, jo)
    b.box((R(0.45), R(1.0), R(0.08)), frame(p, w=n, v=Y), "dark")
    b.box((R(0.12), R(0.7), R(0.12)), frame(p + n * R(0.08), w=n, v=Y), "trim")
    a, n = on_shield(jx + jw / 2, 5.0, jo + 0.02)
    side = Y.cross(n).normalized()
    m.hull([a + q for q in (side * R(0.25), -side * R(0.25), -Y * R(0.45))]
           + [a + q + n * R(0.05) for q in (side * R(0.25), -side * R(0.25), -Y * R(0.45))], "trim", bevel=0.0)

    # Terrain lights: a lens strip set into the underside of each chest plate, aimed down and
    # ahead.
    aim = V((0, -1, -0.7)).normalized()
    for s_ in (-1, 1):
        p = hull_hit(chest[s_], V((s_ * R(7.4), R(-2.0), R(-7.0))), Y)
        lens_strip(m, b, chest_ob[s_], p + Y * R(0.08), aim, -Z, 3.5, 0.5, depth=0.45, mat="strobe")
    # Abdomen bands: thick plates with a small chamfer, each lapped over the one below: its
    # lower edge stands proud, and the lower band's top runs up under it (0.4 m of lap), so no
    # gap shows the frame between them.
    for k in range(3):
        y0 = -0.14 - k * 0.27
        w = 0.42 - 0.05 * k
        zf = -0.8 + 0.04 * k
        yt, zt = (y0 + 0.04, zf + 0.06) if k == 0 else (y0 + 0.14, zf + 0.09)
        m.hull([P(sx * w, y, z) for sx in (-1, 1) for y, z in (
            (yt, zt), (y0 - 0.16, zf), (yt, zf + 0.5), (y0 - 0.16, zf + 0.5))],
            "armor" if k != 1 else "armor2", bevel=R(0.25), segments=1, hard_facets=hard)
    # Shoulder slabs, the hull number on the left one, the navigation lenses wrapped round
    # their outer front corners (red port, cyan starboard); the mounting flange rings the
    # shoulders hang on (the bolt face is hidden under the shoulder in every pose).
    for s_ in (-1, 1):
        slab = [P(s_ * x, y, z) for x, y, z in ((0.24, 0.9, -0.62), (0.9, 0.9, -0.55), (0.24, 0.9, 0.62),
                                                (0.9, 0.9, 0.6), (0.3, 1.04, -0.5), (0.7, 1.0, -0.45),
                                                (0.3, 1.04, 0.5), (0.7, 1.0, 0.48))]
        m.hull(slab, "armor2", hard_facets=hard, **big)
        a, c = P(s_ * 0.8, 0.57, 0), P(s_ * 1.0, 0.57, 0)
        r = hy * 0.32
        m.cyl(a, c - X * s_ * R(0.6), r * 0.94, "dark", 24, bevel=R(0.3), segments=1)
        m.cyl(c - X * s_ * R(0.6), c, r, "metal", 24, bevel=R(0.1), segments=1)
        # Navigation lens: wraps the corner from the front face round to the side.
        corner = hull_hit(slab, V((s_ * R(8.0), R(11.75), R(-1.5))) + V((s_, 0, -1)).normalized() * R(8.0),
                     V((-s_, 0, 1)))
        o = corner + V((-s_ * R(0.45), 0, R(0.45)))
        pts = []
        for dy in (-0.3, 0.3):
            for ang in (0, 30, 60, 90):
                t = math.radians(ang)
                d = V((s_ * math.sin(t), 0, -math.cos(t)))
                for rr in (0.2, 0.62):
                    pts.append(o + d * R(rr) + Y * R(dy))
            pts += [o + V((-s_ * R(0.6), R(dy), -R(0.62))), o + V((s_ * R(0.62), R(dy), R(0.6)))]
        m.hull(pts, "beacon" if s_ < 0 else "nav_green", bevel=R(0.05), segments=1)
    slope = (P(-0.7, 1.0, 0) - P(-0.3, 1.04, 0)).normalized()
    seven_seg(b, P(-0.3, 1.04, -0.05) + slope * R(6.2 - 4.05) + Y * R(0.1), slope, Z, "01", 2.6, "dark")
    # Flank plates over the lower torso's sides. Each hangs from a head tucked under the chest
    # plate (a seam line where the plate meets it), and is carried by the frame: an armoured
    # lip closes the space between the frame's side and the plate's inner face down to the
    # plate's foot, and a gusset at the foot of each edge, front and back (the plate's own cast
    # foot), carries it from the frame below. Each plate carries an armoured reactor-bay
    # service hatch.
    def flank_in(y):  # real metres: the plate's inner face, the frame's side, its outer face
        return 10.8 + (y - 3.78) * (1.89 / 11.34)

    def frame_side(y):
        return 9.99 - (1.26 - y) / 13.1 * 3.91 if y <= 1.26 else 9.99 + (y - 1.26) * (0.81 / 10.08)

    def flank_out(y):
        return 12.15 + (y - 3.15) * (2.16 / 10.08)

    def frame_zr(y):
        return frame_z(R(y)) / R(1) if y <= 1.26 else 6.62 - (y - 1.26) * (1.42 / 10.08)

    for y in (-2.52, -5.92):
        # Back frame ribs from the outer longerons to the frame's corner at the lower band lines.
        x0, x1 = longeron_x(R(y), 9.0, 5.4), R(frame_side(y) - 0.2)
        for sx in (-1, 1):
            b.box((R(0.8), x1 - x0, R(0.6)), frame(V((sx * (x0 + x1) / 2, R(y), frame_z(R(y)))), u=Y, v=X), "metal")
    for s_ in (-1, 1):
        flank = [P(s_ * x, y, z) for x, y, z in ((0.8, 0.3, -0.5), (0.8, 0.3, 0.5), (0.66, -0.6, -0.5),
                                                  (0.66, -0.6, 0.5), (0.9, 0.25, -0.4), (0.9, 0.25, 0.4),
                                                  (0.74, -0.55, -0.4), (0.74, -0.55, 0.4))]
        flank_ob = m.hull(flank, "armor", hard_facets=hard, **big)
        # Seam under the chest plate: the plate's head (above) and its body are separate
        # castings, the dark joint between them 0.3 m wide and 0.35 m deep.
        ys = 1.05
        m.cut_box(flank_ob, (R(5.0), R(0.3), R(12.0)), at=V((s_ * R(11.0), R(ys), 0)))
        m.hull([V((s_ * R(x), R(y), sz * R(z))) for y in (ys - 0.2, ys + 0.2) for sz in (-1, 1)
                for x, z in ((flank_in(y) + 0.35, 4.725 - 0.35), (flank_out(y) - 0.35, 3.78 - 0.35))],
               "dark", bevel=0.0)
        # Mounting lip: the plate's armoured return, a chamfered fairing from just inside the
        # frame's front and rear corners into the plate's inner face, 0.15 m behind the plate's
        # faces; its soffit slopes from the plate's foot down onto the frame.
        m.hull([V((s_ * R(x), R(y), sz * R(z))) for sz in (-1, 1) for x, y, z in (
            (frame_side(2.6) - 0.4, 2.6, frame_zr(2.6) - 0.15), (flank_in(2.6) + 0.35, 2.6, 4.575),
            (frame_side(-8.0) - 0.4, -8.0, frame_zr(-8.0) - 0.15), (flank_in(-7.4) + 0.35, -7.4, 4.575))],
               "armor", bevel=R(0.1), segments=1)
        # Gussets at the plate's foot, front and back: tapered webs from the frame's side
        # (2.6 m of root, sunk into it) up to the plate's lower inner corner, 1.0 m thick at the
        # root and 0.6 m at the tip, their faces flush with the lip's.
        for sz in (-1, 1):
            q = []
            for x, y, t in ((frame_side(-7.2) - 0.3, -7.2, 1.0), (frame_side(-9.8) - 0.3, -9.8, 1.0),
                            (8.75, -7.25, 0.6), (9.4, -7.05, 0.6)):
                q += [V((s_ * R(x), R(y), sz * R(z))) for z in (4.575, 4.575 - t)]
            m.hull(q, "armor", bevel=0.0)
        # Reactor-bay service hatch: an armoured panel, 3.4 × 4.2 m, over the bay in the lower
        # torso that holds the reactor's power conversion and coolant pumps (the reactor itself
        # is in the waist below). It lifts up and out on two hinges at its head (a roof over the
        # worker); the release handle sits in a well at its foot; its seam is a dark joint 0.1 m
        # wide. A stencil under it.
        hn = V((s_, -0.21, 0)).normalized()
        up = (Y - hn * Y.dot(hn)).normalized()
        side = up.cross(hn)
        surf = hull_hit(flank, V((s_ * R(flank_out(-2.4)), R(-2.4), 0)) + hn * R(5.0), -hn)
        w, h_ = 3.4, 4.2
        m.cut_box(flank_ob, (R(w + 0.2), R(h_ + 0.2), R(0.6)), place=frame(surf, u=side, v=up))
        b.box((R(w + 0.2), R(h_ + 0.2), R(0.04)), frame(surf - hn * R(0.28), u=side, v=up), "dark")
        panel = m.box((R(w), R(h_), R(0.5)), place=frame(surf - hn * R(0.23), u=side, v=up), mat="armor2",
                      bevel=R(0.08), segments=1)
        well = surf - up * R(h_ / 2 - 0.55)
        m.cut_box(panel, (R(1.0), R(0.5), R(0.5)), place=frame(well, u=side, v=up))
        b.box((R(1.0), R(0.5), R(0.04)), frame(well - hn * R(0.24), u=side, v=up), "dark")
        b.box((R(0.7), R(0.12), R(0.12)), frame(well - hn * R(0.1), u=side, v=up), "trim")
        for sx in (-1, 1):
            k = surf + up * R(h_ / 2 + 0.02) + side * R(sx * (w / 2 - 0.6))
            m.cyl(k - side * R(0.4), k + side * R(0.4), R(0.2), "metal", 12)
        stencil(b, surf - up * R(h_ / 2 + 0.6), hn, up)

    # Back: blades, the radiator panel between them, the lower back plate with the docking port.
    for s_ in (-1, 1):
        m.hull([P(s_ * x, y, z) for x, y, z in ((0.3, 0.9, 0.62), (0.92, 0.9, 0.52), (0.95, 0.15, 0.62),
                                                 (0.3, 0.05, 0.7), (0.6, 0.5, 0.86), (0.3, 0.9, 0.3),
                                                 (0.92, 0.9, 0.2), (0.95, 0.15, 0.3), (0.3, 0.05, 0.3))],
               "armor", hard_facets=hard, **big)
    # Radiator (standby and life support; the shells reject reactor power): one flat emitting
    # panel facing aft, its fine ribs the heat pipes' fins; the armoured louvre box over it (the
    # supply header inside; the louvres run down over the panel in a fight on guide tracks
    # either side), the armoured return header across its foot, its ends into the frame.
    sink = P(0, 0.44, 0.74)
    rw = hx * 0.5
    m.box((rw, hy * 0.74, hz * 0.14), at=sink, mat="dark", bevel=R(0.2), segments=1)
    face = sink.z + hz * 0.07
    for k in range(12):
        x = (-0.5 + (k + 0.5) / 12) * rw
        b.box((R(0.25), hy * 0.72, R(0.3)), frame(V((x, sink.y, face + R(0.15)))), "metal", pieces(hy * 0.72))
    top = sink.y + hy * 0.37 + R(0.2)
    m.box((rw + R(0.6), R(1.0), R(1.1)), at=V((0, top, face - R(0.1))), mat="armor2", bevel=R(0.2), segments=1)
    foot = sink.y - hy * 0.37
    for s_ in (-1, 1):
        x = s_ * (rw / 2 + R(0.25))
        length = top - R(0.5) - foot
        b.box((R(0.5), length, R(0.9)), frame(V((x, foot + length / 2, face - R(0.25)))), "metal", pieces(length))
        b.box((R(0.7), length, R(0.12)), frame(V((x - s_ * R(0.1), foot + length / 2, face + R(0.26)))), "metal",
              pieces(length))
    hdr = V((0, foot - R(0.4), R(7.0)))
    m.cyl(hdr - X * R(4.4), hdr + X * R(4.4), R(0.45), "armor2", 16, bevel=R(0.05), segments=1)
    for s_ in (-1, 1):
        e = hdr + X * s_ * R(4.4)
        m.cyl(e, e - Z * R(1.2), R(0.45), "armor2", 16)
        m.cyl(e - Z * R(0.35), e - Z * R(0.6), R(0.62), "metal", 16)
    lb = P(0, -0.45, 0.66)
    lb_ob = m.box((hx * 1.05, hy * 0.75, hz * 0.3), at=lb, mat="armor2", bevel=R(0.9), segments=3)
    port = lb + V((0, 0, hz * 0.15))
    docking_port(m, b, port, Z, lamp="strobe")
    wl = port + Y * R(4.27)
    wn = V((0, 0.45, 1)).normalized()
    lens_strip(m, b, lb_ob, wl - wn * R(0.12), wn, Y, 2.0, 0.3, aim=V((0, -0.6, 1)), depth=0.35, mat="strobe")
    for s_ in (-1, 1):
        rail_channel(m, b, lb_ob, port + X * s_ * R(4.7), Z, Y)
    stencil(b, port + X * R(5.6) + Y * R(2.2), Z, Y, (1.0, 0.7))
    # Coolant service hatches, flush and bolted, at the plate's lower outer corners: there the
    # coolant crosses from the frame into the waist and is disconnected for servicing.
    for s_ in (-1, 1):
        c = V((s_ * R(5.45), R(-8.45), port.z))
        b.box((R(2.0), R(2.3), R(0.1)), frame(c), "dark")
        b.box((R(1.8), R(2.1), R(0.14)), frame(c), "armor")
        for bx in (-0.75, 0.75):
            for by in (-0.9, 0, 0.9):
                b.box((R(0.16), R(0.16), R(0.2)), frame(c + V((R(bx), R(by), 0))), "metal")

    # Comms deck on the top back ledge, behind the head: an armour deck between the slabs,
    # flat on top, sloping down aft over the louvre box. Two optical terminals (the long-range
    # laser links) in armoured wells, their iris petals retracted into the rims; on the aft
    # slope a conformal RF phased array (near and planetary links) inset in the deck.
    yt, z0, z1, z2 = R(11.6), R(2.6), R(5.5), R(8.9)
    yr = R(10.95)
    deck = m.hull([V((sx * R(3.15), y, z)) for sx in (-1, 1) for y, z in (
        (yt, z0), (yt, z1), (yr, z2), (yr - R(0.35), z2), (R(10.6), z1), (R(10.6), z0))],
        "armor2", bevel=R(0.25), segments=1)
    for sx in (-1, 1):
        c = V((sx * R(1.9), yt, R(4.1)))
        bore = m.cyl(c + Y * R(0.5), c - Y * R(0.6), R(1.1), sides=24, cutter=True)
        m.cut(deck, bore)
        rim = m.cyl(c - Y * R(0.05), c + Y * R(0.15), R(1.4), "armor2", 24, bevel=R(0.05), segments=1)
        m.cut(rim, bore)
        for k in range(6):
            t = 2 * math.pi * (k + 0.5) / 6
            d = V((math.cos(t), 0, math.sin(t)))
            b.box((R(0.3), R(0.04), R(0.06)), frame(c + d * R(1.25) + Y * R(0.16), u=d, v=Y), "dark")
        m.cyl(c - Y * R(0.6), c - Y * R(0.55), R(1.1), "dark", 24)
        m.cyl(c - Y * R(0.55), c - Y * R(0.25), R(1.0), "metal", 24, bevel=R(0.04), segments=1)
        dc = c - Y * R(0.25)
        m.lathe([(R(0.9 * math.cos(math.radians(a_))), R(0.9 * math.sin(math.radians(a_))))
                 for a_ in (0, 30, 55, 75, 90)], frame(dc), "dark", 16, angle=40.0)
        ap = V((0, 0.6, 0.8)).normalized()
        m.cyl(dc + ap * R(0.84), dc + ap * R(0.93), R(0.4), "glass", 16)
    sl = V((0, yr - yt, z2 - z1))
    t_ = sl.normalized()
    nn = X.cross(t_).normalized()
    if nn.y < 0:
        nn = -nn
    ac = V((0, yt, z1)) + sl * 0.5
    m.cut_box(deck, (R(5.0), R(3.0), R(0.2)), place=frame(ac, u=X, v=t_))
    b.box((R(3.0), R(5.0), R(0.04)), frame(ac - nn * R(0.08), u=t_, v=X), "dark", 3)
    for k in range(1, 10):
        b.box((R(0.05), R(3.0), R(0.05)), frame(ac - nn * R(0.06) + X * R(-2.5 + 0.5 * k), u=X, v=t_), "metal", 2)
    for k in range(1, 6):
        b.box((R(0.05), R(5.0), R(0.05)), frame(ac - nn * R(0.06) + t_ * R(-1.5 + 0.5 * k), u=t_, v=X), "metal", 3)
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
