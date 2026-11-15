"""Core of the block-scene renders: the palette, materials, the shape set and the voxel model.

A model is a Vox: cells of one size (a power of two, block-world.md section 4) holding a palette
colour. Meshing draws the faces between solid and empty cells, merged into the largest rectangles
of one colour, and turns one-block steps into shaped cells (wedges, outer and valley corners) by
the model's recipe of "up" directions (block-world.md section 3)."""
import math
import random
import bpy
import bmesh
from mathutils import Vector, noise

# ---- palette (sRGB 0-255; converted to linear when meshed) ---------------------------------------
PALETTE = {
    "plaster": (236, 226, 200), "plaster2": (222, 210, 182), "beam": (92, 58, 34), "beam2": (74, 46, 26),
    "stone": (150, 146, 140), "stone2": (124, 120, 116), "stone3": (174, 170, 162),
    "tile": (176, 64, 42), "tile2": (150, 50, 34), "tile3": (196, 84, 58),
    "glass": (70, 110, 140), "glasshi": (150, 190, 210),
    "wood": (150, 100, 56), "wood2": (126, 82, 44), "woodhi": (176, 122, 70), "endgrain": (205, 160, 100),
    "iron": (60, 60, 66), "ironhi": (110, 112, 120), "gold": (226, 180, 60),
    "bark": (96, 64, 40), "bark2": (78, 52, 32), "birch": (226, 224, 214), "birch2": (60, 56, 52),
    "leaf": (64, 140, 52), "leaf2": (86, 164, 62), "leaf3": (46, 112, 44), "leafhi": (120, 190, 80),
    "bleaf": (110, 176, 70), "bleafhi": (156, 206, 96), "bleaf3": (80, 140, 56),
    "pine": (36, 100, 64), "pine2": (52, 124, 76), "pine3": (26, 80, 52),
    "grass": (90, 168, 58), "grass2": (120, 190, 70),
    "turf": (112, 168, 64), "turf2": (128, 182, 72), "turf3": (98, 152, 58),
    "path": (170, 138, 96), "path2": (156, 124, 84), "soil": (122, 84, 52), "soil2": (106, 72, 44),
    "tilled": (96, 64, 40), "bedrock": (118, 116, 112), "bedrock2": (100, 98, 96), "ore": (214, 176, 70),
    "water": (60, 110, 150), "waterhi": (110, 160, 190),
    "pink": (240, 120, 150), "yellow": (250, 210, 70), "white": (245, 245, 236), "red": (200, 50, 40),
    "blue": (60, 100, 170), "orange": (236, 130, 40), "purple": (150, 90, 200),
    "berry": (150, 60, 170), "berry2": (190, 90, 200),
    "green": (70, 120, 60), "brown": (120, 80, 46), "brown2": (96, 62, 36), "cream": (230, 214, 176),
    "dark": (50, 44, 42), "steel": (176, 182, 192), "steel2": (140, 146, 158), "steelhi": (215, 220, 228),
    "skin": (246, 206, 160), "skin2": (220, 176, 130), "hair": (66, 42, 28), "hair2": (46, 30, 20),
    "blond": (222, 186, 110), "blond2": (196, 156, 84), "eye": (30, 26, 26),
    "straw": (232, 200, 112), "straw2": (204, 168, 86),
    "rock": (156, 156, 160), "rock2": (128, 128, 134), "rock3": (176, 176, 178), "moss": (96, 140, 60),
    "lantern": (255, 200, 110),
    "hull": (222, 224, 220), "hull2": (186, 190, 190), "hulldark": (62, 68, 76), "hullmid": (110, 116, 124),
    "accent": (240, 120, 30), "canopy": (60, 110, 150), "engine": (90, 170, 255),
    "comb": (210, 40, 30), "beak": (240, 170, 40),
}


def lin(c):
    return tuple((v / 255) ** 2.2 for v in c)


# ---- materials: one flat-colour material from the vertex colours, and its glowing twin ----------
VCOL = VGLOW = None


