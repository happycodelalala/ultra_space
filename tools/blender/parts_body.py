"""The walking body: torso and cockpit, head, pelvis, shoulders, limbs, feet, hands, and the
heavy frame's extras. Front is -Z, up +Y."""

import math

from mech import V, X, Y, Z, along, frame
from parts import builder, nodes


def in_plane(n, d):
    """`d` projected onto the plane with normal `n`, normalized."""
    d = V(d)
    return (d - n * d.dot(n)).normalized()


def ridge(m, p, q, out, base=0.08, height=0.22, mat="trim", trim=0.1):
    """A blade ridge (a keel) along `p`-`q`, standing `height` out along `out`."""
    p, q, out = V(p), V(q), V(out).normalized()
    d = (q - p).normalized()
    side = d.cross(out).normalized()
    p, q = p + d * trim, q - d * trim
    m.hull([x + side * s * base for x in (p, q) for s in (-1, 1)]
           + [p + out * height + d * height * 0.5, q + out * height - d * height * 0.3], mat, bevel=0.004)


@builder("bastion.torso", "frame.torso")
def torso(m, h, part):
    """The torso and cockpit. A faceted chest of dark frame metal under thick ceramic plates
    that overhang it: the two upper plates either side of the keel are the crew hatch (they
    part along it; the crew sees through the head's sensors, so there are no windows); the
    keel deflects hits off the cockpit. Graphite flank plates carry heat vents, and a high
    collar guards the neck joint. Chevron lames layer down the abdomen, flexed by a sealed
    actuator each side. On the back, the reactor housing vents through finned louvres between
    two radiator fins."""
    hx, hy, hz = h

    def P(x, y, z):
        return V((x * hx, y * hy, z * hz))

    O = P(0, 0.3, 0)
    # Core: tapering from the shoulders to the waist.
    m.hull([P(sx * wx, y, sz * wz) for sx in (-1, 1) for sz in (-1, 1)
            for y, wx, wz in ((0.9, 0.8, 0.72), (0.1, 0.74, 0.74), (-0.94, 0.42, 0.6))], "dark", bevel=0.02)
    Kt, Km, Kl = P(0, 0.96, -0.8), P(0, 0.5, -0.97), P(0, 0.02, -0.88)
    St = {s: P(s * 0.84, 0.96, -0.64) for s in (-1, 1)}
    Sm = {s: P(s * 0.96, 0.48, -0.76) for s in (-1, 1)}
    Sl = {s: P(s * 0.8, 0.0, -0.72) for s in (-1, 1)}
    Bt = {s: P(s * 0.9, 0.96, 0.6) for s in (-1, 1)}
    Bl = {s: P(s * 0.88, -0.04, 0.6) for s in (-1, 1)}
    m.hull([Kt, Km, Kl] + [d[s] for d in (St, Sm, Sl, Bt, Bl) for s in (-1, 1)], "dark", bevel=0.02)
    for s in (-1, 1):
        # Hatch plates and the plates below them.
        m.facet([Kt, St[s], Sm[s], Km], 0.26, inset=0.05, outward=O)
        m.facet([Km, Sm[s], Sl[s], Kl], 0.22, inset=0.07, outward=O)
        # Top plates.
        m.facet([Kt, St[s], Bt[s], P(0, 0.96, 0.6)], 0.1, inset=0.1, outward=O, mat="armor2")
        # Flank plate with heat vents, below the shoulder bearing.
        top_f, top_b = P(s * 0.95, 0.28, -0.72), P(s * 0.9, 0.28, 0.6)
        plate, c, n = m.facet([top_f, Sl[s], Bl[s], top_b], 0.2, inset=0.06, outward=O, mat="armor2")
        m.vent(plate, c, n, Y, hz * 0.7, hy * 0.18, fins=4, depth=0.12)
        # High collar.
        m.hull([P(s * x, y, z) for x, y, z in ((0.3, 0.97, -0.6), (0.62, 0.97, -0.5), (0.3, 0.97, -0.3),
                                                (0.62, 0.97, -0.2), (0.34, 1.08, -0.48), (0.56, 1.06, -0.3))],
               "armor", bevel=0.008)
    # Chest flares: armour sweeping out and up at the shoulders (they cover the shoulder
    # bearings from the front) and the torso narrowing to the waist below them: a V.
    for s in (-1, 1):
        m.hull([P(s * x, y, z) for x, y, z in ((0.8, 0.98, -0.68), (1.07, 1.04, -0.56), (1.04, 0.62, -0.8),
                                                (0.88, 0.42, -0.84), (0.8, 0.98, -0.3), (1.05, 1.02, -0.3),
                                                (1.0, 0.55, -0.45))], "armor", bevel=0.01)
    ridge(m, Kt, Km, (0, 0.35, -1), 0.1, 0.24)
    ridge(m, Km, Kl, (0, -0.3, -1), 0.1, 0.2)
    # Abdomen lames: chevrons, each over the next.
    Oa = P(0, -0.5, 0)
    for k in range(3):
        y0 = -0.06 - k * 0.27
        w = 0.56 - 0.07 * k
        z = -0.8 + 0.05 * k
        pts = [P(-w, y0, z), P(w, y0, z), P(w * 0.92, y0 - 0.2, z + 0.02), P(0, y0 - 0.34, z - 0.02),
               P(-w * 0.92, y0 - 0.2, z + 0.02)]
        m.facet(pts, 0.13, inset=0.0, outward=Oa, mat="armor" if k != 1 else "armor2")
    for s in (-1, 1):
        m.piston(P(s * 0.52, 0.0, -0.66), P(s * 0.46, -0.84, -0.58), hx * 0.05, extension=0.35, hint=(0, 0, 1))
    # Shoulder bearings.
    for s in (-1, 1):
        m.drum(P(s * 0.8, 0.57, 0), P(s * 1.0, 0.57, 0), hy * 0.3, "dark", 8, hint=(0, 1, 0))
    # Reactor housing with its vent, radiator fins.
    reactor = m.hull([P(sx * wx, y, z) for sx in (-1, 1) for y in (-0.62, 0.72)
                      for wx, z in ((0.48, 0.6), (0.32, 0.98))]
                     + [P(0, -0.8, 0.8), P(0, 0.86, 0.8)], "armor2", bevel=0.01)
    m.vent(reactor, P(0, 0.05, 0.98), Z, Y, hx * 0.5, hy * 0.9, fins=8, depth=0.14)
    for s in (-1, 1):
        fin = [(0.55 * hz, 0.05 * hy), (0.55 * hz, 0.92 * hy), (1.06 * hz, 1.04 * hy), (0.86 * hz, 0.12 * hy)]
        pl = frame((s * 0.6 * hx, 0, 0), u=(0, 0, -s), v=(0, 1, 0))
        m.plate([(-z, y) for z, y in fin] if s > 0 else fin, 0.07, pl, "armor", chamfer=0.03)
    # Neck collar.
    m.lathe([(hx * 0.36, hy * 0.95), (hx * 0.46, hy * 0.95), (hx * 0.46, hy * 1.0), (hx * 0.42, hy * 1.05),
             (hx * 0.36, hy * 1.05)], frame(), "dark", 8, closed=True, angle=20.0)


