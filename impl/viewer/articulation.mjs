// Designs and jointed parts, ported from sim_vehicle (design.rs, joints.rs) so the viewer shows
// exactly what the game does. Pure functions on plain arrays: no three.js, testable in Node.
//
// Conventions (same as the game): metres, Y up, forward -Z; a part's position and rotation are
// in the vehicle frame; `rotation_deg` turns about the fixed X, then Y, then Z axes.

// --- Small vector and quaternion helpers ([x, y, z] and [x, y, z, w]) -----------------------

export const v3 = {
  add: (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]],
  sub: (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]],
  scale: (a, s) => [a[0] * s, a[1] * s, a[2] * s],
  dot: (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2],
  length: (a) => Math.hypot(a[0], a[1], a[2]),
  normalize: (a) => {
    const l = Math.hypot(a[0], a[1], a[2]) || 1;
    return [a[0] / l, a[1] / l, a[2] / l];
  },
};

export const quat = {
  identity: () => [0, 0, 0, 1],
  axisAngle(axis, angle) {
    const [x, y, z] = v3.normalize(axis);
    const s = Math.sin(angle / 2);
    return [x * s, y * s, z * s, Math.cos(angle / 2)];
  },
  mul(a, b) {
    const [ax, ay, az, aw] = a;
    const [bx, by, bz, bw] = b;
    return [
      aw * bx + ax * bw + ay * bz - az * by,
      aw * by - ax * bz + ay * bw + az * bx,
      aw * bz + ax * by - ay * bx + az * bw,
      aw * bw - ax * bx - ay * by - az * bz,
    ];
  },
  rotate(q, v) {
    const [x, y, z, w] = q;
    // v + 2w(q×v) + 2 q×(q×v)
    const tx = 2 * (y * v[2] - z * v[1]);
    const ty = 2 * (z * v[0] - x * v[2]);
    const tz = 2 * (x * v[1] - y * v[0]);
    return [
      v[0] + w * tx + (y * tz - z * ty),
      v[1] + w * ty + (z * tx - x * tz),
      v[2] + w * tz + (x * ty - y * tx),
    ];
  },
  normalize(q) {
    const l = Math.hypot(q[0], q[1], q[2], q[3]) || 1;
    return [q[0] / l, q[1] / l, q[2] / l, q[3] / l];
  },
};

const DEG = Math.PI / 180;

/// Rotation from degrees about the fixed X, then Y, then Z axes (design.rs).
export function rotationFromDegrees(deg = [0, 0, 0]) {
  const [x, y, z] = deg.map((d) => d * DEG);
  return quat.normalize(
    quat.mul(quat.axisAngle([0, 0, 1], z), quat.mul(quat.axisAngle([0, 1, 0], y), quat.axisAngle([1, 0, 0], x))),
  );
}

// --- Catalog and designs -----------------------------------------------------------------------

/// Builds a catalog (id -> part definition) from the parsed part files.
export function catalogOf(files) {
  const catalog = new Map();
  for (const file of files) for (const part of file.parts) catalog.set(part.id, part);
  return catalog;
}

/// A design file's parts resolved against the catalog: [{name, def, position, rotation, parent,
/// attach}] in file order (parents come first). Throws on unknown parts or parents.
export function partsOf(design, catalog) {
  const index = new Map();
  return design.parts.map((p, i) => {
    const def = catalog.get(p.part);
    if (!def) throw new Error(`part ${p.id}: unknown part ${p.part}`);
    let parent = null;
    if (p.parent != null) {
      if (!index.has(p.parent)) throw new Error(`part ${p.id}: parent ${p.parent} must come first`);
      parent = index.get(p.parent);
    }
    index.set(p.id, i);
    return {
      name: p.id,
      def,
      position: p.position_m,
      rotation: rotationFromDegrees(p.rotation_deg),
      parent,
      attach: p.attach ?? null,
    };
  });
}

/// A node of a part in the vehicle frame: [position, direction], or null.
export function nodeInVehicle(part, id) {
  const node = (part.def.attach_nodes ?? []).find((n) => n.id === id);
  if (!node) return null;
  return [
    v3.add(part.position, quat.rotate(part.rotation, node.position_m)),
    quat.rotate(part.rotation, node.direction),
  ];
}

