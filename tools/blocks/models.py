"""Block models: structures, props and nature. Each builder makes its Vox models at a position
(x, y, z = the ground surface there) and a turn in quarter turns, and meshes them. Sizes follow
block-world.md section 4: structure 25 cm, props and fixtures 6.25 or 12.5 cm, small things
3.125 cm; nature 25 cm (trees, rocks) and 12.5 cm (bushes)."""
import math
import random
from mathutils import Vector, noise

from core import Vox, UP_Z, UP_ZZ, AROUND, X, Y

NIGHT_GLASS = False   # set by the scene for night: windows glow


# ==== the house ======================================================================================
def house(x, y, z):
    """Stone ground floor with quoins, a jettied timber-framed upper floor, a 45-degree tiled roof
    with a ridge, gables with a frame, a chimney; fixtures (windows with shutters and flower boxes,
    a studded door under a porch roof, a lantern, a bench) in 6.25 cm blocks."""
    b = Vox(0.25, UP_ZZ); gab = Vox(0.25, UP_Z); f = Vox(0.0625, UP_Z); B = 0.25

    def blk(i0, j0, k0, i1, j1, k1, col):
        b.box((i0 * B, j0 * B, k0 * B), (i1 * B, j1 * B, k1 * B), col)

    def hole(i0, j0, k0, i1, j1, k1):
        b.cut((i0 * B, j0 * B, k0 * B), (i1 * B, j1 * B, k1 * B))

    def stones(i, j, k):
        ci, cj = i in (-10, 9), j in (-14, 13)
        if ci and cj:
            return "stone3"
        if (ci and j in (-13, 12) and k % 2 == 0) or (cj and i in (-9, 8) and k % 2 == 1):
            return "stone3"     # quoins: long and short stones alternating up the corners
        return "stone3" if (i + 2 * j + 3 * k) % 7 == 0 else ("stone2" if (3 * i + j + 2 * k) % 9 == 0 else "stone")
    blk(-11, -15, -2, 11, 15, 1, "stone2")                                  # footing
    b.box((-10 * B, -14 * B, 1 * B), (10 * B, 14 * B, 11 * B), stones)    # ground floor
    blk(-11, -15, 11, 11, 15, 12, "beam")                                   # jetty beam
    for i in range(-10, 11, 2):
        blk(i, -15, 10, i + 1, -14, 11, "beam2"); blk(i, 14, 10, i + 1, 15, 11, "beam2")   # joist ends
    blk(-11, -15, 12, 11, 15, 21, "plaster")                                # upper floor
    for i in range(-11, 11, 4):
        blk(i, -15, 12, i + 1, -14, 21, "beam"); blk(i, 14, 12, i + 1, 15, 21, "beam")
    for j in range(-15, 15, 4):
        blk(-11, j, 12, -10, j + 1, 21, "beam"); blk(10, j, 12, 11, j + 1, 21, "beam")
    blk(10, 14, 12, 11, 15, 21, "beam")
    for k in (12, 20):
        for (i0, j0, i1, j1) in [(-11, -15, 11, -14), (-11, 14, 11, 15), (-11, -15, -10, 15), (10, -15, 11, 15)]:
            blk(i0, j0, k, i1, j1, k + 1, "beam")
    # Roof: a 45-degree gable along y, two blocks thick, rows of tiles in alternating colours.
    for i in range(-13, 13):
        top = 21 + int(13 - max(abs(i + 0.5) - 0.5, 0))
        for k in range(top - 2, top):
            for j in range(-17, 17):
                col = ("tile3" if i % 2 else "tile") if k == top - 1 else "tile2"
                b.v[i, j, k] = "tile2" if abs(i + 0.5) > 12 else col
        for k in range(21, top - 1):
            if abs(i + 0.5) <= 11:
                for j in (-15, 14):
                    gab.v[i, j, k] = "beam" if (k == 26 or (i in (-7, 6) and k < 26)) else "plaster"
    for j in range(-17, 17):
        b.v[-1, j, 34] = "beam2"; b.v[0, j, 34] = "beam2"                # ridge
    for j in (-16, 15):
        for i in range(-13, 13):
            b.v[i, j, 21 + int(13 - max(abs(i + 0.5) - 0.5, 0)) - 1] = "beam2"   # barge boards
    for k in range(27, 32):
        b.v[-1, -15, k] = "beam"; b.v[-1, 14, k] = "beam"                 # king posts
    blk(5, 4, 12, 7, 6, 38, lambda i, j, k: "stone2" if k % 3 == 0 else "stone")   # chimney
    blk(4, 3, 38, 8, 7, 39, "stone3")
    for c in list(gab.v):
        if c in b.v:
            del gab.v[c]

    def window(cx, cy, cz, nrm, w, h, box=False):
        n = Vector(nrm); a = Vector((-n.y, n.x, 0)); c = Vector((cx, cy, cz))

        def sq(p, ha, hz, d0, d1, col):
            pts = [p + a * sa * ha + n * d + Vector((0, 0, sz * hz)) for sa in (-1, 1) for sz in (-1, 1) for d in (d0, d1)]
            f.box(tuple(min(q[x] for q in pts) for x in range(3)), tuple(max(q[x] for q in pts) for x in range(3)), col)
        sq(c, w + 0.06, h + 0.06, -0.24, 0.06, "beam2")
        sq(c, w - 0.02, h - 0.02, -0.2, 0.07, "glass")
        sq(c, 0.03, h, -0.2, 0.1, "beam"); sq(c, w, 0.03, -0.2, 0.1, "beam")
        sq(c + a * w * 0.5 + Vector((0, 0, h * 0.5)), 0.06, 0.06, 0.06, 0.08, "glasshi")
        sq(c - Vector((0, 0, h + 0.08)), w + 0.12, 0.04, -0.05, 0.16, "woodhi")
        for sd in (-1, 1):
            sp = c + a * sd * (w + 0.26)
            sq(sp, 0.2, h, 0.06, 0.12, "wood")
            for zz in (-0.6, 0.0, 0.6):
                sq(sp + Vector((0, 0, zz * h)), 0.19, 0.03, 0.11, 0.15, "wood2")
        if box:
            bc = c - Vector((0, 0, h + 0.24)) + n * 0.2
            sq(bc, w + 0.06, 0.1, -0.1, 0.12, "wood2")
            for k in range(-int(w / 0.12), int(w / 0.12) + 1):
                p = bc + a * (k * 0.12) + Vector((0, 0, 0.16))
                f.ball(p, 0.09, "leaf2"); f.ball(p + Vector((0, 0, 0.08)), 0.06, ["pink", "red", "yellow", "white"][k % 4])
    hole(-7, -14, 4, -3, -13, 8); window(-1.25, -3.5, 1.5, (0, -1, 0), 0.45, 0.45, box=True)
    hole(2, -14, 1, 6, -13, 10)
    for x0 in (-8, 4):
        hole(x0, -15, 14, x0 + 4, -14, 18); window((x0 + 2) * B, -3.75, 4.0, (0, -1, 0), 0.45, 0.45)
        hole(x0, 14, 14, x0 + 4, 15, 18); window((x0 + 2) * B, 3.75, 4.0, (0, 1, 0), 0.45, 0.45)
    for j0 in (-10, 6):
        hole(-11, j0, 14, -10, j0 + 4, 18); window(-2.75, (j0 + 2) * B, 4.0, (-1, 0, 0), 0.45, 0.45)
        hole(10, j0, 14, 11, j0 + 4, 18); window(2.75, (j0 + 2) * B, 4.0, (1, 0, 0), 0.45, 0.45)
    hole(-10, -2, 4, -9, 2, 8); window(-2.5, 0.0, 1.5, (-1, 0, 0), 0.45, 0.45, box=True)
    hole(9, -2, 4, 10, 2, 8); window(2.5, 0.0, 1.5, (1, 0, 0), 0.45, 0.45, box=True)
    hole(-2, -15, 25, 2, -14, 28); window(0.0, -3.75, 6.6, (0, -1, 0), 0.4, 0.35)
    # The door: frame, planks, iron straps and studs, a ring, a step, a porch roof on brackets.
    dx, dy = 1.0, -3.5
    f.box((dx - 0.56, dy - 0.12, 0.25), (dx + 0.56, dy + 0.06, 2.55), "beam")
    f.cut((dx - 0.46, dy - 0.14, 0.25), (dx + 0.46, dy + 0.08, 2.45))
    for k in range(-4, 4):
        f.box((dx + k * 0.12, dy - 0.05, 0.25), (dx + k * 0.12 + 0.12, dy + 0.05, 2.45), "wood" if k % 2 else "wood2")
    for zz in (0.6, 1.9):
        f.box((dx - 0.46, dy - 0.08, zz), (dx + 0.46, dy - 0.04, zz + 0.08), "iron")
    for (xx, zz) in [(-0.3, 1.2), (0.3, 1.2), (-0.3, 0.9), (0.3, 0.9)]:
        f.box((dx + xx, dy - 0.1, zz), (dx + xx + 0.06, dy - 0.06, zz + 0.06), "ironhi")
    f.box((dx + 0.3, dy - 0.12, 1.3), (dx + 0.38, dy - 0.06, 1.44), "gold")
    f.box((dx - 0.75, dy - 0.6, 0.25), (dx + 0.75, dy - 0.12, 0.375), "stone3")
    for sd in (-1, 1):
        f.rod((dx + sd * 0.7, dy, 2.4), (dx + sd * 0.7, dy - 0.7, 2.75), 0.06, "beam")
    for row in range(4):
        f.box((dx - 0.95, dy - 0.15 - (row + 1) * 0.2, 2.82 - row * 0.06), (dx + 0.95, dy - 0.15 - row * 0.2, 2.88 - row * 0.06), "tile3" if row % 2 else "tile")
    f.box((dx + 0.75, dy - 0.36, 2.0), (dx + 0.81, dy, 2.06), "iron")
    f.box((dx + 0.7, dy - 0.48, 1.72), (dx + 0.86, dy - 0.32, 1.96), "lantern")
    f.box((dx + 0.68, dy - 0.5, 1.96), (dx + 0.88, dy - 0.3, 2.0), "iron")
    # A bench under the right-hand front window.
    f.box((1.9, -4.0, 0.62), (2.9, -3.62, 0.72), "woodhi")
    for bx in (1.95, 2.75):
        f.box((bx, -3.95, 0.25), (bx + 0.12, -3.68, 0.62), "wood2")
    loc = (x, y, z - 0.25)
    b.mesh("house", loc); gab.mesh("house_gable", loc)
    f.mesh("house_fixtures", loc, glow={"glass", "lantern", "glasshi"} if NIGHT_GLASS else {"lantern"})


