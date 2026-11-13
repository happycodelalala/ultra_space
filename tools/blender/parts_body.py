"""The walking body: torso and cockpit, head, pelvis, shoulders, limbs, feet, hands, and the
heavy frame's extras. Front is -Z, up +Y."""

import math

from mech import V, X, Y, Z, along, frame
from parts import builder, nodes


def side_place(origin=(0, 0, 0)):
    """A placement whose outline plane is the side view (outline x -> Z, y -> Y), extruded
    across X."""
    return frame(origin, u=(0, 0, 1), v=(0, 1, 0))


def louvres(m, centre, out, up, width, height, count, mat="dark"):
    """A louvred grille in a recess: a frame and angled slats, `out` the face normal."""
    c, n, u = V(centre), V(out).normalized(), V(up).normalized()
    a = u.cross(n)
    for k in range(count):
        y = -height / 2 + (k + 0.5) * height / count
        tilt = frame(c + u * y - n * 0.02, u=a, v=(u + n * 0.7).normalized())
        m.box((width, height / count * 0.75, 0.015), place=tilt, mat=mat, bevel=0.003)
    for s in (-1, 1):
        m.box((0.02, height, 0.04), place=frame(c + a * (s * width / 2), u=a, v=u), mat="metal", bevel=0.004)


def rung(m, a, b, out, mat="metal"):
    a, b, n = V(a), V(b), V(out).normalized()
    m.hose([a, a + n * 0.09, b + n * 0.09, b], 0.022, mat)


