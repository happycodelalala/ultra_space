"""Shell quarters and engines."""

import json
import math
from pathlib import Path

import bmesh
from mech import V, X, Y, Z, along, frame
from parts import builder, design_units
from parts_titan import Batch, R, beacon


def catmull(points, steps):
    """The game's shell profile: Catmull-Rom through the (height, radius) points, ends
    repeated."""
    p = lambda i: points[max(0, min(len(points) - 1, i))]
    out = []
    for span in range(len(points) - 1):
        p0, p1, p2, p3 = p(span - 1), p(span), p(span + 1), p(span + 2)
        for k in range(steps):
            t = k / steps

            def c(a, b, c_, d):
                return 0.5 * (2 * b + (c_ - a) * t + (2 * a - 5 * b + 4 * c_ - d) * t * t
                              + (3 * b - a - 3 * c_ + d) * t ** 3)
            out.append((c(p0[0], p1[0], p2[0], p3[0]), c(p0[1], p1[1], p2[1], p3[1])))
    out.append(tuple(points[-1]))
    return out


def resample(prof, y_hi, y_lo, step=0.25):
    """Profile points (y, r) between two heights (top to bottom), ends interpolated."""
    def r_at(y):
        for (y0, r0), (y1, r1) in zip(prof, prof[1:]):
            if y1 <= y <= y0:
                return r0 + (r1 - r0) * (y - y0) / (y1 - y0 or 1)
        return prof[-1][1] if y < prof[-1][0] else prof[0][1]
    n = max(2, int((y_hi - y_lo) / step) + 1)
    return [(y_hi - (y_hi - y_lo) * k / (n - 1), r_at(y_hi - (y_hi - y_lo) * k / (n - 1)))
            for k in range(n)]


def normal2(rows, i):
    (y0, r0), (y1, r1) = rows[max(0, i - 1)], rows[min(len(rows) - 1, i + 1)]
    dy, dr = y1 - y0, r1 - r0
    l = math.hypot(dy, dr) or 1
    return -dy / l, dr / l  # outward radial, up


def point(deg, y, r):
    t = math.radians(deg)
    return V((-r * math.sin(t), y, r * math.cos(t)))


