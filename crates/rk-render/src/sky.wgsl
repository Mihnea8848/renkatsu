// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

// The sky behind a map: Unity 5.6's Skybox/Procedural, Skybox/6 Sided and
// Skybox/Cubemap, ported from the programs the game's map bundles carry.
// One triangle covers the screen at the far plane; each pixel works with
// its own direction from the eye.

struct Sky {
    // The camera's axes: right and up reach the edge of the picture at 1.
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    // Unit vector towards the sun (_WorldSpaceLightPos0) and its linear
    // colour times its intensity (_LightColor0).
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    // Which shader (0 procedural, 1 six sided, 2 cubemap), the sun disk (0
    // none, 1 simple, 2 high quality), _Exposure, _Rotation in radians.
    kind: vec4<f32>,
    // _SunSize, _AtmosphereThickness, unity_ColorSpaceDouble as the
    // program was compiled, 1 when alpha is raised to the HDR exponent.
    params: vec4<f32>,
    sky_tint: vec4<f32>,
    ground: vec4<f32>,
    tint: vec4<f32>,
    // The HDR decoding of each texture (`_Tex_HDR`): front, back, left,
    // right, up, down; the cubemap's is the first.
    hdr: array<vec4<f32>, 6>,
    // 1 where a face's texture repeats: front, back, left, right, then up
    // and down.
    wrap_sides: vec4<f32>,
    wrap_ends: vec4<f32>,
}

@group(0) @binding(0) var<uniform> s: Sky;
@group(0) @binding(1) var cube: texture_cube<f32>;
@group(0) @binding(2) var front_tex: texture_2d<f32>;
@group(0) @binding(3) var back_tex: texture_2d<f32>;
@group(0) @binding(4) var left_tex: texture_2d<f32>;
@group(0) @binding(5) var right_tex: texture_2d<f32>;
@group(0) @binding(6) var up_tex: texture_2d<f32>;
@group(0) @binding(7) var down_tex: texture_2d<f32>;
@group(0) @binding(8) var clamp_sampler: sampler;
@group(0) @binding(9) var repeat_sampler: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    var out: VsOut;
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    // At the far plane: behind everything the depth buffer holds.
    out.clip = vec4<f32>(p, 1.0, 1.0);
    out.ndc = p;
    return out;
}

// Skybox/Procedural's `scale` without its factor 0.25: the exponent's
// polynomial in one minus the cosine.
fn depth_curve(cosine: f32) -> f32 {
    let x = 1.0 - cosine;
    return exp(-0.00287 + x * (0.459 + x * (3.83 + x * (-6.80 + x * 5.25))));
}

// Skybox/Procedural, vertex and fragment program in one, for the unit
// vector `eye` from the camera into the scene. The constants are the
// program's own: an atmosphere from radius 1 to 1.025, the camera 0.0001
// above the ground, two samples along the ray.
fn procedural(eye: vec3<f32>) -> vec3<f32> {
    let light = s.sun_dir.xyz;
    let exposure = s.kind.z;
    // The tint moves the scattered wavelengths; it is taken back to gamma.
    let tint = pow(max(s.sky_tint.rgb, vec3<f32>(0.0)), vec3<f32>(0.454545468));
    let wavelength = (1.0 - tint) * 0.3 + vec3<f32>(0.5, 0.42, 0.325);
    let w2 = wavelength * wavelength;
    let inv_wavelength = 1.0 / (w2 * w2);
    let thickness = pow(max(s.params.y, 0.0), 2.5);
    // Rayleigh scattering times the sun's brightness, and times 4 pi.
    let kr_esun = thickness * 0.0499999970;
    let kr_4pi = thickness * 0.0314159282;
    let km_4pi = 0.0125663709;
    let km_esun = 0.0199999996;
    let camera = vec3<f32>(0.0, 1.0001, 0.0);
    let out_scatter = inv_wavelength * kr_4pi + km_4pi;
    var c_in: vec3<f32>;
    var c_out: vec3<f32>;
    if eye.y >= 0.0 {
        // The sky: from the camera to the top of the atmosphere.
        let far = sqrt(eye.y * eye.y + 0.0506249666) - eye.y;
        let start_offset = depth_curve(eye.y) * 0.246031836;
        let sample_length = far * 0.5;
        let scaled_length = far * 20.0;
        var point = eye * sample_length * 0.5 + camera;
        var front = vec3<f32>(0.0);
        for (var i = 0; i < 2; i++) {
            let height = length(point);
            let depth = exp2((1.0 - height) * 230.831207);
            let light_angle = dot(light, point) / height;
            let camera_angle = dot(eye, point) / height;
            let scatter = depth * (depth_curve(light_angle) * 0.25 - depth_curve(camera_angle) * 0.25)
                + start_offset;
            let attenuate = exp(-clamp(scatter, 0.0, 50.0) * out_scatter);
            front += attenuate * (depth * scaled_length);
            point += eye * sample_length;
        }
        c_in = front * (inv_wavelength * kr_esun);
        c_out = front * km_esun;
    } else {
        // The ground: from the camera down to where the ray lands.
        let far = -0.0001 / min(eye.y, -0.001);
        let pos = eye * far + camera;
        let camera_curve = depth_curve(dot(-eye, pos));
        let camera_scale = camera_curve * 0.25;
        let camera_offset = camera_curve * 0.249900013;
        let temp = depth_curve(dot(light, pos)) * 0.25 + camera_scale;
        let sample_length = far * 0.5;
        let scaled_length = far * 20.0;
        let point = eye * sample_length * 0.5 + camera;
        let depth = exp2((1.0 - length(point)) * 230.831207);
        let scatter = depth * temp - camera_offset;
        let attenuate = exp(-clamp(scatter, 0.0, 50.0) * out_scatter);
        let front = attenuate * (depth * scaled_length);
        c_in = front * (inv_wavelength * kr_esun + km_esun);
        c_out = attenuate;
    }
    let ground_color = (s.ground.rgb * c_out + c_in) * exposure;
    let eye_cos = dot(light, -eye);
    let sky_color = c_in * (eye_cos * eye_cos * 0.75 + 0.75) * exposure;
    let sun_color = c_out * s.sun_color.rgb * exposure;

    // The fragment program: sky above the horizon, ground from 0.02 below.
    let ray = -eye;
    var color = mix(sky_color, ground_color, clamp(ray.y * 50.0, 0.0, 1.0));
    if ray.y < 0.0 {
        if s.kind.y > 1.5 {
            // The high quality disk: Mie scattering around the sun.
            let c = dot(light, ray);
            let power = pow(max(s.params.x, 0.0), 0.65) * 10.0;
            let temp = max(pow(c * 1.98 + 1.9801, power), 1e-4);
            color += (c * c + 1.0) * 0.0100164423 / temp * sun_color;
        } else if s.kind.y > 0.5 {
            // The simple disk: a soft spot of the sun's size.
            let t = clamp(length(ray + light) / s.params.x, 0.0, 1.0);
            let spot = 1.0 - t * t * (3.0 - 2.0 * t);
            color += spot * spot * 8000.0 * sun_color;
        }
    }
    return color;
}

