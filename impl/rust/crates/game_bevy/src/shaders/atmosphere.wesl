// Atmosphere and clouds (Batch 2, tasks 2.6–2.7): single scattering (Rayleigh + Mie),
// raymarched per pixel, with a thin cloud deck.
//
// One shader for every view: the blue rim and hazy disc from orbit, the sky, sunsets and night
// from the ground, haze over distant terrain, and clouds seen from above or below. The shell is
// drawn without back-face culling; from outside only its front faces shade, from inside only its
// back faces, so each pixel is computed once. Rays stop at the scene depth (depth prepass), so
// haze and clouds only cover what is in front of terrain, ships and the planet. Blending is
// premultiplied: scattered light is added and the background is dimmed by the transmittance.
//
// Clouds: where a ray crosses the cloud deck (a sphere at the cloud altitude), cover comes from
// noise thresholded by the baked moisture field, so wet belts are cloudy and deserts clear. The
// air in front of the deck scatters as usual, the cloud is lit by sunlight that crossed the air
// (golden at sunset, dark at night), and the air behind it is hidden by the cloud's opacity.

import bevy_pbr::render::{forward_io::VertexOutput, mesh_view_bindings::view};
import bevy_pbr::prepass::utils::prepass_depth;

struct AtmosphereUniform {
    star: vec4<f32>,      // xyz star position (render space), w starlight brightness
    center: vec4<f32>,    // xyz planet centre (render space), w planet radius (sea level)
    shell: vec4<f32>,     // x top radius, y Rayleigh scale height, z Mie scale height, w Mie g
    scatter: vec4<f32>,   // rgb Rayleigh coefficients at sea level (1/m), w Mie coefficient
    rotation: vec4<f32>,  // body rotation quaternion (xyzw), for clouds fixed to the planet
    clouds: vec4<f32>,    // x deck radius, y 1 = clouds on, z time (s), w seed
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> atmosphere: AtmosphereUniform;
// Baked surface fields (height, temperature, moisture, continent signal) in the body's frame.
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var field_map: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var field_sampler: sampler;

const PI: f32 = 3.14159265;
const VIEW_STEPS: i32 = 12;
const LIGHT_STEPS: i32 = 6;
// Mie extinction is scattering plus a little absorption.
const MIE_EXTINCTION: f32 = 1.1;
// Single scattering underestimates skylight; real skies get a lot from multiple scattering.
const MULTIPLE_SCATTERING: f32 = 3.0;
// Bright sky hides what's behind it (stars by day), as eyes and cameras adapt to it: this much
// coverage per unit of sky luminance.
const SKY_HIDES: f32 = 12.0;
// Cloud look.
const CLOUD_ALBEDO: f32 = 0.85;
const CLOUD_MAX_OPACITY: f32 = 0.95;
// Cloud features: about 2.6 noise cycles per 1000 km of deck, drifting slowly.
const CLOUD_FREQUENCY: f32 = 2.6e-6;
const CLOUD_DRIFT: f32 = 4.0e-4;

fn quat_rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

fn pcg3d(v_in: vec3<u32>) -> vec3<u32> {
    var v = v_in * 1664525u + 1013904223u;
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v ^= v >> vec3<u32>(16u);
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    return v;
}

fn hash3(p: vec3<f32>) -> f32 {
    let h = pcg3d(bitcast<vec3<u32>>(vec3<i32>(p)));
    return f32(h.x >> 8u) / 16777215.0;
}

fn value_noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let x00 = mix(hash3(i), hash3(i + vec3<f32>(1.0, 0.0, 0.0)), u.x);
    let x10 = mix(hash3(i + vec3<f32>(0.0, 1.0, 0.0)), hash3(i + vec3<f32>(1.0, 1.0, 0.0)), u.x);
    let x01 = mix(hash3(i + vec3<f32>(0.0, 0.0, 1.0)), hash3(i + vec3<f32>(1.0, 0.0, 1.0)), u.x);
    let x11 = mix(hash3(i + vec3<f32>(0.0, 1.0, 1.0)), hash3(i + vec3<f32>(1.0, 1.0, 1.0)), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

// Cloud noise in [0, 1]; octaves finer than the pixel footprint (`footprint`, in noise units)
// are left out so distant clouds don't sparkle.
fn cloud_noise(p: vec3<f32>, footprint: f32) -> f32 {
    let rot = mat3x3<f32>(
        vec3<f32>(0.00, 0.80, 0.60),
        vec3<f32>(-0.80, 0.36, -0.48),
        vec3<f32>(-0.60, -0.48, 0.64),
    );
    var sum = 0.0;
    var amp = 0.5;
    var norm = 0.0;
    var x = p;
    var wavelength = 1.0;
    for (var i = 0; i < 7; i++) {
        let w = 1.0 - smoothstep(0.25 * wavelength, 0.5 * wavelength, footprint);
        sum += amp * w * value_noise(x);
        norm += amp * w;
        x = rot * x * 2.03 + vec3<f32>(1.7, -2.3, 0.9);
        amp *= 0.55;
        wavelength *= 0.5;
    }
    return sum / max(norm, 1e-3);
}

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

fn extinction(depth: vec2<f32>) -> vec3<f32> {
    return atmosphere.scatter.rgb * depth.x + atmosphere.scatter.w * MIE_EXTINCTION * depth.y;
}

struct Segment {
    light: vec3<f32>,
    // Optical depth (Rayleigh, Mie) accumulated along the segment.
    depth: vec2<f32>,
};

// Single scattering along the ray from distance `a` to `b`, given the optical depth already
// crossed before `a`.
fn march(cam: vec3<f32>, d: vec3<f32>, l: vec3<f32>, a: f32, b: f32, before: vec2<f32>) -> Segment {
    var out: Segment;
    out.light = vec3<f32>(0.0);
    out.depth = vec2<f32>(0.0);
    if b <= a {
        return out;
    }
    let ds = (b - a) / f32(VIEW_STEPS);
    var sum_r = vec3<f32>(0.0);
    var sum_m = vec3<f32>(0.0);
    for (var i = 0; i < VIEW_STEPS; i++) {
        let p = cam + d * (a + (f32(i) + 0.5) * ds);
        let rho = density(p) * ds;
        out.depth += rho;
        let t = exp(-extinction(before + out.depth + light_depth(p, l)));
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
    out.light = MULTIPLE_SCATTERING * irradiance
        * (sum_r * atmosphere.scatter.rgb * phase_r + sum_m * atmosphere.scatter.w * phase_m);
    return out;
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
    let t0 = max(shell.x, 0.0);
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

        // Where the ray crosses the cloud deck, in the body's frame. Computed for every pixel so the
    // footprint derivative stays in uniform control flow.
    let deck = ray_sphere(cam, d, c, atmosphere.clouds.x);
    let t_deck = select(deck.x, deck.y, deck.x < 0.0);
    let q = atmosphere.rotation;
    let local = quat_rotate(vec4<f32>(-q.xyz, q.w), cam + d * max(t_deck, 0.0) - c);
    let footprint = length(fwidth(local)) * CLOUD_FREQUENCY;

    // The cloud in front of anything opaque, if clouds are on.
    var t_cloud = -1.0;
    var cloud_alpha = 0.0;
    var cloud_light = vec3<f32>(0.0);
    if atmosphere.clouds.y > 0.5 {
        let t = t_deck;
        if deck.x < deck.y && t > t0 && t < t1 {
            t_cloud = t;
            let p = cam + d * t;
            let dir = normalize(local);
            let moisture = textureSampleLevel(field_map, field_sampler, dir, 0.0).z;
            // Slow drift: the noise turns about the pole with time.
            let angle = atmosphere.clouds.z * CLOUD_DRIFT;
            let drift = vec3<f32>(
                local.x * cos(angle) - local.z * sin(angle),
                local.y,
                local.x * sin(angle) + local.z * cos(angle),
            );
                        let n = cloud_noise(drift * CLOUD_FREQUENCY + vec3<f32>(atmosphere.clouds.w), footprint);
            // Wetter places need less noise to be cloudy.
                        let threshold = 0.82 - 0.42 * moisture;
            cloud_alpha = smoothstep(threshold, threshold + 0.12, n) * CLOUD_MAX_OPACITY;
            if cloud_alpha > 0.0 {
                // Lit by the star through the air above the deck; darker underneath.
                let up = normalize(p - c);
                let sun = exp(-extinction(light_depth(p, l)));
                let facing = clamp(dot(up, l) * 0.6 + 0.4, 0.0, 1.0);
                let below = select(1.0, 0.55, length(cam - c) < atmosphere.clouds.x);
                cloud_light = CLOUD_ALBEDO * atmosphere.star.w * sun * facing * below;
            }
        }
    }

    var light: vec3<f32>;
    var transmittance: vec3<f32>;
    if cloud_alpha > 0.0 {
        let front_seg = march(cam, d, l, t0, t_cloud, vec2<f32>(0.0));
        let back_seg = march(cam, d, l, t_cloud, t1, front_seg.depth);
        let t_front = exp(-extinction(front_seg.depth));
        let t_back = exp(-extinction(back_seg.depth));
        light = front_seg.light
            + t_front * (cloud_alpha * cloud_light + (1.0 - cloud_alpha) * back_seg.light);
        transmittance = t_front * (1.0 - cloud_alpha) * t_back;
    } else {
        let seg = march(cam, d, l, t0, t1, vec2<f32>(0.0));
        light = seg.light;
        transmittance = exp(-extinction(seg.depth));
    }

    let luminance = dot(light, vec3<f32>(0.2126, 0.7152, 0.0722));
    var alpha = 1.0 - dot(transmittance, vec3<f32>(1.0 / 3.0));
    if sky_behind {
        alpha = clamp(alpha + SKY_HIDES * luminance, 0.0, 1.0);
    }
    // Premultiplied blending: result = light + (1 - alpha) * background.
    return vec4<f32>(light, alpha);
}
