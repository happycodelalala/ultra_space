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
    """A fold knee, an edge hinge: a graphite bracket on the end of one folding-leg segment
    (the base node) reaching across to the seam between it and the next segment, where two
    outer knuckles carry the pin (square to the folded stack, along Z). The next segment's
    roll bearing turns on the pin between them: folded, it lies beside this segment; turned
    half a circle, it runs straight on in line."""
    ns = nodes(part)
    pb, pe = ns["base"][0], ns["end"][0]
    r = 0.16
    # Bracket: over the segment's end, out to the pin.
    m.hull([V((pb.x + dx, pb.y + dy, dz)) for dx in (-0.22, 0.22) for dy in (0.0, -0.12) for dz in (-0.3, 0.3)]
           + [V((pe.x, pe.y + dy, dz)) for dy in (-0.14, 0.0) for dz in (-0.34, 0.34)], "armor2", bevel=0.01)
    for s in (-1, 1):
        m.drum(pe + Z * (s * 0.14), pe + Z * (s * 0.34), r, "dark", 8, hint=X)
        m.cap(pe + Z * (s * 0.34), Z * s, r * 0.6)


@builder("bastion.leg_roll")
def leg_roll(m, h, part):
    """A roll bearing at a folding-leg segment's root: the middle knuckle on the fold knee's
    pin, an arm across to the segment's axis, and there the slewing bearing that turns the
    segment about its length."""
    ns = nodes(part)
    pb, pe = ns["base"][0], ns["end"][0]
    m.drum(pb - Z * 0.13, pb + Z * 0.13, 0.15, "metal", 8, hint=X)
    m.hull([V((pb.x, pb.y + dy, dz)) for dy in (-0.1, 0.1) for dz in (-0.12, 0.12)]
           + [V((pe.x + dx, pe.y + dy, dz)) for dx in (-0.2, 0.2) for dy in (-0.1, 0.1) for dz in (-0.2, 0.2)],
           "dark", bevel=0.008)
    m.drum(pe - Y * 0.14, pe + Y * 0.14, 0.27, "metal", 8, hint=X)
    m.lathe([(0.27, -0.03), (0.3, -0.03), (0.3, 0.03), (0.27, 0.03)], along(pe, pe + Y), "dark", 8, closed=True,
            angle=20.0)


@builder("bastion.arm_rotor")
def rotor(m, h, part):
    """A wrist rotor: a faceted flange on the leg, the sealed slewing drive, two pinion motors
    driving it, and the turning face plate the grapple mounts to."""
    hx, hy, hz = h
    r = min(hx, hz)
    m.plate(octagon(2 * r * 0.95, 2 * r * 0.95, r * 0.55), 0.08, frame((0, hy - 0.08, 0), u=X, w=Y), "armor2",
            chamfer=0.03)
    m.drum((0, hy - 0.08, 0), (0, -hy * 0.35, 0), r * 0.82, "dark", 8, hint=X)
    for s in (-1, 1):
        c = V((s * r * 0.95, 0, 0))
        m.drum(c + V((0, -hy * 0.4, 0)), c + V((0, hy * 0.6, 0)), r * 0.2, "metal", 6, hint=Z)
    m.lathe([(0, -hy * 0.35), (r * 0.82, -hy * 0.35), (r * 0.82, -hy * 0.85), (r * 0.7, -hy), (0, -hy)],
            frame(), "armor2", 8, angle=20.0)


@builder("bastion.leg_1", "bastion.leg_2", "bastion.leg_3", "bastion.leg_4", "bastion.leg_5",
         "bastion.leg_6")