@builder("bastion.torso", "frame.torso")
def torso(m, h, part):
    """The torso and cockpit. An internal frame carries a sloped chest armour block (panel
    lines, bolted edges) with the crew hatch recessed in it: hinged door, viewport, grab
    handles, hazard stripes. Below, overlapping abdomen lames over the frame, flexed by a
    ram each side. Louvred intakes in the sides, the shoulder bearings, the reactor housing
    with its radiator fins, exhausts and conduits on the back, the neck collar, lamps and a
    boarding ladder."""
    hx, hy, hz = h
    m.box((2 * hx * 0.84, 2 * hy * 0.94, 2 * hz * 0.82), mat="armor2", bevel=0.08)
    chest = m.extrude([(-hz * 0.62, hy * 0.98), (hz * 0.25, hy * 0.98), (hz * 0.25, hy * 0.02),
                       (-hz * 0.74, -hy * 0.06), (-hz * 1.0, hy * 0.18), (-hz * 1.0, hy * 0.66)],
                      2 * hx * 0.94, side_place(), "armor", bevel=0.05, segments=3)
    for sx in (-1, 1):
        m.groove(chest, (sx * hx * 0.5, hy * 0.7, -hz), (sx * hx * 0.5, hy * 0.18, -hz), -Z, 0.02, 0.02)
        m.groove(chest, (sx * hx * 0.94, hy * 0.75, -hz * 0.5), (sx * hx * 0.94, hy * 0.1, -hz * 0.5),
                 X * sx, 0.02, 0.02)
    m.groove(chest, (-hx, hy * 0.98, -hz * 0.2), (hx, hy * 0.98, -hz * 0.2), Y, 0.02, 0.02)
    for sx in (-1, 1):
        m.detail(chest, (sx * hx * 0.72, hy * 0.43, -hz), -Z, Y, hx * 0.4, hy * 0.42, seed=11 + sx,
                 grille=sx > 0)
        m.detail(chest, (sx * hx * 0.94, hy * 0.6, -hz * 0.35), X * sx, Y, hz * 0.8, hy * 0.5, seed=21 + sx)
        m.detail(chest, (sx * hx * 0.5, hy * 0.98, -hz * 0.25), Y, -Z, hx * 0.75, hz * 0.6, seed=31 + sx,
                 hatch=sx < 0)
    # Hatch.
    hc = V((0, hy * 0.42, -hz))
    m.cut_box(chest, (hx * 0.78, hy * 0.4, 0.16), at=hc)
    m.box((hx * 0.72, hy * 0.36, 0.06), at=hc + V((0, 0, 0.05)), mat="armor", bevel=0.02)
    m.box((hx * 0.48, hy * 0.08, 0.04), at=hc + V((0, hy * 0.08, 0.0)), mat="metal", bevel=0.01)
    m.box((hx * 0.42, hy * 0.055, 0.04), at=hc + V((0, hy * 0.08, -0.01)), mat="glass", bevel=0.008)
    for sx in (-1, 1):
        m.cyl(hc + V((sx * hx * 0.2, hy * 0.18, -0.02)), hc + V((sx * hx * 0.32, hy * 0.18, -0.02)),
              0.05, "metal", 12)
        rung(m, hc + V((sx * hx * 0.22, -hy * 0.1, 0.02)), hc + V((sx * hx * 0.22, -hy * 0.02, 0.02)), -Z)
        m.box((0.05, hy * 0.36, 0.012), at=hc + V((sx * hx * 0.43, 0, 0.0)), mat="trim", bevel=0.003)
    # Abdomen lames and flex rams.
    for k in range(4):
        y = -hy * 0.18 - k * hy * 0.19
        tilt = frame((0, y, -hz * 0.84 + k * 0.02), u=X, v=(0, 1, 0.18))
        lame = m.box((2 * hx * (0.66 - 0.035 * k), hy * 0.2, 0.16), place=tilt, mat="armor", bevel=0.03)
        m.bolt_row((-hx * 0.5, y - hy * 0.06, -hz * 0.84 + k * 0.02 - 0.08),
                   (hx * 0.5, y - hy * 0.06, -hz * 0.84 + k * 0.02 - 0.08), -Z, 4, 0.022)
    for sx in (-1, 1):
        m.ram((sx * hx * 0.76, hy * 0.0, -hz * 0.7), (sx * hx * 0.76, -hy * 0.88, -hz * 0.62),
              hx * 0.05, extension=0.35, hint=(0, 0, 1))
    # Side intakes.
    for sx in (-1, 1):
        side = m.box((0.16, hy * 0.8, 2 * hz * 0.72), at=(sx * hx * 0.9, -hy * 0.38, -hz * 0.05),
                     mat="armor", bevel=0.03)
        ic = V((sx * hx * 0.98, -hy * 0.38, -hz * 0.15))
        m.cut_box(side, (0.12, hy * 0.5, hz * 0.9), at=ic)
        louvres(m, ic + V((-sx * 0.0, 0, 0)), X * sx, Y, hz * 0.85, hy * 0.46, 6)
        m.bolt_row((sx * (hx * 0.98), -hy * 0.75, -hz * 0.7), (sx * (hx * 0.98), -hy * 0.75, hz * 0.6),
                   X * sx, 6, 0.022)
    # Shoulder bearings.
    ys = hy * 0.57
    for sx in (-1, 1):
        m.lathe([(0, hx * 0.78), (hy * 0.34, hx * 0.78), (hy * 0.34, hx * 0.9), (hy * 0.3, hx * 0.92),
                 (hy * 0.3, hx * 1.0), (0, hx * 1.0)],
                frame((0, ys, 0), u=(0, 0, 1), v=(sx, 0, 0)), "dark", 40, bevel=0.01)
        m.bolts((sx * hx * 1.0, ys, 0), X * sx, hy * 0.24, 10, size=0.025)
    # Back: reactor, radiator, exhausts, conduits.
    rb = m.box((2 * hx * 0.62, hy * 1.0, hz * 0.22), at=(0, -hy * 0.05, hz * 0.8), mat="armor2", bevel=0.04)
    for k in range(13):
        x = -hx * 0.54 + k * hx * 1.08 / 12
        m.box((0.025, hy * 0.8, hz * 0.09), at=(x, -hy * 0.05, hz * 0.94), mat="dark", bevel=0.003)
    for sx in (-1, 1):
        c = V((sx * hx * 0.45, -hy * 0.72, hz * 0.88))
        m.lathe([(0.0, 0.0), (hy * 0.1, 0.0), (hy * 0.1, 0.06), (hy * 0.08, 0.1), (hy * 0.065, 0.1),
                 (hy * 0.065, 0.02), (0.0, 0.02)], frame(c, u=X, v=(0, 0, 1)), "metal", 24)
        m.hose([V((sx * hx * 0.62, hy * 0.2, hz * 0.85)), V((sx * hx * 0.78, hy * 0.25, hz * 0.7)),
                V((sx * hx * 0.84, hy * 0.45, hz * 0.3))], 0.045, "metal")
    m.box((hx * 0.5, 0.04, 0.012), at=(0, hy * 0.5, hz * 0.92), mat="trim", bevel=0.003)
    # Collar, lamps, ladder.
    m.lathe([(hx * 0.36, hy * 0.92), (hx * 0.46, hy * 0.92), (hx * 0.46, hy * 0.98), (hx * 0.42, hy * 1.02),
             (hx * 0.36, hy * 1.02)], frame(), "metal", 48, closed=True)
    m.bolts((0, hy * 1.02, 0), Y, hx * 0.41, 12, size=0.025)
    for sx in (-1, 1):
        c = V((sx * hx * 0.78, hy * 0.86, -hz * 0.78))
        m.box((0.16, 0.08, 0.06), at=c, mat="metal", bevel=0.01)
        m.box((0.12, 0.05, 0.02), at=c + V((0, 0, -0.035)), mat="light", bevel=0.005)
    for k in range(6):
        y = -hy * 0.92 + k * hy * 0.16
        rung(m, (hx * 0.62, y, -hz * 0.9), (hx * 0.82, y, -hz * 0.9), -Z)


