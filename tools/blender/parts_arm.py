"""Shell arm and folding-leg hardware: knees, wrist rotor, leg segments, carrier, saddle,
grapple."""

import math

from mech import V, X, Y, Z, along, frame
from parts import builder, extent, nodes


@builder("bastion.fold_knee_left", "bastion.fold_knee_right")
def fold_knee(m, h, part):
    """A folding knee: a forged U-yoke whose cheeks wrap round the bend between the two
    side-by-side segments. The base segment sits in a bolted socket; the end segment turns
    on a knuckle at its root (the pivot, end node) driven by a ram along the yoke through a
    crank, fed from a small valve block."""
    hx, hy, hz = h
    ns = nodes(part)
    pb, pe = ns["base"][0], ns["end"][0]
    zc = hz * 0.78
    tc = max(0.04, hz * 0.16)
    # Cheeks: U outline round the bottom.
    out = []
    r = hx * 0.98
    for k in range(17):
        t = math.pi + math.pi * k / 16
        out.append((r * math.cos(t), -hy * 0.15 + hy * 0.9 * math.sin(t)))
    out += [(hx, hy * 0.5), (-hx, hy * 0.5)]
    out = [(x, y) for x, y in out]
    for s in (-1, 1):
        cheek = m.extrude(out, tc, frame((0, 0, s * zc)), "armor", bevel=0.015)
        m.cut_box(cheek, (hx * 0.9, hy * 0.5, tc * 0.6), at=(0, -hy * 0.45, s * (zc + tc / 2)))
    # Floor web between the cheeks.
    m.box((2 * hx * 0.9, 0.05, 2 * zc), at=(0, -hy * 0.75, 0), mat="armor2", bevel=0.01)
    # Base socket.
    m.box((hx * 0.85, hy * 0.6, 2 * zc - tc), at=(pb.x, pb.y - hy * 0.35, 0), mat="dark", bevel=0.02)
    m.box((hx * 0.95, 0.06, 2 * zc + tc), at=(pb.x, pb.y - 0.03, 0), mat="metal", bevel=0.01)
    for s in (-1, 1):
        m.bolt_row((pb.x - hx * 0.4, pb.y - 0.03, s * (zc + tc / 2)),
                   (pb.x + hx * 0.4, pb.y - 0.03, s * (zc + tc / 2)), (0, 0, s), 3, size=0.022)
    # Knuckle at the pivot, along X.
    rk = min(hz * 0.7, hy * 0.75)
    m.lathe([(0, pe.x - hx * 0.42), (rk * 0.9, pe.x - hx * 0.42), (rk, pe.x - hx * 0.38),
             (rk, pe.x + hx * 0.38), (rk * 0.9, pe.x + hx * 0.42), (0, pe.x + hx * 0.42)],
            frame(pe, u=(0, 1, 0), v=(1, 0, 0)), "dark", 32, bevel=0.006)
    m.cyl(pe - X * (hx * 0.5), pe + X * (hx * 0.5), rk * 0.3, "chrome", 20)
    for s in (-1, 1):
        c = pe + X * (s * hx * 0.5)
        m.cyl(c, c + X * (s * 0.03), rk * 0.5, "metal", 20, bevel=0.005)
        m.bolts(c + X * (s * 0.03), X * s, rk * 0.36, 6, size=0.016)
    # Crank ram along the yoke's front, valve block and hoses.
    rr = max(0.035, hz * 0.12)
    zr = -(zc + tc + rr * 1.2)
    m.ram((-hx * 0.8, -hy * 0.3, zr), (hx * 0.55, hy * 0.05, zr), rr, extension=0.4,
          hint=(0, 0, 1))
    vb = V((-hx * 0.45, -hy * 0.75, zc + tc + 0.05))
    m.box((hx * 0.5, hy * 0.35, 0.09), at=vb, mat="metal", bevel=0.008)
    m.hose([vb + V((0.05, 0.05, 0.04)), V((0, hy * 0.2, zc + tc + 0.12)),
            V((hx * 0.55, hy * 0.1, zr * 0.2)), V((hx * 0.6, hy * 0.05, zr + rr))], 0.018)
    m.box((hx * 1.6, 0.03, 0.01), at=(0, -hy * 0.75, -(zc + tc / 2) - 0.004), mat="trim",
          bevel=0.002)


