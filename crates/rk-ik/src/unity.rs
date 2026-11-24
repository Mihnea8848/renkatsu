// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Unity 5.6's vector, rotation and number functions, as the solver calls
//! them, so that a pose comes out as the game's does to the last digit.
//!
//! Two kinds:
//! - Managed (C# in `UnityEngine.dll`): read from the game's own copy and
//!   followed step by step. `Mathf` rounds through doubles
//!   (`(float)Math.Sqrt(f)`), and so do these. The game's Mono also works
//!   out float sums in double and rounds them only where they are kept or
//!   handed on: a dot product is three products summed in double, then
//!   rounded (`wide_dot`). Unity's answers show it.
//! - Native (`INTERNAL_CALL_`): `LookRotation`, `FromToRotation`,
//!   `AngleAxis`, `Inverse`, `Lerp` and `Slerp` of rotations, and
//!   `Vector3.OrthoNormalize`. Their code is in the engine and cannot be
//!   read; they are written as the engine's maths library is known to work
//!   and checked against the engine's own answers, which the capture
//!   plugin's `unitymath` key writes (`tests/unity_math.rs`): all of them
//!   agree within 1e-6, and the half turn for opposite vectors was read
//!   from those answers. `Vector3.Slerp` is native too and not yet in
//!   that capture.
//!
//! Vectors and rotations are glam's types; their own operators are not
//! used where Unity computes differently.

use glam::{Quat, Vec3};

// ---- Mathf (managed) ----

/// `Mathf.Sqrt`.
pub fn sqrt(f: f32) -> f32 {
    f64::from(f).sqrt() as f32
}

/// `Mathf.Acos`.
pub fn acos(f: f32) -> f32 {
    f64::from(f).acos() as f32
}

/// `Mathf.Atan`.
pub fn atan(f: f32) -> f32 {
    f64::from(f).atan() as f32
}

/// `Mathf.Floor`.
pub fn floor(f: f32) -> f32 {
    f64::from(f).floor() as f32
}

/// `Mathf.Clamp`.
pub fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// `Mathf.Clamp01`.
pub fn clamp01(value: f32) -> f32 {
    clamp(value, 0.0, 1.0)
}

/// `Mathf.Repeat`.
pub fn repeat(t: f32, length: f32) -> f32 {
    t - floor(t / length) * length
}

/// `Mathf.DeltaAngle`.
pub fn delta_angle(current: f32, target: f32) -> f32 {
    let mut num = repeat(target - current, 360.0);
    if num > 180.0 {
        num -= 360.0;
    }
    num
}

/// `Mathf.Rad2Deg` and `Mathf.Deg2Rad`.
pub const RAD2DEG: f32 = 57.29578;
pub const DEG2RAD: f32 = 0.017453292;

// ---- Vector3 (managed) ----

/// `Vector3.Dot`.
pub fn dot(a: Vec3, b: Vec3) -> f32 {
    wide_dot(a, b) as f32
}

/// Three products summed as Unity's Mono sums them: in double, rounded
/// only where the result is kept.
fn wide_dot(a: Vec3, b: Vec3) -> f64 {
    f64::from(a.x) * f64::from(b.x) + f64::from(a.y) * f64::from(b.y) + f64::from(a.z) * f64::from(b.z)
}

/// `Vector3.Cross`.
pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x)
}

/// `Vector3.magnitude`.
pub fn magnitude(v: Vec3) -> f32 {
    sqrt(wide_dot(v, v) as f32)
}

/// `Vector3.sqrMagnitude`.
pub fn sqr_magnitude(v: Vec3) -> f32 {
    wide_dot(v, v) as f32
}

/// `Vector3.Normalize` and `.normalized`: zero below a length of 1e-5.
pub fn normalize(v: Vec3) -> Vec3 {
    let num = magnitude(v);
    if num > 1e-5 {
        Vec3::new(v.x / num, v.y / num, v.z / num)
    } else {
        Vec3::ZERO
    }
}

/// `Vector3.Lerp`, `t` held to 0 to 1.
pub fn lerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    let t = clamp01(t);
    Vec3::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t)
}

/// `Vector3.Distance`.
pub fn distance(a: Vec3, b: Vec3) -> f32 {
    let v = Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z);
    sqrt(wide_dot(v, v) as f32)
}

