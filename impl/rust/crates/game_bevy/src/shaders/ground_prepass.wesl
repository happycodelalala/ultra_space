// The faceted ground in the depth prepass: the same discard as ground.wesl beyond the circle the
// chunks share with the far tiles, so chunks there don't write depth either (else the tiles
// beyond fail the main pass's depth test and nothing is drawn there: a dark band).

import bevy_pbr::prepass::io::{VertexOutput, FragmentOutput};

struct GroundDetail {
    strength: vec4<f32>,
    centre: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> detail: GroundDetail;

// Rendering is camera-relative: the camera is at the origin.
fn outside(p: vec3<f32>) -> bool {
    if detail.strength.y <= 0.0 {
        return false;
    }
    let up = normalize(-detail.centre.xyz);
    return length(p - up * dot(p, up)) > detail.strength.y;
}

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    if outside(in.world_position.xyz) {
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
    if outside(in.world_position.xyz) {
        discard;
    }
}
