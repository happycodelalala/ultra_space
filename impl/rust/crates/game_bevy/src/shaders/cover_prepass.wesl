// The ground cover in the depth prepass and shadows: the same dissolves as cover.wesl.

import bevy_pbr::prepass::io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex};
import bevy_pbr::render::{mesh_functions, view_transformations::position_world_to_clip};
import bevy_render::globals::Globals;

// The prepass view bind group carries the globals (time) at binding 1.
@group(0) @binding(1) var<uniform> globals: Globals;

struct CoverFade {
    fade: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> cover: CoverFade;

// The fade's progress 0..1 from its start time (fade.x, the clock that wraps every hour) over
// its duration (fade.w).
fn progress(fade: vec4<f32>) -> f32 {
    var t = globals.time - fade.x;
    if t < -1.0 {
        t += 3600.0;
    }
    return clamp(t / max(fade.w, 1.0e-3), 0.0, 1.0);
}

// A dither value in [0, 1) per pixel (interleaved gradient noise: no visible grid).
fn dither(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(floor(pixel), vec2<f32>(0.06711056, 0.00583715))));
}

// Whether a pixel is drawn. `fade`: x start time, w duration, y mode (+1 fading in, -1 fading out,
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
    let q = progress(fade);
    var share = staggered(q, r);
    if fade.y < 0.0 {
        share = staggered(q, 1.0 - r);
    }
    return shown(pixel, fade, share) && !thinned(p, pixel, fade, r);
}

fn drawn(in: VertexOutput) -> bool {
    var r = 0.5;
@if(VERTEX_UVS_A)
    r = in.uv.x;
    return cover_drawn(in.world_position.xyz, in.position.xy, cover.fade, r);
}

// Wind (design/planet-environment.md, E2): each thing bends along the prevailing wind, most at
// its top. uv.y the wind's direction in the chunk's ground plane (radians, x toward z; the
// chunk's y is up), uv_b.x where the thing lies along the wind in gust wavelengths, uv_b.y how
// far it bends at a full gust (m). Gusts roll downwind at 6 m/s, 48 m apart (GUST_WAVELENGTH in
// sim_planet's cover); each thing also sways at its own pace, and leaves flutter.
fn wind_offset(p: vec3<f32>, seed: f32, angle: f32, along: f32, bend: f32, t: f32) -> vec3<f32> {
    if bend <= 0.0 {
        return vec3<f32>(0.0);
    }
    let dir = vec3<f32>(cos(angle), 0.0, sin(angle));
    let side = vec3<f32>(-dir.z, 0.0, dir.x);
    let g = 0.5 + 0.5 * sin(6.2831853 * (along - t * 0.125));
    let gust = g * g;
    let own = sin(t * (1.2 + 0.7 * seed) + seed * 6.2831853);
    let across = cos(t * (0.9 + 0.5 * seed) + seed * 11.0);
    let flutter = sin(t * 6.5 + dot(p, vec3<f32>(2.3, 1.7, 3.1)));
    let a = bend * (0.2 + 0.6 * gust + 0.25 * own * (0.4 + gust) + 0.06 * flutter);
    let b = bend * 0.15 * across * (0.3 + gust);
    // Bending lowers the top a little (it keeps its length).
    let drop = 0.5 * (a * a + b * b) / max(bend * 6.0, 0.3);
    return dir * a + side * b - vec3<f32>(0.0, drop, 0.0);
}
// Bevy's prepass vertex shader (bevy_pbr prepass/prepass.wesl, no skinning or morphs) with the
// same wind as cover.wesl, so depth and shadows move with the things.
@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    var p = vertex.position;
    @if(VERTEX_UVS_A && VERTEX_UVS_B) {
        p += wind_offset(p, vertex.uv.x, vertex.uv.y, vertex.uv_b.x, vertex.uv_b.y, globals.time);
    }
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(p, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
    @if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
        out.unclipped_depth = out.position.z;
        out.position.z = min(out.position.z, 1.0);
    }
    @if(VERTEX_UVS_A)
    out.uv = vertex.uv;
    @if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
    @if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex_in.instance_index);
    @if(VERTEX_COLORS)
    out.color = vertex.color;
    @if(MOTION_VECTOR_PREPASS)
    out.previous_world_position = mesh_functions::mesh_position_local_to_world(
        mesh_functions::get_previous_world_from_local(vertex_in.instance_index), vec4<f32>(p, 1.0));
    @if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
    @if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_in.instance_index, world_from_local[3]);
    return out;
}

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    @if(FADING) {
        if !drawn(in) {
            discard;
        }
    }
    var out: FragmentOutput;
@if(NORMAL_PREPASS)
    out.normal = vec4(in.world_normal * 0.5 + vec3(0.5), 1.0);
    return out;
}
@else
@fragment
fn fragment(in: VertexOutput) {
    @if(FADING) {
        if !drawn(in) {
            discard;
        }
    }
}
