"""The block-world style scene: a timber house in its yard with a garden, a woodshed, a well and a
wagon, a path out to a landed ship and a dug pit, woods around, villagers and chickens; rendered
in one light and view.

    blender -b --factory-startup --python scene.py -- <day|golden|rain|fog|night> <wide|house|people|close|ship> <out.png>

Everything is placed through the Plot (world.py): a structure claims its footprint on a levelled
pad, a tree its trunk and crown, a bush or rock its ground, so nothing overlaps; models stand on
the ground grid in quarter turns (only the ship, on its own grid, and the people turn freely)."""
import math
import os
import random
import sys

import bpy
from mathutils import Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import core       # noqa: E402
import world      # noqa: E402
import models     # noqa: E402
import people     # noqa: E402
import ship       # noqa: E402

variant, view, out = sys.argv[sys.argv.index("--") + 1:][:3]
B5 = bpy.app.version >= (5, 0, 0)
random.seed(int(os.environ.get("SEED", "7")))
for o in list(bpy.data.objects):
    bpy.data.objects.remove(o)
sc = bpy.context.scene
coll = sc.collection
core.setup_materials()
EXPORT_DIR = os.environ.get("BLOCKS_EXPORT")
if EXPORT_DIR:
    core.EXPORT = {}
if variant == "night":
    core.PALETTE["glass"] = (255, 170, 80); core.PALETTE["glasshi"] = (255, 214, 150)
    models.NIGHT_GLASS = True

# ---- layout: pads, paths, the pit ---------------------------------------------------------------------
T = world.Terrain(seed=0.0)
T.pad_rect(-5.75, -7.75, 8.0, 4.75, 2.0)        # the yard: house, garden, fences
T.pad_rect(-10.0, -7.5, -6.0, -3.5, 1.5)         # woodshed
T.pad_circle(-7.0, 1.5, 1.3)                      # well
T.pad_circle(-11.0, -12.5, 4.0, 2.0)              # the ship's landing ground
T.add_path([(1.0, -4.1), (0.9, -7.5), (0.7, -11.0), (0.6, -16.0), (0.3, -22.0)], 0.6)
T.add_path([(0.9, -7.6), (3.0, -8.0), (5.1, -8.3)], 0.5)
T.add_path([(0.7, -11.0), (-4.5, -10.0), (-9.0, -9.2), (-12.2, -9.4)], 0.5)
T.add_pit(7.5, -8.5, 2.2, (4.5, 5.0, -1.4))
T.build()
P = world.Plot(T)
Z = T.height

CAMS = {"wide": (Vector((17, -26, 9)), Vector((0, -2, 2.5)), 28), "house": (Vector((3.5, -19.5, 3.0)), Vector((0.5, 0, 3.8)), 32),
        "close": (Vector((-1.5, -14.5, 1.7)), Vector((4.0, -4.0, 1.8)), 24), "people": (Vector((0.0, -23.0, 1.4)), Vector((0.0, -15.5, 0.8)), 50),
        "ship": (Vector((-3.4, -12.6, 3.0)), Vector((-11.0, -12.0, 1.5)), 35)}


def out_of_frames(x, y, frac):
    """False if (x, y) is inside a camera's view nearer than `frac` of the way to what it looks at
    (tall things there would fill the frame)."""
    p = Vector((x, y))
    for name, (c, target, lens) in CAMS.items():
        fwd = (target.xy - c.xy); dist = fwd.length; fwd.normalize()
        d = p - c.xy; along = d.dot(fwd)
        half = math.atan(18.0 / lens) + math.radians(6)
        if 0 < along < frac * dist and math.acos(max(-1.0, min(1.0, along / max(d.length, 1e-6)))) < half:
            return False
    return True


def clear_of_views(x, y, margin):
    p = Vector((x, y))
    for name in ("house", "close", "wide", "people", "ship"):
        c, target, _ = CAMS[name]
        a = c.xy; ab = target.xy - a; t = max(0.0, min(1.0, (p - a).dot(ab) / ab.length_squared))
        if (p - (a + ab * t)).length < margin:
            return False
    return True


