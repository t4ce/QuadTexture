//! Camera projection shared in shape with the validated Picasso Example.
use trueos::vgpu::RetainedCamera;
use trueos_picasso::cam::{Camera, Projection, Quaternion};

/// Frame the complete imported gallery, with a five-percent border on each
/// screen edge and a slight oblique view that reveals the panels' thickness.
#[cfg(test)]
pub(crate) fn gallery_camera(
    bounds_min: [f32; 3],
    bounds_max: [f32; 3],
    viewport_width: u32,
    viewport_height: u32,
) -> Camera {
    framed_camera(
        bounds_min,
        bounds_max,
        viewport_width,
        viewport_height,
        [0.12, -0.08, 1.0],
    )
}

/// An elevated reset view fits both the upright catalog and its tiled floor.
pub(crate) fn floor_gallery_camera(
    bounds_min: [f32; 3],
    bounds_max: [f32; 3],
    viewport_width: u32,
    viewport_height: u32,
) -> Camera {
    framed_camera(
        bounds_min,
        bounds_max,
        viewport_width,
        viewport_height,
        [0.12, 0.65, 1.0],
    )
}

fn framed_camera(
    bounds_min: [f32; 3],
    bounds_max: [f32; 3],
    viewport_width: u32,
    viewport_height: u32,
    direction: [f32; 3],
) -> Camera {
    const NEAR: f32 = 0.05;
    const NDC_MARGIN: f32 = 0.90;
    let center: [f32; 3] = core::array::from_fn(|axis| (bounds_min[axis] + bounds_max[axis]) * 0.5);
    let rotation = look_at_camera_rotation(direction, [0.0; 3], [0.0, -1.0, 0.0]);
    let [x, y, z, w] = rotation.0;
    let inverse_rotation = Quaternion([-x, -y, -z, w]);
    let backward = rotation.rotate([0.0, 0.0, 1.0]);
    let yfov = core::f32::consts::FRAC_PI_3;
    let tan_y = libm::tanf(yfov * 0.5);
    let aspect = viewport_width.max(1) as f32 / viewport_height.max(1) as f32;
    let mut distance = NEAR * 2.0;
    let mut furthest_offset = 0.0f32;
    for corner in 0..8 {
        let offset = core::array::from_fn(|axis| {
            let coordinate = if corner & (1 << axis) == 0 {
                bounds_min[axis]
            } else {
                bounds_max[axis]
            };
            coordinate - center[axis]
        });
        let local = inverse_rotation.rotate(offset);
        // Corner depth is distance - local.z. Solve both perspective
        // inequalities directly, so the oblique corners fit as well as the
        // center plane at every viewport aspect.
        distance = distance.max(local[2] + local[0].abs() / (tan_y * aspect * NDC_MARGIN));
        distance = distance.max(local[2] + local[1].abs() / (tan_y * NDC_MARGIN));
        distance = distance.max(local[2] + NEAR * 2.0);
        furthest_offset = furthest_offset.max(-local[2]);
    }
    Camera {
        position: core::array::from_fn(|axis| center[axis] + backward[axis] * distance),
        rotation,
        projection: Projection::Perspective {
            yfov,
            znear: NEAR,
            zfar: Some(200.0f32.max(distance + furthest_offset + 1.0)),
            aspect_ratio: None,
        },
    }
}