@builder("bastion.head", "frame.head")
def head(m, h, part):
    """The sensor head on its neck bearing: a pointed ceramic helm over a dark faceted face;
    the visor (the main wide-angle optic) lit in the gap between them and a three-lens
    rangefinder array in the face; a blade antenna on the crest and two swept antennas."""
    hx, hy, hz = h
    k = 1.18  # the head stands larger than its collision box, grown up from the neck

    def P(x, y, z):
        return V((x * hx * k, (-1 + (y + 1) * k) * hy, z * hz * k))

    m.drum(P(0, -1.0, 0), P(0, -0.7, 0), hx * 0.5, "dark", 8, hint=(1, 0, 0))
    m.hull([P(s * 0.6, 0.62, 0.65) for s in (-1, 1)] + [P(s * 0.42, 0.5, -0.5) for s in (-1, 1)]
           + [P(0, 0.7, 0.15), P(0, 0.34, -1.0)] + [P(s * 0.88, 0.02, -0.05) for s in (-1, 1)]
           + [P(s * 0.72, 0.0, 0.78) for s in (-1, 1)] + [P(s * 0.34, 0.02, -0.86) for s in (-1, 1)],
           "armor", bevel=0.01)
    m.hull([P(s * 0.32, -0.06, -0.8) for s in (-1, 1)] + [P(s * 0.32, -0.56, -0.8) for s in (-1, 1)]
           + [P(s * 0.8, -0.06, -0.05) for s in (-1, 1)] + [P(s * 0.6, -0.64, 0.1) for s in (-1, 1)]
           + [P(s * 0.74, -0.06, 0.7) for s in (-1, 1)] + [P(s * 0.52, -0.72, 0.6) for s in (-1, 1)],
           "dark", bevel=0.01)
    m.hull([P(s * x * 0.985, y, z * 0.985) for s in (-1, 1) for y in (-0.07, 0.03)
            for x, z in ((0.33, -0.83), (0.84, -0.05), (0.7, 0.6))], "light", bevel=0.0)
    m.sensor(P(0, -0.3, -0.8), -Z, Y, 3, r=hx * k * 0.075)
    crest = [P(0, y, z) for y, z in ((0.5, -0.45), (0.68, 0.25), (0.96, 0.98), (0.58, 0.6))]
    m.plate([(-c.z, c.y) for c in crest], 0.07, frame((-0.035, 0, 0), u=(0, 0, -1), v=(0, 1, 0)), "armor2",
            chamfer=0.025)
    for s in (-1, 1):
        ear = [(c.z, c.y) for c in (P(0, y, z) for y, z in ((0.12, -0.15), (0.24, 0.3), (0.55, 1.02),
                                                               (-0.12, 0.4)))]
        pl = frame((s * 0.8 * hx * k, 0, 0), u=(0, 0, -s), v=(0, 1, 0))
        m.plate([(-z, y) for z, y in ear] if s > 0 else ear, 0.05, pl, "dark", chamfer=0.02)


