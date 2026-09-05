pub const VERTEX_STRIDE_BYTES: usize = 20;
pub const VERTICES: [[f32; 5]; 4] = [
    [-0.95, -0.95, 0.0, 0.0, 1.0],
    [0.95, -0.95, 0.0, 1.0, 1.0],
    [0.95, 0.95, 0.0, 1.0, 0.0],
    [-0.95, 0.95, 0.0, 0.0, 0.0],
];

// Key 1 keeps the native quad probe. The gallery has its own persisted
// triangle mesh prepared from the source GLBs.
pub const INDICES: [u32; 4] = [0, 1, 2, 3];

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
        let triangles = quad.key_event('3' as u32, true);
        assert_eq!(triangles, DrawMode::Triangles);
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
        let quad_indices = &INDICES;
        assert_eq!(quad_indices, &[0, 1, 2, 3]);
        let uv_corners: Vec<_> = quad_indices
            .iter()
            .map(|&index| (VERTICES[index as usize][3], VERTICES[index as usize][4]))
            .collect();
        assert_eq!(uv_corners, [(0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)]);
        assert_eq!(core::mem::size_of_val(&VERTICES[0]), VERTEX_STRIDE_BYTES);
    }
}
