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
    """The torso and cockpit. A faceted chest of dark frame metal is armoured in ceramic
    plates: the two upper plates either side of the keel are the crew hatch (they part along
    it), with glowing vision slits; graphite flank plates carry the reactor's heat vents.
    Chevron lames layer down the abdomen, flexed by a sealed actuator each side. The shoulder
    bearings and the neck collar glow at their gaps. On the back, the reactor housing shows
    its core through three slits between two swept heat fins."""
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
        # Hatch plates, with vision slits.
        plate, c, n = m.facet([Kt, St[s], Sm[s], Km], 0.16, inset=0.08, outward=O)
        along_ = in_plane(n, St[s] - Kt)
        up = in_plane(n, Kt - Km)
        for k, dy in enumerate((0.22, 0.1)):
            q = c + up * (dy * hy) - along_ * 0.02 * hx
            m.slit(plate, q - along_ * hx * (0.18 - 0.05 * k), q + along_ * hx * (0.18 - 0.05 * k), n, 0.035, 0.05)
        plate, c, n = m.facet([Km, Sm[s], Sl[s], Kl], 0.14, inset=0.09, outward=O)
        up = in_plane(n, Km - Kl)
        acr = in_plane(n, Sm[s] - Km)
        for k in (-1, 1):
            q = c + acr * (k * hx * 0.14)
            m.slit(plate, q - up * hy * 0.12, q + up * hy * 0.12, n, 0.035, 0.05)
        # Top plates.
        m.facet([Kt, St[s], Bt[s], P(0, 0.96, 0.6)], 0.1, inset=0.1, outward=O, mat="armor2")
        # Flank plate with heat vents, below the shoulder bearing.
        top_f, top_b = P(s * 0.95, 0.28, -0.72), P(s * 0.9, 0.28, 0.6)
        plate, c, n = m.facet([top_f, Sl[s], Bl[s], top_b], 0.14, inset=0.06, outward=O, mat="armor2")
        fwd = in_plane(n, -Z)
        up = in_plane(n, Y)
        for k in range(3):
            q = c + up * ((k - 1) * hy * 0.075) + fwd * hz * 0.05
            m.slit(plate, q - fwd * hz * 0.45, q + fwd * hz * 0.25, n, 0.06, 0.08)
    ridge(m, Kt, Km, (0, 0.35, -1), 0.09, 0.18)
    ridge(m, Km, Kl, (0, -0.3, -1), 0.09, 0.16)
    # Abdomen lames: chevrons, each over the next.
    Oa = P(0, -0.5, 0)
    for k in range(3):
        y0 = -0.06 - k * 0.27
        w = 0.62 - 0.07 * k
        z = -0.8 + 0.05 * k
        pts = [P(-w, y0, z), P(w, y0, z), P(w * 0.92, y0 - 0.2, z + 0.02), P(0, y0 - 0.34, z - 0.02),
               P(-w * 0.92, y0 - 0.2, z + 0.02)]
        m.facet(pts, 0.13, inset=0.0, outward=Oa, mat="armor" if k != 1 else "armor2")
    for s in (-1, 1):
        m.piston(P(s * 0.52, 0.0, -0.66), P(s * 0.46, -0.84, -0.58), hx * 0.05, extension=0.35, hint=(0, 0, 1))
    # Shoulder bearings.
    for s in (-1, 1):
        m.drum(P(s * 0.8, 0.57, 0), P(s * 1.0, 0.57, 0), hy * 0.3, "dark", 8, hint=(0, 1, 0))
        m.ring_glow(P(s * 1.0, 0.57, 0), X, hy * 0.12, hy * 0.2, 0.02)
    # Reactor housing, core slits, heat fins.
    reactor = m.hull([P(sx * wx, y, z) for sx in (-1, 1) for y in (-0.62, 0.72)
                      for wx, z in ((0.48, 0.6), (0.32, 0.98))]
                     + [P(0, -0.8, 0.8), P(0, 0.86, 0.8)], "armor2", bevel=0.01)
    for x in (-0.16, 0.0, 0.16):
        m.slit(reactor, P(x, -0.42, 0.98), P(x, 0.52, 0.98), Z, 0.07, 0.1)
    for s in (-1, 1):
        fin = [(0.55 * hz, 0.05 * hy), (0.55 * hz, 0.92 * hy), (1.06 * hz, 1.04 * hy), (0.86 * hz, 0.12 * hy)]
        pl = frame((s * 0.6 * hx, 0, 0), u=(0, 0, -s), v=(0, 1, 0))
        m.plate([(-z, y) for z, y in fin] if s > 0 else fin, 0.07, pl, "armor", chamfer=0.03)
    # Neck collar.
    m.lathe([(hx * 0.36, hy * 0.95), (hx * 0.46, hy * 0.95), (hx * 0.46, hy * 1.0), (hx * 0.42, hy * 1.05),
             (hx * 0.36, hy * 1.05)], frame(), "dark", 8, closed=True, angle=20.0)
    m.ring_glow(P(0, 1.05, 0), Y, hx * 0.37, hx * 0.41, 0.02)


