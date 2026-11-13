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
    const turn = quat.axisAngle(g.axis, angles[i] ?? 0);
    const local = { rotation: turn, translation: v3.sub(g.pivot, quat.rotate(turn, g.pivot)) };
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

/// A joint's angle clamped to its limits (radians; rotors turn freely).
export function clampJoint(joint, angle) {
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
  for (const [joint, deg] of Object.entries(pose)) {
    const part = parts.findIndex((p) => p.name === joint);
    const g = articulation.groups.findIndex((gr) => gr.jointPart === part);
    if (g > 0) angles[g] = clampJoint(articulation.groups[g].joint, deg * DEG);
  }
  return angles;
}

/// A named pose from joint angles, in the design file's format (joint part name -> whole
/// degrees; joints at rest left out).
export function poseFromAngles(parts, articulation, angles) {
  const pose = {};
  articulation.groups.forEach((g, i) => {
    if (g.jointPart == null) return;
    const deg = Math.round((angles[i] / DEG) * 10) / 10;
    if (deg !== 0) pose[parts[g.jointPart].name] = deg;
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
    moment = v3.add(moment, v3.scale(applyPose(poses[articulation.partGroup[i]], p.position), m));
  });
  return { mass, centre: mass > 0 ? v3.scale(moment, 1 / mass) : [0, 0, 0] };
}

/// Half extents of a collision shape in part axes.
export function halfExtents(shape) {
  if (shape.type === "box") return shape.size_m.map((s) => s / 2);
  if (shape.type === "cylinder") return [shape.radius_m, shape.length_m / 2, shape.radius_m];
  return [shape.radius_m, shape.radius_m, shape.radius_m];
}

/// The lowest point (vehicle-frame y) of the posed collision shapes: where the ground is when
/// the vehicle stands on it.
export function lowestPoint(parts, articulation, poses) {
  let lowest = Infinity;
  parts.forEach((p, i) => {
    const pose = poses[articulation.partGroup[i]];
    const rot = quat.mul(pose.rotation, p.rotation);
    const centre = applyPose(pose, p.position);
    const h = halfExtents(p.def.shape);
    // Extent along world Y of a box turned by `rot`.
    const ax = quat.rotate(rot, [1, 0, 0]);
    const ay = quat.rotate(rot, [0, 1, 0]);
    const az = quat.rotate(rot, [0, 0, 1]);
    const extent = Math.abs(ax[1]) * h[0] + Math.abs(ay[1]) * h[1] + Math.abs(az[1]) * h[2];
    lowest = Math.min(lowest, centre[1] - extent);
  });
  return lowest;
}
