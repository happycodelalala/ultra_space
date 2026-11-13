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


def skin(m, rows, d0, d1, inner, outer, mat, bevel=0.012, step_deg=1.5, angle=35.0, facets=None):
    """A curved panel: the shell surface between heights (rows) and azimuths, from `inner`
    to `outer` along the surface normal. With `facets` (first azimuth, step) its columns
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
    return m._object(bm, mat, frame(), bevel, 2 if angle > 20 else 1, angle=angle)


@builder("bastion.shell_quarter")
def shell_quarter(m, h, part):
    """A quarter of the cocoon, faceted like a cut stone. A dark structural skin carries thick
    ceramic armour plates in bands that overlap like scales (each band sheds hits onto the
    one below), a graphite band at the shoulder line where the upper and lower plate sets
    meet; a graphite keel runs down the middle (it deflects hits and stiffens the quarter)
    and rises to a point above the top; the hem is cut into points, with a marker lamp on
    the outer ones. Inside, ribs down the curve and two ring frames across it carry the lift
    engines and the folded leg; a mounting pad takes the carrier."""
    shape = part["shape"]
    P = shape["profile_m"]
    a0, a1 = shape["arc_deg"]
    t = shape["thickness_m"]
    prof = catmull(P, 40)
    top, bottom = P[0][0], P[-1][0]
    facets = 6
    fdeg = (a1 - a0) / facets
    sharp = 4.0
    hem = []
    # Structural skin.
    hem.append(skin(m, resample(prof, top, bottom, 1.0), a0, a1, -t / 2, t / 2 - 0.05, "dark",
                    facets=(a0, fdeg), angle=sharp))
    # Armour plates: bands down the curve (raised bands overlap their neighbours), three
    # columns of two facets each; the shoulder band is graphite.
    bands = [top, 4.6, 2.75, 2.45, -1.5, -6.0, -10.5, bottom]
    gap_deg = 0.45
    cols = [(a0 + 2 * fdeg * k, a0 + 2 * fdeg * (k + 1)) for k in range(3)]
    for i, (y_hi, y_lo) in enumerate(zip(bands, bands[1:])):
        shoulder = abs(y_hi - 2.75) < 1e-6
        raised = i % 2 == 0 and not shoulder
        lap = 0.14 if raised else 0.0
        hi = min(top, y_hi + lap) - (0.03 if y_hi < top and not raised else 0)
        lo = max(bottom, y_lo - lap) + (0.03 if y_lo > bottom and not raised else 0)
        rows = resample(prof, hi, lo, 0.8)
        out = t / 2 + (0.2 if raised else 0.06)
        if shoulder:
            skin(m, resample(prof, hi, lo, 0.5), a0, a1, t / 2 - 0.06, t / 2 + 0.06, "armor2",
                 facets=(a0, fdeg), angle=sharp, bevel=0.008)
            continue
        for c0, c1 in cols:
            g0 = gap_deg if c0 > a0 + 1e-6 else 0.1
            g1 = gap_deg if c1 < a1 - 1e-6 else 0.1
            panel = skin(m, rows, c0 + g0, c1 - g1, t / 2 - 0.06, out, "armor", bevel=0.01,
                         facets=(a0, fdeg), angle=sharp)
            if y_lo <= bottom + 1e-6:
                hem.append(panel)
            # A raised plate inset on the panel.
            if hi - lo > 1.6:
                r_mid = resample(prof, (hi + lo) / 2 + 0.01, (hi + lo) / 2 - 0.01)[0][1]
                ins = math.degrees(0.4 / r_mid)
                sub = resample(prof, hi - 0.45, lo + (0.45 if y_lo > bottom + 1e-6 else 1.6), 0.8)
                d0, d1 = c0 + g0 + ins, c1 - g1 - ins
                plate = skin(m, sub, d0, d1, out - 0.04, out + 0.09, "armor", bevel=0.012,
                             facets=(a0, fdeg), angle=sharp)
                if y_lo <= bottom + 1e-6:
                    hem.append(plate)
                # Cut to a chevron: its lower corners off, so it points down.
                dm = (d0 + d1) / 2
                ylo, rlo = sub[-1]
                w = math.radians(d1 - d0) / 2 * (rlo + out)
                hg = min(1.1, w * 0.55)
                pl = frame(point(dm, ylo, rlo + out), u=point(dm + 90, 0, 1), v=Y)
                for sx in (-1, 1):
                    tri = [(sx * 1.6 * w, 1.6 * hg), (-sx * 0.2 * w, -0.2 * hg), (sx * 1.6 * w, -hg)]
                    m.cut(plate, m.extrude(tri, 1.0, pl, bevel=0, cutter=True))
    # Keel: a blade down the middle, rising to a point above the top.
    mid = (a0 + a1) / 2
    krows = resample(prof, top, bottom + 0.9, 0.9)
    for i, ((y0, r0), (y1, r1)) in enumerate(zip(krows, krows[1:])):
        f0, f1 = i / (len(krows) - 1), (i + 1) / (len(krows) - 1)
        h0, h1 = 0.3 + 0.35 * (1 - f0), 0.3 + 0.35 * (1 - f1)
        base0, base1 = r0 + t / 2 + 0.1, r1 + t / 2 + 0.1
        pts = [point(mid + s * math.degrees(0.16 / max(r, 1)), y, rb) for s in (-1, 1)
               for y, r, rb in ((y0, r0, base0), (y1, r1, base1))]
        pts += [point(mid, y0, base0 + h0), point(mid, y1, base1 + h1)]
        if i == 0:
            pts.append(point(mid, y0 + 0.9, r0 + t / 2 - 0.1))
        m.hull(pts, "armor2", bevel=0.004)
    # Hem cut into points: a V notch at every column boundary.
    rb = resample(prof, bottom + 0.01, bottom - 0.01)[0][1]
    for d in (a0, cols[0][1], cols[1][1], a1):
        half = math.radians(fdeg) * rb
        c = point(d, bottom, rb)
        pl = frame(c, u=point(d + 90, 0, 1), v=Y)
        cutter = m.extrude([(-half, -0.5), (half, -0.5), (0, 1.3)], 3.0, pl, bevel=0, cutter=True)
        for ob in hem:
            m.cut(ob, cutter)
    for c0, c1 in (cols[0], cols[2]):
        d = (c0 + c1) / 2
        y = bottom + 0.5
        r = resample(prof, y + 0.01, y - 0.01)[0][1]
        m.lamp(point(d, y, r + t / 2 + 0.2), point(d, 0, 1), 0.07)
    # Ribs inside: I-sections down the curve.
    inner = [(y, r) for y, r in resample(prof, top - 0.3, bottom + 1.5, 0.6)]
    for frac in (0.1, 0.5, 0.9):
        d = a0 + (a1 - a0) * frac
        for (y0, r0), (y1, r1) in zip(inner, inner[1:]):
            pa = point(d, y0, r0 - t / 2 - 0.1)
            pb = point(d, y1, r1 - t / 2 - 0.1)
            tg = point(d + 90, 0, 1)
            pl = along(pa, pb, hint=tg)
            ln = (pb - pa).length
            m.box((0.04, ln + 0.01, 0.2), place=pl @ frame((0, ln / 2, 0)), mat="dark", bevel=0)
            m.box((0.16, ln + 0.01, 0.03), place=pl @ frame((0, ln / 2, 0.09)), mat="dark", bevel=0)
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
        k = 0.3
        out = [(-0.9 + k, -0.75), (0.9 - k, -0.75), (0.9, -0.75 + k), (0.9, 0.75 - k), (0.9 - k, 0.75),
               (-0.9 + k, 0.75), (-0.9, 0.75 - k), (-0.9, -0.75 + k)]
        m.plate(out, 0.14, frame(p - d * 0.14, w=d, v=Y), "armor2", chamfer=0.05)


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
