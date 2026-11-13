"""Shell arm and folding-leg hardware: knees, wrist rotor, leg segments, carrier, saddle,
grapple."""

import math

from mech import V, X, Y, Z, along, frame
from parts import builder, nodes


def octagon(w, h, k):
    """An elongated octagon, `w` by `h`, corners cut by `k`."""
    return [(-w / 2 + k, -h / 2), (w / 2 - k, -h / 2), (w / 2, -h / 2 + k), (w / 2, h / 2 - k),
            (w / 2 - k, h / 2), (-w / 2 + k, h / 2), (-w / 2, h / 2 - k), (-w / 2, -h / 2 + k)]


@builder("bastion.fold_knee_left", "bastion.fold_knee_right")
def fold_knee(m, h, part):
    """A folding knee: a yoke whose faceted blade cheeks wrap round the bend between the two
    side-by-side segments. The base segment sits in a graphite socket; the end segment turns
    on an octagonal knuckle at its root (the pivot, end node), glowing at its gaps, driven
    by a sealed actuator along the yoke through a crank."""
    hx, hy, hz = h
    ns = nodes(part)
    pb, pe = ns["base"][0], ns["end"][0]
    zc = hz * 0.78
    tc = max(0.04, hz * 0.16)
    out = [(-hx * 0.98, hy * 0.5), (-hx * 0.98, -hy * 0.35), (-hx * 0.6, -hy * 0.98), (hx * 0.6, -hy * 0.98),
           (hx * 0.98, -hy * 0.35), (hx * 0.98, hy * 0.5)]
    for s in (-1, 1):
        if s > 0:
            pl = frame((0, 0, zc - tc / 2), u=(1, 0, 0), v=(0, 1, 0))
            pts = out
        else:
            pl = frame((0, 0, -zc + tc / 2), u=(-1, 0, 0), v=(0, 1, 0))
            pts = [(-x, y) for x, y in out]
        cheek = m.plate(pts, tc, pl, "armor", chamfer=min(0.03, tc * 0.4))
        m.slit(cheek, (-hx * 0.55, -hy * 0.62, s * (zc + tc / 2)), (hx * 0.55, -hy * 0.62, s * (zc + tc / 2)),
               (0, 0, s), 0.022, 0.025)
    m.box((2 * hx * 0.9, 0.05, 2 * zc), at=(0, -hy * 0.75, 0), mat="dark", bevel=0.01, segments=1)
    m.hull([(pb.x + sx * hx * 0.42, pb.y - y, sz * (zc - tc / 2)) for sx in (-1, 1) for sz in (-1, 1)
            for y in (0.0,)] + [(pb.x + sx * hx * 0.3, pb.y - hy * 0.65, sz * (zc - tc / 2)) for sx in (-1, 1)
                                 for sz in (-1, 1)], "armor2", bevel=0.01)
    rk = min(hz * 0.7, hy * 0.75)
    m.drum(pe - X * (hx * 0.42), pe + X * (hx * 0.42), rk, "dark", 8, hint=(0, 1, 0))
    for s in (-1, 1):
        m.ring_glow(pe + X * (s * hx * 0.43), X, rk * 0.35, rk * 0.55, 0.02)
    rr = max(0.035, hz * 0.12)
    zr = -(zc + tc + rr * 1.2)
    m.piston((-hx * 0.8, -hy * 0.3, zr), (hx * 0.55, hy * 0.05, zr), rr, extension=0.4, hint=(0, 0, 1))


@builder("bastion.arm_rotor")
def rotor(m, h, part):
    """A wrist rotor: a faceted flange on the leg, the sealed slewing drive glowing round its
    gap, two pinion motors driving it, and the turning face plate the grapple mounts to."""
    hx, hy, hz = h
    r = min(hx, hz)
    m.plate(octagon(2 * r * 0.95, 2 * r * 0.95, r * 0.55), 0.08, frame((0, hy - 0.08, 0), u=X, w=Y), "armor2",
            chamfer=0.03)
    m.drum((0, hy - 0.08, 0), (0, -hy * 0.35, 0), r * 0.82, "dark", 8, hint=X)
    for s in (-1, 1):
        c = V((s * r * 0.95, 0, 0))
        m.drum(c + V((0, -hy * 0.4, 0)), c + V((0, hy * 0.6, 0)), r * 0.2, "metal", 6, glow=True, hint=Z)
    m.lathe([(0, -hy * 0.35), (r * 0.82, -hy * 0.35), (r * 0.82, -hy * 0.85), (r * 0.7, -hy), (0, -hy)],
            frame(), "armor2", 8, angle=20.0)
    m.ring_glow((0, -hy * 0.6, 0), Y, r * 0.83, r * 0.86, 0.04)


@builder("bastion.leg_1", "bastion.leg_2", "bastion.leg_3", "bastion.leg_4", "bastion.leg_5",
         "bastion.leg_6")
