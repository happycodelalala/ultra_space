"""Part model builders: one function per model, `builder(m, h, part)` with `m` the
`mech.Model`, `h` the part's half sizes (from its collision shape) and `part` its definition
(attach nodes, joint axis). Every model is an original design, built from what the part does.
"""

import math

from mathutils import Vector

from mech import V, X, Y, Z, along, frame

BUILDERS = {}


def builder(*names):
    def register(f):
        for n in names:
            BUILDERS[n] = f
        return f
    return register


def half_size(part):
    shape = part["shape"]
    if shape["type"] == "box":
        return V([s / 2 for s in shape["size_m"]])
    if shape["type"] == "cylinder":
        return V((shape["radius_m"], shape["length_m"] / 2, shape["radius_m"]))
    if shape["type"] == "shell":
        return V((1, 1, 1))
    r = shape["radius_m"]
    return V((r, r, r))


def nodes(part):
    return {n["id"]: (V(n["position_m"]), V(n["direction"]).normalized())
            for n in part.get("attach_nodes", [])}


def extent(h, d):
    return abs(d.x) * h.x + abs(d.y) * h.y + abs(d.z) * h.z


# --- Joints -----------------------------------------------------------------------------------


@builder("bastion.arm_hinge", "bastion.leg_hinge", "frame.hinge", "bastion.bend_front_left",
         "bastion.bend_front_right", "bastion.bend_back_left", "bastion.bend_back_right",
         "bastion.elbow_front_left", "bastion.elbow_front_right", "bastion.elbow_back_left",
         "bastion.elbow_back_right")
