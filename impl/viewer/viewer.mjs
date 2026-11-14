// Design viewer: renders the game's designs and part models from the game's own files, with
// joints, poses, overlays, review shots and live reload. Saves only in the game's formats
// (design JSON, glTF binary with slot-named materials), so the game loads whatever it writes.
import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import { applyHullDetail } from "./hull_detail.mjs";
import { GLTFExporter } from "three/addons/exporters/GLTFExporter.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { EffectComposer } from "three/addons/postprocessing/EffectComposer.js";
import { RenderPass } from "three/addons/postprocessing/RenderPass.js";
import { GTAOPass } from "three/addons/postprocessing/GTAOPass.js";
import { OutputPass } from "three/addons/postprocessing/OutputPass.js";
import {
  applyPose,
  articulationOf,
  catalogOf,
  clampJoint,
  jointUnit,
  halfExtents,
  lowestPoint,
  shapeBoxes,
  massOf,
  partsOf,
  poseAngles,
  poseFromAngles,
  posesOf,
} from "./articulation.mjs";

const $ = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);
const DEG = Math.PI / 180;

// --- Materials: the game's, by name, from content/data/part_materials.json ---------------------

/// Placeholder materials (no vertex colours) and model materials (baked occlusion in the
/// vertex colours), filled from the palette by loadPalette().
const MATERIALS = {};
const MODEL_MATERIALS = {};
function srgb(r, g, b) {
  return new THREE.Color().setRGB(r, g, b, THREE.SRGBColorSpace);
}
async function loadPalette() {
  const { materials } = await getJson("/content/data/part_materials.json");
  for (const [name, m] of Object.entries(materials)) {
    for (const [into, vertexColors] of [[MATERIALS, false], [MODEL_MATERIALS, true]]) {
      into[name] = new THREE.MeshStandardMaterial({
        name,
        color: srgb(...m.color),
        metalness: m.metallic,
        roughness: m.roughness,
        vertexColors,
        emissive: m.emissive ? srgb(...m.emissive) : new THREE.Color(0, 0, 0),
        emissiveIntensity: m.emissive_strength ?? 1,
      });
    }
    // Models carry the surface position in metres (TEXCOORD_0): plate seams and wear.
    applyHullDetail(MODEL_MATERIALS[name], m.detail);
  }
}
/// The game's material for a model primitive's material name (part_models.rs).
function materialNamed(name, vertexColors = false) {
  const set = vertexColors ? MODEL_MATERIALS : MATERIALS;
  if (name === "joint") return set.dark;
  return set[name] ?? set.armor;
}
/// The game's placeholder material for a part kind (vehicle_visual.rs part_material).
function kindMaterial(kind) {
  if (kind === "cockpit") return MATERIALS.glass;
  if (["thruster", "reaction_wheel", "cruise_drive", "landing_leg", "wheel", "joint"].includes(kind)) {
    return MATERIALS.dark;
  }
  return MATERIALS.armor;
}

// --- Scene ---------------------------------------------------------------------------------

const view = $("view");
const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
renderer.outputColorSpace = THREE.SRGBColorSpace;
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.shadowMap.enabled = true;
renderer.shadowMap.type = THREE.PCFSoftShadowMap;
view.prepend(renderer.domElement);
const scene = new THREE.Scene();
scene.background = new THREE.Color(0x2a3440);
// Reflections and soft ambient light from a studio environment.
scene.environment = new THREE.PMREMGenerator(renderer).fromScene(new RoomEnvironment(), 0.04).texture;
scene.environmentIntensity = 0.6;
const camera = new THREE.PerspectiveCamera(40, 1, 0.1, 2000);
const controls = new OrbitControls(camera, renderer.domElement);
controls.enableDamping = true;
scene.add(new THREE.HemisphereLight(0xcfe3ff, 0x3a3226, 0.4));
const sun = new THREE.DirectionalLight(0xfff2dd, 2.4);
sun.position.set(-30, 50, -20);
sun.castShadow = true;
sun.shadow.mapSize.set(4096, 4096);
sun.shadow.bias = -0.0004;
sun.shadow.normalBias = 0.03;
scene.add(sun, sun.target);
// Screen-space ambient occlusion over the baked occlusion, then tone mapping.
const composer = new EffectComposer(renderer);
composer.addPass(new RenderPass(scene, camera));
const gtao = new GTAOPass(scene, camera, 1, 1);
gtao.updateGtaoMaterial({ radius: 0.6, distanceExponent: 1.5, thickness: 1.0, scale: 1.0, samples: 16 });
gtao.blendIntensity = 0.85;
composer.addPass(gtao);
composer.addPass(new OutputPass());
const fill = new THREE.DirectionalLight(0x9fb8d8, 0.6);
fill.position.set(25, 10, 30);
scene.add(fill);