def spider(m, h, part):
    """A folding-leg segment: a hexagonal spine of dark frame metal, ribbed, tapering to the
    tip; faceted clevis cheeks at each end round the pins; a pointed ceramic fairing on the
    outer face with a glowing seam; the fold actuator along one side and a ribbed power
    conduit along the other."""
    hx, hy, hz = h
    r = hx * 0.55
    rb = hx * 0.62
    end = hy - rb * 1.1
    m.lathe([(r * 0.95, -end), (r * 1.05, -end + 0.1), (r * 0.9, -end * 0.2), (r * 0.98, end - 0.1),
             (r * 0.9, end)], frame(), "dark", 6, angle=20.0)
    for y in (-end * 0.6, -end * 0.2, end * 0.2, end * 0.6):
        m.lathe([(r * 0.9, y - 0.06), (r * 1.12, y - 0.03), (r * 1.12, y + 0.03), (r * 0.9, y + 0.06)],
                frame(), "metal", 6, closed=True, angle=20.0)
    for s in (-1, 1):
        y = s * (hy - rb)
        for sx in (-1, 1):
            inner = s * (end - rb * 0.8) - y
            out = [(-rb * 0.7, inner), (rb * 0.7, inner), (rb * 0.95, 0), (rb * 0.4, s * rb),
                   (-rb * 0.4, s * rb), (-rb * 0.95, 0)]
            if sx > 0:
                pl = frame((hx * 0.6, y, 0), u=(0, 0, -1), v=(0, 1, 0))
                pts = [(-x, yy) for x, yy in out]
            else:
                pl = frame((-hx * 0.6, y, 0), u=(0, 0, 1), v=(0, 1, 0))
                pts = out
            m.plate(pts, hx * 0.3, pl, "armor2", chamfer=0.02)
        m.cyl((-hx * 0.98, y, 0), (hx * 0.98, y, 0), rb * 0.3, "chrome", 12)
        for sx in (-1, 1):
            m.ring_glow((sx * hx * 0.91, y, 0), X, rb * 0.33, rb * 0.48, 0.02)
    fl = end * 0.75
    fair = m.plate([(-r * 0.7, -fl), (0, -fl - r * 1.2), (r * 0.7, -fl), (r * 0.55, fl), (0, fl + r * 0.8),
                    (-r * 0.55, fl)], 0.07, frame((0, 0, r * 0.95)), "armor", chamfer=0.03)
    m.slit(fair, (0, -fl * 0.85, r * 0.95 + 0.07), (0, fl * 0.85, r * 0.95 + 0.07), Z, 0.025, 0.035)
    rr = max(0.03, r * 0.24)
    xr = r * 1.05 + rr * 1.4
    m.piston((xr, end * 0.15, 0), (xr, -end * 0.92, 0), rr, extension=0.4, hint=(0, 0, 1))
    m.conduit([(-r * 1.15, end * 0.85, 0), (-r * 1.15, -end * 0.85, 0)], max(0.03, r * 0.16))


@builder("bastion.carrier_front_left", "bastion.carrier_front_right", "bastion.carrier_back_left",
         "bastion.carrier_back_right")
def carrier(m, h, part):
    """The carrier on the wrist: a faceted dark body on the wrist hinge, with tapered
    octagonal arms out to the quarter's mounting pad and to the leg's deploy hinge, each
    ending in a flange that glows at its seam; ribbed conduits carry power to the leg and
    the quarter's engines."""
    hx, hy, hz = h
    ns = nodes(part)
    m.hull([(sx * hx * 0.7, sy * hy * 0.65, sz * hz * 0.7) for sx in (-1, 1) for sy in (-1, 1) for sz in (-1, 1)]
           + [(sx * hx * 0.85, 0, 0) for sx in (-1, 1)] + [(0, 0, sz * hz * 0.85) for sz in (-1, 1)], "dark",
           bevel=0.01)
    pb = ns["base"][0]
    m.drum((0, pb.y - 0.14, 0), (0, pb.y, 0), hx * 0.75, "metal", 8, glow=False, hint=X)
    for name in ("shell", "leg"):
        if name not in ns:
            continue
        p, d = ns[name]
        root = d * hx * 0.4
        m.cyl(root, p - d * 0.16, hx * 0.42, "armor2", 8, r2=hx * 0.3, bevel=0.01, segments=1)
        m.drum(p - d * 0.16, p, hx * 0.42, "metal", 8, hint=X if abs(d.x) < 0.9 else Y)
        if name == "shell":
            up = Y - d * Y.dot(d) if abs(d.y) < 0.95 else X
            m.plate(octagon(1.5, 1.2, 0.35), 0.1, frame(p - d * 0.1, w=d, v=up), "armor", chamfer=0.04)
            for s in (-1, 1):
                pad = frame(p - d * 0.1, w=d, v=up)
                m.hull([pad @ V((s * 0.45 + s * dx, 0, z)) for dx in (-0.02, 0.02) for z in (0, -0.02)]
                       + [pad @ V((s * 0.45, 0, -0.5)), pad @ V((s * 0.45, 0.45, -0.02)),
                          pad @ V((s * 0.45, -0.45, -0.02))], "armor2", bevel=0.004)
        m.conduit([V((hx * 0.6, 0, 0)), V((hx * 0.8, 0, 0)) + d * 0.3, p - d * 0.35 + V((0.18, 0, 0))], 0.04)