@builder("bastion.waist", "frame.waist")
def waist(m, h, part):
    """The pelvis: the sealed slewing drive the torso turns on; a faceted pelvis frame; both
    hip actuators with their pin retainers; thick pointed skirt armour front and back over
    the hip joints, and tassets at the sides over the hip actuators."""
    hx, hy, hz = h

    def P(x, y, z):
        return V((x * hx, y * hy, z * hz))

    rr = min(hx, hz) * 0.82
    m.drum(P(0, 0.5, 0), P(0, 1.0, 0), rr, "dark", 12, hint=X)
    m.hull([P(sx * wx, y, sz * wz) for sx in (-1, 1) for sz in (-1, 1)
            for y, wx, wz in ((0.55, 0.62, 0.72), (-0.9, 0.42, 0.55))]
           + [P(0, -0.3, -0.85), P(0, -0.3, 0.85)], "dark", bevel=0.02)
    for sx in (-1, 1):
        m.drum(P(sx * 0.4, -0.45, 0), P(sx * 0.94, -0.45, 0), hy * 0.55, "metal", 8, hint=Y)
        m.cap(P(sx * 0.94, -0.45, 0), X * sx, hy * 0.3)
    for sz in (-1, 1):
        y0 = -hy * 0.1
        out = [(-hx * 0.42, 0), (hx * 0.42, 0), (hx * 0.3, -hy * 0.7), (0, -hy * 1.05), (-hx * 0.3, -hy * 0.7)]
        pl = frame((0, y0, sz * hz * 0.96), u=(sz, 0, 0), v=(0, 1, sz * 0.25))
        m.plate(out, 0.18, pl, "armor", chamfer=0.06)
        m.hull([pl @ V((sx * 0.06, y, 0.18)) for sx in (-1, 1) for y in (-hy * 0.05, -hy * 0.85)]
               + [pl @ V((0, y, 0.3)) for y in (-hy * 0.1, -hy * 0.75)], "armor2", bevel=0.004)
    for sx in (-1, 1):
        out = [(-hz * 0.5, 0), (hz * 0.5, 0), (hz * 0.3, -hy * 0.6), (-hz * 0.05, -hy * 0.95), (-hz * 0.4, -hy * 0.5)]
        pl = frame((sx * hx * 0.97, hy * 0.25, 0), u=(0, 0, -sx), v=(0, 1, 0))
        m.plate([(-z, y) for z, y in out] if sx > 0 else out, 0.14, pl, "armor2", chamfer=0.04)


