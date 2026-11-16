// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

// CharaStudio's screen effects, in the order its camera runs them: Amplify
// Occlusion, BloomAndFlares, Amplify Color and VignetteAndChromaticAberration.
// Each function follows the game's compiled shader program named above it.

struct Fx {
    // Frame width, height and their inverses.
    size: vec4f,
    // xy: texel size of the pass's source. zw: blur step, in UV for the
    // bloom and vignette blurs and in pixels for the occlusion blur.
    src: vec4f,
    // Radius, half projection scale, power exponent, bias.
    ao: vec4f,
    // Multiplier, blur sharpness, camera near, camera far.
    ao2: vec4f,
    // Tint and intensity.
    levels: vec4f,
    uv_to_view: vec4f,
    // Threshold, intensity, exposure, whether a colour table is bound.
    bloom: vec4f,
    // Intensity and blur, as the material gets them.
    vignette: vec4f,
    // The 4 by 4 random texture: cosine, sine and a ray offset.
    rand: array<vec4f, 16>,
}

@group(0) @binding(0) var<uniform> fx: Fx;
@group(0) @binding(1) var tex_a: texture_2d<f32>;
@group(0) @binding(2) var tex_b: texture_2d<f32>;
@group(0) @binding(3) var depth_tex: texture_depth_2d;
@group(0) @binding(4) var samp: sampler;
@group(0) @binding(5) var lut_tex: texture_2d<f32>;