const ground = new THREE.Group();
scene.add(ground);
const gridMajor = new THREE.GridHelper(200, 20, 0x6b7a88, 0x4a5662);
const gridMinor = new THREE.GridHelper(200, 200, 0x3b4550, 0x3b4550);
gridMinor.material.transparent = true;
gridMinor.material.opacity = 0.35;
ground.add(gridMinor, gridMajor);

const root = new THREE.Group(); // the vehicle (vehicle frame = world here)
scene.add(root);
const overlays = new THREE.Group();
scene.add(overlays);
const comMarker = new THREE.Mesh(
  new THREE.SphereGeometry(0.35, 16, 12),
  new THREE.MeshBasicMaterial({ color: 0xff4fd8, depthTest: false }),
);
comMarker.renderOrder = 10;
scene.add(comMarker);

function resize() {
  const w = view.clientWidth;
  const h = view.clientHeight;
  renderer.setSize(w, h);
  composer.setSize(w, h);
  camera.aspect = w / h;
  camera.updateProjectionMatrix();
}
new ResizeObserver(resize).observe(view);

// --- Data ----------------------------------------------------------------------------------

const state = {
  list: null,
  catalog: new Map(),
  designId: null,
  design: null, // the design file as parsed JSON
  parts: [],
  articulation: null,
  angles: [],
  mode: params.get("part") ? "part" : "design",
  partId: params.get("part"),
  models: new Map(), // model name -> Promise<THREE.Group | null>
  groupNodes: [],
  size: 10,
  version: null,
};

async function getJson(url) {
  const r = await fetch(url, { cache: "no-store" });
  if (!r.ok) throw new Error(`${url}: ${r.status}`);
  return r.json();
}

async function loadCatalog() {
  state.list = await getJson("/api/list");
  await loadPalette();
  const files = await Promise.all(
    state.list.partFiles.map((f) => getJson(`/content/data/parts/${f}`)),
  );
  state.catalog = catalogOf(files);
}

const loader = new GLTFLoader();
/// A part model by name (the game's assets/export/models/<name>.glb), materials replaced by
/// the game's, or null without one.
function model(name) {
  if (!state.models.has(name)) {
    const available = state.list.models.includes(name);
    state.models.set(
      name,
      !available
        ? Promise.resolve(null)
        : loader.loadAsync(`/assets/export/models/${name}.glb?v=${state.version}`).then((gltf) => {
            gltf.scene.traverse((o) => {
              if (o.isMesh) {
                o.material = materialNamed(o.material?.name, Boolean(o.geometry.attributes.color));
                o.castShadow = true;
                o.receiveShadow = true;
              }
            });
            return gltf.scene;
          }),
    );
  }
  return state.models.get(name);
}

/// The placeholder for a part without a model (vehicle_visual.rs shape_mesh).
function placeholder(def) {
  const s = def.shape;
  if (s.type === "shell") {
    // A procedural shell without a model: its tiles.
    const group = new THREE.Group();
    for (const t of shapeBoxes(s)) {
      const tile = new THREE.Mesh(new THREE.BoxGeometry(...t.half.map((h) => 2 * h)), kindMaterial(def.kind));
      tile.position.set(...t.centre);
      tile.quaternion.set(...t.rotation);
      group.add(tile);
    }
    return group;
  }
  let geometry;
  if (s.type === "box") geometry = new THREE.BoxGeometry(...s.size_m);
  else if (s.type === "cylinder") geometry = new THREE.CylinderGeometry(s.radius_m, s.radius_m, s.length_m, 24);
  else geometry = new THREE.IcosahedronGeometry(s.radius_m, 2);
  const mesh = new THREE.Mesh(geometry, kindMaterial(def.kind));
  if (def.wheel) mesh.rotation.z = Math.PI / 2;
  return mesh;
}