@builder("bastion.head", "frame.head")
def head(m, h, part):
    """The sensor head on its neck bearing: a pointed ceramic helm over a dark faceted face,
    the visor glowing in the gap between them; a sensor array of three lenses in the face;
    a crest blade and two swept antenna blades."""
    hx, hy, hz = h

    def P(x, y, z):
        return V((x * hx, y * hy, z * hz))

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
    m.sensor(P(0, -0.3, -0.8), -Z, Y, 3, r=hx * 0.075)
    crest = [(-0.45 * hz, 0.5 * hy), (0.25 * hz, 0.68 * hy), (0.98 * hz, 0.96 * hy), (0.6 * hz, 0.58 * hy)]
    m.plate([(-z, y) for z, y in crest], 0.07, frame((-0.035, 0, 0), u=(0, 0, -1), v=(0, 1, 0)), "armor2",
            chamfer=0.025)
    for s in (-1, 1):
        ear = [(-0.15 * hz, 0.12 * hy), (0.3 * hz, 0.24 * hy), (1.02 * hz, 0.55 * hy), (0.4 * hz, -0.12 * hy)]
        pl = frame((s * 0.8 * hx, 0, 0), u=(0, 0, -s), v=(0, 1, 0))
        m.plate([(-z, y) for z, y in ear] if s > 0 else ear, 0.05, pl, "dark", chamfer=0.02)


@builder("bastion.waist", "frame.waist")
def waist(m, h, part):
    """The pelvis: the sealed slewing drive the torso turns on, glowing round its gap; a
    faceted pelvis frame; both hip actuators, octagonal and glowing where they turn; pointed
    skirt plates front and back with glowing seams, and blade tassets at the sides."""
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
        m.ring_glow(P(sx * 0.945, -0.45, 0), X, hy * 0.2, hy * 0.32, 0.02)
    for sz in (-1, 1):
        y0 = -hy * 0.1
        out = [(-hx * 0.42, 0), (hx * 0.42, 0), (hx * 0.3, -hy * 0.7), (0, -hy * 1.05), (-hx * 0.3, -hy * 0.7)]
        pl = frame((0, y0, sz * hz * 0.96), u=(sz, 0, 0), v=(0, 1, sz * 0.25))
        sk = m.plate(out, 0.1, pl, "armor", chamfer=0.04)
        n = (pl.to_3x3() @ Z).normalized()
        for x in (-hx * 0.12, hx * 0.12):
            m.slit(sk, pl @ V((x, -hy * 0.15, 0.1)), pl @ V((x, -hy * 0.6, 0.1)), n, 0.025, 0.03)
    for sx in (-1, 1):
        out = [(-hz * 0.5, 0), (hz * 0.5, 0), (hz * 0.3, -hy * 0.6), (-hz * 0.05, -hy * 0.95), (-hz * 0.4, -hy * 0.5)]
        pl = frame((sx * hx * 0.97, hy * 0.25, 0), u=(0, 0, -sx), v=(0, 1, 0))
        m.plate([(-z, y) for z, y in out] if sx > 0 else out, 0.08, pl, "armor2", chamfer=0.03)


