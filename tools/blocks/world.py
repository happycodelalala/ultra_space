"""The ground and the plot: a terrain of 25 cm blocks with levelled pads under structures, a path
and a dug pit; and an occupancy plot that every placed thing claims, so nothing overlaps."""
import math
import random
from mathutils import Vector, noise

from core import Vox, UP_Z

GB = 0.25          # the ground block (m)
R = 22.0           # half-size of the scene square (m)


def smooth(t):
    t = max(0.0, min(1.0, t))
    return t * t * (3 - 2 * t)


class Terrain:
    def __init__(self, seed=0.0):
        self.seed = seed
        self.pads = []      # (kind, centre, size, blend, level) levelled areas under structures
        self.paths = []     # polylines of (x, y), with a half-width
        self.pits = []      # (centre, radius, tunnel end) dug out of the ground
        self._tops = {}

    # -- shape --
    def raw(self, x, y):
        s = self.seed
        h = 1.4 * noise.noise(Vector((x * 0.05 + s, y * 0.05, 0.3))) + 0.5 * noise.noise(Vector((x * 0.16, y * 0.16 + s, 2.0)))
        d = max(abs(x) - 5.5, abs(y) - 6.5, 0.0)
        return h * min(1.0, d / 6.0)

    def pad_rect(self, x0, y0, x1, y1, blend=1.5):
        lvl = self.raw((x0 + x1) / 2, (y0 + y1) / 2)
        self.pads.append(("rect", (x0, y0, x1, y1), blend, lvl))

    def pad_circle(self, x, y, r, blend=1.5):
        self.pads.append(("circle", (x, y, r), blend, self.raw(x, y)))

    def height_raw(self, x, y):
        h = self.raw(x, y)
        for kind, g, blend, lvl in self.pads:
            if kind == "rect":
                x0, y0, x1, y1 = g
                d = max(x0 - x, x - x1, y0 - y, y - y1, 0.0)
            else:
                d = max(0.0, math.hypot(x - g[0], y - g[1]) - g[2])
            w = smooth(d / blend)
            h = lvl + (h - lvl) * w
        return h

    def top(self, i, j):
        """The index of the top block of the column (i, j)."""
        key = (i, j)
        if key not in self._tops:
            self._tops[key] = math.floor(self.height_raw((i + 0.5) * GB, (j + 0.5) * GB) / GB)
        return self._tops[key]

    def height(self, x, y):
        """The ground surface under (x, y) (the top of its block column)."""
        return (self.top(math.floor(x / GB), math.floor(y / GB)) + 1) * GB

    # -- paths and pits --
    def add_path(self, pts, half=0.6):
        self.paths.append(([Vector(p) for p in pts], half))

    def path_d(self, x, y):
        best = 1e9; p = Vector((x, y))
        for pts, half in self.paths:
            for a, b in zip(pts, pts[1:]):
                ab = b - a; t = max(0.0, min(1.0, (p - a).dot(ab) / ab.length_squared))
                best = min(best, (p - (a + ab * t)).length - half)
        return best

    def on_path(self, x, y):
        return self.path_d(x, y) < 0.15 * noise.noise(Vector((x * 0.9, y * 0.9, 5.0)))

    def add_pit(self, x, y, r, tunnel):
        self.pits.append((Vector((x, y)), r, Vector(tunnel)))

    def dug(self, p):
        """Negative inside a dug space."""
        best = 1e9
        for c, r, tun in self.pits:
            z0 = self.height_raw(c.x, c.y)
            q = Vector((p.x - c.x, p.y - c.y, (p.z - z0) * 1.3)); best = min(best, q.length - r)
            a = Vector((c.x + 1.0, c.y, z0 - 1.6)); b = a + tun
            t = max(0.0, min(1.0, (p - a).dot(b - a) / (b - a).length_squared))
            best = min(best, (p - (a + (b - a) * t)).length - 1.0)
        return best

    def in_pit(self, x, y, margin=0.0):
        return any((Vector((x, y)) - c).length < r + margin for c, r, _ in self.pits)

    # -- the block model --
    def build(self):
        g = Vox(GB, UP_Z); n = int(R / GB)
        self.topcols = {}; self.carved = []
        deep = math.floor(-5.5 / GB)
        for i in range(-n, n):
            for j in range(-n, n):
                x, y = (i + 0.5) * GB, (j + 0.5) * GB
                t = self.top(i, j)
                lo = min(self.top(i + a, j + b) for a in (-1, 0, 1) for b in (-1, 0, 1)) - 1
                if self.in_pit(x, y, 3.5) or any((Vector((x, y)) - (c + tun.xy)).length < 3.0 for c, _, tun in self.pits):
                    lo = deep
                if self.on_path(x, y):
                    topc = "path" if (i * 7 + j * 3) % 5 else "path2"
                else:
                    nz = noise.noise(Vector((x * 0.3, y * 0.3, 9.0))) + random.uniform(-0.3, 0.3)
                    topc = "turf3" if nz < -0.25 else ("turf" if nz < 0.2 else "turf2")
                self.topcols[i, j] = topc
                for k in range(lo, t + 1):
                    if self.pits and self.dug(Vector((x, y, (k + 0.5) * GB))) <= 0:
                        self.carved.append((i, j, k))
                        continue
                    d = t - k
                    if d == 0:
                        c = topc
                    elif d < 7:
                        c = "soil" if (i + j + k) % 4 else "soil2"
                    else:
                        c = "ore" if (i * 5 + j * 3 + k * 7) % 41 == 0 else ("bedrock" if (i + 2 * j + k) % 3 else "bedrock2")
                    g.v[i, j, k] = c
        return g.mesh("ground")