def setup_materials():
    global VCOL, VGLOW
    VCOL = bpy.data.materials.new("blocks"); VCOL.use_nodes = True
    nt = VCOL.node_tree; b = nt.nodes["Principled BSDF"]
    b.inputs["Roughness"].default_value = 0.8
    b.inputs["Specular IOR Level"].default_value = 0.15
    a = nt.nodes.new("ShaderNodeVertexColor"); a.layer_name = "C"
    nt.links.new(a.outputs["Color"], b.inputs["Base Color"])
    VGLOW = bpy.data.materials.new("glow"); VGLOW.use_nodes = True
    nt = VGLOW.node_tree; b = nt.nodes["Principled BSDF"]
    a = nt.nodes.new("ShaderNodeVertexColor"); a.layer_name = "C"
    nt.links.new(a.outputs["Color"], b.inputs["Base Color"]); nt.links.new(a.outputs["Color"], b.inputs["Emission Color"])
    b.inputs["Emission Strength"].default_value = 0.0


def set_glow(strength):
    VGLOW.node_tree.nodes["Principled BSDF"].inputs["Emission Strength"].default_value = strength


def set_wet(wet):
    VCOL.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.35 if wet else 0.8


# ---- the shape set ----------------------------------------------------------------------------------
X, Y, Z = 0, 1, 2
UP_Z = [(Z, 1)]
UP_ZZ = [(Z, 1), (Z, -1)]
AROUND = [(X, 1), (X, -1), (Y, 1), (Y, -1)]
RING = [(0, 0), (1, 0), (1, 1), (0, 1)]


def _add(p, ax, d):
    q = list(p); q[ax] += d
    return tuple(q)


def shape_cells(occ, ups):
    """{cell: (axis, sign, top-corner heights)} for cells that become wedges, outer or valley corners.
    A cell slopes toward a direction when the neighbour there is empty with solid below it (a step
    one block high) and the solid continues behind the cell; a single slope also needs a solid
    neighbour along its edge (so small bumps stay blocks). The same rule as sim_blocks::shape."""
    out = {}
    for (a, sg) in ups:
        p_ax, q_ax = [x for x in range(3) if x != a]
        dirs = [(p_ax, 1), (p_ax, -1), (q_ax, 1), (q_ax, -1)]

        def low(c, ax, d):
            n = _add(c, ax, d)
            return n not in occ and _add(n, a, -sg) in occ
        for c in occ:
            if c in out or _add(c, a, sg) in occ:
                continue
            lows = [(ax, d) for (ax, d) in dirs if low(c, ax, d) and _add(c, ax, -d) in occ]
            H = None

            def along(ax):
                r = q_ax if ax == p_ax else p_ax
                return _add(c, r, 1) in occ or _add(c, r, -1) in occ
            if (len(lows) == 1 and along(lows[0][0])) or (len(lows) == 2 and lows[0][0] != lows[1][0]):
                H = [1, 1, 1, 1]
                for k, (cp, cq) in enumerate(RING):
                    for (ax, d) in lows:
                        cc = cp if ax == p_ax else cq
                        if (d > 0 and cc == 1) or (d < 0 and cc == 0):
                            H[k] = 0
            elif not lows:
                for (pa, pd) in dirs[:2]:
                    for (qa, qd) in dirs[2:]:
                        n1, n2 = _add(c, pa, pd), _add(c, qa, qd); dg = _add(n1, qa, qd)
                        if (n1 in occ and n2 in occ and dg not in occ and _add(dg, a, -sg) in occ
                                and _add(n1, a, sg) not in occ and _add(n2, a, sg) not in occ
                                and low(n1, qa, qd) and low(n2, pa, pd)):
                            if H is None:      # the first match, as in sim_blocks
                                H = [1, 1, 1, 1]
                                for k, (cp, cq) in enumerate(RING):
                                    if cp == (1 if pd > 0 else 0) and cq == (1 if qd > 0 else 0):
                                        H[k] = 0
            if H is not None:
                out[c] = (a, sg, tuple(H))
    return out


