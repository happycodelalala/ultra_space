// Surface detail for part models: the viewer's copy of the game's hull detail shader
// (impl/rust/crates/game_bevy/src/shaders/hull_detail.wesl; keep the two in sync). Plate seams
// in staggered rows, heavier structural seams every few plates, fastener rows, a tint and sheen
// difference plate to plate and wear, at real sizes from the model's first UV set (the surface
// position in metres along each plate). Seams stay at least a pixel wide (fainter when thinner)
// and fade once the plates are only a few pixels across.

const GLSL = /* glsl */ `
uniform vec4 hdPlate; // x plate height (m), y width / height, z structural every, w strength
uniform vec4 hdSeam;  // x seam width (m), y fastener spacing (m), z grime
float hdHash(vec2 p) {
  p = floor(p);
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}
vec3 hdSeams(vec2 uv, vec2 size) {
  float row = floor(uv.y / size.y);
  vec2 shifted = vec2(uv.x + (mod(row, 2.0) > 0.5 ? 0.5 * size.x : 0.0), uv.y);
  vec2 cell = floor(shifted / size);
  vec2 f = shifted / size - cell;
  float d = min(min(f.x, 1.0 - f.x) * size.x, min(f.y, 1.0 - f.y) * size.y);
  return vec3(d, cell);
}
`;

const COLOR = /* glsl */ `
#include <color_fragment>
float hdGroove = 0.0, hdEdge = 0.0, hdR = 0.5;
if (hdPlate.w > 0.0) {
  vec2 uv = vUv;
  float footprint = max(length(fwidth(uv)), 1e-5);
  float h = hdPlate.x;
  vec3 small = hdSeams(uv, vec2(h * hdPlate.y, h));
  vec3 big = hdSeams(uv + vec2(0.37, 0.61) * h, vec2(h * hdPlate.y, h) * hdPlate.z);
  float w = hdSeam.x;
  float fadeSmall = clamp(w / footprint, 0.25, 1.0) * (1.0 - smoothstep(h * 0.04, h * 0.12, footprint));
  float fadeBig = clamp(2.0 * w / footprint, 0.5, 1.0) * (1.0 - smoothstep(h * 0.2, h * 0.6, footprint));
  float lw = max(w, footprint);
  float lwBig = max(2.0 * w, footprint);
  hdGroove = (1.0 - smoothstep(lw * 0.5, lw + footprint * 0.5, small.x)) * fadeSmall
    + (1.0 - smoothstep(lwBig * 0.5, lwBig + footprint * 0.5, big.x)) * fadeBig;
  hdEdge = (smoothstep(w, w + footprint, small.x) - smoothstep(w * 2.5, w * 2.5 + footprint, small.x)) * fadeSmall;
  float s = hdSeam.y;
  vec2 along = abs(fract(uv / s) - 0.5) * s;
  float inset = abs(small.x - w * 3.0);
  float head = (1.0 - smoothstep(w * 0.6, w * 0.6 + footprint, length(vec2(inset, min(along.x, along.y)))))
    * clamp(1.0 - footprint / (w * 0.8), 0.0, 1.0);
  hdR = hdHash(small.yz + vec2(17.0, 3.0) * floor(big.yz));
  float tint = 1.0 + (hdR - 0.5) * 0.14 + (hdHash(big.yz) - 0.5) * 0.08;
  float grime = hdSeam.z * smoothstep(0.6, 0.0, fract(uv.y / (h * hdPlate.z))) * 0.12;
  vec3 c = diffuseColor.rgb * tint * (1.0 - grime);
  c = mix(c, c * 0.25, clamp(hdGroove, 0.0, 1.0) * 0.85);
  c = mix(c, c * 1.35 + 0.02, hdEdge * 0.5);
  c = mix(c, c * 0.55, head * 0.7);
  diffuseColor.rgb = mix(diffuseColor.rgb, c, hdPlate.w);
}
`;

const ROUGHNESS = /* glsl */ `
#include <roughnessmap_fragment>
roughnessFactor = clamp(roughnessFactor + hdPlate.w * ((hdR - 0.5) * 0.16 + hdGroove * 0.25 - hdEdge * 0.15), 0.05, 1.0);
`;

/// Gives `material` (a MeshStandardMaterial) the hull detail of palette entry `detail`
/// (content/data/part_materials.json), or none.
export function applyHullDetail(material, detail) {
  if (!detail) return material;
  const plate = [detail.plate_m ?? 3, detail.aspect ?? 2, detail.structural_every ?? 4, detail.strength ?? 1];
  const seam = [detail.seam_m ?? 0.05, detail.fastener_m ?? 0.5, detail.grime ?? 1, 0];
  material.defines = { ...(material.defines ?? {}), USE_UV: "" };
  material.onBeforeCompile = (shader) => {
    shader.uniforms.hdPlate = { value: plate };
    shader.uniforms.hdSeam = { value: seam };
    shader.fragmentShader = shader.fragmentShader
      .replace("#include <common>", `#include <common>\n${GLSL}`)
      .replace("#include <color_fragment>", COLOR)
      .replace("#include <roughnessmap_fragment>", ROUGHNESS);
  };
  material.customProgramCacheKey = () => `hull-detail-${plate.join(",")}-${seam.join(",")}`;
  return material;
}