@builder("bastion.arm_rotor")
def rotor(m, h, part):
    """A wrist rotor: a base flange bolted to the leg, the slewing bearing with its gear
    ring, two pinion motors driving it, and the turning face plate the grapple bolts to."""
    hx, hy, hz = h
    r = min(hx, hz)
    place = frame((0, 0, 0))
    m.lathe([(0, hy), (r * 0.95, hy), (r * 0.95, hy - 0.06), (r * 0.7, hy - 0.08),
             (r * 0.7, -hy * 0.1), (0, -hy * 0.1)], place, "dark", 40, bevel=0.006)
    m.bolts((0, hy, 0), Y, r * 0.82, 10, size=0.018)
    # Gear ring (teeth) and bearing race.
    m.lathe([(0, -hy * 0.05), (r * 0.9, -hy * 0.05), (r * 0.9, -hy * 0.35), (0, -hy * 0.35)],
            place, "metal", 48)
    for k in range(36):
        t = 2 * math.pi * k / 36
        d = V((math.sin(t), 0, math.cos(t)))
        m.box((0.03, hy * 0.26, 0.035), place=frame(d * r * 0.92 + V((0, -hy * 0.2, 0)), u=d.cross(Y), v=Y),
              mat="metal", bevel=0)
    for s in (-1, 1):
        c = V((s * r * 0.95, hy * 0.25, 0))
        m.cyl(c + V((0, -hy * 0.45, 0)), c + V((0, hy * 0.5, 0)), r * 0.22, "dark", 20, bevel=0.005)
        m.cyl(c + V((0, hy * 0.5, 0)), c + V((0, hy * 0.6, 0)), r * 0.16, "metal", 16)
        m.hose([c + V((0, hy * 0.6, 0)), c + V((-s * r * 0.3, hy * 0.85, 0.05)),
                V((0, hy * 1.05, r * 0.5))], 0.012)
    m.lathe([(0, -hy * 0.35), (r * 0.82, -hy * 0.35), (r * 0.82, -hy * 0.9), (r * 0.78, -hy),
             (0, -hy)], place, "armor2", 40, bevel=0.006)
    m.bolts((0, -hy, 0), -Y, r * 0.6, 8, size=0.02)


@builder("bastion.leg_1", "bastion.leg_2", "bastion.leg_3", "bastion.leg_4", "bastion.leg_5",
         "bastion.leg_6")
def spider(m, h, part):
    """A folding-leg segment: a turned, slightly tapered tube with stepped collars, a forked
    clevis at each end round its pin boss, an armoured fairing on the outer face, the fold
    ram on brackets, hydraulic and signal lines in clamps, and a marker lamp."""
    hx, hy, hz = h
    r = hx * 0.55
    rb = hx * 0.62
    end = hy - rb * 1.1
    place = frame((0, 0, 0))
    m.lathe([(r * 0.7, -end), (r * 1.05, -end), (r * 1.05, -end + 0.12), (r * 0.95, -end + 0.16),
             (r * 0.88, -end * 0.2), (r * 0.92, end * 0.2), (r * 0.98, end - 0.16),
             (r * 1.1, end - 0.12), (r * 1.1, end), (r * 0.7, end)], place, "armor2", 24, bevel=0.005)
    for y in (-end * 0.5, 0.0, end * 0.5):
        m.lathe([(r, y - 0.05), (r * 1.08, y - 0.03), (r * 1.08, y + 0.03), (r, y + 0.05)],
                place, "metal", 24, closed=True)
    for s in (-1, 1):
        y = s * (hy - rb)
        for sx in (-1, 1):
            m.lug((sx * hx * 0.7, s * end, 0), (sx * hx * 0.7, y, 0), rb * 1.9, hx * 0.22, X,
                  "armor")
        m.cyl((-hx * 0.98, y, 0), (hx * 0.98, y, 0), rb * 0.32, "chrome", 16)
        for sx in (-1, 1):
            m.cyl((sx * hx * 0.82, y, 0), (sx * hx * 0.98, y, 0), rb * 0.5, "metal", 16, bevel=0.004)
    # Fairing on the outer (+Z) face.
    fl = end * 0.75
    fair = m.extrude([(-r * 0.75, -fl), (r * 0.75, -fl), (r * 0.6, fl), (-r * 0.6, fl)], 0.05,
                     frame((0, 0, r * 1.08)), "armor", bevel=0.01)
    for y in (-fl * 0.35, fl * 0.35):
        m.groove(fair, (-r, y, r * 1.08 + 0.025), (r, y, r * 1.08 + 0.025), Z, 0.012, 0.01)
    m.box((r * 0.5, 0.12, 0.012), at=(0, fl * 0.7, r * 1.08 + 0.03), mat="trim", bevel=0.002)
    m.box((0.06, 0.04, 0.03), at=(0, fl * 0.92, r * 1.08 + 0.035), mat="light", bevel=0.005)
    # Fold ram on brackets along +X.
    rr = max(0.03, r * 0.24)
    xr = r * 1.05 + rr * 1.4
    ya, yb = end * 0.15, -end * 0.92
    m.ram((xr, ya, 0), (xr, yb, 0), rr, extension=0.4, hint=(0, 0, 1))
    for y in (ya, yb + rr):
        m.lug((r * 0.9, y, 0), (xr, y, 0), rr * 1.8, 0.03, Z, "metal")
    # Lines in clamps along -X.
    for dz in (-0.05, 0.05):
        m.hose([(-r * 1.2, end, dz), (-r * 1.22, 0, dz), (-r * 1.2, -end, dz)],
               max(0.014, r * 0.08))
    n = max(2, int(2 * end / 1.2))
    for k in range(n):
        y = -end * 0.85 + 1.7 * end * k / (n - 1)
        m.box((r * 0.3, 0.04, 0.2), at=(-r * 1.1, y, 0), mat="metal", bevel=0.005)


