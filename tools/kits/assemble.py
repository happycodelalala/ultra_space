"""Assembles building kits (content/data/building_kits/*.json) from third-party pieces
(assets/third_party/<source>/) into one model per building: assets/export/buildings/<kit>/<id>.glb,
plus kit.json with each building's footprint and height (metres, measured from the model) for
the layouts. Run through tools/build-kits.sh:

    blender -b --factory-startup --python tools/kits/assemble.py -- [kit ids...] [--preview DIR]

Recipes: `house` (cells of `unit_m`: width across X, depth along Z, storeys, walls wood|stone|mixed,
roof high|low|flat, chimney, banners, lanterns, tower N storeys at the back), `temple` (a
colonnade of pillars round width × depth cells, `height` units tall, a tiled roof, and a walled
inner room when there is space), and/or `pieces` (a piece at a position in kit units with a turn
about the vertical, degrees; optionally from another `source` folder and at a `scale`, for packs
of other units). Fronts (doors) face +Z.
Coordinates here are the game's (glTF: Y up); Blender's are converted on the way in and out.
"""

import json
import math
import sys
from pathlib import Path

import bpy
from mathutils import Matrix, Vector

ROOT = Path(__file__).resolve().parents[2]
KITS = ROOT / "content/data/building_kits"
THIRD = ROOT / "assets/third_party"
OUT = ROOT / "assets/export/buildings"


def clear():
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o)
    for m in list(bpy.data.meshes):
        bpy.data.meshes.remove(m)


_piece_cache = {}


def place(source, name, x, y, z, rot_deg, scale=1.0):
    """Imports piece `name` and places it at game coordinates (x, y up, z) turned `rot_deg`
    about the vertical (counter-clockwise seen from above), scaled by `scale`."""
    folder = THIRD / source
    path = folder / f"{name}.glb"
    if not path.exists():
        path = folder / f"{name}.gltf"
    if not path.exists():
        raise SystemExit(f"assemble: no piece {name} in {folder}")
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(path))
    new = [o for o in bpy.data.objects if o not in before]
    # Game (x, y, z) is Blender (x, -z, y); a turn about game +Y is a turn about Blender +Z.
    m = (Matrix.Translation((x, -z, y)) @ Matrix.Rotation(math.radians(rot_deg), 4, "Z")
         @ Matrix.Scale(scale, 4))
    for o in new:
        if o.parent is None:
            o.matrix_world = m @ o.matrix_world
    return new


def roof_over(source, w, d, top, roof):
    """A roof over w × d cells at height `top`: hipped (slopes on the long sides, corner pieces
    at the ends; a pyramid over one cell), high or low; flat; or none."""
    cx = lambda i: i - (w - 1) / 2
    cz = lambda j: j - (d - 1) / 2
    slope, corner = ("roof-high", "roof-high-corner") if roof == "high" else ("roof", "roof-corner")
    if roof == "none":
        pass
    elif roof == "flat":
        for i in range(w):
            for j in range(d):
                place(source, "roof-flat", cx(i), top, cz(j), 0)
    elif w == 1 and d == 1:
        place(source, "roof-high-point" if roof == "high" else "roof-point", 0, top, 0, 0)
    else:
        for i in range(w):
            for j in range(d):
                x, z = cx(i), cz(j)
                west = i == 0
                if 0 < j < d - 1:
                    place(source, slope, x, top, z, 0 if west else 180)
                else:
                    front = j == d - 1
                    rot = {(True, True): 0, (False, True): 90, (False, False): 180, (True, False): -90}[(west, front)]
                    place(source, corner, x, top, z, rot)


