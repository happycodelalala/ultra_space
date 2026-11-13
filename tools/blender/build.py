"""Builds part models with Blender: run through tools/build-models.sh.

    blender -b --factory-startup --python tools/blender/build.py -- [names...] [--preview DIR]

Builds every model named by a part's `form.model` that a builder exists for (or just
`names`), writes `assets/export/models/<name>.glb`, and records a stamp of the inputs
(`assets/export/models/models.stamp`) that `tools/build-models.sh --check` compares. With
`--preview DIR`, also renders each built model to `DIR/<name>.png` (studio light, Cycles),
seen from the direction `--view x,y,z` (default front-left, above).
"""

import hashlib
import json
import math
import sys
import time
from pathlib import Path

import bpy
from mathutils import Matrix, Vector

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import mech  # noqa: E402
import parts  # noqa: E402

ROOT = HERE.parent.parent
PARTS = ROOT / "content" / "data" / "parts"
PALETTE = ROOT / "content" / "data" / "part_materials.json"
OUT = ROOT / "assets" / "export" / "models"
STAMP = OUT / "models.stamp"


def inputs_hash():
    """Hash of everything a model depends on: the scripts, the palette and the parts."""
    h = hashlib.sha256()
    files = sorted(HERE.glob("*.py")) + [PALETTE] + sorted(PARTS.glob("*.json"))
    for f in files:
        h.update(f.name.encode())
        h.update(f.read_bytes())
    return h.hexdigest()


def part_defs():
    out = {}
    for path in sorted(PARTS.glob("*.json")):
        for part in json.loads(path.read_text())["parts"]:
            model = (part.get("form") or {}).get("model")
            if model in parts.BUILDERS and model not in out:
                out[model] = part
    return out


def preview(objects, path, view=(-0.9, 0.55, -1.1)):
    """A studio render of the frozen model: three area lights, a grey world, 3/4 view."""
    scene = bpy.context.scene
    lo = Vector((min(v[i] for o in objects for v in o.bound_box) for i in range(3)))
    hi = Vector((max(v[i] for o in objects for v in o.bound_box) for i in range(3)))
    # bound_box is local; the frozen objects carry their placement.
    pts = [o.matrix_world @ Vector(c) for o in objects for c in o.bound_box]
    lo = Vector((min(p[i] for p in pts) for i in range(3)))
    hi = Vector((max(p[i] for p in pts) for i in range(3)))
    centre = (lo + hi) / 2
    size = (hi - lo).length
    for o in objects:
        me = o.data
        for m in me.materials:
            spec = palette.get(m.name, {})
            m.use_nodes = True
            bsdf = m.node_tree.nodes.get("Principled BSDF")
            col = [mech.srgb_to_linear(c) for c in spec.get("color", [0.6] * 3)]
            bsdf.inputs["Base Color"].default_value = (*col, 1)
            bsdf.inputs["Metallic"].default_value = spec.get("metallic", 0.5)
            bsdf.inputs["Roughness"].default_value = spec.get("roughness", 0.5)
            if "emissive" in spec:
                bsdf.inputs["Emission Color"].default_value = (*[mech.srgb_to_linear(c) for c in spec["emissive"]], 1)
                bsdf.inputs["Emission Strength"].default_value = spec.get("emissive_strength", 1)
    world = scene.world
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = (0.05, 0.055, 0.065, 1)
    world.node_tree.nodes["Background"].inputs[1].default_value = 1.0
    # Our Y is up: lights and camera in part axes.
    def add_light(name, offset, power, sz):
        light = bpy.data.lights.new(name, "AREA")
        light.energy = power * size * size
        light.size = sz * size
        ob = bpy.data.objects.new(name, light)
        ob.location = centre + offset * size
        look(ob, centre)
        scene.collection.objects.link(ob)

    def look(ob, target):
        # Our up is +Y: the camera's -Z toward the target, its Y as near +Y as it gets.
        d = (target - ob.location).normalized()
        z = -d
        x = Vector((0, 1, 0)).cross(z)
        x = x.normalized() if x.length > 1e-6 else Vector((1, 0, 0))
        y = z.cross(x)
        ob.rotation_euler = Matrix((x, y, z)).transposed().to_euler()

    add_light("key", Vector((-1.0, 1.4, -1.2)), 120, 0.8)
    add_light("fill", Vector((1.4, 0.4, -0.6)), 40, 1.2)
    add_light("rim", Vector((0.6, 1.2, 1.4)), 80, 0.6)
    cam = bpy.data.cameras.new("cam")
    cam.lens = 50
    co = bpy.data.objects.new("cam", cam)
    co.location = centre + Vector(view).normalized() * size * 1.7
    look(co, centre)
    scene.collection.objects.link(co)
    scene.camera = co
    scene.render.engine = "CYCLES"
    scene.cycles.samples = 64
    scene.cycles.use_denoising = True
    scene.render.resolution_x = 1000
    scene.render.resolution_y = 750
    scene.render.filepath = str(path)
    scene.view_settings.view_transform = "AgX"
    bpy.ops.render.render(write_still=True)


palette = mech.palette(PALETTE)


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    preview_dir = None
    if "--preview" in argv:
        i = argv.index("--preview")
        preview_dir = Path(argv[i + 1])
        del argv[i:i + 2]
    view = (-0.9, 0.55, -1.1)
    if "--view" in argv:
        i = argv.index("--view")
        view = tuple(float(c) for c in argv[i + 1].split(","))
        del argv[i:i + 2]
    defs = part_defs()
    names = argv or sorted(defs)
    OUT.mkdir(parents=True, exist_ok=True)
    total = 0
    for name in names:
        if name not in defs:
            print(f"build-models: no part uses model {name}")
            sys.exit(1)
        start = time.time()
        mech.clear()
        part = defs[name]
        model = mech.Model(name)
        parts.BUILDERS[name](model, parts.half_size(part), part)
        objects = mech.freeze(model)
        mech.bake_ao(objects)
        tris = mech.write_glb(objects, OUT / f"{name}.glb", palette)
        total += tris
        print(f"build-models: {name}: {tris} triangles, {time.time() - start:.1f} s")
        if preview_dir:
            preview_dir.mkdir(parents=True, exist_ok=True)
            preview(objects, preview_dir / f"{name}.png", view)
    if not argv:
        STAMP.write_text(inputs_hash() + "\n")
    print(f"build-models: {len(names)} models, {total} triangles")


main()
