// Terrain tiles in the depth prepass (Batch 5): the same discard as terrain.wesl, so tiles hidden
// over the block ground don't write depth either (else the blocks under them fail the main pass's
// depth test and nothing is drawn there).

import bevy_pbr::prepass::io::{VertexOutput, FragmentOutput};

struct TerrainUniform {
    star: vec4<f32>,
    center: vec4<f32>,
    rotation: vec4<f32>,
    params: vec4<f32>,
    surface: vec4<f32>,
    palette: array<vec4<f32>, 12>,
    blocks: vec4<f32>,
    hide: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> terrain: TerrainUniform;

fn quat_rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

// Rendering is camera-relative: the camera is at the origin.
fn hidden(p: vec3<f32>) -> bool {
    if terrain.hide.x <= 0.0 {
        return false;
    }
    let rel = p - terrain.center.xyz;
    let q = terrain.rotation;
    let dir = normalize(quat_rotate(vec4<f32>(-q.xyz, q.w), rel));
    if dot(dir, terrain.blocks.xyz) <= terrain.blocks.w {
        return false;
    }
    let cup = normalize(-terrain.center.xyz);
    return length(p - cup * dot(p, cup)) < terrain.hide.x;
}

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    if hidden(in.world_position.xyz) {
        discard;
    }
    var out: FragmentOutput;
@if(NORMAL_PREPASS)
    out.normal = vec4(in.world_normal * 0.5 + vec3(0.5), 1.0);
    return out;
}
@else
@fragment
fn fragment(in: VertexOutput) {
    if hidden(in.world_position.xyz) {
        discard;
    }
}