@builder("bastion.shoulder", "frame.shoulder")
def shoulder(m, h, part):
    """A shoulder block: the bearing that turns it on the torso; a bulky pauldron of stepped,
    faceted lames over the top (they shed hits from above), swept to points fore and aft,
    a keel along the top lame; the arm's swing actuator below."""
    hx, hy, hz = h
    m.drum((-hx, 0, 0), (hx, 0, 0), hy * 0.6, "dark", 8, hint=Y)
    m.hull([(sx * hx * 0.7, sy * hy * 0.7, sz * hz * 0.72) for sx in (-1, 1) for sy in (-1, 1) for sz in (-1, 1)],
           "dark", bevel=0.02)
    top = None
    for k, (r0, y0) in enumerate(((hx * 1.05, hy * 0.05), (hx * 0.9, hy * 0.3), (hx * 0.74, hy * 0.52))):
        zz = hz * (1.0 - 0.07 * k)
        ys = (y0 - 0.12, y0 + 0.36 * r0)
        pts = []
        for z in (-zz, zz):
            pts += [(-r0, y0 - 0.12, z), (r0, y0 - 0.12, z), (-r0 * 0.55, ys[1], z), (r0 * 0.55, ys[1], z)]
        for sz in (-1, 1):
            tip = hz * (0.15 + 0.12 * k)
            pts += [(-r0 * 0.3, ys[1] - 0.05, sz * (zz + tip)), (r0 * 0.3, ys[1] - 0.05, sz * (zz + tip))]
        top = m.hull(pts, "armor" if k != 1 else "armor2", bevel=0.01), ys[1], r0, zz
    lame, yt, r0, zz = top
    m.hull([(sx * 0.08, yt, sz * zz * 0.9) for sx in (-1, 1) for sz in (-1, 1)]
           + [(0, yt + 0.22, sz * zz * 0.75) for sz in (-1, 1)], "armor2", bevel=0.004)
    m.drum((0, -hy * 0.4, 0), (0, -hy, 0), hx * 0.5, "dark", 8, hint=X)


