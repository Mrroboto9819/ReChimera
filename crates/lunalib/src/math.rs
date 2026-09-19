

pub fn zyx_euler_to_quat(angles: [f32; 3]) -> [f32; 4] {
    let (ax, ay, az) = (angles[0], angles[1], angles[2]);
    let (cx, sx) = ((ax * 0.5).cos(), (ax * 0.5).sin());
    let (cy, sy) = ((ay * 0.5).cos(), (ay * 0.5).sin());
    let (cz, sz) = ((az * 0.5).cos(), (az * 0.5).sin());

    [
        sx * cy * cz - cx * sy * sz,
        cx * sy * cz + sx * cy * sz,
        cx * cy * sz - sx * sy * cz,
        cx * cy * cz + sx * sy * sz,
    ]
}

pub fn decompose_row_major(m: &[f32; 16]) -> ([f32; 3], [f32; 3], [f32; 4]) {
    let translation = [m[12], m[13], m[14]];

    let rx = [m[0], m[1], m[2]];
    let ry = [m[4], m[5], m[6]];
    let rz = [m[8], m[9], m[10]];

    let sx = (rx[0] * rx[0] + rx[1] * rx[1] + rx[2] * rx[2]).sqrt();
    let sy = (ry[0] * ry[0] + ry[1] * ry[1] + ry[2] * ry[2]).sqrt();
    let sz = (rz[0] * rz[0] + rz[1] * rz[1] + rz[2] * rz[2]).sqrt();
    let scale = [sx, sy, sz];

    if sx == 0.0 || sy == 0.0 || sz == 0.0 {
        return (translation, scale, [0.0, 0.0, 0.0, 1.0]);
    }

    let mut r = [
        [rx[0] / sx, ry[0] / sy, rz[0] / sz],
        [rx[1] / sx, ry[1] / sy, rz[1] / sz],
        [rx[2] / sx, ry[2] / sy, rz[2] / sz],
    ];
    let mut scale = scale;

    // A mirrored instance (negative determinant) is a reflection, which a
    // quaternion cannot represent. Shepperd's method below then emits a
    // NON-UNIT quaternion; three.js applies that as rotation + a spurious
    // scale of |q|^2, shrinking/distorting the object. ~24% of R3 ties are
    // mirrored (e.g. corrugated-chunk / foundation-wall props). Fold the
    // reflection into a negative X scale and flip column 0 so what remains is
    // a proper rotation — three.js reconstructs the mirror from the signed
    // scale. Matches three.js Matrix4.decompose. No-op for det>0 matrices, so
    // correctly-decomposed instances (R2/RFOM included) are unchanged.
    let det = r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1])
        - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
        + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]);
    if det < 0.0 {
        scale[0] = -scale[0];
        r[0][0] = -r[0][0];
        r[1][0] = -r[1][0];
        r[2][0] = -r[2][0];
    }

    let trace = r[0][0] + r[1][1] + r[2][2];
    let quat = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (r[2][1] - r[1][2]) / s,
            (r[0][2] - r[2][0]) / s,
            (r[1][0] - r[0][1]) / s,
            0.25 * s,
        ]
    } else if r[0][0] > r[1][1] && r[0][0] > r[2][2] {
        let s = (1.0 + r[0][0] - r[1][1] - r[2][2]).sqrt() * 2.0;
        [
            0.25 * s,
            (r[0][1] + r[1][0]) / s,
            (r[0][2] + r[2][0]) / s,
            (r[2][1] - r[1][2]) / s,
        ]
    } else if r[1][1] > r[2][2] {
        let s = (1.0 + r[1][1] - r[0][0] - r[2][2]).sqrt() * 2.0;
        [
            (r[0][1] + r[1][0]) / s,
            0.25 * s,
            (r[1][2] + r[2][1]) / s,
            (r[0][2] - r[2][0]) / s,
        ]
    } else {
        let s = (1.0 + r[2][2] - r[0][0] - r[1][1]).sqrt() * 2.0;
        [
            (r[0][2] + r[2][0]) / s,
            (r[1][2] + r[2][1]) / s,
            0.25 * s,
            (r[1][0] - r[0][1]) / s,
        ]
    };

    (translation, scale, quat)
}

