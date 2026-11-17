// Cotton clouds (planet-geography.md §3.7): soft puffs with bright sunlit tops, blue-grey
// undersides, a silver rim against the sun and soft, slightly see-through edges; dark at night;
// faded out round the camera when flying through.

import bevy_pbr::render::{forward_io::VertexOutput, mesh_view_bindings::view};

struct CloudUniform {
    star: vec4<f32>,      // xyz star position (render space), w starlight brightness
    center: vec4<f32>,    // xyz body centre (render space)
    sky: vec4<f32>,       // rgb sky light (linear) for the shaded sides
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> cloud: CloudUniform;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xyz;
    let n = normalize(in.world_normal);
    let to_eye = view.world_position - p;
    let dist = length(to_eye);
    let v = to_eye / dist;
    let l = normalize(cloud.star.xyz - p);
    let up = normalize(p - cloud.center.xyz);
    // Daylight at the cloud: the sun's height over it (golden and dim near sunset).
    let sun_up = dot(up, l);
    let day = clamp(sun_up * 4.0 + 0.25, 0.02, 1.0);
    let warm = mix(vec3<f32>(1.0, 0.62, 0.4), vec3<f32>(1.0, 0.98, 0.95), smoothstep(0.0, 0.35, sun_up));
    // Wrapped diffuse: light reaches round the puff, never black.
    let wrap = clamp(dot(n, l) * 0.5 + 0.5, 0.0, 1.0);
    // uv.x: 0 at the cloud's base, 1 at its top.
    let height = clamp(in.uv.x, 0.0, 1.0);
    let lit = clamp(0.25 + 0.55 * wrap + 0.35 * height, 0.0, 1.0);
    let top = warm;
    let bottom = vec3<f32>(0.5, 0.57, 0.7);
    var colour = mix(bottom, top, lit) * cloud.star.w * day * (0.35 + 0.75 * wrap);
    colour += cloud.sky.rgb * day * 0.3;
    // Silver lining: the edge seen against the sun.
    let rim = pow(1.0 - max(dot(n, v), 0.0), 3.0) * pow(max(dot(-v, l), 0.0), 4.0);
    colour += warm * cloud.star.w * day * rim * 1.5;
    // Soft edges, and fading out round the camera.
    var alpha = smoothstep(0.0, 0.55, dot(n, v)) * 0.92;
    alpha *= smoothstep(150.0, 700.0, dist);
    return vec4<f32>(colour * alpha, alpha);
}