def limb_body(m, h, kind):
    """An arm or leg segment (`kind`: upper_arm, forearm, thigh or shin); the joint above at
    +Y, the one below at -Y, front -Z."""
    hx, hy, hz = h
    yb = hy * 0.84
    rb = min(hx, hz) * 0.5
    # Armour width (wider than the frame it covers), at the top and at the bottom.
    wt, wb = {"upper_arm": (1.0, 0.95), "forearm": (1.05, 1.1), "thigh": (1.04, 1.0),
              "shin": (1.0, 1.12)}[kind]

    def F(x, y, z):
        w = wb + (wt - wb) * (y + 1) / 2
        return V((x * hx * w, y * hy, z * hz))

    # Spine.
    cx, cz = hx * 0.42, hz * 0.5
    sec = [(cx, 0), (cx * 0.6, cz), (-cx * 0.6, cz), (-cx, 0), (-cx * 0.6, -cz), (cx * 0.6, -cz)]
    m.hull([(x * (1.0 if y > 0 else 0.8), y, z) for x, z in sec for y in (-yb, yb)], "dark", bevel=0.01)
    # Pin bosses: the joint's hub between two faceted cheeks.
    for s in (-1, 1):
        y = s * yb
        m.drum((-hx * 0.45, y, 0), (hx * 0.45, y, 0), rb, "metal", 8, hint=(0, 0, 1))
        for sx in (-1, 1):
            m.drum((sx * hx * 0.47, y, 0), (sx * hx * 0.8, y, 0), rb * 0.86, "dark", 8, hint=(0, 0, 1))
            m.cap((sx * hx * 0.8, y, 0), X * sx, rb * 0.5)
    # Front armour: a keeled graphite base with a thick ceramic plate either side of the keel.
    top_y = 0.3 if kind == "shin" else 0.6
    top_k, bot_k = F(0, top_y, -0.98), F(0, -0.46, -1.02)
    top_s = {s: F(s * 0.74, top_y + 0.04, -0.78) for s in (-1, 1)}
    bot_s = {s: F(s * 0.62, -0.5, -0.84) for s in (-1, 1)}
    m.hull([top_k, bot_k] + [d[s] for d in (top_s, bot_s) for s in (-1, 1)]
           + [F(s * w, y, -0.42) for s in (-1, 1) for y, w in ((top_y - 0.04, 0.9), (-0.5, 0.75))], "armor2",
           bevel=0.01)
    O = F(0, 0, 0)
    for s in (-1, 1):
        m.facet([top_k, top_s[s], bot_s[s], bot_k], 0.18, inset=0.05, outward=O)
    ridge(m, top_k, bot_k, (0, 0, -1), 0.07, 0.18)
    # Side plates, following the armour's width.
    thick = {"upper_arm": 0.3, "forearm": 0.42, "thigh": 0.4, "shin": 0.36}[kind]
    for s in (-1, 1):
        x0 = hx * 0.5
        side = [(-0.55 * hz, 0.6 * hy), (0.5 * hz, 0.5 * hy), (0.55 * hz, -0.5 * hy), (-0.55 * hz, -0.6 * hy)]
        # The plate's outer face leans out toward the wider end.
        pl = frame((s * x0, 0, 0), u=(0, 0, -s), v=(s * (wb - wt) * hx * 0.25, hy, 0))
        m.plate([(-z, y) for z, y in side] if s > 0 else side, hx * thick, pl, "armor2", chamfer=0.05)
    # Back: two sealed actuators (they work the joint below) either side of their cable.
    rr = min(hx, hz) * 0.13
    zr = cz + rr * 1.3
    for sx in (-1, 1):
        m.piston((sx * hx * 0.3, hy * 0.62, zr), (sx * hx * 0.3, -hy * 0.72, zr), rr, extension=0.42,
                 hint=(1, 0, 0))
    m.cable([(0, hy * 0.6, cz + rr * 0.5), (0, -hy * 0.6, cz + rr * 0.5)], rr * 0.6)
    if kind == "forearm":
        # Elbow guard: a spur over the back of the elbow, the joint above.
        m.hull([F(s * 0.55, y, 0.55) for s in (-1, 1) for y in (0.45, 0.98)]
               + [F(0, 1.02, 1.2), F(0, 0.4, 1.0), F(0, 0.9, 1.1)], "armor", bevel=0.008)
        # Gauntlet: armour over the wrist drive, the joint below.
        m.hull([F(s * 1.0, y, z) for s in (-1, 1) for y in (-0.15, -0.9) for z in (-0.6, 0.6)]
               + [F(s * 0.8, -0.05, z) for s in (-1, 1) for z in (-0.5, 0.5)], "armor", bevel=0.01)
    if kind == "thigh":
        # Hip-side armour flaring over the top joint.
        m.hull([F(s * 1.02, y, z) for s in (-1, 1) for y in (0.2, 0.75) for z in (-0.7, 0.55)]
               + [F(s * 0.85, 0.98, z) for s in (-1, 1) for z in (-0.55, 0.4)], "armor", bevel=0.01)
    if kind == "shin":
        # Knee guard over the joint above, swept to a point; calf armour over the actuators'
        # lower ends, widening to the ankle; an ankle guard.
        m.hull([F(s * 0.7, 0.5, -0.66) for s in (-1, 1)] + [F(s * 0.5, 1.04, -0.62) for s in (-1, 1)]
               + [F(0, 0.86, -1.1), F(0, 0.35, -1.0)]
               + [F(s * 0.55, y, -0.45) for s in (-1, 1) for y in (0.45, 1.0)], "armor", bevel=0.008)
        m.hull([F(s * w, y, z) for s in (-1, 1) for y, w, z in ((0.2, 0.62, 0.5), (-0.75, 0.85, 0.5),
                                                                (0.1, 0.55, 0.92), (-0.68, 0.8, 1.0))]
               + [F(0, -0.45, 1.1)], "armor", bevel=0.008)
        m.hull([F(s * 0.95, y, z) for s in (-1, 1) for y in (-0.62, -0.98) for z in (-0.95, -0.4)]
               + [F(0, -0.78, -1.15)], "armor2", bevel=0.008)


