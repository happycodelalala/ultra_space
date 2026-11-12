// Planet, moon and star surfaces seen from space (Batch 1, task 1.7).
// Procedural colors in the body's own frame, lit from the star's actual position.

import bevy_pbr::render::{forward_io::VertexOutput, mesh_view_bindings::view};

struct PlanetUniform {
    color_low: vec4<f32>,
    color_high: vec4<f32>,
    ocean: vec4<f32>,     // rgb color, w coverage
    glow: vec4<f32>,      // rgb color, w strength
    star: vec4<f32>,      // xyz star position (render space), w starlight brightness
    rotation: vec4<f32>,  // body rotation quaternion (xyzw)
            params: vec4<f32>,    // x kind (0 rocky, 1 gas giant, 2 star), y seed, z polar ice, w baked maps
    surface: vec4<f32>,   // x has ocean, y has atmosphere
    palette: array<vec4<f32>, 12>, // linear biome colours by biome index
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> planet: PlanetUniform;
// Baked from the terrain function: surface fields (height, temperature, moisture, continent
// signal) and surface normals, both in the body's own frame.
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var field_map: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var field_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var normal_map: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var normal_sampler: sampler;

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

// Integer hash (PCG3D). The earlier product-of-fractions hash went to ~0 along whole planes of
// cells, which showed up as long straight seams across planets.
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

// Random value in [0, 1] for an integer lattice point (given as whole-number floats).
fn hash3(p: vec3<f32>) -> f32 {
    let h = pcg3d(bitcast<vec3<u32>>(vec3<i32>(p)));
    return f32(h.x >> 8u) / 16777215.0;
}

fn value_noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let n000 = hash3(i);
    let n100 = hash3(i + vec3<f32>(1.0, 0.0, 0.0));
    let n010 = hash3(i + vec3<f32>(0.0, 1.0, 0.0));
    let n110 = hash3(i + vec3<f32>(1.0, 1.0, 0.0));
    let n001 = hash3(i + vec3<f32>(0.0, 0.0, 1.0));
    let n101 = hash3(i + vec3<f32>(1.0, 0.0, 1.0));
    let n011 = hash3(i + vec3<f32>(0.0, 1.0, 1.0));
    let n111 = hash3(i + vec3<f32>(1.0, 1.0, 1.0));
    let x00 = mix(n000, n100, u.x);
    let x10 = mix(n010, n110, u.x);
    let x01 = mix(n001, n101, u.x);
    let x11 = mix(n011, n111, u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

fn fbm(p: vec3<f32>) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var x = p;
    // Each octave is rotated so lattice edges never line up across octaves into visible lines.
    let rot = mat3x3<f32>(
        vec3<f32>(0.00, 0.80, 0.60),
        vec3<f32>(-0.80, 0.36, -0.48),
        vec3<f32>(-0.60, -0.48, 0.64),
    );
    for (var i = 0; i < 6; i++) {
        sum += amp * value_noise(x);
        x = rot * x * 2.03 + vec3<f32>(1.7, -2.3, 0.9);
        amp *= 0.5;
    }
    return sum;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let n_world = normalize(in.world_normal);
    // Surface direction in the body's own frame, so patterns turn with the ground.
    let q = planet.rotation;
    let n_local = quat_rotate(vec4<f32>(-q.xyz, q.w), n_world);
    let seed = planet.params.y;
    let p = n_local * 2.2 + vec3<f32>(seed, seed * 0.37, -seed * 0.71);
    let kind = planet.params.x;

    if kind > 1.5 {
        // Star: bright with limb darkening.
        let v = normalize(view.world_position - in.world_position.xyz);
        let mu = clamp(dot(n_world, v), 0.0, 1.0);
        let granules = 0.92 + 0.08 * fbm(p * 6.0);
        let limb = 0.35 + 0.65 * pow(mu, 0.5);
        return vec4<f32>(planet.glow.rgb * planet.glow.w * limb * granules, 1.0);
    }

        var albedo: vec3<f32>;
    var emission = vec3<f32>(0.0);
    var shininess = 0.0;
    // Normal used for diffuse light; baked maps replace it with the terrain's.
    var n_light = n_world;

    if kind < 0.5 && planet.params.w > 0.5 {
                // Rocky body with baked maps: the same terrain and biome rules as the close-up tiles.
        let fields = textureSample(field_map, field_sampler, n_local);
        let n_map = normalize(textureSample(normal_map, normal_sampler, n_local).xyz * 2.0 - 1.0);
        let slope = acos(clamp(dot(n_map, n_local), -1.0, 1.0));
        let biome = biome_index(fields, slope, planet.surface);
        albedo = planet.palette[biome].rgb;
        if biome == 0u {
            shininess = 1.0;
        }
        n_light = normalize(quat_rotate(q, n_map));
    } else if kind < 0.5 {
        // Rocky: terrain colors, optional oceans, ice caps and lava glow.
        let h = fbm(p);
        let detail = fbm(p * 5.0);
        albedo = mix(planet.color_low.rgb, planet.color_high.rgb, smoothstep(0.3, 0.7, h + 0.2 * (detail - 0.5)));
        let coverage = planet.ocean.w;
        if coverage > 0.0 && h < 0.3 + coverage * 0.32 {
            albedo = planet.ocean.rgb;
            shininess = 1.0;
        }
        let ice = planet.params.z;
        if ice > 0.0 {
            let lat = asin(clamp(abs(n_local.y), 0.0, 1.0)) / 1.5707963;
            if lat + 0.08 * (detail - 0.5) > 1.0 - ice {
                albedo = vec3<f32>(0.9, 0.93, 0.96);
                shininess = 0.2;
            }
        }
        if planet.glow.w > 0.0 {
            emission = planet.glow.rgb * planet.glow.w * smoothstep(0.42, 0.3, h) * (0.6 + 0.4 * detail);
        }
    } else {
        // Gas giant: latitude bands warped by turbulence.
        let warp = (fbm(p * 1.5) - 0.5) * 0.25;
        let bands = sin((n_local.y + warp) * 22.0 + seed) * 0.5 + 0.5;
        let streaks = fbm(vec3<f32>(p.x * 0.6, n_local.y * 40.0, p.z * 0.6));
        albedo = mix(planet.color_low.rgb, planet.color_high.rgb, clamp(bands * 0.75 + streaks * 0.35, 0.0, 1.0));
    }

    // Lighting from the star's actual position.
    let to_star = planet.star.xyz - in.world_position.xyz;
    let l = normalize(to_star);
        let ndl = dot(n_world, l);
    // Terrain slopes shade by their own normal, but never light up the night side.
    let terrain_ndl = min(dot(n_light, l), ndl + 0.15);
    let diffuse = clamp((terrain_ndl + 0.04) / 1.04, 0.0, 1.0);
    let v = normalize(view.world_position - in.world_position.xyz);
    let h = normalize(l + v);
    let spec = shininess * pow(max(dot(n_world, h), 0.0), 80.0) * 0.6 * step(0.0, ndl);
    let light = planet.star.w;
    let ambient = 0.002;
    let color = albedo * (diffuse * light + ambient) + spec * light + emission;
    return vec4<f32>(color, 1.0);
}