def spider(m, h, part):
    """A folding-leg segment, a manipulator boom (design/bastion.md): a tapered tube in the
    second paint between the joint housings, clevis cheeks round the pins at each end, held by
    retainers; a flat armour strip on the outer face (outermost when folded against the
    shell); the fold actuator (rod under a dust boot) along one side, the utility harness
    feeding the next joints along the other in clamps. Every second segment carries an
    inspection camera and its light at its far end, looking along the boom at the grapple."""
    hx, hy, hz = h
    r = hx * 0.55
    rb = hx * 0.62
    end = hy - rb * 1.1
    m.lathe([(r * 0.92, -end), (r * 1.0, -end + 0.12), (r * 0.9, end - 0.12), (r * 0.82, end)], frame(),
            "armor2", 12, angle=35.0)
    for s_ in (-1, 1):
        y = s_ * (end - 0.05)
        m.cyl((0, y - 0.1, 0), (0, y + 0.1, 0), r * 1.08, "dark", 12, bevel=0.01, segments=1)
    for s_ in (-1, 1):
        y = s_ * (hy - rb)
        for sx in (-1, 1):
            inner = s_ * (end - rb * 0.8) - y
            out = [(-rb * 0.7, inner), (rb * 0.7, inner), (rb * 0.95, 0), (rb * 0.4, s_ * rb),
                   (-rb * 0.4, s_ * rb), (-rb * 0.95, 0)]
            if sx > 0:
                pl = frame((hx * 0.6, y, 0), u=(0, 0, -1), v=(0, 1, 0))
                pts = [(-x, yy) for x, yy in out]
            else:
                pl = frame((-hx * 0.6, y, 0), u=(0, 0, 1), v=(0, 1, 0))
                pts = out
            m.plate(pts, hx * 0.3, pl, "dark", chamfer=0.02)
        m.cyl((-hx * 0.98, y, 0), (hx * 0.98, y, 0), rb * 0.3, "chrome", 12)
        for sx in (-1, 1):
            m.cap((sx * hx * 0.9, y, 0), X * sx, rb * 0.45)
    fl = end * 0.8
    m.box((r * 1.1, 2 * fl, 0.06), at=(0, 0, r * 0.98), mat="armor", bevel=0.02, segments=2)
    rr = max(0.03, r * 0.24)
    xr = r * 1.05 + rr * 1.4
    m.ram((xr, end * 0.15, 0), (xr, -end * 0.92, 0), rr, extension=0.4, hint=(0, 0, 1))
    m.cable([(-r * 1.15, end * 0.85, 0), (-r * 1.15, -end * 0.85, 0)], max(0.03, r * 0.16))
    for y in (end * 0.5, 0.0, -end * 0.5):
        m.clamp(V((-r * 1.15, y, 0)), Y, -X, max(0.03, r * 0.16))
    if part["id"] in ("bastion.leg_2", "bastion.leg_4"):
        # Inspection camera and its light near the far (-Y) end, looking down the boom.
        c = V((0, -end * 0.8, -r * 1.05))
        m.box((r * 0.7, r * 0.5, r * 0.5), at=c - Z * r * 0.2, mat="dark", bevel=0.01, segments=1)
        m.cyl(c - Y * r * 0.25, c - Y * r * 0.55, r * 0.16, "glass", 12)
        m.box((r * 0.35, r * 0.1, r * 0.28), at=c + X * r * 0.45 - Y * r * 0.3, mat="light", bevel=0.0)


@builder("bastion.carrier_front_left", "bastion.carrier_front_right", "bastion.carrier_back_left",
         "bastion.carrier_back_right")
def carrier(m, h, part):
    """The carrier on the wrist: a faceted dark body on the wrist hinge, with tapered
    octagonal arms out to the quarter's mounting pad (with gussets) and to the leg's deploy
    hinge, each ending in a flange; armoured cables carry power to the leg and the quarter's
    engines."""
    hx, hy, hz = h
    ns = nodes(part)
    m.hull([(sx * hx * 0.7, sy * hy * 0.65, sz * hz * 0.7) for sx in (-1, 1) for sy in (-1, 1) for sz in (-1, 1)]
           + [(sx * hx * 0.85, 0, 0) for sx in (-1, 1)] + [(0, 0, sz * hz * 0.85) for sz in (-1, 1)], "dark",
           bevel=0.01)
    pb = ns["base"][0]
    m.drum((0, pb.y - 0.14, 0), (0, pb.y, 0), hx * 0.75, "metal", 8, hint=X)
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
        m.cable([V((hx * 0.6, 0, 0)), V((hx * 0.8, 0, 0)) + d * 0.3, p - d * 0.35 + V((0.18, 0, 0))], 0.04)