# ==== outbuildings ===================================================================================
def woodshed(x, y, z):
    """An open-fronted woodshed: corner posts, plank walls on three sides, a 45-degree roof, a stack
    of split logs showing their end grain; 12.5 cm blocks."""
    v = Vox(0.125, UP_ZZ)
    v.box((-1.5, -1.5, 0.0), (1.5, 1.5, 0.125), "wood2")
    for k in range(6):
        u = -1.5 + k * 0.5; col = "wood" if k % 2 else "wood2"
        v.box((-1.5, u, 0.125), (-1.375, u + 0.5, 2.0), col); v.box((1.375, u, 0.125), (1.5, u + 0.5, 2.0), col)
        v.box((u, 1.375, 0.125), (u + 0.5, 1.5, 2.0), col)
    for (cx, cy) in [(-1.5, -1.5), (1.25, -1.5), (-1.5, 1.25), (1.25, 1.25)]:
        v.box((cx, cy, 0.0), (cx + 0.25, cy + 0.25, 2.25), "beam")
    v.box((-1.5, -1.5, 2.0), (1.5, -1.25, 2.125), "beam")
    for k in v.r(2.0, 3.6):
        zz = (k + 0.5) * 0.125 - 2.0
        for i in v.r(-1.5 + zz, 1.5 - zz):
            for j in list(v.r(-1.5, -1.375)) + list(v.r(1.375, 1.5)):
                v.v[i, j, k] = "wood" if (i + k) % 2 else "wood2"
    for i in v.r(-1.875, 1.875):
        xx = (i + 0.5) * 0.125; z0 = 1.875 + math.floor((1.875 - abs(xx)) / 0.125) * 0.125
        for j in v.r(-1.875, 1.875):
            for k in v.r(z0 - 0.25, z0):
                v.v[i, j, k] = ("tile3" if i % 2 else "tile") if k == v.r(z0 - 0.25, z0).stop - 1 else "tile2"
    for i in v.r(-1.25, 1.25):
        for k in v.r(0.125, 1.375):
            for j in v.r(0.25, 1.375):
                v.v[i, j, k] = "bark"
            v.v[i, v.r(0.25, 1.375).start, k] = "endgrain" if (i + k) % 3 else "woodhi"
    v.mesh("woodshed", (x, y, z))


