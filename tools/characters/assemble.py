"""Assembles people (content/data/clothing_sets/*.json) from Quaternius packs (CC0) into one
glTF binary per character with its skeleton, meshes and the shared animations:
assets/export/characters/<set>/<id>.glb, plus set.json listing the characters and animations.
Run through tools/build-characters.sh:

    blender -b --factory-startup --python tools/characters/assemble.py -- [set ids...] [--preview DIR]

A character is a base body (Universal Base Characters), cut to its head and neck when it wears
an outfit (Modular Character Outfits - Fantasy), hair pieces, and an optional tint of the base
body's suit (for workwear). All packs share one 65-bone skeleton, so the outfit and hair are
bound to the base armature and the animations (Universal Animation Library) apply as they are.
Textures are reduced to 512 px (normal and roughness maps dropped): people are small on screen.

The packs are large, so they stay outside the repo: $ASSET_LIBRARY (default ~/asset-library,
see assets/third_party/README.md for where each comes from).
"""

import json
import math
import os
import sys
from pathlib import Path

import bpy
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[2]
SETS = ROOT / "content/data/clothing_sets"
OUT = ROOT / "assets/export/characters"
LIBRARY = Path(os.environ.get("ASSET_LIBRARY", Path.home() / "asset-library"))
BASE = LIBRARY / "q-universalbasecharacters/extracted/Universal Base Characters[Standard]"
BODIES = BASE / "Base Characters/Godot - UE"
HAIR = BASE / "Hairstyles/Rigged to Head Bone/glTF (Godot -Unreal)"
OUTFITS = (LIBRARY / "q-modularcharacteroutfitsfantasy/extracted/Modular Character Outfits - Fantasy[Standard]"
           / "Exports/glTF (Godot-Unreal)/Outfits")
ANIMATIONS = (LIBRARY / "quaternius-ual/extracted/Universal Animation Library[Standard]"
              / "Unreal-Godot/UAL1_Standard.glb")

# The shared animation set: our name -> the library's action.
ANIMATION_SET = {
    "idle": "Idle_Loop",
    "walk": "Walk_Loop",
    "run": "Jog_Fwd_Loop",
    "work": "Fixing_Kneeling",
    "use": "Interact",
    "carry": "PickUp_Table",
    "sit": "Sitting_Idle_Loop",
    "talk": "Idle_Talking_Loop",
}
TEXTURE_SIZE = 512


def clear():
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o)
    for coll in (bpy.data.meshes, bpy.data.armatures, bpy.data.actions, bpy.data.materials, bpy.data.images):
        for x in list(coll):
            coll.remove(x)


def import_gltf(path):
    if not path.exists():
        raise SystemExit(f"characters: missing {path} (set ASSET_LIBRARY)")
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(path))
    return [o for o in bpy.data.objects if o not in before]


def armature_of(objects):
    return next(o for o in objects if o.type == "ARMATURE")


def drop(objects):
    for o in objects:
        bpy.data.objects.remove(o)


def bind(meshes, armature):
    """Binds meshes from another import to `armature` (same skeleton)."""
    for m in meshes:
        world = m.matrix_world.copy()
        m.parent = armature
        m.matrix_world = world
        for mod in m.modifiers:
            if mod.type == "ARMATURE":
                mod.object = armature


def keep_head(mesh):
    """Deletes the body's vertices below the neck (those mostly weighted to other bones)."""
    groups = {g.index: g.name for g in mesh.vertex_groups}
    head = {"Head", "neck_01"}
    import bmesh
    bm = bmesh.new()
    bm.from_mesh(mesh.data)
    deform = bm.verts.layers.deform.active
    gone = []
    for v in bm.verts:
        w = v[deform] if deform else {}
        total = sum(w.values()) or 1.0
        on_head = sum(x for g, x in w.items() if groups.get(g) in head)
        if on_head / total < 0.5:
            gone.append(v)
    bmesh.ops.delete(bm, geom=gone, context="VERTS")
    bm.to_mesh(mesh.data)
    bm.free()