def socket_section(p, d, along_, half_a, half_w, k):
    """An elongated octagon round `p` in the plane across `d`: `half_a` along `along_`,
    `half_w` across, corners cut by `k`."""
    a = (along_ - d * along_.dot(d)).normalized()
    w = d.cross(a).normalized()
    pts = [(-half_a + k, -half_w), (half_a - k, -half_w), (half_a, -half_w + k), (half_a, half_w - k),
           (half_a - k, half_w), (-half_a + k, half_w), (-half_a, half_w - k), (-half_a, -half_w + k)]
    return [p + a * x + w * y for x, y in pts]


@builder("bastion.saddle_left", "bastion.saddle_right")
def saddle(m, h, part):
    """A shoulder housing: a heavy faceted body on the shoulder block, and for each shell arm a
    socket tower rising from it, its root hinge sunk in the tower's cavity (the hinge's pin
    just above the rim, so the arm swings out of it) behind a reinforcing collar, the tower
    braced into the body by gussets; between the towers, armour over the body and the power
    junction that feeds both arms."""
    hx, hy, hz = h
    ns = nodes(part)
    sx = 1.0 if part["id"].endswith("_left") else -1.0
    # Body.
    top = -hy * 0.1
    body = m.hull([(cx * hx * fx, y, cz * hz * fz) for cx in (-1, 1) for cz in (-1, 1)
                   for y, fx, fz in ((-hy, 1.0, 0.98), (top, 0.9, 0.9))], "dark", bevel=0.02)
    cap = m.hull([(cx * hx * 0.8, y, cz * hz * 0.45) for cx in (-1, 1) for cz in (-1, 1)
                  for y in (top - 0.05, top + 0.12)], "armor", bevel=0.01)
    for name, (p, d) in ns.items():
        # The hinge turns about the curve's tangent at its quarter's middle.
        middle = 135.0 if name == "front" else 45.0
        t = math.radians(middle)
        tangent = V((math.cos(t) * sx, 0, math.sin(t)))
        rim = p + d * 0.42
        foot = V((rim.x, -hy, rim.z))
        # The root hinge reaches 0.84 m along its axis (its actuator) and 0.5 m across.
        outer = socket_section(rim, d, tangent, 1.08, 0.72, 0.28)
        base = [V((q.x, -hy, q.z)) for q in socket_section(foot, d, tangent, 1.12, 0.76, 0.28)]
        tower = m.hull(outer + base, "armor", bevel=0.012)

        def cavity(grow):
            return m.hull(socket_section(p - d * (0.25 + 2 * grow), d, tangent, 0.93 + grow, 0.58 + grow, 0.22)
                          + socket_section(p + d * 1.2, d, tangent, 0.93 + grow, 0.58 + grow, 0.22),
                          cutter=True)
        m.cut(tower, cavity(0.0))
        # The body and cap are cut wider and deeper, so their cut faces stay hidden behind the
        # tower's (no two surfaces in the same place).
        wide = cavity(0.06)
        for ob in (body, cap):
            m.cut(ob, wide)
        # Collar on the rim (above the tower's top, no faces shared with it).
        collar = m.hull(socket_section(rim + d * 0.005, d, tangent, 1.13, 0.77, 0.3)
                        + socket_section(rim + d * 0.09, d, tangent, 1.1, 0.74, 0.3), "metal", bevel=0.006)
        m.cut(collar, cavity(-0.02))
        # Gussets from the tower's sides down into the body.
        a = (tangent - d * tangent.dot(d)).normalized()
        w = d.cross(a).normalized()
        for s in (-1, 1):
            q = rim - d * 0.15 + w * s * 0.74
            down = V((q.x + w.x * s * 0.35, -hy * 0.6, q.z + w.z * s * 0.35))
            m.hull([q + a * e for e in (-0.06, 0.06)] + [down + a * e for e in (-0.08, 0.08)]
                   + [V((q.x, -hy * 0.6, q.z)) + a * e for e in (-0.08, 0.08)], "armor2", bevel=0.004)
    m.cable([V((0, top + 0.15, -hz * 0.3)), V((0, top + 0.3, 0)), V((0, top + 0.15, hz * 0.3))], 0.06)