/// `Vector3.Project`. Below the smallest float it is zero (`Mathf.Epsilon`,
/// flush to zero being off).
pub fn project(vector: Vec3, on_normal: Vec3) -> Vec3 {
    let num = dot(on_normal, on_normal);
    if num < f32::from_bits(1) {
        return Vec3::ZERO;
    }
    let d = dot(vector, on_normal);
    // `onNormal * Dot(...) / num`: scaled, then divided.
    let s = Vec3::new(on_normal.x * d, on_normal.y * d, on_normal.z * d);
    Vec3::new(s.x / num, s.y / num, s.z / num)
}

/// `Vector3.Angle`, in degrees.
pub fn angle(from: Vec3, to: Vec3) -> f32 {
    acos(clamp(dot(normalize(from), normalize(to)), -1.0, 1.0)) * RAD2DEG
}

/// `Vector3 == Vector3`: closer than about 1e-5.
pub fn vec_eq(a: Vec3, b: Vec3) -> bool {
    sqr_magnitude(Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)) < 1e-10
}

/// `Mathf.Sin`.
pub fn sin(f: f32) -> f32 {
    f64::from(f).sin() as f32
}

// ---- Quaternion (managed) ----

/// `Quaternion * Quaternion`.
pub fn mul(l: Quat, r: Quat) -> Quat {
    Quat::from_xyzw(
        l.w * r.x + l.x * r.w + l.y * r.z - l.z * r.y,
        l.w * r.y + l.y * r.w + l.z * r.x - l.x * r.z,
        l.w * r.z + l.z * r.w + l.x * r.y - l.y * r.x,
        l.w * r.w - l.x * r.x - l.y * r.y - l.z * r.z,
    )
}

/// `Quaternion * Vector3`.
pub fn rotate(q: Quat, p: Vec3) -> Vec3 {
    let (n1, n2, n3) = (q.x * 2.0, q.y * 2.0, q.z * 2.0);
    let (n4, n5, n6) = (q.x * n1, q.y * n2, q.z * n3);
    let (n7, n8, n9) = (q.x * n2, q.x * n3, q.y * n3);
    let (n10, n11, n12) = (q.w * n1, q.w * n2, q.w * n3);
    Vec3::new(
        (1.0 - (n5 + n6)) * p.x + (n7 - n12) * p.y + (n8 + n11) * p.z,
        (n7 + n12) * p.x + (1.0 - (n4 + n6)) * p.y + (n9 - n10) * p.z,
        (n8 - n11) * p.x + (n9 + n10) * p.y + (1.0 - (n4 + n5)) * p.z,
    )
}

/// `Quaternion.Dot`.
pub fn quat_dot(a: Quat, b: Quat) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w
}

/// `Quaternion == Quaternion`: a dot product above 0.999999.
pub fn quat_eq(a: Quat, b: Quat) -> bool {
    quat_dot(a, b) > 0.999_999
}

// ---- Native ----

/// The engine's `Vector3f::epsilon`.
const EPSILON: f32 = 0.00001;

fn quat_normalize(q: Quat) -> Quat {
    let m = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
    Quat::from_xyzw(q.x / m, q.y / m, q.z / m, q.w / m)
}

/// A 3x3 matrix, `m[row][col]`.
type Mat3 = [[f32; 3]; 3];

/// The engine's `MatrixToQuaternion`: from the trace, or from the largest
/// diagonal term, then normalised.
fn matrix_to_quat(m: &Mat3) -> Quat {
    let trace = m[0][0] + m[1][1] + m[2][2];
    let mut q = [0.0f32; 4];
    if trace > 0.0 {
        let mut root = (trace + 1.0).sqrt();
        q[3] = 0.5 * root;
        root = 0.5 / root;
        q[0] = (m[2][1] - m[1][2]) * root;
        q[1] = (m[0][2] - m[2][0]) * root;
        q[2] = (m[1][0] - m[0][1]) * root;
    } else {
        const NEXT: [usize; 3] = [1, 2, 0];
        let mut i = 0;
        if m[1][1] > m[0][0] {
            i = 1;
        }
        if m[2][2] > m[i][i] {
            i = 2;
        }
        let j = NEXT[i];
        let k = NEXT[j];
        let mut root = (m[i][i] - m[j][j] - m[k][k] + 1.0).sqrt();
        q[i] = 0.5 * root;
        root = 0.5 / root;
        q[3] = (m[k][j] - m[j][k]) * root;
        q[j] = (m[j][i] + m[i][j]) * root;
        q[k] = (m[k][i] + m[i][k]) * root;
    }
    quat_normalize(Quat::from_xyzw(q[0], q[1], q[2], q[3]))
}