# ---- structures and props, each claiming its ground ----------------------------------------------
models.house(0.0, 0.0, Z(0, 0)); P.solid(-3.0, -4.3, 3.0, 4.0, "house"); P.solids[-1] = (-3.3, -4.4, 3.3, 4.4)
for bx in (4.625, 6.625):
    models.garden(bx, -0.25, Z(bx, -0.25), crop="cabbage" if bx < 5 else "carrot"); P.solid(bx - 0.7, -2.2, bx + 0.7, 1.7, "garden")
models.woodshed(-8.0, -5.5, Z(-8.0, -5.5)); P.solid(-10.0, -7.4, -6.0, -3.6, "shed")
models.stump_with_axe(-6.9, -7.9, Z(-6.9, -7.9)); P.claim(P.circle(-6.9, -7.9, 0.4), "stump")
models.well(-7.0, 1.5, Z(-7.0, 1.5)); P.solid(-8.0, 0.5, -6.0, 2.5, "well")
for (a, b, n) in [((-5.0, -6.8), (-1.75, -6.8), 4), ((3.0, -6.8), (5.25, -6.8), 3), ((-5.0, -6.8), (-5.0, -2.0), 5)]:
    models.fence(a, b, Z(*a), n); P.claim(P.rect(min(a[0], b[0]) - 0.15, min(a[1], b[1]) - 0.15, max(a[0], b[0]) + 0.15, max(a[1], b[1]) + 0.15), "fence")
for (bx, by) in [(-2.0, -4.25), (-2.8, -4.0)]:
    models.barrel(bx, by, Z(bx, by)); P.claim(P.circle(bx, by, 0.4), "barrel")
models.crates(-6.4, -2.9, Z(-6.4, -2.9)); P.claim(P.rect(-6.7, -3.2, -5.5, -2.6), "crates")
models.wagon(-3.9, -0.6, Z(-3.9, -0.6)); P.claim(P.rect(-4.8, -3.4, -3.0, 0.7), "wagon")
POSTS = [(-0.6, -8.5), (1.9, -18.5)]
for (lx, ly) in POSTS:
    models.lantern_post(lx, ly, Z(lx, ly)); P.claim(P.circle(lx, ly, 0.3), "post")
models.mined_pile(4.2, -10.9, Z(4.2, -10.9)); P.claim(P.rect(3.9, -11.5, 5.3, -10.3), "mined")
ship.ship(-11.0, -12.5, Z(-11.0, -12.5), 35); P.claim(P.circle(-11.0, -12.5, 3.6), "ship"); P.solids.append((-14.6, -16.2, -7.4, -8.8))

# ---- people and animals --------------------------------------------------------------------------
if view == "people":
    lineup = [("farmer", "green", "brown", "hair", "skin", (30, 10), 1.0, None), ("woman", "blue", "dark", "hair", "skin", (40, 30), 1.0, "basket"),
              ("guard", "steel2", "dark", "hair", "skin", (25, 25), 1.0, "sword"), ("bearded", "brown", "brown2", "blond", "skin2", (40, 10), 1.0, "sack"),
              ("plain", "red", "brown", "blond", "skin", (20, -10), 0.75, None)]
    for k, (kind, top, legs, hr, sk, ar, scl, tool) in enumerate(lineup):
        px = -2.4 + k * 1.2
        people.villager(f"{kind}_l", px, -15.5, Z(px, -15.5), 0, kind, top, legs, hr, sk, ar, scl, tool)
people.villager("farmer", 5.62, -0.6, Z(5.62, -0.6), 90, "farmer", "green", "brown", "hair", arms=(30, 45), tool="hoe")
people.villager("woman", 1.75, -4.6, Z(1.75, -4.6), -25, "woman", "blue", "dark", "hair", arms=(40, 30), tool="basket")
people.villager("guard", 0.8, -11.6, Z(0.8, -11.6), 10, "guard", "steel2", "dark", arms=(25, 25), tool="sword")
people.villager("carter", -3.9, -3.85, Z(-3.9, -3.85), -20, "bearded", "brown", "brown2", "blond", "skin2", arms=(40, 10), tool="sack")
people.villager("child", -1.2, -5.8, Z(-1.2, -5.8), 30, "plain", "red", "brown", "blond", arms=(20, -10), scale=0.75)
for (cx, cy, f) in [(-0.4, -6.2, 140), (-1.9, -6.35, 60), (-0.75, -5.15, 200)]:
    people.chicken(cx, cy, Z(cx, cy), f)