pub fn decompose_col_major(m: &[f32; 16]) -> ([f32; 3], [f32; 3], [f32; 4]) {
    let translation = [m[12], m[13], m[14]];

    let c0 = [m[0], m[1], m[2]];
    let c1 = [m[4], m[5], m[6]];
    let c2 = [m[8], m[9], m[10]];

    let s0 = (c0[0] * c0[0] + c0[1] * c0[1] + c0[2] * c0[2]).sqrt();
    let s1 = (c1[0] * c1[0] + c1[1] * c1[1] + c1[2] * c1[2]).sqrt();
    let s2 = (c2[0] * c2[0] + c2[1] * c2[1] + c2[2] * c2[2]).sqrt();
    let scale = [s0, s1, s2];

    if s0 == 0.0 || s1 == 0.0 || s2 == 0.0 {
        return (translation, scale, [0.0, 0.0, 0.0, 1.0]);
    }

    let r00 = c0[0] / s0;
    let r10 = c0[1] / s0;
    let r20 = c0[2] / s0;
    let r01 = c1[0] / s1;
    let r11 = c1[1] / s1;
    let r21 = c1[2] / s1;
    let r02 = c2[0] / s2;
    let r12 = c2[1] / s2;
    let r22 = c2[2] / s2;

    let trace = r00 + r11 + r22;
    let quat = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (r21 - r12) / s,
            (r02 - r20) / s,
            (r10 - r01) / s,
            0.25 * s,
        ]
    } else if r00 > r11 && r00 > r22 {
        let s = (1.0 + r00 - r11 - r22).sqrt() * 2.0;
        [
            0.25 * s,
            (r01 + r10) / s,
            (r02 + r20) / s,
            (r21 - r12) / s,
        ]
    } else if r11 > r22 {
        let s = (1.0 + r11 - r00 - r22).sqrt() * 2.0;
        [
            (r01 + r10) / s,
            0.25 * s,
            (r12 + r21) / s,
            (r02 - r20) / s,
        ]
    } else {
        let s = (1.0 + r22 - r00 - r11).sqrt() * 2.0;
        [
            (r02 + r20) / s,
            (r12 + r21) / s,
            0.25 * s,
            (r10 - r01) / s,
        ]
    };

    (translation, scale, quat)
}

pub fn mat4_mul_row_major(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0f32; 16];
    for r in 0..4 {
        for c in 0..4 {
            let mut sum = 0.0f32;
            for k in 0..4 {
                sum += a[r * 4 + k] * b[k * 4 + c];
            }
            out[r * 4 + c] = sum;
        }
    }
    out
}

pub fn transpose_4x4(m: &[f32; 16]) -> [f32; 16] {
    [
        m[0], m[4], m[8],  m[12],
        m[1], m[5], m[9],  m[13],
        m[2], m[6], m[10], m[14],
        m[3], m[7], m[11], m[15],
    ]
}