def face_full(sh, ax, d):
    """Whether a shaped cell's face toward (ax, d) covers the whole square."""
    a, sg, H = sh
    if ax == a:
        return d == -sg
    p_ax, q_ax = [x for x in range(3) if x != a]
    at = 1 if d > 0 else 0
    return all(H[k] for k, (cp, cq) in enumerate(RING) if (cp if ax == p_ax else cq) == at)


def covers(occ, shp, n, ax, d):
    """Whether the cell at n covers its whole face toward (ax, d)."""
    return n in occ and (n not in shp or face_full(shp[n], ax, d))


def shaped_faces(c, sh, s, full, occ=None, shp=None):
    """The faces (point lists, outward wound) of one shaped cell, skipping sides against full cells."""
    a, sg, H = sh
    p_ax, q_ax = [x for x in range(3) if x != a]

    def pt(cp, cq, h):
        co = [0.0, 0.0, 0.0]
        co[p_ax] = (c[p_ax] + cp) * s; co[q_ax] = (c[q_ax] + cq) * s
        co[a] = (c[a] + (h if sg > 0 else 1 - h)) * s
        return Vector(co)
    upv = Vector((0, 0, 0)); upv[a] = sg
    polys = []
    hidden = (lambda n, ax, d: covers(occ, shp, n, ax, d)) if occ is not None else (lambda n, ax, d: n in full)
    if not hidden(_add(c, a, -sg), a, sg):
        polys.append(([pt(cp, cq, 0) for cp, cq in RING], -upv))
    for k in range(4):
        m, n = k, (k + 1) % 4
        (mp, mq), (np_, nq) = RING[m], RING[n]
        ax, d = (p_ax, 1 if mp == 1 else -1) if mp == np_ else (q_ax, 1 if mq == 1 else -1)
        if hidden(_add(c, ax, d), ax, -d):
            continue
        pts = [pt(mp, mq, 0), pt(np_, nq, 0)]
        if H[n]:
            pts.append(pt(np_, nq, 1))
        if H[m]:
            pts.append(pt(mp, mq, 1))
        if len(pts) >= 3:
            o = Vector((0, 0, 0)); o[ax] = d
            polys.append((pts, o))
    odd = [k for k in range(4) if sum(H) in (1, 3) and H[k] == (1 if sum(H) == 1 else 0)]   # the odd corner
    k0 = odd[0] if odd else 0
    T = [pt(*RING[k], H[k]) for k in range(4)]
    for tri in [(k0, k0 + 1, k0 + 2), (k0, k0 + 2, k0 + 3)]:
        pts = [T[k % 4] for k in tri]
        if (pts[1] - pts[0]).cross(pts[2] - pts[0]).length > 1e-12:
            polys.append((pts, upv))
    out = []
    for pts, o in polys:
        if (pts[1] - pts[0]).cross(pts[2] - pts[0]).dot(o) < 0:
            pts = pts[::-1]
        out.append(pts)
    return out