for (px, py, r) in [(5.62, -0.6, 0.3), (1.75, -4.6, 0.35), (0.8, -11.6, 0.4), (-3.9, -3.85, 0.4), (-1.2, -5.8, 0.3)]:
    P.claim(P.circle(px, py, r), "person")

# ---- nature: trees by trunk, crown and flat ground; then bushes, rocks, flowers, grass ------------
def try_place(n, rmin, rmax, attempt):
    k = 0
    for _ in range(n * 60):
        if k >= n:
            break
        a = random.uniform(0, math.tau); r = random.uniform(rmin, rmax)
        x = round(math.cos(a) * r / world.GB) * world.GB; y = round(math.sin(a) * r / world.GB) * world.GB
        if attempt(x, y):
            k += 1
    return k


def tree_at(kind, crown):
    def attempt(x, y):
        s = random.uniform(0.9, 1.15) if kind != "birch" else random.uniform(0.95, 1.2)
        cr = crown * s
        trunk = P.circle(x, y, 0.6)
        if not (P.free(trunk) and P.flat(trunk) and P.crown_ok(x, y, cr) and clear_of_views(x, y, cr * 0.9) and out_of_frames(x, y, 0.8)):
            return False
        getattr(models, kind)(x, y, Z(x, y), s, random.randrange(4))
        P.claim(P.circle(x, y, 0.85), kind); P.crowns.append((x, y, cr))
        return True
    return attempt


try_place(9, 8.5, 21.0, tree_at("oak", 1.8))
try_place(7, 11.0, 21.0, tree_at("pine", 1.9))
try_place(5, 8.0, 20.0, tree_at("birch", 1.1))


def bush_at(x, y):
    fp = P.circle(x, y, 0.75)
    if not (P.free(fp) and P.flat(fp) and clear_of_views(x, y, 1.2) and out_of_frames(x, y, 0.35)):
        return False
    models.bush(x, y, Z(x, y), random.random() < 0.7, random.randrange(4)); P.claim(fp, "bush")
    return True


def rock_at(x, y):
    s = random.uniform(0.7, 1.05); fp = P.circle(x, y, 1.15 * s)
    if not (P.free(fp) and clear_of_views(x, y, 1.6) and out_of_frames(x, y, 0.5)):
        return False
    models.rock(x, y, Z(x, y), s, random.randrange(4)); P.claim(fp, "rock")
    return True


try_place(16, 4.5, 20.5, bush_at)
try_place(6, 8.0, 20.5, rock_at)


def small_at(fn, r):
    def attempt(x, y):
        x += random.choice([0.0, 0.125]); y += random.choice([0.0, 0.125])
        fp = P.circle(x, y, r)
        if not (P.free(fp) and P.flat(fp)):
            return False
        fn(x, y, Z(x, y)); P.claim(fp, "small")
        return True
    return attempt


try_place(45, 3.5, 21.0, small_at(models.flowers, 0.2))
mush = 0
for (tx, ty, _) in P.crowns:
    for _ in range(4):
        a = random.uniform(0, math.tau); mx, my = tx + math.cos(a) * 1.3, ty + math.sin(a) * 1.3
        if mush < 12 and small_at(models.mushrooms, 0.15)(round(mx / 0.25) * 0.25, round(my / 0.25) * 0.25):
            mush += 1
            break
models.tuft_templates(coll)
tufts = 0
for _ in range(4000):
    if tufts >= 520:
        break
    x, y = random.uniform(-world.R + 1, world.R - 1), random.uniform(-world.R + 1, world.R - 1)
    if math.hypot(x, y) < 3.0:
        continue
    cell = [(math.floor(x / world.GB), math.floor(y / world.GB))]
    if P.free(cell):
        models.tuft(x, y, Z(x, y) - 0.02, coll); P.claim(cell, "tuft"); tufts += 1

