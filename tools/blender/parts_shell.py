"""Shell quarters and engines."""

import math

import bmesh
from mech import V, X, Y, Z, along, frame
from parts import builder


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


def skin(m, rows, d0, d1, inner, outer, mat, bevel=0.012, step_deg=1.5):
    """A curved panel: the shell surface between heights (rows) and azimuths, from `inner`
    to `outer` along the surface normal."""
    bm = bmesh.new()
    cols = max(2, int(abs(d1 - d0) / step_deg) + 1)
    degs = [d0 + (d1 - d0) * k / (cols - 1) for k in range(cols)]
    grids = []
    for off in (outer, inner):
        g = []
        for i, (y, r) in enumerate(rows):
            nr, ny = normal2(rows, i)
            g.append([bm.verts.new(point(d, y + ny * off, r + nr * off)) for d in degs])
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
    return m._object(bm, mat, frame(), bevel, 2, angle=35.0)


@builder("bastion.shell_quarter")
def shell_quarter(m, h, part):
    """A quarter of the cocoon. A structural skin carries bolted armour panels outside
    (with gaps between them and a hazard band at the shoulder line) and, inside, I-section
    ribs down the curve and two ring frames across it, which the lift engines and the
    folded leg mount on; a reinforced pad takes the carrier's bolts; marker lamps near
    the hem."""
    shape = part["shape"]
    P = shape["profile_m"]
    a0, a1 = shape["arc_deg"]
    t = shape["thickness_m"]
    prof = catmull(P, 40)
    top, bottom = P[0][0], P[-1][0]
    # Structural skin.
    skin(m, resample(prof, top, bottom), a0, a1, -t / 2, t / 2 - 0.05, "armor2", step_deg=1.5)
    # Armour panels: bands down the curve, three columns across.
    bands = [top, 4.6, 2.75, 2.45, -1.5, -6.0, -10.5, bottom]
    gap_deg = 0.35
    mid = (a0 + a1) / 2
    cols = [(a0, a0 + (a1 - a0) / 3), (a0 + (a1 - a0) / 3, a0 + 2 * (a1 - a0) / 3),
            (a0 + 2 * (a1 - a0) / 3, a1)]
    for y_hi, y_lo in zip(bands, bands[1:]):
        hazard = abs(y_hi - 2.75) < 1e-6
        rows = resample(prof, y_hi - (0.02 if y_hi < top else 0), y_lo + (0.02 if y_lo > bottom else 0))
        for c0, c1 in ([(a0, a1)] if hazard else cols):
            g0 = gap_deg if c0 > a0 + 1e-6 else 0
            g1 = gap_deg if c1 < a1 - 1e-6 else 0
            skin(m, rows, c0 + g0, c1 - g1, t / 2 - 0.06, t / 2 + 0.03,
                 "trim" if hazard else "armor", bevel=0.012)
    # Bolts along a few panel seams.
    for y in (4.6, -1.5, -6.0):
        r = resample(prof, y + 0.01, y - 0.01)[0][1]
        for k in range(12):
            d = a0 + 2 + (a1 - a0 - 4) * k / 11
            nr = 1.0
            m.bolt(point(d, y + 0.12, r + t / 2 + 0.03), point(d, 0, 1), 0.025)
    # Ribs inside: I-sections down the curve.
    inner = [(y, r) for y, r in resample(prof, top - 0.3, bottom + 0.3, 0.6)]
    for frac in (0.1, 0.5, 0.9):
        d = a0 + (a1 - a0) * frac
        for (y0, r0), (y1, r1) in zip(inner, inner[1:]):
            pa = point(d, y0, r0 - t / 2 - 0.1)
            pb = point(d, y1, r1 - t / 2 - 0.1)
            tg = point(d + 90, 0, 1)
            pl = along(pa, pb, hint=tg)
            ln = (pb - pa).length
            m.box((0.04, ln + 0.01, 0.2), place=pl @ frame((0, ln / 2, 0)), mat="armor2", bevel=0)
            m.box((0.16, ln + 0.01, 0.03), place=pl @ frame((0, ln / 2, 0.09)), mat="armor2", bevel=0)
    # Ring frames across, at the engine rows and the leg cradle.
    for y in (1.5, -6.0):
        r = resample(prof, y + 0.01, y - 0.01)[0][1] - t / 2 - 0.12
        pts = [point(a0 + 1 + (a1 - a0 - 2) * k / 16, y, r) for k in range(17)]
        for pa, pb in zip(pts, pts[1:]):
            pl = along(pa, pb, hint=Y)
            ln = (pb - pa).length
            m.box((0.22, ln + 0.01, 0.05), place=pl @ frame((0, ln / 2, 0)), mat="dark", bevel=0)
            m.box((0.03, ln + 0.01, 0.24), place=pl @ frame((0, ln / 2, 0)), mat="dark", bevel=0)
    # Carrier pad.
    for node in part.get("attach_nodes", []):
        p, d = V(node["position_m"]), V(node["direction"]).normalized()
        pad = frame(p - d * 0.06, w=d, v=Y)
        m.box((1.8, 1.5, 0.12), place=pad, mat="metal", bevel=0.02)
        for x in (-0.75, -0.25, 0.25, 0.75):
            for y in (-0.6, 0.6):
                m.bolt(pad @ V((x, y, 0.06)), d, 0.03)
    # Marker lamps near the hem.
    for frac in (0.2, 0.8):
        d = a0 + (a1 - a0) * frac
        y = bottom + 1.2
        r = resample(prof, y + 0.01, y - 0.01)[0][1]
        c = point(d, y, r + t / 2 + 0.03)
        n = point(d, 0, 1)
        m.cyl(c, c + n * 0.05, 0.09, "metal", 16, bevel=0.005)
        m.cyl(c + n * 0.05, c + n * 0.08, 0.065, "light", 16)