struct FxOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_fx(@builtin(vertex_index) i: u32) -> FxOut {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var out: FxOut;
    out.pos = vec4f(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2f(p.x, 1.0 - p.y);
    return out;
}

// ---- Amplify Occlusion ----

// Eye depth of a pixel of the camera's depth texture. `p` counts rows from
// the bottom, as the game's screen coordinates do.
fn eye_depth(p: vec2i) -> f32 {
    let size = vec2i(fx.size.xy);
    let q = clamp(p, vec2i(0), size - vec2i(1));
    let z = textureLoad(depth_tex, vec2i(q.x, size.y - 1 - q.y), 0);
    let near = fx.ao2.z;
    let far = fx.ao2.w;
    return near * far / (far - z * (far - near));
}

fn view_pos(uv: vec2f, depth: f32) -> vec3f {
    return vec3f((uv * fx.uv_to_view.xy + fx.uv_to_view.zw) * depth, depth);
}

fn shorter(a: vec3f, b: vec3f) -> vec3f {
    if dot(a, a) < dot(b, b) {
        return a;
    }
    return b;
}

// Hidden/Amplify Occlusion/Occlusion, pass 24 (medium sample count, normals
// from depth), program 37: six directions of four steps.
@fragment
fn fs_ao(in: FxOut) -> @location(0) vec4f {
    let size = vec2i(fx.size.xy);
    let top = vec2i(in.pos.xy);
    let p = vec2i(top.x, size.y - 1 - top.y);
    let texel = fx.size.zw;
    let uv = (vec2f(p) + 0.5) * texel;
    let rand = fx.rand[(p.x & 3) + (p.y & 3) * 4];

    let depth = eye_depth(p);
    let pos = view_pos(uv, depth);
    let right = view_pos(uv + vec2f(texel.x, 0.0), eye_depth(p + vec2i(1, 0)));
    let left = view_pos(uv - vec2f(texel.x, 0.0), eye_depth(p - vec2i(1, 0)));
    let up = view_pos(uv + vec2f(0.0, texel.y), eye_depth(p + vec2i(0, 1)));
    let down = view_pos(uv - vec2f(0.0, texel.y), eye_depth(p - vec2i(0, 1)));
    let du = shorter(right - pos, pos - left);
    let dv = shorter(up - pos, pos - down);
    let n = normalize(cross(du, dv));

    let radius = fx.ao.x;
    let neg_inv_r2 = -1.0 / (radius * radius);
    let radius_px = radius * fx.ao.y / depth;
    let step_px = radius_px * 0.2;
    let first = rand.z * step_px + 1.0;
    var sum = 0.0;
    for (var d = 0; d < 6; d++) {
        let angle = f32(d) * 1.04719758;
        let s = sin(angle);
        let c = cos(angle);
        let dir = vec2f(c * rand.x - s * rand.y, c * rand.y + s * rand.x);
        var ray = first;
        for (var k = 0; k < 4; k++) {
            let snapped = round(dir * ray);
            let hit = view_pos(uv + snapped * texel, eye_depth(p + vec2i(snapped)));
            ray += step_px;
            let v = hit - pos;
            let vv = dot(v, v);
            let nv = dot(n, v) * inverseSqrt(vv);
            sum += saturate(nv - fx.ao.w) * max(vv * neg_inv_r2 + 1.0, 0.0);
        }
    }
    let lit = saturate(1.0 - sum * fx.ao2.x * 0.0833333358);
    return vec4f(min(pow(lit, fx.ao.z), 1.0), depth, 0.0, 0.0);
}

// Hidden/Amplify Occlusion/Blur, passes 6 and 7 (radius 4), programs 5 and
// 6: a Gaussian of sigma 2 that depth differences cut short.
@fragment
fn fs_ao_blur(in: FxOut) -> @location(0) vec4f {
    let size = vec2i(fx.size.xy);
    let p = vec2i(in.pos.xy);
    let along = vec2i(fx.src.zw);
    let centre = textureLoad(tex_a, p, 0).xy;
    let sharp = (saturate(1.0 - centre.y) * 100.0 + 1.0) * fx.ao2.y;
    var total = centre.x;
    var weight = 1.0;
    for (var k = 1; k <= 4; k++) {
        for (var side = -1; side <= 1; side += 2) {
            let q = clamp(p + along * (k * side), vec2i(0), size - vec2i(1));
            let s = textureLoad(tex_a, q, 0).xy;
            let dd = (centre.y - s.y) * sharp;
            let w = exp(-dd * dd - f32(k * k) * 0.125);
            total += s.x * w;
            weight += w;
        }
    }
    return vec4f(total / weight, centre.y, 0.0, 0.0);
}

// Hidden/Amplify Occlusion/Occlusion, pass 39, program 2. The pass blends
// DstColor Zero: the picture is multiplied by the result. Distance fade is
// off in the Studio scene, which leaves the intensity as set.
@fragment
fn fs_ao_apply(in: FxOut) -> @location(0) vec4f {
    let occlusion = textureLoad(tex_a, vec2i(in.pos.xy), 0).x;
    let tint = mix(fx.levels.rgb, vec3f(1.0), occlusion);
    return vec4f(vec3f(1.0) + fx.levels.a * (tint - vec3f(1.0)), 1.0);
}

// ---- BloomAndFlares ----

// Hidden/Blend, pass 2, programs 2 and 1: four taps half a source texel
// out. Hidden/ChromaticAberration pass 0 (program 0) is the same filter.
@fragment
fn fs_down4(in: FxOut) -> @location(0) vec4f {
    let o = fx.src.xy * 0.5;
    return (textureSampleLevel(tex_a, samp, in.uv + o, 0.0)
        + textureSampleLevel(tex_a, samp, in.uv - o, 0.0)
        + textureSampleLevel(tex_a, samp, in.uv + vec2f(o.x, -o.y), 0.0)
        + textureSampleLevel(tex_a, samp, in.uv - vec2f(o.x, -o.y), 0.0)) * 0.25;
}

// Hidden/BrightPassFilterForBloom, program 0, with useSrcAlphaAsMask at the
// scene's zero.
@fragment
fn fs_bright(in: FxOut) -> @location(0) vec4f {
    let c = textureSampleLevel(tex_a, samp, in.uv, 0.0);
    return max(c - vec4f(fx.bloom.x), vec4f(0.0));
}

// Hidden/SeparableBlurPlus, programs 1 and 0.
@fragment
fn fs_blur_plus(in: FxOut) -> @location(0) vec4f {
    let o = fx.src.zw;
    var c = textureSampleLevel(tex_a, samp, in.uv, 0.0) * 0.225;
    c += (textureSampleLevel(tex_a, samp, in.uv + o, 0.0) + textureSampleLevel(tex_a, samp, in.uv - o, 0.0)) * 0.15;
    c += (textureSampleLevel(tex_a, samp, in.uv + o * 2.0, 0.0) + textureSampleLevel(tex_a, samp, in.uv - o * 2.0, 0.0)) * 0.11;
    c += (textureSampleLevel(tex_a, samp, in.uv + o * 3.0, 0.0) + textureSampleLevel(tex_a, samp, in.uv - o * 3.0, 0.0)) * 0.075;
    c += (textureSampleLevel(tex_a, samp, in.uv + o * 6.5, 0.0) + textureSampleLevel(tex_a, samp, in.uv - o * 6.5, 0.0)) * 0.0525;
    return c;
}

// ---- Bloom's last pass and Amplify Color ----

// Hidden/Blend pass 1 (program 0) adds the blurred bright parts to the
// picture. Hidden/Amplify Color/Base pass 10 (HDR camera, linear colour
// space, no tone mapping, no dithering; program 25) then looks the colour
// up in the 32-slice table, in gamma space.
@fragment
fn fs_color(in: FxOut) -> @location(0) vec4f {
    var c = textureSampleLevel(tex_a, samp, in.uv, 0.0);
    c += textureSampleLevel(tex_b, samp, in.uv, 0.0) * fx.bloom.y;
    if fx.bloom.w == 0.0 {
        return c;
    }
    let lin = max(c.rgb * fx.bloom.z, vec3f(0.0));
    let g = saturate(pow(lin, vec3f(0.416666657)) * 1.055 - 0.055);
    let blue = g.b * 31.0;
    let f = fract(blue);
    let slice = blue - f;
    let uv0 = vec2f(g.r * 0.0302734375 + 0.00048828125 + slice * 0.03125, g.g * 0.96875 + 0.015625);
    let a = textureSampleLevel(lut_tex, samp, uv0, 0.0).rgb;
    let b = textureSampleLevel(lut_tex, samp, uv0 + vec2f(0.03125, 0.0), 0.0).rgb;
    let t = mix(a, b, f);
    return vec4f(t * (t * (t * 0.305306017 + 0.682171106) + 0.0125228781), c.a);
}

// ---- VignetteAndChromaticAberration ----

// Hidden/SeparableBlur, programs 0 and 1.
@fragment
fn fs_blur(in: FxOut) -> @location(0) vec4f {
    let o = fx.src.zw;
    var c = textureSampleLevel(tex_a, samp, in.uv, 0.0) * 0.4;
    c += (textureSampleLevel(tex_a, samp, in.uv + o, 0.0) + textureSampleLevel(tex_a, samp, in.uv - o, 0.0)) * 0.15;
    c += (textureSampleLevel(tex_a, samp, in.uv + o * 2.0, 0.0) + textureSampleLevel(tex_a, samp, in.uv - o * 2.0, 0.0)) * 0.1;
    c += (textureSampleLevel(tex_a, samp, in.uv + o * 3.0, 0.0) + textureSampleLevel(tex_a, samp, in.uv - o * 3.0, 0.0)) * 0.05;
    return c;
}

// Hidden/Vignetting, program 0: the picture fades to its blurred copy and
// darkens with the squared distance from the centre. The chromatic
// aberration pass that follows (Hidden/ChromaticAberration pass 1) moves
// nothing at the scene's zero aberration.
@fragment
fn fs_vignette(in: FxOut) -> @location(0) vec4f {
    let coords = (in.uv - 0.5) * 2.0;
    let d = dot(coords, coords);
    let sharp = textureSampleLevel(tex_a, samp, in.uv, 0.0);
    let soft = textureSampleLevel(tex_b, samp, in.uv, 0.0);
    return mix(sharp, soft, saturate(d * fx.vignette.y)) * (1.0 - d * fx.vignette.x);
}
