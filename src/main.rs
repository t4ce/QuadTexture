#![no_std]

extern crate alloc;

mod camera;
mod frame_retry;
mod gallery;
mod geometry;

use alloc::vec::Vec;
use core::fmt;
use geometry::{DrawMode, INDICES, VERTEX_STRIDE_BYTES, VERTICES};

use trueos::input::KEYBOARD_OUTPUT_FLAG_PRESS;
use trueos::ui4_scene::{Damage, Error as Ui4Error, Frame, ResizeEvent};
use trueos::vgpu::{
    BUFFER_USAGE_INDEX, BUFFER_USAGE_MAP_WRITE, BUFFER_USAGE_VERTEX, Buffer, Capabilities, Device,
    IndexedDraw, PRIMITIVE_TOPOLOGY_QUAD_LIST, Queue, QueueClass, RenderPipeline,
    SAMPLER_ADDRESS_U_REPEAT, SAMPLER_ADDRESS_V_REPEAT,
    SHADER_PACKAGE_CLIP_POSITION3_UV_TEXTURE_FNV1A64, ShaderModule,
};
use trueos::{
    logl::{self, level},
    vshell, vsys,
};

include!(concat!(env!("OUT_DIR"), "/intel_logo_meta.rs"));

const INTEL_LOGO_RGBA8: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/intel_logo_rgba8.bin"));

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const FRAME_X: i32 = 96;
const FRAME_Y: i32 = 72;
const CLEAR_RGBA8_SRGB: u32 = u32::from_le_bytes([0, 0, 0, 255]);

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
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    texture_buffer: Buffer,
    timeline: u64,
    pending_resize: Option<ResizeEvent>,
    mode: DrawMode,
    gallery: gallery::Gallery,
}

