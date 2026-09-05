//! Retained triangle scene using the same camera/material path as Picasso Example.
use super::{DemoError, camera, write_exact};
use trueos::ui4_scene::Frame;
use trueos::vgpu::{
    BUFFER_USAGE_INDEX, BUFFER_USAGE_MAP_WRITE, BUFFER_USAGE_VERTEX, Buffer, Device,
    PRIMITIVE_TOPOLOGY_TRIANGLE_LIST, Queue, RETAINED_VERTEX_LAYOUT_POS_NORMAL_UV_TANGENT,
    RetainedFrameSubmit, RetainedFrameSubmitV2, RetainedMaterial, RetainedMaterialParameters,
    RetainedMesh, RetainedMeshDescriptor, RetainedTransformSeed, TimelinePoint, Ui4Surface,
};
use trueos::{clock, logl::{self, level}, vmedia::RetainedTexture};
use trueos_picasso::{Picasso, cam::{Camera, FlyCam, Projection}};

mod prepared {
    include!(concat!(env!("OUT_DIR"), "/tile_scene_meta.rs"));
}

pub struct Gallery {
    // Runtime redb owns the prepared bytes independently of GPU residency.
    // The build's source/derived database is the durable import boundary.
    _assets: Picasso,
    vertices: Buffer,
    indices: Buffer,
    mesh: RetainedMesh,
    base_color: RetainedTexture,
    orm: RetainedTexture,
    normal: RetainedTexture,
    flycam: FlyCam,
    initial_camera: Camera,
    previous_view_projection: [f32; 16],
    submitted_view_projection: [f32; 16],
    previous_millis: u64,
    radius: f32,
    frames: u64,
}

impl Gallery {
    pub fn open(device: Device, width: u32, height: u32) -> Result<Self, DemoError> {
        use prepared::*;
        if SCENE_VERTICES.len() != SCENE_VERTEX_COUNT as usize * 48
            || SCENE_INDICES.len() != SCENE_INDEX_COUNT as usize * 4
            || SCENE_INDEX_COUNT == 0 || SCENE_INDEX_COUNT % 3 != 0
        {
            return Err(DemoError::Contract("prepared scene layout"));
        }
        let assets = Picasso::new().map_err(|_| DemoError::Contract("Picasso runtime database"))?;
        let store = |name: &str, source: &[u8]| {
            assets.put_embedded_asset(name, source)
                .map_err(|_| DemoError::Contract("Picasso asset insert"))?;
            let bytes = assets.embedded_asset(name)
                .map_err(|_| DemoError::Contract("Picasso asset read"))?
                .ok_or(DemoError::Contract("Picasso asset missing"))?;
            if bytes != source {
                return Err(DemoError::Contract("Picasso asset round trip"));
            }
            Ok(bytes)
        };
        store("scene/metadata.json", SCENE_METADATA.as_bytes())?;
        let vertex_bytes = store("scene/vertices.pnut", SCENE_VERTICES)?;
        let index_bytes = store("scene/indices.u32", SCENE_INDICES)?;
        let vertices = device.create_buffer(vertex_bytes.len(), BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_VERTEX)
            .map_err(|code| DemoError::Vgpu("gallery-vertex-create", code))?;
        let indices = device.create_buffer(index_bytes.len(), BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_INDEX)
            .map_err(|code| DemoError::Vgpu("gallery-index-create", code))?;
        write_exact(device, vertices, &vertex_bytes).map_err(|code| DemoError::Vgpu("gallery-vertex-upload", code))?;
        write_exact(device, indices, &index_bytes).map_err(|code| DemoError::Vgpu("gallery-index-upload", code))?;
        let mesh = device.create_retained_mesh(vertices, indices, RetainedMeshDescriptor {
            vertex_count: SCENE_VERTEX_COUNT,
            index_count: SCENE_INDEX_COUNT,
            vertex_layout: RETAINED_VERTEX_LAYOUT_POS_NORMAL_UV_TANGENT,
            topology: PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
            ..RetainedMeshDescriptor::default()
        }).map_err(|code| DemoError::Vgpu("gallery-retained-mesh-create", code))?;
        let decode = |name: &str, source: &[u8]| {
            let encoded = store(name, source)?;
            logl::log(level::INFO, format_args!("QuadTexture: atlas loading role={} encoded_bytes={} source=picasso-runtime-db", name, encoded.len()));
            let texture = trueos::async_fs::block_on(trueos::vmedia::decode_retained_asset(device, name, &encoded))
                .map_err(|code| DemoError::Vgpu("gallery-atlas-decode", code))?;
            let info = texture.info();
            if info.width != ATLAS_WIDTH || info.height != ATLAS_HEIGHT {
                return Err(DemoError::Contract("atlas decoded extent"));
            }
            logl::log(level::INFO, format_args!("QuadTexture: atlas resident role={} size={}x{} texture=0x{:X}", name, info.width, info.height, texture.id().raw()));
            Ok(texture)
        };
        // Decode sequentially: only one temporary RGBA atlas is live at once.
        let base_color = decode("scene/base-color.png", BASE_COLOR_PNG)?;
        let orm = decode("scene/orm.png", ORM_PNG)?;
        let normal = decode("scene/normal.png", NORMAL_PNG)?;
        let center = core::array::from_fn(|i| (SCENE_BOUNDS_MIN[i] + SCENE_BOUNDS_MAX[i]) * 0.5);
        let extent = (SCENE_BOUNDS_MAX[0] - SCENE_BOUNDS_MIN[0]).max(SCENE_BOUNDS_MAX[1] - SCENE_BOUNDS_MIN[1]);
        let distance = extent.max(1.0) * 1.3;
        let position = [center[0] + distance * 0.2, center[1] - distance * 0.15, center[2] + distance];
        let initial_camera = Camera {
            position,
            rotation: camera::look_at_camera_rotation(position, center, [0.0, -1.0, 0.0]),
            projection: Projection::Perspective {
                yfov: core::f32::consts::FRAC_PI_3, znear: 0.05, zfar: Some(200.0), aspect_ratio: None,
            },
        };
        let previous_view_projection = camera::retained_camera(initial_camera, width, height, camera::identity_mat4()).view_projection;
        let radius = libm::sqrtf((0..3).map(|i| {
            let distance = SCENE_BOUNDS_MIN[i].abs().max(SCENE_BOUNDS_MAX[i].abs());
            distance * distance
        }).sum::<f32>()) + 0.01;
        logl::log(level::INFO, format_args!("QuadTexture: gallery ready assets={} vertices={} triangles={} layout=6x4 material=base+ORM+normal source=durable-picasso-import runtime=ephemeral-redb", SCENE_ASSET_COUNT, SCENE_VERTEX_COUNT, SCENE_INDEX_COUNT / 3));
        Ok(Self {
            _assets: assets, vertices, indices, mesh, base_color, orm, normal,
            flycam: FlyCam::new(initial_camera, 4.0), initial_camera,
            previous_view_projection, submitted_view_projection: previous_view_projection,
            previous_millis: clock::monotonic_millis(), radius, frames: 0,
        })
    }