def hinge(m, h, part):
    """A heavy hinge. A flange bolts to the parent; two clevis lugs, pocketed and gusseted,
    come down to the pivot (the end node, where the joint turns); the child's knuckle turns
    between them on a polished pin. On one side a rotary vane actuator, finned and bolted
    shut, is fed by two hoses from a valve block under the flange; on the other an angle
    encoder reports the joint's position."""
    a = V(part["joint"]["axis"]).normalized()
    ns = nodes(part)
    pb, db = ns["base"]
    pivot = ns["end"][0]
    v = db - a * db.dot(a)
    v = v.normalized() if v.length > 1e-6 else a.orthogonal().normalized()
    w = a.cross(v)
    J = frame(pivot, u=a, v=v)

    def p(x, y, z):
        return J @ V((x, y, z))

    L = extent(h, a)
    R = max(0.14, min(0.85 * extent(h, w), 0.6))
    H = max((pb - pivot).dot(v), R * 0.6)
    t = max(0.05, L * 0.24)
    xo = L - 0.02
    # Flange, overhanging on the actuator side to carry the valve block.
    over = R * 0.75
    flange = m.box((2 * L + over, 0.09, 2 * R * 1.12), place=J @ frame((over / 2, H - 0.045, 0)),
                   mat="armor2", bevel=0.015)
    m.groove(flange, p(-L + 0.06, H, -R), p(-L + 0.06, H, R), v, width=0.015, depth=0.01)
    for s in (-1, 1):
        m.bolt_row(p(-L, H - 0.09, s * R * 0.98), p(L + over, H - 0.09, s * R * 0.98), -v,
                   4, size=0.03)
    # Clevis lugs with pockets; gussets on the encoder side.
    for s in (-1, 1):
        x = s * (xo - t / 2)
        lug = m.lug(p(x, H - 0.09, 0), p(x, 0, 0), 2 * R * 0.96, t, a, "armor")
        m.cut_box(lug, (t * 0.5, H * 0.4, R * 0.95), place=J @ frame((s * xo, H * 0.5, 0)))
    for z in (-R * 0.55, R * 0.55):
        m.extrude([(0, 0), (0.14, 0), (0, -0.2)], 0.04,
                  J @ frame((-xo, H - 0.09, z), u=(-1, 0, 0), v=(0, 1, 0)), "armor", 0.008, 1)
    # Knuckle (grooved) and pin.
    g = xo - t - 0.025
    m.lathe([(0, -g), (R * 0.8, -g), (R * 0.9, -g + 0.03), (R * 0.9, -0.06), (R * 0.84, -0.045),
             (R * 0.84, 0.045), (R * 0.9, 0.06), (R * 0.9, g - 0.03), (R * 0.8, g), (0, g)],
            J @ frame((0, 0, 0), u=v, v=(1, 0, 0)), "dark", 32, bevel=0.008)
    m.cyl(p(-L - 0.04, 0, 0), p(L + 0.02, 0, 0), R * 0.26, "chrome", 20)
    m.cyl(p(-L - 0.02, 0, 0), p(-L - 0.06, 0, 0), R * 0.42, "metal", 24, bevel=0.008)
    m.bolts(p(-L - 0.06, 0, 0), -a, R * 0.32, 6, size=0.022, up=v)
    # Vane actuator.
    x0 = L
    ra = R * 0.82
    m.lathe([(0, x0), (ra, x0), (ra, x0 + 0.03), (ra * 1.08, x0 + 0.04), (ra * 1.08, x0 + 0.07),
             (ra, x0 + 0.08), (ra, x0 + 0.1), (ra * 1.08, x0 + 0.11), (ra * 1.08, x0 + 0.14),
             (ra, x0 + 0.15), (ra, x0 + 0.2), (ra * 0.92, x0 + 0.22), (0, x0 + 0.22)],
            J @ frame((0, 0, 0), u=v, v=(1, 0, 0)), "dark", 32, bevel=0.006)
    m.cyl(p(x0 + 0.22, 0, 0), p(x0 + 0.25, 0, 0), ra * 0.9, "metal", 32, bevel=0.006)
    m.bolts(p(x0 + 0.25, 0, 0), a, ra * 0.75, 10, size=0.022, up=v)
    m.cyl(p(x0 + 0.25, 0, 0), p(x0 + 0.29, 0, 0), ra * 0.3, "chrome", 16)
    # Valve block and hoses.
    vb = (L + over * 0.5, H - 0.09 - 0.09, 0)
    m.box((over * 0.7, 0.16, R * 0.9), at=p(*vb), place=J @ frame(vb), mat="metal", bevel=0.01)
    for s in (-1, 1):
        port = p(x0 + 0.11, ra * 0.98, s * ra * 0.35)
        m.cyl(port - v * 0.02, port + v * 0.05, 0.03, "metal", 6)
        start = p(vb[0], vb[1] - 0.08, s * R * 0.3)
        m.cyl(start, start - v * 0.04, 0.03, "metal", 6)
        m.hose([start - v * 0.04, (start + port) / 2 - v * 0.04 + w * s * 0.08, port + v * 0.05],
               0.025)
        top = p(vb[0] + over * 0.25, vb[1], s * R * 0.42)
        m.hose([top, top + v * 0.08 + a * 0.05, p(L + over * 0.9, H + 0.05, s * R * 0.5)], 0.025)
    # Encoder and its cable.
    m.cyl(p(-L - 0.06, R * 0.55, 0), p(-L - 0.13, R * 0.55, 0), R * 0.16, "dark", 16, bevel=0.005)
    m.hose([p(-L - 0.13, R * 0.55, 0), p(-L - 0.16, R * 0.8, 0.05), p(-L - 0.05, H - 0.1, R * 0.3)],
           0.012)


# --- Arm segments -----------------------------------------------------------------------------


@builder("bastion.link_front", "bastion.link_back", "bastion.forearm_front",
         "bastion.forearm_back", "bastion.drop_front", "bastion.drop_back")
