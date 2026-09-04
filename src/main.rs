#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

use trueos::input::KEYBOARD_OUTPUT_FLAG_PRESS;
use trueos::ui4_scene::{Damage, Error as Ui4Error, Frame, ResizeEvent};
use trueos::vgpu::{
    BUFFER_USAGE_INDEX, BUFFER_USAGE_MAP_WRITE, BUFFER_USAGE_VERTEX, Buffer, Capabilities, Device,
    IndexedBatchDrawV2, IndexedDraw, IndexedDrawBatchV2, Queue, QueueClass, RenderPipeline,
    ShaderModule,
    PRIMITIVE_TOPOLOGY_QUAD_LIST, PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
    SAMPLER_FLAGS_ALL, SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64,
    SHADER_PACKAGE_CLIP_POSITION3_UV_TEXTURE_FNV1A64,
};
use trueos::{logl::{self, level}, vshell, vsys};

include!(concat!(env!("OUT_DIR"), "/intel_logo_meta.rs"));

const INTEL_LOGO_RGBA8: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/intel_logo_rgba8.bin"));

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const FRAME_X: i32 = 96;
const FRAME_Y: i32 = 72;
const CLEAR_RGBA8_SRGB: u32 = u32::from_le_bytes([0, 0, 0, 255]);
const VERTEX_STRIDE_BYTES: usize = 20;
const VERTEX_COUNT: usize = 4;
const QUAD_INDEX_COUNT: usize = 4;
const TRIANGLE_INDEX_COUNT: usize = 6;

const VERTICES: [[f32; 5]; VERTEX_COUNT] = [
    [-0.95, -0.95, 0.0, 0.0, 1.0],
    [0.95, -0.95, 0.0, 1.0, 1.0],
    [0.95, 0.95, 0.0, 1.0, 0.0],
    [-0.95, 0.95, 0.0, 0.0, 0.0],
];
const INDICES: [u32; TRIANGLE_INDEX_COUNT] = [0, 1, 2, 3, 0, 2];

#[global_allocator]
static ALLOCATOR: trueos::TrueosAllocator = trueos::TrueosAllocator;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    trueos::panic_abort("QuadTexture panic\n")
}

struct QuadTexture {
    frame: Frame,
    device: Device,
    queue: Queue,
    _shader: ShaderModule,
    pipeline: RenderPipeline,
    _texture_shader: ShaderModule,
    texture_pipeline: RenderPipeline,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    texture_buffer: Buffer,
    draw_batch: IndexedDrawBatchV2,
    timeline: u64,
    pending_resize: Option<ResizeEvent>,
    triangles: bool,
}