/// The rotation matrix that turns unit `from` onto unit `to`
/// (`Matrix3x3f::SetFromToRotation`, Möller and Hughes).
fn from_to_matrix(from: Vec3, to: Vec3) -> Mat3 {
    const E: f32 = 0.000001;
    let v = cross(from, to);
    let e = dot(from, to);
    if e > 1.0 - E {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    if e < -1.0 + E {
        // Nearly opposite: half a turn about the world's X axis made
        // square to `from`, or about Y when `from` lies along X; read from
        // Unity's answers.
        let square = |a: Vec3| cross(from, cross(a, from));
        let mut left = square(Vec3::X);
        if dot(left, left) < E {
            left = square(Vec3::Y);
        }
        let invlen = 1.0 / dot(left, left).sqrt();
        let l = Vec3::new(left.x * invlen, left.y * invlen, left.z * invlen);
        // A half turn about `l`: 2 l lT - I.
        let (xy, xz, yz) = (2.0 * l.x * l.y, 2.0 * l.x * l.z, 2.0 * l.y * l.z);
        return [
            [2.0 * l.x * l.x - 1.0, xy, xz],
            [xy, 2.0 * l.y * l.y - 1.0, yz],
            [xz, yz, 2.0 * l.z * l.z - 1.0],
        ];
    }
    let h = (1.0 - e) / dot(v, v);
    let hvx = h * v.x;
    let hvz = h * v.z;
    let hvxy = hvx * v.y;
    let hvxz = hvx * v.z;
    let hvyz = hvz * v.y;
    [
        [e + hvx * v.x, hvxy - v.z, hvxz + v.y],
        [hvxy + v.z, e + h * v.y * v.y, hvyz - v.x],
        [hvxz - v.y, hvyz + v.x, e + hvz * v.z],
    ]
}

/// `Quaternion.FromToRotation`: identity when either vector is shorter than
/// the engine's epsilon.
pub fn from_to_rotation(from: Vec3, to: Vec3) -> Quat {
    let (lm, rm) = (from.length(), to.length());
    if lm < EPSILON || rm < EPSILON {
        return Quat::IDENTITY;
    }
    matrix_to_quat(&from_to_matrix(from / lm, to / rm))
}

/// `Quaternion.LookRotation`: Z along `forward`, Y towards `up`. When the
/// two are parallel it turns Z onto `forward` the shortest way; a zero
/// `forward` gives the identity.
pub fn look_rotation(forward: Vec3, up: Vec3) -> Quat {
    let mag = forward.length();
    if mag < EPSILON {
        return Quat::IDENTITY;
    }
    let z = forward / mag;
    let x = cross(up, z);
    let xm = x.length();
    if xm >= EPSILON {
        let x = x / xm;
        let y = cross(z, x);
        if (dot(y, y) - 1.0).abs() <= 1e-5 {
            // Columns x, y, z.
            let m = [[x.x, y.x, z.x], [x.y, y.y, z.y], [x.z, y.z, z.z]];
            return matrix_to_quat(&m);
        }
    }
    matrix_to_quat(&from_to_matrix(Vec3::Z, z))
}

/// `Quaternion.AngleAxis`, the angle in degrees; identity for an axis of
/// no length.
pub fn angle_axis(degrees: f32, axis: Vec3) -> Quat {
    let mag = axis.length();
    if mag <= 0.000001 {
        return Quat::IDENTITY;
    }
    let a = axis / mag;
    let half = degrees * DEG2RAD * 0.5;
    let s = half.sin();
    Quat::from_xyzw(a.x * s, a.y * s, a.z * s, half.cos())
}

/// `Quaternion.Inverse`: the conjugate.
pub fn inverse(q: Quat) -> Quat {
    Quat::from_xyzw(-q.x, -q.y, -q.z, q.w)
}

/// `Quaternion.Lerp`: a straight blend the short way round, normalised,
/// `t` held to 0 to 1.
pub fn quat_lerp(a: Quat, b: Quat, t: f32) -> Quat {
    let t = clamp01(t);
    let q = if quat_dot(a, b) < 0.0 {
        Quat::from_xyzw(
            a.x + t * (-b.x - a.x),
            a.y + t * (-b.y - a.y),
            a.z + t * (-b.z - a.z),
            a.w + t * (-b.w - a.w),
        )
    } else {
        Quat::from_xyzw(
            a.x + t * (b.x - a.x),
            a.y + t * (b.y - a.y),
            a.z + t * (b.z - a.z),
            a.w + t * (b.w - a.w),
        )
    };
    quat_normalize(q)
}

/// `Quaternion.Slerp`, `t` held to 0 to 1: along the arc the short way
/// round, or a straight blend when the two are within about 18 degrees.
pub fn quat_slerp(a: Quat, b: Quat, t: f32) -> Quat {
    let t = clamp01(t);
    let mut d = quat_dot(a, b);
    let b = if d < 0.0 {
        d = -d;
        Quat::from_xyzw(-b.x, -b.y, -b.z, -b.w)
    } else {
        b
    };
    if d < 0.95 {
        let angle = d.acos();
        let inv = 1.0 / angle.sin();
        let (sa, sb) = ((angle * (1.0 - t)).sin(), (angle * t).sin());
        Quat::from_xyzw(
            (a.x * sa + b.x * sb) * inv,
            (a.y * sa + b.y * sb) * inv,
            (a.z * sa + b.z * sb) * inv,
            (a.w * sa + b.w * sb) * inv,
        )
    } else {
        quat_lerp(a, b, t)
    }
}

/// `Vector3.OrthoNormalize(ref a, ref b)`: `a` made unit (X when it has no
/// length), `b` made square to it and unit (any square vector when it lies
/// along `a`).
pub fn ortho_normalize(a: &mut Vec3, b: &mut Vec3) {
    let mag = a.length();
    *a = if mag > EPSILON { *a / mag } else { Vec3::X };
    let d = dot(*a, *b);
    *b -= *a * d;
    let mag = b.length();
    *b = if mag < EPSILON {
        ortho_normal_vector(*a)
    } else {
        *b / mag
    };
}

/// The rotation matrix of a turn of `angle` radians about unit `axis`
/// (`Matrix3x3f::SetAxisAngle`).
fn axis_angle_matrix(axis: Vec3, angle: f32) -> Mat3 {
    let (s, c) = (angle.sin(), angle.cos());
    let (xx, yy, zz) = (axis.x * axis.x, axis.y * axis.y, axis.z * axis.z);
    let (xy, yz, zx) = (axis.x * axis.y, axis.y * axis.z, axis.z * axis.x);
    let (xs, ys, zs) = (axis.x * s, axis.y * s, axis.z * s);
    let one_c = 1.0 - c;
    [
        [one_c * xx + c, one_c * xy - zs, one_c * zx + ys],
        [one_c * xy + zs, one_c * yy + c, one_c * yz - xs],
        [one_c * zx - ys, one_c * yz + xs, one_c * zz + c],
    ]
}

fn mat_mul_vec(m: &Mat3, v: Vec3) -> Vec3 {
    Vec3::new(
        m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
        m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
        m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
    )
}

/// `Vector3.Slerp`, `t` held to 0 to 1: the direction turned along the
/// arc, the length blended. Written as the engine's maths library is known
/// to work; not yet checked against Unity's own answers (the `unitymath`
/// capture has no rows for it).
pub fn vec_slerp(lhs: Vec3, rhs: Vec3, t: f32) -> Vec3 {
    let t = clamp01(t);
    let straight = |t: f32| lhs + (rhs - lhs) * t;
    let (lhs_mag, rhs_mag) = (lhs.length(), rhs.length());
    if lhs_mag < EPSILON || rhs_mag < EPSILON {
        return straight(t);
    }
    let lerped_magnitude = lhs_mag + (rhs_mag - lhs_mag) * t;
    let d = dot(lhs, rhs) / (lhs_mag * rhs_mag);
    if d > 1.0 - EPSILON {
        return straight(t);
    }
    let lhs_norm = lhs / lhs_mag;
    let m = if d < -1.0 + EPSILON {
        axis_angle_matrix(ortho_normal_vector(lhs_norm), std::f32::consts::PI * t)
    } else {
        let axis = cross(lhs, rhs);
        axis_angle_matrix(axis / axis.length(), d.acos() * t)
    };
    mat_mul_vec(&m, lhs_norm) * lerped_magnitude
}

/// A unit vector square to unit `n` (`OrthoNormalVectorFast`).
fn ortho_normal_vector(n: Vec3) -> Vec3 {
    const K1_OVER_SQRT2: f32 = std::f32::consts::FRAC_1_SQRT_2;
    if n.z.abs() > K1_OVER_SQRT2 {
        let k = 1.0 / (n.y * n.y + n.z * n.z).sqrt();
        Vec3::new(0.0, -n.z * k, n.y * k)
    } else {
        let k = 1.0 / (n.x * n.x + n.y * n.y).sqrt();
        Vec3::new(-n.y * k, n.x * k, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn rotations_turn_the_unity_way() {
        // Unity is left-handed: a quarter turn about up takes right to back.
        let q = angle_axis(90.0, Vec3::Y);
        assert!(near(rotate(q, Vec3::X), Vec3::NEG_Z), "{}", rotate(q, Vec3::X));
        // LookRotation along Z with Y up is no turn at all.
        assert!(quat_dot(look_rotation(Vec3::Z, Vec3::Y), Quat::IDENTITY).abs() > 0.99999);
        // Looking along X turns Z onto X.
        let look = look_rotation(Vec3::X, Vec3::Y);
        assert!(near(rotate(look, Vec3::Z), Vec3::X));
        assert!(near(rotate(look, Vec3::Y), Vec3::Y));
    }

    #[test]
    fn from_to_turns_one_vector_onto_the_other() {
        for (a, b) in [
            (Vec3::X, Vec3::Y),
            (Vec3::new(1.0, 2.0, 3.0), Vec3::new(-2.0, 0.5, 1.0)),
            (Vec3::Z, Vec3::NEG_Z),
            (Vec3::Y, Vec3::Y),
        ] {
            let q = from_to_rotation(a, b);
            assert!(near(rotate(q, a.normalize()), b.normalize()), "{a} {b}");
        }
        assert_eq!(from_to_rotation(Vec3::ZERO, Vec3::X), Quat::IDENTITY);
    }

    #[test]
    fn inverse_undoes_and_blends_end_where_they_should() {
        let q = look_rotation(Vec3::new(0.3, -0.4, 0.8), Vec3::Y);
        let back = mul(inverse(q), q);
        assert!(quat_dot(back, Quat::IDENTITY).abs() > 0.99999);
        let r = angle_axis(170.0, Vec3::new(1.0, 1.0, 0.0));
        for f in [quat_lerp, quat_slerp] {
            assert!(quat_dot(f(q, r, 0.0), q).abs() > 0.99999);
            assert!(quat_dot(f(q, r, 1.0), r).abs() > 0.99999);
        }
    }

    #[test]
    fn ortho_normalize_makes_a_square_pair() {
        let (mut a, mut b) = (Vec3::new(0.0, 0.0, 2.0), Vec3::new(0.0, 1.0, 1.0));
        ortho_normalize(&mut a, &mut b);
        assert!(near(a, Vec3::Z) && near(b, Vec3::Y));
        let (mut a, mut b) = (Vec3::X, Vec3::X * 3.0);
        ortho_normalize(&mut a, &mut b);
        assert!(dot(a, b).abs() < 1e-6 && (b.length() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn managed_vector_functions_follow_unity() {
        assert_eq!(normalize(Vec3::new(1e-6, 0.0, 0.0)), Vec3::ZERO);
        assert_eq!(lerp(Vec3::ZERO, Vec3::X, 2.0), Vec3::X);
        assert!((angle(Vec3::X, Vec3::Y) - 90.0).abs() < 1e-4);
        assert!((delta_angle(350.0, 10.0) - 20.0).abs() < 1e-5);
        assert_eq!(project(Vec3::new(1.0, 2.0, 3.0), Vec3::ZERO), Vec3::ZERO);
    }
}