@builder("bastion.head", "frame.head")
def head(m, h, part):
    """The sensor head on its neck bearing: an armoured housing with a recessed visor band,
    the main optic in a bolted bezel, side sensor pods, a sensor mast with a small dish and
    antenna, louvred vents at the back, cheek plates."""
    hx, hy, hz = h
    m.lathe([(0, -hy), (hx * 0.62, -hy), (hx * 0.62, -hy * 0.86), (hx * 0.52, -hy * 0.82),
             (hx * 0.52, -hy * 0.7), (0, -hy * 0.7)], frame(), "dark", 40, bevel=0.008)
    m.bolts((0, -hy * 0.86, 0), Y, hx * 0.56, 10, size=0.02)
    body = m.box((2 * hx * 0.86, hy * 1.3, 2 * hz * 0.86), at=(0, -hy * 0.05, 0), mat="armor",
                 bevel=0.06, segments=3, taper=(0.82, 0.86))
    vy = hy * 0.14
    m.cut_box(body, (hx * 1.5, hy * 0.2, hz * 0.5), at=(0, vy, -hz * 0.82))
    m.box((hx * 1.42, hy * 0.15, 0.05), at=(0, vy, -hz * 0.66), mat="glass", bevel=0.01)
    m.groove(body, (-hx, hy * 0.42, -hz), (hx, hy * 0.42, -hz), -Z, 0.015, 0.015)
    lens = V((0, -hy * 0.32, -hz * 0.8))
    m.lathe([(0, 0.0), (hx * 0.28, 0.0), (hx * 0.28, 0.1), (hx * 0.22, 0.14), (hx * 0.18, 0.14),
             (hx * 0.18, 0.08), (0, 0.08)], frame(lens, u=X, v=(0, 0, -1)), "metal", 32, bevel=0.004)
    m.cyl(lens + V((0, 0, -0.07)), lens + V((0, 0, -0.1)), hx * 0.17, "glass", 32)
    m.bolts(lens + V((0, 0, -0.1)), -Z, hx * 0.23, 6, size=0.014)
    for sx in (-1, 1):
        c = V((sx * hx * 0.82, -hy * 0.08, hz * 0.05))
        m.lathe([(0, 0), (hy * 0.24, 0), (hy * 0.24, hx * 0.18), (hy * 0.18, hx * 0.24), (0, hx * 0.24)],
                frame(c, u=(0, 0, 1), v=(sx, 0, 0)), "dark", 24, bevel=0.005)
        m.cyl(c + V((sx * hx * 0.24, 0, 0)), c + V((sx * hx * 0.26, 0, 0)), hy * 0.14, "glass", 24)
        m.extrude([(-hz * 0.5, -hy * 0.5), (hz * 0.3, -hy * 0.5), (hz * 0.5, -hy * 0.1), (hz * 0.2, hy * 0.2),
                   (-hz * 0.4, hy * 0.2)], 0.05, side_place((sx * hx * 0.78, -hy * 0.1, 0)), "armor2", 0.012)
    mast = V((hx * 0.42, hy * 0.58, hz * 0.4))
    m.cyl(mast, mast + Y * hy * 0.25, 0.05, "metal", 12)
    m.lathe([(0.0, 0.0), (0.18, 0.06), (0.2, 0.07), (0.02, 0.03)], frame(mast + Y * hy * 0.25, u=X, v=(0, 0.6, -0.8)),
            "metal", 24, closed=True)
    m.cyl(mast + V((-0.15, 0, 0)), mast + V((-0.15, hy * 0.7, 0)), 0.012, "dark", 6)
    louvres(m, (0, hy * 0.05, hz * 0.76), Z, Y, hx * 1.0, hy * 0.4, 5)
    m.box((hx * 0.3, 0.03, 0.01), at=(0, hy * 0.48, -hz * 0.62), mat="trim", bevel=0.002)