@builder("bastion.shoulder", "frame.shoulder")
def shoulder(m, h, part):
    """A shoulder block: the bearing that turns it on the torso, glowing at its gap; stepped,
    faceted pauldron lames over the top, pointed fore and aft, with glowing seams on the top
    lame; the arm's swing actuator below."""
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
            pts += [(-r0 * 0.3, ys[1] - 0.05, sz * (zz + hz * 0.15)), (r0 * 0.3, ys[1] - 0.05, sz * (zz + hz * 0.15))]
        top = m.hull(pts, "armor" if k != 1 else "armor2", bevel=0.01), ys[1], r0, zz
    lame, yt, r0, zz = top
    for x in (-r0 * 0.25, r0 * 0.25):
        m.slit(lame, (x, yt, -zz * 0.7), (x, yt, zz * 0.7), Y, 0.025, 0.035)
    m.drum((0, -hy * 0.4, 0), (0, -hy, 0), hx * 0.5, "dark", 8, hint=X)


def limb_body(m, h, knee):
    hx, hy, hz = h
    yb = hy * 0.84
    rb = min(hx, hz) * 0.5
    # Spine.
    cx, cz = hx * 0.42, hz * 0.5
    sec = [(cx, 0), (cx * 0.6, cz), (-cx * 0.6, cz), (-cx, 0), (-cx * 0.6, -cz), (cx * 0.6, -cz)]
    m.hull([(x * (1.0 if y > 0 else 0.8), y, z) for x, z in sec for y in (-yb, yb)], "dark", bevel=0.01)
    # Pin bosses: the joint's hub between two faceted cheeks.
    for s in (-1, 1):
        y = s * yb
        m.drum((-hx * 0.45, y, 0), (hx * 0.45, y, 0), rb, "metal", 8, glow=True, hint=(0, 0, 1))
        for sx in (-1, 1):
            m.drum((sx * hx * 0.47, y, 0), (sx * hx * 0.8, y, 0), rb * 0.86, "dark", 8, glow=False,
                   hint=(0, 0, 1))
            m.ring_glow((sx * hx * 0.805, y, 0), X, rb * 0.4, rb * 0.58, 0.02)
            m.cyl((sx * hx * 0.8, y, 0), (sx * hx * 0.85, y, 0), rb * 0.36, "metal", 6)
    # Front armour: a keeled graphite base with a ceramic plate either side of the keel.
    def F(x, y, z):
        return V((x * hx, y * hy, z * hz))
    top_k, bot_k = F(0, 0.6, -0.98), F(0, -0.46, -1.02)
    top_s = {s: F(s * 0.74, 0.64, -0.78) for s in (-1, 1)}
    bot_s = {s: F(s * 0.56, -0.5, -0.84) for s in (-1, 1)}
    m.hull([top_k, bot_k] + [d[s] for d in (top_s, bot_s) for s in (-1, 1)]
           + [F(s * w, y, -0.42) for s in (-1, 1) for y, w in ((0.56, 0.9), (-0.5, 0.68))], "armor2",
           bevel=0.01)
    O = F(0, 0, 0)
    for s in (-1, 1):
        plate, c, n = m.facet([top_k, top_s[s], bot_s[s], bot_k], 0.1, inset=0.1, outward=O)
        up = in_plane(n, Y)
        acr = up.cross(n)
        acr = acr if acr.x * s > 0 else -acr
        q = c + acr * hx * 0.18
        m.slit(plate, q - up * hy * 0.32, q + up * hy * 0.38, n, 0.03, 0.04)
    ridge(m, top_k, bot_k, (0, 0, -1), 0.07, 0.18)
    # Side plates with cooling seams.
    for s in (-1, 1):
        side = [(-0.5 * hz, 0.6 * hy), (0.45 * hz, 0.5 * hy), (0.5 * hz, -0.45 * hy), (-0.5 * hz, -0.55 * hy)]
        pl = frame((s * hx * 0.5, 0, 0), u=(0, 0, -s), v=(0, 1, 0))
        plate = m.plate([(-z, y) for z, y in side] if s > 0 else side, hx * 0.24, pl, "armor2", chamfer=0.03)
        x = s * hx * 0.74
        for k in range(2):
            y0 = -0.25 * hy + k * 0.4 * hy
            m.slit(plate, (x, y0 - 0.12 * hy, -0.3 * hz), (x, y0 + 0.12 * hy, 0.3 * hz), X * s, 0.025, 0.035)
    # Back: two sealed actuators (they work the joint below) either side of a power conduit.
    rr = min(hx, hz) * 0.13
    zr = cz + rr * 1.3
    for sx in (-1, 1):
        m.piston((sx * hx * 0.3, hy * 0.62, zr), (sx * hx * 0.3, -hy * 0.72, zr), rr, extension=0.42,
                 hint=(1, 0, 0))
    m.conduit([(0, hy * 0.6, cz + rr * 0.5), (0, -hy * 0.6, cz + rr * 0.5)], rr * 0.6)
    if knee:
        m.hull([F(s * 0.6, -0.5, -0.66) for s in (-1, 1)] + [F(s * 0.4, -1.0, -0.62) for s in (-1, 1)]
               + [F(0, -0.86, -1.24), F(0, -0.46, -1.06)]
               + [F(s * 0.5, y, -0.45) for s in (-1, 1) for y in (-0.55, -0.98)], "armor", bevel=0.008)


