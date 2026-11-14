// Surface detail for part models, drawn at real sizes so it scales from a frame to a capital
// ship: on painted armour, plate seams in staggered rows (and heavier structural seams every few
// plates), fastener rows beside the seams, a slight tint and sheen difference plate to plate,
// and wear (paint lighter at the seams' edges, grime low down). The model's first UV set is the
// surface position in metres along its plate (written by tools/blender), so seams follow each
// plate's own edges. Seams stay at least a pixel wide (fainter when thinner than that) and fade once the
// plates are only a few pixels across.
// Keep in sync with the viewer's copy (impl/viewer/hull_detail.mjs).

import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
};

struct HullDetail {
    // x plate height (m), y plate width / height, z structural seam every this many plates,
    // w strength (0 none, 1 painted armour).
    plate: vec4<f32>,
    // x seam width (m), y fastener spacing (m), z grime strength, w unused.
    seam: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> detail: HullDetail;

fn hash2(p: vec2<f32>) -> f32 {
    var q = bitcast<vec2<u32>>(vec2<i32>(floor(p)));
    q = q * vec2<u32>(1664525u, 1013904223u) + vec2<u32>(1013904223u, 1664525u);
    var h = q.x ^ (q.y * 747796405u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = h ^ (h >> 13u);
    return f32(h & 0xffffu) / 65535.0;
}

// Distance (m) to the nearest seam of a staggered grid of cells `size` (m), the cell's id.
fn seams(uv: vec2<f32>, size: vec2<f32>) -> vec3<f32> {
    let row = floor(uv.y / size.y);
    let shifted = vec2(uv.x + select(0.0, 0.5 * size.x, (row - 2.0 * floor(row * 0.5)) > 0.5), uv.y);
    let cell = floor(shifted / size);
    let f = shifted / size - cell;
    let d = min(min(f.x, 1.0 - f.x) * size.x, min(f.y, 1.0 - f.y) * size.y);
    return vec3(d, cell);
}

@fragment
fn fragment(in_: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var in = in_;
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );

@if(VERTEX_UVS_A) {
    let strength = detail.plate.w;
    let uv = in.uv;
    // Metres per pixel here: detail smaller than about two pixels fades out.
    let footprint = max(length(fwidth(uv)), 1e-5);
    let h = detail.plate.x;
    let small = seams(uv, vec2(h * detail.plate.y, h));
    let big = seams(uv + vec2(0.37, 0.61) * h, vec2(h * detail.plate.y, h) * detail.plate.z);
    let w = detail.seam.x;
    // Seams stay at least a pixel wide, fainter as they get thinner than that, until the
    // plates themselves are only a few pixels (then they would only flicker).
    let fade_small = clamp(w / footprint, 0.25, 1.0) * (1.0 - smoothstep(h * 0.04, h * 0.12, footprint));
    let fade_big = clamp(2.0 * w / footprint, 0.5, 1.0) * (1.0 - smoothstep(h * 0.2, h * 0.6, footprint));
    let lw = max(w, footprint);
    let lw_big = max(2.0 * w, footprint);
    let groove = (1.0 - smoothstep(lw * 0.5, lw + footprint * 0.5, small.x)) * fade_small
        + (1.0 - smoothstep(lw_big * 0.5, lw_big + footprint * 0.5, big.x)) * fade_big;
    let edge = (smoothstep(w, w + footprint, small.x) - smoothstep(w * 2.5, w * 2.5 + footprint, small.x))
        * fade_small;
    // Fasteners: a row of heads just inside each plate edge.
    let s = detail.seam.y;
    let along = abs(fract(uv / s) - 0.5) * s;
    let inset = abs(small.x - w * 3.0);
    let head = (1.0 - smoothstep(w * 0.6, w * 0.6 + footprint, length(vec2(inset, min(along.x, along.y)))))
        * clamp(1.0 - footprint / (w * 0.8), 0.0, 1.0);
    // Plate to plate: tint and sheen.
    let r = hash2(small.yz + vec2(17.0, 3.0) * floor(big.yz));
    let tint = 1.0 + (r - 0.5) * 0.14 + (hash2(big.yz) - 0.5) * 0.08;
    // Grime: darker low on each big plate.
    let grime = detail.seam.z * smoothstep(0.6, 0.0, fract(uv.y / (h * detail.plate.z))) * 0.12;
    var color = pbr_input.material.base_color.rgb * tint * (1.0 - grime);
    color = mix(color, color * 0.25, clamp(groove, 0.0, 1.0) * 0.85);
    color = mix(color, color * 1.35 + 0.02, edge * 0.5);
    color = mix(color, color * 0.55, head * 0.7);
    pbr_input.material.base_color = vec4(mix(pbr_input.material.base_color.rgb, color, strength),
        pbr_input.material.base_color.a);
    pbr_input.material.perceptual_roughness = clamp(pbr_input.material.perceptual_roughness
        + strength * ((r - 0.5) * 0.16 + groove * 0.25 - edge * 0.15), 0.05, 1.0);
}

    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
