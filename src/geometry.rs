pub const VERTEX_STRIDE_BYTES: usize = 20;
pub const VERTICES: [[f32; 5]; 4] = [
    [-0.95, -0.95, 0.0, 0.0, 1.0],
    [0.95, -0.95, 0.0, 1.0, 1.0],
    [0.95, 0.95, 0.0, 1.0, 0.0],
    [-0.95, 0.95, 0.0, 0.0, 0.0],
];

// The first four indices form one native quad. All six form two triangles
// over those same vertices, UVs, and winding for the key 3 comparison.
pub const INDICES: [u32; 6] = [0, 1, 2, 3, 0, 2];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DrawMode {
    Quad,
    #[default]
    Triangles,
}

impl DrawMode {
    pub fn key_event(self, codepoint: u32, pressed: bool) -> Self {
        if !pressed {
            return self;
        }
        match codepoint {
            value if value == '1' as u32 => Self::Quad,
            value if value == '3' as u32 => Self::Triangles,
            _ => self,
        }
    }

    pub const fn index_count(self) -> u32 {
        match self {
            Self::Quad => 4,
            Self::Triangles => 6,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Quad => "one textured native quad",
            Self::Triangles => "GLB triangle gallery",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_gallery_can_switch_to_quad_and_back() {
        let initial = DrawMode::default();
        assert_eq!(initial, DrawMode::Triangles);
        let quad = initial.key_event('1' as u32, true);
        assert_eq!(quad, DrawMode::Quad);
        assert_eq!(quad.index_count(), 4);
        let triangles = quad.key_event('3' as u32, true);
        assert_eq!(triangles, DrawMode::Triangles);
        assert_eq!(triangles.index_count(), 6);
        assert_eq!(triangles.key_event('1' as u32, true), quad);
    }

    #[test]
    fn releases_other_keys_and_repeated_presses_preserve_mode() {
        for mode in [DrawMode::Quad, DrawMode::Triangles] {
            for key in ['1', '3'] {
                assert_eq!(mode.key_event(key as u32, false), mode);
            }
            assert_eq!(mode.key_event('2' as u32, true), mode);
        }
        assert_eq!(DrawMode::Quad.key_event('1' as u32, true), DrawMode::Quad);
        assert_eq!(
            DrawMode::Triangles.key_event('3' as u32, true),
            DrawMode::Triangles,
        );
    }

    #[test]
    fn native_quad_uses_four_distinct_corners_with_full_image_uvs() {
        let quad_indices = &INDICES[..DrawMode::Quad.index_count() as usize];
        assert_eq!(quad_indices, &[0, 1, 2, 3]);
        let uv_corners: Vec<_> = quad_indices
            .iter()
            .map(|&index| (VERTICES[index as usize][3], VERTICES[index as usize][4]))
            .collect();
        assert_eq!(uv_corners, [(0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)]);
        assert_eq!(core::mem::size_of_val(&VERTICES[0]), VERTEX_STRIDE_BYTES);
    }

    #[test]
    fn triangle_comparison_covers_same_quad_with_same_uv_orientation() {
        let area2 = |indices: &[u32], axis: usize| -> f32 {
            indices
                .iter()
                .enumerate()
                .map(|(i, &index)| {
                    let a = VERTICES[index as usize];
                    let b = VERTICES[indices[(i + 1) % indices.len()] as usize];
                    a[axis] * b[axis + 1] - b[axis] * a[axis + 1]
                })
                .sum()
        };
        let quad = &INDICES[..DrawMode::Quad.index_count() as usize];
        let triangles = &INDICES[..DrawMode::Triangles.index_count() as usize];
        for axis in [0, 3] {
            let quad_area = area2(quad, axis);
            let mut triangle_area = 0.0;
            for triangle in triangles.chunks_exact(3) {
                assert!(triangle.iter().all(|index| quad.contains(index)));
                let area = area2(triangle, axis);
                assert_eq!(area.signum(), quad_area.signum());
                triangle_area += area;
            }
            assert!((triangle_area - quad_area).abs() < 0.00001);
        }
    }
}