@builder("bastion.limb", "frame.limb")
def limb(m, h, part):
    """An arm segment: a hexagonal spine of dark frame metal between octagonal pin bosses
    (glowing where they turn); in front a keeled graphite base under two ceramic plates with
    glowing seams; graphite side plates with cooling seams; two sealed actuators down the
    back (they work the joint below) either side of a ribbed power conduit."""
    limb_body(m, h, knee=False)


@builder("bastion.leg", "frame.leg")
def leg(m, h, part):
    """A leg segment: as an arm segment, heavier, with a pointed knee guard at the bottom
    front."""
    limb_body(m, h, knee=True)


@builder("bastion.foot", "frame.foot")
def foot(m, h, part):
    """A talon foot: two pointed ceramic toes and a graphite heel spur round a dark sole
    with grip pads under it; the faceted ankle shroud, glowing round the ankle; sealed shock
    struts from the shroud to the toes and heel."""
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
    m.ring_glow(P(0, 0.9, 0), Y, hx * 0.42, hx * 0.47, 0.03)
    for sx in (-1, 1):
        for sz, zt in ((-1, -hz * 0.55), (1, hz * 0.6)):
            m.piston((sx * hx * 0.55, hy * 0.35, sz * hz * 0.15), (sx * hx * 0.7, -hy * 0.35, zt), 0.06,
                     extension=0.35, hint=(1, 0, 0))


