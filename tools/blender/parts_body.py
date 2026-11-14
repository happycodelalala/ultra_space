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
    """The torso, heavy industrial: a dark frame under thick sculpted armour. Two chest plates
    flank the reactor housing, the core glowing behind its radial cooling fins; rounded
    abdomen bands with the frame and two flex pistons showing between them; a high armoured
    collar round the neck joint; hazard stripes at the pinch point between chest and abdomen;
    floodlights at the upper corners; servo drums for the shoulders. On the back, the heat
    sink (a bank of fins) with two exhaust ports."""
    hx, hy, hz = h

    def P(x, y, z):
        return V((x * hx, y * hy, z * hz))

    round_ = dict(bevel=0.1, segments=3, angle=35.0)
    m.hull([P(sx * wx, y, sz * wz) for sx in (-1, 1) for sz in (-1, 1)
            for y, wx, wz in ((0.9, 0.8, 0.55), (0.1, 0.74, 0.7), (-0.94, 0.45, 0.6))], "dark", bevel=0.03)
    # Chest plates, sculpted, either side of the reactor.
    for s in (-1, 1):
        m.hull([P(s * x, y, z) for x, y, z in (
            (0.25, 0.94, -0.62), (0.92, 0.94, -0.5), (0.98, 0.5, -0.7), (0.85, 0.12, -0.72), (0.28, 0.08, -0.86),
            (0.55, 0.55, -1.0), (0.28, 0.94, -0.2), (0.92, 0.94, -0.2), (0.95, 0.12, -0.3), (0.28, 0.08, -0.3))],
            "armor", **round_)
    # Reactor housing and its glowing core behind radial fins.
    rc = P(0, 0.5, -0.9)
    fwd = V((0, 0.1, -1)).normalized()
    rr = hx * 0.24
    m.lathe([(rr * 0.6, -0.25), (rr, -0.25), (rr * 1.08, -0.1), (rr * 1.08, 0.08), (rr * 0.9, 0.12),
             (rr * 0.75, 0.12), (rr * 0.75, -0.05), (rr * 0.6, -0.05)], along(rc, rc + fwd), "metal", 24,
            closed=True, angle=40.0)
    m.cyl(rc - fwd * 0.2, rc - fwd * 0.05, rr * 0.75, "light", 24)
    for k in range(12):
        t = 2 * math.pi * k / 12
        u = V((math.cos(t), math.sin(t), 0))
        u = (u - fwd * u.dot(fwd)).normalized()
        m.box((0.03, rr * 0.62, 0.14), place=frame(rc + u * rr * 0.42, u=fwd.cross(u), v=u), mat="dark", bevel=0)
    m.cyl(rc - fwd * 0.04, rc + fwd * 0.04, rr * 0.18, "dark", 12, bevel=0.01, segments=1)
    # Hazard stripes at the pinch point under the chest.
    m.hazard(P(-0.5, 0.06, -0.84), P(0.5, 0.06, -0.84), V((0, -0.2, -1)).normalized(), 0.2, 0.4)
    # Abdomen bands, rounded, the frame and two flex pistons between them.
    for k in range(3):
        y0 = -0.14 - k * 0.27
        w = 0.42 - 0.05 * k
        m.box((2 * hx * w, hy * 0.2, hz * 0.5), at=P(0, y0 - 0.06, -0.55 + 0.04 * k), mat="armor" if k != 1 else "armor2",
              bevel=0.1, segments=3)
    for s in (-1, 1):
        m.ram(P(s * 0.56, 0.05, -0.62), P(s * 0.44, -0.86, -0.58), hx * 0.07, extension=0.35, hint=(0, 0, 1),
              booted=False)
    # Trapezius slabs either side of the neck joint, floodlights in their front edges.
    for s in (-1, 1):
        m.hull([P(s * x, y, z) for x, y, z in ((0.24, 0.9, -0.62), (0.9, 0.9, -0.55), (0.24, 0.9, 0.62),
                                                (0.9, 0.9, 0.6), (0.3, 1.04, -0.5), (0.7, 1.0, -0.45),
                                                (0.3, 1.04, 0.5), (0.7, 1.0, 0.48))], "armor2", **round_)
        m.lamp(P(s * 0.55, 0.96, -0.6), V((0, 0.15, -1)).normalized(), 0.09)
    # Shoulder servos.
    for s in (-1, 1):
        m.servo(P(s * 0.8, 0.57, 0), P(s * 1.0, 0.57, 0), hy * 0.32, "dark", 12)
    # Sternum plate over the reactor, flank plates under the shoulder servos.
    m.hull([P(sx * x, y, z) for sx in (-1, 1) for x, y, z in ((0.3, 0.92, -0.72), (0.3, 0.74, -0.86),
                                                               (0.22, 0.92, -0.5), (0.22, 0.74, -0.6))],
           "armor2", bevel=0.05, segments=2)
    for s in (-1, 1):
        m.hull([P(s * x, y, z) for x, y, z in ((0.8, 0.3, -0.5), (0.8, 0.3, 0.5), (0.66, -0.6, -0.5),
                                                (0.66, -0.6, 0.5), (0.9, 0.25, -0.4), (0.9, 0.25, 0.4),
                                                (0.74, -0.55, -0.4), (0.74, -0.55, 0.4))], "armor", **round_)
    # Back: two armoured shoulder blades either side of the heat sink (a bank of fins), the
    # lower back plate below with two exhaust ports.
    for s in (-1, 1):
        m.hull([P(s * x, y, z) for x, y, z in ((0.3, 0.9, 0.62), (0.92, 0.9, 0.52), (0.95, 0.15, 0.62),
                                                (0.3, 0.05, 0.7), (0.6, 0.5, 0.86), (0.3, 0.9, 0.3),
                                                (0.92, 0.9, 0.2), (0.95, 0.15, 0.3), (0.3, 0.05, 0.3))],
               "armor", **round_)
    sink = P(0, 0.48, 0.74)
    m.box((hx * 0.56, hy * 0.82, hz * 0.14), at=sink, mat="metal", bevel=0.03, segments=2)
    for k in range(7):
        x = (-0.5 + k / 6) * hx * 0.48
        m.box((0.05, hy * 0.74, hz * 0.3), at=sink + V((x, 0, hz * 0.12)), mat="dark", bevel=0.005, segments=1)
    lb = P(0, -0.45, 0.66)
    m.box((hx * 1.05, hy * 0.75, hz * 0.3), at=lb, mat="armor2", bevel=0.1, segments=3)
    for s in (-1, 1):
        e = lb + V((s * 0.28 * hx, -0.05 * hy, hz * 0.15))
        m.lathe([(0.1, 0), (0.2, 0), (0.2, 0.1), (0.17, 0.13), (0.1, 0.13)], along(e, e + Z), "metal", 16,
                closed=True, angle=40.0)
        m.cyl(e - Z * 0.05, e + Z * 0.02, 0.1, "dark", 16)
    m.hazard(lb + V((-hx * 0.5, hy * 0.33, hz * 0.15)), lb + V((hx * 0.5, hy * 0.33, hz * 0.15)), Z, 0.12, 0.35)