async function partObject(def) {
  const name = def.form?.model;
  const m = name ? await model(name) : null;
  const object = m ? m.clone(true) : placeholder(def);
  object.traverse((o) => {
    if (o.isMesh) {
      o.castShadow = true;
      o.receiveShadow = true;
    }
  });
  object.userData.def = def;
  return object;
}

// --- Building and posing -------------------------------------------------------------------

async function build() {
  const keepAngles = new Map();
  if (state.articulation) {
    state.articulation.groups.forEach((g, i) => {
      if (g.jointPart != null) keepAngles.set(state.parts[g.jointPart]?.name, state.angles[i]);
    });
  }
  root.clear();
  overlays.clear();
  if (state.mode === "part") {
    const def = state.catalog.get(state.partId);
    if (!def) return;
    state.design = null;
    state.parts = [{ name: def.id, def, position: [0, 0, 0], rotation: [0, 0, 0, 1], parent: null, attach: null }];
  } else {
    state.design = await getJson(`/content/data/vehicles/${state.designId}.json`);
    state.parts = partsOf(state.design, state.catalog);
  }
  state.articulation = articulationOf(state.parts);
  const n = state.articulation.groups.length;
  const fresh = state.angles.length !== n || keepAngles.size === 0;
  state.angles = new Array(n).fill(0);
  if (fresh && state.design) {
    // Start like the game: in the design's "stand" pose if it has one.
    state.angles = namedPose(params.get("pose") ?? "stand") ?? state.angles;
  } else {
    state.articulation.groups.forEach((g, i) => {
      const name = g.jointPart != null ? state.parts[g.jointPart].name : null;
      if (keepAngles.has(name)) state.angles[i] = keepAngles.get(name);
    });
  }

  state.groupNodes = state.articulation.groups.map(() => {
    const g = new THREE.Group();
    g.matrixAutoUpdate = false;
    root.add(g);
    return g;
  });
  const objects = await Promise.all(state.parts.map((p) => partObject(p.def)));
  state.parts.forEach((p, i) => {
    const object = objects[i];
    object.position.set(...p.position);
    object.quaternion.set(...p.rotation);
    object.userData.partIndex = i;
    state.groupNodes[state.articulation.partGroup[i]].add(object);
    // Overlays in the same group: collision boxes (a shell's tiles) and attach nodes.
    for (const b of shapeBoxes(p.def.shape)) {
      const box = new THREE.Mesh(
        new THREE.BoxGeometry(...b.half.map((h) => 2 * h)),
        new THREE.MeshBasicMaterial({ color: 0x4fd1ff, wireframe: true, transparent: true, opacity: 0.5 }),
      );
      box.position.set(...b.centre).applyQuaternion(object.quaternion).add(object.position);
      box.quaternion.copy(object.quaternion).multiply(new THREE.Quaternion(...b.rotation));
      box.userData.overlay = "boxes";
      state.groupNodes[state.articulation.partGroup[i]].add(box);
    }
    for (const node of p.def.attach_nodes ?? []) {
      const dir = new THREE.Vector3(...node.direction).applyQuaternion(object.quaternion);
      const at = new THREE.Vector3(...node.position_m).applyQuaternion(object.quaternion).add(object.position);
      const arrow = new THREE.ArrowHelper(dir, at, 0.6, 0xffd34f, 0.2, 0.12);
      arrow.userData.overlay = "nodes";
      state.groupNodes[state.articulation.partGroup[i]].add(arrow);
    }
  });
  const bounds = new THREE.Box3().setFromObject(root);
  state.size = Math.max(2, bounds.getSize(new THREE.Vector3()).length());
  buildPanel();
  applyPoses();
  updateOverlays();
}

