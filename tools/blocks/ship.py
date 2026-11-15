"""A landed ship on its own grid (block-world.md section 5), designed for the shape set: a 25 cm
hull whose nose narrows in plan only under a flat top (a hip-roof wedge), chamfered edges, a
raised bubble canopy and a dorsal spine, swept wings with nacelles beneath; 12.5 cm detail
(twin fins, canopy frame, nozzles, lights, markings, legs and a 45-degree ramp)."""
from core import Vox, UP_Z, UP_ZZ, X, Y


def cells(v, x0, x1, y0, y1, z0, z1):
    for i in v.r(x0, x1):
        for j in v.r(y0, y1):
            for k in v.r(z0, z1):
                c = v.centre(i, j, k)
                yield i, j, k, c.x, c.y, c.z


def ship(x, y, z, rot):
    h = Vox(0.25, UP_ZZ + [(X, 1), (X, -1)]); d = Vox(0.125, UP_Z + [(Y, 1), (Y, -1)])
    # Hull: 1 m half-width, z 1.0 to 2.25; the nose narrows at 45 degrees in plan.
    for i, j, k, cx, cy, cz in cells(h, -1.0, 1.0, -3.75, 3.0, 1.0, 2.25):
        t = cy + 3.75; hw = min(1.0, 0.25 + t)
        if abs(cx) > hw or abs(cx) + (cz - 1.0) > hw + 0.75 or abs(cx) - (cz - 1.0) > hw - 0.25:
            continue
        side = abs(cx) > hw - 0.25
        if t < 0.5 or cy > 2.75:
            col = "hulldark"
        elif side and 1.5 < cz < 1.75 and -2.5 < cy < 2.25:
            col = "accent"
        elif cz < 1.25:
            col = "hullmid"
        elif cz < 1.5:
            col = "hull2"
        else:
            col = "hull"
        h.v[i, j, k] = col
    # Canopy: rises at 45 degrees from the nose, a narrower top course.
    for i, j, k, cx, cy, cz in cells(h, -0.5, 0.5, -2.5, -0.75, 2.25, 2.75):
        if cz - 2.25 > cy + 2.5 or (cz > 2.5 and abs(cx) > 0.25):
            continue
        h.v[i, j, k] = "canopy"
    # Dorsal spine with intake slots.
    for i, j, k, cx, cy, cz in cells(h, -0.5, 0.5, -0.75, 2.5, 2.25, 2.5):
        h.v[i, j, k] = "hulldark" if 0.0 < cy < 0.5 else "hull"
    for sd in (-1, 1):
        # Wings: swept at 45 degrees, orange tips.
        for i, j, k, cx, cy, cz in cells(h, 1.0 if sd > 0 else -3.25, 3.25 if sd > 0 else -1.0, -0.75, 2.0, 1.25, 1.5):
            if cy < -0.5 + (abs(cx) - 1.0):
                continue
            h.v[i, j, k] = "accent" if abs(cx) > 2.75 else ("hull2" if 0.75 < cy < 1.0 else "hull")
        # Nacelles under the wings.
        for i, j, k, cx, cy, cz in cells(h, sd * 2.0 - 0.5, sd * 2.0 + 0.5, 0.0, 2.75, 0.5, 1.25):
            if abs(cx - sd * 2.0) > 0.375 or abs(cz - 0.875) > 0.375 or abs(cx - sd * 2.0) + abs(cz - 0.875) > 0.5:
                continue
            h.v[i, j, k] = "hulldark" if cy < 0.25 or cy > 2.5 else "hull2"
        for i, j, k, cx, cy, cz in cells(d, sd * 2.0 - 0.5, sd * 2.0 + 0.5, -0.125, 0.0, 0.375, 1.375):
            if abs(cx - sd * 2.0) + abs(cz - 0.875) <= 0.625 and (abs(cx - sd * 2.0) > 0.25 or abs(cz - 0.875) > 0.25):
                d.v[i, j, k] = "steel2"
        for i, j, k, cx, cy, cz in cells(d, sd * 2.0 - 0.25, sd * 2.0 + 0.25, 2.75, 2.875, 0.625, 1.125):
            d.v[i, j, k] = "engine"
        # Twin fins, swept at 45 degrees, with a light on top.
        fx0, fx1 = sorted([sd * 0.625, sd * 0.75])
        for i, j, k, cx, cy, cz in cells(d, fx0, fx1, 1.5, 3.0, 2.25, 3.25):
            if cz - 2.25 <= cy - 1.5:
                d.v[i, j, k] = "accent" if cz > 3.0 else "hull"
        # Wing-tip lights, flank markings, a hatch, legs with pads.
        tx0, tx1 = sorted([sd * 3.25, sd * 3.375])
        for i, j, k, cx, cy, cz in cells(d, tx0, tx1, 1.25, 1.75, 1.25, 1.5):
            d.v[i, j, k] = "red" if sd < 0 else "green"
        mx0, mx1 = sorted([sd * 1.0, sd * 1.125])
        for (z0, w) in [(1.75, 0.375), (1.875, 0.125), (2.0, 0.375)]:
            for i, j, k, cx, cy, cz in cells(d, mx0, mx1, 0.5, 0.5 + w, z0, z0 + 0.125):
                d.v[i, j, k] = "hulldark"
        for i, j, k, cx, cy, cz in cells(d, mx0, mx1, -1.5, -1.0, 1.25, 1.5):
            d.v[i, j, k] = "hulldark"
        for t in (-2.25, 1.25):
            lx = sd * 0.625
            for i, j, k, cx, cy, cz in cells(d, lx - 0.125, lx + 0.125, t - 0.125, t + 0.125, 0.125, 1.0):
                d.v[i, j, k] = "hullmid"
            for i, j, k, cx, cy, cz in cells(d, lx - 0.25, lx + 0.25, t - 0.25, t + 0.25, 0.0, 0.125):
                d.v[i, j, k] = "hulldark"
    # Main engines in the tail, canopy frame, a glint, a mast, the ramp down at 45 degrees.
    for i, j, k, cx, cy, cz in cells(d, -0.625, 0.625, 3.0, 3.125, 1.375, 1.875):
        d.v[i, j, k] = "engine" if abs(cx) > 0.125 else "hulldark"
    for i, j, k, cx, cy, cz in cells(d, -0.0625, 0.0625, -2.5, -0.75, 2.25, 2.8):
        if cz - 2.25 <= cy + 2.5 + 0.06:
            d.v[i, j, k] = "hulldark"
    for i, j, k, cx, cy, cz in cells(d, -0.5625, 0.5625, -0.875, -0.75, 2.25, 2.375):
        d.v[i, j, k] = "hulldark"
    for i, j, k, cx, cy, cz in cells(d, -0.375, -0.125, -1.75, -1.25, 2.5, 2.625):
        d.v[i, j, k] = "glasshi"
    for i, j, k, cx, cy, cz in cells(d, 0.5, 0.625, 1.0, 1.125, 2.5, 3.25):
        d.v[i, j, k] = "hulldark"
    for i, j, k, cx, cy, cz in cells(d, -0.5, 0.5, 2.0, 3.25, 0.0, 1.0):
        zz = 1.0 - (cy - 2.0)
        if zz - 0.125 <= cz <= zz:
            d.v[i, j, k] = "hullmid" if abs(cx) < 0.375 else "hulldark"
    h.mesh("ship", (x, y, z), rot)
    d.mesh("ship_detail", (x, y, z), rot, glow={"engine", "red", "green"})