@builder("bastion.carrier_front_left", "bastion.carrier_front_right", "bastion.carrier_back_left",
         "bastion.carrier_back_right")
def carrier(m, h, part):
    """The carrier: a cast bracket on the wrist. A ribbed central body bolted to the wrist
    hinge; a tapered arm to the quarter's mounting pad (bolted all round); another to the
    leg's deploy hinge; the valve manifold that feeds the leg and the quarter's engines, with
    its hose bundle."""
    hx, hy, hz = h
    ns = nodes(part)
    m.box((hx * 1.4, hy * 1.3, hz * 1.4), mat="armor2", bevel=0.08, segments=3)
    for s in (-1, 1):
        m.box((0.05, hy * 1.2, hz * 1.1), at=(s * hx * 0.72, 0, 0), mat="armor", bevel=0.015)
    pb = ns["base"][0]
    m.lathe([(0, pb.y - 0.12), (hx * 0.75, pb.y - 0.12), (hx * 0.75, pb.y), (0, pb.y)],
            frame((0, 0, 0)), "metal", 40, bevel=0.006)
    m.bolts(pb, Y, hx * 0.62, 10, size=0.026)
    for name in ("shell", "leg"):
        if name not in ns:
            continue
        p, d = ns[name]
        root = d * hx * 0.4
        m.cyl(root, p - d * 0.14, hx * 0.42, "armor2", 8, r2=hx * 0.3, bevel=0.02)
        m.cyl(p - d * 0.14, p, hx * 0.42, "metal", 32, bevel=0.008)
        m.bolts(p, d, hx * 0.34, 8, size=0.024)
        if name == "shell":
            pad = frame(p - d * 0.07, w=d, v=Y - d * Y.dot(d) if abs(d.y) < 0.95 else X)
            m.box((1.5, 1.2, 0.1), place=pad, mat="armor", bevel=0.02)
            for x in (-0.65, -0.22, 0.22, 0.65):
                for y in (-0.5, 0.5):
                    m.bolt(pad @ V((x, y, 0.05)), d, 0.026)
            for s in (-1, 1):
                m.extrude([(0, 0), (0.55, 0), (0, 0.5)], 0.05,
                          pad @ frame((s * 0.5, 0, -0.05), u=(0, 0, -1), v=(0, 1, 0)), "armor2",
                          0.008, 1)
    # Manifold and hoses.
    mv = V((hx * 0.95, -hy * 0.15, 0))
    m.box((0.14, hy * 0.7, hz * 0.8), at=mv, mat="metal", bevel=0.012)
    for k, z in enumerate((-hz * 0.25, 0.0, hz * 0.25)):
        m.cyl(mv + V((0.07, 0, z)), mv + V((0.13, 0, z)), 0.028, "metal", 6)
        for name in ("shell", "leg"):
            if name in ns:
                p, d = ns[name]
                m.hose([mv + V((0.13, 0, z)), mv + V((0.3, 0.1 * k, z)) + d * 0.2,
                        p - d * 0.3 + V((0.2, 0.05 * k, z * 0.5))], 0.02)