def house(source, h):
    """A house on a grid of cells: walls round the edge each storey (door and windows at the
    front, +Z), a hipped roof, and its extras."""
    w, d = h["width"], h["depth"]
    storeys = h.get("storeys", 1)
    walls = h.get("walls", "wood")
    roof = h.get("roof", "high")
    cx = lambda i: i - (w - 1) / 2
    cz = lambda j: j - (d - 1) / 2

    def wall(kind, storey):
        stone = walls == "stone" or (walls == "mixed" and storey == 0)
        base = "wall" if stone else "wall-wood"
        return {"plain": base, "door": f"{base}-door", "window": f"{base}-window-shutters"}[kind]

    for s in range(storeys):
        for i in range(w):
            for j in range(d):
                x, z = cx(i), cz(j)
                # Game +Z front: the wall piece stands on its cell's +X edge; turning it by
                # -90° puts it on the +Z edge.
                if i == w - 1:
                    place(source, wall("window" if (j + s) % 2 else "plain", s), x, s, z, 0)
                if i == 0:
                    place(source, wall("window" if (j + s) % 2 == 0 else "plain", s), x, s, z, 180)
                if j == d - 1:
                    front = "door" if s == 0 and i == w // 2 else "window"
                    place(source, wall(front, s), x, s, z, -90)
                if j == 0:
                    place(source, wall("window" if s else "plain", s), x, s, z, 90)
    top = storeys
    roof_over(source, w, d, top, roof)
    if h.get("chimney"):
        place(source, "chimney", cx(w - 1), top, cz(max(0, d // 2 - 1)), 0)
    if h.get("banners"):
        for j in (0, d - 1):
            place(source, "banner-red", cx(w - 1) + 0.05, 1, cz(j), 0)
    if h.get("lanterns"):
        place(source, "lantern", cx(w // 2) + 0.4, 0, cz(d - 1) + 0.6, 0)
    tower = h.get("tower")
    if tower:
        # A 1×1 tower behind the back wall (at -Z), with a spire.
        tz = cz(0) - 1
        for s in range(tower):
            for rot in (0, 90, 180, -90):
                place(source, "wall", 0, s, tz, rot)
        place(source, "roof-high-point", 0, tower, tz, 0)


def temple(source, t):
    """Pillars round the edge of width × depth cells (at every cell corner), `height` units
    tall, a flat roof over all of it, and a walled inner room one cell in when there is room."""
    w, d, height = t["width"], t["depth"], t.get("height", 2)
    for i in range(w + 1):
        for j in range(d + 1):
            if 0 < i < w and 0 < j < d:
                continue
            x, z = i - w / 2, j - d / 2
            for s in range(height):
                place(source, "pillar-stone", x, s, z, 0)
    roof_over(source, w, d, height, t.get("roof", "high"))
    if w >= 3 and d >= 3:
        house(source, {"width": w - 2, "depth": d - 2, "storeys": height, "walls": "stone",
                       "roof": "none"})


def build(kit, b):
    clear()
    r = b["recipe"]
    if "house" in r:
        house(kit["source"], r["house"])
    if "temple" in r:
        temple(kit["source"], r["temple"])
    for p in r.get("pieces", []):
        x, y, z = p["at"]
        place(p.get("source", kit["source"]), p["piece"], x, y, z, p.get("rot", 0), p.get("scale", 1.0))
    # Apply the kit's scale; free every mesh from its import hierarchy (keeping where it is),
    # then join them into one object.
    for o in bpy.data.objects:
        if o.parent is None:
            o.matrix_world = Matrix.Scale(kit["unit_m"], 4) @ o.matrix_world
    bpy.context.view_layer.update()
    meshes = [o for o in bpy.data.objects if o.type == "MESH"]
    for o in meshes:
        world = o.matrix_world.copy()
        o.parent = None
        o.matrix_world = world
    bpy.ops.object.select_all(action="DESELECT")
    for o in meshes:
        o.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    bpy.ops.object.join()
    obj = bpy.context.view_layer.objects.active
    obj.name = b["id"]
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    for o in list(bpy.data.objects):
        if o is not obj:
            bpy.data.objects.remove(o)
    lo = Vector([min(v.co[k] for v in obj.data.vertices) for k in range(3)])
    hi = Vector([max(v.co[k] for v in obj.data.vertices) for k in range(3)])
    # Back to game axes: X, Y up (Blender Z), Z (Blender -Y).
    return obj, {
        "id": b["id"],
        "uses": b["uses"],
        "model": f"{b['id']}.glb",
        "min_m": [round(lo.x, 3), round(lo.z, 3), round(-hi.y, 3)],
        "max_m": [round(hi.x, 3), round(hi.z, 3), round(-lo.y, 3)],
    }


def preview(obj, path):
    scene = bpy.context.scene
    size = max(obj.dimensions)
    centre = sum((obj.matrix_world @ Vector(c) for c in obj.bound_box), Vector()) / 8
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    scene.collection.objects.link(cam)
    cam.location = centre + Vector((0.9, -1.4, 0.9)) * size * 1.3
    cam.rotation_euler = (centre - cam.location).to_track_quat("-Z", "Y").to_euler()
    scene.camera = cam
    sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
    sun.data.energy = 4
    sun.rotation_euler = (0.8, 0.2, 0.6)
    scene.collection.objects.link(sun)
    scene.world = bpy.data.worlds.new("w")
    scene.world.use_nodes = True
    scene.world.node_tree.nodes["Background"].inputs[1].default_value = 0.8
    scene.render.engine = "BLENDER_EEVEE_NEXT"
    scene.render.resolution_x, scene.render.resolution_y = 400, 300
    scene.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)
    bpy.data.objects.remove(cam)
    bpy.data.objects.remove(sun)


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    previews = None
    if "--preview" in argv:
        i = argv.index("--preview")
        previews = Path(argv[i + 1])
        del argv[i:i + 2]
    for path in sorted(KITS.glob("*.json")):
        kit = json.loads(path.read_text())
        if argv and kit["id"] not in argv:
            continue
        out = OUT / kit["id"]
        out.mkdir(parents=True, exist_ok=True)
        meta = []
        for b in kit["buildings"]:
            obj, info = build(kit, b)
            bpy.ops.object.select_all(action="DESELECT")
            obj.select_set(True)
            bpy.ops.export_scene.gltf(filepath=str(out / info["model"]), export_format="GLB",
                                      use_selection=True, export_apply=True, export_yup=True)
            meta.append(info)
            print(f"build-kits: {kit['id']}.{b['id']}: {info['max_m'][0] - info['min_m'][0]:.1f} x "
                  f"{info['max_m'][2] - info['min_m'][2]:.1f} m, {info['max_m'][1]:.1f} m tall")
            if previews:
                previews.mkdir(parents=True, exist_ok=True)
                preview(obj, previews / f"{kit['id']}.{b['id']}.png")
        (out / "kit.json").write_text(json.dumps({
            "id": kit["id"],
            "source": kit["source"],
            "licence": f"assets/third_party/{kit['source']}/License.txt",
            "buildings": meta,
        }, indent=2) + "\n")


main()
