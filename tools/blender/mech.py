"""Hard-surface building blocks for part models, run inside Blender (bpy, bmesh).

Models are built in part axes (metres, Y up, as the game and the glTF file use them; Blender's
own Z-up convention plays no part). Every object carries one material, named by its slot in
`content/data/part_materials.json`. The visual language (design/vehicle-builder.md §7.3): a
dark skeletal frame under sharp, faceted armour plates (hulls, chamfered plates), sealed
octagonal actuators, and energy that glows in seams, joint gaps and ribbed conduits; no
bolts, hoses or stencils. Cuts (grooves, slits) are booleans. A finished model is frozen,
its ambient occlusion baked into vertex colours (Cycles, with every piece of the part
shadowing the others) and written as a single-mesh glTF binary, one primitive per material:
the layout `game_bevy/src/part_models.rs` and the design viewer read.
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

    def hull(self, points, mat="armor", place=None, bevel=0.008, segments=1, angle=30.0,
             cutter=False):
        """The convex hull of `points` (in the placement's axes): faceted solids, wedges and
        blades. Coplanar triangles are merged into flat facets."""
        bm = bmesh.new()
        for p in points:
            bm.verts.new(vec(p))
        res = bmesh.ops.convex_hull(bm, input=bm.verts[:])
        drop = list({g for g in res["geom_interior"] + res["geom_unused"] if isinstance(g, bmesh.types.BMVert)})
        if drop:
            bmesh.ops.delete(bm, geom=drop, context="VERTS")
        bmesh.ops.dissolve_limit(bm, angle_limit=math.radians(0.5), verts=bm.verts[:], edges=bm.edges[:])
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        return self._object(bm, mat, place if place is not None else frame(), bevel, segments, angle,
                            cutter=cutter)

    def plate(self, outline, thickness, place, mat="armor", chamfer=None, bevel=0.006,
              cutter=False):
        """An armour plate: a flat outline ((x, y) in the placement's plane) given
        `thickness` along its +Z from z = 0, the outer face chamfered all round."""
        pts = list(outline)
        area = sum(x0 * y1 - x1 * y0 for (x0, y0), (x1, y1) in zip(pts, pts[1:] + pts[:1]))
        if area < 0:
            pts.reverse()
        ch = min(thickness * 0.6, 0.08) if chamfer is None else chamfer
        bm = bmesh.new()
        face = bm.faces.new([bm.verts.new((x, y, 0.0)) for x, y in pts])
        ext = bmesh.ops.extrude_face_region(bm, geom=[face])
        moved = [e for e in ext["geom"] if isinstance(e, bmesh.types.BMVert)]
        top = [e for e in ext["geom"] if isinstance(e, bmesh.types.BMFace)]
        bmesh.ops.translate(bm, verts=moved, vec=(0, 0, thickness - ch))
        bm.normal_update()
        if ch > 0:
            bmesh.ops.inset_region(bm, faces=top, thickness=ch, depth=ch)
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        return self._object(bm, mat, place, bevel, 1, 30.0, cutter=cutter)

    def facet(self, corners, thickness, inset=0.06, outward=(0, 0, 0), mat="armor",
              chamfer=None, bevel=0.005):
        """An armour plate lying on a face of a faceted body: `corners` (roughly planar, in
        order) shrunk by `inset` (a fraction) toward their centre, raised `thickness` away
        from the `outward` point and chamfered. Returns the plate, its outer centre and
        normal."""
        pts = [vec(p) for p in corners]
        c = sum(pts, V((0, 0, 0))) / len(pts)
        n = V((0, 0, 0))
        for p, q in zip(pts, pts[1:] + pts[:1]):
            n += (p - c).cross(q - c)
        n.normalize()
        if n.dot(c - vec(outward)) < 0:
            n = -n
        pts = [p - n * (p - c).dot(n) for p in pts]
        size = sum(((p - c).length for p in pts)) / len(pts)
        ch = min(thickness * 0.6, 0.06) if chamfer is None else chamfer
        f = 1 - ch / max(size, 1e-6)
        base = [c + (p - c) * (1 - inset) for p in pts]
        top = [c + (p - c) * (1 - inset) * f + n * thickness for p in pts]
        mid = [b + n * (thickness - ch) for b in base]
        ob = self.hull(base + mid + top, mat, bevel=bevel)
        return ob, c + n * thickness, n

    def ring_glow(self, centre, axis, r_in, r_out, width, sides=8):
        """An emissive ring (a joint's energy gap) about `axis` through `centre`."""
        c = vec(centre)
        place = along(c, c + vec(axis))
        return self.lathe([(r_in, -width / 2), (r_out, -width / 2), (r_out, width / 2),
                           (r_in, width / 2)], place, "light", sides, closed=True)

    def drum(self, a, b, r, mat="dark", sides=8, glow=True, hint=None):
        """An enclosed rotary actuator from `a` to `b`: a faceted housing with chamfered
        ends, split round its middle by a glowing energy gap."""
        a, b = vec(a), vec(b)
        place = along(a, b, hint)
        length = (b - a).length
        ch = min(r * 0.2, length * 0.18)
        g = min(r * 0.18, length * 0.12) if glow else 0.0
        half = (length - g) / 2
        self.lathe([(0, 0), (r - ch, 0), (r, ch), (r, half), (0, half)], place, mat, sides,
                   angle=20.0)
        self.lathe([(0, length - half), (r, length - half), (r, length - ch), (r - ch, length),
                    (0, length)], place, mat, sides, angle=20.0)
        if glow:
            self.lathe([(0, half - 0.001), (r * 0.9, half - 0.001), (r * 0.9, length - half + 0.001),
                        (0, length - half + 0.001)], place, "light", sides)
        return place

    def piston(self, a, b, r, extension=0.4, hint=None):
        """A sealed linear actuator from `a` (sleeve) to `b` (rod): a hexagonal sleeve with
        chamfered ends, a glowing gland, the polished rod, faceted eyes on their pins."""
        a, b = vec(a), vec(b)
        place = along(a, b, hint)
        length = (b - a).length
        eye = r * 1.1
        y0 = eye * 0.8
        y1 = y0 + (length - 2 * eye) * (1 - extension * 0.5)
        self.lathe([(0, y0), (r * 0.6, y0), (r, y0 + r * 0.6), (r, y1 - r * 0.5), (r * 0.72, y1),
                    (0, y1)], place, "metal", 6, angle=20.0)
        self.lathe([(r * 0.42, y1), (r * 0.66, y1), (r * 0.66, y1 + r * 0.25), (r * 0.42, y1 + r * 0.25)],
                   place, "light", 6, closed=True)
        self.cyl(place @ V((0, y1, 0)), place @ V((0, length - eye * 0.7, 0)), r * 0.38, "chrome", 12)
        u = (place.to_3x3() @ X).normalized()
        for y in (0.0, length):
            c = place @ V((0, y, 0))
            self.cyl(c - u * eye * 0.5, c + u * eye * 0.5, eye * 0.75, "dark", 6, bevel=eye * 0.08,
                     segments=1)
        return place

    def conduit(self, points, r, spacing=None):
        """A ribbed power conduit through `points`: a glowing core sheathed in dark rings."""
        pts = [vec(p) for p in points]
        spacing = spacing or r * 1.7
        for p, q in zip(pts, pts[1:]):
            d = q - p
            n = max(1, int(d.length / spacing))
            self.cyl(p, q, r * 0.42, "light", 6)
            for k in range(n):
                c = p + d * ((k + 0.5) / n)
                self.cyl(c - d.normalized() * r * 0.32, c + d.normalized() * r * 0.32, r, "dark", 6,
                         bevel=r * 0.15, segments=1)

    def slit(self, target, a, b, normal, width=0.03, depth=0.04):
        """A glowing seam: a groove from `a` to `b` with an emissive strip down in it."""
        a, b, n = vec(a), vec(b), vec(normal).normalized()
        d = b - a
        self.groove(target, a, b, n, width, depth)
        self.box((d.length - width * 0.5, width * 0.55, depth * 0.5),
                 place=frame((a + b) / 2 - n * depth * 0.7, u=d, w=n), mat="light", bevel=0)

    def sensor(self, centre, normal, up, count=3, r=0.05, mat="armor2"):
        """A sensor cluster: lenses in a row on a faceted mount, each with a glowing iris."""
        c, n = vec(centre), vec(normal).normalized()
        u = vec(up)
        u = (u - n * u.dot(n)).normalized()
        a = u.cross(n)
        w = r * 2.6 * count
        self.hull([c + a * sx * w / 2 + u * sy * r * 1.6 + n * dz
                   for sx in (-1, 1) for sy in (-1, 1) for dz in (0, r * 0.6)]
                  + [c + a * sx * (w / 2 + r) + n * r * 0.3 for sx in (-1, 1)], mat, bevel=0.004)
        for k in range(count):
            p = c + a * ((k - (count - 1) / 2) * r * 2.6) + n * r * 0.6
            self.cyl(p - n * 0.01, p + n * r * 0.4, r, "glass", 8)
            self.cyl(p + n * r * 0.4, p + n * r * 0.45, r * 0.35, "light", 8)

    # Cuts -------------------------------------------------------------------------------

    def cut(self, target, cutter):
        m = target.modifiers.new("cut", "BOOLEAN")
        m.operation = "DIFFERENCE"
        m.object = cutter
        m.solver = "EXACT"
        return target

    def cut_box(self, target, size, at=(0, 0, 0), place=None):
        return self.cut(target, self.box(size, at, place=place, bevel=0, cutter=True))

    def groove(self, target, a, b, normal, width=0.025, depth=0.025):
        """A panel line from `a` to `b` on a face with outward `normal`."""
        a, b, n = vec(a), vec(b), vec(normal).normalized()
        d = b - a
        place = frame((a + b) / 2, u=d, w=n)
        return self.cut_box(target, (d.length, width, depth * 2), place=place)

    # Details ----------------------------------------------------------------------------

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
        densify(me)
        fo = bpy.data.objects.new(ob.name, me)
        fo.matrix_world = ob.matrix_world.copy()
        fo["angle"] = float(ob.get("bevel", (0.0, 1, 30.0))[2])
        frozen.objects.link(fo)
        out.append(fo)
    model.pieces.hide_render = True
    return out


def densify(me, longest=0.5, rounds=4):
    """Splits long edges (flat faces stay flat) so the occlusion baked per corner has
    enough points to follow: big low-poly facets otherwise show diagonal smears."""
    bm = bmesh.new()
    bm.from_mesh(me)
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    for _ in range(rounds):
        long_ = [e for e in bm.edges if e.calc_length() > longest]
        if not long_:
            break
        bmesh.ops.subdivide_edges(bm, edges=long_, cuts=1, use_grid_fill=False)
        bmesh.ops.triangulate(bm, faces=[f for f in bm.faces if len(f.verts) > 3])
    bm.to_mesh(me)
    bm.free()


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
        normals = smooth_normals(me, min(35.0, o.get("angle", 35.0)))
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