def stump_with_axe(x, y, z):
    v = Vox(0.0625, AROUND + UP_Z)
    v.rod((0, 0, 0), (0, 0, 0.45), 0.26, lambda p, t: "endgrain" if t > 0.95 else ("bark" if (p.x * 31 + p.y * 17) % 1 > 0.3 else "bark2"), flat=True)
    v.box((0.06, -0.03, 0.45), (0.12, 0.03, 0.95), "wood2")
    v.box((-0.04, -0.03, 0.45), (0.22, 0.03, 0.6), "iron"); v.box((-0.06, -0.03, 0.47), (0.0, 0.03, 0.58), "steelhi")
    v.mesh("stump", (x, y, z))


def well(x, y, z):
    """A stone well: an octagonal ring of stones, water inside, two posts, a windlass with a rope
    and bucket, a little 45-degree roof; 12.5 cm blocks."""
    v = Vox(0.125, UP_ZZ)
    for i in v.r(-0.875, 0.875):
        for j in v.r(-0.875, 0.875):
            px, py = (i + 0.5) * 0.125, (j + 0.5) * 0.125
            d = max(abs(px), abs(py), (abs(px) + abs(py)) / 1.3)
            if d <= 0.875:
                for k in v.r(0.0, 0.75):
                    if d > 0.55:
                        v.v[i, j, k] = "stone3" if (i + j + k) % 3 == 0 else ("stone2" if (i * 2 + k) % 5 == 0 else "stone")
                    elif k < 3:
                        v.v[i, j, k] = "water" if k == 2 else "stone2"
                if d > 0.55:
                    v.v[i, j, v.r(0.0, 0.75).stop] = "stone3"
    for sx in (-1, 1):
        v.box((sx * 0.75 - 0.0625 - 0.0625, -0.125, 0.875), (sx * 0.75 + 0.0625, 0.125, 2.0), "beam")
    v.box((-0.75, -0.0625, 1.5), (0.75, 0.0625, 1.625), "wood")
    v.box((-0.0625, -0.0625, 0.875), (0.0625, 0.0625, 1.5), "dark")
    v.box((-0.125, -0.125, 0.75), (0.125, 0.125, 1.0), lambda i, j, k: "iron" if k == v.r(0.75, 1.0).start + 1 else "wood")
    for j in v.r(-0.875, 0.875):
        yy = (j + 0.5) * 0.125; z0 = 2.0 + math.floor((0.875 - abs(yy)) / 0.125) * 0.125
        for i in v.r(-1.0, 1.0):
            for k in v.r(z0 - 0.25, z0):
                v.v[i, j, k] = ("tile3" if j % 2 else "tile") if k == v.r(z0 - 0.25, z0).stop - 1 else "tile2"
    v.mesh("well", (x, y, z))