@builder("bastion.waist", "frame.waist")
def waist(m, h, part):
    """The pelvis: the slewing ring the torso turns on (gear teeth, bolted race), the
    pelvis frame, both hip actuator housings with bolted flanges, hinged skirt plates front
    and back and tassets at the sides, cable runs."""
    hx, hy, hz = h
    rr = min(hx, hz) * 0.82
    m.lathe([(0, hy * 0.55), (rr, hy * 0.55), (rr, hy * 0.75), (rr * 0.92, hy * 0.8), (rr * 0.92, hy),
             (0, hy)], frame(), "dark", 56, bevel=0.01)
    m.bolts((0, hy, 0), Y, rr * 0.8, 16, size=0.025)
    for k in range(48):
        t = 2 * math.pi * k / 48
        d = V((math.sin(t), 0, math.cos(t)))
        m.box((0.05, hy * 0.18, 0.06), place=frame(d * rr + Y * hy * 0.65, u=d.cross(Y), v=Y), mat="metal",
              bevel=0)
    m.box((2 * hx * 0.62, hy * 1.2, 2 * hz * 0.72), at=(0, -hy * 0.05, 0), mat="armor2", bevel=0.06)
    for sx in (-1, 1):
        x = sx * hx * 0.67
        m.lathe([(0, -hx * 0.27), (hy * 0.55, -hx * 0.27), (hy * 0.55, -hx * 0.2), (hy * 0.5, -hx * 0.18),
                 (hy * 0.5, hx * 0.18), (hy * 0.55, hx * 0.2), (hy * 0.55, hx * 0.27), (0, hx * 0.27)],
                frame((x, -hy * 0.45, 0), u=(0, 0, 1), v=(1, 0, 0)), "dark", 40, bevel=0.008)
        for s in (-1, 1):
            m.bolts((x + s * hx * 0.27, -hy * 0.45, 0), X * s, hy * 0.42, 10, size=0.022)
    for sz in (-1, 1):
        hinge_y = -hy * 0.15
        m.cyl((-hx * 0.4, hinge_y, sz * hz * 0.96), (hx * 0.4, hinge_y, sz * hz * 0.96), 0.07, "metal", 16)
        sk = m.extrude([(-hx * 0.42, 0), (hx * 0.42, 0), (hx * 0.34, -hy * 1.1), (-hx * 0.34, -hy * 1.1)], 0.1,
                       frame((0, hinge_y, sz * hz * 1.0), u=X, v=(0, 1, sz * 0.25)), "armor", 0.025)
        m.groove(sk, (-hx * 0.3, hinge_y - hy * 0.55, sz * (hz * 1.14)), (hx * 0.3, hinge_y - hy * 0.55, sz * (hz * 1.14)),
                 Z * sz, 0.015, 0.015)
        m.box((hx * 0.2, 0.05, 0.012), at=(0, hinge_y - hy * 0.25, sz * hz * 1.09), mat="trim", bevel=0.003)
    for sx in (-1, 1):
        m.extrude([(-hz * 0.5, 0), (hz * 0.5, 0), (hz * 0.4, -hy * 0.9), (-hz * 0.4, -hy * 0.9)], 0.08,
                  side_place((sx * hx * 0.98, hy * 0.25, 0)), "armor", 0.02)
        m.hose([(sx * hx * 0.3, hy * 0.5, hz * 0.7), (sx * hx * 0.5, 0, hz * 0.8), (sx * hx * 0.6, -hy * 0.3, hz * 0.45)],
               0.035)