pub(crate) fn look_at_camera_rotation(
    position: [f32; 3],
    target: [f32; 3],
    world_up: [f32; 3],
) -> Quaternion {
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

pub(crate) const fn identity_mat4() -> [f32; 16] {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 0.0002, "{actual} != {expected}");
    }

    fn transform(matrix: [f32; 16], point: [f32; 4]) -> [f32; 4] {
        core::array::from_fn(|row| {
            (0..4)
                .map(|column| matrix[column * 4 + row] * point[column])
                .sum()
        })
    }

    fn perspective() -> Camera {
        Camera {
            position: [0.0; 3],
            rotation: Quaternion::IDENTITY,
            projection: Projection::Perspective {
                yfov: core::f32::consts::FRAC_PI_2,
                znear: 0.25,
                zfar: Some(100.0),
                aspect_ratio: None,
            },
        }
    }

    #[test]
    fn perspective_preserves_w_and_maps_near_far_to_zero_one() {
        let camera = retained_camera(perspective(), 1200, 600, identity_mat4());
        for (distance, expected_depth) in [(0.25, 0.0), (100.0, 1.0)] {
            let clip = transform(camera.view_projection, [0.0, 0.0, -distance, 1.0]);
            assert_close(clip[3], distance);
            assert_close(clip[2] / clip[3], expected_depth);
        }
        let near = transform(camera.view_projection, [1.0, 0.0, -2.0, 1.0]);
        let far = transform(camera.view_projection, [1.0, 0.0, -4.0, 1.0]);
        assert_close(near[0] / near[3], 2.0 * far[0] / far[3]);
        assert!(transform(camera.view_projection, [0.0, 0.0, 1.0, 1.0])[3] < 0.0);
    }

    #[test]
    fn resize_changes_horizontal_field_without_moving_camera() {
        let initial = perspective();
        let square = retained_camera(initial, 600, 600, identity_mat4());
        let wide = retained_camera(initial, 1200, 600, square.view_projection);
        assert_eq!(square.view, wide.view);
        assert_eq!(square.position_near, wide.position_near);
        assert_close(square.projection[0], 2.0 * wide.projection[0]);
        assert_close(square.projection[5], wide.projection[5]);
        assert_eq!(wide.previous_view_projection, square.view_projection);
        let mut authored = initial;
        if let Projection::Perspective { aspect_ratio, .. } = &mut authored.projection {
            *aspect_ratio = Some(1.5);
        }
        assert_eq!(
            retained_camera(authored, 600, 600, identity_mat4()).projection,
            retained_camera(authored, 1200, 600, identity_mat4()).projection
        );
    }

    #[test]
    fn translated_rotated_camera_round_trips_world_and_clip_points() {
        let mut input = perspective();
        input.position = [2.0, -1.0, 4.0];
        input.rotation = Quaternion::from_axis_angle([0.0, 1.0, 0.0], 0.7)
            * Quaternion::from_axis_angle([1.0, 0.0, 0.0], -0.3);
        let camera = retained_camera(input, 960, 540, identity_mat4());
        let origin = transform(camera.view, [2.0, -1.0, 4.0, 1.0]);
        for (actual, expected) in origin.into_iter().zip([0.0, 0.0, 0.0, 1.0]) {
            assert_close(actual, expected);
        }
        for point in [[2.0, 3.0, -4.0, 1.0], [-3.0, -2.0, -8.0, 1.0]] {
            let clip = transform(camera.view_projection, point);
            let restored = transform(camera.inverse_view_projection, clip);
            for (actual, expected) in restored.into_iter().zip(point) {
                assert_close(actual, expected);
            }
        }
        for (actual, expected) in
            multiply_mat4(camera.view_projection, camera.inverse_view_projection)
                .into_iter()
                .zip(identity_mat4())
        {
            assert_close(actual, expected);
        }
    }

    #[test]
    fn gallery_look_at_keeps_target_centered_with_negative_y_up() {
        for position in [[2.0, -1.5, 10.0], [-10.0, 2.0, -5.0], [0.0, 0.0, -10.0]] {
            let target = [0.0; 3];
            let rotation = look_at_camera_rotation(position, target, [0.0, -1.0, 0.0]);
            let camera = retained_camera(
                Camera {
                    position,
                    rotation,
                    ..perspective()
                },
                640,
                360,
                identity_mat4(),
            );
            let clip = transform(camera.view_projection, [0.0, 0.0, 0.0, 1.0]);
            assert!(clip[3] > 0.0);
            assert_close(clip[0] / clip[3], 0.0);
            assert_close(clip[1] / clip[3], 0.0);
            let up = rotation.rotate([0.0, 1.0, 0.0]);
            assert!(up[1] < 0.0);
        }
    }

    #[test]
    fn gallery_framing_fits_every_corner_and_uses_available_screen_space() {
        let bounds_min = [-7.25, -4.75, -0.3];
        let bounds_max = [7.25, 4.75, 0.3];
        for (width, height) in [(640, 360), (360, 640), (600, 600)] {
            let input = gallery_camera(bounds_min, bounds_max, width, height);
            let camera = retained_camera(input, width, height, identity_mat4());
            let mut largest_ndc = 0.0f32;
            for corner in 0..8 {
                let point = core::array::from_fn(|axis| {
                    if axis == 3 {
                        1.0
                    } else if corner & (1 << axis) == 0 {
                        bounds_min[axis]
                    } else {
                        bounds_max[axis]
                    }
                });
                let clip = transform(camera.view_projection, point);
                assert!(clip[3] > 0.0);
                for coordinate in [clip[0] / clip[3], clip[1] / clip[3]] {
                    assert!(coordinate.abs() <= 0.9002, "{width}x{height}: {coordinate}");
                    largest_ndc = largest_ndc.max(coordinate.abs());
                }
                assert!((0.0..1.0).contains(&(clip[2] / clip[3])));
            }
            assert_close(largest_ndc, 0.9);
            assert!(input.rotation.rotate([0.0, 1.0, 0.0])[1] < 0.0);
        }
    }

    #[test]
    fn reset_frames_the_floor_from_above_at_landscape_and_portrait_aspects() {
        let bounds_min = [-16.0, -5.6, -0.3];
        let bounds_max = [16.0, 4.75, 32.0];
        for (width, height) in [(640, 360), (360, 640), (600, 600)] {
            let input = floor_gallery_camera(bounds_min, bounds_max, width, height);
            assert!(input.position[1] > bounds_max[1]);
            assert!(input.position[2] > bounds_max[2]);
            assert!(input.rotation.rotate([0.0, 0.0, -1.0])[1] < 0.0);
            let camera = retained_camera(input, width, height, identity_mat4());
            for corner in 0..8 {
                let point = core::array::from_fn(|axis| {
                    if axis == 3 {
                        1.0
                    } else if corner & (1 << axis) == 0 {
                        bounds_min[axis]
                    } else {
                        bounds_max[axis]
                    }
                });
                let clip = transform(camera.view_projection, point);
                assert!(clip[3] > 0.0);
                assert!((clip[0] / clip[3]).abs() <= 0.9002);
                assert!((clip[1] / clip[3]).abs() <= 0.9002);
                assert!((0.0..1.0).contains(&(clip[2] / clip[3])));
            }
        }
    }

    #[test]
    fn gallery_framing_is_translation_invariant_and_accepts_flat_bounds() {
        let input = gallery_camera([-7.25, -4.75, 0.0], [7.25, 4.75, 0.0], 640, 360);
        let offset = [20.0, -30.0, 10.0];
        let shifted = gallery_camera([12.75, -34.75, 10.0], [27.25, -25.25, 10.0], 640, 360);
        for axis in 0..3 {
            assert_close(shifted.position[axis] - input.position[axis], offset[axis]);
        }
        assert_eq!(input.rotation, shifted.rotation);
        assert_eq!(input.projection, shifted.projection);
        let distance = libm::sqrtf(input.position.into_iter().map(|value| value * value).sum());
        assert!(
            distance < 12.0,
            "gallery should fill the initial window: distance={distance}"
        );
    }

    #[test]
    fn infinite_perspective_and_orthographic_depth_are_finite() {
        let mut input = perspective();
        if let Projection::Perspective { zfar, .. } = &mut input.projection {
            *zfar = None;
        }
        let infinite = retained_camera(input, 640, 360, identity_mat4());
        let near = transform(infinite.projection, [0.0, 0.0, -0.25, 1.0]);
        assert_close(near[2] / near[3], 0.0);
        let distant = transform(infinite.projection, [0.0, 0.0, -10_000.0, 1.0]);
        assert!(distant[2] / distant[3] < 1.0);
        assert!(
            infinite
                .inverse_view_projection
                .into_iter()
                .all(f32::is_finite)
        );
        input.projection = Projection::Orthographic {
            xmag: 2.0,
            ymag: 3.0,
            znear: 0.25,
            zfar: 100.0,
        };
        let orthographic = retained_camera(input, 640, 360, identity_mat4());
        for (distance, depth) in [(0.25, 0.0), (100.0, 1.0)] {
            let clip = transform(orthographic.projection, [2.0, 3.0, -distance, 1.0]);
            for (actual, expected) in clip.into_iter().zip([1.0, 1.0, depth, 1.0]) {
                assert_close(actual, expected);
            }
        }
        assert!(invert_mat4([0.0; 16]).is_none());
    }
}