impl QuadTexture {
    fn open() -> Result<Self, DemoError> {
        let frame = Frame::open_streaming(FRAME_X, FRAME_Y, WIDTH, HEIGHT)
            .map_err(|error| DemoError::Ui4("frame-open", error))?;
        let device = Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))
            .map_err(|error| DemoError::Vgpu("device-open", error))?;
        let queue = device
            .create_queue(QueueClass::Render)
            .map_err(|error| DemoError::Vgpu("queue-create", error))?;
        let shader = device
            .create_shader_module(SHADER_PACKAGE_CLIP_POSITION3_UV_TEXTURE_FNV1A64)
            .map_err(|error| DemoError::Vgpu("shader-create", error))?;
        let pipeline = device
            .create_render_pipeline(shader, VERTEX_STRIDE_BYTES as u32, 0)
            .map_err(|error| DemoError::Vgpu("pipeline-create", error))?;

        let vertex_buffer = device
            .create_buffer(
                vertex_bytes().len(),
                BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_VERTEX,
            )
            .map_err(|error| DemoError::Vgpu("vertex-buffer-create", error))?;
        let index_buffer = device
            .create_buffer(
                index_bytes().len(),
                BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_INDEX,
            )
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
        let gallery = gallery::Gallery::open(device, WIDTH, HEIGHT)?;

        Ok(Self {
            frame,
            device,
            queue,
            _shader: shader,
            pipeline,
            vertex_buffer,
            index_buffer,
            texture_buffer,
            timeline: 0,
            pending_resize: None,
            mode: DrawMode::default(),
            gallery,
        })
    }

    fn service_keyboard_events(&mut self) -> Result<(), DemoError> {
        while let Some(event) = self
            .frame
            .take_keyboard_event()
            .map_err(|error| DemoError::Ui4("keyboard-event-take", error))?
        {
            if event.flags & KEYBOARD_OUTPUT_FLAG_PRESS != 0
                && matches!(event.codepoint, 82 | 114)
                && self.mode == DrawMode::Triangles
            {
                self.gallery
                    .reset_camera(self.frame.width(), self.frame.height());
            }
            let mode = self.mode.key_event(
                event.codepoint,
                event.flags & KEYBOARD_OUTPUT_FLAG_PRESS != 0,
            );
            if mode != self.mode {
                self.mode = mode;
                logl::log(
                    level::INFO,
                    format_args!("QuadTexture: switched to {}", mode.label()),
                );
            }
        }
        Ok(())
    }

    fn render_frame(&mut self) -> Result<(), DemoError> {
        self.service_resize_events()?;
        self.gallery
            .service_input(&mut self.frame, self.mode == DrawMode::Triangles)?;

        let width = self.frame.width();
        let height = self.frame.height();
        self.frame
            .begin_gpu_frame()
            .map_err(|error| DemoError::Ui4("frame-begin", error))?;
        // A failed surface import retains the write lease acquired above.
        // Retry that import without beginning or resizing another frame.
        let surface = frame_retry::retry_while(
            || self.device.acquire_ui4_surface(self.frame.window_id()),
            |code| *code == trueos::vgpu::ERR_BUSY,
            yield_frame_retry,
        )
        .map_err(|error| DemoError::Vgpu("surface-acquire", error))?;

        let point = if self.mode == DrawMode::Triangles {
            self.gallery
                .submit(self.device, self.queue, surface, width, height)?
        } else {
            self.device
                .submit_ui4_indexed(
                    self.queue,
                    surface,
                    self.pipeline,
                    self.vertex_buffer,
                    self.index_buffer,
                    IndexedDraw {
                        index_count: INDICES.len() as u32,
                        topology: PRIMITIVE_TOPOLOGY_QUAD_LIST,
                        clear_rgba8_srgb: CLEAR_RGBA8_SRGB,
                        sampled_texture: self.texture_buffer.raw(),
                        texture_width: INTEL_LOGO_WIDTH,
                        texture_height: INTEL_LOGO_HEIGHT,
                        texture_pitch: INTEL_LOGO_WIDTH * 4,
                        sampler_flags: SAMPLER_ADDRESS_U_REPEAT | SAMPLER_ADDRESS_V_REPEAT,
                        ..IndexedDraw::default()
                    },
                )
                .map_err(|error| DemoError::Vgpu("textured-indexed-submit", error))?
        };

        // Submission consumed the surface. An incomplete fence must keep
        // waiting for this exact point before its frame can be published.
        frame_retry::retry_while(
            || self.device.wait(self.queue, point.value),
            |code| *code == trueos::vgpu::ERR_BUSY,
            yield_frame_retry,
        )
        .map_err(|code| DemoError::Vgpu("timeline-wait", code))?;
        frame_retry::retry_while(
            || self.frame.publish(Damage::full(width, height)),
            |error| matches!(error, Ui4Error::Busy),
            yield_frame_retry,
        )
        .map_err(|error| DemoError::Ui4("frame-publish", error))?;

        self.timeline = point.value;
        if self.mode == DrawMode::Triangles {
            self.gallery.published(point.value);
        }
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
    let mut bytes = Vec::with_capacity(INDICES.len() * core::mem::size_of::<u32>());
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
    Contract(&'static str),
}

impl fmt::Display for DemoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ui4(stage, error) => write!(f, "UI4 {stage} failed: {error:?}"),
            Self::Vgpu(stage, error) => write!(f, "vGPU {stage} failed: {error}"),
            Self::Contract(stage) => write!(f, "asset contract failed: {stage}"),
        }
    }
}

fn main() {
    if let Err(error) = run() {
        logl::log(
            level::ERROR,
            format_args!("QuadTexture: fatal error: {error}"),
        );
        if !vshell::shutdown_current_blueprint("QuadTexture terminated after a fatal error") {
            logl::log(
                level::ERROR,
                "QuadTexture: could not request Blueprint shutdown",
            );
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
            "QuadTexture: {} rendered. keys: 1=quad logo, 3=triangle gallery, WASD=move, middle-drag=look, R=reset. frame={}x{} timeline={}",
            app.mode.label(),
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
    // Begin Busy never acquired a lease. Both submit APIs consume Ui4Surface
    // and cancel its lease through Drop on failure. Later stages retain the
    // same lease and are retried inside render_frame instead.
    matches!(error, DemoError::Ui4("frame-begin", Ui4Error::Busy))
        || matches!(error,
            DemoError::Vgpu("gallery-retained-submit" | "textured-indexed-submit", code)
                if *code == trueos::vgpu::ERR_BUSY)
}

fn yield_frame_retry() {
    vsys::poll_once();
    vsys::sleep_ms(1);
}