# ---- the voxel model ------------------------------------------------------------------------------
class Vox:
    """Cells of one size `s`, each a palette colour name. Positions are in metres in the model's own
    frame; a cell (i, j, k) spans [i*s, (i+1)*s) on each axis."""

    def __init__(self, s, ups=()):
        self.s = s; self.ups = list(ups); self.v = {}

    def r(self, lo, hi):
        s = self.s
        return range(math.floor(lo / s + 1e-6), math.ceil(hi / s - 1e-6))

    def centre(self, i, j, k):
        s = self.s
        return Vector(((i + 0.5) * s, (j + 0.5) * s, (k + 0.5) * s))

    def put(self, i, j, k, col):
        self.v[i, j, k] = col

    def box(self, lo, hi, col):
        for i in self.r(lo[0], hi[0]):
            for j in self.r(lo[1], hi[1]):
                for k in self.r(lo[2], hi[2]):
                    c = col(i, j, k) if callable(col) else col
                    if c:
                        self.v[i, j, k] = c

    def cut(self, lo, hi):
        for i in self.r(lo[0], hi[0]):
            for j in self.r(lo[1], hi[1]):
                for k in self.r(lo[2], hi[2]):
                    self.v.pop((i, j, k), None)

    def ball(self, c, rad, col, wobble=0.0, seed=0.0):
        """A solid ellipsoid; `wobble` roughens its surface with noise (a fraction of the radius)."""
        c = Vector(c)
        rad = Vector((rad, rad, rad)) if isinstance(rad, (int, float)) else Vector(rad)
        m = 1.0 + wobble
        for i in self.r(c.x - rad.x * m, c.x + rad.x * m):
            for j in self.r(c.y - rad.y * m, c.y + rad.y * m):
                for k in self.r(c.z - rad.z * m, c.z + rad.z * m):
                    p = self.centre(i, j, k)
                    d = Vector(((p.x - c.x) / rad.x, (p.y - c.y) / rad.y, (p.z - c.z) / rad.z)).length
                    lim = 1.0 + (wobble * noise.noise(p * (1.6 / max(rad)) + Vector((seed, seed, seed))) if wobble else 0.0)
                    if d <= lim:
                        self.v[i, j, k] = col(p) if callable(col) else col

    def rod(self, a, b, rad, col, flat=False):
        a, b = Vector(a), Vector(b); ab = b - a; L2 = max(ab.length_squared, 1e-12)
        rm = max(rad(t / 10) for t in range(11)) if callable(rad) else rad
        lo = Vector([min(a[x], b[x]) - rm for x in range(3)]); hi = Vector([max(a[x], b[x]) + rm for x in range(3)])
        for i in self.r(lo.x, hi.x):
            for j in self.r(lo.y, hi.y):
                for k in self.r(lo.z, hi.z):
                    p = self.centre(i, j, k)
                    t0 = (p - a).dot(ab) / L2
                    if flat and not (0.0 <= t0 <= 1.0):
                        continue
                    t = max(0.0, min(1.0, t0))
                    rr = rad(t) if callable(rad) else rad
                    if (p - (a + ab * t)).length <= rr:
                        self.v[i, j, k] = col(p, t) if callable(col) else col

    def shade(self, match, top, body, under, var=None, k=0.0, seed=1.0):
        """Recolour cells of the colours in `match` by light: open above gets `top`, open below
        `under`, the rest `body`, a noise share of it `var` (lit tops, shaded undersides)."""
        for (i, j, kk), c in list(self.v.items()):
            if c not in match:
                continue
            if (i, j, kk + 1) not in self.v:
                n = top
            elif (i, j, kk - 1) not in self.v:
                n = under
            else:
                n = body
                if var and noise.noise(Vector((i * 0.37 + seed, j * 0.37, kk * 0.37))) > 1 - 2 * k:
                    n = var
            self.v[i, j, kk] = n

    def mesh(self, name, loc=(0, 0, 0), rot=0.0, glow=(), scale=1.0):
        """Mesh the cells and link the object at `loc`, turned `rot` degrees about z."""
        s = self.s; glow = set(glow)
        shp = shape_cells(self.v, self.ups) if self.ups else {}
        full = {c for c in self.v if c not in shp}
        faces = {}
        for p, c in self.v.items():
            if p in shp:
                continue
            for ax in range(3):
                for sg in (1, -1):
                    if covers(self.v, shp, _add(p, ax, sg), ax, -sg):
                        continue
                    u, w = [a for a in range(3) if a != ax]
                    faces.setdefault((ax, sg, p[ax]), {})[(p[u], p[w])] = c
        bm = bmesh.new(); cols = []

        def add(pts, c):
            try:
                bm.faces.new([bm.verts.new(q) for q in pts]); cols.append(c)
            except ValueError:
                pass
        for (ax, sg, sl), cells in faces.items():
            u, w = [a for a in range(3) if a != ax]
            done = set()
            for key in sorted(cells):
                if key in done:
                    continue
                c = cells[key]; u0, v0 = key
                wd = 1
                while cells.get((u0 + wd, v0)) == c and (u0 + wd, v0) not in done:
                    wd += 1
                ht = 1
                while all(cells.get((u0 + du, v0 + ht)) == c and (u0 + du, v0 + ht) not in done for du in range(wd)):
                    ht += 1
                for du in range(wd):
                    for dv in range(ht):
                        done.add((u0 + du, v0 + dv))
                plane = (sl + (1 if sg > 0 else 0)) * s
                pts = []
                for (cu, cv) in [(u0, v0), (u0 + wd, v0), (u0 + wd, v0 + ht), (u0, v0 + ht)]:
                    co = [0.0, 0.0, 0.0]; co[ax] = plane; co[u] = cu * s; co[w] = cv * s
                    pts.append(Vector(co))
                n = Vector((0, 0, 0)); n[ax] = sg
                if (pts[1] - pts[0]).cross(pts[2] - pts[0]).dot(n) < 0:
                    pts = pts[::-1]
                add(pts, c)
        for p, sh in shp.items():
            for pts in shaped_faces(p, sh, s, full, self.v, shp):
                add(pts, self.v[p])
        if EXPORT is not None:
            key = export_part(self, name, bm, cols, glow)
        me = bpy.data.meshes.new(name); bm.to_mesh(me); bm.free()
        me.materials.append(VCOL); me.materials.append(VGLOW)
        ca = me.color_attributes.new("C", "FLOAT_COLOR", "CORNER")
        for poly, c in zip(me.polygons, cols):
            poly.use_smooth = False; poly.material_index = 1 if c in glow else 0
            rgb = lin(PALETTE[c])
            for li in poly.loop_indices:
                ca.data[li].color = (*rgb, 1.0)
        o = bpy.data.objects.new(name, me); bpy.context.scene.collection.objects.link(o)
        o.location = loc; o.rotation_euler = (0, 0, math.radians(rot)); o.scale = (scale,) * 3
        if EXPORT is not None:
            o["block_model"] = key
        return o


