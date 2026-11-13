// Checks the viewer's port of the game's designs and joints against the game's own results
// (numbers from the Rust tests). Run: node --test impl/viewer/
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

import {
  articulationOf,
  catalogOf,
  lowestPoint,
  partsOf,
  poseAngles,
  poseFromAngles,
  posesOf,
  applyPose,
  quat,
  rotationFromDegrees,
  shapeTiles,
  shapeCentroid,
} from "./articulation.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "content", "data");
const catalog = catalogOf(
  readdirSync(join(root, "parts"))
    .filter((f) => f.endsWith(".json"))
    .map((f) => JSON.parse(readFileSync(join(root, "parts", f), "utf8"))),
);
const load = (id) => JSON.parse(readFileSync(join(root, "vehicles", `${id}.json`), "utf8"));
const near = (a, b, tol, what) =>
  assert.ok(Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]) < tol, `${what}: ${a} vs ${b}`);

test("rotations follow the design convention (fixed X, then Y, then Z)", () => {
  // 90° about X then 90° about Y: +Y goes to +Z, then to +X.
  const q = rotationFromDegrees([90, 90, 0]);
  near(quat.rotate(q, [0, 1, 0]), [1, 0, 0], 1e-9, "+Y");
});

test("the Bastion splits into the same rigid groups as in the game", () => {
  const design = load("bastion");
  const parts = partsOf(design, catalog);
  const a = articulationOf(parts);
  // 94 joints (Rust: the_bastion_assembles_into_a_valid_balanced_design).
  assert.equal(a.groups.length - 1, 94);
});

test("poses put the shells where the game does", () => {
  const design = load("bastion");
  const parts = partsOf(design, catalog);
  const a = articulationOf(parts);
  // A quarter's middle (its centroid) and its distance from the cocoon's axis (z = 0.3).
  const radius = (angles, name) => {
    const i = parts.findIndex((p) => p.name === name);
    const p = parts[i];
    const at = applyPose(posesOf(a, angles)[a.partGroup[i]], [0, 0, 0].map((_, k) => p.position[k] + quat.rotate(p.rotation, shapeCentroid(p.def.shape))[k]));
    return Math.hypot(at[0], at[2] - 0.3);
  };
  // Rust (every_pose_and_every_move_between_them_is_free_of_interference): closed 5.5 m;
  // held out in stand (forearms telescoped) 10.7 m (front) and 11.9 m (back).
  const close = (x, want, what) => assert.ok(Math.abs(x - want) < 0.1, `${what}: ${x} vs ${want}`);
  close(radius(poseAngles(design, parts, a, "cocoon"), "front_shell_left"), 5.5, "cocoon");
  close(radius(poseAngles(design, parts, a, "stand"), "front_shell_left"), 10.7, "stand front");
  close(radius(poseAngles(design, parts, a, "stand"), "back_shell_right"), 11.9, "stand back");
});

test("a pose read and written back is the same pose", () => {
  const design = load("bastion");
  const parts = partsOf(design, catalog);
  const a = articulationOf(parts);
  const angles = poseAngles(design, parts, a, "stand");
  assert.deepEqual(poseFromAngles(parts, a, angles), design.poses.stand);
});

test("the heavy frame stands on its soles", () => {
  const design = load("heavy_frame");
  const parts = partsOf(design, catalog);
  const a = articulationOf(parts);
  const rest = new Array(a.groups.length).fill(0);
  // Soles 16.8 m below the origin (Rust: legs ... sole -16.8).
  assert.ok(Math.abs(lowestPoint(parts, a, posesOf(a, rest)) + 16.8) < 0.05);
});

test("shells tile like the game's", () => {
  const shell = catalog.get("bastion.shell_quarter_front_left").shape;
  const tiles = shapeTiles(shell);
  // 87° arc in 7.5° steps: 12 columns; 3 spans x 6 steps: 18 rows; 1.2 m thick.
  assert.equal(tiles.length, 12 * 18);
  for (const t of tiles) assert.ok(t.half[0] > 0 && t.half[1] > 0 && Math.abs(t.half[2] - 0.6) < 1e-12);
  // Symmetric arc: the centroid lies on the part's +Z side, in its middle plane.
  const c = shapeCentroid(shell);
  assert.ok(Math.abs(c[0]) < 1e-9 && c[2] > 3);
});