@builder("bastion.shoulder", "frame.shoulder")
def shoulder(m, h, part):
    """A shoulder block: the bearing that turns it on the torso, layered pauldron lames over
    the top, the arm's swing actuator housing below with its manifold and hoses."""
    hx, hy, hz = h
    m.lathe([(0, -hx), (hy * 0.6, -hx), (hy * 0.6, -hx * 0.85), (hy * 0.55, -hx * 0.8), (hy * 0.55, hx * 0.8),
             (hy * 0.6, hx * 0.85), (hy * 0.6, hx), (0, hx)], frame(u=(0, 0, 1), v=(1, 0, 0)), "dark", 40,
            bevel=0.008)
    for s in (-1, 1):
        m.bolts((s * hx, 0, 0), X * s, hy * 0.45, 10, size=0.024)
    m.box((2 * hx * 0.7, 2 * hy * 0.7, 2 * hz * 0.72), mat="armor2", bevel=0.06)
    for k, (r0, y0) in enumerate(((hx * 1.05, hy * 0.05), (hx * 0.92, hy * 0.28), (hx * 0.78, hy * 0.5))):
        pts = []
        for j in range(13):
            t = math.pi * j / 12
            pts.append((r0 * math.cos(t), y0 + r0 * 0.55 * math.sin(t)))
        for j in range(12, -1, -1):
            t = math.pi * j / 12
            pts.append(((r0 - 0.1) * math.cos(t), y0 + (r0 - 0.1) * 0.55 * math.sin(t) - 0.02))
        lame = m.extrude(pts, 2 * hz * (1.0 - 0.06 * k), frame(), "armor", bevel=0.02)
        m.bolt((r0 * 0.0, y0 + r0 * 0.55 + 0.0, -hz * (0.9 - 0.06 * k)), (0, 0.3, -1), 0.025)
    m.box((hx * 1.6, 0.03, 0.012), at=(0, hy * 0.35, -hz * 1.0), mat="trim", bevel=0.003)
    m.lathe([(0, -hy * 0.4), (hx * 0.5, -hy * 0.4), (hx * 0.5, -hy * 0.85), (hx * 0.42, -hy), (0, -hy)],
            frame(), "dark", 36, bevel=0.008)
    mv = V((hx * 0.55, -hy * 0.55, hz * 0.4))
    m.box((0.12, 0.25, 0.3), at=mv, mat="metal", bevel=0.01)
    m.hose([mv + V((0.06, 0.1, 0)), V((hx * 0.6, hy * 0.2, hz * 0.6)), V((hx * 0.2, hy * 0.5, hz * 0.7))], 0.03)


def limb_body(m, h, knee):
    hx, hy, hz = h
    m.box((2 * hx * 0.55, 2 * hy * 0.84, 2 * hz * 0.55), mat="armor2", bevel=0.05)
    rb = min(hx, hz) * 0.5
    for s in (-1, 1):
        y = s * hy * 0.84
        m.lathe([(0, -hx * 0.8), (rb, -hx * 0.8), (rb, -hx * 0.7), (rb * 0.94, -hx * 0.66), (rb * 0.94, hx * 0.66),
                 (rb, hx * 0.7), (rb, hx * 0.8), (0, hx * 0.8)], frame((0, y, 0), u=(0, 0, 1), v=(1, 0, 0)),
                "metal", 40, bevel=0.006)
        for sx in (-1, 1):
            m.cyl((sx * hx * 0.8, y, 0), (sx * hx * 0.86, y, 0), rb * 0.55, "metal", 24, bevel=0.005)
            m.bolts((sx * hx * 0.86, y, 0), X * sx, rb * 0.4, 6, size=0.02)
    front = m.extrude([(-hz * 0.5, hy * 0.72), (-hz * 0.98, hy * 0.56), (-hz * 0.98, -hy * 0.5),
                       (-hz * 0.5, -hy * 0.68)], 2 * hx * 0.84, side_place(), "armor", bevel=0.04, segments=3)
    m.detail(front, (0, 0.0, -hz * 0.98), -Z, Y, hx * 1.5, hy * 0.95, seed=int(hy * 100), grille=False)
    m.box((hx * 1.6, 0.05, 0.012), at=(0, hy * 0.5, -hz * 0.985), mat="trim", bevel=0.003)
    for sx in (-1, 1):
        side = m.box((0.1, hy * 1.2, 2 * hz * 0.72), at=(sx * hx * 0.76, hy * 0.05, -hz * 0.05), mat="armor",
                     bevel=0.025)
        vc = V((sx * hx * 0.81, -hy * 0.3, -hz * 0.1))
        m.cut_box(side, (0.08, hy * 0.3, hz * 0.8), at=vc)
        louvres(m, vc, X * sx, Y, hz * 0.75, hy * 0.28, 4)
        m.detail(side, (sx * hx * 0.81, hy * 0.2, -hz * 0.05), X * sx, Y, hz * 1.3, hy * 0.6,
                 seed=int(hy * 10) + sx, grille=False)
        m.bolt_row((sx * hx * 0.81, -hy * 0.53, -hz * 0.65), (sx * hx * 0.81, -hy * 0.53, hz * 0.55), X * sx, 4,
                   0.02)
    rr = min(hx, hz) * 0.13
    zr = hz * 0.55 + rr * 1.4
    for sx in (-1, 1):
        x = sx * hx * 0.32
        m.ram((x, hy * 0.62, zr), (x, -hy * 0.72, zr), rr, extension=0.42, hint=(1, 0, 0))
        for y in (hy * 0.62, -hy * 0.72):
            m.lug((x, y, hz * 0.55), (x, y, zr), rr * 1.8, 0.05, X, "armor2")
    m.hose([(-hx * 0.62, hy * 0.75, hz * 0.4), (-hx * 0.68, 0, hz * 0.45), (-hx * 0.62, -hy * 0.75, hz * 0.4)],
           0.035)
    m.hose([(-hx * 0.55, hy * 0.75, hz * 0.45), (-hx * 0.6, 0, hz * 0.52), (-hx * 0.55, -hy * 0.75, hz * 0.45)],
           0.025)
    if knee:
        m.extrude([(-hz * 0.4, -hy * 0.45), (-hz * 1.1, -hy * 0.55), (-hz * 1.05, -hy * 0.95),
                   (-hz * 0.5, -hy * 1.02)], 2 * hx * 0.7, side_place(), "armor", bevel=0.04, segments=3)
        m.bolt((0, -hy * 0.75, -hz * 1.09), (0, -0.15, -1), 0.04)


