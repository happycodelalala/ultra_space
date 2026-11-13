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
  // 22 joints (Rust: the_bastion_assembles_into_a_valid_balanced_design).
  assert.equal(a.groups.length - 1, 22);
});

test("poses put the shells where the game does", () => {
  const design = load("bastion");
  const parts = partsOf(design, catalog);
  const a = articulationOf(parts);
  const middle = (angles, shell) => {
    const poses = posesOf(a, angles);
    const facets = parts
      .map((p, i) => [p, i])
      .filter(([p]) => p.name.startsWith(`${shell}_facet_`) && p.name.endsWith("_left"));
    const sum = facets.reduce(
      (s, [p, i]) => {
        const x = applyPose(poses[a.partGroup[i]], p.position);
        return [s[0] + x[0], s[1] + x[1], s[2] + x[2]];
      },
      [0, 0, 0],
    );
    return sum.map((c) => c / facets.length);
  };
  // Rust (the_quarters_open_like_petals): "bloom front left: middle (-7.3, 0.4, -5.6)",
  // "bloom back left: middle (-7.0, 1.0, 6.5)"; closed "(-2.8, -3.9, -2.4)".
  const bloom = poseAngles(design, parts, a, "bloom");
  near(middle(bloom, "front"), [-7.3, 0.4, -5.6], 0.1, "bloom front");
  near(middle(bloom, "back"), [-7.0, 1.0, 6.5], 0.1, "bloom back");
  near(middle(poseAngles(design, parts, a, "cocoon"), "front"), [-2.8, -3.9, -2.4], 0.1, "cocoon");
});

test("a pose read and written back is the same pose", () => {
  const design = load("bastion");
  const parts = partsOf(design, catalog);
  const a = articulationOf(parts);
  const angles = poseAngles(design, parts, a, "bloom");
  assert.deepEqual(poseFromAngles(parts, a, angles), design.poses.bloom);
});

test("the heavy frame stands on its soles", () => {
  const design = load("heavy_frame");
  const parts = partsOf(design, catalog);
  const a = articulationOf(parts);
  const rest = new Array(a.groups.length).fill(0);
  // Soles 16.8 m below the origin (Rust: legs ... sole -16.8).
  assert.ok(Math.abs(lowestPoint(parts, a, posesOf(a, rest)) + 16.8) < 0.05);
});
