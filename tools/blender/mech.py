"""Hard-surface building blocks for part models, run inside Blender (bpy, bmesh).

Models are built in part axes (metres, Y up, as the game and the glTF file use them; Blender's
own Z-up convention plays no part). Every object carries one material, named by its slot in
`content/data/part_materials.json`. Edges are bevelled and normals hardened, cuts (recesses,
holes, panel grooves) are booleans, turned parts are lathed profiles, hoses are curves. A
finished model is frozen, its ambient occlusion baked into vertex colours (Cycles, with every
piece of the part shadowing the others) and written as a single-mesh glTF binary, one
primitive per material: the layout `game_bevy/src/part_models.rs` and the design viewer read.
"""

import json
import math
import struct

import bmesh
import bpy
from mathutils import Matrix, Vector

V = Vector
X, Y, Z = V((1, 0, 0)), V((0, 1, 0)), V((0, 0, 1))


def vec(p):
    return p if isinstance(p, Vector) else V(p)


def frame(origin=(0, 0, 0), u=None, v=None, w=None):
    """A placement: origin and axes (any two given; right-handed)."""
    if u is not None and v is not None:
        u, v = vec(u).normalized(), vec(v).normalized()
        w = u.cross(v).normalized()
        v = w.cross(u)
    elif v is not None and w is not None:
        v, w = vec(v).normalized(), vec(w).normalized()
        u = v.cross(w).normalized()
        w = u.cross(v)
    elif u is not None and w is not None:
        u, w = vec(u).normalized(), vec(w).normalized()
        v = w.cross(u).normalized()
        w = u.cross(v)
    else:
        u, v, w = X.copy(), Y.copy(), Z.copy()
    m = Matrix.Identity(4)
    for i in range(3):
        m[i][0], m[i][1], m[i][2], m[i][3] = u[i], v[i], w[i], vec(origin)[i]
    return m


def along(a, b, hint=None):
    """A placement at `a` with its Y axis toward `b` (X as near `hint` as it gets)."""
    a, b = vec(a), vec(b)
    d = (b - a).normalized()
    h = vec(hint) if hint is not None else (X if abs(d.x) < 0.9 else Z)
    u = h - d * h.dot(d)
    if u.length < 1e-6:
        u = Z - d * Z.dot(d) if abs(d.z) < 0.9 else X - d * X.dot(d)
    return frame(a, u=u, v=d)


# --- Materials ----------------------------------------------------------------------------


def palette(path):
    return json.loads(open(path).read())["materials"]


_materials = {}


def material(name):
    if name not in _materials:
        m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
        _materials[name] = m
    return _materials[name]


# --- The model being built ----------------------------------------------------------------