@builder("bastion.lift_engine", "thruster.main.medium")
def engine(m, h, part):
    """A gimballed engine: a square mounting plate bolted to the structure, a thrust frame of
    four struts down to the gimbal ring on its trunnions, the combustion chamber with its
    injector dome, a turbopump feeding it through a duct, two gimbal rams, and a thin-walled
    bell nozzle with cooling bands and the exit manifold (-Y)."""
    r, hl, _ = h
    top = hl
    m.box((r * 1.5, 0.05, r * 1.5), at=(0, top - 0.025, 0), mat="metal", bevel=0.01)
    for sx in (-1, 1):
        m.bolt_row((sx * r * 0.65, top, -r * 0.65), (sx * r * 0.65, top, r * 0.65), Y, 4, size=0.02)
    gy = hl * 0.05
    rg = r * 0.5
    for sx in (-1, 1):
        for sz in (-1, 1):
            m.cyl((sx * r * 0.55, top - 0.05, sz * r * 0.55), (sx * rg * 0.75, gy + 0.05, sz * rg * 0.75),
                  0.025, "metal", 8)
    place = frame((0, 0, 0))
    m.lathe([(rg * 0.85, gy - 0.04), (rg, gy - 0.04), (rg * 1.05, gy), (rg, gy + 0.04),
             (rg * 0.85, gy + 0.04)], place, "dark", 40, closed=True)
    m.cyl((-rg * 1.15, gy, 0), (rg * 1.15, gy, 0), 0.03, "chrome", 12)
    rc = r * 0.3
    m.lathe([(0, top - 0.08), (rc * 0.6, top - 0.08), (rc * 0.9, top - 0.14), (rc, top - 0.2),
             (rc, hl * 0.0), (rc * 0.62, -hl * 0.12), (rc * 0.55, -hl * 0.16)], place, "metal", 32,
            bevel=0.004)
    tp = V((r * 0.55, hl * 0.45, 0))
    m.lathe([(0, -0.1), (r * 0.14, -0.1), (r * 0.16, -0.06), (r * 0.16, 0.08), (r * 0.12, 0.1), (0, 0.1)],
            frame(tp), "dark", 24, bevel=0.003)
    m.hose([tp + V((0, -0.1, 0)), tp + V((-r * 0.1, -0.2, 0)), V((rc * 0.9, hl * 0.2, 0))], 0.035,
           mat="metal")
    m.hose([tp + V((0, 0.1, 0)), tp + V((-0.05, 0.2, 0.1)), V((0, top - 0.06, rc))], 0.025, mat="metal")
    for u in (X, Z):
        m.ram(u * r * 0.68 + Y * (top - 0.08), u * rc * 1.15 + Y * hl * 0.05 + Z.cross(u) * 0.0, 0.03,
              extension=0.3, hint=Y.cross(u), ports=False)
    # Bell nozzle: thin wall from the throat to the exit.
    ys = [-hl * 0.16 - (hl * 0.84) * k / 10 for k in range(11)]
    rt, re = rc * 0.55, r * 0.72
    outer = [(rt + (re - rt) * (math.sqrt(k / 10)), y) for k, y in enumerate(ys)]
    wall = 0.018
    prof = outer + [(x - wall, y) for x, y in reversed(outer)]
    m.lathe(prof, place, "dark", 40, angle=60.0, closed=True)
    for k in (3, 6, 9):
        x, y = outer[k]
        m.lathe([(x, y + 0.012), (x + 0.012, y + 0.008), (x + 0.012, y - 0.008), (x, y - 0.012)],
                place, "metal", 40, closed=True)
    x, y = outer[-1]
    m.lathe([(x - 0.005, y + 0.03), (x + 0.03, y + 0.02), (x + 0.03, y - 0.005), (x - 0.005, y - 0.005)],
            place, "metal", 40, closed=True)