# ---- export: block models for the game (block-world.md section 10) ------------------------------
EXPORT = None    # set to {} by the scene when exporting: model key -> model file content


def export_part(v, name, bm, cols, glow):
    """Records `v` as a block model (deduplicated by content) and returns its key."""
    import hashlib
    import json
    keys = list(v.v)
    lo = [min(k[a] for k in keys) for a in range(3)]
    hi = [max(k[a] for k in keys) + 1 for a in range(3)]
    dims = [hi[a] - lo[a] for a in range(3)]
    names = sorted(set(v.v.values()))
    index = {n: i + 1 for i, n in enumerate(names)}
    runs = []; last = None; count = 0
    for k in range(lo[2], hi[2]):
        for j in range(lo[1], hi[1]):
            for i in range(lo[0], hi[0]):
                c = index.get(v.v.get((i, j, k)), 0)
                if c == last:
                    count += 1
                else:
                    if last is not None:
                        runs += [count, last]
                    last, count = c, 1
    runs += [count, last]
    area = {}
    for f, c in zip(bm.faces, cols):
        area[c] = area.get(c, 0.0) + f.calc_area()
    ups = [("+" if sg > 0 else "-") + "xyz"[a] for (a, sg) in v.ups]
    part = {"size": v.s, "ups": ups, "origin": lo, "dims": dims, "palette": names, "runs": runs,
            "glow": sorted(g for g in glow if g in index), "reference": {"area": {k: round(a, 6) for k, a in sorted(area.items())}}}
    digest = hashlib.sha256(json.dumps(part, sort_keys=True).encode()).hexdigest()[:8]
    key = f"{name}_{digest}"
    EXPORT.setdefault(key, {"name": key, "parts": [part]})
    return key