function applyPoses() {
  const poses = posesOf(state.articulation, state.angles);
  poses.forEach((pose, i) => {
    const m = new THREE.Matrix4().compose(
      new THREE.Vector3(...pose.translation),
      new THREE.Quaternion(...pose.rotation),
      new THREE.Vector3(1, 1, 1),
    );
    state.groupNodes[i].matrix.copy(m);
    state.groupNodes[i].matrixWorldNeedsUpdate = true;
  });
  const { mass, centre } = massOf(state.parts, state.articulation, poses);
  comMarker.position.set(...centre);
  const low = lowestPoint(state.parts, state.articulation, poses);
  ground.position.y = state.mode === "part" ? -halfExtents(state.parts[0].def.shape)[1] - 0.001 : low - 0.001;
  const bounds = new THREE.Box3().setFromObject(root);
  const size = bounds.getSize(new THREE.Vector3());
  const joints = state.articulation.groups.length - 1;
  $("info").textContent =
    `${state.parts.length} parts, ${joints} joints\n` +
    `mass ${(mass / 1000).toFixed(1)} t\n` +
    `size ${size.x.toFixed(1)} x ${size.y.toFixed(1)} x ${size.z.toFixed(1)} m\n` +
    `centre of mass ${centre.map((c) => c.toFixed(2)).join(", ")}\n` +
    `lowest point y ${low.toFixed(2)} m` +
    (state.design ? `\n\n${state.design.name}: ${state.design.description ?? ""}` : "");
}

function updateOverlays() {
  const show = { boxes: $("showBoxes").checked, nodes: $("showNodes").checked };
  root.traverse((o) => {
    if (o.userData.overlay) o.visible = show[o.userData.overlay];
  });
  comMarker.visible = $("showCom").checked;
  ground.visible = $("showGround").checked;
}

// --- Panel ---------------------------------------------------------------------------------

/// A design pose by name, or one of its stages as "name.k" (k from 1); null without one.
function namedPose(name) {
  const m = /^(.*)\.(\d+)$/.exec(name);
  const pose = m ? state.design.pose_stages?.[m[1]]?.[Number(m[2]) - 1] : state.design.poses?.[name];
  return pose ? poseAngles({ poses: { [name]: pose } }, state.parts, state.articulation, name) : null;
}

function buildPanel() {
  const poses = $("poses");
  poses.innerHTML = "";
  $("poseSection").style.display = state.mode === "design" ? "" : "none";
  if (state.design) {
    const names = Object.keys(state.design.poses ?? {});
    // Each pose, preceded by the stages it passes through on the way (name·1, name·2, ...).
    const buttons = [["rest", null]];
    for (const name of names) {
      (state.design.pose_stages?.[name] ?? []).forEach((stage, k) => buttons.push([`${name}·${k + 1}`, stage]));
      buttons.push([name, state.design.poses[name]]);
    }
    for (const [label, pose] of buttons) {
      const b = document.createElement("button");
      b.textContent = label;
      b.onclick = () => {
        const n = state.articulation.groups.length;
        state.angles = pose === null
          ? new Array(n).fill(0)
          : poseAngles({ poses: { [label]: pose } }, state.parts, state.articulation, label);
        syncSliders();
        applyPoses();
      };
      poses.append(b);
    }
  }
  const joints = $("joints");
  joints.innerHTML = "";
  let count = 0;
  state.articulation.groups.forEach((g, i) => {
    if (g.jointPart == null) return;
    count++;
    const name = state.parts[g.jointPart].name;
    const slides = g.joint.type === "slider";
    const limits = slides
      ? g.joint.limits_m ?? [0, 1]
      : g.joint.type === "hinge" && g.joint.limits_deg ? g.joint.limits_deg : [-180, 180];
    const row = document.createElement("div");
    row.className = "joint";
    row.dataset.name = name;
    row.innerHTML = `<span title="${name}">${name}</span><span class="val"></span>
      <input type="range" min="${limits[0]}" max="${limits[1]}" step="${slides ? 0.05 : 1}">`;
    const slider = row.querySelector("input");
    slider.dataset.group = i;
    slider.oninput = () => {
      state.angles[i] = clampJoint(g.joint, Number(slider.value) * jointUnit(g.joint));
      row.querySelector(".val").textContent = jointLabel(g.joint, Number(slider.value));
      applyPoses();
    };
    joints.append(row);
  });
  $("jointCount").textContent = `(${count})`;
  syncSliders();
  filterJoints();
}