pub fn mat4_inverse_row_major(m: &[f32; 16]) -> Option<[f32; 16]> {
    let a = |r: usize, c: usize| m[r * 4 + c];
    let mut inv = [0f32; 16];

    inv[0] =
        a(1, 1) * a(2, 2) * a(3, 3) - a(1, 1) * a(2, 3) * a(3, 2)
            - a(2, 1) * a(1, 2) * a(3, 3) + a(2, 1) * a(1, 3) * a(3, 2)
            + a(3, 1) * a(1, 2) * a(2, 3) - a(3, 1) * a(1, 3) * a(2, 2);
    inv[1] =
        -a(0, 1) * a(2, 2) * a(3, 3) + a(0, 1) * a(2, 3) * a(3, 2)
            + a(2, 1) * a(0, 2) * a(3, 3) - a(2, 1) * a(0, 3) * a(3, 2)
            - a(3, 1) * a(0, 2) * a(2, 3) + a(3, 1) * a(0, 3) * a(2, 2);
    inv[2] =
        a(0, 1) * a(1, 2) * a(3, 3) - a(0, 1) * a(1, 3) * a(3, 2)
            - a(1, 1) * a(0, 2) * a(3, 3) + a(1, 1) * a(0, 3) * a(3, 2)
            + a(3, 1) * a(0, 2) * a(1, 3) - a(3, 1) * a(0, 3) * a(1, 2);
    inv[3] =
        -a(0, 1) * a(1, 2) * a(2, 3) + a(0, 1) * a(1, 3) * a(2, 2)
            + a(1, 1) * a(0, 2) * a(2, 3) - a(1, 1) * a(0, 3) * a(2, 2)
            - a(2, 1) * a(0, 2) * a(1, 3) + a(2, 1) * a(0, 3) * a(1, 2);

    inv[4] =
        -a(1, 0) * a(2, 2) * a(3, 3) + a(1, 0) * a(2, 3) * a(3, 2)
            + a(2, 0) * a(1, 2) * a(3, 3) - a(2, 0) * a(1, 3) * a(3, 2)
            - a(3, 0) * a(1, 2) * a(2, 3) + a(3, 0) * a(1, 3) * a(2, 2);
    inv[5] =
        a(0, 0) * a(2, 2) * a(3, 3) - a(0, 0) * a(2, 3) * a(3, 2)
            - a(2, 0) * a(0, 2) * a(3, 3) + a(2, 0) * a(0, 3) * a(3, 2)
            + a(3, 0) * a(0, 2) * a(2, 3) - a(3, 0) * a(0, 3) * a(2, 2);
    inv[6] =
        -a(0, 0) * a(1, 2) * a(3, 3) + a(0, 0) * a(1, 3) * a(3, 2)
            + a(1, 0) * a(0, 2) * a(3, 3) - a(1, 0) * a(0, 3) * a(3, 2)
            - a(3, 0) * a(0, 2) * a(1, 3) + a(3, 0) * a(0, 3) * a(1, 2);
    inv[7] =
        a(0, 0) * a(1, 2) * a(2, 3) - a(0, 0) * a(1, 3) * a(2, 2)
            - a(1, 0) * a(0, 2) * a(2, 3) + a(1, 0) * a(0, 3) * a(2, 2)
            + a(2, 0) * a(0, 2) * a(1, 3) - a(2, 0) * a(0, 3) * a(1, 2);

    inv[8] =
        a(1, 0) * a(2, 1) * a(3, 3) - a(1, 0) * a(2, 3) * a(3, 1)
            - a(2, 0) * a(1, 1) * a(3, 3) + a(2, 0) * a(1, 3) * a(3, 1)
            + a(3, 0) * a(1, 1) * a(2, 3) - a(3, 0) * a(1, 3) * a(2, 1);
    inv[9] =
        -a(0, 0) * a(2, 1) * a(3, 3) + a(0, 0) * a(2, 3) * a(3, 1)
            + a(2, 0) * a(0, 1) * a(3, 3) - a(2, 0) * a(0, 3) * a(3, 1)
            - a(3, 0) * a(0, 1) * a(2, 3) + a(3, 0) * a(0, 3) * a(2, 1);
    inv[10] =
        a(0, 0) * a(1, 1) * a(3, 3) - a(0, 0) * a(1, 3) * a(3, 1)
            - a(1, 0) * a(0, 1) * a(3, 3) + a(1, 0) * a(0, 3) * a(3, 1)
            + a(3, 0) * a(0, 1) * a(1, 3) - a(3, 0) * a(0, 3) * a(1, 1);
    inv[11] =
        -a(0, 0) * a(1, 1) * a(2, 3) + a(0, 0) * a(1, 3) * a(2, 1)
            + a(1, 0) * a(0, 1) * a(2, 3) - a(1, 0) * a(0, 3) * a(2, 1)
            - a(2, 0) * a(0, 1) * a(1, 3) + a(2, 0) * a(0, 3) * a(1, 1);

    inv[12] =
        -a(1, 0) * a(2, 1) * a(3, 2) + a(1, 0) * a(2, 2) * a(3, 1)
            + a(2, 0) * a(1, 1) * a(3, 2) - a(2, 0) * a(1, 2) * a(3, 1)
            - a(3, 0) * a(1, 1) * a(2, 2) + a(3, 0) * a(1, 2) * a(2, 1);
    inv[13] =
        a(0, 0) * a(2, 1) * a(3, 2) - a(0, 0) * a(2, 2) * a(3, 1)
            - a(2, 0) * a(0, 1) * a(3, 2) + a(2, 0) * a(0, 2) * a(3, 1)
            + a(3, 0) * a(0, 1) * a(2, 2) - a(3, 0) * a(0, 2) * a(2, 1);
    inv[14] =
        -a(0, 0) * a(1, 1) * a(3, 2) + a(0, 0) * a(1, 2) * a(3, 1)
            + a(1, 0) * a(0, 1) * a(3, 2) - a(1, 0) * a(0, 2) * a(3, 1)
            - a(3, 0) * a(0, 1) * a(1, 2) + a(3, 0) * a(0, 2) * a(1, 1);
    inv[15] =
        a(0, 0) * a(1, 1) * a(2, 2) - a(0, 0) * a(1, 2) * a(2, 1)
            - a(1, 0) * a(0, 1) * a(2, 2) + a(1, 0) * a(0, 2) * a(2, 1)
            + a(2, 0) * a(0, 1) * a(1, 2) - a(2, 0) * a(0, 2) * a(1, 1);

    let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    for v in inv.iter_mut() {
        *v *= inv_det;
    }
    Some(inv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn mat4_identity_mul_returns_other() {
        #[rustfmt::skip]
        let id = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
        #[rustfmt::skip]
        let other = [
            2.0, 0.0, 0.0, 0.0,
            0.0, 3.0, 0.0, 0.0,
            0.0, 0.0, 4.0, 0.0,
            5.0, 6.0, 7.0, 1.0,
        ];
        let r = mat4_mul_row_major(&id, &other);
        assert_eq!(r, other);
        let r2 = mat4_mul_row_major(&other, &id);
        assert_eq!(r2, other);
    }

    #[test]
    fn transpose_swaps_translation_row_and_col() {

        #[rustfmt::skip]
        let row = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            5.0, 6.0, 7.0, 1.0,
        ];

        let col = transpose_4x4(&row);
        assert!(approx(col[3], 5.0));
        assert!(approx(col[7], 6.0));
        assert!(approx(col[11], 7.0));
    }

    #[test]
    fn identity_decomposes_to_unit_quat() {
        #[rustfmt::skip]
        let m = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
        let (t, s, q) = decompose_row_major(&m);
        assert_eq!(t, [0.0, 0.0, 0.0]);
        assert_eq!(s, [1.0, 1.0, 1.0]);
        assert!(approx(q[3].abs(), 1.0));
    }

    #[test]
    fn translation_and_scale_recovered() {

        #[rustfmt::skip]
        let m = [
            2.0, 0.0, 0.0, 0.0,
            0.0, 3.0, 0.0, 0.0,
            0.0, 0.0, 4.0, 0.0,
            5.0, 6.0, 7.0, 1.0,
        ];
        let (t, s, _) = decompose_row_major(&m);
        assert_eq!(t, [5.0, 6.0, 7.0]);
        assert_eq!(s, [2.0, 3.0, 4.0]);
    }

    #[test]
    fn zero_euler_gives_identity_quat() {
        let q = zyx_euler_to_quat([0.0, 0.0, 0.0]);
        assert!(approx(q[0], 0.0));
        assert!(approx(q[1], 0.0));
        assert!(approx(q[2], 0.0));
        assert!(approx(q[3], 1.0));
    }
}

#[cfg(test)]
mod reflection_tests {
    use super::*;
    #[test]
    fn reflected_matrix_yields_unit_quaternion() {
        // identity rotation with X mirrored + nonuniform scale (row-major R*S)
        let m = [
            -0.66, 0.0, 0.0, 0.0,
            0.0, 0.59, 0.0, 0.0,
            0.0, 0.0, 0.92, 0.0,
            10.0, 20.0, 30.0, 1.0,
        ];
        let (t, s, q) = decompose_row_major(&m);
        assert_eq!(t, [10.0, 20.0, 30.0]);
        let qn = (q[0]*q[0]+q[1]*q[1]+q[2]*q[2]+q[3]*q[3]).sqrt();
        assert!((qn - 1.0).abs() < 1e-4, "quat must be unit, got |q|={qn}");
        // one scale axis carries the mirror sign
        let neg = [s[0] < 0.0, s[1] < 0.0, s[2] < 0.0].iter().filter(|b| **b).count();
        assert_eq!(neg, 1, "exactly one axis negative, got scale={s:?}");
    }
}
