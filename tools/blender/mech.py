"""Hard-surface building blocks for part models, run inside Blender (bpy, bmesh).

Models are built in part axes (metres, Y up, as the game and the glTF file use them; Blender's
own Z-up convention plays no part). Every object carries one material, named by its slot in
`content/data/part_materials.json`. The visual language (design/vehicle-builder.md §7.3): a
dark skeletal frame under sharp, faceted armour plates (hulls, chamfered plates), sealed
octagonal actuators, armour bulked where it protects something; every detail does a job
(plate edges, hatches, vents, pin retainers, cables across joints), and only optics and
marker lamps glow. Cuts (grooves, slits) are booleans. A finished model is frozen,
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

    def drum(self, a, b, r, mat="dark", sides=8, hint=None):
        """A rotary housing (a joint's actuator or hub) from `a` to `b`: faceted, its ends
        chamfered."""
        a, b = vec(a), vec(b)
        place = along(a, b, hint)
        length = (b - a).length
        ch = min(r * 0.2, length * 0.3)
        self.lathe([(0, 0), (r - ch, 0), (r, ch), (r, length - ch), (r - ch, length), (0, length)],
                   place, mat, sides, angle=20.0)
        return place

    def piston(self, a, b, r, extension=0.4, hint=None):
        """A sealed linear actuator from `a` (sleeve) to `b` (rod): a hexagonal sleeve with
        chamfered ends, the gland collar, the polished rod, faceted eyes on their pins."""
        a, b = vec(a), vec(b)
        place = along(a, b, hint)
        length = (b - a).length
        eye = r * 1.1
        y0 = eye * 0.8
        y1 = y0 + (length - 2 * eye) * (1 - extension * 0.5)
        self.lathe([(0, y0), (r * 0.6, y0), (r, y0 + r * 0.6), (r, y1 - r * 0.5), (r * 0.72, y1),
                    (0, y1)], place, "metal", 6, angle=20.0)
        self.lathe([(r * 0.42, y1), (r * 0.7, y1), (r * 0.7, y1 + r * 0.3), (r * 0.42, y1 + r * 0.3)],
                   place, "metal", 6, closed=True, angle=20.0)
        self.cyl(place @ V((0, y1, 0)), place @ V((0, length - eye * 0.7, 0)), r * 0.38, "chrome", 12)
        u = (place.to_3x3() @ X).normalized()
        for y in (0.0, length):
            c = place @ V((0, y, 0))
            self.cyl(c - u * eye * 0.5, c + u * eye * 0.5, eye * 0.75, "dark", 6, bevel=eye * 0.08,
                     segments=1)
        return place

    def cap(self, centre, axis, r):
        """A pin retainer on a joint's outer face: a faceted plate holding the pin, and the
        pin's end."""
        c, ax = vec(centre), vec(axis).normalized()
        self.cyl(c, c + ax * 0.03, r, "metal", 8, bevel=0.006, segments=1)
        self.cyl(c + ax * 0.03, c + ax * 0.06, r * 0.45, "dark", 6, bevel=0.004, segments=1)

    def cable(self, points, r):
        """An armoured cable (power and hydraulic lines) through `points`: a flexible sleeve
        with ferrules, where a line has to cross a joint."""
        pts = [vec(p) for p in points]
        for p, q in zip(pts, pts[1:]):
            d = q - p
            self.cyl(p, q, r * 0.7, "rubber", 8)
            n = max(1, int(d.length / (r * 4)))
            for k in range(n + 1):
                c = p + d * (k / n)
                self.cyl(c - d.normalized() * r * 0.4, c + d.normalized() * r * 0.4, r, "dark", 8,
                         bevel=r * 0.1, segments=1)

    def vent(self, target, centre, normal, up, width, height, fins=5, depth=0.12):
        """A heat vent cut into `target`: a recess with angled fins that shed heat outward
        and downward (so rain and debris don't fall in)."""
        c, n = vec(centre), vec(normal).normalized()
        u = vec(up)
        u = (u - n * u.dot(n)).normalized()
        a = u.cross(n)
        self.cut_box(target, (width, height, depth * 2), place=frame(c, u=a, v=u))
        for k in range(fins):
            y = -height / 2 + (k + 0.5) * height / fins
            tilt = (u + n * 0.8).normalized()
            self.box((width * 0.98, height / fins * 0.95, 0.02),
                     place=frame(c + u * y - n * depth * 0.5, u=a, v=tilt), mat="dark", bevel=0.003,
                     segments=1)

    def lamp(self, p, normal, r=0.06):
        """A marker lamp (shows the machine's extent to others): a housing and its lens."""
        p, n = vec(p), vec(normal).normalized()
        self.cyl(p, p + n * 0.04, r * 1.4, "dark", 8, bevel=0.005, segments=1)
        self.cyl(p + n * 0.04, p + n * 0.07, r, "light", 8)

    # Near-future military hardware --------------------------------------------------------

    def bolt(self, p, normal, size=0.04, mat="dark"):
        """A flush fastener: a countersunk head barely proud of the plate (add-on armour is
        fastened on: field-replaceable)."""
        p, n = vec(p), vec(normal).normalized()
        self.cyl(p - n * 0.01, p + n * 0.012, size * 0.7, mat, 8)
        self.cyl(p + n * 0.012, p + n * 0.016, size * 0.3, "metal", 6)

    def bolt_row(self, a, b, normal, count, size=0.04, mat="dark"):
        a, b = vec(a), vec(b)
        for k in range(count):
            self.bolt(a.lerp(b, (k + 0.5) / count), normal, size, mat)

    def corner_bolts(self, centre, normal, up, width, height, size=0.04, inset=0.12):
        """Flush fasteners at the four corners of an add-on module."""
        c, n = vec(centre), vec(normal).normalized()
        u = vec(up)
        u = (u - n * u.dot(n)).normalized()
        a = u.cross(n)
        for sx in (-1, 1):
            for sy in (-1, 1):
                self.bolt(c + a * sx * (width / 2 - inset) + u * sy * (height / 2 - inset), n, size)

    def lift_eye(self, p, normal, along_=X, size=0.14):
        """A recessed lift point (a crane's hook or a tie-down takes it): a flush steel socket
        plate with a bar across its pocket."""
        p, n = vec(p), vec(normal).normalized()
        a = vec(along_)
        a = (a - n * a.dot(n)).normalized()
        self.box((size * 2.0, size * 1.4, 0.03), place=frame(p + n * 0.01, u=a, w=n), mat="metal", bevel=0.006,
                 segments=1)
        self.box((size * 1.3, size * 0.75, 0.03), place=frame(p + n * 0.027, u=a, w=n), mat="dark", bevel=0.004,
                 segments=1)
        w = n.cross(a).normalized()
        self.cyl(p + n * 0.035 - a * size * 0.6, p + n * 0.035 + a * size * 0.6, size * 0.12, "metal", 8)
        del w

    def boot(self, a, b, r, folds=6):
        """A rubber dust boot (bellows) over an actuator's rod: keeps grit off the seal."""
        a, b = vec(a), vec(b)
        length = (b - a).length
        prof = [(0, 0)]
        for k in range(folds * 2 + 1):
            prof.append((r if k % 2 else r * 0.78, length * k / (folds * 2)))
        prof.append((0, length))
        self.lathe(prof, along(a, b), "rubber", 12, angle=60.0)

    def ram(self, a, b, r, extension=0.4, hint=None, booted=True):
        """A hydraulic actuator from `a` to `b`: the barrel with its end cap and the hose port,
        the rod under a rubber dust boot, eyes on their pins."""
        a, b = vec(a), vec(b)
        place = along(a, b, hint)
        length = (b - a).length
        eye = r * 1.1
        y0 = eye * 0.8
        y1 = y0 + (length - 2 * eye) * (1 - extension * 0.5)
        self.lathe([(0, y0), (r * 0.8, y0), (r, y0 + r * 0.3), (r, y1 - r * 0.2), (r * 0.85, y1), (0, y1)], place,
                   "dark", 12, angle=40.0)
        rod_end = place @ V((0, length - eye * 0.7, 0))
        self.cyl(place @ V((0, y1, 0)), rod_end, r * 0.38, "chrome", 12)
        if booted:
            self.boot(place @ V((0, y1, 0)), place @ V((0, y1 + (length - eye - y1) * 0.85, 0)), r * 0.7)
        u = (place.to_3x3() @ X).normalized()
        w = (place.to_3x3() @ Z).normalized()
        port = place @ V((0, y0 + r * 0.8, 0))
        self.cyl(port + w * r * 0.9, port + w * r * 1.35, r * 0.25, "metal", 6)
        for y in (0.0, length):
            c = place @ V((0, y, 0))
            self.cyl(c - u * eye * 0.5, c + u * eye * 0.5, eye * 0.75, "dark", 12, bevel=eye * 0.08, segments=1)
            self.cyl(c - u * eye * 0.62, c + u * eye * 0.62, eye * 0.3, "metal", 8)
        return place

    def rail(self, a, b, normal, standoff=0.07, r=0.025):
        """A low-profile handhold (for the crew): a bar close to the surface on two short feet."""
        a, b, n = vec(a), vec(b), vec(normal).normalized()
        self.cyl(a + n * standoff, b + n * standoff, r, "metal", 8)
        for p in (a, b):
            self.cyl(p, p + n * (standoff + r), r * 1.1, "dark", 8)

    def rungs(self, top, bottom, normal, across, count, width=0.4):
        """Fold-flat steps up a surface (for the crew climbing to the hatch): each a short
        plate on a hinge, folded up flush."""
        top, bottom, n, a = vec(top), vec(bottom), vec(normal).normalized(), vec(across).normalized()
        u = a.cross(n).normalized()
        for k in range(count):
            p = top.lerp(bottom, k / max(1, count - 1))
            self.box((width, 0.14, 0.035), place=frame(p + n * 0.02, u=a, v=u), mat="dark", bevel=0.008, segments=1)
            self.cyl(p - a * width * 0.5 - u * 0.07 + n * 0.02, p + a * width * 0.5 - u * 0.07 + n * 0.02, 0.018,
                     "metal", 8)

    def periscope(self, c, normal, up, width=0.32):
        """An electro-optical / infrared camera module: a faceted housing, its lens and the IR
        illuminator beside it."""
        c, n = vec(c), vec(normal).normalized()
        u = vec(up)
        u = (u - n * u.dot(n)).normalized()
        a = u.cross(n)
        h, d = width * 0.6, width * 0.5
        self.hull([c + a * sx * width / 2 + u * y + n * z for sx in (-1, 1)
                   for y, z in ((0, 0), (h, 0), (h * 0.85, d), (h * 0.15, d))], "armor2", bevel=0.008)
        lens = c + u * h * 0.5 + n * d - a * width * 0.12
        self.cyl(lens - n * 0.01, lens + n * 0.02, width * 0.18, "glass", 16)
        ir = c + u * h * 0.5 + n * d + a * width * 0.25
        self.cyl(ir - n * 0.01, ir + n * 0.012, width * 0.07, "light", 8)

    def grille(self, target, centre, normal, up, width, height, slats=6, depth=0.14):
        """A cooling vent set flush in a panel: angled louvres in a recess (they shed rain and
        hide the hot interior from view)."""
        c, n = vec(centre), vec(normal).normalized()
        u = vec(up)
        u = (u - n * u.dot(n)).normalized()
        a = u.cross(n)
        self.cut_box(target, (width, height, depth * 2), place=frame(c, u=a, v=u))
        for k in range(slats):
            y = -height / 2 + (k + 0.5) * height / slats
            self.box((width * 0.98, height / slats * 0.92, 0.025),
                     place=frame(c + u * y - n * depth * 0.45, u=a, v=(u + n * 1.1).normalized()), mat="dark",
                     bevel=0.004, segments=1)

    def antenna(self, base, normal, length=1.2):
        """A short whip antenna (line-of-sight radio) on its base."""
        b, n = vec(base), vec(normal).normalized()
        self.cyl(b, b + n * 0.06, 0.06, "dark", 8, bevel=0.01, segments=1)
        self.cyl(b + n * 0.06, b + n * length, 0.012, "dark", 6)

    def blade_antenna(self, base, normal, along_, height=0.35, length=0.5):
        """A blade antenna (datalink, navigation): a swept fin, low and robust."""
        b, n = vec(base), vec(normal).normalized()
        a = vec(along_)
        a = (a - n * a.dot(n)).normalized()
        w = n.cross(a).normalized()
        self.hull([b + a * x + w * s * 0.02 for x in (-length / 2, length / 2) for s in (-1, 1)]
                  + [b + a * (length * 0.15) + n * height + w * s * 0.012 for s in (-1, 1)], "dark", bevel=0.004)

    def latch(self, p, normal, up, size=0.1):
        """A flush panel latch: a recessed lever."""
        p, n = vec(p), vec(normal).normalized()
        u = vec(up)
        u = (u - n * u.dot(n)).normalized()
        a = u.cross(n)
        self.box((size, size * 0.5, 0.02), place=frame(p + n * 0.005, u=a, v=u), mat="dark", bevel=0.004,
                 segments=1)
        self.box((size * 0.7, size * 0.18, 0.012), place=frame(p + n * 0.016, u=a, v=u), mat="metal",
                 bevel=0.003, segments=1)

    def clamp(self, p, direction, normal, r):
        """A hose clamp holding a line to the structure."""
        p, d, n = vec(p), vec(direction).normalized(), vec(normal).normalized()
        self.box((r * 3.0, r * 0.8, r * 2.6), place=frame(p - n * r * 0.4, u=n.cross(d), v=d), mat="metal",
                 bevel=r * 0.1, segments=1)

    def stack(self, base, normal, r=0.14, height=0.7):
        """A low-profile exhaust diffuser: a flat outlet with louvres turning the exhaust down
        and back (cooled and spread, a smaller heat signature)."""
        b, n = vec(base), vec(normal).normalized()
        a = X if abs(n.x) < 0.9 else Z
        u = n.cross(a).normalized()
        a = u.cross(n)
        self.hull([b + a * sx * r * 2.2 + u * sy * r * 1.2 + n * z for sx in (-1, 1) for sy in (-1, 1)
                   for z in (0, r * 0.8)], "dark", bevel=0.01)
        for k in range(4):
            y = (-0.75 + k * 0.5) * r * 1.2
            self.box((r * 4.0, r * 0.4, 0.02), place=frame(b + u * y + n * r * 0.82, u=a, v=(u - n * 0.8).normalized()),
                     mat="metal", bevel=0.003, segments=1)

    def hazard(self, a, b, normal, width=0.18, stripe=0.22, thick=0.02):
        """A hazard band from `a` to `b` on a face (`normal` out of it): a yellow strip with
        diagonal black stripes over it, marking a pinch point or a moving edge."""
        a, b, n = vec(a), vec(b), vec(normal).normalized()
        d = b - a
        length = d.length
        u = d.normalized()
        w = n.cross(u).normalized()
        c = a + u * (length / 2)
        self.hull([c + u * x + w * y + n * z for x in (-length / 2, length / 2) for y in (-width / 2, width / 2)
                   for z in (0.0, thick)], "trim", bevel=0.0)
        count = max(1, int(length / stripe))
        step = length / count
        for k in range(count):
            x0 = -length / 2 + k * step + step * 0.1
            pts = [(x0, -width / 2), (x0 + step * 0.5, -width / 2), (x0 + step * 0.5 + width * 0.6, width / 2),
                   (x0 + width * 0.6, width / 2)]
            pts = [(max(-length / 2, min(length / 2, x)), y) for x, y in pts]
            if pts[1][0] - pts[0][0] < 1e-3 and pts[2][0] - pts[3][0] < 1e-3:
                continue
            self.hull([c + u * x + w * y + n * z for x, y in pts for z in (thick * 0.5, thick * 1.4)], "dark",
                      bevel=0.0)

    def servo(self, a, b, r, mat="dark", bolts=10):
        """A heavy servo drum from `a` to `b`: a housing with a steel flange and a bolt circle on
        its outer face."""
        a, b = vec(a), vec(b)
        place = self.drum(a, b, r, mat, 16, hint=X if abs((b - a).normalized().x) < 0.9 else Y)
        d = (b - a).normalized()
        self.cyl(b - d * 0.02, b + d * 0.05, r * 0.82, "metal", 16, bevel=0.01, segments=1)
        u = d.orthogonal().normalized()
        w = d.cross(u)
        for k in range(bolts):
            t = 2 * math.pi * k / bolts
            p = b + d * 0.05 + (u * math.cos(t) + w * math.sin(t)) * r * 0.66
            self.cyl(p, p + d * 0.04, r * 0.07, "dark", 6, bevel=0.005, segments=1)
        self.cyl(b + d * 0.05, b + d * 0.1, r * 0.3, "dark", 12, bevel=0.01, segments=1)
        return place

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

    def hole(self, target, a, b, r, sides=24):
        return self.cut(target, self.cyl(a, b, r, sides=sides, cutter=True))

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


def write_glb(objects, path, colours, scale=1.0):
    """Writes the objects, scaled by `scale`, as one glTF mesh: a primitive per material with
    positions, normals, the baked occlusion as COLOR_0 and, as TEXCOORD_0, the surface
    position in real metres along each piece (projected along the piece's own axis nearest
    its face's normal, offset per piece), which the hull detail shader draws plate seams in."""
    prims = {}
    for o in objects:
        me = o.data
        me.calc_loop_triangles()
        nm = o.matrix_world.to_3x3().inverted().transposed()
        m = Matrix.Scale(scale, 4) @ o.matrix_world
        ao = me.color_attributes.get("ao")
        normals = smooth_normals(me, min(35.0, o.get("angle", 35.0)))
        name = me.materials[0].name if me.materials else "armor"
        verts, nrms, cols, uvs, idx, seen = prims.setdefault(name, ([], [], [], [], [], {}))
        seed = sum(ord(c) * (i + 1) for i, c in enumerate(o.name)) % 997
        offset = Vector(((seed * 0.731) % 7.0, (seed * 0.377) % 5.0))
        # One projection for the faces turned the piece's main way (a curved plate keeps one
        # continuous grid), the face's own nearest axis for the rest (its sides).
        main = Vector((0.0, 0.0, 0.0))
        for poly in me.polygons:
            main += poly.normal * poly.area
        area = sum(poly.area for poly in me.polygons) or 1.0
        planar = main.length > 0.5 * area
        if planar:
            main.normalize()
            pu = (Vector((0, 1, 0)).cross(main) if abs(main.y) < 0.9 else Vector((1, 0, 0)).cross(main)).normalized()
            pv = main.cross(pu)
        for tri in me.loop_triangles:
            fn = me.polygons[tri.polygon_index].normal
            along_main = planar and fn.dot(main) > 0.5
            ax = max(range(3), key=lambda i: abs(fn[i]))
            for loop in tri.loops:
                co = me.vertices[me.loops[loop].vertex_index].co
                p = m @ co
                n = (nm @ normals[loop]).normalized()
                a = ao.data[loop].color[0] if ao else 1.0
                a = max(0.3, min(1.0, a))
                q = co * scale
                if along_main:
                    uv = (q.dot(pu), q.dot(pv))
                else:
                    uv = (q.z, q.y) if ax == 0 else ((q.x, q.z) if ax == 1 else (q.x, q.y))
                uv = (uv[0] + offset.x, uv[1] + offset.y)
                key = (round(p.x, 5), round(p.y, 5), round(p.z, 5), round(n.x, 3), round(n.y, 3),
                       round(n.z, 3), round(a, 2), round(uv[0], 3), round(uv[1], 3))
                i = seen.get(key)
                if i is None:
                    i = seen[key] = len(verts)
                    verts.append((p.x, p.y, p.z))
                    nrms.append((n.x, n.y, n.z))
                    cols.append((a, a, a, 1.0))
                    uvs.append(uv)
                idx.append(i)
    open(path, "wb").write(glb(prims, colours))
    return sum(len(v[4]) // 3 for v in prims.values())


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
        verts, nrms, cols, uvs, idx, _ = prims[name]
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
        accessors.append({"bufferView": view(struct.pack(f"<{len(uvs) * 2}f", *[c for v in uvs for c in v]), 34962),
                          "componentType": 5126, "count": len(uvs), "type": "VEC2"})
        accessors.append({"bufferView": view(struct.pack(f"<{len(idx)}I", *idx), 34963),
                          "componentType": 5125, "count": len(idx), "type": "SCALAR"})
        spec = colours.get(name, {"color": [0.6, 0.6, 0.6], "metallic": 0.5, "roughness": 0.5})
        mat = {"name": name, "pbrMetallicRoughness": {
            "baseColorFactor": [*(srgb_to_linear(c) for c in spec["color"]), 1.0],
            "metallicFactor": spec["metallic"], "roughnessFactor": spec["roughness"]}}
        if "emissive" in spec:
            mat["emissiveFactor"] = [srgb_to_linear(c) for c in spec["emissive"]]
        materials.append(mat)
        primitives.append({"attributes": {"POSITION": acc, "NORMAL": acc + 1, "COLOR_0": acc + 2,
                                          "TEXCOORD_0": acc + 3},
                           "indices": acc + 4, "material": len(materials) - 1})
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