@builder("bastion.forearm_front", "bastion.forearm_back")
def sleeve(m, h, part):
    """A telescoping forearm's outer sleeve: an octagonal tube, its mouth at the bottom
    where the slide collar sits and the inner beam runs out; the elbow's pin boss at the
    top; armour plates over the front and the sides."""
    hx, hy, hz = h
    R = min(hz * 0.62, hy * 0.25)
    end = hy - R
    rt = hx * 0.55
    m.lathe([(rt * 0.75, -hy), (rt, -hy), (rt, end), (rt * 0.75, end)], frame(), "dark", 8, closed=True,
            angle=20.0)
    m.drum((-hx * 0.98, end, 0), (hx * 0.98, end, 0), R, "dark", 8, hint=(0, 0, 1))
    for sx in (-1, 1):
        m.cap((sx * hx * 0.98, end, 0), X * sx, R * 0.55)
    m.lathe([(rt * 0.75, -hy), (rt * 1.15, -hy), (rt * 1.15, -hy + 0.12), (rt * 0.75, -hy + 0.12)], frame(),
            "metal", 8, closed=True, angle=20.0)
    span = end - R * 1.2 + hy - 0.15
    yc = (end - R * 1.2 - hy + 0.15) / 2
    tip = span / 2
    out = [(-hx * 0.86, -tip + 0.3), (0, -tip), (hx * 0.86, -tip + 0.3), (hx * 0.86, tip - 0.3), (0, tip),
           (-hx * 0.86, tip - 0.3)]
    zf = -rt
    m.plate(out, 0.14, frame((0, yc, zf), u=(-1, 0, 0), w=(0, 0, -1)), "armor", chamfer=0.05)
    m.hull([(sx * 0.06, yc + y, zf - 0.14) for sx in (-1, 1) for y in (-tip * 0.8, tip * 0.8)]
           + [(0, yc + y, zf - 0.28) for y in (-tip * 0.65, tip * 0.65)], "armor2", bevel=0.004)
    for sx in (-1, 1):
        side = [(-hz * 0.5, yc - tip * 0.9), (hz * 0.45, yc - tip * 0.8), (hz * 0.45, yc + tip * 0.8),
                (-hz * 0.5, yc + tip * 0.9)]
        pl = frame((sx * rt * 0.95, 0, 0), u=(0, 0, -sx), v=(0, 1, 0))
        m.plate([(-z, y) for z, y in side] if sx > 0 else side, hx * 0.25, pl, "armor2", chamfer=0.03)