class Model:
    """A part model: pieces (one material each) and the cutters that carve them."""

    def __init__(self, name):
        self.name = name
        scene = bpy.context.scene
        self.pieces = bpy.data.collections.new(name)
        self.cutters = bpy.data.collections.new(name + ".cutters")
        scene.collection.children.link(self.pieces)
        scene.collection.children.link(self.cutters)
        self.cutters.hide_render = True
        self.cutters.hide_viewport = True

    # Pieces -----------------------------------------------------------------------------

    def _object(self, bm, mat, matrix, bevel=0.0, segments=2, angle=40.0, cutter=False):
        me = bpy.data.meshes.new(self.name)
        sharp_by_angle(bm, angle)
        bm.to_mesh(me)
        bm.free()
        ob = bpy.data.objects.new(self.name, me)
        ob.matrix_world = matrix
        (self.cutters if cutter else self.pieces).objects.link(ob)
        if not cutter:
            me.materials.append(material(mat))
            ob["bevel"] = (bevel, segments, angle)
        return ob

    def box(self, size, at=(0, 0, 0), mat="armor", place=None, bevel=0.03, segments=2,
            taper=None, cutter=False):
        """A box (full sizes) centred at `at` (or on `place`); `taper` (x, z) scales the top
        (+Y) face."""
        bm = bmesh.new()
        bmesh.ops.create_cube(bm, size=1.0)
        for v in bm.verts:
            v.co.x *= size[0]
            v.co.y *= size[1]
            v.co.z *= size[2]
            if taper and v.co.y > 0:
                v.co.x *= taper[0]
                v.co.z *= taper[1]
        m = place if place is not None else frame(at)
        return self._object(bm, mat, m, bevel, segments, cutter=cutter)

    def cyl(self, a, b, r, mat="metal", sides=24, r2=None, bevel=0.0, segments=2, hint=None,
            cutter=False):
        """A cylinder (or cone to `r2`) from `a` to `b`."""
        a, b = vec(a), vec(b)
        length = (b - a).length
        bm = bmesh.new()
        bmesh.ops.create_cone(bm, cap_ends=True, segments=sides, radius1=r,
                              radius2=r if r2 is None else r2, depth=length)
        # create_cone runs along Z, centred: turn it to run along Y from 0 to length.
        bmesh.ops.rotate(bm, verts=bm.verts, cent=(0, 0, 0), matrix=Matrix.Rotation(-math.pi / 2, 3, "X"))
        bmesh.ops.translate(bm, verts=bm.verts, vec=(0, length / 2, 0))
        return self._object(bm, mat, along(a, b, hint), bevel, segments, angle=30.0, cutter=cutter)

    def lathe(self, profile, place, mat="metal", sides=32, bevel=0.0, angle=30.0, closed=False):
        """A turned part: `profile` is (radius, height) points from one end to the other,
        revolved about the placement's Y axis; ends with a radius are capped. A `closed`
        profile (a loop, e.g. a ring's section) joins its last point to its first instead."""
        bm = bmesh.new()
        rings = []
        for r, y in profile:
            ring = []
            for k in range(sides):
                t = 2 * math.pi * k / sides
                ring.append(bm.verts.new((r * math.cos(t), y, -r * math.sin(t))))
            rings.append(ring)
        pairs = list(zip(rings, rings[1:])) + ([(rings[-1], rings[0])] if closed else [])
        for r0, r1 in pairs:
            for k in range(sides):
                j = (k + 1) % sides
                bm.faces.new((r0[k], r0[j], r1[j], r1[k]))
        if not closed and profile[0][0] > 1e-6:
            bm.faces.new(list(reversed(rings[0])))
        if not closed and profile[-1][0] > 1e-6:
            bm.faces.new(rings[-1])
        bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-6)
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        return self._object(bm, mat, place, bevel, 2, angle)

    def extrude(self, outline, depth, place, mat="armor", bevel=0.02, segments=2,
                cutter=False):
        """A flat profile (`outline`: (x, y) points, counter-clockwise) extruded `depth` along
        the placement's Z, centred."""
        bm = bmesh.new()
        verts = [bm.verts.new((x, y, -depth / 2)) for x, y in outline]
        face = bm.faces.new(verts)
        ext = bmesh.ops.extrude_face_region(bm, geom=[face])
        moved = [e for e in ext["geom"] if isinstance(e, bmesh.types.BMVert)]
        bmesh.ops.translate(bm, verts=moved, vec=(0, 0, depth))
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        return self._object(bm, mat, place, bevel, segments, cutter=cutter)

    def hose(self, points, r, mat="rubber", resolution=10):
        """A hose through `points` (a smooth curve)."""
        cu = bpy.data.curves.new(self.name, "CURVE")
        cu.dimensions = "3D"
        cu.bevel_depth = r
        cu.bevel_resolution = 3
        cu.use_fill_caps = True
        sp = cu.splines.new("NURBS")
        sp.points.add(len(points) - 1)
        for p, q in zip(sp.points, points):
            p.co = (*vec(q), 1.0)
        sp.use_endpoint_u = True
        sp.order_u = min(4, len(points))
        sp.resolution_u = resolution
        ob = bpy.data.objects.new(self.name, cu)
        self.pieces.objects.link(ob)
        cu.materials.append(material(mat))
        ob["bevel"] = (0.0, 1, 30.0)
        return ob

    # Cuts -------------------------------------------------------------------------------

    def cut(self, target, cutter):
        m = target.modifiers.new("cut", "BOOLEAN")
        m.operation = "DIFFERENCE"
        m.object = cutter
        m.solver = "EXACT"
        return target

    def cut_box(self, target, size, at=(0, 0, 0), place=None):
        return self.cut(target, self.box(size, at, place=place, bevel=0, cutter=True))

    def hole(self, target, a, b, r, sides=24):
        return self.cut(target, self.cyl(a, b, r, sides=sides, cutter=True))

    def groove(self, target, a, b, normal, width=0.025, depth=0.025):
        """A panel line from `a` to `b` on a face with outward `normal`."""
        a, b, n = vec(a), vec(b), vec(normal).normalized()
        d = b - a
        place = frame((a + b) / 2, u=d, w=n)
        return self.cut_box(target, (d.length, width, depth * 2), place=place)

    # Details ----------------------------------------------------------------------------

    def bolts(self, centre, normal, radius, count, size=0.035, mat="metal", phase=0.0,
              up=None):
        """A ring of hex bolt heads with washers on a face (`normal` out of it)."""
        c, n = vec(centre), vec(normal).normalized()
        u = (vec(up) if up is not None else (Y if abs(n.y) < 0.9 else X))
        u = (u - n * u.dot(n)).normalized()
        w = n.cross(u)
        for k in range(count):
            t = 2 * math.pi * (k + phase) / count
            p = c + (u * math.cos(t) + w * math.sin(t)) * radius
            self.bolt(p, n, size, mat)

    def bolt(self, p, normal, size=0.035, mat="metal"):
        p, n = vec(p), vec(normal).normalized()
        self.cyl(p, p + n * size * 0.25, size * 1.45, mat, sides=12)
        self.cyl(p + n * size * 0.25, p + n * size * 0.95, size, mat, sides=6, bevel=size * 0.12,
                 segments=1)

    def bolt_row(self, a, b, normal, count, size=0.035, mat="metal"):
        a, b = vec(a), vec(b)
        for k in range(count):
            self.bolt(a.lerp(b, (k + 0.5) / count), normal, size, mat)

    def ram(self, a, b, r, extension=0.5, hint=None, ports=True):
        """A double-acting hydraulic ram from `a` (barrel eye) to `b` (rod eye): a turned
        barrel with its end cap and hose ports, the gland nut, the polished rod and a forged
        eye with a pin at each end."""
        a, b = vec(a), vec(b)
        d = (b - a)
        length = d.length
        place = along(a, b, hint)
        eye = r * 1.05
        barrel = (length - 2 * eye) * (1 - extension * 0.5)
        y0, y1 = eye * 0.9, eye * 0.9 + barrel
        self.lathe([(0, y0 - 0.01), (r * 0.85, y0 - 0.01), (r * 1.0, y0 + r * 0.25),
                    (r * 1.0, y0 + r * 0.6), (r * 0.9, y0 + r * 0.7), (r * 0.9, y1 - r * 0.7),
                    (r * 1.0, y1 - r * 0.6), (r * 1.0, y1), (r * 0.62, y1 + r * 0.25),
                    (r * 0.55, y1 + r * 0.45), (0, y1 + r * 0.45)], place, "dark", 24,
                   bevel=r * 0.04)
        self.lathe([(0, y1 + r * 0.2), (r * 0.66, y1 + r * 0.2), (r * 0.66, y1 + r * 0.55),
                    (0, y1 + r * 0.55)], place, "metal", 8)
        rod = r * 0.45
        self.cyl(place @ V((0, y1, 0)), place @ V((0, length - eye * 0.9, 0)), rod, "chrome", 16)
        u = (place.to_3x3() @ X).normalized()
        for y, mat in ((0.0, "dark"), (length, "metal")):
            c = place @ V((0, y, 0))
            self.cyl(c - u * eye * 0.55, c + u * eye * 0.55, eye, mat, 16, bevel=eye * 0.12)
            self.cyl(c - u * eye * 0.75, c + u * eye * 0.75, eye * 0.38, "chrome", 12)
        if ports:
            w = (place.to_3x3() @ Z).normalized()
            for y in (y0 + r * 0.45, y1 - r * 0.45):
                c = place @ V((0, y, 0))
                self.cyl(c + w * r * 0.7, c + w * r * 1.3, r * 0.22, "metal", 6)
        return place

    def detail(self, target, centre, normal, up, width, height, seed=0, hatch=True, grille=True):
        """Surface detail on a flat face of `target` (centre on the face, `normal` out of it,
        `up` along its height): panel lines splitting it, a recessed access hatch with a bolted
        cover, a slotted grille, a rivet row and a small data plate. Deterministic per
        `seed`."""
        import random
        rnd = random.Random(seed)
        c, n = vec(centre), vec(normal).normalized()
        u = vec(up)
        u = (u - n * u.dot(n)).normalized()
        a = u.cross(n)
        if min(width, height) < 0.25:
            return
        xs = rnd.uniform(-0.22, 0.22) * width
        ys = rnd.uniform(-0.18, 0.18) * height
        g = min(0.02, 0.012 + 0.004 * min(width, height))
        self.groove(target, c + a * xs - u * height * 0.5, c + a * xs + u * height * 0.5, n, g, g)
        self.groove(target, c + u * ys - a * width * 0.5, c + u * ys + a * width * 0.5, n, g, g)
        quads = [(sx, sy) for sx in (-1, 1) for sy in (-1, 1)]
        rnd.shuffle(quads)

        def quad_centre(q):
            sx, sy = q
            x0, x1 = (-width / 2, xs) if sx < 0 else (xs, width / 2)
            y0, y1 = (-height / 2, ys) if sy < 0 else (ys, height / 2)
            return (x0 + x1) / 2, (y0 + y1) / 2, x1 - x0, y1 - y0

        if hatch:
            qx, qy, qw, qh = quad_centre(quads[0])
            hw, hh = qw * 0.55, qh * 0.5
            hc = c + a * qx + u * qy
            place = frame(hc, u=a, v=u)
            self.cut_box(target, (hw, hh, 0.04), place=place)
            self.box((hw * 0.94, hh * 0.92, 0.018), place=frame(hc - n * 0.004, u=a, v=u), mat="armor2",
                     bevel=0.005, segments=1)
            for sx in (-1, 1):
                for sy in (-1, 1):
                    self.bolt(hc + a * (sx * hw * 0.38) + u * (sy * hh * 0.36) + n * 0.005, n, 0.016)
            self.box((hw * 0.25, 0.02, 0.015), place=frame(hc + u * (hh * 0.25) + n * 0.01, u=a, v=u),
                     mat="metal", bevel=0.003, segments=1)
        if grille:
            qx, qy, qw, qh = quad_centre(quads[1])
            gc = c + a * qx + u * qy
            slots = 4
            for k in range(slots):
                y = (-0.5 + (k + 0.5) / slots) * qh * 0.5
                self.cut_box(target, (qw * 0.5, qh * 0.05, 0.05), place=frame(gc + u * y, u=a, v=u))
        qx, qy, qw, qh = quad_centre(quads[2])
        self.bolt_row(c + a * (qx - qw * 0.4) + u * (qy + qh * 0.4), c + a * (qx + qw * 0.4) + u * (qy + qh * 0.4),
                      n, max(2, int(qw / 0.12)), size=0.014)
        qx, qy, qw, qh = quad_centre(quads[3])
        pc = c + a * qx + u * qy + n * 0.004
        self.box((min(0.3, qw * 0.4), min(0.12, qh * 0.25), 0.008), place=frame(pc, u=a, v=u),
                 mat=rnd.choice(["metal", "trim"]), bevel=0.002, segments=1)

    def lug(self, base, tip, width, thickness, normal, mat="armor", hole=None):
        """A clevis lug: a plate from `base` (its straight edge) to a rounded end around
        `tip`, `normal` through its thickness; `hole` the pin radius."""
        base, tip, n = vec(base), vec(tip), vec(normal).normalized()
        d = tip - base
        h = d.length
        r = width / 2
        pts = [(-r, 0.0), (r, 0.0)]
        for k in range(13):
            t = math.pi * k / 12
            pts.append((r * math.cos(t), h + r * math.sin(t)))
        pts.append((-r, 0.0))
        pts = pts[:-1]
        place = frame(base, v=d, w=n)
        ob = self.extrude(pts, thickness, place, mat, bevel=min(0.02, thickness * 0.2))
        if hole:
            self.hole(ob, tip - n * thickness, tip + n * thickness, hole)
        return ob