@builder("bastion.hand")
def hand(m, h, part):
    """A heavy hand: the wrist drive glowing round its gap, a faceted palm under a pointed
    ceramic back plate with a glowing seam, three fingers in a row (three tapered phalanges
    each, the last a talon) curling toward the palm side (+X) and an opposed thumb.
    Symmetric front to back, so one part serves both arms."""
    hx, hy, hz = h
    m.drum((0, hy, 0), (0, hy * 0.62, 0), hx * 0.6, "dark", 8, hint=X)
    m.hull([(x, y, sz * hz * 0.82) for sz in (-1, 1) for x, y in ((-hx * 0.5, hy * 0.6), (hx * 0.5, hy * 0.6),
                                                                  (hx * 0.45, 0.0), (-hx * 0.45, 0.0))],
           "dark", bevel=0.015)
    out = [(-hz * 0.85, hy * 0.6), (hz * 0.85, hy * 0.6), (hz * 0.8, hy * 0.05), (0, -hy * 0.15),
           (-hz * 0.8, hy * 0.05)]
    back = m.plate(out, 0.1, frame((-hx * 0.5, 0, 0), u=(0, 0, 1), v=(0, 1, 0)), "armor", chamfer=0.04)
    m.slit(back, (-hx * 0.6, hy * 0.5, 0), (-hx * 0.6, hy * 0.08, 0), -X, 0.025, 0.035)
    seg = hx * 0.2
    for z in (-hz * 0.58, 0.0, hz * 0.58):
        m.drum((0, 0, z - hz * 0.2), (0, 0, z + hz * 0.2), seg * 0.8, "dark", 8, glow=False, hint=X)
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
                m.drum(q - Z * hz * 0.18, q + Z * hz * 0.18, w0 * 0.7, "dark", 8, glow=False, hint=X)
    thumb = [V((hx * 0.42, hy * 0.15, 0)), V((hx * 0.62, -hy * 0.25, 0)), V((hx * 0.42, -hy * 0.58, 0))]
    m.drum(thumb[0] - Z * hz * 0.2, thumb[0] + Z * hz * 0.2, seg * 0.9, "dark", 8, glow=False, hint=X)
    for i, (p, q) in enumerate(zip(thumb, thumb[1:])):
        pl = along(p, q, hint=Z)
        ln = (q - p).length
        w0, w1 = seg * 1.05, seg * (0.85 if i == 0 else 0.2)
        d = hz * 0.22
        m.hull([pl @ V((x, 0, zz)) for x in (-w0, w0) for zz in (-d, d)]
               + [pl @ V((x, ln * (1.1 if i else 0.92), zz)) for x in (-w1, w1) for zz in (-d * 0.8, d * 0.8)],
               "armor", bevel=0.008)
        if i == 0:
            m.drum(q - Z * hz * 0.2, q + Z * hz * 0.2, seg * 0.7, "dark", 8, glow=False, hint=X)


@builder("frame.backpack")
def backpack(m, h, part):
    """A backpack: a faceted ceramic housing whose radiator glows through vertical slits,
    two octagonal propellant tanks on the sides with glowing bands, two small faceted thrust
    nozzles under it, and a graphite mounting plate."""
    hx, hy, hz = h
    body = m.hull([(sx * wx * hx, y * hy, z * hz) for sx in (-1, 1)
                   for y, wx in ((0.92, 0.75), (-0.92, 0.9)) for z in (-0.85, 0.6)]
                  + [(sx * 0.6 * hx, y * hy, 0.85 * hz) for sx in (-1, 1) for y in (0.8, -0.8)], "armor",
                  bevel=0.015)
    for k in range(5):
        x = (k - 2) * 0.22 * hx
        m.slit(body, (x, -hy * 0.55, hz * 0.85), (x, hy * 0.55, hz * 0.85), Z, 0.05, 0.08)
    for sx in (-1, 1):
        c = V((sx * hx * 0.98, 0, 0))
        m.lathe([(0, -hy * 0.8), (hz * 0.3, -hy * 0.72), (hz * 0.42, -hy * 0.55), (hz * 0.42, hy * 0.55),
                 (hz * 0.3, hy * 0.72), (0, hy * 0.8)], frame(c), "armor2", 8, angle=20.0)
        for y in (-hy * 0.4, hy * 0.4):
            m.ring_glow(c + V((0, y, 0)), Y, hz * 0.4, hz * 0.44, 0.04)
        n = V((sx * hx * 0.45, -hy * 0.92, 0))
        m.lathe([(hx * 0.12, 0.0), (hx * 0.18, -0.05), (hx * 0.3, -0.3), (hx * 0.28, -0.3), (hx * 0.16, -0.06),
                 (hx * 0.1, 0.0)], frame(n), "dark", 8, closed=True, angle=20.0)
        m.ring_glow(n + V((0, -0.06, 0)), Y, hx * 0.1, hx * 0.16, 0.02)
    m.plate([(-hx * 0.8, -hy * 0.8), (hx * 0.8, -hy * 0.8), (hx * 0.8, hy * 0.8), (-hx * 0.8, hy * 0.8)], 0.1,
            frame((0, 0, -hz * 0.85), u=(-1, 0, 0), w=(0, 0, -1)), "armor2", chamfer=0.04)