// --- Articulation (joints.rs) ------------------------------------------------------------------

/// Splits parts at their joints into rigid groups: parts attached by their node to a joint
/// part's `end` node start a new group turning about the joint's axis through that node.
/// Returns {groups: [{parent, jointPart, joint, pivot, axis}], partGroup: [group per part]}.
export function articulationOf(parts) {
  const groups = [{ parent: null, jointPart: null, joint: null, pivot: [0, 0, 0], axis: [1, 0, 0] }];
  const partGroup = [];
  parts.forEach((p) => {
    const parent = p.parent != null && p.parent < partGroup.length ? p.parent : null;
    let group = 0;
    if (parent != null) {
      const attach = p.attach;
      const jp = parts[parent];
      if (attach && typeof attach === "object" && attach.to === "end" && jp.def.joint) {
        const [pivot] = nodeInVehicle(jp, "end") ?? [[0, 0, 0]];
        groups.push({
          parent: partGroup[parent],
          jointPart: parent,
          joint: jp.def.joint,
          pivot,
          axis: v3.normalize(quat.rotate(jp.rotation, jp.def.joint.axis)),
        });
        group = groups.length - 1;
      } else {
        group = partGroup[parent];
      }
    }
    partGroup.push(group);
  });
  return { groups, partGroup };
}

/// Each group's pose {rotation, translation} for joint `angles` (radians, one per group): a
/// rest-frame point x of group g is at rotation * x + translation.
export function posesOf(articulation, angles) {
  const poses = [];
  articulation.groups.forEach((g, i) => {
    if (g.parent == null) {
      poses.push({ rotation: quat.identity(), translation: [0, 0, 0] });
      return;
    }
    const value = angles[i] ?? 0;
    let local;
    if (g.joint?.type === "slider") {
      local = { rotation: quat.identity(), translation: v3.scale(g.axis, value) };
    } else {
      const turn = quat.axisAngle(g.axis, value);
      local = { rotation: turn, translation: v3.sub(g.pivot, quat.rotate(turn, g.pivot)) };
    }
    const parent = poses[g.parent];
    poses.push({
      rotation: quat.normalize(quat.mul(parent.rotation, local.rotation)),
      translation: v3.add(quat.rotate(parent.rotation, local.translation), parent.translation),
    });
  });
  return poses;
}

export function applyPose(pose, x) {
  return v3.add(quat.rotate(pose.rotation, x), pose.translation);
}

/// Radians (or metres, for a slider) per joint value in data (degrees, or metres).
export function jointUnit(joint) {
  return joint?.type === "slider" ? 1 : DEG;
}

/// A joint's angle clamped to its limits (radians, or metres for a slider; rotors turn freely).
export function clampJoint(joint, angle) {
  if (joint?.type === "slider") {
    const [lo, hi] = joint.limits_m ?? [-Infinity, Infinity];
    return Math.min(hi, Math.max(lo, angle));
  }
  if (!joint || joint.type !== "hinge" || !joint.limits_deg) return angle;
  const [lo, hi] = joint.limits_deg.map((d) => d * DEG);
  return Math.min(hi, Math.max(lo, angle));
}

/// The angles (one per group) of the design's named pose: its joints at their angles, every
/// other joint at rest (joints.rs pose_angles). Null without such a pose.
export function poseAngles(design, parts, articulation, name) {
  const pose = design.poses?.[name];
  if (!pose) return null;
  const angles = new Array(articulation.groups.length).fill(0);
  for (const [name, value] of Object.entries(pose)) {
    const part = parts.findIndex((p) => p.name === name);
    const g = articulation.groups.findIndex((gr) => gr.jointPart === part);
    const joint = articulation.groups[g]?.joint;
    if (g > 0) angles[g] = clampJoint(joint, value * jointUnit(joint));
  }
  return angles;
}

/// A named pose from joint angles, in the design file's format (joint part name -> degrees to
/// a tenth, or metres to a centimetre for a slider; joints at rest left out).
export function poseFromAngles(parts, articulation, angles) {
  const pose = {};
  articulation.groups.forEach((g, i) => {
    if (g.jointPart == null) return;
    const slides = g.joint?.type === "slider";
    const value = slides ? Math.round(angles[i] * 100) / 100 : Math.round((angles[i] / DEG) * 10) / 10;
    if (value !== 0) pose[parts[g.jointPart].name] = value;
  });
  return pose;
}