@builder("bastion.limb", "frame.limb")
def upper_arm(m, h, part):
    """An upper arm: a hexagonal spine of dark frame metal between octagonal pin bosses (pins
    held by retainers); in front a keeled graphite base under two thick ceramic plates;
    graphite side plates; two sealed actuators down the back (they work the elbow) either
    side of their armoured cable."""
    limb_body(m, h, "upper_arm")


@builder("bastion.forearm")
def forearm(m, h, part):
    """A forearm: as an upper arm, heavier toward the wrist: a gauntlet of thick armour over
    the wrist drive and a spur guarding the back of the elbow."""
    limb_body(m, h, "forearm")


@builder("bastion.leg", "frame.leg")
def thigh(m, h, part):
    """A thigh: as an upper arm, bulkier, its armour widest at the hip, where flared plates
    cover the hip joint."""
    limb_body(m, h, "thigh")


@builder("bastion.shin")
def shin(m, h, part):
    """A shin: the knee guard over the joint above, swept to a point; armour widening toward
    the ankle, calf armour over the actuators' lower ends at the back, and an ankle guard."""
    limb_body(m, h, "shin")


@builder("bastion.foot", "frame.foot")
def foot(m, h, part):
    """A talon foot: two pointed ceramic toes and a graphite heel spur round a dark sole
    with grip pads under it; the faceted ankle shroud; sealed shock struts from the shroud
    to the toes and heel."""
    hx, hy, hz = h

    def P(x, y, z):
        return V((x * hx, y * hy, z * hz))

    m.hull([P(sx * 0.92, y, sz * 0.75) for sx in (-1, 1) for sz in (-1, 1) for y in (-0.92, -0.5)],
           "dark", bevel=0.02)
    for sx in (-1, 1):
        cx = sx * 0.5
        m.hull([P(cx + dx, y, -0.15) for dx in (-0.44, 0.44) for y in (-0.95, 0.0)]
               + [P(cx + dx, y, -1.0) for dx in (-0.12, 0.12) for y in (-0.95, -0.6)]
               + [P(cx, -0.1, -0.6)], "armor", bevel=0.01)
        m.box((hx * 0.7, hy * 0.08, hz * 0.6), at=P(cx, -0.98, -0.45), mat="rubber", bevel=0.01, segments=1)
    m.hull([P(dx, y, 0.15) for dx in (-0.55, 0.55) for y in (-0.95, -0.05)]
           + [P(dx, y, 1.0) for dx in (-0.14, 0.14) for y in (-0.95, -0.62)], "armor2", bevel=0.01)
    m.box((hx * 0.9, hy * 0.08, hz * 0.5), at=P(0, -0.98, 0.5), mat="rubber", bevel=0.01, segments=1)
    m.lathe([(0, hy), (hx * 0.4, hy), (hx * 0.45, hy * 0.85), (hx * 0.62, hy * 0.2), (hx * 0.66, -hy * 0.45),
             (0, -hy * 0.45)], frame(), "armor2", 8, angle=20.0)
    for sx in (-1, 1):
        for sz, zt in ((-1, -hz * 0.55), (1, hz * 0.6)):
            m.piston((sx * hx * 0.55, hy * 0.35, sz * hz * 0.15), (sx * hx * 0.7, -hy * 0.35, zt), 0.06,
                     extension=0.35, hint=(1, 0, 0))