/// A joint's value as shown: whole degrees, or metres for a slider.
function jointLabel(joint, value) {
  return joint?.type === "slider" ? `${value.toFixed(2)} m` : `${Math.round(value)}°`;
}

function syncSliders() {
  for (const slider of document.querySelectorAll("#joints input[type=range]")) {
    const g = state.articulation.groups[Number(slider.dataset.group)];
    const value = state.angles[Number(slider.dataset.group)] / jointUnit(g.joint);
    slider.value = value;
    slider.parentElement.querySelector(".val").textContent = jointLabel(g.joint, value);
  }
}

function filterJoints() {
  const f = $("jointFilter").value.toLowerCase();
  for (const row of document.querySelectorAll("#joints .joint")) {
    row.style.display = row.dataset.name.toLowerCase().includes(f) ? "" : "none";
  }
}

// Views: yaw 0 looks at the back (from behind, +Z), 180 at the front; like the game's chase
// camera orbit.
const VIEWS = {
  front: [180, 8],
  "front-left": [225, 12],
  left: [270, 8],
  "back-left": [315, 12],
  back: [0, 8],
  "back-right": [45, 12],
  right: [90, 8],
  "front-right": [135, 12],
  top: [180, 80],
  low: [200, -12],
};

/// With `?focus=<name prefixes, comma separated>`, the box around the parts whose names start
/// with one of them (to frame shots on a detail); otherwise none.
function focusBounds() {
  const prefixes = params.get("focus")?.split(",");
  if (!prefixes || state.mode === "part") return null;
  const bounds = new THREE.Box3();
  root.updateMatrixWorld(true);
  root.traverse((o) => {
    if (o.userData.partIndex == null) return;
    const name = state.parts[o.userData.partIndex]?.name ?? "";
    if (prefixes.some((f) => name.startsWith(f))) bounds.expandByObject(o);
  });
  return bounds.isEmpty() ? null : bounds;
}

/// Fits the sun's shadow camera round the whole vehicle.
function fitShadow() {
  const bounds = new THREE.Box3().setFromObject(root);
  if (bounds.isEmpty()) return;
  const centre = bounds.getCenter(new THREE.Vector3());
  const radius = bounds.getSize(new THREE.Vector3()).length() / 2;
  const dir = new THREE.Vector3(-30, 50, -20).normalize();
  sun.position.copy(centre).addScaledVector(dir, radius * 2);
  sun.target.position.copy(centre);
  const cam = sun.shadow.camera;
  cam.left = cam.bottom = -radius;
  cam.right = cam.top = radius;
  cam.near = radius * 0.5;
  cam.far = radius * 4;
  cam.updateProjectionMatrix();
}

function setView(name) {
  fitShadow();
  const [yaw, pitch] = VIEWS[name] ?? VIEWS["front-right"];
  const bounds = focusBounds() ?? new THREE.Box3().setFromObject(root);
  const centre = bounds.getCenter(new THREE.Vector3());
  const radius = bounds.getSize(new THREE.Vector3()).length() / 2;
  const distance = radius / Math.sin((camera.fov * DEG) / 2) * 0.95;
  const dir = new THREE.Vector3(
    Math.sin(yaw * DEG) * Math.cos(pitch * DEG),
    Math.sin(pitch * DEG),
    Math.cos(yaw * DEG) * Math.cos(pitch * DEG),
  );
  controls.target.copy(centre);
  camera.position.copy(centre).addScaledVector(dir, distance);
  camera.near = distance / 100;
  camera.far = distance * 20;
  camera.updateProjectionMatrix();
  controls.update();
}