/// Mass and centre of mass for the posed parts (point masses at the part centres, like the
/// game's rest-pose mass properties).
export function massOf(parts, articulation, poses) {
  let mass = 0;
  let moment = [0, 0, 0];
  parts.forEach((p, i) => {
    const m = p.def.mass_kg ?? 0;
    mass += m;
    const at = v3.add(p.position, quat.rotate(p.rotation, shapeCentroid(p.def.shape)));
    moment = v3.add(moment, v3.scale(applyPose(poses[articulation.partGroup[i]], at), m));
  });
  return { mass, centre: mass > 0 ? v3.scale(moment, 1 / mass) : [0, 0, 0] };
}

// --- Procedural shells (catalog.rs Shape::Shell) ------------------------------------------

export const SHELL_PROFILE_STEPS = 6; // catalog.rs
export const SHELL_ARC_STEP_DEG = 7.5;

/// A shell's side profile sampled top to bottom: [height, radius] on the Catmull-Rom curve
/// through its four points (catalog.rs shell_profile).
export function shellProfile(points, steps) {
  const p = (i) => points[Math.max(0, Math.min(3, i))];
  const out = [];
  for (let span = 0; span < 3; span++) {
    const [p0, p1, p2, p3] = [p(span - 1), p(span), p(span + 1), p(span + 2)];
    for (let k = 0; k < steps; k++) {
      const t = k / steps;
      const c = (a, b, cc, d) =>
        0.5 * (2 * b + (cc - a) * t + (2 * a - 5 * b + 4 * cc - d) * t * t + (3 * b - a - 3 * cc + d) * t * t * t);
      out.push([c(p0[0], p1[0], p2[0], p3[0]), c(p0[1], p1[1], p2[1], p3[1])]);
    }
  }
  out.push(points[3]);
  return out;
}

function shellPoint(deg, [y, r]) {
  const phi = deg * DEG;
  return [-r * Math.sin(phi), y, r * Math.cos(phi)];
}

/// A procedural shape's collision tiles: [{centre, rotation, half}] in part axes (catalog.rs
/// Shape::tiles); empty for the simple shapes.
export function shapeTiles(shape) {
  if (shape.type !== "shell") return [];
  const profile = shellProfile(shape.profile_m, SHELL_PROFILE_STEPS);
  const [a0, a1] = shape.arc_deg;
  const span = a1 - a0;
  const n = Math.max(1, Math.ceil(Math.abs(span) / SHELL_ARC_STEP_DEG));
  const tiles = [];
  for (let i = 0; i < n; i++) {
    const a = a0 + (span * i) / n;
    const b = a0 + (span * (i + 1)) / n;
    for (let k = 0; k + 1 < profile.length; k++) {
      const [top, bottom] = [profile[k], profile[k + 1]];
      const c = [shellPoint(a, top), shellPoint(b, top), shellPoint(a, bottom), shellPoint(b, bottom)];
      const centre = v3.scale(c.reduce(v3.add), 0.25);
      const across = v3.scale(v3.sub(v3.add(c[1], c[3]), v3.add(c[0], c[2])), 0.5);
      const down = v3.scale(v3.sub(v3.add(c[2], c[3]), v3.add(c[0], c[1])), 0.5);
      let x = v3.normalize(across);
      const upv = v3.scale(down, -1);
      let y = v3.normalize(v3.sub(upv, v3.scale(x, v3.dot(upv, x))));
      let z = cross(x, y);
      if (v3.dot(z, [centre[0], 0, centre[2]]) < 0) {
        z = v3.scale(z, -1);
        x = v3.scale(x, -1);
      }
      tiles.push({
        centre,
        rotation: quatFromBasis(x, y, z),
        half: [v3.length(across) / 2, v3.length(down) / 2, shape.thickness_m / 2],
      });
    }
  }
  return tiles;
}

function cross(a, b) {
  return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
}

