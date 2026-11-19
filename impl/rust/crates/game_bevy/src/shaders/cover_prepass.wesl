// The ground cover in the depth prepass and shadows: the same dissolves as cover.wesl.

import bevy_pbr::prepass::io::{VertexOutput, FragmentOutput};

struct CoverFade {
    fade: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> cover: CoverFade;

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

// Far away each thing has its own distance where it goes, so the cover thins out (rendering is
// camera-relative: the camera is at the origin).
fn thinned(p: vec3<f32>, pixel: vec2<f32>, fade: vec4<f32>, r: f32) -> bool {
    if fade.z <= 0.0 {
        return false;
    }
    let cut = fade.z * (0.82 + 0.18 * r);
    return dither(pixel) >= clamp((cut - length(p)) / (0.04 * fade.z), 0.0, 1.0);
}

// Whether a pixel of a thing with random number `r` is drawn.
fn cover_drawn(p: vec3<f32>, pixel: vec2<f32>, fade: vec4<f32>, r: f32) -> bool {
    var share = staggered(fade.x, r);
    if fade.y < 0.0 {
        share = staggered(fade.x, 1.0 - r);
    }
    return shown(pixel, fade, share) && !thinned(p, pixel, fade, r);
}

fn drawn(in: VertexOutput) -> bool {
    var r = 0.5;
@if(VERTEX_UVS_A)
    r = in.uv.x;
    return cover_drawn(in.world_position.xyz, in.position.xy, cover.fade, r);
}

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    if !drawn(in) {
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
    if !drawn(in) {
        discard;
    }
}
