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
         "bastion.elbow_back_right", "bastion.engine_mount", "bastion.engine_gimbal",
         "bastion.bay_hinge")
def hinge(m, h, part):
    """A heavy hinge. A faceted mount plate on the parent; two armoured cheeks carry the pin
    down past the pivot (the end node, where the joint turns) to a spur that covers the
    bend; the child's knuckle, an octagonal hub, turns between them. Outside one cheek the
    sealed rotary actuator drives the pin; outside the other the angle encoder reads it."""
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
    t = max(0.06, L * 0.22)
    xo = L - 0.02
    # Mount plate: an elongated octagon, chamfered, under the parent.
    k = R * 0.35
    out = [(-xo + k, -R), (xo - k, -R), (xo, -R + k), (xo, R - k), (xo - k, R), (-xo + k, R),
           (-xo, R - k), (-xo, -R + k)]
    m.plate(out, 0.1, J @ frame((0, H - 0.1, 0), u=(1, 0, 0), w=(0, 1, 0)), "armor2", chamfer=0.04)
    # Blade cheeks: down from the plate, round the pivot to a spur.
    side = [(-R, H - 0.1), (R, H - 0.1), (R * 0.98, R * 0.1), (R * 0.55, -R * 0.82),
            (-R * 0.25, -R * 1.18), (-R * 0.92, -R * 0.42)]
    for s in (-1, 1):
        x = s * (xo - t)
        if s > 0:
            pl = J @ frame((x, 0, 0), u=(0, 0, -1), v=(0, 1, 0))
            pts = [(-z, y) for z, y in side]
        else:
            pl = J @ frame((x, 0, 0), u=(0, 0, 1), v=(0, 1, 0))
            pts = side
        m.plate(pts, t, pl, "armor", chamfer=min(0.05, t * 0.4))
    # Knuckle.
    g = xo - t - 0.02
    m.drum(p(-g, 0, 0), p(g, 0, 0), R * 0.82, "dark", 8, hint=v)
    # Actuator (its output shaft is the pin) and the encoder.
    m.drum(p(L, 0, 0), p(L + 0.26, 0, 0), R * 0.78, "metal", 8, hint=v)
    m.cap(p(L + 0.26, 0, 0), a, R * 0.42)
    m.drum(p(-L, 0, 0), p(-L - 0.09, 0, 0), R * 0.45, "dark", 8, hint=v)
    m.cable([p(-L - 0.05, R * 0.4, 0), p(-L - 0.05, H - 0.15, 0)], 0.025)


# --- Arm segments -----------------------------------------------------------------------------


@builder("bastion.link_front", "bastion.link_back", "bastion.drop_front", "bastion.drop_back")
def beam(m, h, part):
    """An arm segment: a hexagonal spine of dark frame metal between octagonal pin bosses
    (their pins held by retainers), sheathed in armour: two overlapping chevron-ended plates
    over the front with a keel to deflect hits, graphite side plates; along the back a sealed
    actuator (it works the next joint through a crank) runs between two guard blades, fed by
    a cable from the spine."""
    hx, hy, hz = h
    R = min(hz * 0.62, hy * 0.25)
    end = hy - R
    # Spine.
    cx, cz = hx * 0.55, hz * 0.6
    sec = [(cx, 0), (cx * 0.6, cz), (-cx * 0.6, cz), (-cx, 0), (-cx * 0.6, -cz), (cx * 0.6, -cz)]
    m.hull([(x, y, z) for x, z in sec for y in (-end, end)], "dark", bevel=0.01)
    # Pin bosses.
    for s in (-1, 1):
        y = s * end
        m.drum((-hx * 0.98, y, 0), (hx * 0.98, y, 0), R, "dark", 8, hint=(0, 0, 1))
        for sx in (-1, 1):
            m.cap((sx * hx * 0.98, y, 0), X * sx, R * 0.55)
    # Front fairing: two overlapping chevron-ended plates (the upper one over the lower),
    # a keel down the upper one.
    tip = end - R * 1.05
    shoulder = end - R * 1.7
    zf = -hz * 0.62
    split = shoulder * 0.1
    low = [(-hx * 0.8, -shoulder), (0, -tip), (hx * 0.8, -shoulder), (hx * 0.8, split + 0.2),
           (0, split + 0.35), (-hx * 0.8, split + 0.2)]
    m.plate(low, 0.14, frame((0, 0, zf), u=(-1, 0, 0), w=(0, 0, -1)), "armor", chamfer=0.05)
    up = [(-hx * 0.88, split - 0.05), (0, split - 0.3), (hx * 0.88, split - 0.05), (hx * 0.88, shoulder),
          (0, tip), (-hx * 0.88, shoulder)]
    m.plate(up, 0.14, frame((0, 0, zf - 0.1), u=(-1, 0, 0), w=(0, 0, -1)), "armor", chamfer=0.05)
    m.hull([(sx * 0.06, y, zf - 0.24) for sx in (-1, 1) for y in (split + 0.1, shoulder * 0.9)]
           + [(0, y, zf - 0.38) for y in (split + 0.25, shoulder * 0.75)], "armor2", bevel=0.004)
    # Side plates.
    for sx in (-1, 1):
        x = sx * hx * 0.6
        out = [(-hz * 0.55, -shoulder * 0.95), (hz * 0.45, -shoulder * 0.8), (hz * 0.45, shoulder * 0.8),
               (-hz * 0.55, shoulder * 0.95)]
        pl = frame((x, 0, 0), u=(0, 0, -sx), v=(0, 1, 0))
        m.plate([(-z, y) for z, y in out] if sx > 0 else out, hx * 0.28, pl, "armor2", chamfer=0.03)
    # Actuator between guard blades on the back (+Z), and its cable.
    rr = max(0.06, hx * 0.15)
    zr = cz + rr * 1.2
    ya, yb = end * 0.8, -end * 0.78
    m.piston((0, ya, zr), (0, yb, zr), rr, extension=0.45, hint=(1, 0, 0))
    for sx in (-1, 1):
        x = sx * rr * 1.9
        m.hull([(x + dx, y, cz) for dx in (-0.03, 0.03) for y in (-shoulder, shoulder)]
               + [(x, y, zr + rr * 0.9) for y in (-shoulder * 0.85, shoulder * 0.85)], "armor2",
               bevel=0.004)
    m.cable([(0, shoulder * 0.5, cz), (rr * 0.6, ya - rr * 2.0, zr - rr * 0.9)], 0.025)


# The other builders register themselves on import.
import parts_arm  # noqa: E402,F401
import parts_body  # noqa: E402,F401
import parts_shell  # noqa: E402,F401