@builder("bastion.limb", "frame.limb")
def limb(m, h, part):
    """An arm segment: a structural spine between turned pin bosses, a sloped front armour
    block with panel lines, bolted side plates with louvred vents, twin rams on lugs down
    the back that work the joint below, hose runs."""
    limb_body(m, h, knee=False)


@builder("bastion.leg", "frame.leg")
def leg(m, h, part):
    """A leg segment: as an arm segment, heavier, with a knee guard at the bottom front."""
    limb_body(m, h, knee=True)


@builder("bastion.foot", "frame.foot")
def foot(m, h, part):
    """A foot: a rubber-treaded sole on a steel sole plate, two hinged toe plates and a heel
    plate, the domed ankle shroud, shock struts from the shroud to toes and heel, side armour
    and a hazard edge."""
    hx, hy, hz = h
    m.box((2 * hx, hy * 0.3, 2 * hz), at=(0, -hy * 0.85, 0), mat="rubber", bevel=0.03)
    for k in range(9):
        z = -hz * 0.88 + k * hz * 1.76 / 8
        for sx in (-1, 1):
            m.box((hx * 0.8, 0.06, hz * 0.1), at=(sx * hx * 0.45, -hy * 1.02, z), mat="rubber", bevel=0.01)
    m.box((2 * hx * 0.96, hy * 0.25, 2 * hz * 0.96), at=(0, -hy * 0.58, 0), mat="dark", bevel=0.03)
    for sx in (-1, 1):
        toe = m.extrude([(-hz, -hy * 0.46), (-hz * 0.25, -hy * 0.46), (-hz * 0.25, hy * 0.05), (-hz * 0.6, hy * 0.05),
                         (-hz * 0.98, -hy * 0.25)], hx * 0.94, side_place((sx * hx * 0.5, 0, 0)), "armor",
                        bevel=0.035, segments=3)
        m.groove(toe, (sx * hx * 0.5 - hx * 0.47, -hy * 0.1, -hz * 0.45), (sx * hx * 0.5 + hx * 0.47, -hy * 0.1, -hz * 0.45),
                 Y, 0.015, 0.012)
    m.cyl((-hx * 0.98, -hy * 0.15, -hz * 0.22), (hx * 0.98, -hy * 0.15, -hz * 0.22), 0.07, "metal", 16)
    m.extrude([(hz * 0.3, -hy * 0.46), (hz * 1.0, -hy * 0.46), (hz * 0.95, -hy * 0.1), (hz * 0.4, hy * 0.15)],
              2 * hx * 0.8, side_place(), "armor", bevel=0.035, segments=3)
    m.lathe([(0, hy), (hx * 0.4, hy), (hx * 0.45, hy * 0.85), (hx * 0.62, hy * 0.2), (hx * 0.66, -hy * 0.45),
             (0, -hy * 0.45)], frame(), "armor2", 40, bevel=0.01)
    m.bolts((0, hy * 0.62, 0), (0, 1, 0), hx * 0.5, 10, size=0.025)
    for sx in (-1, 1):
        for sz, zt in ((-1, -hz * 0.55), (1, hz * 0.6)):
            m.ram((sx * hx * 0.55, hy * 0.35, sz * hz * 0.15), (sx * hx * 0.7, -hy * 0.35, zt), 0.06,
                  extension=0.35, hint=(1, 0, 0), ports=False)
        m.box((0.06, hy * 0.35, hz * 1.2), at=(sx * hx * 0.99, -hy * 0.6, 0), mat="armor", bevel=0.015)
        m.bolt_row((sx * hx * 1.02, -hy * 0.6, -hz * 0.5), (sx * hx * 1.02, -hy * 0.6, hz * 0.5), X * sx, 4, 0.022)
    m.box((2 * hx * 0.95, 0.04, 0.03), at=(0, -hy * 0.7, -hz * 1.0), mat="trim", bevel=0.005)


