// The faceted ground's detail (block-world.md, the density field): Bevy's standard lighting and
// shadows over each facet's flat colour, with a procedural pattern by what the ground is made of
// (grass streaks, rock bands, sand ripples, earth clumps, soft snow). Patterns run in the face
// grid's metres (uv_b) and the height (uv.y), so they stay fixed to the ground and continue
// across chunks; each fades out once it is smaller than a pixel, so it never adds aliasing.
// uv.x is the kind: 0 grass, 1 rock, 2 sand and paths, 3 earth, 4 snow.

import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct GroundDetail {
    // x: pattern strength (0 none, 1 default)
    strength: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> detail: GroundDetail;

fn hash2(p: vec2<f32>) -> f32 {
    let q = vec2<u32>(bitcast<u32>(i32(floor(p.x))), bitcast<u32>(i32(floor(p.y))));
    var h = q.x * 1664525u + q.y * 1013904223u;
    h ^= h >> 16u;
    h *= 2246822519u;
    h ^= h >> 13u;
    return f32(h & 0xffffffu) / 16777215.0;
}

fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// How visible a pattern of wavelength `w` metres is when a pixel covers `fp` metres.
fn keep(w: f32, fp: f32) -> f32 {
    return 1.0 - smoothstep(0.25 * w, 0.6 * w, fp);
}

// Brightness factor of the pattern for `kind` at grid point `g` (m) and height `h` (m).
fn pattern(kind: f32, g: vec2<f32>, h: f32, fp: f32) -> f32 {
    if kind < 0.5 {
        // Grass: fine streaks along a wandering direction, and darker tufts.
        let a = noise2(g * 0.05) * 6.283;
        let dir = vec2<f32>(cos(a), sin(a));
        let across = dot(g, vec2<f32>(-dir.y, dir.x));
        let along = dot(g, dir);
        let streak = noise2(vec2<f32>(across * 9.0, along * 1.2)) - 0.5;
        let tuft = smoothstep(0.62, 0.8, noise2(g * 2.3));
        return 1.0 + 0.32 * streak * keep(0.12, fp) - 0.16 * tuft * keep(0.45, fp);
    }
    if kind < 1.5 {
        // Rock: bands along the height, wavering, with cracks.
        let band = sin((h + 0.6 * noise2(g * 0.4)) * 7.0) * 0.5 + 0.5;
        let crack = smoothstep(0.92, 0.98, noise2(g * 3.1 + vec2<f32>(h * 2.0, 0.0)));
        let grain = noise2(g * 6.0) - 0.5;
        return 1.0 + 0.16 * (band - 0.5) * keep(0.9, fp) - 0.25 * crack * keep(0.35, fp)
            + 0.12 * grain * keep(0.17, fp);
    }
    if kind < 2.5 {
        // Sand and paths: soft ripples and pebbles.
        let warp = noise2(g * 0.3) * 3.0;
        let ripple = sin(g.x * 4.5 + g.y * 1.3 + warp * 4.0) * 0.5;
        let pebble = smoothstep(0.85, 0.95, noise2(g * 5.0));
        return 1.0 + 0.1 * ripple * keep(1.4, fp) - 0.15 * pebble * keep(0.2, fp);
    }
    if kind < 3.5 {
        // Earth: clumps.
        let clump = noise2(g * 2.0 + vec2<f32>(h, h)) - 0.5;
        let fine = noise2(g * 7.0) - 0.5;
        return 1.0 + 0.25 * clump * keep(0.5, fp) + 0.12 * fine * keep(0.14, fp);
    }
    // Snow: broad, soft drifts.
    return 1.0 + 0.06 * (noise2(g * 0.25) - 0.5) * keep(4.0, fp);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let g = in.uv_b;
    // Metres per pixel on the ground.
    let fp = max(length(fwidth(g)), 1.0e-4);
    let k = mix(1.0, pattern(in.uv.x, g, in.uv.y, fp), detail.strength.x);
    pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * k, pbr_input.material.base_color.a);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
