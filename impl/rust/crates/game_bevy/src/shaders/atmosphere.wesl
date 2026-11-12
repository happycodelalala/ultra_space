// Atmosphere seen from space (Batch 1, task 1.7): additive glow on a shell around the planet.
// Brightness follows the air density along each sight line: it peaks where the line passes
// lowest over the surface and fades exponentially with altitude. A cheap stand-in until real
// scattering (Batch 2).

import bevy_pbr::render::{forward_io::VertexOutput, mesh_view_bindings::view};

struct AtmosphereUniform {
    color: vec4<f32>,   // rgb color, w strength
    star: vec4<f32>,    // xyz star position (render space), w starlight brightness
    center: vec4<f32>,  // xyz planet center (render space), w planet radius
    shell: vec4<f32>,   // x atmosphere top radius, y scale height
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> atmosphere: AtmosphereUniform;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let cam = view.world_position;
    let c = atmosphere.center.xyz;
    let radius = atmosphere.center.w;
    let scale_height = atmosphere.shell.y;
    let d = normalize(in.world_position.xyz - cam);

    // Closest approach of the sight line to the planet's center.
    let t_closest = max(dot(c - cam, d), 0.0);
    let closest = cam + d * t_closest;
    let altitude = length(closest - c) - radius;

    var density: f32;
    var sample_point: vec3<f32>;
    if altitude < 0.0 {
        // Looking at the disc: a thin haze, thicker toward the edge.
        let to_surface = normalize(in.world_position.xyz - c);
        let grazing = 1.0 - clamp(dot(to_surface, -d), 0.0, 1.0);
        density = 0.12 + 0.5 * pow(grazing, 3.0);
        sample_point = in.world_position.xyz;
    } else {
        // Above the limb: bright near the surface, fading with height.
        density = exp(-altitude / scale_height);
        sample_point = closest;
    }

    let n = normalize(sample_point - c);
    let l = normalize(atmosphere.star.xyz - sample_point);
    let day = smoothstep(-0.25, 0.35, dot(n, l));
    let glow = atmosphere.color.rgb * density * day * atmosphere.color.w * atmosphere.star.w;
    // Bevy draws AlphaMode::Add with premultiplied blending (src + (1 - src.a) * dst), so
    // alpha must be 0 for a purely additive result.
    return vec4<f32>(glow, 0.0);
}