def beam(m, h, part):
    """A welded box girder: two side webs with lightening holes, cover plates bolted top
    and bottom, diaphragms inside, turned pin bosses at both ends. Its ram rides on welded
    brackets along the back (it works the next joint through a crank); a pair of hydraulic
    lines run down the front in clamps."""
    hx, hy, hz = h
    R = min(hz * 0.62, hy * 0.25)
    end = hy - R
    tw = max(0.035, hx * 0.1)
    xw = hx * 0.8 - tw / 2
    # Webs with lightening holes.
    span = 2 * end
    holes = max(1, int(span / (2 * hz * 1.25)))
    for s in (-1, 1):
        web = m.box((tw, span + R, 2 * hz * 0.86), at=(s * xw, 0, 0), mat="armor", bevel=0.012)
        for k in range(holes):
            y = -end + span * (k + 0.5) / holes
            m.hole(web, (s * (xw - tw), y, 0), (s * (xw + tw), y, 0), hz * 0.48, 32)
    # Cover plates, bolted.
    tf = max(0.03, hz * 0.07)
    for s in (-1, 1):
        z = s * (hz * 0.86 + tf / 2)
        plate = m.box((2 * hx * 0.86, span + R * 0.6, tf), at=(0, 0, z), mat="armor2", bevel=0.01)
        for y in (-end * 0.5, end * 0.5):
            m.groove(plate, (-hx * 0.86, y, z + s * tf / 2), (hx * 0.86, y, z + s * tf / 2),
                     (0, 0, s), width=0.012, depth=0.008)
        for sx in (-1, 1):
            m.bolt_row((sx * hx * 0.7, -end, z + s * tf / 2), (sx * hx * 0.7, end, z + s * tf / 2),
                       (0, 0, s), max(3, int(span / 0.45)), size=0.022)
    # Diaphragms, seen through the holes.
    for k in range(holes + 1):
        y = -end + span * k / holes
        m.box((2 * (xw - tw / 2), 0.03, 2 * hz * 0.84), at=(0, y, 0), mat="dark", bevel=0)
    # Pin bosses at both ends, with the bore and pin.
    for s in (-1, 1):
        y = s * end
        m.lathe([(R * 0.45, -hx * 0.98), (R, -hx * 0.98), (R, -hx * 0.9), (R * 0.94, -hx * 0.86),
                 (R * 0.94, hx * 0.86), (R, hx * 0.9), (R, hx * 0.98), (R * 0.45, hx * 0.98)],
                frame((0, y, 0), u=(0, 1, 0), v=(1, 0, 0)), "metal", 40, bevel=0.006)
        m.cyl((-hx * 1.02, y, 0), (hx * 1.02, y, 0), R * 0.45, "chrome", 24)
        for sx in (-1, 1):
            m.bolts((sx * hx * 1.02, y, 0), (sx, 0, 0), R * 0.3, 6, size=0.02)
    # Ram on brackets along the back (+Z).
    rr = max(0.06, hx * 0.17)
    zr = hz * 0.86 + tf + rr * 1.3
    ya, yb = end * 0.82, -end * 0.78
    m.ram((0, ya, zr), (0, yb, zr), rr, extension=0.45, hint=(1, 0, 0))
    for y in (ya, yb):
        for sx in (-1, 1):
            m.lug((sx * rr * 0.9, y, hz * 0.86 + tf), (sx * rr * 0.9, y, zr), rr * 1.6, 0.04,
                  (1, 0, 0), "armor2")
    # Hydraulic lines in clamps down the front (-Z).
    zf = -(hz * 0.86 + tf)
    for sx in (-0.35, 0.35):
        x = sx * hx
        m.hose([(x, end, zf - 0.04), (x, 0, zf - 0.045), (x, -end, zf - 0.04)], 0.028)
    for k in range(max(2, int(span / 0.7))):
        y = -end * 0.9 + 1.8 * end * k / max(1, int(span / 0.7) - 1)
        m.box((hx * 1.1, 0.05, 0.03), at=(0, y, zf - 0.075), mat="metal", bevel=0.006)
        m.bolt((0, y, zf - 0.09), (0, 0, -1), 0.018)
    # Data plate and hazard bands near the ends.
    m.box((0.012, 0.18, 0.28), at=(hx * 0.8 + 0.006, end * 0.35, 0), mat="metal", bevel=0.003)
    for s in (-1, 1):
        m.box((2 * hx * 0.9, 0.06, 0.012), at=(0, s * end * 0.62, hz * 0.86 + tf + 0.006),
              mat="trim", bevel=0.003)


# The other builders register themselves on import.
import parts_arm  # noqa: E402,F401
import parts_body  # noqa: E402,F401
import parts_shell  # noqa: E402,F401