impl QuadTexture {
    fn open() -> Result<Self, DemoError> {
        let frame = Frame::open_streaming(FRAME_X, FRAME_Y, WIDTH, HEIGHT).map_err(|error| {
            DemoError::Ui4("frame-open", error)
        })?;
        let device = Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))
            .map_err(|error| DemoError::Vgpu("device-open", error))?;
        let queue = device
            .create_queue(QueueClass::Render)
            .map_err(|error| DemoError::Vgpu("queue-create", error))?;
        let shader = device
            .create_shader_module(SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64)
            .map_err(|error| DemoError::Vgpu("shader-create", error))?;
        let pipeline = device
            .create_render_pipeline(shader, VERTEX_STRIDE_BYTES as u32, 0)
            .map_err(|error| DemoError::Vgpu("pipeline-create", error))?;
        let texture_shader = device
            .create_shader_module(SHADER_PACKAGE_CLIP_POSITION3_UV_TEXTURE_FNV1A64)
            .map_err(|error| DemoError::Vgpu("texture-shader-create", error))?;
        let texture_pipeline = device
            .create_render_pipeline(texture_shader, VERTEX_STRIDE_BYTES as u32, 0)
            .map_err(|error| DemoError::Vgpu("texture-pipeline-create", error))?;

        let vertex_buffer = device
            .create_buffer(vertex_bytes().len(), BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_VERTEX)
            .map_err(|error| DemoError::Vgpu("vertex-buffer-create", error))?;
        let index_buffer = device
            .create_buffer(index_bytes().len(), BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_INDEX)
            .map_err(|error| DemoError::Vgpu("index-buffer-create", error))?;
        let texture_buffer = device
            .create_buffer(INTEL_LOGO_RGBA8.len(), BUFFER_USAGE_MAP_WRITE)
            .map_err(|error| DemoError::Vgpu("texture-buffer-create", error))?;

        write_exact(device, vertex_buffer, &vertex_bytes())
            .map_err(|error| DemoError::Vgpu("vertex-upload", error))?;
        write_exact(device, index_buffer, &index_bytes())
            .map_err(|error| DemoError::Vgpu("index-upload", error))?;
        write_exact(device, texture_buffer, INTEL_LOGO_RGBA8)
            .map_err(|error| DemoError::Vgpu("texture-upload", error))?;

        let mut draw_batch = IndexedDrawBatchV2 {
            clear_rgba8_srgb: CLEAR_RGBA8_SRGB,
            draw_count: 1,
            draws: [IndexedBatchDrawV2::default(); trueos::vgpu::MAX_INDEXED_BATCH_V2_DRAWS],
            ..IndexedDrawBatchV2::default()
        };
        draw_batch.draws[0] = IndexedBatchDrawV2 {
            index_count: QUAD_INDEX_COUNT as u32,
            first_index: 0,
            base_vertex: 0,
            rgba8_srgb: u32::from_le_bytes([255, 255, 255, 255]),
            topology: PRIMITIVE_TOPOLOGY_QUAD_LIST,
            reserved: 0,
        };

        Ok(Self {
            frame,
            device,
            queue,
            _shader: shader,
            pipeline,
            _texture_shader: texture_shader,
            texture_pipeline,
            vertex_buffer,
            index_buffer,
            texture_buffer,
            draw_batch,
            timeline: 0,
            pending_resize: None,
            triangles: false,
        })
    }

    fn service_keyboard_events(&mut self) -> Result<(), DemoError> {
        while let Some(event) = self
            .frame
            .take_keyboard_event()
            .map_err(|error| DemoError::Ui4("keyboard-event-take", error))?
        {
            if event.flags & KEYBOARD_OUTPUT_FLAG_PRESS != 0 && event.codepoint == '3' as u32 {
                if self.triangles {
                    continue;
                }
                self.triangles = true;
                self.draw_batch.draws[0].index_count = TRIANGLE_INDEX_COUNT as u32;
                self.draw_batch.draws[0].topology = PRIMITIVE_TOPOLOGY_TRIANGLE_LIST;
                logl::log(level::INFO, "QuadTexture: switched to two triangles");
            }
        }
        Ok(())
    }

    fn render_frame(&mut self) -> Result<(), DemoError> {
        self.service_resize_events()?;

        let width = self.frame.width();
        let height = self.frame.height();
        self.frame
            .begin_gpu_frame()
            .map_err(|error| DemoError::Ui4("frame-begin", error))?;
        let surface = self
            .device
            .acquire_ui4_surface(self.frame.window_id())
            .map_err(|error| DemoError::Vgpu("surface-acquire", error))?;

        let point = if self.triangles {
            self.device
                .submit_ui4_indexed(
                    self.queue,
                    surface,
                    self.texture_pipeline,
                    self.vertex_buffer,
                    self.index_buffer,
                    IndexedDraw {
                        index_count: TRIANGLE_INDEX_COUNT as u32,
                        clear_rgba8_srgb: CLEAR_RGBA8_SRGB,
                        sampled_texture: self.texture_buffer.raw(),
                        texture_width: INTEL_LOGO_WIDTH,
                        texture_height: INTEL_LOGO_HEIGHT,
                        texture_pitch: INTEL_LOGO_WIDTH * 4,
                        sampler_flags: SAMPLER_FLAGS_ALL,
                        ..IndexedDraw::default()
                    },
                )
                .map_err(|error| DemoError::Vgpu("textured-indexed-submit", error))?
        } else {
            self.device
                .submit_ui4_indexed_batch_v2(
                    self.queue,
                    surface,
                    self.pipeline,
                    self.vertex_buffer,
                    self.index_buffer,
                    self.draw_batch,
                )
                .map_err(|error| DemoError::Vgpu("indexed-batch-v2-submit", error))?
        };

        self.device
            .wait(self.queue, point.value)
            .map_err(|code| DemoError::Vgpu("timeline-wait", code))?;
        self.frame
            .publish(Damage::full(width, height))
            .map_err(|error| DemoError::Ui4("frame-publish", error))?;

        self.timeline = point.value;
        Ok(())
    }

    fn service_resize_events(&mut self) -> Result<(), DemoError> {
        while let Some(event) = self
            .frame
            .take_resize_event()
            .map_err(|error| DemoError::Ui4("resize-event-take", error))?
        {
            self.pending_resize = Some(event);
        }

        let Some(event) = self.pending_resize else {
            return Ok(());
        };

        if (event.width, event.height) == (self.frame.width(), self.frame.height()) {
            self.pending_resize = None;
            return Ok(());
        }

        match self.frame.resize(event.width, event.height) {
            Ok(()) => {
                self.pending_resize = None;
            }
            Err(Ui4Error::Busy) => {}
            Err(error) => return Err(DemoError::Ui4("frame-resize", error)),
        }

        Ok(())
    }

}