def sharp_by_angle(bm, degrees):
    """Marks edges sharper than `degrees` sharp; faces smooth (so curved parts shade
    smoothly and hard edges stay crisp)."""
    limit = math.radians(degrees)
    for f in bm.faces:
        f.smooth = True
    bm.normal_update()
    for e in bm.edges:
        if len(e.link_faces) == 2:
            e.smooth = e.calc_face_angle(0.0) < limit
        else:
            e.smooth = False


# --- Finishing: freeze, bake, write ----------------------------------------------------------


def clear():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    _materials.clear()


def freeze(model):
    """Applies every piece's cuts and bevel; returns the frozen mesh objects."""
    for ob in model.pieces.objects:
        bevel, segments, angle = ob.get("bevel", (0.0, 1, 30.0))
        if ob.type == "MESH" and bevel > 0:
            m = ob.modifiers.new("bevel", "BEVEL")
            m.width = bevel
            m.segments = int(segments)
            m.limit_method = "ANGLE"
            m.angle_limit = math.radians(angle)
            m.harden_normals = True
            m.miter_outer = "MITER_ARC"
            # Bevel the solid first, then cut: bevelling boolean seams goes wrong.
            ob.modifiers.move(len(ob.modifiers) - 1, 0)
    dg = bpy.context.evaluated_depsgraph_get()
    frozen = bpy.data.collections.new(model.name + ".frozen")
    bpy.context.scene.collection.children.link(frozen)
    out = []
    for ob in list(model.pieces.objects):
        ev = ob.evaluated_get(dg)
        me = bpy.data.meshes.new_from_object(ev, preserve_all_data_layers=True, depsgraph=dg)
        if not me.polygons:
            continue
        if ob.type == "CURVE":
            for p in me.polygons:
                p.use_smooth = True
        fo = bpy.data.objects.new(ob.name, me)
        fo.matrix_world = ob.matrix_world.copy()
        frozen.objects.link(fo)
        out.append(fo)
    model.pieces.hide_render = True
    return out