@builder("bastion.arm_slide")
def arm_slide(m, h, part):
    """The slide collar on a forearm sleeve's mouth: an octagonal collar round the inner beam,
    with two pinion motors either side that drive the beam's rack, and wiper seals at the
    mouth."""
    hx, hy, hz = h
    r = min(hx, hz)
    m.lathe([(0.38, -hy), (r * 0.82, -hy), (r * 0.95, -hy * 0.6), (r * 0.95, hy * 0.6), (r * 0.82, hy),
             (0.38, hy)], frame(), "metal", 8, closed=True, angle=20.0)
    m.lathe([(0.36, -hy - 0.04), (0.44, -hy - 0.04), (0.44, -hy), (0.36, -hy)], frame(), "rubber", 8,
            closed=True, angle=20.0)
    for sx in (-1, 1):
        m.drum((sx * r * 0.95, -hy * 0.6, 0), (sx * (r * 0.95 + 0.25), -hy * 0.6, 0), 0.22, "dark", 8,
               hint=(0, 1, 0))
        m.drum((sx * (r * 0.95 + 0.25), -hy * 0.6, 0), (sx * (r * 0.95 + 0.3), -hy * 0.6, 0), 0.15, "metal",
               6, hint=(0, 1, 0))


@builder("bastion.forearm_beam")
def forearm_beam(m, h, part):
    """A telescoping forearm's inner beam: a hexagonal beam with a stop collar at its top
    (it can't run out of the sleeve), a rack down each side for the collar's pinions, and
    the wrist's mount at its bottom end."""
    hx, hy, hz = h
    r = hx * 0.9
    m.lathe([(r, -hy + 0.3), (r, hy - 0.1), (r * 0.8, hy), (0, hy)], frame(), "metal", 6, angle=20.0)
    m.lathe([(r, hy - 0.4), (r * 1.2, hy - 0.35), (r * 1.2, hy - 0.15), (r, hy - 0.1)], frame(), "dark", 6,
            closed=True, angle=20.0)
    for sx in (-1, 1):
        m.box((0.06, 2 * hy - 0.9, 0.16), at=(sx * r * 0.88, -0.1, 0), mat="dark", bevel=0.01, segments=1)
    m.hull([(sx * hx * 1.05, y, sz * hz * 1.05) for sx in (-1, 1) for sz in (-1, 1) for y in (-hy, -hy + 0.35)]
           + [(sx * r, -hy + 0.55, sz * r) for sx in (-1, 1) for sz in (-1, 1)], "armor2", bevel=0.01)


@builder("bastion.grapple")
def grapple(m, h, part):
    """A three-claw grapple: the wrist flange, a faceted palm housing with the wrist camera and
    its light looking down between the claws, and three claws, each between two blade cheeks on the palm: a tapered proximal finger and a
    hooked, hardened talon, closed by its own sealed actuator."""
    hx, hy, hz = h
    m.lathe([(0, hy), (hx * 0.8, hy), (hx * 0.8, hy * 0.86), (hx * 0.6, hy * 0.8), (hx * 0.66, hy * 0.5),
             (hx * 0.8, hy * 0.3), (hx * 0.8, hy * 0.12), (0, hy * 0.12)], frame(), "dark", 8, angle=20.0)
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
        m.drum(k1 - tg * 0.08, k1 + tg * 0.08, 0.06, "dark", 8, hint=rd)
        tip = frame(k1, u=rd, w=tg)
        out = [(-0.05, 0.0), (0.06, 0.0), (0.03, -0.25), (-0.06, -0.48), (-0.18, -0.6), (-0.13, -0.47),
               (-0.07, -0.25)]
        m.extrude(out, 0.09, tip, "metal", bevel=0.006, segments=1)
        m.piston(Y * hy * 0.3 + rd * hx * 0.25, k0.lerp(k1, 0.5) - rd * 0.06, 0.03, extension=0.35, hint=tg)
    # Wrist camera and its light between two claws, looking down between them.
    for k, mat in ((0.5, "glass"), (1.5, "light")):
        t = 2 * math.pi * k / 3
        rd = V((math.sin(t), 0, math.cos(t)))
        c = rd * hx * 0.7 + Y * hy * 0.45
        m.box((0.12, 0.1, 0.12), place=frame(c, u=Y.cross(rd), v=Y), mat="dark", bevel=0.01, segments=1)
        m.cyl(c - Y * 0.05, c - Y * 0.1, 0.04, mat, 10)
