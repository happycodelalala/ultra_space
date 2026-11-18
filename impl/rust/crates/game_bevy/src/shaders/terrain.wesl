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
    surface: vec4<f32>,   // x has ocean, y has atmosphere, z cover mode (a body with geography), w faceted ground
    palette: array<vec4<f32>, 12>, // linear biome colours by biome index
    blocks: vec4<f32>,    // xyz block ground centre (body frame, unit), w cosine of its angular radius
    hide: vec4<f32>,      // x: tiles this close (horizontally) to the camera inside the block ground are hidden
    planes: array<vec4<f32>, 32>, // a body of plains: each plain's plane (body frame)
    plain_count: vec4<f32>, // x number of planes (0: a sphere)
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

// COVER BEGIN
// Ground colours on a body with geography (planet-geography.md §1b): what grows there from the
// temperature and rain (as sim_planet::globe's land_cover) and the block ground's colour picks
// (sim_planet::blocks top_colour), so the far ground matches the blocks. Palette (cover mode):
// 0 sea floor, 1 turf, 2 turf2, 3 turf3, 4 moss, 5 stone, 6 stone2, 7 straw, 8 straw2, 9 white,
// 10 rock, 11 path2. fields: x height (m), y temperature (°C), z wetness (rain / 1.8 m).
// n: the colour jitter (about ±0.5; 0 far away).
fn pick3(n: f32, a: u32, b: u32, c: u32) -> u32 {
    if n < -0.12 { return a; }
    if n < 0.1 { return b; }
    return c;
}

fn cover_index(f: vec4<f32>, slope: f32, n: f32) -> u32 {
    let h = f.x;
    if h < 0.0 { return 0u; }
    if slope > 1.15 { return 10u; }
    if h < 1.5 {
        if n < 0.0 { return 7u; }
        return 8u;
    }
    let t = f.y + 2.0 * n;
    let p = f.z * 1800.0 * (1.0 + 0.25 * n);
    if t < -8.0 { return 9u; }
    if t < -3.0 && h > 1500.0 { return 9u; }
    if t < 0.0 { return pick3(n, 4u, 5u, 4u); }
    if t < 7.0 {
        if p < 300.0 { return pick3(n, 5u, 6u, 11u); }
        return pick3(n, 4u, 3u, 4u);
    }
    if t < 20.0 {
        if p < 250.0 { return pick3(n, 7u, 8u, 7u); }
        if p < 550.0 { return pick3(n, 8u, 2u, 7u); }
        if p < 900.0 { return pick3(n, 3u, 1u, 2u); }
        if p < 1700.0 { return pick3(n, 3u, 1u, 3u); }
        return pick3(n, 4u, 3u, 4u);
    }
    if p < 250.0 { return pick3(n, 7u, 8u, 7u); }
    if p < 900.0 { return pick3(n, 8u, 2u, 7u); }
    if p < 1800.0 { return pick3(n, 3u, 1u, 3u); }
    return pick3(n, 4u, 3u, 4u);
}
// COVER END


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
    var n = normalize(in.world_normal);
    if terrain.surface.w > 0.5 {
        // Faceted ground: each triangle flat, its normal from the screen-space derivatives.
        let flat = normalize(cross(dpdx(p), dpdy(p)));
        let oriented = select(-flat, flat, dot(flat, normalize(p - terrain.center.xyz)) > 0.0);
        // The tiles' skirts (near-vertical walls hanging from their edges) keep the smooth
        // ground normal, so where one shows it reads as ground, not as a dark line.
        if dot(oriented, normalize(p - terrain.center.xyz)) > 0.25 {
            n = oriented;
        }
    }
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
    // "Up" for slopes: the plain's normal on a body of plains (the nearest plane along the ray
    // from the centre), else away from the centre.
    var ground_up = up;
    let count = i32(terrain.plain_count.x);
    if count > 0 {
        var best = 1.0e30;
        var normal = vec3<f32>(0.0, 1.0, 0.0);
        for (var i = 0; i < count; i++) {
            let c = dot(dir, terrain.planes[i].xyz);
            if c > 1.0e-4 {
                let r = terrain.planes[i].w / c;
                if r < best {
                    best = r;
                    normal = terrain.planes[i].xyz;
                }
            }
        }
        ground_up = quat_rotate(q, normal);
    }
    let slope = acos(clamp(dot(n, ground_up), -1.0, 1.0));
    var albedo: vec3<f32>;
    if terrain.surface.z > 0.5 {
        // The block ground's colours, jittered like its tops (fading out where a pixel covers
        // more than the pattern), and its 50 cm steps drawn as dark risers along the contours.
        let footprint = length(fwidth(local));
        let fade = 1.0 - smoothstep(0.8, 1.6, footprint);
        let speck = 1.0 - smoothstep(0.06, 0.12, footprint);
        let jitter = ((value_noise(local * 0.3) - 0.5) * 0.8
            + 0.15 * (hash3(floor(local * 2.0)) - 0.5) * speck) * fade;
        albedo = terrain.palette[cover_index(fields, slope, jitter)].rgb;
        if fields.x > 0.0 && terrain.surface.w < 0.5 {
            // How much of this pixel the risers cover, worked out exactly: a riser line where
            // steps are wider than the pixel, their average share where several fall in it.
            let s = fields.x / 0.5;
            let w = fwidth(s);
            let riser = 1.0 - smoothstep(0.0, max(w * 1.5, 0.001), fract(s));
            let cover = mix(riser, 0.22, smoothstep(0.4, 1.2, w));
            // Risers face downhill: lit by the sun like the sides of real blocks.
            let tilt = n - ground_up * dot(n, ground_up);
            let downhill = select(ground_up, normalize(tilt), length(tilt) > 1.0e-4);
            let sun = normalize(terrain.star.xyz - p);
            let riser_light = mix(0.5, 1.15, clamp(dot(downhill, sun) * 0.5 + 0.5, 0.0, 1.0));
            albedo = mix(albedo, albedo * riser_light * 0.85, cover);
        }
    } else {
        albedo = terrain.palette[biome_index(fields, slope, terrain.surface)].rgb;
    }

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