    pub fn reset_camera(&mut self) {
        self.flycam.camera = self.initial_camera;
    }

    pub fn service_input(&mut self, frame: &Frame, active: bool) -> Result<(), DemoError> {
        let now = clock::monotonic_millis();
        let delta = now.saturating_sub(self.previous_millis).min(100) as f32 * 0.001;
        self.previous_millis = now;
        self.flycam.step_ui4(frame, if active { delta } else { 0.0 })
            .map_err(|error| DemoError::Ui4("gallery-flycam", error))?;
        while let Some(event) = frame.take_pointer_event()
            .map_err(|error| DemoError::Ui4("gallery-pointer", error))?
        {
            self.flycam.handle_ui4_pointer_event(&event, active);
        }
        Ok(())
    }

    pub fn submit(&mut self, device: Device, queue: Queue, surface: Ui4Surface, width: u32, height: u32) -> Result<TimelinePoint, DemoError> {
        let camera = camera::retained_camera(self.flycam.camera, width, height, self.previous_view_projection);
        let mut frame = RetainedFrameSubmit {
            camera, seed_count: 1,
            clear_rgba8_srgb: u32::from_le_bytes([12, 16, 24, 255]),
            material: RetainedMaterial {
                // glTF's packed image serves two roles, sharing one residency.
                textures: [self.base_color.id().raw(), self.orm.id().raw(), 0, self.orm.id().raw(), self.normal.id().raw()],
                ..RetainedMaterial::default()
            },
            ..RetainedFrameSubmit::default()
        };
        frame.seeds[0] = RetainedTransformSeed {
            scale: [1.0; 3], rotation: [0.0, 0.0, 0.0, 1.0], local_radius: self.radius,
            ..RetainedTransformSeed::default()
        };
        let point = device.submit_retained_frame_v2(queue, surface, self.mesh, self.vertices, self.indices,
            RetainedFrameSubmitV2 { frame, material_parameters: RetainedMaterialParameters::default() })
            .map_err(|code| DemoError::Vgpu("gallery-retained-submit", code))?;
        self.submitted_view_projection = camera.view_projection;
        Ok(point)
    }

    pub fn published(&mut self, timeline: u64) {
        self.previous_view_projection = self.submitted_view_projection;
        self.frames += 1;
        if self.frames == 1 || self.frames % 256 == 0 {
            logl::log(level::INFO, format_args!("QuadTexture: gallery frame published assets={} triangles={} topology=TriangleList timeline={} frame={}", prepared::SCENE_ASSET_COUNT, prepared::SCENE_INDEX_COUNT / 3, timeline, self.frames));
        }
    }
}