def garden(x, y, z, w=1.25, d=3.75, crop="cabbage"):
    """A raised bed: a plank frame of 12.5 cm blocks with tilled soil in furrows, and its crop in
    6.25 cm blocks (cabbages, or carrots with their leaves)."""
    v = Vox(0.125)
    v.box((-w / 2, -d / 2, 0.0), (w / 2, d / 2, 0.25), "wood")
    v.box((-w / 2 + 0.125, -d / 2 + 0.125, 0.125), (w / 2 - 0.125, d / 2 - 0.125, 0.25), lambda i, j, k: "tilled" if j % 2 else "soil")
    v.mesh("bed", (x, y, z))
    c = Vox(0.0625, UP_Z if crop == "cabbage" else ())
    yy = -d / 2 + 0.375
    while yy < d / 2 - 0.25:
        for xx in (-w / 4, w / 4):
            if crop == "cabbage":
                c.ball((xx, yy, 0.38), (0.17, 0.17, 0.13), "leaf2")
                c.ball((xx, yy, 0.42), (0.1, 0.1, 0.1), "leafhi")
            else:
                for dx in (-0.06, 0.06):
                    c.rod((xx + dx, yy, 0.25), (xx + dx * 2, yy, 0.55), 0.03, "grass2")
                c.box((xx - 0.03, yy - 0.03, 0.25), (xx + 0.03, yy + 0.03, 0.31), "orange")
        yy += 0.5
    c.shade({"leaf2"}, "leafhi", "leaf2", "leaf")
    c.mesh("crop", (x, y, z))


