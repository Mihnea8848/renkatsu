// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

// CharaStudio's screen effects, in the order its camera runs them: Amplify
// Occlusion, GlobalFog, BloomAndFlares, Amplify Color, SunShafts,
// VignetteAndChromaticAberration and DepthOfField.
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
    // Intensity and blur, as the material gets them. z: the colour
    // table's blend amount.
    vignette: vec4f,
    // GlobalFog. The fog colour, linear.
    fog_color: vec4f,
    // _HeightParams: the fog's top, the camera's height over it, 1 when the
    // camera is inside the fog, half the height density.
    fog_height: vec4f,
    // _DistanceParams.x, the eye depth of a pixel nothing was drawn on,
    // _SceneFogMode.x, and which fogs are on: 1 distance, 2 radial, 4 height.
    fog_distance: vec4f,
    // _SceneFogParams.
    fog_scene: vec4f,
    // The world ray of a pixel for one unit of eye depth is
    // z + x * ndc.x + y * ndc.y. x.w: the camera's height. z.w: the
    // game camera's near plane.
    fog_ray_x: vec4f,
    fog_ray_y: vec4f,
    fog_ray_z: vec4f,
    // DepthOfField. _CurveParams.yzw: the focal size, the aperture term
    // and the focus distance over the game camera's far plane. w: that far
    // plane.
    dof: vec4f,
    // SunShafts. The caster on the screen as a UV (rows from the top),
    // the largest radius, and the game camera's far plane.
    shaft_sun: vec4f,
    // _SunColor, zero with the caster behind the camera. w: 1 for the
    // Screen blend mode, 0 for Add.
    shaft_color: vec4f,
    // _SunThreshold.
    shaft_threshold: vec4f,
    // The 4 by 4 random texture: cosine, sine and a ray offset.
    rand: array<vec4f, 16>,
    // Lights glowing in Renkatsu's own fog: strength, how many lamps, the
    // fog's density, 1 when on.
    glow: vec4f,
    glow_eye: vec4f,
    // The way towards the sun (w 1 when there is one), and its colour.
    glow_sun_dir: vec4f,
    glow_sun: vec4f,
    // Lamps as toon.wgsl takes them: place, colour times intensity and
    // range, the way a spot shines and its cosine.
    glow_lamps: array<vec4f, 24>,
}

@group(0) @binding(0) var<uniform> fx: Fx;
@group(0) @binding(1) var tex_a: texture_2d<f32>;
@group(0) @binding(2) var tex_b: texture_2d<f32>;
@group(0) @binding(3) var depth_tex: texture_depth_2d;
@group(0) @binding(4) var samp: sampler;
@group(0) @binding(5) var lut_tex: texture_2d<f32>;
@group(0) @binding(6) var tex_c: texture_2d<f32>;

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

// ---- GlobalFog ----

// Hidden/GlobalFog, programs 0 (pass 1, distance), 2 (pass 0, both) and 3
// (pass 2, height: the one the Studio camera runs). The three differ only
// in which terms they add to g. `v` goes from the camera to the point.
fn fog_factor(v: vec3f, depth: f32) -> f32 {
    let flags = u32(fx.fog_distance.w);
    var g = fx.fog_distance.x;
    if (flags & 1u) != 0u {
        var d = depth;
        if (flags & 2u) != 0u {
            d = length(v);
        }
        g += d - fx.fog_ray_z.w;
    }
    if (flags & 4u) != 0u {
        let hp = fx.fog_height;
        let point_over = v.y + fx.fog_ray_x.w - hp.x;
        let c1 = hp.z * (point_over + hp.y);
        let c2 = min((1.0 - 2.0 * hp.z) * point_over, 0.0);
        g += -length(v * hp.w) * (c1 - c2 * c2 / abs(v.y + 1.0e-5));
    }
    g = max(g, 0.0);
    let mode = fx.fog_distance.z;
    var f = 0.0;
    if mode == 1.0 {
        f = g * fx.fog_scene.z + fx.fog_scene.w;
    } else if mode == 2.0 {
        f = exp2(-g * fx.fog_scene.y);
    } else if mode == 3.0 {
        let e = g * fx.fog_scene.x;
        f = exp2(-e * e);
    }
    return saturate(f);
}