class Plot:
    """Occupancy of the ground at 25 cm, and the crowns of tall things, so placements never overlap."""

    def __init__(self, terrain):
        self.t = terrain
        self.cells = {}       # (i, j) -> owner
        self.crowns = []      # (x, y, radius) of tree crowns
        self.solids = []      # (x0, y0, x1, y1) of buildings, for crowns to keep clear of

    def circle(self, x, y, r):
        i0, i1 = math.floor((x - r) / GB), math.floor((x + r) / GB)
        j0, j1 = math.floor((y - r) / GB), math.floor((y + r) / GB)
        return [(i, j) for i in range(i0, i1 + 1) for j in range(j0, j1 + 1)
                if math.hypot((i + 0.5) * GB - x, (j + 0.5) * GB - y) <= r + GB * 0.5]

    def rect(self, x0, y0, x1, y1):
        return [(i, j) for i in range(math.floor(x0 / GB), math.ceil(x1 / GB)) for j in range(math.floor(y0 / GB), math.ceil(y1 / GB))]

    def free(self, cells, allow_path=False):
        for (i, j) in cells:
            x, y = (i + 0.5) * GB, (j + 0.5) * GB
            if (i, j) in self.cells or abs(x) > R - 0.6 or abs(y) > R - 0.6:
                return False
            if not allow_path and self.t.on_path(x, y):
                return False
            if self.t.in_pit(x, y, 0.6):
                return False
        return True

    def flat(self, cells):
        return len({self.t.top(i, j) for (i, j) in cells}) == 1

    def claim(self, cells, owner):
        for c in cells:
            self.cells[c] = owner

    def solid(self, x0, y0, x1, y1, owner):
        self.solids.append((x0, y0, x1, y1)); self.claim(self.rect(x0, y0, x1, y1), owner)

    def crown_ok(self, x, y, r):
        for (cx, cy, cr) in self.crowns:
            if math.hypot(x - cx, y - cy) < 0.8 * (r + cr):
                return False
        for (x0, y0, x1, y1) in self.solids:
            dx = max(x0 - x, x - x1, 0.0); dy = max(y0 - y, y - y1, 0.0)
            if math.hypot(dx, dy) < r + 0.3:
                return False
        return True
