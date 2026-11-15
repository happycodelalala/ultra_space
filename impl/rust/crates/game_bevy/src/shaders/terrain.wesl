// Terrain tiles close up (Batch 2, task 2.5).
// Same lighting formula as planet.wesl, so the handover from the distant planet is seamless:
// colours blend from per-vertex biomes (close) to the baked map (far), steep slopes turn to rock,
// and fine detail varies the ground near the camera.

import bevy_pbr::render::{forward_io::VertexOutput, mesh_view_bindings::view};

struct TerrainUniform {
    star: vec4<f32>,      // xyz star position (render space), w starlight brightness
    center: vec4<f32>,    // xyz body centre (render space), w radius
    rotation: vec4<f32>,  // body rotation quaternion (xyzw)
    params: vec4<f32>,    // x baked map present, y blend start (m), z blend end (m), w detail fade (m)
    surface: vec4<f32>,   // x has ocean, y has atmosphere
    palette: array<vec4<f32>, 12>, // linear biome colours by biome index
    blocks: vec4<f32>,    // xyz block ground centre (body frame, unit), w cosine of its angular radius
    hide: vec4<f32>,      // x: tiles this close (horizontally) to the camera inside the block ground are hidden
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> terrain: TerrainUniform;
// Baked surface fields (height, temperature, moisture, continent signal) in the body's frame.
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var field_map: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var field_sampler: sampler;

// BIOMES BEGIN
// Biome rules: keep in sync with sim_planet::climate::Climate::biome (authoritative for
// gameplay) and with the same block in the other planet shader (a test checks the copies).
// Indices follow sim_planet::Biome: 0 Ocean, 1 SeaIce, 2 Beach, 3 Desert, 4 Grassland,
// 5 Forest, 6 Jungle, 7 Taiga, 8 Tundra, 9 Ice, 10 Rock, 11 Regolith.
// fields: x height (m), y temperature (°C), z moisture (0–1), w continent signal (land > 0).
// slope: radians from horizontal. flags: x has ocean, y has atmosphere.
fn biome_index(f: vec4<f32>, slope: f32, flags: vec4<f32>) -> u32 {
    let h = f.x;
    let t = f.y;
    let m = f.z;
    let land = f.w;
    let ocean = flags.x > 0.5;
    if ocean && h < 0.0 {
        if t < -2.0 { return 1u; }
        return 0u;
    }
    if flags.y < 0.5 {
        if t < -150.0 { return 9u; }
        return 11u;
    }
    if slope > 0.7 { return 10u; }
    if t < -50.0 { return 9u; }
    if m < 0.2 { return 3u; }
    if t < -10.0 { return 9u; }
    if t < 0.0 { return 8u; }
    if ocean && h < 25.0 && land < 0.03 { return 2u; }
    if t < 8.0 {
        if m > 0.4 { return 7u; }
        return 8u;
    }
    if t > 22.0 && m > 0.6 { return 6u; }
    if m > 0.45 { return 5u; }
    return 4u;
}
// BIOMES END


fn quat_rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

fn pcg3d(v_in: vec3<u32>) -> vec3<u32> {
    var v = v_in * 1664525u + 1013904223u;
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v ^= v >> vec3<u32>(16u);
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    return v;
}

fn hash3(p: vec3<f32>) -> f32 {
    let h = pcg3d(bitcast<vec3<u32>>(vec3<i32>(p)));
    return f32(h.x >> 8u) / 16777215.0;
}

fn value_noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let x00 = mix(hash3(i), hash3(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let x10 = mix(hash3(i + vec3<f32>(0.0, 1.0, 0.0)), hash3(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let x01 = mix(hash3(i + vec3<f32>(0.0, 0.0, 1.0)), hash3(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let x11 = mix(hash3(i + vec3<f32>(0.0, 1.0, 1.0)), hash3(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xyz;
    let rel = p - terrain.center.xyz;
    let up = normalize(rel);
    let q = terrain.rotation;
    // Position in the body's own frame (metres), so patterns stay fixed to the ground.
    let local = quat_rotate(vec4<f32>(-q.xyz, q.w), rel);
    let dir = normalize(local);
    let n = normalize(in.world_normal);
    let dist = length(p - view.world_position);
    // Inside the block ground near the camera the blocks are drawn instead.
    if terrain.hide.x > 0.0 && dot(dir, terrain.blocks.xyz) > terrain.blocks.w {
        let v = p - view.world_position;
        let cup = normalize(view.world_position - terrain.center.xyz);
        if length(v - cup * dot(v, cup)) < terrain.hide.x {
            discard;
        }
    }

        // Surface fields: interpolated from the vertices close up, from the baked map far away.
    // Biomes are decided per pixel from the interpolated fields, so boundaries (coasts, snow
    // lines, desert edges) are smooth curves instead of following the vertex grid.
    var fields = vec4<f32>(in.uv.x, in.uv.y, in.uv_b.x, in.uv_b.y);
    if terrain.params.x > 0.5 {
        let far = textureSample(field_map, field_sampler, dir);
        fields = mix(fields, far, smoothstep(terrain.params.y, terrain.params.z, dist));
    }
    let slope = acos(clamp(dot(n, up), -1.0, 1.0));
    var albedo = terrain.palette[biome_index(fields, slope, terrain.surface)].rgb;

        // Fine detail near the camera: two scales of brightness variation. Each fades out once a
    // pixel covers more than a quarter of its wavelength, or it would alias into sparkle.
    let footprint = length(fwidth(local));
    let coarse = 1.0 - smoothstep(0.25 * 6.7, 0.5 * 6.7, footprint);
    let fine = 1.0 - smoothstep(0.25 * 1.1, 0.5 * 1.1, footprint);
    if coarse > 0.0 {
        let d = (value_noise(local * 0.15) - 0.5) * 0.6 * coarse
            + (value_noise(local * 0.9) - 0.5) * 0.4 * fine;
        albedo *= 1.0 + 0.4 * d;
    }

    // Lighting: identical to the distant planet shader.
    let l = normalize(terrain.star.xyz - p);
    let ndl_sphere = dot(up, l);
    let terrain_ndl = min(dot(n, l), ndl_sphere + 0.15);
    let diffuse = clamp((terrain_ndl + 0.04) / 1.04, 0.0, 1.0);
    let light = terrain.star.w;
    let ambient = 0.002;
    return vec4<f32>(albedo * (diffuse * light + ambient), 1.0);
}