// The shaders' turn about the vertical axis by _Rotation, undone: from a
// direction in the world to the one on the sky's mesh.
fn unturn(dir: vec3<f32>) -> vec3<f32> {
    let c = cos(s.kind.w);
    let n = sin(s.kind.w);
    return vec3<f32>(c * dir.x + n * dir.z, dir.y, c * dir.z - n * dir.x);
}

// DecodeHDR, then the tint, the exposure and unity_ColorSpaceDouble.
fn textured(texel: vec4<f32>, hdr: vec4<f32>) -> vec3<f32> {
    var alpha = hdr.w * (texel.a - 1.0) + 1.0;
    if s.params.w > 0.5 {
        alpha = pow(max(alpha, 0.0), hdr.y);
    }
    return texel.rgb * (alpha * hdr.x) * s.tint.rgb * s.kind.z * s.params.z;
}

fn face(t: texture_2d<f32>, repeats: f32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    if repeats > 0.5 {
        return textureSampleGrad(t, repeat_sampler, uv, dx, dy);
    }
    return textureSampleGrad(t, clamp_sampler, uv, dx, dy);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dir = normalize(s.forward.xyz + s.right.xyz * in.ndc.x + s.up.xyz * in.ndc.y);
    if s.kind.x < 0.5 {
        return vec4<f32>(procedural(dir), 1.0);
    }
    let m = unturn(dir);
    if s.kind.x > 1.5 {
        return vec4<f32>(textured(textureSample(cube, clamp_sampler, m), s.hdr[0]), 1.0);
    }
    // Six sided: each texture lies on one face of a cube, upright as seen
    // from inside. The direction projected on the three pairs of faces,
    // with its change across the pixel taken before the face is chosen.
    let on_z = m.xy / m.z;
    let on_x = m.zy / m.x;
    let on_y = m.xz / m.y;
    let dx = array<vec2<f32>, 3>(dpdx(on_z), dpdx(on_x), dpdx(on_y));
    let dy = array<vec2<f32>, 3>(dpdy(on_z), dpdy(on_x), dpdy(on_y));
    let a = abs(m);
    var texel: vec4<f32>;
    var hdr: vec4<f32>;
    if a.z >= a.x && a.z >= a.y {
        let flip = vec2<f32>(1.0, sign(m.z)) * 0.5;
        let uv = on_z * flip + 0.5;
        if m.z > 0.0 {
            texel = face(front_tex, s.wrap_sides.x, uv, dx[0] * flip, dy[0] * flip);
            hdr = s.hdr[0];
        } else {
            texel = face(back_tex, s.wrap_sides.y, uv, dx[0] * flip, dy[0] * flip);
            hdr = s.hdr[1];
        }
    } else if a.x >= a.y {
        let flip = vec2<f32>(-1.0, sign(m.x)) * 0.5;
        let uv = on_x * flip + 0.5;
        if m.x > 0.0 {
            texel = face(left_tex, s.wrap_sides.z, uv, dx[1] * flip, dy[1] * flip);
            hdr = s.hdr[2];
        } else {
            texel = face(right_tex, s.wrap_sides.w, uv, dx[1] * flip, dy[1] * flip);
            hdr = s.hdr[3];
        }
    } else {
        let flip = vec2<f32>(sign(m.y), -1.0) * 0.5;
        let uv = on_y * flip + 0.5;
        if m.y > 0.0 {
            texel = face(up_tex, s.wrap_ends.x, uv, dx[2] * flip, dy[2] * flip);
            hdr = s.hdr[4];
        } else {
            texel = face(down_tex, s.wrap_ends.y, uv, dx[2] * flip, dy[2] * flip);
            hdr = s.hdr[5];
        }
    }
    return vec4<f32>(textured(texel, hdr), 1.0);
}