@builder("frame.binder")
def binder(m, h, part):
    """A binder: a pointed ceramic armour plate with a raised inner plate, a graphite keel
    and glowing seams, on a dark mounting bracket."""
    hx, hy, hz = h
    out = [(-hz * 0.55, -hy), (hz * 0.55, -hy), (hz, -hy * 0.6), (hz, hy * 0.7), (0, hy), (-hz, hy * 0.7),
           (-hz, -hy * 0.6)]
    pl = frame((hx * 0.0, 0, 0), u=(0, 0, -1), v=(0, 1, 0))
    m.plate([(-z, y) for z, y in out], hx * 0.6, pl, "armor", chamfer=0.06)
    inner = [(x * 0.7, y * 0.75 - hy * 0.05) for x, y in out]
    x0 = hx * 0.58
    xt = x0 + 0.12
    plate = m.plate([(-z, y) for z, y in inner], 0.12, frame((x0, 0, 0), u=(0, 0, -1), v=(0, 1, 0)), "armor",
                    chamfer=0.05)
    for z in (-hz * 0.45, hz * 0.45):
        m.slit(plate, (xt, -hy * 0.6, z), (xt, hy * 0.4, z), X, 0.03, 0.05)
    m.hull([(xt, y, sz * 0.08) for y in (-hy * 0.75, hy * 0.6) for sz in (-1, 1)]
           + [(xt + 0.2, y, 0) for y in (-hy * 0.65, hy * 0.5)], "armor2", bevel=0.005)
    m.hull([(x, sy * hy * 0.25, sz * hz * 0.25) for x in (-hx, 0) for sy in (-1, 1) for sz in (-1, 1)]
           + [(-hx, 0, sz * hz * 0.32) for sz in (-1, 1)], "dark", bevel=0.01)


@builder("frame.drone_pod")
def drone_pod(m, h, part):
    """A drone pod: an octagonal launch tube, chamfered, a dark breech and a glowing ring at
    the muzzle."""
    hx, hy, hz = h
    r = min(hx, hy)
    m.lathe([(0, -hz), (r * 0.75, -hz), (r * 0.9, -hz * 0.85), (r * 0.9, hz * 0.8), (r * 0.95, hz * 0.85),
             (r * 0.95, hz), (r * 0.7, hz), (r * 0.7, hz * 0.9), (0, hz * 0.9)],
            frame(u=(1, 0, 0), v=(0, 0, 1)), "armor", 8, angle=20.0)
    m.lathe([(0, -hz * 1.0), (r * 0.6, -hz * 1.0), (r * 0.6, -hz * 0.8), (0, -hz * 0.8)],
            frame((0, 0, -0.01), u=(1, 0, 0), v=(0, 0, 1)), "dark", 8, angle=20.0)
    m.ring_glow((0, 0, hz * 0.88), Z, r * 0.72, r * 0.86, 0.03)


@builder("frame.gyro")
def gyro(m, h, part):
    """A reaction-wheel unit: a faceted housing ringed by glowing bands."""
    r = h.x
    m.lathe([(0, -r * 0.55), (r * 0.8, -r * 0.55), (r * 0.9, -r * 0.42), (r * 0.9, r * 0.42),
             (r * 0.8, r * 0.55), (0, r * 0.55)], frame(), "dark", 12, angle=20.0)
    for y in (-r * 0.3, 0.0, r * 0.3):
        m.ring_glow((0, y, 0), Y, r * 0.88, r * 0.93, 0.03, sides=12)