for (const name of Object.keys(VIEWS)) {
  const b = document.createElement("button");
  b.textContent = name;
  b.onclick = () => setView(name);
  $("views").append(b);
}

// --- Output: review shots, poses, models ----------------------------------------------------

async function save(path, payload) {
  const r = await fetch("/api/save", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ path, ...payload }),
  });
  const result = await r.json();
  if (!r.ok) throw new Error(result.error);
  return result;
}

function render() {
  controls.update();
  root.updateMatrixWorld(true);
  composer.render();
}

/// Renders each standard view and saves it to .viewer-shots/<subject>-<view>.png.
async function reviewShots(views = Object.keys(VIEWS)) {
  const focus = params.get("focus") ? `-${params.get("focus").replaceAll(",", "-")}` : "";
  const subject = state.mode === "part" ? state.partId : `${state.designId}-${params.get("pose") ?? "current"}${focus}`;
  for (const name of views) {
    setView(name);
    render();
    const data = renderer.domElement.toDataURL("image/png").split(",")[1];
    await save(`.viewer-shots/${subject}-${name}.png`, { base64: data });
  }
  status(`saved ${views.length} shots to .viewer-shots/`);
}

/// A design file in the game's layout (design.rs design_json): one part per line.
function designText(design) {
  const value = (v) => {
    if (Array.isArray(v)) return `[${v.map(value).join(", ")}]`;
    if (v && typeof v === "object") {
      return `{${Object.entries(v).map(([k, x]) => `${JSON.stringify(k)}: ${value(x)}`).join(", ")}}`;
    }
    return JSON.stringify(v);
  };
  const lines = ["{"];
  const top = Object.entries(design).filter(([k]) => !["parts", "poses", "pose_stages", "gait"].includes(k));
  for (const [k, v] of top) lines.push(`  ${JSON.stringify(k)}: ${JSON.stringify(v)},`);
  lines.push('  "parts": [');
  design.parts.forEach((p, i) => lines.push(`    ${value(p)}${i + 1 < design.parts.length ? "," : ""}`));
  lines.push("  ]");
  const poses = Object.entries(design.poses ?? {});
  if (poses.length) {
    lines[lines.length - 1] += ",";
    lines.push('  "poses": {');
    poses.forEach(([name, angles], i) => lines.push(`    ${JSON.stringify(name)}: ${value(angles)}${i + 1 < poses.length ? "," : ""}`));
    lines.push("  }");
  }
  const stages = Object.entries(design.pose_stages ?? {});
  if (stages.length) {
    lines[lines.length - 1] += ",";
    lines.push('  "pose_stages": {');
    stages.forEach(([name, list], i) =>
      lines.push(`    ${JSON.stringify(name)}: [${list.map(value).join(", ")}]${i + 1 < stages.length ? "," : ""}`));
    lines.push("  }");
  }
  if (design.gait) {
    lines[lines.length - 1] += ",";
    lines.push(`  "gait": ${JSON.stringify(design.gait)}`);
  }
  lines.push("}");
  return lines.join("\n") + "\n";
}

$("savePose").onclick = async () => {
  const name = prompt("Pose name (saved into the design file; the game's P key cycles poses):");
  if (!name || !/^[a-z0-9_]+$/.test(name)) return;
  state.design.poses = { ...(state.design.poses ?? {}), [name]: poseFromAngles(state.parts, state.articulation, state.angles) };
  await save(`content/data/vehicles/${state.designId}.json`, { text: designText(state.design) });
  status(`saved pose "${name}" into content/data/vehicles/${state.designId}.json`);
};

/// Exports the shown part's model as a glTF binary in part axes and metres, each primitive's
/// material named by its slot (armor, trim, dark, glass): the format the game loads.
$("exportGlb").onclick = async () => {
  if (state.mode !== "part") {
    status("export works on a single part: choose 'A part and its model'");
    return;
  }
  const def = state.catalog.get(state.partId);
  const name = prompt(
    "Save as model (assets/export/models/<name>.glb; the part's form.model names the one the game uses):",
    def.form?.model ?? def.id,
  );
  if (!name) return;
  const object = state.groupNodes[0].children.find((o) => o.userData.def);
  const copy = object.clone(true);
  copy.position.set(0, 0, 0);
  copy.quaternion.identity();
  const glb = await new GLTFExporter().parseAsync(copy, { binary: true });
  const bytes = new Uint8Array(glb);
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  await save(`assets/export/models/${name}.glb`, { base64: btoa(binary) });
  status(`saved assets/export/models/${name}.glb (${bytes.length} bytes)`);
};

