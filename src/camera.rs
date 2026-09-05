//! Camera projection shared in shape with the validated Picasso Example.
use trueos::vgpu::RetainedCamera;
use trueos_picasso::cam::{Camera, Projection, Quaternion};

pub(crate) fn look_at_camera_rotation(position: [f32; 3], target: [f32; 3], world_up: [f32; 3]) -> Quaternion {
    let forward = [
        target[0] - position[0],
        target[1] - position[1],
        target[2] - position[2],
    ];
    let forward_length =
        libm::sqrtf(forward[0] * forward[0] + forward[1] * forward[1] + forward[2] * forward[2]);
    let forward = [
        forward[0] / forward_length,
        forward[1] / forward_length,
        forward[2] / forward_length,
    ];
    // Local +X is camera right; local +Y completes the orthonormal frame.
    let right = [
        forward[1] * world_up[2] - forward[2] * world_up[1],
        forward[2] * world_up[0] - forward[0] * world_up[2],
        forward[0] * world_up[1] - forward[1] * world_up[0],
    ];
    let right_length = libm::sqrtf(right[0] * right[0] + right[1] * right[1] + right[2] * right[2]);
    let right = [
        right[0] / right_length,
        right[1] / right_length,
        right[2] / right_length,
    ];
    let up = [
        right[1] * forward[2] - right[2] * forward[1],
        right[2] * forward[0] - right[0] * forward[2],
        right[0] * forward[1] - right[1] * forward[0],
    ];
    // Matrix columns map local +X, +Y, +Z to world right, up, and -forward.
    let (m00, m01, m02) = (right[0], up[0], -forward[0]);
    let (m10, m11, m12) = (right[1], up[1], -forward[1]);
    let (m20, m21, m22) = (right[2], up[2], -forward[2]);
    let trace = m00 + m11 + m22;
    let rotation = if trace > 0.0 {
        let s = libm::sqrtf(trace + 1.0) * 2.0;
        Quaternion([(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s])
    } else if m00 > m11 && m00 > m22 {
        let s = libm::sqrtf(1.0 + m00 - m11 - m22) * 2.0;
        Quaternion([0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s])
    } else if m11 > m22 {
        let s = libm::sqrtf(1.0 + m11 - m00 - m22) * 2.0;
        Quaternion([(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s])
    } else {
        let s = libm::sqrtf(1.0 + m22 - m00 - m11) * 2.0;
        Quaternion([(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s])
    };
    rotation.normalized()
}

pub(crate) fn retained_camera(
    camera: Camera,
    viewport_width: u32,
    viewport_height: u32,
    previous_view_projection: [f32; 16],
) -> RetainedCamera {
    let [qx, qy, qz, qw] = camera.rotation.normalized().0;
    let world_to_view = Quaternion([-qx, -qy, -qz, qw]);
    let x_axis = world_to_view.rotate([1.0, 0.0, 0.0]);
    let y_axis = world_to_view.rotate([0.0, 1.0, 0.0]);
    let z_axis = world_to_view.rotate([0.0, 0.0, 1.0]);
    let translation = world_to_view.rotate([
        -camera.position[0],
        -camera.position[1],
        -camera.position[2],
    ]);
    // WGSL matrices are column-major. `world_to_view.rotate(e_i)` is column i
    // of the rotation, and the fourth column supplies camera translation.
    let view = [
        x_axis[0],
        x_axis[1],
        x_axis[2],
        0.0,
        y_axis[0],
        y_axis[1],
        y_axis[2],
        0.0,
        z_axis[0],
        z_axis[1],
        z_axis[2],
        0.0,
        translation[0],
        translation[1],
        translation[2],
        1.0,
    ];
    let (projection, znear, zfar) = match camera.projection {
        Projection::Perspective {
            yfov,
            znear,
            zfar,
            aspect_ratio,
        } => {
            let aspect = aspect_ratio
                .unwrap_or_else(|| viewport_width as f32 / viewport_height.max(1) as f32);
            let focal_y = 1.0 / libm::tanf(yfov * 0.5);
            let focal_x = focal_y / aspect.max(f32::EPSILON);
            let (depth_scale, depth_offset) = match zfar {
                Some(zfar) => (zfar / (znear - zfar), zfar * znear / (znear - zfar)),
                None => (-1.0, -znear),
            };
            (
                [
                    focal_x,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    focal_y,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    depth_scale,
                    -1.0,
                    0.0,
                    0.0,
                    depth_offset,
                    0.0,
                ],
                znear,
                zfar.unwrap_or(f32::MAX),
            )
        }
        Projection::Orthographic {
            xmag,
            ymag,
            znear,
            zfar,
        } => {
            let range = (zfar - znear).max(f32::EPSILON);
            (
                [
                    1.0 / xmag.max(f32::EPSILON),
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0 / ymag.max(f32::EPSILON),
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    -1.0 / range,
                    0.0,
                    0.0,
                    0.0,
                    -znear / range,
                    1.0,
                ],
                znear,
                zfar,
            )
        }
    };
    let view_projection = multiply_mat4(projection, view);
    let inverse_view_projection = invert_mat4(view_projection).unwrap_or(identity_mat4());
    let forward = camera.rotation.rotate([0.0, 0.0, -1.0]);
    RetainedCamera {
        view,
        projection,
        view_projection,
        inverse_view_projection,
        position_near: [
            camera.position[0],
            camera.position[1],
            camera.position[2],
            znear,
        ],
        forward_far: [forward[0], forward[1], forward[2], zfar],
        jitter_frame: [0.0; 4],
        previous_view_projection,
    }
}

const pub(crate) fn identity_mat4() -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

pub(crate) fn multiply_mat4(left: [f32; 16], right: [f32; 16]) -> [f32; 16] {
    let mut product = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            product[column * 4 + row] = (0..4)
                .map(|inner| left[inner * 4 + row] * right[column * 4 + inner])
                .sum();
        }
    }
    product
}

pub(crate) fn invert_mat4(matrix: [f32; 16]) -> Option<[f32; 16]> {
    let mut augmented = [[0.0; 8]; 4];
    for row in 0..4 {
        for column in 0..4 {
            augmented[row][column] = matrix[column * 4 + row];
            augmented[row][column + 4] = if row == column { 1.0 } else { 0.0 };
        }
    }
    for pivot_column in 0..4 {
        let mut pivot_row = pivot_column;
        for candidate in pivot_column + 1..4 {
            if libm::fabsf(augmented[candidate][pivot_column])
                > libm::fabsf(augmented[pivot_row][pivot_column])
            {
                pivot_row = candidate;
            }
        }
        let pivot = augmented[pivot_row][pivot_column];
        if libm::fabsf(pivot) <= f32::EPSILON {
            return None;
        }
        augmented.swap(pivot_column, pivot_row);
        for value in &mut augmented[pivot_column] {
            *value /= pivot;
        }
        for row in 0..4 {
            if row == pivot_column {
                continue;
            }
            let factor = augmented[row][pivot_column];
            for column in 0..8 {
                augmented[row][column] -= factor * augmented[pivot_column][column];
            }
        }
    }
    let mut inverse = [0.0; 16];
    for row in 0..4 {
        for column in 0..4 {
            inverse[column * 4 + row] = augmented[row][column + 4];
        }
    }
    Some(inverse)
}