/// The rotation whose columns are the orthonormal axes x, y, z.
function quatFromBasis(x, y, z) {
  const [m00, m10, m20] = x;
  const [m01, m11, m21] = y;
  const [m02, m12, m22] = z;
  const trace = m00 + m11 + m22;
  let q;
  if (trace > 0) {
    const s = Math.sqrt(trace + 1) * 2;
    q = [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, s / 4];
  } else if (m00 > m11 && m00 > m22) {
    const s = Math.sqrt(1 + m00 - m11 - m22) * 2;
    q = [s / 4, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s];
  } else if (m11 > m22) {
    const s = Math.sqrt(1 + m11 - m00 - m22) * 2;
    q = [(m01 + m10) / s, s / 4, (m12 + m21) / s, (m02 - m20) / s];
  } else {
    const s = Math.sqrt(1 + m22 - m00 - m11) * 2;
    q = [(m02 + m20) / s, (m12 + m21) / s, s / 4, (m10 - m01) / s];
  }
  return quat.normalize(q);
}

/// The box around a shape in part axes: {mid, half} (catalog.rs Shape::bounds).
export function shapeBounds(shape) {
  if (shape.type === "box") return { mid: [0, 0, 0], half: shape.size_m.map((s) => s / 2) };
  if (shape.type === "cylinder") return { mid: [0, 0, 0], half: [shape.radius_m, shape.length_m / 2, shape.radius_m] };
  if (shape.type === "sphere") return { mid: [0, 0, 0], half: [shape.radius_m, shape.radius_m, shape.radius_m] };
  let lo = [Infinity, Infinity, Infinity];
  let hi = [-Infinity, -Infinity, -Infinity];
  for (const t of shapeTiles(shape)) {
    for (const sx of [-1, 1]) for (const sy of [-1, 1]) for (const sz of [-1, 1]) {
      const c = v3.add(t.centre, quat.rotate(t.rotation, [t.half[0] * sx, t.half[1] * sy, t.half[2] * sz]));
      lo = lo.map((v, i) => Math.min(v, c[i]));
      hi = hi.map((v, i) => Math.max(v, c[i]));
    }
  }
  return { mid: v3.scale(v3.add(lo, hi), 0.5), half: v3.scale(v3.sub(hi, lo), 0.5) };
}

/// Where a shape's mass is centred in part axes (catalog.rs Shape::centroid).
export function shapeCentroid(shape) {
  const tiles = shapeTiles(shape);
  if (!tiles.length) return [0, 0, 0];
  let area = 0;
  let sum = [0, 0, 0];
  for (const t of tiles) {
    const a = t.half[0] * t.half[1];
    area += a;
    sum = v3.add(sum, v3.scale(t.centre, a));
  }
  return v3.scale(sum, 1 / area);
}

/// A part's collision boxes in part axes: its tiles, or the box around its shape.
export function shapeBoxes(shape) {
  const tiles = shapeTiles(shape);
  if (tiles.length) return tiles;
  const { mid, half } = shapeBounds(shape);
  return [{ centre: mid, rotation: [0, 0, 0, 1], half }];
}

/// Half extents of a collision shape's bounding box in part axes.
export function halfExtents(shape) {
  return shapeBounds(shape).half;
}

/// The lowest point (vehicle-frame y) of the posed collision shapes: where the ground is when
/// the vehicle stands on it.
export function lowestPoint(parts, articulation, poses) {
  let lowest = Infinity;
  parts.forEach((p, i) => {
    const pose = poses[articulation.partGroup[i]];
    for (const b of shapeBoxes(p.def.shape)) {
      const rot = quat.mul(pose.rotation, quat.mul(p.rotation, b.rotation));
      const centre = applyPose(pose, v3.add(p.position, quat.rotate(p.rotation, b.centre)));
      const h = b.half;
      // Extent along world Y of a box turned by `rot`.
      const ax = quat.rotate(rot, [1, 0, 0]);
      const ay = quat.rotate(rot, [0, 1, 0]);
      const az = quat.rotate(rot, [0, 0, 1]);
      const extent = Math.abs(ax[1]) * h[0] + Math.abs(ay[1]) * h[1] + Math.abs(az[1]) * h[2];
      lowest = Math.min(lowest, centre[1] - extent);
    }
  });
  return lowest;
}