@builder("bastion.saddle_left", "bastion.saddle_right")
def saddle(m, h, part):
    """A shoulder saddle: a machined base plate bolted round its edge to the shoulder block;
    a turned pedestal with four gussets up to each shell arm's shoulder hinge, its flange
    bolted; the manifold between them feeding both arms, with hard lines and hoses."""
    hx, hy, hz = h
    ns = nodes(part)
    base = m.box((2 * hx, hy * 0.5, 2 * hz), at=(0, -hy * 0.75, 0), mat="armor2", bevel=0.03)
    for z in (-hz * 0.35, hz * 0.35):
        m.groove(base, (-hx, -hy * 0.5, z), (hx, -hy * 0.5, z), Y, 0.015, 0.01)
    for sx in (-1, 1):
        m.bolt_row((sx * hx * 0.85, -hy * 0.5, -hz * 0.95), (sx * hx * 0.85, -hy * 0.5, hz * 0.95), Y,
                   8, size=0.03)
    for name, (p, d) in ns.items():
        foot = V((p.x, -hy * 0.5, p.z))
        top = p - d * 0.1
        m.cyl(foot, top, 0.36, "armor", 32, r2=0.3, bevel=0.015)
        m.cyl(top, p, 0.5, "metal", 40, bevel=0.01)
        m.bolts(p, d, 0.42, 10, size=0.028)
        for k in range(4):
            t = math.pi / 4 + math.pi / 2 * k
            u = V((math.cos(t), 0, math.sin(t)))
            m.extrude([(0, 0), (0.42, 0), (0, 0.5)], 0.04,
                      frame(foot + u * 0.28, u=u, v=Y), "armor2", 0.006, 1)
    mv = V((0, -hy * 0.2, 0))
    m.box((hx * 0.8, hy * 0.55, 0.5), at=mv, mat="metal", bevel=0.015)
    m.box((hx * 0.6, 0.04, 0.3), at=mv + V((0, hy * 0.29, 0)), mat="trim", bevel=0.005)
    for name, (p, d) in ns.items():
        sz = 1 if p.z > 0 else -1
        for dx in (-0.12, 0.12):
            start = mv + V((dx, 0.05, sz * 0.25))
            m.cyl(start, start + V((0, 0, sz * 0.06)), 0.03, "metal", 6)
            m.hose([start + V((0, 0, sz * 0.06)), V((dx * 2, hy * 0.6, p.z * 0.6)),
                    p + V((dx + 0.42, -0.25, 0))], 0.026)
    m.box((0.02, hy * 0.25, hz * 1.6), at=(-hx, -hy * 0.75, 0), mat="trim", bevel=0.004)


@builder("bastion.grapple")
def grapple(m, h, part):
    """A three-claw grapple: the wrist flange, a turned palm housing with the claw
    actuators' manifold, and three claws, each a pair of lugs on the palm, a ribbed proximal
    finger and a hooked, hardened tip, closed by its own ram."""
    hx, hy, hz = h
    place = frame((0, 0, 0))
    m.lathe([(0, hy), (hx * 0.8, hy), (hx * 0.8, hy * 0.86), (hx * 0.6, hy * 0.82),
             (hx * 0.66, hy * 0.5), (hx * 0.8, hy * 0.3), (hx * 0.8, hy * 0.12), (0, hy * 0.12)],
            place, "dark", 36, bevel=0.006)
    m.bolts((0, hy, 0), Y, hx * 0.68, 8, size=0.02)
    m.lathe([(hx * 0.67, hy * 0.62), (hx * 0.72, hy * 0.6), (hx * 0.72, hy * 0.54),
             (hx * 0.67, hy * 0.52)], place, "trim", 36, closed=True)
    for k in range(3):
        t = 2 * math.pi * k / 3
        rd = V((math.sin(t), 0, math.cos(t)))
        tg = Y.cross(rd)
        k0 = rd * hx * 0.62 + Y * hy * 0.08
        for s in (-1, 1):
            m.lug(k0 + tg * (s * 0.09) + Y * hy * 0.2, k0 + tg * (s * 0.09), 0.15, 0.04, tg, "metal")
        m.cyl(k0 - tg * 0.14, k0 + tg * 0.14, 0.035, "chrome", 12)
        k1 = k0 + rd * hx * 0.38 - Y * hy * 0.48
        fin = along(k0, k1, hint=tg)
        m.box((0.12, (k1 - k0).length, 0.11), place=fin @ frame((0, (k1 - k0).length / 2, 0)),
              mat="armor", bevel=0.015, taper=(0.85, 0.85))
        m.cyl(k1 - tg * 0.08, k1 + tg * 0.08, 0.06, "dark", 16)
        # Hooked tip: a curved outline in the claw's plane.
        tip = frame(k1, u=rd, w=tg)
        out = [(-0.05, 0.0), (0.06, 0.0), (0.03, -0.25), (-0.06, -0.48), (-0.16, -0.58),
               (-0.13, -0.47), (-0.07, -0.25)]
        m.extrude(out, 0.09, tip, "metal", bevel=0.008)
        m.ram(Y * hy * 0.3 + rd * hx * 0.25, k0.lerp(k1, 0.5) - rd * 0.06, 0.03, extension=0.35,
              hint=tg, ports=False)
