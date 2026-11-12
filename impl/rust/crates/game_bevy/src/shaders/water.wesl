// Sea surface on terrain tiles (Batch 2, task 2.5).
// Lit like the planet shader's oceans: diffuse ocean colour, sun glints from small waves, and a
// fresnel reflection of the sky towards grazing angles.

import bevy_pbr::render::{forward_io::VertexOutput, mesh_view_bindings::view};

struct WaterUniform {
    star: vec4<f32>,      // xyz star position (render space), w starlight brightness
    center: vec4<f32>,    // xyz body centre (render space), w radius
    rotation: vec4<f32>,  // body rotation quaternion (xyzw)
    color: vec4<f32>,     // rgb ocean colour (linear), w time (s)
    sky: vec4<f32>,       // rgb sky colour reflected at grazing angles (linear)
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> water: WaterUniform;

fn quat_rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

// Small wave slopes from a few moving sine waves (cheap, no seams). Each wave fades out once a
// pixel covers more than a quarter of its wavelength (`footprint`, metres per pixel), or the
// highlights would alias into sparkle.
fn wave_slope(p: vec3<f32>, time: f32, footprint: f32) -> vec3<f32> {
    let w1 = 1.0 - smoothstep(0.25 * 50.0, 0.5 * 50.0, footprint);
    let w2 = 1.0 - smoothstep(0.25 * 20.0, 0.5 * 20.0, footprint);
    let w3 = 1.0 - smoothstep(0.25 * 7.5, 0.5 * 7.5, footprint);
    var s = vec3<f32>(0.0);
    s += vec3<f32>(cos(p.x * 0.11 + time * 0.9), 0.0, cos(p.z * 0.13 + time * 1.1)) * 0.03 * w1;
    s += vec3<f32>(cos((p.x + p.z) * 0.31 + time * 1.7), 0.0, cos((p.z - p.y) * 0.27 + time * 1.3)) * 0.02 * w2;
    s += vec3<f32>(cos((p.y - p.x) * 0.83 + time * 2.3), 0.0, cos((p.x + p.y) * 0.71 + time * 2.9)) * 0.012 * w3;
    return s;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xyz;
    let rel = p - water.center.xyz;
    let up = normalize(rel);
    let q = water.rotation;
    let local = quat_rotate(vec4<f32>(-q.xyz, q.w), rel);
    let to_eye = view.world_position - p;
    let dist = length(to_eye);
    let v = to_eye / dist;

        let footprint = length(fwidth(local));
    let slope = quat_rotate(q, wave_slope(local, water.color.w, footprint));
    let n = normalize(up - (slope - up * dot(slope, up)));

    let l = normalize(water.star.xyz - p);
    let ndl_sphere = dot(up, l);
    let diffuse = clamp((ndl_sphere + 0.04) / 1.04, 0.0, 1.0);
    let light = water.star.w;
    let h = normalize(l + v);
    let spec = pow(max(dot(n, h), 0.0), 120.0) * 0.9 * step(0.0, ndl_sphere);
    let fresnel = 0.02 + 0.98 * pow(1.0 - max(dot(n, v), 0.0), 5.0);
    let base = water.color.rgb * (diffuse * light + 0.002);
    let reflected = water.sky.rgb * light * diffuse;
    let color = mix(base, reflected, fresnel) + spec * light;
    return vec4<f32>(color, 1.0);
}