# ==== props ===========================================================================================
def fence(a, b, z, n):
    """Posts with caps and two rails, along a line on the grid; 6.25 cm blocks."""
    v = Vox(0.0625); a, b = Vector(a), Vector(b); d = b - a
    along_x = abs(d.x) > abs(d.y)
    for k in range(n + 1):
        p = d * (k / n)
        v.box((p.x - 0.0625, p.y - 0.0625, -0.0625), (p.x + 0.0625, p.y + 0.0625, 1.0), "wood2")
        v.box((p.x - 0.0625, p.y - 0.0625, 1.0), (p.x + 0.0625, p.y + 0.0625, 1.0625), "woodhi")
    for zz in (0.375, 0.75):
        lo = (min(0, d.x) - (0 if along_x else 0.03125), min(0, d.y) - (0.03125 if along_x else 0), zz)
        hi = (max(0, d.x) + (0 if along_x else 0.03125), max(0, d.y) + (0.03125 if along_x else 0), zz + 0.125)
        v.box(lo, hi, lambda i, j, k: "wood" if ((i if along_x else j) // 6) % 2 else "woodhi")
    v.mesh("fence", (a.x, a.y, z))


def barrel(x, y, z):
    v = Vox(0.0625, AROUND + UP_Z)
    v.rod((0, 0, 0), (0, 0, 1.0), lambda t: 0.33 + 0.06 * math.sin(t * math.pi),
          lambda p, t: "iron" if abs(t - 0.15) < 0.04 or abs(t - 0.85) < 0.04 else ("wood" if int(math.atan2(p.y, p.x) * 4) % 2 else "wood2"), flat=True)
    v.cut((-0.25, -0.25, 0.94), (0.25, 0.25, 1.05))
    v.box((-0.25, -0.25, 0.875), (0.25, 0.25, 0.9375), "brown2")
    v.mesh("barrel", (x, y, z))


def crate(v, x0, y0, z0, s):
    """One crate into `v`: plank rows, a darker frame."""
    def col(i, j, k):
        p = v.centre(i, j, k); ex = [abs(p.x - x0) > s - 0.07, abs(p.y - y0) > s - 0.07, (p.z - z0) < 0.07 or (p.z - z0) > 2 * s - 0.07]
        if sum(ex) >= 2:
            return "wood2"
        return "wood" if ((k - v.r(z0, z0 + 2 * s).start) // 2) % 2 else "woodhi"
    v.box((x0 - s, y0 - s, z0), (x0 + s, y0 + s, z0 + 2 * s), col)


def crates(x, y, z):
    v = Vox(0.0625)
    crate(v, 0.0, 0.0, 0.0, 0.25); crate(v, 0.5625, 0.0625, 0.0, 0.25); crate(v, 0.25, 0.0, 0.5, 0.1875)
    v.mesh("crates", (x, y, z))


def wagon(x, y, z):
    """A hay wagon: a plank bed and sides, spoked wheels with iron tyres, shafts, a load of hay."""
    v = Vox(0.0625, [(Y, 1), (Y, -1)])
    v.box((-0.7, -1.25, 0.625), (0.7, 1.25, 0.75), "wood")
    for sd in (-1, 1):
        for row in range(2):
            z0 = 0.75 + row * 0.1875; col = "wood2" if row else "wood"
            v.box((sd * 0.7 - (0.0625 if sd > 0 else 0), -1.25, z0), (sd * 0.7 + (0 if sd > 0 else 0.0625), 1.25, z0 + 0.1875), col)
            v.box((-0.7, sd * 1.25 - (0.0625 if sd > 0 else 0), z0), (0.7, sd * 1.25 + (0 if sd > 0 else 0.0625), z0 + 0.1875), col)
        for wy in (-0.8, 0.8):
            cx = sd * 0.8125
            for j in v.r(wy - 0.45, wy + 0.45):
                for k in v.r(0.0, 0.875):
                    p = v.centre(0, j, k); dy, dz = p.y - wy, p.z - 0.4375; r = math.hypot(dy, dz)
                    if r > 0.44:
                        continue
                    c = "iron" if r > 0.37 else ("wood2" if r > 0.31 or r < 0.09 or abs(dy) < 0.04 or abs(dz) < 0.04 or abs(abs(dy) - abs(dz)) < 0.045 else None)
                    if c:
                        v.v[v.r(cx - 0.03, cx + 0.03).start, j, k] = c
        v.rod((sd * 0.3, -1.25, 0.68), (sd * 0.26, -2.75, 0.5), 0.04, "wood2")
    v.rod((-0.8, -0.8, 0.4375), (0.8, -0.8, 0.4375), 0.04, "iron"); v.rod((-0.8, 0.8, 0.4375), (0.8, 0.8, 0.4375), 0.04, "iron")
    for k in range(4):
        v.ball((random.uniform(-0.35, 0.35), random.uniform(-0.8, 0.8), 1.0), (0.34, 0.3, 0.24), "straw", 0.15, k)
    v.ball((0.3, 0.9, 1.05), (0.18, 0.15, 0.22), "cream")
    v.shade({"straw"}, "straw", "straw", "straw2", "straw2", 0.3)
    v.mesh("wagon", (x, y, z))


def lantern_post(x, y, z):
    v = Vox(0.0625)
    v.box((-0.125, -0.125, 0.0), (0.125, 0.125, 0.125), "stone2")
    v.box((-0.0625, -0.0625, 0.125), (0.0625, 0.0625, 2.0), "beam")
    v.box((0.0, -0.03125, 1.875), (0.5, 0.03125, 1.9375), "iron")
    v.box((0.3125, -0.09375, 1.5625), (0.5, 0.09375, 1.8125), "lantern")
    v.box((0.28125, -0.125, 1.8125), (0.53125, 0.125, 1.875), "iron")
    v.mesh("lantern_post", (x, y, z), glow={"lantern"})


def mined_pile(x, y, z):
    """What digging the pit yielded: blocks of soil, stone and ore stacked as items, and a pick."""
    v = Vox(0.125)
    kinds = ["soil", "soil", "soil2", "bedrock", "bedrock2", "soil", "ore", "soil", "bedrock"]
    k = 0
    for (i, j, kk) in [(0, 0, 0), (1, 0, 0), (2, 0, 0), (0, 1, 0), (1, 1, 0), (0, 0, 1), (1, 0, 1), (0, 1, 1), (3, 1, 0)]:
        v.v[i * 2, j * 2, kk] = kinds[k]; v.v[i * 2 + 1, j * 2, kk] = kinds[k]
        v.v[i * 2, j * 2 + 1, kk] = kinds[k]; v.v[i * 2 + 1, j * 2 + 1, kk] = kinds[k]
        k += 1
    v.mesh("mined", (x, y, z))
    p = Vox(0.0625)
    p.rod((0.0, 0.0, 0.0), (0.0, 0.25, 0.9), 0.035, "wood2")
    p.rod((-0.3, 0.25, 0.85), (0.3, 0.25, 0.95), 0.04, "iron")
    p.mesh("pick", (x + 0.8, y - 0.3, z))


# ==== nature ==========================================================================================
def lobe(v, c, r, h, col):
    """A crown lobe: a cushion of stacked rounded squares, flat on top and underneath, its edge
    narrowing toward the top and bottom; filled solid so its edges take slopes."""
    n = max(3, round(h / v.s))
    for k in range(n):
        t = (k + 0.5) / n
        rr = r * (1.0 - 0.55 * max(0.0, 2 * abs(t - 0.45) - 0.3) ** 1.5)
        kk = math.floor(c[2] / v.s) + k
        for i in v.r(c[0] - rr, c[0] + rr):
            for j in v.r(c[1] - rr, c[1] + rr):
                px = (i + 0.5) * v.s - c[0]; py = (j + 0.5) * v.s - c[1]
                if max(abs(px), abs(py)) <= rr and abs(px) + abs(py) <= rr * 1.45:
                    v.v[i, j, kk] = col


def oak(x, y, z, s, turn):
    """A broadleaf tree: a 2-block trunk with a root flare and two limbs, a crown of four cushion
    lobes lit on top and shaded underneath; 25 cm blocks, smoothed up and down."""
    v = Vox(0.25, UP_ZZ)
    th = 3.0 * s
    v.box((-0.25, -0.25, -0.25), (0.25, 0.25, th), lambda i, j, k: "bark2" if (k + i * 2 + j) % 5 == 0 else "bark")
    for (i, j) in [(-2, -1), (-2, 0), (1, -1), (1, 0), (-1, -2), (0, -2), (-1, 1), (0, 1)]:
        v.v[i, j, 0] = "bark2"
    for (dx, dy) in [(1.0, 0.6), (-0.9, -0.5)]:
        v.rod((0, 0, 2.1 * s), (dx * s, dy * s, 3.2 * s), 0.13, "bark")
    for (cx, cy, cz, r, h) in [(0, 0, 3.4, 1.6, 1.25), (1.0, 0.35, 3.0, 1.05, 0.9), (-0.85, -0.6, 3.05, 1.0, 0.85), (0.15, -0.1, 4.45, 1.1, 0.8)]:
        lobe(v, (cx * s, cy * s, cz * s), r * s, h * s, "leaf")
    v.shade({"leaf"}, "leafhi", "leaf", "leaf3", "leaf2", 0.3, random.uniform(0, 50))
    v.mesh("oak", (x, y, z), turn * 90)


def birch(x, y, z, s, turn):
    """A birch: a slim white trunk marked dark, a tall narrow crown of three light lobes."""
    v = Vox(0.25, UP_ZZ)
    for k in v.r(-0.25, 4.2 * s):
        v.v[0, 0, k] = "birch2" if (k * 7) % 5 == 0 else "birch"
    v.rod((0.125, 0.125, 2.2 * s), (0.7 * s, 0.4 * s, 3.0 * s), 0.1, "birch")
    for (cx, cy, cz, r, h) in [(0.125, 0.125, 2.7, 1.0, 0.9), (0.3, 0.0, 3.5, 0.9, 0.85), (0.125, 0.2, 4.3, 0.7, 0.75)]:
        lobe(v, (cx * s, cy * s, cz * s), r * s, h * s, "bleaf")
    v.shade({"bleaf"}, "bleafhi", "bleaf", "bleaf3", "bleafhi", 0.15, random.uniform(0, 50))
    v.mesh("birch", (x, y, z), turn * 90)


def pine(x, y, z, s, turn):
    """A conifer: five tiers, each a cone that narrows one block per layer (45-degree slopes)."""
    v = Vox(0.25, UP_Z)
    v.box((-0.25, -0.25, -0.25), (0.25, 0.25, 1.25 * s), "bark")
    for t in range(5):
        z0 = (0.9 + 0.95 * t) * s; r0 = (1.85 - 0.33 * t) * s
        m = 0
        while True:
            rr = r0 - m * 0.25
            if rr < 0.2 or m > 7:
                break
            k = math.floor(z0 / 0.25) + m
            for i in v.r(-rr, rr):
                for j in v.r(-rr, rr):
                    px, py = abs((i + 0.5) * 0.25), abs((j + 0.5) * 0.25)
                    if max(px, py) <= rr and px + py <= rr * 1.35:
                        v.v[i, j, k] = "pine"
            m += 1
    # The leader: the last tier tapers to a 2 x 2 point; one more course of it on top.
    top = max(k for (_, _, k) in v.v)
    for (i, j) in [(-1, -1), (0, -1), (-1, 0), (0, 0)]:
        v.v[i, j, top + 1] = "pine2"
    v.shade({"pine"}, "pine2", "pine", "pine3", "pine3", 0.2)
    v.mesh("pine", (x, y, z), turn * 90)


def bush(x, y, z, berries, turn):
    v = Vox(0.125, UP_ZZ); seed = random.uniform(0, 50)
    v.ball((0, 0, 0.3), (0.7, 0.6, 0.45), "leaf2", 0.2, seed)
    v.ball((0.35, 0.25, 0.55), (0.45, 0.42, 0.35), "leaf2", 0.2, seed + 2)
    v.shade({"leaf2"}, "leafhi", "leaf2", "leaf", "leaf", 0.25, seed)
    if berries:
        bc = random.choice(["berry", "pink", "yellow", "white", "red"])
        tops = [c for c, col in v.v.items() if col == "leafhi"]
        for c in random.sample(tops, min(len(tops), 7)):
            v.v[c] = bc
    v.mesh("bush", (x, y, z), turn * 90)


def rock(x, y, z, s, turn):
    v = Vox(0.25, UP_Z + AROUND); seed = random.uniform(0, 50)
    v.ball((0, 0, 0.1), (1.0 * s, 0.8 * s, 0.65 * s), "rock", 0.25, seed)
    v.shade({"rock"}, "rock3", "rock", "rock2", "rock2", 0.35, seed)
    for c, col in list(v.v.items()):
        if col == "rock3" and noise.noise(Vector(c) * 0.5 + Vector((seed, 0, 0))) > -0.1:
            v.v[c] = "moss"
    v.mesh("rock", (x, y, z - 0.25), turn * 90)
    p = Vox(0.125, UP_Z)
    p.ball((0, 0, 0.1), (0.35 * s, 0.3 * s, 0.25 * s), "rock2", 0.2, seed + 5)
    p.shade({"rock2"}, "rock3", "rock2", "rock2")
    p.mesh("pebble", (x + 1.05 * s, y - 0.55 * s, z - 0.125), turn * 90)


TUFTS = []


def tuft_templates(scene_coll):
    for g in range(5):
        v = Vox(0.03125)
        for k in range(random.randint(5, 8)):
            a = random.uniform(0, math.tau); r = random.uniform(0.0, 0.12); h = random.uniform(0.18, 0.42)
            base = Vector((math.cos(a) * r, math.sin(a) * r, 0)); lean = Vector((math.cos(a), math.sin(a), 0)) * 0.1
            v.rod(base, base + lean + Vector((0, 0, h)), 0.022, lambda p, t: "grass2" if t > 0.6 else "grass")
        o = v.mesh(f"tuft{g}"); o.hide_render = True; o.location = (0, 0, -100); TUFTS.append(o)


def tuft(x, y, z, coll):
    o = random.choice(TUFTS).copy(); o.hide_render = False
    o.location = (x, y, z); o.rotation_euler = (0, 0, random.choice([0, 1.5708, 3.1416, 4.7124])); o.scale = (random.uniform(0.9, 1.4),) * 3
    coll.objects.link(o)


def flowers(x, y, z):
    v = Vox(0.03125); col = random.choice(["pink", "yellow", "white", "red", "purple", "blue"])
    for k in range(3):
        p = Vector((random.uniform(-0.15, 0.15), random.uniform(-0.15, 0.15), 0)); h = random.uniform(0.22, 0.38)
        v.rod(p, p + Vector((0, 0, h)), 0.018, "grass")
        v.ball(p + Vector((0, 0, h)), (0.07, 0.07, 0.035), col); v.ball(p + Vector((0, 0, h + 0.025)), 0.025, "yellow")
    v.mesh("flowers", (x, y, z))


def mushrooms(x, y, z):
    v = Vox(0.03125, UP_Z)
    for k in range(3):
        p = Vector((random.uniform(-0.15, 0.15), random.uniform(-0.15, 0.15), 0)); h = random.uniform(0.08, 0.16)
        v.rod(p, p + Vector((0, 0, h)), 0.025, "white")
        v.ball(p + Vector((0, 0, h)), (0.09, 0.09, 0.05), lambda q: "white" if (math.floor(q.x / 0.03125) + math.floor(q.y / 0.03125)) % 4 == 0 else "red")
    v.mesh("mushrooms", (x, y, z))