# ---- light and weather --------------------------------------------------------------------------------
w = bpy.data.worlds.new("w"); sc.world = w; w.use_nodes = True
wn = w.node_tree.nodes; wl = w.node_tree.links; bg = wn["Background"]
sky = wn.new("ShaderNodeTexSky")
for t in ("NISHITA", "SINGLE_SCATTERING", "MULTIPLE_SCATTERING"):
    try:
        sky.sky_type = t
        break
    except TypeError:
        pass
sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN")); coll.objects.link(sun)


def set_sun(elev, azim, energy, colour, angle=1.5, use_sky=True):
    sun.rotation_euler = (math.radians(90 - elev), 0, math.radians(azim))
    sun.data.energy = energy; sun.data.color = colour; sun.data.angle = math.radians(angle)
    sky.sun_elevation = math.radians(max(elev, -5)); sky.sun_rotation = math.radians(azim + 180)
    if use_sky:
        wl.new(sky.outputs["Color"], bg.inputs["Color"])


def fog(density, colour):
    bpy.ops.mesh.primitive_cube_add(size=1, location=(0, 0, 10)); c = bpy.context.object; c.scale = (90, 90, 40)
    m = bpy.data.materials.new("fog"); m.use_nodes = True; n = m.node_tree.nodes; n.remove(n["Principled BSDF"])
    v = n.new("ShaderNodeVolumePrincipled"); v.inputs["Density"].default_value = density; v.inputs["Color"].default_value = (*colour, 1)
    m.node_tree.links.new(v.outputs[0], n["Material Output"].inputs["Volume"]); c.data.materials.append(m)


def lamp(loc, energy, colour=(1.0, 0.62, 0.28), r=0.15):
    l = bpy.data.objects.new("lamp", bpy.data.lights.new("lamp", "POINT")); coll.objects.link(l)
    l.location = loc; l.data.energy = energy; l.data.color = colour; l.data.shadow_soft_size = r


def lanterns(energy_door, energy_post):
    lamp((1.78, -3.9, Z(0, 0) + 1.6), energy_door)
    for (lx, ly) in POSTS:
        lamp((lx + 0.4, ly, Z(lx, ly) + 1.6), energy_post)


def rain(n=4000):
    import bmesh
    bm = bmesh.new()
    for _ in range(n):
        x, y, z = random.uniform(-20, 20), random.uniform(-24, 16), random.uniform(0, 14)
        v0 = bm.verts.new((x - 0.007, y, z)); v1 = bm.verts.new((x + 0.007, y, z))
        v2 = bm.verts.new((x + 0.1, y + 0.03, z - 0.8)); v3 = bm.verts.new((x + 0.086, y + 0.03, z - 0.8))
        bm.faces.new((v0, v1, v2, v3))
    me = bpy.data.meshes.new("rain"); bm.to_mesh(me); o = bpy.data.objects.new("rain", me); coll.objects.link(o)
    m = bpy.data.materials.new("rain"); m.use_nodes = True; b = m.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = (0.75, 0.8, 0.85, 1); b.inputs["Alpha"].default_value = 0.3
    m.surface_render_method = "BLENDED"; me.materials.append(m)


exposure = 0.0
if variant == "day":
    set_sun(42, 140, 4.0, (1.0, 0.96, 0.9)); bg.inputs["Strength"].default_value = 0.35
elif variant == "golden":
    set_sun(7, 210, 4.5, (1.0, 0.66, 0.36), 2.5); bg.inputs["Strength"].default_value = 0.3; fog(0.003, (1.0, 0.8, 0.6))
elif variant == "rain":
    set_sun(50, 140, 1.2, (0.9, 0.93, 1.0), 40, use_sky=False)
    bg.inputs["Color"].default_value = (0.42, 0.46, 0.52, 1); bg.inputs["Strength"].default_value = 0.9
    fog(0.012, (0.75, 0.78, 0.82)); rain(); core.set_wet(True)
    core.set_glow(3.0); lanterns(25, 30)
elif variant == "fog":
    set_sun(4, 120, 2.5, (1.0, 0.8, 0.6), 3); bg.inputs["Strength"].default_value = 0.25; fog(0.04, (0.95, 0.92, 0.88))