def bake_ao(objects, distance=0.6, samples=48):
    """Bakes ambient occlusion into a colour attribute `ao` on each object (every object
    shadows the others)."""
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = samples
    if scene.world is None:
        scene.world = bpy.data.worlds.new("world")
    scene.world.light_settings.distance = distance
    for o in scene.objects:
        o.select_set(False)
    for o in objects:
        me = o.data
        attr = me.color_attributes.new("ao", "FLOAT_COLOR", "CORNER")
        me.color_attributes.active_color = attr
        o.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    bpy.ops.object.bake(type="AO", target="VERTEX_COLORS")


def write_glb(objects, path, colours):
    """Writes the objects as one glTF mesh: a primitive per material with positions,
    normals and the baked occlusion as COLOR_0."""
    prims = {}
    for o in objects:
        me = o.data
        me.calc_loop_triangles()
        m = o.matrix_world
        nm = m.to_3x3().inverted().transposed()
        ao = me.color_attributes.get("ao")
        normals = smooth_normals(me)
        name = me.materials[0].name if me.materials else "armor"
        verts, nrms, cols, idx, seen = prims.setdefault(name, ([], [], [], [], {}))
        for tri in me.loop_triangles:
            for loop in tri.loops:
                p = m @ me.vertices[me.loops[loop].vertex_index].co
                n = (nm @ normals[loop]).normalized()
                a = ao.data[loop].color[0] if ao else 1.0
                a = max(0.3, min(1.0, a))
                key = (round(p.x, 5), round(p.y, 5), round(p.z, 5), round(n.x, 3), round(n.y, 3),
                       round(n.z, 3), round(a, 2))
                i = seen.get(key)
                if i is None:
                    i = seen[key] = len(verts)
                    verts.append((p.x, p.y, p.z))
                    nrms.append((n.x, n.y, n.z))
                    cols.append((a, a, a, 1.0))
                idx.append(i)
    open(path, "wb").write(glb(prims, colours))
    return sum(len(v[3]) // 3 for v in prims.values())


def smooth_normals(me, degrees=35.0):
    """Corner normals: each corner averages (by area) the normals of the faces round its
    vertex that are within `degrees` of its own face; sharper edges stay hard."""
    limit = math.cos(math.radians(degrees))
    faces = [(p.normal.copy(), p.area) for p in me.polygons]
    around = [[] for _ in me.vertices]
    for i, p in enumerate(me.polygons):
        for v in p.vertices:
            around[v].append(i)
    out = [None] * len(me.loops)
    for i, p in enumerate(me.polygons):
        fn = faces[i][0]
        for li in p.loop_indices:
            v = me.loops[li].vertex_index
            acc = Vector((0.0, 0.0, 0.0))
            for j in around[v]:
                n, area = faces[j]
                if n.dot(fn) >= limit:
                    acc += n * max(area, 1e-9)
            out[li] = acc.normalized() if acc.length > 1e-12 else fn
    return out


def glb(prims, colours):
    binary = bytearray()
    views, accessors, primitives, materials = [], [], [], []

    def view(data, target):
        while len(binary) % 4:
            binary.append(0)
        views.append({"buffer": 0, "byteOffset": len(binary), "byteLength": len(data),
                      "target": target})
        binary.extend(data)
        return len(views) - 1

    for name in sorted(prims):
        verts, nrms, cols, idx, _ = prims[name]
        if not idx:
            continue
        acc = len(accessors)
        lo = [min(v[i] for v in verts) for i in range(3)]
        hi = [max(v[i] for v in verts) for i in range(3)]
        accessors.append({"bufferView": view(struct.pack(f"<{len(verts) * 3}f", *[c for v in verts for c in v]), 34962),
                          "componentType": 5126, "count": len(verts), "type": "VEC3", "min": lo, "max": hi})
        accessors.append({"bufferView": view(struct.pack(f"<{len(nrms) * 3}f", *[c for v in nrms for c in v]), 34962),
                          "componentType": 5126, "count": len(nrms), "type": "VEC3"})
        accessors.append({"bufferView": view(struct.pack(f"<{len(cols) * 4}f", *[c for v in cols for c in v]), 34962),
                          "componentType": 5126, "count": len(cols), "type": "VEC4"})
        accessors.append({"bufferView": view(struct.pack(f"<{len(idx)}I", *idx), 34963),
                          "componentType": 5125, "count": len(idx), "type": "SCALAR"})
        spec = colours.get(name, {"color": [0.6, 0.6, 0.6], "metallic": 0.5, "roughness": 0.5})
        mat = {"name": name, "pbrMetallicRoughness": {
            "baseColorFactor": [*(srgb_to_linear(c) for c in spec["color"]), 1.0],
            "metallicFactor": spec["metallic"], "roughnessFactor": spec["roughness"]}}
        if "emissive" in spec:
            mat["emissiveFactor"] = [srgb_to_linear(c) for c in spec["emissive"]]
        materials.append(mat)
        primitives.append({"attributes": {"POSITION": acc, "NORMAL": acc + 1, "COLOR_0": acc + 2},
                           "indices": acc + 3, "material": len(materials) - 1})
    while len(binary) % 4:
        binary.append(0)
    doc = {"asset": {"version": "2.0", "generator": "tools/blender"}, "scene": 0,
           "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
           "meshes": [{"primitives": primitives}], "materials": materials,
           "buffers": [{"byteLength": len(binary)}], "bufferViews": views, "accessors": accessors}
    text = json.dumps(doc, separators=(",", ":")).encode()
    while len(text) % 4:
        text += b" "
    chunks = struct.pack("<II", len(text), 0x4E4F534A) + text
    chunks += struct.pack("<II", len(binary), 0x004E4942) + bytes(binary)
    return struct.pack("<III", 0x46546C67, 2, 12 + len(chunks)) + chunks


def srgb_to_linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
