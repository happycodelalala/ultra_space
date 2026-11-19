// The faceted ground in the depth prepass: the same discard as ground.wesl beyond the circle the
// chunks share with the far tiles, so chunks there don't write depth either (else the tiles
// beyond fail the main pass's depth test and nothing is drawn there: a dark band).

import bevy_pbr::prepass::io::{VertexOutput, FragmentOutput};

struct GroundDetail {
    strength: vec4<f32>,
    centre: vec4<f32>,
    fade: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> detail: GroundDetail;

// A dither value in [0, 1) per pixel (interleaved gradient noise: no visible grid).
fn dither(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(floor(pixel), vec2<f32>(0.06711056, 0.00583715))));
}

// Whether a pixel is drawn. `fade`: x progress 0..1, y mode (+1 fading in, -1 fading out,
// 0 steady); `share` how much of the fade this pixel's thing has reached (the progress for the
// ground; staggered per thing for the cover). Fading out uses the mirrored pattern, so an old
// chunk and its replacements started together never both draw a pixel.
fn shown(pixel: vec2<f32>, fade: vec4<f32>, share: f32) -> bool {
    if fade.y == 0.0 {
        return true;
    }
    let d = dither(pixel);
    if fade.y > 0.0 {
        return d < share;
    }
    return 1.0 - d > share;
}

// A thing's own progress: things with a higher random number `r` start later, so a stand of
// trees grows in tree by tree.
fn staggered(progress: f32, r: f32) -> f32 {
    return clamp(progress * 1.6 - r * 0.6, 0.0, 1.0);
}

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
    if outside(in.world_position.xyz) || !shown(in.position.xy, detail.fade, detail.fade.x) {
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
    if outside(in.world_position.xyz) || !shown(in.position.xy, detail.fade, detail.fade.x) {
        discard;
    }
}
