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
    rock: vec4<f32>,      // rock colour (linear rgb)
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> terrain: TerrainUniform;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var color_map: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var color_sampler: sampler;

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

    // Colour: vertex biomes close up, the baked map far away.
    var albedo = in.color.rgb;
    if terrain.params.x > 0.5 {
        let far = textureSample(color_map, color_sampler, dir).rgb;
        albedo = mix(albedo, far, smoothstep(terrain.params.y, terrain.params.z, dist));
    }

    // Steep slopes are bare rock (the mesh normal is sharper than the vertex biomes).
    let near = 1.0 - smoothstep(terrain.params.y, terrain.params.z, dist);
    let steepness = 1.0 - dot(n, up);
    albedo = mix(albedo, terrain.rock.rgb, smoothstep(0.22, 0.35, steepness) * near);

    // Fine detail near the camera: two scales of brightness variation, fading with distance.
    let fade = 1.0 - smoothstep(0.0, terrain.params.w, dist);
    if fade > 0.0 {
        let d = value_noise(local * 0.15) * 0.6 + value_noise(local * 0.9) * 0.4;
        albedo *= mix(1.0, 0.8 + 0.4 * d, fade);
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