def skin(m, rows, d0, d1, inner, outer, mat, bevel=0.012, step_deg=1.5, angle=35.0, facets=None):
    """A curved panel: the shell surface between heights (rows) and azimuths, from `inner`
    to `outer` along the surface normal (each a distance, or a function of azimuth and
    height). With `facets` (first azimuth, step) its columns
    break exactly on those facet lines, so layers built on one facet grid stay parallel."""
    bm = bmesh.new()
    if facets:
        f0, fs = facets
        k0 = math.floor((d0 - f0) / fs) + 1
        degs = [d0] + [f0 + k * fs for k in range(k0, k0 + 64) if d0 + 1e-6 < f0 + k * fs < d1 - 1e-6] + [d1]
    else:
        cols = max(2, int(abs(d1 - d0) / step_deg) + 1)
        degs = [d0 + (d1 - d0) * k / (cols - 1) for k in range(cols)]
    cols = len(degs)
    grids = []
    for off in (outer, inner):
        g = []
        for i, (y, r) in enumerate(rows):
            nr, ny = normal2(rows, i)
            row = []
            for d in degs:
                o = off(d, y) if callable(off) else off
                row.append(bm.verts.new(point(d, y + ny * o, r + nr * o)))
            g.append(row)
        grids.append(g)
    out, inn = grids
    for i in range(len(rows) - 1):
        for j in range(cols - 1):
            bm.faces.new((out[i][j], out[i][j + 1], out[i + 1][j + 1], out[i + 1][j]))
            bm.faces.new((inn[i][j], inn[i + 1][j], inn[i + 1][j + 1], inn[i][j + 1]))
    for j in range(cols - 1):
        bm.faces.new((out[0][j], inn[0][j], inn[0][j + 1], out[0][j + 1]))
        bm.faces.new((out[-1][j], out[-1][j + 1], inn[-1][j + 1], inn[-1][j]))
    for i in range(len(rows) - 1):
        bm.faces.new((out[i][0], out[i + 1][0], inn[i + 1][0], inn[i][0]))
        bm.faces.new((out[i][-1], inn[i][-1], inn[i + 1][-1], out[i + 1][-1]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    return m._object(bm, mat, frame(), bevel, 2 if angle > 20 else 1, angle=angle)


# A quarter's shape (the bay cover is built in its axes, from its curve).
QUARTER = design_units(next(p for p in json.loads((Path(__file__).resolve().parents[2] / "content/data/parts/bastion.json")
                                     .read_text())["parts"] if p["id"] == "bastion.shell_quarter_front_left"))
# The drone bay in a quarter's bulge (azimuths, heights; matches bastion.bay_cover) and the
# lift engines' sockets in its inner skin (rows of height and azimuths; the design's layout).
BAY_DEG, BAY_TOP, BAY_BOTTOM = 24.0, 5.0, -10.0
# The blister door: how far it rises out of the outer face at its middle (m).
BLISTER = 2.0
# The bulge keeps its full depth this far either side of the middle (degrees).
LENS_FLAT = 28.0
ENGINE_SOCKETS = [(-0.3, (-31.0, 31.0)), (1.9, (-31.0, 31.0))]
BANDS_BELOW_TOP = [4.6, 2.75, 2.45, -1.5, -6.0, -10.5]


def lens(t):
    """The bulge's outer face (offset from the mid-surface) at azimuth `d`: the full
    thickness across the middle, thinning toward the side seams (a lens in section)."""
    def outer(d, y):
        x = min(1.0, max(0.0, (abs(d) - LENS_FLAT) / (43.5 - LENS_FLAT)))
        return t / 2 * (1.0 - 0.75 * x * x * (3 - 2 * x))
    return outer


def blister(t):
    """The bay door's outer face (offset from the quarter's mid-surface): the bulge's face
    raised by up to BLISTER at the middle of the bay, falling to the face at its edges."""
    outer = lens(t)

    def face(d, y):
        fy = (BAY_TOP - y) / (BAY_TOP - BAY_BOTTOM)
        fy = math.sin(math.pi * min(1.0, max(0.0, fy))) ** 0.7
        fd = max(0.0, 1.0 - (d / BAY_DEG) ** 2) ** 0.5
        return outer(d, y) + BLISTER * fy * fd
    return face


def armour(m, prof, t, a0, a1, fdeg, top, bottom, inside, surface=None):
    """The armour plates over a quarter's bulge: bands down the curve (raised bands overlap
    their neighbours), three columns of two facets, a graphite band at the shoulder line,
    a second plate lapped on each of the larger panels (no bolts showing: they fix from inside). `inside(y_hi, y_lo, d0, d1)` splits each
    panel into the pieces this model draws (the quarter outside the drone bay, the bay
    cover inside it). Returns the pieces at the hem."""
    sharp = 4.0
    outer = surface or lens(t)
    bands = [top] + BANDS_BELOW_TOP + [bottom]
    gap_deg = 0.9
    cols = [(a0 + 2 * fdeg * k, a0 + 2 * fdeg * (k + 1)) for k in range(3)]
    hem = []
    for i, (y_hi, y_lo) in enumerate(zip(bands, bands[1:])):
        shoulder = abs(y_hi - 2.75) < 1e-6
        raised = i % 2 == 0 and not shoulder
        lap = 0.14 if raised else 0.0
        hi = min(top, y_hi + lap) - (0.03 if y_hi < top and not raised else 0)
        lo = max(bottom, y_lo - lap) + (0.03 if y_lo > bottom and not raised else 0)
        lift = 0.2 if raised else 0.06
        spans = [(-43.5, 43.5)] if shoulder else cols
        for c0, c1 in spans:
            g0 = gap_deg if c0 > a0 + 1e-6 else 0.1
            g1 = gap_deg if c1 < a1 - 1e-6 else 0.1
            for ph, pl_, d0, d1 in inside(hi, lo, c0 + g0, c1 - g1):
                rows = resample(prof, ph, pl_, 0.5 if shoulder else 0.8)
                if shoulder:
                    skin(m, rows, d0, d1, lambda d, y: outer(d, y) - 0.06,
                         lambda d, y: outer(d, y) + 0.06, "armor2", facets=(a0, fdeg), angle=sharp,
                         bevel=0.008)
                    continue
                panel = skin(m, rows, d0, d1, lambda d, y: outer(d, y) - 0.06,
                             lambda d, y, k=lift: outer(d, y) + k, "armor", bevel=0.07,
                             facets=(a0, fdeg), angle=sharp)
                if pl_ <= bottom + 1e-6:
                    hem.append(panel)
                # A second plate lapped on the panel.
                if ph - pl_ > 1.6 and d1 - d0 > 8:
                    r_mid = resample(prof, (ph + pl_) / 2 + 0.01, (ph + pl_) / 2 - 0.01)[0][1]
                    ins = math.degrees(0.4 / r_mid)
                    sub = resample(prof, ph - 0.45, pl_ + (0.45 if pl_ > bottom + 1e-6 else 1.6), 0.8)
                    e0, e1 = d0 + ins, d1 - ins
                    plate = skin(m, sub, e0, e1, lambda d, y, k=lift: outer(d, y) + k - 0.04,
                                 lambda d, y, k=lift: outer(d, y) + k + 0.12, "armor2", bevel=0.06,
                                 facets=(a0, fdeg), angle=sharp)
                    if pl_ <= bottom + 1e-6:
                        hem.append(plate)
    return hem


def keel(m, prof, t, top, bottom, keep, raise_=None):
    """A graphite blade down a quarter's middle (deflecting hits, stiffening it), rising to a
    point above the top; only its stretches where `keep(y)`."""
    mid = 0.0
    rows = resample(prof, top, bottom + 0.9, 0.9)
    for i, ((y0, r0), (y1, r1)) in enumerate(zip(rows, rows[1:])):
        if not keep((y0 + y1) / 2):
            continue
        f0, f1 = i / (len(rows) - 1), (i + 1) / (len(rows) - 1)
        h0, h1 = 0.3 + 0.35 * (1 - f0), 0.3 + 0.35 * (1 - f1)
        lift0, lift1 = (raise_(y0), raise_(y1)) if raise_ else (0.0, 0.0)
        base0, base1 = r0 + t / 2 + 0.1 + lift0, r1 + t / 2 + 0.1 + lift1
        pts = [point(mid + s * math.degrees(0.16 / max(r, 1)), y, rb) for s in (-1, 1)
               for y, r, rb in ((y0, r0, base0), (y1, r1, base1))]
        pts += [point(mid, y0, base0 + h0), point(mid, y1, base1 + h1)]
        if i == 0:
            pts.append(point(mid, y0 + 0.9, r0 + t / 2 - 0.1))
        m.hull(pts, "armor2", bevel=0.004)


def outside_bay(hi, lo, d0, d1):
    """The pieces of a panel outside the drone bay."""
    if lo >= BAY_TOP or hi <= BAY_BOTTOM or d1 <= -BAY_DEG or d0 >= BAY_DEG:
        return [(hi, lo, d0, d1)]
    out = []
    if hi > BAY_TOP:
        out.append((hi, BAY_TOP, d0, d1))
    if lo < BAY_BOTTOM:
        out.append((BAY_BOTTOM, lo, d0, d1))
    yh, yl = min(hi, BAY_TOP), max(lo, BAY_BOTTOM)
    if d0 < -BAY_DEG - 0.3:
        out.append((yh, yl, d0, -BAY_DEG - 0.3))
    if d1 > BAY_DEG + 0.3:
        out.append((yh, yl, BAY_DEG + 0.3, d1))
    return [p for p in out if p[0] - p[1] > 0.15]


def inside_bay(hi, lo, d0, d1):
    """The piece of a panel over the drone bay (the cover's)."""
    yh, yl = min(hi, BAY_TOP - 0.05), max(lo, BAY_BOTTOM + 0.05)
    e0, e1 = max(d0, -BAY_DEG + 0.2), min(d1, BAY_DEG - 0.2)
    return [(yh, yl, e0, e1)] if yh - yl > 0.15 and e1 > e0 else []


@builder("bastion.shell_quarter")
def shell_quarter(m, h, part):
    """A quarter of the cocoon, a platform in flight (design/bastion.md): a thick faceted
    bulge, lens-shaped in section, its structure between a dark inner skin and big banded
    armour plates (a second plate lapped on the larger ones, a band in the second paint at
    the shoulder line), a steel band along the hem. In the middle of the bulge the drone bay
    (floor and walls; its door is a separate part) with lights under its top wall and amber
    approach lights along its rim for drones homing on it; red obstruction beacons on the top
    (the cocoon's highest point) and at the hem's outer corners. The four lift engines'
    gimbal mounts sit in sockets in the inner skin; a mounting pad takes the carrier."""
    shape = part["shape"]
    P = shape["profile_m"]
    a0, a1 = shape["arc_deg"]
    t = shape["thickness_m"]
    prof = catmull(P, 40)
    top, bottom = P[0][0], P[-1][0]
    fdeg = (a1 - a0) / 6
    sharp = 4.0
    outer = lens(t)
    inner_face = -t / 2
    hem = []
    # Inner skin, the side walls closing the bulge at the seams, the core.
    rows = resample(prof, top, bottom, 1.0)
    inner = skin(m, rows, a0, a1, inner_face, inner_face + 0.15, "dark", facets=(a0, fdeg), angle=sharp)
    hem.append(inner)
    for d0, d1 in ((a0, a0 + 0.4), (a1 - 0.4, a1)):
        hem.append(skin(m, rows, d0, d1, inner_face + 0.15, lambda d, y: outer(d, y) - 0.06, "dark",
                        facets=(a0, fdeg), angle=sharp))
    # Engine sockets through the inner skin, each with a reinforcing ring.
    for y, degs in ENGINE_SOCKETS:
        r = resample(prof, y + 0.01, y - 0.01)[0][1]
        for d in degs:
            n = point(d, 0, 1)
            a, b = point(d, y, r + inner_face - 0.3), point(d, y, r + inner_face + 0.9)
            m.hole(inner, a, b, 0.55, 8)
            c = point(d, y, r + inner_face)
            m.lathe([(0.55, -0.03), (0.75, -0.03), (0.75, 0.03), (0.55, 0.03)], along(c, c + n), "metal", 8,
                    closed=True, angle=20.0)
    # The armour, except over the drone bay.
    hem += armour(m, prof, t, a0, a1, fdeg, top, bottom, outside_bay)
    # The drone bay: a floor on the inner skin and walls round it, up to the armour.
    bay_rows = resample(prof, BAY_TOP, BAY_BOTTOM, 0.8)
    skin(m, bay_rows, -BAY_DEG, BAY_DEG, inner_face + 0.15, inner_face + 0.25, "armor2", facets=(a0, fdeg),
         angle=sharp)
    for d0, d1 in ((-BAY_DEG - 0.3, -BAY_DEG), (BAY_DEG, BAY_DEG + 0.3)):
        skin(m, bay_rows, d0, d1, inner_face + 0.15, lambda d, y: outer(d, y) + 0.06, "dark",
             facets=(a0, fdeg), angle=sharp)
    for y0, y1 in ((BAY_TOP + 0.15, BAY_TOP), (BAY_BOTTOM, BAY_BOTTOM - 0.15)):
        skin(m, resample(prof, y0, y1, 0.15), -BAY_DEG, BAY_DEG, inner_face + 0.15,
             lambda d, y: outer(d, y) + 0.06, "dark", facets=(a0, fdeg), angle=sharp)
    # A steel edge band along the hem (it takes the knocks), lifting eyes near the top.
    skin(m, resample(prof, bottom + 0.35, bottom, 0.35), a0 + 0.3, a1 - 0.3, inner_face - 0.02,
         lambda d, y: outer(d, y) + 0.1, "metal", facets=(a0, fdeg), angle=sharp, bevel=0.01)
    # Red beacons along the top and at the hem's outer corners; amber approach lights round
    # the drone bay's rim, for drones coming home.
    bt = Batch(m)
    for d, y in ((0.0, top - 0.25), (a0 + 3.0, top - 1.2), (a1 - 3.0, top - 1.2), (a0 + 3.0, bottom + 0.6),
                 (a1 - 3.0, bottom + 0.6)):
        r = resample(prof, y + 0.01, y - 0.01)[0][1]
        beacon(bt, point(d, y, r + outer(d, y) + 0.2), 0.9)
    for d in (-BAY_DEG - 1.2, BAY_DEG + 1.2):
        for k in range(8):
            y = BAY_TOP - 0.5 - k * (BAY_TOP - BAY_BOTTOM - 1.0) / 7
            r = resample(prof, y + 0.01, y - 0.01)[0][1]
            p = point(d, y, r + outer(d, y) + 0.14)
            n = point(d, 0, 1)
            bt.box((R(0.8), R(0.8), R(0.5)), frame(p, w=n, v=Y), "light")
    # Bay lights under the bay's top wall, lighting it when the door is open.
    for d in (-16.0, -5.0, 5.0, 16.0):
        y = BAY_TOP - 0.12
        r = resample(prof, y + 0.01, y - 0.01)[0][1]
        bt.box((R(2.0), R(0.3), R(0.9)), frame(point(d, y, r + inner_face + 0.9), w=point(d, 0, 1), v=Y), "light")
    bt.done()
    # Carrier pad.
    for node in part.get("attach_nodes", []):
        p, d = V(node["position_m"]), V(node["direction"]).normalized()
        k = 0.3
        out = [(-0.9 + k, -0.75), (0.9 - k, -0.75), (0.9, -0.75 + k), (0.9, 0.75 - k), (0.9 - k, 0.75),
               (-0.9 + k, 0.75), (-0.9, 0.75 - k), (-0.9, -0.75 + k)]
        m.plate(out, 0.14, frame(p - d * 0.14, w=d, v=Y), "armor2", chamfer=0.05)


@builder("bastion.bay_cover")
def bay_cover(m, h, part, quarter=None):
    """A drone bay's door: a faceted blister rising out of the middle of its quarter's bulge
    (up to BLISTER at its middle, meeting the quarter's face all round its seam), under the
    same scale armour and keel as the rest of the quarter; a stiffening inner skin and a
    hinge knuckle along its top edge. It pivots up and out to open the bay under it."""
    q = quarter or QUARTER
    shape = q["shape"]
    P = shape["profile_m"]
    a0, a1 = shape["arc_deg"]
    t = shape["thickness_m"]
    prof = catmull(P, 40)
    top, bottom = P[0][0], P[-1][0]
    fdeg = (a1 - a0) / 6
    face = blister(t)
    lid = part["shape"]["thickness_m"]
    rows = resample(prof, BAY_TOP - 0.05, BAY_BOTTOM + 0.05, 0.8)
    skin(m, rows, -BAY_DEG + 0.2, BAY_DEG - 0.2, lambda d, y: face(d, y) - lid,
         lambda d, y: face(d, y) - 0.06, "dark", facets=(a0, fdeg), angle=4.0)
    armour(m, prof, t, a0, a1, fdeg, top, bottom, inside_bay, surface=face)
    # The door's lower edge swings out: a hazard band along it, latches above it.
    y = BAY_BOTTOM + 0.3
    r = resample(prof, y + 0.01, y - 0.01)[0][1]
    step = 8.0
    d = -BAY_DEG + 1.0
    while d < BAY_DEG - 1.5:
        e = min(d + step, BAY_DEG - 1.0)
        mid = (d + e) / 2
        lift = 0.25
        m.hazard(point(d, y, r + face(d, y) + lift), point(e, y, r + face(e, y) + lift), point(mid, 0, 1),
                 0.35, 0.6, 0.04)
        d = e
    for d in (-12.0, 0.0, 12.0):
        y = BAY_BOTTOM + 1.0
        r = resample(prof, y + 0.01, y - 0.01)[0][1]
        n = point(d, 0, 1)
        m.latch(point(d, y, r + face(d, y) + 0.12), n, Y, 0.16)


@builder("bastion.lift_engine", "thruster.main.medium")
def engine(m, h, part):
    """A gimballed engine: a faceted mounting plate on the structure, a thrust frame of four
    struts down to the gimbal ring on its trunnions, the octagonal combustion chamber, two
    sealed gimbal actuators, and a faceted bell nozzle (-Y) with cooling bands round the
    injector plate (the game draws the exhaust when it fires)."""
    r, hl, _ = h
    top = hl
    k = r * 0.4
    out = [(-r * 0.75 + k, -r * 0.75), (r * 0.75 - k, -r * 0.75), (r * 0.75, -r * 0.75 + k), (r * 0.75, r * 0.75 - k),
           (r * 0.75 - k, r * 0.75), (-r * 0.75 + k, r * 0.75), (-r * 0.75, r * 0.75 - k), (-r * 0.75, -r * 0.75 + k)]
    m.plate(out, 0.06, frame((0, top - 0.06, 0), u=X, w=Y), "armor2", chamfer=0.02)
    gy = hl * 0.05
    rg = r * 0.5
    for sx in (-1, 1):
        for sz in (-1, 1):
            m.cyl((sx * r * 0.5, top - 0.06, sz * r * 0.5), (sx * rg * 0.75, gy + 0.05, sz * rg * 0.75),
                  0.03, "dark", 6)
    place = frame((0, 0, 0))
    m.lathe([(rg * 0.85, gy - 0.04), (rg, gy - 0.04), (rg * 1.05, gy), (rg, gy + 0.04), (rg * 0.85, gy + 0.04)],
            place, "dark", 8, closed=True, angle=20.0)
    m.cyl((-rg * 1.15, gy, 0), (rg * 1.15, gy, 0), 0.03, "chrome", 8)
    rc = r * 0.3
    m.lathe([(0, top - 0.06), (rc * 0.6, top - 0.06), (rc * 0.9, top - 0.12), (rc, top - 0.2),
             (rc, hl * 0.0), (rc * 0.62, -hl * 0.12), (rc * 0.55, -hl * 0.16)], place, "metal", 8, angle=20.0)
    for u in (X, Z):
        m.piston(u * r * 0.62 + Y * (top - 0.08), u * rc * 1.15 + Y * hl * 0.05, 0.03, extension=0.3,
                 hint=Y.cross(u))
    ys = [-hl * 0.16 - (hl * 0.84) * k / 10 for k in range(11)]
    rt, re = rc * 0.55, r * 0.72
    outer = [(rt + (re - rt) * (math.sqrt(k / 10)), y) for k, y in enumerate(ys)]
    wall = 0.02
    prof = outer + [(x - wall, y) for x, y in reversed(outer)]
    m.lathe(prof, place, "dark", 12, angle=20.0, closed=True)
    m.lathe([(0, ys[0] + 0.01), (rt * 0.95, ys[0] + 0.01), (rt * 1.3, ys[2]), (0, ys[2])], place, "metal", 12)
    for k in (4, 8):
        x, y = outer[k]
        m.lathe([(x - 0.005, y + 0.03), (x + 0.02, y + 0.02), (x + 0.02, y - 0.02), (x - 0.005, y - 0.03)],
                place, "metal", 12, closed=True, angle=20.0)
    x, y = outer[-1]
    m.lathe([(x - 0.005, y + 0.04), (x + 0.035, y + 0.025), (x + 0.035, y), (x - 0.005, y)],
            place, "armor2", 12, closed=True, angle=20.0)
