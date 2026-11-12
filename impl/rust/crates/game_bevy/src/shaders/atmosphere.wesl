// Atmosphere (Batch 2, task 2.6): single scattering (Rayleigh + Mie), raymarched per pixel.
//
// One shader for every view: the blue rim and hazy disc from orbit, the sky, sunsets and night
// from the ground, and haze over distant terrain. The shell is drawn without back-face culling;
// from outside only its front faces shade, from inside only its back faces, so each pixel is
// computed once. Rays stop at the scene depth (depth prepass), so haze covers only the air in
// front of terrain, ships and the planet. Blending is premultiplied: scattered light is added
// and the background is dimmed by the transmittance.

import bevy_pbr::render::{forward_io::VertexOutput, mesh_view_bindings::view};
import bevy_pbr::prepass::utils::prepass_depth;

struct AtmosphereUniform {
    star: vec4<f32>,      // xyz star position (render space), w starlight brightness
    center: vec4<f32>,    // xyz planet centre (render space), w planet radius (sea level)
    shell: vec4<f32>,     // x top radius, y Rayleigh scale height, z Mie scale height, w Mie g
    scatter: vec4<f32>,   // rgb Rayleigh coefficients at sea level (1/m), w Mie coefficient
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> atmosphere: AtmosphereUniform;

const PI: f32 = 3.14159265;
const VIEW_STEPS: i32 = 16;
const LIGHT_STEPS: i32 = 6;
// Mie extinction is scattering plus a little absorption.
const MIE_EXTINCTION: f32 = 1.1;
// Single scattering underestimates skylight; real skies get a lot from multiple scattering.
const MULTIPLE_SCATTERING: f32 = 3.0;
// Bright sky hides what's behind it (stars by day), as eyes and cameras adapt to it: this much
// coverage per unit of sky luminance.
const SKY_HIDES: f32 = 12.0;

// Distances along `d` (unit) from `o` where it enters and leaves the sphere (c, r); x > y
// means no hit.
fn ray_sphere(o: vec3<f32>, d: vec3<f32>, c: vec3<f32>, r: f32) -> vec2<f32> {
    let oc = o - c;
    let b = dot(oc, d);
    let q = dot(oc, oc) - r * r;
    let disc = b * b - q;
    if disc < 0.0 {
        return vec2<f32>(1.0, -1.0);
    }
    let s = sqrt(disc);
    return vec2<f32>(-b - s, -b + s);
}

fn density(p: vec3<f32>) -> vec2<f32> {
    let h = max(length(p - atmosphere.center.xyz) - atmosphere.center.w, 0.0);
    return vec2<f32>(exp(-h / atmosphere.shell.y), exp(-h / atmosphere.shell.z));
}

// Optical depth (Rayleigh, Mie densities integrated) from `p` towards the star; a large value
// when the planet blocks the star.
fn light_depth(p: vec3<f32>, l: vec3<f32>) -> vec2<f32> {
    let c = atmosphere.center.xyz;
    let ground = ray_sphere(p, l, c, atmosphere.center.w);
    if ground.x < ground.y && ground.x > 0.0 {
        return vec2<f32>(1.0e9);
    }
    let out = ray_sphere(p, l, c, atmosphere.shell.x).y;
    let ds = max(out, 0.0) / f32(LIGHT_STEPS);
    var depth = vec2<f32>(0.0);
    for (var i = 0; i < LIGHT_STEPS; i++) {
        depth += density(p + l * ((f32(i) + 0.5) * ds)) * ds;
    }
    return depth;
}

// Distance from the camera to the nearest opaque surface at this pixel (prepass depth), or a
// huge value where only sky is behind.
fn scene_distance(frag: vec4<f32>) -> f32 {
    var distance = 1.0e30;
    @if(DEPTH_PREPASS) {
        let depth = prepass_depth(frag, 0u);
        if depth > 0.0 {
            let uv = (frag.xy - view.viewport.xy) / view.viewport.zw;
            let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
            let p = view.view_from_clip * ndc;
            distance = length(p.xyz / p.w);
        }
    }
    return distance;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let cam = view.world_position;
    let c = atmosphere.center.xyz;
    let top = atmosphere.shell.x;
    let inside = length(cam - c) < top;
    // Outside: shade front faces only; inside: back faces only (each pixel once).
    if inside == front {
        discard;
    }
    let d = normalize(in.world_position.xyz - cam);
    let shell = ray_sphere(cam, d, c, top);
    var t0 = max(shell.x, 0.0);
    var t1 = shell.y;
        let ground = ray_sphere(cam, d, c, atmosphere.center.w);
    let hits_ground = ground.x < ground.y && ground.x > 0.0;
    if hits_ground {
        t1 = min(t1, ground.x);
    }
    let scene = scene_distance(in.position);
    // Only empty sky (stars) is hidden by bright skylight; surfaces are just dimmed by the air.
    let sky_behind = !hits_ground && scene > t1;
    t1 = min(t1, scene);
    if t1 <= t0 {
        discard;
    }

    let l = normalize(atmosphere.star.xyz - (cam + d * (0.5 * (t0 + t1))));
    let beta_r = atmosphere.scatter.rgb;
    let beta_m = atmosphere.scatter.w;
    let ds = (t1 - t0) / f32(VIEW_STEPS);
    var view_depth = vec2<f32>(0.0);
    var sum_r = vec3<f32>(0.0);
    var sum_m = vec3<f32>(0.0);
    for (var i = 0; i < VIEW_STEPS; i++) {
        let p = cam + d * (t0 + (f32(i) + 0.5) * ds);
        let rho = density(p) * ds;
        view_depth += rho;
        let to_light = light_depth(p, l);
        let tau = beta_r * (view_depth.x + to_light.x)
            + beta_m * MIE_EXTINCTION * (view_depth.y + to_light.y);
        let t = exp(-tau);
        sum_r += t * rho.x;
        sum_m += t * rho.y;
    }

    let mu = dot(d, l);
    let phase_r = 3.0 / (16.0 * PI) * (1.0 + mu * mu);
    let g = atmosphere.shell.w;
    let phase_m = 3.0 / (8.0 * PI) * ((1.0 - g * g) * (1.0 + mu * mu))
        / ((2.0 + g * g) * pow(1.0 + g * g - 2.0 * g * mu, 1.5));
    // Starlight irradiance in shader units: a white surface facing the star shows `star.w`.
    let irradiance = atmosphere.star.w * PI;
        let light = MULTIPLE_SCATTERING * irradiance
        * (sum_r * beta_r * phase_r + sum_m * beta_m * phase_m);

    let transmittance = exp(-(beta_r * view_depth.x + beta_m * MIE_EXTINCTION * view_depth.y));
    let luminance = dot(light, vec3<f32>(0.2126, 0.7152, 0.0722));
        var alpha = 1.0 - dot(transmittance, vec3<f32>(1.0 / 3.0));
    if sky_behind {
        alpha = clamp(alpha + SKY_HIDES * luminance, 0.0, 1.0);
    }
    // Premultiplied blending: result = light + (1 - alpha) * background.
    return vec4<f32>(light, alpha);
}