@builder("bastion.hand")
def hand(m, h, part):
    """A heavy hand: the wrist bearing, a palm frame with an armoured back and knuckle guard,
    three fingers in a row (three phalanges each, rubber pads, a small ram on the back of
    each proximal) curling toward the palm side (+X) and an opposed thumb. Symmetric front
    to back, so one part serves both arms."""
    hx, hy, hz = h
    m.lathe([(0, hy), (hx * 0.62, hy), (hx * 0.62, hy * 0.85), (hx * 0.55, hy * 0.8), (hx * 0.55, hy * 0.62),
             (0, hy * 0.62)], frame(), "dark", 36, bevel=0.006)
    m.bolts((0, hy * 0.85, 0), Y, hx * 0.57, 8, size=0.018)
    m.box((hx * 1.0, hy * 0.62, 2 * hz * 0.82), at=(0, hy * 0.3, 0), mat="armor2", bevel=0.04)
    back = m.extrude([(-hz * 0.85, hy * 0.6), (hz * 0.85, hy * 0.6), (hz * 0.8, -hy * 0.02), (-hz * 0.8, -hy * 0.02)],
                     0.1, frame((-hx * 0.55, 0, 0), u=(0, 0, 1), v=(0, 1, 0)), "armor", bevel=0.02)
    m.detail(back, (-hx * 0.6, hy * 0.3, 0), -X, Y, hz * 1.5, hy * 0.55, seed=5, grille=False)
    m.cyl((0, 0, -hz * 0.9), (0, 0, hz * 0.9), hx * 0.2, "dark", 20)
    seg = hx * 0.2
    for z in (-hz * 0.58, 0.0, hz * 0.58):
        pts = [V((-hx * 0.28, 0, z)), V((-hx * 0.36, -hy * 0.38, z)), V((-hx * 0.08, -hy * 0.68, z)),
               V((hx * 0.3, -hy * 0.85, z))]
        for i, (p, q) in enumerate(zip(pts, pts[1:])):
            pl = along(p, q, hint=Z)
            ln = (q - p).length
            w = seg * (1.0 - 0.12 * i)
            m.box((w * 2, ln * 0.92, hz * 0.42), place=pl @ frame((0, ln / 2, 0)), mat="armor", bevel=0.025,
                  taper=(0.9, 0.9))
            m.cyl(q - Z * hz * 0.18, q + Z * hz * 0.18, w * 0.75, "dark", 16)
            pad = pl @ frame((w * 0.95, ln / 2, 0))
            m.box((0.04, ln * 0.6, hz * 0.32), place=pad, mat="rubber", bevel=0.01)
        m.ram(pts[0] + V((-seg * 1.3, hy * 0.25, 0)), pts[0].lerp(pts[1], 0.6) + V((-seg * 1.1, 0, 0)), 0.03,
              extension=0.35, hint=Z, ports=False)
    thumb = [V((hx * 0.42, hy * 0.15, 0)), V((hx * 0.62, -hy * 0.25, 0)), V((hx * 0.42, -hy * 0.58, 0))]
    m.cyl(thumb[0] - Z * hz * 0.2, thumb[0] + Z * hz * 0.2, seg * 0.9, "dark", 16)
    for p, q in zip(thumb, thumb[1:]):
        pl = along(p, q, hint=Z)
        ln = (q - p).length
        m.box((seg * 2.1, ln * 0.92, hz * 0.45), place=pl @ frame((0, ln / 2, 0)), mat="armor", bevel=0.025)
        m.cyl(q - Z * hz * 0.2, q + Z * hz * 0.2, seg * 0.7, "dark", 16)


