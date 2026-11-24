// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

// The cloth solver for skirts and dresses: `rk_chara::physics::drape::run`
// on the GPU, one invocation per point, and the pass that puts a garment's
// vertices on the solved nets. The rules and their sources are told there;
// this file follows it sum by sum.

struct Sub {
    // The floor's way up and how far along it the floor is.
    floor: vec4<f32>,
    // How the cloth moved with the character before this frame.
    carry: mat4x4<f32>,
    h: f32,
    damp: f32,
    fade: f32,
    // How far off a map triangle a point is given its plane.
    reach: f32,
    // The boundary between substeps this one starts at.
    boundary: u32,
    slots: u32,
    shells: u32,
    // 1 arms, 2 floor, 4 map, 8 reset, 16 carry.
    flags: u32,
    points: u32,
    triangles: u32,
    friction: vec2<f32>,
    thickness: f32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

struct Point {
    rest: vec3<f32>,
    gap: f32,
    gap_out: f32,
    allow: f32,
    tether: f32,
    inner: f32,
    anchor: u32,
    // Bit 0: fixed. From bit 8: its net.
    bits: u32,
    turn: f32,
    pad: u32,
    around: vec4<u32>,
}

struct Pose {
    m: mat4x4<f32>,
    inv: mat4x4<f32>,
}

struct Bound {
    // The vertex and the three points of the net it is bound to.
    at: vec4<u32>,
    // How much of each point, and how much it follows the net.
    bary: vec4<f32>,
    place: vec4<f32>,
    normal: vec4<f32>,
    tangent: vec4<f32>,
}

@group(0) @binding(0) var<uniform> sub: Sub;
// Places with how much each is pressed, speeds, predicted places.
@group(0) @binding(1) var<storage, read_write> pos: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> vel: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> next: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read> points: array<Point>;
// Per point 8 links (to, rest, soft, buckle) and 6 hinges (a, b, rest,
// soft), floats as bits.
@group(0) @binding(5) var<storage, read> topo: array<vec4<u32>>;
// Per shell 5 x 4 floats, then every shell's thicknesses.
@group(0) @binding(6) var<storage, read> body: array<f32>;
// Per boundary and slot.
@group(0) @binding(7) var<storage, read> poses: array<Pose>;
// Four planes of the map per point.
@group(0) @binding(8) var<storage, read_write> planes: array<vec4<f32>>;
// Three corners per triangle.
@group(0) @binding(9) var<storage, read> triangles: array<vec4<f32>>;
// Per point: place and pressed, round and inner, down.
@group(0) @binding(10) var<storage, read_write> frames: array<vec4<f32>>;
@group(0) @binding(11) var<storage, read> bound: array<Bound>;
@group(0) @binding(12) var<storage, read_write> out: array<f32>;

const NONE: u32 = 0xffffffffu;
const LINKS: u32 = 8u;
const HINGES: u32 = 6u;
const PER_POINT: u32 = 14u;
const MAP_PLANES: u32 = 4u;
const BODY_PLANES: u32 = 16u;
const ANGLES: u32 = 24u;
const SHELL: u32 = 20u;
const TAU: f32 = 6.2831855;
const GRAVITY: f32 = 9.81;
const FASTEST: f32 = 4.0;
const NEAR: f32 = 0.02;
const HINGE_LEAST: f32 = 30.0;
const FLATTEN: f32 = 0.8;
const VERTEX_GAP: f32 = 0.003;

// A vertex: floats, and where the posed fields sit, as in skin.wgsl.
const STRIDE: u32 = 22u;
const NORMAL: u32 = 3u;
const TANGENT: u32 = 18u;

fn normalize_or_zero(v: vec3<f32>) -> vec3<f32> {
    let len = length(v);
    if len > 0.0 {
        let rcp = 1.0 / len;
        if rcp < 3.4e38 {
            return v * rcp;
        }
    }
    return vec3<f32>(0.0);
}

fn point_of(m: mat4x4<f32>, p: vec3<f32>) -> vec3<f32> {
    return (m * vec4<f32>(p, 1.0)).xyz;
}

fn vector_of(m: mat4x4<f32>, v: vec3<f32>) -> vec3<f32> {
    return (m * vec4<f32>(v, 0.0)).xyz;
}

fn fixed(i: u32) -> bool {
    return (points[i].bits & 1u) != 0u;
}

fn light(i: u32) -> f32 {
    return select(1.0, 0.0, fixed(i));
}

// The pose of slot `k` at the substep's start (0) or end (1).
fn pose_at(end: u32, k: u32) -> u32 {
    return (sub.boundary + end) * sub.slots + k;
}

fn waist_at(end: u32, i: u32) -> u32 {
    return pose_at(end, sub.shells + (points[i].bits >> 8u));
}

fn shell_vec(k: u32, row: u32) -> vec3<f32> {
    let o = k * SHELL + row * 4u;
    return vec3<f32>(body[o], body[o + 1u], body[o + 2u]);
}

fn shell_num(k: u32, row: u32) -> f32 {
    return body[k * SHELL + row * 4u + 3u];
}

fn shell_outside(k: u32) -> bool {
    return (u32(shell_num(k, 2u)) & 4u) != 0u;
}

// A shell's thickness `t` of the way along its line at `angle` round it.
fn radius(k: u32, t: f32, angle: f32) -> f32 {
    let stations = u32(shell_num(k, 0u));
    let first = u32(shell_num(k, 1u));
    let s = clamp(t, 0.0, 1.0) * f32(stations - 1u);
    let s0 = min(u32(s), stations - 1u);
    let s1 = min(s0 + 1u, stations - 1u);
    let fs = s - f32(s0);
    let turn = (angle - floor(angle / TAU) * TAU) / TAU * f32(ANGLES);
    let a0 = u32(turn) % ANGLES;
    let a1 = (a0 + 1u) % ANGLES;
    let fa = turn - floor(turn);
    let n0 = body[first + s0 * ANGLES + a0];
    let n1 = body[first + s0 * ANGLES + a1];
    let f0 = body[first + s1 * ANGLES + a0];
    let f1 = body[first + s1 * ANGLES + a1];
    let near = n0 + (n1 - n0) * fa;
    let far = f0 + (f1 - f0) * fa;
    return near + (far - near) * fs;
}

struct OnShell {
    ok: bool,
    on: vec3<f32>,
    dir: vec3<f32>,
    dist: f32,
    r: f32,
}

// Where `l`, in shell `k`'s own space, is on the shell.
fn on_shell(k: u32, l: vec3<f32>) -> OnShell {
    var o: OnShell;
    o.ok = false;
    let a = shell_vec(k, 0u);
    let axis = shell_vec(k, 1u) - a;
    let caps = u32(shell_num(k, 2u));
    let len2 = max(dot(axis, axis), 1e-12);
    var t = dot(l - a, axis) / len2;
    if (t < 0.0 && (caps & 1u) == 0u) || (t > 1.0 && (caps & 2u) == 0u) {
        return o;
    }
    let beyond = t < 0.0 || t > 1.0;
    t = clamp(t, 0.0, 1.0);
    o.on = a + axis * t;
    let off = l - o.on;
    o.dist = length(off);
    if o.dist < 1e-6 {
        return o;
    }
    o.dir = off / o.dist;
    if beyond {
        o.r = select(body[k * SHELL + 16u], body[k * SHELL + 17u], t > 0.5);
    } else {
        o.r = radius(k, t, atan2(dot(off, shell_vec(k, 3u)), dot(off, shell_vec(k, 2u))));
    }
    o.ok = true;
    return o;
}

struct Plane {
    n: vec3<f32>,
    d: f32,
    moved: vec3<f32>,
    began: f32,
    pushed: f32,
}

// How a frame begins: every point where it rests, or moved with the
// character.
@compute @workgroup_size(64)
fn cs_begin(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= sub.points {
        return;
    }
    if (sub.flags & 8u) != 0u {
        pos[i] = vec4<f32>(point_of(poses[waist_at(0u, i)].m, points[i].rest), 0.0);
        vel[i] = vec4<f32>(0.0);
    } else if (sub.flags & 16u) != 0u {
        pos[i] = vec4<f32>(point_of(sub.carry, pos[i].xyz), pos[i].w);
        vel[i] = vec4<f32>(vector_of(sub.carry, vel[i].xyz), 0.0);
    }
}

// The closest point of a triangle to `p` (Ericson 5.1.5).
fn closest(p: vec3<f32>, a: vec3<f32>, b: vec3<f32>, c: vec3<f32>) -> vec3<f32> {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    return a + ab * (vb * denom) + ac * (vc * denom);
}

// The planes of the map each point may meet this frame.
@compute @workgroup_size(64)
fn cs_find(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= sub.points {
        return;
    }
    var best: array<vec4<f32>, 4>;
    var far: array<f32, 4>;
    for (var k = 0u; k < MAP_PLANES; k++) {
        best[k] = vec4<f32>(0.0);
        far[k] = 3.4e38;
    }
    if (sub.flags & 4u) != 0u && !fixed(i) {
        let p = pos[i].xyz;
        for (var t = 0u; t < sub.triangles; t++) {
            let a = triangles[t * 3u].xyz;
            let b = triangles[t * 3u + 1u].xyz;
            let c = triangles[t * 3u + 2u].xyz;
            var front = cross(b - a, c - a);
            let area = length(front);
            if area < 1e-12 {
                continue;
            }
            front = front / area;
            let height = dot(p - a, front);
            if height < -sub.thickness || height > sub.reach {
                continue;
            }
            let q = closest(p, a, b, c);
            let off = p - q;
            var dist = length(off);
            if dist > sub.reach {
                continue;
            }
            let along = length(off - front * height);
            var n = front;
            if !(along < 1e-5 || dist < 1e-6) {
                n = off / dist;
            }
            if dot(n, front) < 0.0 {
                continue;
            }
            dist = dot(p - q, n);
            var k = MAP_PLANES;
            while k > 0u && dist < far[k - 1u] {
                k -= 1u;
            }
            if k < MAP_PLANES {
                for (var j = MAP_PLANES - 1u; j > k; j--) {
                    far[j] = far[j - 1u];
                    best[j] = best[j - 1u];
                }
                far[k] = dist;
                best[k] = vec4<f32>(n, dot(q, n));
            }
        }
    }
    for (var k = 0u; k < MAP_PLANES; k++) {
        planes[i * MAP_PLANES + k] = best[k];
    }
}

@compute @workgroup_size(64)
fn cs_predict(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= sub.points {
        return;
    }
    if fixed(i) {
        next[i] = vec4<f32>(point_of(poses[waist_at(1u, i)].m, points[i].rest), 0.0);
    } else {
        let v = (vel[i].xyz + vec3<f32>(0.0, -GRAVITY, 0.0) * sub.h) * sub.damp;
        next[i] = vec4<f32>(pos[i].xyz + v * sub.h, 0.0);
    }
}

@compute @workgroup_size(64)
fn cs_solve(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= sub.points {
        return;
    }
    let h = sub.h;
    let x0 = pos[i].xyz;
    let y = next[i].xyz;
    if fixed(i) {
        vel[i] = vec4<f32>((y - x0) / h, 0.0);
        pos[i] = vec4<f32>(y, pos[i].w);
        return;
    }
    let p = points[i];
    // 1. Lengths.
    var sum = vec3<f32>(0.0);
    var count = 0.0;
    for (var s = 0u; s < LINKS; s++) {
        let link = topo[i * PER_POINT + s];
        if link.x == NONE {
            continue;
        }
        let d = y - next[link.x].xyz;
        let len = length(d);
        if len < 1e-9 {
            continue;
        }
        let c = len - bitcast<f32>(link.y);
        let w = light(link.x);
        if c > 0.0 {
            sum -= d * (c / (len * (1.0 + w + bitcast<f32>(link.z) / (h * h))));
        } else {
            sum += d * (min(bitcast<f32>(link.w) * h * h, -c / (1.0 + w)) / len);
        }
        count += 1.0;
    }
    var x = y;
    if count > 0.0 {
        x += sum / count;
    }
    // 2. Bends.
    var bend = vec3<f32>(0.0);
    for (var s = 0u; s < HINGES; s++) {
        let hinge = topo[i * PER_POINT + LINKS + s];
        if hinge.x == NONE {
            continue;
        }
        let ya = next[hinge.x].xyz;
        let yb = next[hinge.y].xyz;
        let middle = s < 2u;
        var d = ya - (y + yb) * 0.5;
        var weights = light(hinge.x) + (1.0 + light(hinge.y)) * 0.25;
        var share = -0.5;
        if middle {
            d = y - (ya + yb) * 0.5;
            weights = 1.0 + (light(hinge.x) + light(hinge.y)) * 0.25;
            share = 1.0;
        }
        let len = length(d);
        if len < 1e-9 {
            continue;
        }
        let soft = max(bitcast<f32>(hinge.w) / (h * h), HINGE_LEAST);
        let lambda = -(len - bitcast<f32>(hinge.z)) / (weights + soft);
        bend += d * (lambda / len * share);
    }
    x += bend;
    // 3. The tether.
    if p.anchor != NONE {
        let top = next[p.anchor].xyz;
        let d = x - top;
        let len = length(d);
        if len > p.tether {
            x = top + d * (p.tether / len);
        }
    }
    // 4. Contact: the planes of the body near the point.
    var held: array<Plane, 21>;
    var bodies = 0u;
    for (var k = 0u; k < sub.shells; k++) {
        let outside = shell_outside(k);
        if bodies == BODY_PLANES || (outside && (sub.flags & 1u) == 0u) {
            continue;
        }
        let start = poses[pose_at(0u, k)];
        let fin = poses[pose_at(1u, k)].m;
        let scale = length(fin[0].xyz);
        let l = point_of(start.inv, x0);
        let o = on_shell(k, l);
        if !o.ok {
            continue;
        }
        let gap = select(p.gap - p.allow, p.gap_out, outside);
        let skin = o.r * scale + gap;
        let began = skin - o.dist * scale;
        if began < -NEAR {
            continue;
        }
        let moved = point_of(fin, l) - x0;
        var plane: Plane;
        plane.n = normalize_or_zero(vector_of(fin, o.dir));
        plane.moved = moved;
        plane.pushed = 0.0;
        plane.d = dot(point_of(fin, o.on), plane.n) + skin;
        plane.began = dot(x0, plane.n) + began;
        if outside && began > 0.0 {
            var clear = 3.4e38;
            var way = vec3<f32>(0.0);
            for (var j = 0u; j < sub.shells; j++) {
                if shell_outside(j) {
                    continue;
                }
                let at = poses[pose_at(0u, j)];
                let lt = point_of(at.inv, x0);
                let ta = shell_vec(j, 0u);
                let axis = shell_vec(j, 1u) - ta;
                let along = clamp(dot(lt - ta, axis) / max(dot(axis, axis), 1e-12), 0.0, 1.0);
                let off = lt - (ta + axis * along);
                let far = length(off);
                if far < 1e-6 {
                    continue;
                }
                let thick = radius(j, along, atan2(dot(off, shell_vec(j, 3u)), dot(off, shell_vec(j, 2u))));
                let c = (far - thick) * length(at.m[0].xyz);
                if c < clear {
                    clear = c;
                    way = normalize_or_zero(vector_of(at.m, -off));
                }
            }
            if clear < 3.4e38 {
                let off = vector_of(start.m, l - o.on);
                let along = dot(off, way);
                let far = -along + sqrt(max(along * along + skin * skin - dot(off, off), 0.0));
                plane.n = way;
                plane.began = dot(x0 + way * far, way);
                plane.d = dot(x0 + way * far + moved, way);
            }
        }
        held[bodies] = plane;
        bodies += 1u;
    }
    // The middles of its links, by where they are predicted.
    for (var s = 0u; s < LINKS; s++) {
        let link = topo[i * PER_POINT + s];
        if link.x == NONE || light(link.x) == 0.0 {
            continue;
        }
        let other = next[link.x].xyz;
        let middle = (x + other) * 0.5;
        for (var k = 0u; k < sub.shells; k++) {
            let outside = shell_outside(k);
            if bodies == BODY_PLANES || (outside && (sub.flags & 1u) == 0u) {
                continue;
            }
            let start = poses[pose_at(0u, k)];
            let fin = poses[pose_at(1u, k)];
            let scale = length(fin.m[0].xyz);
            let o = on_shell(k, point_of(fin.inv, middle));
            if !o.ok {
                continue;
            }
            let gap = select(p.gap - p.allow, p.gap_out, outside);
            let skin = o.r * scale + gap;
            let deep = skin - o.dist * scale;
            if deep <= 0.0 {
                continue;
            }
            var plane: Plane;
            plane.n = normalize_or_zero(vector_of(fin.m, o.dir));
            plane.d = dot(x, plane.n) + deep;
            plane.moved = point_of(fin.m, point_of(start.inv, x0)) - x0;
            plane.began = -3.4028235e38;
            plane.pushed = 0.0;
            held[bodies] = plane;
            bodies += 1u;
        }
    }
    // What cannot give way: the floor and the map.
    var total = bodies;
    if (sub.flags & 2u) != 0u {
        var plane: Plane;
        plane.n = sub.floor.xyz;
        plane.d = sub.floor.w + p.gap;
        plane.moved = vec3<f32>(0.0);
        plane.began = plane.d;
        plane.pushed = 0.0;
        held[total] = plane;
        total += 1u;
    }
    if (sub.flags & 4u) != 0u {
        for (var k = 0u; k < MAP_PLANES; k++) {
            let m = planes[i * MAP_PLANES + k];
            if all_zero(m.xyz) {
                continue;
            }
            var plane: Plane;
            plane.n = m.xyz;
            plane.d = m.w + p.gap;
            plane.moved = vec3<f32>(0.0);
            plane.began = plane.d;
            plane.pushed = 0.0;
            held[total] = plane;
            total += 1u;
        }
    }
    // A point that began inside something is put out of it without speed.
    var started = x0;
    for (var turn = 0u; turn < 2u; turn++) {
        for (var half = 0u; half < 2u; half++) {
            let lo = select(0u, bodies, half == 1u);
            let hi = select(bodies, total, half == 1u);
            var push = vec3<f32>(0.0);
            var pushes = 0.0;
            for (var k = lo; k < hi; k++) {
                let c = dot(started, held[k].n) - held[k].began;
                if c < 0.0 {
                    push += held[k].n * (c * c);
                    pushes -= c;
                }
            }
            if pushes > 0.0 {
                started += push / pushes;
            }
        }
    }
    x += started - x0;
    // Two rounds: the body's planes averaged, then the ground's.
    for (var turn = 0u; turn < 2u; turn++) {
        for (var half = 0u; half < 2u; half++) {
            let lo = select(0u, bodies, half == 1u);
            let hi = select(bodies, total, half == 1u);
            var push = vec3<f32>(0.0);
            var pushes = 0.0;
            for (var k = lo; k < hi; k++) {
                let c = dot(x, held[k].n) - held[k].d;
                if c < 0.0 {
                    push += held[k].n * (c * c);
                    pushes -= c;
                }
            }
            if pushes > 0.0 {
                for (var k = lo; k < hi; k++) {
                    let c = dot(x, held[k].n) - held[k].d;
                    if c < 0.0 {
                        held[k].pushed += c * c / pushes;
                    }
                }
                x += push / pushes;
            }
        }
    }
    // 5. Friction, against each thing that pushed.
    var rub = vec3<f32>(0.0);
    var rubs = 0.0;
    for (var k = 0u; k < total; k++) {
        if held[k].pushed <= 0.0 {
            continue;
        }
        rubs += held[k].pushed;
        let slid = x - started - held[k].moved;
        let across = slid - held[k].n * dot(slid, held[k].n);
        let far = length(across);
        if far < 1e-9 {
            continue;
        }
        if far < sub.friction.x * held[k].pushed {
            rub -= across * held[k].pushed;
        } else {
            rub -= across * (min(sub.friction.y * held[k].pushed / far, 1.0) * held[k].pushed);
        }
    }
    if rubs > 0.0 {
        x += rub / rubs;
    }
    var v = (x - started) / h;
    let speed = length(v);
    if speed > FASTEST {
        v *= FASTEST / speed;
    }
    vel[i] = vec4<f32>(v, 0.0);
    pos[i] = vec4<f32>(x, select(pos[i].w * sub.fade, 1.0, rubs > 0.0));
}

fn all_zero(v: vec3<f32>) -> bool {
    return v.x == 0.0 && v.y == 0.0 && v.z == 0.0;
}

// Each point with the directions round the skirt and down it there, for
// the vertices bound to it.
@compute @workgroup_size(64)
fn cs_frames(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= sub.points {
        return;
    }
    let p = points[i];
    let ring = (pos[p.around.y].xyz - pos[p.around.x].xyz) * p.turn;
    let down = pos[p.around.w].xyz - pos[p.around.z].xyz;
    frames[i * 3u] = pos[i];
    frames[i * 3u + 1u] = vec4<f32>(ring, p.inner);
    frames[i * 3u + 2u] = vec4<f32>(down, 0.0);
}

// Puts a skinned mesh's bound vertices on the nets: each where it rests
// in the frame of the net's surface, by how much it follows the net,
// then out of the body and above the floor.
@compute @workgroup_size(64)
fn cs_dress(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= arrayLength(&bound) {
        return;
    }
    let b = bound[id.x];
    let weight = b.bary.w;
    if weight <= 0.0 {
        return;
    }
    var at = vec4<f32>(0.0);
    var ring = vec4<f32>(0.0);
    var down = vec3<f32>(0.0);
    for (var k = 0u; k < 3u; k++) {
        let c = b.at[k + 1u] * 3u;
        at += frames[c] * b.bary[k];
        ring += frames[c + 1u] * b.bary[k];
        down += frames[c + 2u].xyz * b.bary[k];
    }
    let e1 = normalize_or_zero(down);
    let e3 = normalize_or_zero(cross(ring.xyz, down));
    let e2 = cross(e3, e1);
    // Where the net is folded flat its surface has no direction: the
    // vertex stays as it was skinned.
    if abs(dot(e1, cross(e2, e3))) < 0.5 {
        return;
    }
    let o = b.at.x * STRIDE;
    let skinned = vec3<f32>(out[o], out[o + 1u], out[o + 2u]);
    let skinned_n = vec3<f32>(out[o + NORMAL], out[o + NORMAL + 1u], out[o + NORMAL + 2u]);
    let skinned_t = vec3<f32>(out[o + TANGENT], out[o + TANGENT + 1u], out[o + TANGENT + 2u]);
    // Cloth pressed on something lies flat on it: what the layers stand
    // off the net by, more than the innermost one, mostly goes.
    let inner = ring.w;
    let off = inner + (b.place.z - inner) * (1.0 - FLATTEN * at.w);
    let moved = at.xyz + e1 * b.place.x + e2 * b.place.y + e3 * off;
    var p = mix(skinned, moved, weight);
    let n = normalize_or_zero(mix(skinned_n, e1 * b.normal.x + e2 * b.normal.y + e3 * b.normal.z, weight));
    let t = normalize_or_zero(mix(skinned_t, e1 * b.tangent.x + e2 * b.tangent.y + e3 * b.tangent.z, weight));
    for (var k = 0u; k < sub.shells; k++) {
        if shell_outside(k) {
            continue;
        }
        let pose = poses[pose_at(0u, k)];
        let scale = length(pose.m[0].xyz);
        let s = on_shell(k, point_of(pose.inv, p));
        if !s.ok {
            continue;
        }
        let deep = s.r * scale + VERTEX_GAP - s.dist * scale;
        if deep > 0.0 {
            p += normalize_or_zero(vector_of(pose.m, s.dir)) * (deep * weight);
        }
    }
    if (sub.flags & 2u) != 0u {
        let height = dot(p, sub.floor.xyz) - sub.floor.w;
        if height < VERTEX_GAP {
            p += sub.floor.xyz * ((VERTEX_GAP - height) * weight);
        }
    }
    out[o] = p.x;
    out[o + 1u] = p.y;
    out[o + 2u] = p.z;
    out[o + NORMAL] = n.x;
    out[o + NORMAL + 1u] = n.y;
    out[o + NORMAL + 2u] = n.z;
    out[o + TANGENT] = t.x;
    out[o + TANGENT + 1u] = t.y;
    out[o + TANGENT + 2u] = t.z;
}