// The pass writes lerp(unity_FogColor, picture, factor); here the fog
// colour is blended over the picture by one minus the factor. The game's
// far-pixel test compares the raw depth with 1, which its reversed depth
// only has on the near plane, so the sky is fogged as a point on the far
// plane: a pixel nothing was drawn on takes the game camera's far plane.
@fragment
fn fs_fog(in: FxOut) -> @location(0) vec4f {
    let z = textureLoad(depth_tex, vec2i(in.pos.xy), 0);
    let near = fx.ao2.z;
    let far = fx.ao2.w;
    var depth = near * far / (far - z * (far - near));
    if z >= 1.0 {
        depth = fx.fog_distance.y;
    }
    let ndc = vec2f(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let ray = fx.fog_ray_z.xyz + fx.fog_ray_x.xyz * ndc.x + fx.fog_ray_y.xyz * ndc.y;
    let a = 1.0 - fog_factor(ray * depth, depth);
    if fx.glow.w < 0.5 {
        return vec4f(fx.fog_color.rgb, a);
    }
    // Premultiplied: the fog's colour as much as there is fog, and the
    // light the fog gives back on top.
    return vec4f(fx.fog_color.rgb * a + fog_glow(ray * depth, a), a);
}

// The light lamps and the sun give back from the fog on the way to a
// point `way` from the camera, `a` the fog over it. A lamp's light is
// gathered along the way inside its range: Unity's falloff 1 / (1 + 25 q),
// q the squared distance over the squared range, is k2 / (k2 + d^2) with
// k2 = range^2 / 25, whose sum along a line has a closed form. The fog in
// front of the glow takes some of it, as fog takes the picture.
fn fog_glow(way: vec3f, a: f32) -> vec3f {
    let len = length(way);
    let dir = way / max(len, 1e-4);
    let eye = fx.glow_eye.xyz;
    let density = fx.glow.z;
    var light = vec3f(0.0);
    for (var k = 0u; k < u32(fx.glow.y); k++) {
        let place = fx.glow_lamps[k * 3u];
        let colour = fx.glow_lamps[k * 3u + 1u];
        let spot = fx.glow_lamps[k * 3u + 2u];
        let to = place.xyz - eye;
        let t0 = dot(to, dir);
        let h2 = max(dot(to, to) - t0 * t0, 0.0);
        let range = colour.w;
        let chord = sqrt(max(range * range - h2, 0.0));
        let ta = max(0.0, t0 - chord);
        let tb = min(len, t0 + chord);
        if tb <= ta {
            continue;
        }
        let k2 = range * range / 25.0;
        let h = sqrt(k2 + h2);
        var gathered = k2 * (atan((tb - t0) / h) - atan((ta - t0) / h)) / h;
        let nearest = clamp(t0, ta, tb);
        if spot.w > -1.0 {
            let along = dot(normalize(eye + dir * nearest - place.xyz), normalize(spot.xyz));
            gathered *= smoothstep(spot.w, mix(spot.w, 1.0, 0.2), along);
        }
        let front = density * nearest;
        // Fog scatters light mostly onwards: brightest looking towards the
        // lamp, a halo (Henyey-Greenstein, g 0.6).
        let towards = dot(dir, normalize(to));
        light += colour.rgb * gathered * exp(-front * front) * scatter(towards, 0.6);
    }
    light *= density * fx.glow.x;
    // The sun: fog towards it brighter.
    if fx.glow_sun_dir.w > 0.5 {
        light += fx.glow_sun.rgb * scatter(dot(dir, fx.glow_sun_dir.xyz), 0.7) * a * fx.glow.x;
    }
    return light;
}

// How much of the light fog sends on at an angle whose cosine is `c` from
// where the light goes (Henyey-Greenstein), per steradian: 1 / (4 pi) for
// fog that scatters every way alike.
fn scatter(c: f32, g: f32) -> f32 {
    return (1.0 - g * g) / (12.566371 * pow(1.0 + g * g - 2.0 * g * c, 1.5));
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

// A texel of the blend cache. Hidden/Amplify Color/BlendCache (program 0)
// fades the table to the neutral one (CreateDefaultLut, bytes cut down)
// by the blend amount, into an eight bit target.
fn blend_texel(p: vec2i, size: vec2i) -> vec3f {
    let q = vec2u(clamp(p, vec2i(0), size - 1));
    let table = textureLoad(lut_tex, q, 0).rgb;
    let neutral = vec3f(vec3u(q.x % 32u, q.y, q.x / 32u) * 255u / 31u) / 255.0;
    return floor((table + fx.vignette.z * (neutral - table)) * 255.0 + 0.5) / 255.0;
}

// The blend cache read with its bilinear, clamped sampler.
fn blend_cache(uv: vec2f) -> vec3f {
    let size = vec2i(textureDimensions(lut_tex));
    let p = uv * vec2f(size) - 0.5;
    let p0 = floor(p);
    let w = p - p0;
    let i = vec2i(p0);
    let low = mix(blend_texel(i, size), blend_texel(i + vec2i(1, 0), size), w.x);
    let high = mix(blend_texel(i + vec2i(0, 1), size), blend_texel(i + vec2i(1, 1), size), w.x);
    return mix(low, high, w.y);
}

// Hidden/Blend pass 1 (program 0) adds the blurred bright parts to the
// picture. Hidden/Amplify Color/Base pass 10 (HDR camera, linear colour
// space, no tone mapping, no dithering; program 25) then looks the colour
// up in the 32-slice table, in gamma space. With a blend amount the game
// draws Hidden/Amplify Color/Blend pass 10 (program 8), the same lookup in
// the blend cache.
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
    var a: vec3f;
    var b: vec3f;
    if fx.vignette.z == 0.0 {
        a = textureSampleLevel(lut_tex, samp, uv0, 0.0).rgb;
        b = textureSampleLevel(lut_tex, samp, uv0 + vec2f(0.03125, 0.0), 0.0).rgb;
    } else {
        a = blend_cache(uv0);
        b = blend_cache(uv0 + vec2f(0.03125, 0.0));
    }
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

// ---- SunShafts ----

// Linear01Depth of a pixel of the camera's depth texture: its eye depth
// over the game camera's far plane, and 1 where nothing was drawn. `p`
// counts rows from the top.
fn depth01(p: vec2i, game_far: f32) -> f32 {
    let q = clamp(p, vec2i(0), vec2i(fx.size.xy) - vec2i(1));
    let z = textureLoad(depth_tex, q, 0);
    if z >= 1.0 {
        return 1.0;
    }
    let near = fx.ao2.z;
    let far = fx.ao2.w;
    return near * far / (far - z * (far - near)) / game_far;
}

// The depth texel a point sampler takes at a UV. A pixel of a half size
// target has its centre on the corner of four texels; the game's textures
// count rows from the bottom, so the sampler takes the texel to the right
// and up.
fn depth_texel(uv: vec2f) -> vec2i {
    return vec2i(floor(uv * fx.size.xy + vec2f(0.01, -0.01)));
}

// Hidden/SunShaftsComposite, pass 2 (useDepthTexture), programs 1 and 6:
// how far the picture is over the threshold, where the sky shows, fading
// with the distance from the caster. The component then clears a border
// of one pixel (DrawBorder with Hidden/SimpleClear), done here as well.
// `src.xy` is the texel size of the target.
@fragment
fn fs_shaft_mask(in: FxOut) -> @location(0) vec4f {
    let size = 1.0 / fx.src.xy;
    if in.pos.x < 1.0 || in.pos.y < 1.0 || in.pos.x > size.x - 1.0 || in.pos.y > size.y - 1.0 {
        return vec4f(0.0);
    }
    let near = saturate(fx.shaft_sun.z - length(fx.shaft_sun.xy - in.uv));
    let c = textureSampleLevel(tex_a, samp, in.uv, 0.0).rgb;
    let over = max(c - fx.shaft_threshold.rgb, vec3f(0.0));
    if depth01(depth_texel(in.uv), fx.shaft_sun.w) > 0.99 {
        return vec4f((over.r + over.g + over.b) * near);
    }
    return vec4f(0.0);
}

// Hidden/SunShaftsComposite, pass 1, programs 2 and 0: six taps towards
// the caster. `src.zw` is _BlurRadius4.xy.
@fragment
fn fs_shaft_blur(in: FxOut) -> @location(0) vec4f {
    let step = (fx.shaft_sun.xy - in.uv) * fx.src.zw;
    var uv = in.uv;
    var sum = vec4f(0.0);
    for (var i = 0; i < 6; i++) {
        sum += textureSampleLevel(tex_a, samp, uv, 0.0);
        uv += step;
    }
    return sum * 0.166666672;
}

// Hidden/SunShaftsComposite, pass 4 (Add, program 4) and pass 0 (Screen,
// program 5): the rays in the shaft colour over the picture. The alpha
// stays the picture's.
@fragment
fn fs_shaft_blend(in: FxOut) -> @location(0) vec4f {
    let c = textureSampleLevel(tex_a, samp, in.uv, 0.0);
    let rays = saturate(textureSampleLevel(tex_b, samp, in.uv, 0.0).rgb * fx.shaft_color.rgb);
    if fx.shaft_color.w != 0.0 {
        return vec4f(vec3f(1.0) - (vec3f(1.0) - c.rgb) * (vec3f(1.0) - rays), c.a);
    }
    return vec4f(c.rgb + rays, c.a);
}

// ---- DepthOfField ----

// The disc of the shader's immediate constants: 28 offsets and their
// lengths.
const DISC = array<vec3f, 28>(
    vec3f(0.62463, 0.54337, 0.8279),
    vec3f(-0.13414, -0.94488, 0.95435),
    vec3f(0.38772, -0.43475, 0.58253),
    vec3f(0.12126, -0.19282, 0.22778),
    vec3f(-0.20388, 0.11133, 0.2323),
    vec3f(0.83114, -0.29218, 0.881),
    vec3f(0.10759, -0.57839, 0.58831),
    vec3f(0.28285, 0.79036, 0.83945),
    vec3f(-0.36622, 0.39516, 0.53876),
    vec3f(0.75591, 0.21916, 0.78704),
    vec3f(-0.5261, 0.02386, 0.52664),
    vec3f(-0.88216, -0.24471, 0.91547),
    vec3f(-0.48888, -0.2933, 0.57011),
    vec3f(0.44014, -0.08558, 0.44838),
    vec3f(0.21179, 0.51373, 0.55567),
    vec3f(0.05483, 0.95701, 0.95858),
    vec3f(-0.59001, -0.70509, 0.91938),
    vec3f(-0.80065, 0.24631, 0.83768),
    vec3f(-0.19424, -0.18402, 0.26757),
    vec3f(-0.43667, 0.76751, 0.88304),
    vec3f(0.21666, 0.11602, 0.24577),
    vec3f(0.15696, -0.856, 0.87027),
    vec3f(-0.75821, 0.58363, 0.95682),
    vec3f(0.99284, -0.02904, 0.99327),
    vec3f(-0.22234, -0.57907, 0.62029),
    vec3f(0.55052, -0.66984, 0.86704),
    vec3f(0.46431, 0.28115, 0.5428),
    vec3f(-0.07214, 0.60554, 0.60982),
);

fn smooth01(x: f32) -> f32 {
    return x * x * (3.0 - 2.0 * x);
}

// Hidden/Dof/DepthOfFieldHdr, pass 4, program 20: the circle of confusion
// in front of the focus, into the alpha of a half size target.
// _CurveParams.x is 1.
@fragment
fn fs_dof_front(in: FxOut) -> @location(0) vec4f {
    let d = depth01(depth_texel(in.uv), fx.dof.w);
    let coc = (fx.dof.z - d) * fx.dof.y / (d + 1.0e-5) - fx.dof.x;
    return vec4f(0.0, 0.0, 0.0, min(max(coc, 0.0), 1.0));
}

// Hidden/Dof/DepthOfFieldHdr, pass 2, programs 14 and 15: a blur that
// lets larger circles spread over smaller ones. `src.zw` is _Offsets.xy
// times the texel size.
@fragment
fn fs_dof_spread(in: FxOut) -> @location(0) vec4f {
    let weights = array<f32, 5>(0.8, 0.675, 0.5, 0.2, 0.075);
    let c = textureSampleLevel(tex_a, samp, in.uv, 0.0);
    var sum = c * c.a;
    var total = c.a;
    for (var i = 0; i < 5; i++) {
        let o = fx.src.zw * (f32(i + 1) / 6.0);
        for (var side = -1.0; side <= 1.0; side += 2.0) {
            let t = textureSampleLevel(tex_a, samp, in.uv + o * side, 0.0);
            let w = smooth01(saturate((t.a - c.a + 0.5) * 2.0)) * weights[i];
            sum += t * w;
            total += w;
        }
    }
    return sum / (total + 1.0e-4);
}

// Hidden/Dof/DepthOfFieldHdr, pass 13 (near blur, program 19) and pass 0
// (program 16): the circle of confusion on both sides of the focus, and
// the spread circles of the front where they are larger. The game writes
// it into the picture's alpha; here the picture is copied along.
// `src.z` is 1 with near blur.
@fragment
fn fs_dof_coc(in: FxOut) -> @location(0) vec4f {
    let p = vec2i(in.pos.xy);
    let d = depth01(p, fx.dof.w);
    var coc = min(max(abs(d - fx.dof.z) * fx.dof.y / (d + 1.0e-5) - fx.dof.x, 0.0), 1.0);
    if fx.src.z != 0.0 {
        coc = max(coc, textureSampleLevel(tex_b, samp, in.uv, 0.0).a);
    }
    return vec4f(textureLoad(tex_a, p, 0).rgb, coc);
}

// Hidden/Dof/DepthOfFieldHdr, pass 6, program 3: half size, leaving out
// what is in focus. `src.xy` is the texel size of the picture.
@fragment
fn fs_dof_half(in: FxOut) -> @location(0) vec4f {
    let o = fx.src.xy * 0.75;
    let p = vec2f(o.x, -o.y);
    let a = textureSampleLevel(tex_a, samp, in.uv + o, 0.0);
    let b = textureSampleLevel(tex_a, samp, in.uv - o, 0.0);
    let c = textureSampleLevel(tex_a, samp, in.uv + p, 0.0);
    let d = textureSampleLevel(tex_a, samp, in.uv - p, 0.0);
    let w = saturate(vec4f(a.a, b.a, c.a, d.a) * 10.0);
    let total = w.x + w.y + w.z + w.w;
    let centre = textureSampleLevel(tex_a, samp, in.uv, 0.0);
    if total * centre.a > 1.24999997e-6 {
        return vec4f((a.rgb * w.x + b.rgb * w.y + c.rgb * w.z + d.rgb * w.w) / total, centre.a);
    }
    return centre;
}

// The weight of a tap of the disc: full when its circle reaches the
// centre's, scaled by `reach`.
fn disc_weight(tap: f32, reach: f32) -> f32 {
    return smooth01(saturate((tap - reach + 0.264999986) * 3.77358508));
}

// Hidden/Dof/DepthOfFieldHdr, pass 17 (medium and high sample count),
// program 18: the disc blur on the half size picture, two taps along each
// of the 28 offsets. `src.xy` is the texel size, `src.zw` _Offsets.zw.
// The offsets' rows are turned, since the game's count from the bottom.
@fragment
fn fs_dof_disc(in: FxOut) -> @location(0) vec4f {
    let c = textureSampleLevel(tex_a, samp, in.uv, 0.0);
    let scale = c.a * fx.src.xy * fx.src.w;
    var count = max(c.a * 0.25, fx.src.z);
    var sum = c.rgb * count;
    for (var l = 0; l < 28; l++) {
        let k = DISC[l];
        let o = vec2f(k.x, -k.y) * scale;
        let inner = textureSampleLevel(tex_a, samp, in.uv + o / 1.20000005, 0.0);
        let rim = textureSampleLevel(tex_a, samp, in.uv + o / k.z, 0.0);
        if inner.a + rim.a > 0.0 {
            let wi = disc_weight(inner.a, c.a * k.z * 0.833333313);
            let wr = disc_weight(rim.a, c.a);
            sum += inner.rgb * wi + rim.rgb * wr;
            count += wi + wr;
        }
    }
    return vec4f(sum / count, c.a);
}

// Hidden/Dof/DepthOfFieldHdr, pass 18 (high sample count), program 4: a
// small disc blur of the picture, fading to the half size blur for
// circles from 0.65 to 0.85, and the picture as it is under 0.01.
// `tex_a` is the picture with its circles, `tex_b` the half size blur
// (_LowRez) and `tex_c` the picture before, for its alpha, which the game
// leaves holding the circle. `src.w` is _Offsets.w.
@fragment
fn fs_dof_apply(in: FxOut) -> @location(0) vec4f {
    let low = textureSampleLevel(tex_b, samp, in.uv, 0.0);
    let c = textureSampleLevel(tex_a, samp, in.uv, 0.0);
    let alpha = textureLoad(tex_c, vec2i(in.pos.xy), 0).a;
    let scale = c.a * fx.src.xy * fx.src.w;
    var count = max(c.a * 0.25, 0.100000001);
    var sum = c.rgb * count;
    for (var l = 0; l < 28; l++) {
        let k = DISC[l];
        let tap = textureSampleLevel(tex_a, samp, in.uv + vec2f(k.x, -k.y) * scale, 0.0);
        let w = disc_weight(tap.a, c.a * k.z);
        sum += tap.rgb * w;
        count += w;
    }
    let small = sum / (count + 1.0e-5);
    let far = smooth01(saturate((c.a - 0.649999976) * 4.99999905));
    if c.a < 0.01 {
        return vec4f(c.rgb, alpha);
    }
    return vec4f(mix(small, low.rgb, far), alpha);
}