@builder("bastion.hand")
def hand(m, h, part):
    """A heavy hand: the wrist drive, a faceted palm under a pointed ceramic back plate,
    three fingers in a row (three tapered phalanges
    each, the last a talon) curling toward the palm side (+X) and an opposed thumb.
    Symmetric front to back, so one part serves both arms."""
    hx, hy, hz = h
    m.drum((0, hy, 0), (0, hy * 0.62, 0), hx * 0.6, "dark", 8, hint=X)
    m.hull([(x, y, sz * hz * 0.82) for sz in (-1, 1) for x, y in ((-hx * 0.5, hy * 0.6), (hx * 0.5, hy * 0.6),
                                                                  (hx * 0.45, 0.0), (-hx * 0.45, 0.0))],
           "dark", bevel=0.015)
    out = [(-hz * 0.85, hy * 0.6), (hz * 0.85, hy * 0.6), (hz * 0.8, hy * 0.05), (0, -hy * 0.15),
           (-hz * 0.8, hy * 0.05)]
    m.plate(out, 0.14, frame((-hx * 0.5, 0, 0), u=(0, 0, 1), v=(0, 1, 0)), "armor", chamfer=0.05)
    seg = hx * 0.2
    for z in (-hz * 0.58, 0.0, hz * 0.58):
        m.drum((0, 0, z - hz * 0.2), (0, 0, z + hz * 0.2), seg * 0.8, "dark", 8, hint=X)
        pts = [V((-hx * 0.28, 0, z)), V((-hx * 0.36, -hy * 0.38, z)), V((-hx * 0.08, -hy * 0.68, z)),
               V((hx * 0.3, -hy * 0.85, z))]
        for i, (p, q) in enumerate(zip(pts, pts[1:])):
            pl = along(p, q, hint=Z)
            ln = (q - p).length
            w0 = seg * (1.0 - 0.15 * i)
            w1 = w0 * (0.2 if i == 2 else 0.85)
            d = hz * 0.21
            m.hull([pl @ V((x, 0, zz)) for x in (-w0, w0) for zz in (-d, d)]
                   + [pl @ V((x, ln * (1.15 if i == 2 else 0.92), zz)) for x in (-w1, w1)
                      for zz in (-d * (0.4 if i == 2 else 0.85), d * (0.4 if i == 2 else 0.85))]
                   + [pl @ V((-w0 * 1.25, ln * 0.45, 0))], "armor" if i != 1 else "armor2", bevel=0.008)
            if i < 2:
                m.drum(q - Z * hz * 0.18, q + Z * hz * 0.18, w0 * 0.7, "dark", 8, hint=X)
    thumb = [V((hx * 0.42, hy * 0.15, 0)), V((hx * 0.62, -hy * 0.25, 0)), V((hx * 0.42, -hy * 0.58, 0))]
    m.drum(thumb[0] - Z * hz * 0.2, thumb[0] + Z * hz * 0.2, seg * 0.9, "dark", 8, hint=X)
    for i, (p, q) in enumerate(zip(thumb, thumb[1:])):
        pl = along(p, q, hint=Z)
        ln = (q - p).length
        w0, w1 = seg * 1.05, seg * (0.85 if i == 0 else 0.2)
        d = hz * 0.22
        m.hull([pl @ V((x, 0, zz)) for x in (-w0, w0) for zz in (-d, d)]
               + [pl @ V((x, ln * (1.1 if i else 0.92), zz)) for x in (-w1, w1) for zz in (-d * 0.8, d * 0.8)],
               "armor", bevel=0.008)
        if i == 0:
            m.drum(q - Z * hz * 0.2, q + Z * hz * 0.2, seg * 0.7, "dark", 8, hint=X)


@builder("frame.backpack")
def backpack(m, h, part):
    """A backpack: a faceted ceramic housing with the radiator vent on its back, two
    octagonal propellant tanks on the sides held by straps, two small faceted thrust nozzles
    under it, and a graphite mounting plate."""
    hx, hy, hz = h
    body = m.hull([(sx * wx * hx, y * hy, z * hz) for sx in (-1, 1)
                   for y, wx in ((0.92, 0.75), (-0.92, 0.9)) for z in (-0.85, 0.6)]
                  + [(sx * 0.6 * hx, y * hy, 0.85 * hz) for sx in (-1, 1) for y in (0.8, -0.8)], "armor",
                  bevel=0.015)
    m.vent(body, (0, 0, hz * 0.85), Z, Y, hx * 0.9, hy * 1.1, fins=8, depth=0.15)
    for sx in (-1, 1):
        c = V((sx * hx * 0.98, 0, 0))
        m.lathe([(0, -hy * 0.8), (hz * 0.3, -hy * 0.72), (hz * 0.42, -hy * 0.55), (hz * 0.42, hy * 0.55),
                 (hz * 0.3, hy * 0.72), (0, hy * 0.8)], frame(c), "armor2", 8, angle=20.0)
        for y in (-hy * 0.4, hy * 0.4):
            m.lathe([(hz * 0.4, y - 0.05), (hz * 0.45, y - 0.05), (hz * 0.45, y + 0.05), (hz * 0.4, y + 0.05)],
                    frame(c), "metal", 8, closed=True, angle=20.0)
        n = V((sx * hx * 0.45, -hy * 0.92, 0))
        m.lathe([(hx * 0.12, 0.0), (hx * 0.18, -0.05), (hx * 0.3, -0.3), (hx * 0.28, -0.3), (hx * 0.16, -0.06),
                 (hx * 0.1, 0.0)], frame(n), "dark", 8, closed=True, angle=20.0)
    m.plate([(-hx * 0.8, -hy * 0.8), (hx * 0.8, -hy * 0.8), (hx * 0.8, hy * 0.8), (-hx * 0.8, hy * 0.8)], 0.1,
            frame((0, 0, -hz * 0.85), u=(-1, 0, 0), w=(0, 0, -1)), "armor2", chamfer=0.04)