@builder("bastion.head", "frame.head")
def head(m, h, part):
    """The head, an armoured helmet on its neck servo: the brow juts over a recessed visor slit
    across the front, lit from within by the main optics; the jaw guard below it; a servo
    either side (the sensor pods turn on them); floodlights under the brow and a low crest."""
    hx, hy, hz = h
    k = 1.18

    def P(x, y, z):
        return V((x * hx * k, (-1 + (y + 1) * k) * hy, z * hz * k))

    round_ = dict(bevel=0.08, segments=3, angle=35.0)
    m.servo(P(0, -1.0, 0), P(0, -0.72, 0), hx * 0.55, "dark", 10)
    # Dark core: fills the band between helmet and jaw round the sides and back.
    m.hull([P(sx * x, y, z) for sx in (-1, 1) for y in (-0.3, 0.15)
            for x, z in ((0.66, 0.55), (0.7, -0.45), (0.42, -0.74))], "dark", bevel=0.01)
    # Helmet: brow jutting forward over the visor.
    m.hull([P(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
        (0.75, 0.0, 0.75), (0.8, 0.0, -0.5), (0.5, -0.02, -0.98), (0.7, 0.55, 0.65), (0.62, 0.6, -0.45),
        (0.38, 0.42, -0.92), (0.3, 0.7, 0.2))], "armor", **round_)
    # Jaw guard.
    m.hull([P(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
        (0.7, -0.22, 0.6), (0.72, -0.22, -0.45), (0.42, -0.22, -0.8), (0.6, -0.7, 0.5), (0.52, -0.72, -0.42),
        (0.26, -0.62, -0.72))], "armor2", **round_)
    # Visor slit, recessed under the brow.
    m.hull([P(sx * x, y, z) for sx in (-1, 1) for y in (-0.16, -0.04)
            for x, z in ((0.42, -0.78), (0.64, -0.5))], "light", bevel=0.0)
    for s in (-1, 1):
        m.servo(P(s * 0.74, -0.3, 0.05), P(s * 0.9, -0.3, 0.05), hy * k * 0.22, "dark", 8)
        m.lamp(P(s * 0.62, -0.05, -0.72), V((s * 0.3, 0, -1)).normalized(), 0.05)
    m.hull([P(sx * 0.05, y, z) for sx in (-1, 1) for y, z in ((0.68, -0.3), (0.7, 0.5))]
           + [P(0, 0.82, 0.3), P(0, 0.78, -0.1)], "armor2", bevel=0.01)


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
    +Y, the one below at -Y, front -Z. Heavy industrial: a dark frame between servo hubs,
    thick rounded armour front and back with a dark gap between the layers, the hydraulic
    rams that work the joint below out in the open along the sides, chrome rods showing,
    their hoses bundled down the back."""
    hx, hy, hz = h
    yb = hy * 0.84
    rb = min(hx, hz) * 0.5
    wt, wb = {"upper_arm": (1.0, 0.95), "forearm": (1.05, 1.1), "thigh": (1.04, 1.0),
              "shin": (1.0, 1.12)}[kind]
    round_ = dict(bevel=min(0.09, hx * 0.18), segments=3, angle=35.0)

    def F(x, y, z):
        w = wb + (wt - wb) * (y + 1) / 2
        return V((x * hx * w, y * hy, z * hz))

    # Frame.
    cx, cz = hx * 0.42, hz * 0.5
    sec = [(cx, 0), (cx * 0.6, cz), (-cx * 0.6, cz), (-cx, 0), (-cx * 0.6, -cz), (cx * 0.6, -cz)]
    m.hull([(x * (1.0 if y > 0 else 0.8), y, z) for x, z in sec for y in (-yb, yb)], "dark", bevel=0.01)
    # Servo hubs at both joints.
    for s in (-1, 1):
        y = s * yb
        m.drum((-hx * 0.45, y, 0), (hx * 0.45, y, 0), rb, "metal", 12, hint=(0, 0, 1))
        for sx in (-1, 1):
            m.servo((sx * hx * 0.45, y, 0), (sx * hx * 0.78, y, 0), rb * 0.9, "dark", 8)
    # Front armour: a thick plate curved across, and a second one lapped over its lower half;
    # the back armour the same, shallower.
    top_y = 0.3 if kind == "shin" else 0.62

    def curved(y0, y1, z_in, z_out, w0=0.8, w1=0.8, front=-1):
        pts = []
        for y, w in ((y0, w0), (y1, w1)):
            for x, f in ((0.0, 1.0), (0.4, 0.96), (0.68, 0.82), (0.86, 0.55)):
                for sx in ((1,) if x == 0 else (-1, 1)):
                    pts.append(F(sx * x * w / 0.86, y, front * (z_in + (z_out - z_in) * f)))
            for sx in (-1, 1):
                pts.append(F(sx * w, y, front * z_in))
        return pts

    m.hull(curved(top_y, -0.1, 0.42, 1.0, 0.8, 0.74), "armor", **round_)
    m.hull(curved(0.0, -0.58, 0.45, 1.06, 0.76, 0.68), "armor2", **round_)
    m.hull(curved(0.55, -0.5, 0.42, 0.93, 0.72, 0.66, front=1), "armor", **round_)
    # Rams out along the sides, chrome rods down to the joint below.
    rr = min(hx, hz) * 0.14
    for sx in (-1, 1):
        x = sx * hx * 0.66
        m.ram((x, hy * 0.6, -hz * 0.1), (x, -hy * 0.7, -hz * 0.05), rr, extension=0.55, hint=(0, 0, 1),
              booted=False)
    # Hose bundle down the back, in clamps, out of the armour at both ends.
    zc = hz * 0.98
    for dx in (-1, 0, 1):
        m.cable([(dx * rr * 0.7, hy * 0.72, cz), (dx * rr * 0.7, hy * 0.55, zc),
                 (dx * rr * 0.7, -hy * 0.5, zc), (dx * rr * 0.7, -hy * 0.7, cz)], rr * 0.32)
    for yy in (0.3, -0.25):
        m.clamp(V((0, yy * hy, zc)), Y, Z, rr * 1.1)
    if kind == "forearm":
        # Gauntlet over the wrist drive, a hazard band at its pinch edge.
        m.hull([F(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
            (1.0, -0.2, -0.65), (1.0, -0.2, 0.6), (1.0, -0.9, -0.65), (1.0, -0.9, 0.6), (0.8, -0.05, -0.5),
            (0.8, -0.05, 0.45))], "armor", **round_)
        m.hazard(F(-0.8, -0.8, -0.66), F(0.8, -0.8, -0.66), -Z, hy * 0.08, 0.3)
    if kind == "thigh":
        # Hip armour flaring over the top joint.
        m.hull([F(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
            (1.02, 0.2, -0.7), (1.02, 0.2, 0.55), (1.02, 0.75, -0.7), (1.02, 0.75, 0.55), (0.85, 0.98, -0.55),
            (0.85, 0.98, 0.4))], "armor", **round_)
    if kind == "shin":
        # Knee cap, big and rounded, over the joint above; the knee's pinch edge striped; an
        # ankle guard.
        m.hull(curved(1.04, 0.38, 0.5, 1.12, 0.6, 0.74), "armor2", **round_)
        m.hazard(F(-0.4, 0.33, -1.0), F(0.4, 0.33, -1.0), V((0, -0.2, -1)).normalized(), hy * 0.05, 0.3)
        m.hull([F(sx * x, y, z) for sx in (-1, 1) for x, y, z in (
            (0.95, -0.62, -0.95), (0.95, -0.62, -0.4), (0.95, -0.98, -0.95), (0.95, -0.98, -0.4),
            (0.5, -0.78, -1.12))], "armor", **round_)


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


@builder("frame.gyro", "bastion.gyro")
def gyro(m, h, part):
    """A reaction-wheel unit: a faceted housing with stiffening bands round the wheel."""
    r = h.x
    m.lathe([(0, -r * 0.55), (r * 0.8, -r * 0.55), (r * 0.9, -r * 0.42), (r * 0.9, r * 0.42),
             (r * 0.8, r * 0.55), (0, r * 0.55)], frame(), "dark", 12, angle=20.0)
    for y in (-r * 0.3, 0.0, r * 0.3):
        m.lathe([(r * 0.88, y - 0.03), (r * 0.94, y - 0.03), (r * 0.94, y + 0.03), (r * 0.88, y + 0.03)],
                frame(), "metal", 12, closed=True, angle=20.0)