elif variant == "night":
    set_sun(30, 60, 0.35, (0.55, 0.65, 1.0), 2, use_sky=False)
    bg.inputs["Color"].default_value = (0.02, 0.03, 0.07, 1); bg.inputs["Strength"].default_value = 1.0
    core.set_glow(4.0); lanterns(60, 70)
    lamp((0, 0, Z(0, 0) + 1.4), 150); lamp((0, 0, Z(0, 0) + 4.0), 150)
    fog(0.004, (0.55, 0.6, 0.8)); exposure = 0.6
sc.view_settings.view_transform = "AgX"; sc.view_settings.look = "AgX - Medium High Contrast"
sc.view_settings.exposure = exposure + (float(os.environ.get("EXP5", "-1.3")) if B5 else 0.0)

cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam")); coll.objects.link(cam); sc.camera = cam
loc, look, lens = CAMS[view]
cam.location = loc; cam.data.lens = lens
cam.rotation_euler = (look - loc).to_track_quat("-Z", "Y").to_euler()
try:
    sc.render.engine = "BLENDER_EEVEE_NEXT"
except TypeError:
    sc.render.engine = "BLENDER_EEVEE"
for k, v in [("use_shadows", True), ("use_volumetric_shadows", True), ("taa_render_samples", int(os.environ.get("SAMPLES", "48"))),
             ("use_raytracing", True), ("volumetric_tile_size", "4")]:
    try:
        setattr(sc.eevee, k, v)
    except Exception:
        pass
sc.render.resolution_x, sc.render.resolution_y = 1600, 900
sc.render.filepath = out
if EXPORT_DIR:
    import json
    from pathlib import Path
    out_dir = Path(EXPORT_DIR); out_dir.mkdir(parents=True, exist_ok=True)
    for old in out_dir.glob("*.json"):
        old.unlink()
    (out_dir / "palette.json").write_text(json.dumps({k: list(v) for k, v in sorted(core.PALETTE.items())}, indent=0) + "\n")
    skip = ("ship", "ship_detail", "ground")
    used = set()
    placements = []
    for o in sc.objects:
        key = o.get("block_model")
        if not key or o.hide_render or o.name.split(".")[0] in skip:
            continue
        used.add(key)
        placements.append({"model": key, "pos": [round(o.location.x, 5), round(o.location.y, 5), round(o.location.z, 5)],
                           "rot": round(math.degrees(o.rotation_euler.z), 3), "scale": round(o.scale.x, 4)})
    placements.sort(key=lambda p: (p["model"], p["pos"]))
    for key in sorted(used):
        (out_dir / f"{key}.json").write_text(json.dumps(core.EXPORT[key], separators=(",", ":")) + "\n")
    n = int(world.R / world.GB)
    names = sorted(set(T.topcols.values()))
    ground = {"size": world.GB, "origin": [-n, -n], "dims": [2 * n, 2 * n],
              "tops": [T.top(i, j) for j in range(-n, n) for i in range(-n, n)],
              "palette": names, "colours": [names.index(T.topcols[i, j]) for j in range(-n, n) for i in range(-n, n)],
              "carved": sorted(T.carved)}
    lights = [{"kind": "lantern", "pos": [lx + 0.4, ly, round(Z(lx, ly) + 1.7, 4)]} for (lx, ly) in POSTS]
    lights.append({"kind": "lantern", "pos": [1.78, -3.98, round(Z(0, 0) + 1.6, 4)]})
    lights += [{"kind": "window", "pos": [0.0, 0.0, round(Z(0, 0) + h, 4)]} for h in (1.4, 4.0)]
    village = {"name": "demo_village", "frame": "x east, y north, z up; metres from the village centre",
               "ground": ground, "placements": placements, "lights": lights,
               "ship": {"pos": [-11.0, -12.5, Z(-11.0, -12.5)], "rot": 35.0},
               "start": {"pos": [-7.5, -9.3, Z(-7.5, -9.3)], "facing": 141.0}}
    (out_dir / "village.json").write_text(json.dumps(village, separators=(",", ":")) + "\n")
    print(f"EXPORTED {len(used)} models, {len(placements)} placements to {out_dir}")
elif os.environ.get("NORENDER"):
    print("PLACED", {k: sum(1 for o in sc.objects if o.name.startswith(k)) for k in ("oak", "pine", "birch", "bush", "rock", "flowers", "mushrooms", "tuft")})
else:
    bpy.ops.render.render(write_still=True)