$("shots").onclick = () => reviewShots();

// --- Choosing what to show, live reload ----------------------------------------------------

function status(text) {
  $("status").textContent = text;
}

function fillSelects() {
  $("design").innerHTML = state.list.designs.map((d) => `<option>${d}</option>`).join("");
  const ids = [...state.catalog.keys()].sort();
  $("part").innerHTML = ids.map((d) => `<option>${d}</option>`).join("");
  $("design").value = state.designId;
  if (state.partId) $("part").value = state.partId;
  $("mode").value = state.mode;
  $("design").style.display = state.mode === "design" ? "" : "none";
  $("part").style.display = state.mode === "part" ? "" : "none";
}

$("mode").onchange = async () => {
  state.mode = $("mode").value;
  state.partId = state.partId ?? [...state.catalog.keys()][0];
  state.angles = [];
  fillSelects();
  await build();
  setView("front-right");
};
$("design").onchange = async () => {
  state.designId = $("design").value;
  state.angles = [];
  await build();
  setView("front-right");
};
$("part").onchange = async () => {
  state.partId = $("part").value;
  await build();
  setView("front-right");
};
for (const id of ["showBoxes", "showNodes", "showCom", "showGround"]) $(id).onchange = updateOverlays;
$("jointFilter").oninput = filterJoints;

// Part names under the pointer.
const raycaster = new THREE.Raycaster();
renderer.domElement.addEventListener("pointermove", (e) => {
  const rect = renderer.domElement.getBoundingClientRect();
  const p = new THREE.Vector2(((e.clientX - rect.left) / rect.width) * 2 - 1, -((e.clientY - rect.top) / rect.height) * 2 + 1);
  raycaster.setFromCamera(p, camera);
  const hit = raycaster.intersectObject(root, true).find((h) => h.object.visible && !h.object.parent?.userData?.overlay && !h.object.userData.overlay);
  let o = hit?.object;
  while (o && o.userData.partIndex == null) o = o.parent;
  const hover = $("hover");
  if (o) {
    const p = state.parts[o.userData.partIndex];
    hover.textContent = `${p.name}  (${p.def.id})`;
    hover.style.left = `${e.clientX - rect.left + 12}px`;
    hover.style.top = `${e.clientY - rect.top + 12}px`;
    hover.style.display = "block";
  } else {
    hover.style.display = "none";
  }
});

async function poll() {
  try {
    const { version } = await getJson("/api/version");
    if (state.version !== null && version !== state.version) {
      state.version = version;
      state.models.clear();
      const position = camera.position.clone();
      const target = controls.target.clone();
      await loadCatalog();
      await build();
      camera.position.copy(position);
      controls.target.copy(target);
      status(`reloaded ${new Date().toLocaleTimeString()}`);
    }
    state.version = version;
  } catch (e) {
    status(`server not reachable: ${e.message}`);
  }
}

async function main() {
  const { version } = await getJson("/api/version");
  state.version = version;
  await loadCatalog();
  state.designId = params.get("design") ?? state.list.designs[0];
  fillSelects();
  if (params.get("bare")) {
    // Review shots show the model alone.
    document.body.classList.add("bare");
    $("showCom").checked = false;
  }
  await build();
  setView(params.get("view") ?? "front-right");
  resize();
  if (params.get("shots")) {
    // Headless review: save the standard views, then signal done.
    const views = params.get("views")?.split(",");
    await reviewShots(views);
    document.title = "shots-done";
    return;
  }
  setInterval(poll, 1000);
  renderer.setAnimationLoop(render);
  status("ready");
}

main().catch((e) => {
  status(`error: ${e.message}`);
  document.title = "error";
  console.error(e);
});