fn vertex_bytes() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(VERTICES.len() * VERTEX_STRIDE_BYTES);
    for vertex in VERTICES {
        for axis in vertex {
            bytes.extend_from_slice(&axis.to_le_bytes());
        }
    }
    bytes
}

fn index_bytes() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(TRIANGLE_INDEX_COUNT * core::mem::size_of::<u32>());
    for index in INDICES {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes
}

fn write_exact(device: Device, buffer: Buffer, bytes: &[u8]) -> Result<(), i32> {
    let written = device.write_buffer(buffer, 0, bytes)?;
    (written == bytes.len())
        .then_some(())
        .ok_or(trueos::vgpu::ERR_IO)
}

#[derive(Debug)]
enum DemoError {
    Ui4(&'static str, Ui4Error),
    Vgpu(&'static str, i32),
}

impl fmt::Display for DemoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ui4(stage, error) => write!(f, "UI4 {stage} failed: {error:?}"),
            Self::Vgpu(stage, error) => write!(f, "vGPU {stage} failed: {error}"),
        }
    }
}

fn main() {
    if let Err(error) = run() {
        logl::log(level::ERROR, format_args!("QuadTexture: fatal error: {error}"));
        if !vshell::shutdown_current_blueprint("QuadTexture terminated after a fatal error") {
            logl::log(level::ERROR, "QuadTexture: could not request Blueprint shutdown");
        }
    }
}

fn run() -> Result<(), DemoError> {
    let mut app = QuadTexture::open()?;

    loop {
        vsys::poll_once();
        app.service_keyboard_events()?;
        match app.render_frame() {
            Ok(()) => break,
            Err(error) if transient_frame_error(&error) => vsys::sleep_ms(16),
            Err(error) => return Err(error),
        }
    }

    logl::log(
        level::INFO,
        format_args!(
            "QuadTexture: single quad rendered. frame={}x{} timeline={}",
            WIDTH,
            HEIGHT,
            app.timeline,
        ),
    );

    loop {
        vsys::poll_once();
        app.service_keyboard_events()?;
        match app.render_frame() {
            Ok(()) => {}
            Err(error) if transient_frame_error(&error) => {}
            Err(error) => return Err(error),
        }
        vsys::sleep_ms(16);
    }
}

fn transient_frame_error(error: &DemoError) -> bool {
    matches!(error, DemoError::Ui4("frame-begin", Ui4Error::Busy))
        || matches!(error, DemoError::Vgpu(_, code) if *code == trueos::vgpu::ERR_BUSY)
}