def solid(name, colour, roughness=0.75):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = next(n for n in mat.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    bsdf.inputs["Base Color"].default_value = (*colour, 1.0)
    bsdf.inputs["Roughness"].default_value = roughness
    return mat


def dress(mesh, suit):
    """Workwear on a base body: each face by the bones moving it gets the suit's colour, gloves
    on the hands, boots on the feet; the head and neck keep their skin."""
    groups = {g.index: g.name for g in mesh.vertex_groups}
    hands = ("hand_", "index_", "middle_", "ring_", "pinky_", "thumb_")
    feet = ("foot_", "ball_")
    slots = {}
    for key, colour in (("suit", suit["suit"]), ("gloves", suit["gloves"]), ("boots", suit["boots"])):
        mesh.data.materials.append(solid(f"{mesh.name}_{key}", colour))
        slots[key] = len(mesh.data.materials) - 1
    weights = {}
    for v in mesh.data.vertices:
        w = {}
        for g in v.groups:
            w[groups.get(g.group, "")] = g.weight
        weights[v.index] = w
    for poly in mesh.data.polygons:
        total = {}
        for vi in poly.vertices:
            for bone, w in weights[vi].items():
                total[bone] = total.get(bone, 0.0) + w
        if not total:
            continue
        bone = max(total, key=total.get)
        if bone in ("Head", "neck_01"):
            continue
        if bone.startswith(hands):
            poly.material_index = slots["gloves"]
        elif bone.startswith(feet):
            poly.material_index = slots["boots"]
        else:
            poly.material_index = slots["suit"]


def tint(images, colour):
    """Multiplies the suit's colour texture by `colour` (linear RGB)."""
    for img in images:
        px = list(img.pixels)
        for i in range(0, len(px), 4):
            px[i] *= colour[0]
            px[i + 1] *= colour[1]
            px[i + 2] *= colour[2]
        img.pixels[:] = px
        img.update()


def slim_materials(meshes):
    """Keeps only base colour textures, reduced to TEXTURE_SIZE."""
    seen = set()
    for m in meshes:
        for slot in m.material_slots:
            mat = slot.material
            if not mat or not mat.use_nodes or mat.name in seen:
                continue
            seen.add(mat.name)
            nodes, links = mat.node_tree.nodes, mat.node_tree.links
            bsdf = next((n for n in nodes if n.type == "BSDF_PRINCIPLED"), None)
            if not bsdf:
                continue
            for name in ("Normal", "Roughness", "Metallic"):
                for link in list(bsdf.inputs[name].links):
                    links.remove(link)
            bsdf.inputs["Roughness"].default_value = 0.8
            bsdf.inputs["Metallic"].default_value = 0.0
            for n in nodes:
                if n.type == "TEX_IMAGE" and n.image and max(n.image.size) > TEXTURE_SIZE:
                    n.image.scale(TEXTURE_SIZE, TEXTURE_SIZE)


def base_colour_images(mesh):
    out = []
    for slot in mesh.material_slots:
        mat = slot.material
        if not mat or not mat.use_nodes:
            continue
        bsdf = next((n for n in mat.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
        if bsdf:
            for link in bsdf.inputs["Base Color"].links:
                if link.from_node.type == "TEX_IMAGE" and link.from_node.image:
                    out.append(link.from_node.image)
    return out


def build(c):
    clear()
    sex = {"male": "Male", "female": "Female"}[c["body"]]
    base = import_gltf(BODIES / f"Superhero_{sex}_FullBody.gltf")
    arm = armature_of(base)
    spheres = [o for o in base if o.type == "MESH" and o.name.startswith("Icosphere")]
    base = [o for o in base if o not in spheres]
    drop(spheres)
    body = next(o for o in base if o.type == "MESH" and o.name.lower().startswith("superhero"))
    if c.get("outfit"):
        keep_head(body)
        outfit = import_gltf(OUTFITS / f"{c['outfit']}.gltf")
        parts = [o for o in outfit if o.type == "MESH" and not o.name.startswith("Icosphere")]
        bind(parts, arm)
        drop([o for o in outfit if o not in parts])
    if c.get("suit"):
        dress(body, c["suit"])
    for h in c.get("hair", []):
        hair = import_gltf(HAIR / f"{h}.gltf")
        parts = [o for o in hair if o.type == "MESH"]
        hair_arm = [o for o in hair if o.type == "ARMATURE"]
        if hair_arm:
            bind(parts, arm)
        else:
            # Rigged to the head bone: follow it.
            for p in parts:
                world = p.matrix_world.copy()
                p.parent = arm
                p.parent_type = "BONE"
                p.parent_bone = "Head"
                p.matrix_world = world
        drop([o for o in hair if o not in parts])
        if c.get("hair_colour") and not h.startswith("Eyebrows"):
            for p in parts:
                tint(base_colour_images(p), c["hair_colour"])
    meshes = [m for m in bpy.data.objects if m.type == "MESH"]
    slim_materials(meshes)

    # The animations: from the library, renamed, every other action dropped.
    for a in list(bpy.data.actions):
        bpy.data.actions.remove(a)
    lib = import_gltf(ANIMATIONS)
    drop(lib)
    names = {}
    for ours, theirs in ANIMATION_SET.items():
        action = next((a for a in bpy.data.actions
                       if a.name == theirs or a.name.startswith(f"{theirs}_Armature")), None)
        if action is None:
            raise SystemExit(f"characters: no animation {theirs}")
        names[action.name] = ours
    for a in list(bpy.data.actions):
        if a.name in names:
            a.name = names[a.name]
            a.use_fake_user = True
        else:
            bpy.data.actions.remove(a)
    arm.animation_data_create()
    arm.animation_data.action = bpy.data.actions["idle"]

    # Height (m) at rest, for the game.
    bpy.context.view_layer.update()
    top = max((m.matrix_world @ Vector(v)).z for m in meshes for v in m.bound_box)
    return arm, meshes, top


def export(arm, meshes, path):
    bpy.ops.object.select_all(action="DESELECT")
    arm.select_set(True)
    for m in meshes:
        m.select_set(True)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_animations=True,
        export_animation_mode="ACTIONS",
        export_force_sampling=True,
        export_image_format="AUTO",
        export_tangents=False,
    )


def preview(arm, meshes, path):
    scene = bpy.context.scene
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    scene.collection.objects.link(cam)
    cam.location = Vector((1.6, -3.2, 1.4))
    cam.rotation_euler = (Vector((0, 0, 0.9)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    scene.camera = cam
    sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
    sun.data.energy = 4
    sun.rotation_euler = (0.8, 0.2, 0.6)
    scene.collection.objects.link(sun)
    scene.world = bpy.data.worlds.new("w")
    scene.world.use_nodes = True
    scene.world.node_tree.nodes["Background"].inputs[1].default_value = 0.8
    scene.render.engine = "BLENDER_EEVEE_NEXT"
    scene.render.resolution_x, scene.render.resolution_y = 300, 400
    # Mid-walk.
    arm.animation_data.action = bpy.data.actions["walk"]
    scene.frame_set(10)
    scene.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)
    arm.animation_data.action = bpy.data.actions["idle"]
    bpy.data.objects.remove(cam)
    bpy.data.objects.remove(sun)


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    previews = None
    if "--preview" in argv:
        i = argv.index("--preview")
        previews = Path(argv[i + 1])
        del argv[i:i + 2]
    for path in sorted(SETS.glob("*.json")):
        cs = json.loads(path.read_text())
        if argv and cs["id"] not in argv:
            continue
        out = OUT / cs["id"]
        out.mkdir(parents=True, exist_ok=True)
        meta = []
        for c in cs["characters"]:
            arm, meshes, height = build(c)
            export(arm, meshes, out / f"{c['id']}.glb")
            if previews:
                previews.mkdir(parents=True, exist_ok=True)
                preview(arm, meshes, previews / f"{cs['id']}.{c['id']}.png")
            meta.append({"id": c["id"], "model": f"{c['id']}.glb", "roles": c["roles"],
                         "height_m": round(height, 3)})
            print(f"build-characters: {cs['id']}.{c['id']}: {height:.2f} m tall, "
                  f"{sum(len(m.data.vertices) for m in meshes)} vertices")
        (out / "set.json").write_text(json.dumps({
            "id": cs["id"],
            "licence": "assets/third_party/quaternius/License.txt",
            "animations": list(ANIMATION_SET),
            "characters": meta,
        }, indent=2) + "\n")


main()