@builder("bastion.saddle_left", "bastion.saddle_right")
def saddle(m, h, part):
    """A shoulder saddle: a faceted graphite base on the shoulder block; for each shell arm
    a tapered octagonal pedestal with four blade gussets up to the arm's shoulder hinge,
    its flange glowing at the seam; a power junction between them feeding both arms through
    ribbed conduits."""
    hx, hy, hz = h
    ns = nodes(part)
    base = m.plate(octagon(2 * hx, 2 * hz, hx * 0.4), hy * 0.5, frame((0, -hy, 0), u=X, w=Y), "armor2",
                   chamfer=0.05)
    for z in (-hz * 0.35, hz * 0.35):
        m.slit(base, (-hx * 0.6, -hy * 0.5, z), (hx * 0.6, -hy * 0.5, z), Y, 0.025, 0.03)
    for name, (p, d) in ns.items():
        foot = V((p.x, -hy * 0.5, p.z))
        top = p - d * 0.12
        m.cyl(foot, top, 0.38, "armor", 8, r2=0.3, bevel=0.01, segments=1)
        m.drum(top, p, 0.5, "metal", 8, hint=X)
        for k in range(4):
            t = math.pi / 4 + math.pi / 2 * k
            u = V((math.cos(t), 0, math.sin(t)))
            w = u.cross(Y)
            m.hull([foot + u * 0.28 + w * s * 0.02 for s in (-1, 1)] + [foot + u * 0.7 + w * s * 0.02 for s in (-1, 1)]
                   + [foot + u * 0.28 + Y * 0.5], "armor2", bevel=0.003)
    mv = V((0, -hy * 0.2, 0))
    m.hull([mv + V((sx * hx * 0.4, sy * hy * 0.28, sz * 0.25)) for sx in (-1, 1) for sy in (-1, 1) for sz in (-1, 1)]
           + [mv + V((0, hy * 0.4, 0))], "dark", bevel=0.01)
    for name, (p, d) in ns.items():
        sz = 1 if p.z > 0 else -1
        m.conduit([mv + V((0, 0.05, sz * 0.25)), V((0, hy * 0.3, p.z * 0.6)), p + V((0.45, -0.3, 0))], 0.04)


@builder("bastion.grapple")
def grapple(m, h, part):
    """A three-claw grapple: the wrist flange, a faceted palm housing glowing at its seam, and
    three claws, each between two blade cheeks on the palm: a tapered proximal finger and a
    hooked, hardened talon, closed by its own sealed actuator."""
    hx, hy, hz = h
    m.lathe([(0, hy), (hx * 0.8, hy), (hx * 0.8, hy * 0.86), (hx * 0.6, hy * 0.8), (hx * 0.66, hy * 0.5),
             (hx * 0.8, hy * 0.3), (hx * 0.8, hy * 0.12), (0, hy * 0.12)], frame(), "dark", 8, angle=20.0)
    m.ring_glow((0, hy * 0.57, 0), Y, hx * 0.6, hx * 0.71, 0.04)
    for k in range(3):
        t = 2 * math.pi * k / 3
        rd = V((math.sin(t), 0, math.cos(t)))
        tg = Y.cross(rd)
        k0 = rd * hx * 0.62 + Y * hy * 0.08
        for s in (-1, 1):
            pl = frame(k0 + tg * (s * 0.09 - 0.02), u=rd, v=Y)
            m.hull([pl @ V((x, y, z)) for x, y in ((-0.08, hy * 0.2), (0.08, hy * 0.2), (0.08, 0.0), (0.0, -0.09),
                                                   (-0.08, 0.0)) for z in (0, 0.04)], "armor2", bevel=0.004)
        m.cyl(k0 - tg * 0.14, k0 + tg * 0.14, 0.035, "chrome", 8)
        k1 = k0 + rd * hx * 0.38 - Y * hy * 0.48
        fin = along(k0, k1, hint=tg)
        ln = (k1 - k0).length
        m.hull([fin @ V((x, y, z)) for y, w in ((0.0, 0.06), (ln, 0.045)) for x in (-w, w) for z in (-w, w)]
               + [fin @ V((-0.09, ln * 0.5, 0))], "armor", bevel=0.006)
        m.drum(k1 - tg * 0.08, k1 + tg * 0.08, 0.06, "dark", 8, glow=False, hint=rd)
        tip = frame(k1, u=rd, w=tg)
        out = [(-0.05, 0.0), (0.06, 0.0), (0.03, -0.25), (-0.06, -0.48), (-0.18, -0.6), (-0.13, -0.47),
               (-0.07, -0.25)]
        m.extrude(out, 0.09, tip, "metal", bevel=0.006, segments=1)
        m.piston(Y * hy * 0.3 + rd * hx * 0.25, k0.lerp(k1, 0.5) - rd * 0.06, 0.03, extension=0.35, hint=tg)