@builder("frame.backpack")
def backpack(m, h, part):
    """A backpack: an armoured housing with louvred radiators, two propellant tanks on the
    sides, two small thrust nozzles under it, conduits and a bolted mount."""
    hx, hy, hz = h
    body = m.box((2 * hx * 0.9, 2 * hy * 0.92, 2 * hz * 0.85), mat="armor", bevel=0.06, segments=3)
    m.cut_box(body, (hx * 1.2, hy * 0.9, 0.12), at=(0, hy * 0.2, hz * 0.85))
    louvres(m, (0, hy * 0.2, hz * 0.8), Z, Y, hx * 1.15, hy * 0.85, 7)
    for sx in (-1, 1):
        c = V((sx * hx * 0.98, 0, 0))
        m.lathe([(0, -hy * 0.8), (hz * 0.3, -hy * 0.75), (hz * 0.42, -hy * 0.6), (hz * 0.42, hy * 0.6),
                 (hz * 0.3, hy * 0.75), (0, hy * 0.8)], frame(c), "dark", 24, bevel=0.006)
        for y in (-hy * 0.4, hy * 0.4):
            m.lathe([(hz * 0.42, y - 0.04), (hz * 0.46, y - 0.03), (hz * 0.46, y + 0.03), (hz * 0.42, y + 0.04)],
                    frame(c), "metal", 24, closed=True)
        n = V((sx * hx * 0.45, -hy * 0.92, 0))
        m.lathe([(hx * 0.12, 0.0), (hx * 0.18, -0.05), (hx * 0.3, -0.3), (hx * 0.28, -0.3), (hx * 0.16, -0.06),
                 (hx * 0.1, 0.0)], frame(n), "dark", 24, closed=True)
    m.box((hx * 0.5, 0.04, 0.012), at=(0, hy * 0.8, hz * 0.86), mat="trim", bevel=0.003)
    m.bolt_row((-hx * 0.8, -hy * 0.85, -hz * 0.86), (hx * 0.8, -hy * 0.85, -hz * 0.86), -Z, 6, 0.025)


@builder("frame.binder")
def binder(m, h, part):
    """A binder: an armour plate with a chamfered outline, raised edge bands, panel lines
    and hazard stripes, on a mounting bracket."""
    hx, hy, hz = h
    out = [(-hz * 0.7, -hy), (hz * 0.7, -hy), (hz, -hy * 0.7), (hz, hy * 0.75), (hz * 0.6, hy),
           (-hz * 0.6, hy), (-hz, hy * 0.75), (-hz, -hy * 0.7)]
    plate = m.extrude(out, 2 * hx * 0.6, side_place(), "armor", bevel=0.04, segments=3)
    for y in (-hy * 0.3, hy * 0.35):
        m.groove(plate, (hx, y, -hz), (hx, y, hz), X, 0.02, 0.015)
        m.groove(plate, (-hx, y, -hz), (-hx, y, hz), -X, 0.02, 0.015)
    for k in range(5):
        z = -hz * 0.5 + k * hz * 0.25
        m.box((0.012, hy * 0.18, 0.07), place=frame((hx * 0.61, -hy * 0.8, z), u=(0, 0, 1), v=(0, 1, 0.6)),
              mat="trim", bevel=0.002)
    m.box((2 * hx, hy * 0.5, hz * 0.5), mat="dark", bevel=0.03)
    m.bolts((hx, 0, 0), X, hz * 0.18, 6, size=0.025)


@builder("frame.drone_pod")
def drone_pod(m, h, part):
    """A drone pod: a turned launch tube with a bolted breech, latches and a hatch lip."""
    hx, hy, hz = h
    r = min(hx, hy)
    m.lathe([(0, -hz), (r * 0.8, -hz), (r * 0.9, -hz * 0.85), (r * 0.9, hz * 0.8), (r * 0.95, hz * 0.85),
             (r * 0.95, hz), (r * 0.7, hz), (r * 0.7, hz * 0.9), (0, hz * 0.9)],
            frame(u=(1, 0, 0), v=(0, 0, 1)), "armor", 32, bevel=0.01)
    m.bolts((0, 0, -hz), -Z, r * 0.6, 8, size=0.025)
    for k in range(4):
        t = math.pi / 4 + math.pi / 2 * k
        p = V((math.cos(t) * r * 0.92, math.sin(t) * r * 0.92, hz * 0.7))
        m.box((0.06, 0.06, 0.18), at=p, mat="metal", bevel=0.01)


@builder("frame.gyro")
def gyro(m, h, part):
    """A reaction-wheel unit: a turned, finned housing with bolted covers."""
    r = h.x
    m.lathe([(0, -r * 0.55), (r * 0.85, -r * 0.55), (r * 0.9, -r * 0.45), (r * 0.9, r * 0.45),
             (r * 0.85, r * 0.55), (0, r * 0.55)], frame(), "dark", 32, bevel=0.01)
    for y in (-r * 0.3, 0.0, r * 0.3):
        m.lathe([(r * 0.9, y - 0.02), (r * 0.98, y - 0.015), (r * 0.98, y + 0.015), (r * 0.9, y + 0.02)],
                frame(), "metal", 32, closed=True)
    m.bolts((0, r * 0.55, 0), Y, r * 0.6, 8, size=0.03)