@builder("frame.binder")
def binder(m, h, part):
    """A binder: a pointed ceramic armour plate with a raised inner plate (two layers) and a
    graphite keel to deflect hits, on a dark mounting bracket."""
    hx, hy, hz = h
    out = [(-hz * 0.55, -hy), (hz * 0.55, -hy), (hz, -hy * 0.6), (hz, hy * 0.7), (0, hy), (-hz, hy * 0.7),
           (-hz, -hy * 0.6)]
    pl = frame((hx * 0.0, 0, 0), u=(0, 0, -1), v=(0, 1, 0))
    m.plate([(-z, y) for z, y in out], hx * 0.6, pl, "armor", chamfer=0.06)
    inner = [(x * 0.7, y * 0.75 - hy * 0.05) for x, y in out]
    x0 = hx * 0.58
    xt = x0 + 0.12
    m.plate([(-z, y) for z, y in inner], 0.12, frame((x0, 0, 0), u=(0, 0, -1), v=(0, 1, 0)), "armor",
            chamfer=0.05)
    m.hull([(xt, y, sz * 0.08) for y in (-hy * 0.75, hy * 0.6) for sz in (-1, 1)]
           + [(xt + 0.2, y, 0) for y in (-hy * 0.65, hy * 0.5)], "armor2", bevel=0.005)
    m.hull([(x, sy * hy * 0.25, sz * hz * 0.25) for x in (-hx, 0) for sy in (-1, 1) for sz in (-1, 1)]
           + [(-hx, 0, sz * hz * 0.32) for sz in (-1, 1)], "dark", bevel=0.01)


@builder("frame.drone_pod")
def drone_pod(m, h, part):
    """A drone pod: an octagonal launch tube, chamfered, with a dark breech; a marker lamp
    at the muzzle shows it is armed."""
    hx, hy, hz = h
    r = min(hx, hy)
    m.lathe([(0, -hz), (r * 0.75, -hz), (r * 0.9, -hz * 0.85), (r * 0.9, hz * 0.8), (r * 0.95, hz * 0.85),
             (r * 0.95, hz), (r * 0.7, hz), (r * 0.7, hz * 0.9), (0, hz * 0.9)],
            frame(u=(1, 0, 0), v=(0, 0, 1)), "armor", 8, angle=20.0)
    m.lathe([(0, -hz * 1.0), (r * 0.6, -hz * 1.0), (r * 0.6, -hz * 0.8), (0, -hz * 0.8)],
            frame((0, 0, -0.01), u=(1, 0, 0), v=(0, 0, 1)), "dark", 8, angle=20.0)
    m.lamp((0, r * 0.95, hz * 0.7), Y, 0.04)


@builder("frame.gyro")
def gyro(m, h, part):
    """A reaction-wheel unit: a faceted housing with stiffening bands round the wheel."""
    r = h.x
    m.lathe([(0, -r * 0.55), (r * 0.8, -r * 0.55), (r * 0.9, -r * 0.42), (r * 0.9, r * 0.42),
             (r * 0.8, r * 0.55), (0, r * 0.55)], frame(), "dark", 12, angle=20.0)
    for y in (-r * 0.3, 0.0, r * 0.3):
        m.lathe([(r * 0.88, y - 0.03), (r * 0.94, y - 0.03), (r * 0.94, y + 0.03), (r * 0.88, y + 0.03)],
                frame(), "metal", 12, closed=True, angle=20.0)
