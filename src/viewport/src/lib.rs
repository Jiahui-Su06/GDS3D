use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use eframe::{egui_wgpu, wgpu};

const CAMERA_PITCH_MIN: f32 = -1.48;
const CAMERA_PITCH_MAX: f32 = 1.48;
const CAMERA_DISTANCE_FACTOR: f32 = 2.0;
const CAMERA_ROTATE_SPEED: f32 = 0.003;
const AXIS_LINE_WIDTH_PX: f32 = 2.0;
const AXIS_GIZMO_MARGIN_PX: f32 = 52.0;
const AXIS_GIZMO_LENGTH_PX: f32 = 36.0;
const AXIS_GIZMO_LABEL_GAP_PX: f32 = 9.0;
const SELECTION_LINE_WIDTH_PX: f32 = 2.0;
const SELECTION_PAD_FACTOR: f32 = 0.0025;
const BUFFER_SIZE_MIN: u64 = 1024;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const PNG_COLOR_TYPE_RGBA: u8 = 6;
const PNG_BIT_DEPTH_8: u8 = 8;
const PNG_COMPRESSION_DEFLATE: u8 = 0;
const PNG_FILTER_NONE: u8 = 0;
const PNG_INTERLACE_NONE: u8 = 0;
const RENDER_PIXELS_MAX: u64 = 64_000_000;
const EDGE_KEY_SCALE: f32 = 1000.0;

pub const RECOMMENDED_MSAA_SAMPLES: u16 = 4;

#[derive(Clone, Debug, PartialEq)]
pub struct Bounds2d {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

/// A single closed 2D polygon ring rendered by the viewport.
#[derive(Clone, Debug, PartialEq)]
pub struct Polygon2d {
    pub points: Vec<[f32; 2]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ViewportObject {
    pub id: String,
    pub bounds: Bounds2d,
    pub visible: bool,
    pub color: String,
    pub brightness: f32,
    pub z_min: f32,
    pub z_max: f32,
    pub polygons: Arc<[Polygon2d]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewportScene {
    pub revision: u64,
    pub objects: Vec<ViewportObject>,
    pub selected_id: Option<String>,
}

impl ViewportScene {
    pub fn object_count(&self) -> usize {
        self.objects.iter().filter(|obj| obj.visible).count()
    }
}

#[derive(Clone)]
pub struct ViewportState {
    pub show_axes: bool,
    pub zoom: f32,
    pub pan: Vec2,
    pub yaw: f32,
    pub pitch: f32,
    pub view_size: Vec2,
    last_drag_pos: Option<Pos2>,
    renderer: Arc<Mutex<Option<WgpuViewport>>>,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            show_axes: true,
            zoom: 1.0,
            pan: Vec2::ZERO,
            yaw: -0.65,
            pitch: 0.72,
            view_size: Vec2::new(1.0, 1.0),
            last_drag_pos: None,
            renderer: Arc::new(Mutex::new(None)),
        }
    }
}

impl ViewportState {
    pub fn new(render_state: Option<&egui_wgpu::RenderState>) -> Self {
        let mut state = Self::default();
        if let Some(render_state) = render_state {
            let renderer = WgpuViewport::new(&render_state.device, render_state.target_format);
            state.renderer = Arc::new(Mutex::new(Some(renderer)));
        }
        state
    }

    pub fn reset_camera(&mut self) {
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
        self.yaw = -0.65;
        self.pitch = 0.72;
        self.last_drag_pos = None;
    }
}

pub fn show_viewport(
    ui: &mut egui::Ui,
    scene: &ViewportScene,
    state: &mut ViewportState,
    empty_label: &str,
) {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::drag());
    state.view_size = rect.size();
    handle_camera_input(ui, &response, state);

    ui.painter()
        .rect_filled(rect, 0.0, Color32::from_rgb(248, 250, 252));

    let request = RenderRequest::from_scene(scene, state, rect);
    let callback = egui_wgpu::Callback::new_paint_callback(
        rect,
        ViewportCallback {
            renderer: Arc::clone(&state.renderer),
            request,
        },
    );
    ui.painter().add(callback);

    if state.show_axes {
        paint_axis_labels(ui.painter(), rect, state, ui.ctx().pixels_per_point());
    }

    if scene.object_count() == 0 {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            empty_label,
            egui::FontId::proportional(14.0),
            Color32::from_rgb(91, 101, 112),
        );
    }
}

/// Render the viewport scene from the current camera state into PNG bytes.
pub fn render_view_png(
    render_state: &egui_wgpu::RenderState,
    scene: &ViewportScene,
    state: &ViewportState,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("export image size must be non-zero".to_owned());
    }
    if u64::from(width) * u64::from(height) > RENDER_PIXELS_MAX {
        return Err(format!("export image is too large: {width} x {height}"));
    }

    let capture_size = capture_size_for_canvas(width, height, state.view_size);
    let capture_rgba =
        render_view_rgba(render_state, scene, state, capture_size.0, capture_size.1)?;
    let rgba = center_on_canvas(width, height, capture_size.0, capture_size.1, &capture_rgba)?;
    encode_png(width, height, &rgba).map_err(|err| format!("encode png: {err}"))
}

/// Render the current viewport into a target-size RGBA canvas without PNG encoding.
pub fn render_view_rgba_canvas(
    render_state: &egui_wgpu::RenderState,
    scene: &ViewportScene,
    state: &ViewportState,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("export image size must be non-zero".to_owned());
    }
    if u64::from(width) * u64::from(height) > RENDER_PIXELS_MAX {
        return Err(format!("export image is too large: {width} x {height}"));
    }

    let capture_size = capture_size_for_canvas(width, height, state.view_size);
    let capture_rgba =
        render_view_rgba(render_state, scene, state, capture_size.0, capture_size.1)?;
    center_on_canvas(width, height, capture_size.0, capture_size.1, &capture_rgba)
}

/// Encode RGBA8 pixels as PNG bytes.
pub fn encode_rgba_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    encode_png(width, height, rgba)
}

/// Wrap PNG bytes in an SVG image element.
pub fn embedded_png_svg(
    width: u32,
    height: u32,
    title: &str,
    png: &[u8],
) -> Result<String, String> {
    if width == 0 || height == 0 {
        return Err("svg size must be non-zero".to_owned());
    }
    if u64::from(width) * u64::from(height) > RENDER_PIXELS_MAX {
        return Err(format!("svg image is too large: {width} x {height}"));
    }
    if !png.starts_with(PNG_SIGNATURE) {
        return Err("embedded svg image must be png data".to_owned());
    }

    let encoded = base64_encode(png);
    let mut svg = String::new();
    svg.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    svg.push('\n');
    svg.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"#
    ));
    svg.push('\n');
    svg.push_str("  <title>");
    push_xml_escaped(&mut svg, title);
    svg.push_str("</title>\n");
    svg.push_str(r##"  <rect width="100%" height="100%" fill="#FFFFFF"/>"##);
    svg.push('\n');
    svg.push_str(&format!(
        r#"  <image x="0" y="0" width="{width}" height="{height}" href="data:image/png;base64,{encoded}" preserveAspectRatio="none"/>"#
    ));
    svg.push_str("\n</svg>\n");
    Ok(svg)
}

fn render_view_rgba(
    render_state: &egui_wgpu::RenderState,
    scene: &ViewportScene,
    state: &ViewportState,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("capture size must be non-zero".to_owned());
    }

    let device = &render_state.device;
    let queue = &render_state.queue;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gds3d_export_target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: render_state.target_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let msaa = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gds3d_export_msaa"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: RECOMMENDED_MSAA_SAMPLES as u32,
        dimension: wgpu::TextureDimension::D2,
        format: render_state.target_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let msaa_view = msaa.create_view(&wgpu::TextureViewDescriptor::default());
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gds3d_export_depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: RECOMMENDED_MSAA_SAMPLES as u32,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth24Plus,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());

    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(width as f32, height as f32));
    let request = RenderRequest::from_scene(scene, state, rect);
    let screen_descriptor = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [width, height],
        pixels_per_point: 1.0,
    };
    let mut renderer = WgpuViewport::new(device, render_state.target_format);
    renderer.prepare(device, queue, &screen_descriptor, &request);

    let bytes_per_pixel = 4_u32;
    let row_bytes = width
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| "export image row is too large".to_owned())?;
    let padded_row_bytes = align_to(row_bytes, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer_size = u64::from(padded_row_bytes)
        .checked_mul(u64::from(height))
        .ok_or_else(|| "export image buffer is too large".to_owned())?;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gds3d_export_readback"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("gds3d_export_encoder"),
    });
    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("gds3d_export_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &msaa_view,
                resolve_target: Some(&target_view),
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        renderer.paint(&mut render_pass);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row_bytes),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let (sender, receiver) = mpsc::channel();
    readback.map_async(wgpu::MapMode::Read, .., move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|err| format!("wait for export render: {err}"))?;
    receiver
        .recv()
        .map_err(|err| format!("wait for export readback: {err}"))?
        .map_err(|err| format!("map export readback: {err}"))?;

    let texture_format = render_state.target_format;
    let mapped = readback.get_mapped_range(..);
    let rgba_size = usize::try_from(u64::from(row_bytes) * u64::from(height))
        .map_err(|_| "export image is too large".to_owned())?;
    let row_stride =
        usize::try_from(padded_row_bytes).map_err(|_| "invalid export row stride".to_owned())?;
    let row_len = usize::try_from(row_bytes).map_err(|_| "invalid export row size".to_owned())?;
    let mut rgba = Vec::with_capacity(rgba_size);
    for row in mapped.chunks(row_stride) {
        push_texture_row_as_rgba(&mut rgba, &row[..row_len], texture_format)?;
    }
    drop(mapped);
    readback.unmap();

    Ok(rgba)
}

fn handle_camera_input(ui: &egui::Ui, response: &egui::Response, state: &mut ViewportState) {
    let shift_down = ui.input(|input| input.modifiers.shift);
    if response.dragged() {
        let Some(pos) = response.interact_pointer_pos() else {
            state.last_drag_pos = None;
            return;
        };
        let delta = state
            .last_drag_pos
            .map_or(Vec2::ZERO, |last_pos| pos - last_pos);
        state.last_drag_pos = Some(pos);

        if response.dragged_by(egui::PointerButton::Primary) && !shift_down {
            state.yaw =
                (state.yaw - delta.x * CAMERA_ROTATE_SPEED).rem_euclid(std::f32::consts::TAU);
            state.pitch = (state.pitch + delta.y * CAMERA_ROTATE_SPEED)
                .clamp(CAMERA_PITCH_MIN, CAMERA_PITCH_MAX);
        }
        if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Primary) && shift_down
        {
            state.pan += delta;
        }
    } else {
        state.last_drag_pos = None;
    }
    if response.hovered() {
        let (zoom_delta, scroll_y) =
            ui.input(|input| (input.zoom_delta(), input.smooth_scroll_delta.y));
        if scroll_y.abs() > f32::EPSILON {
            let scroll_zoom = (scroll_y * 0.002).exp();
            state.zoom = (state.zoom * scroll_zoom).clamp(0.12, 24.0);
        } else if (zoom_delta - 1.0).abs() > f32::EPSILON {
            state.zoom = (state.zoom * zoom_delta).clamp(0.12, 24.0);
        }
    }
}

fn paint_axis_labels(
    painter: &egui::Painter,
    rect: Rect,
    state: &ViewportState,
    pixels_per_point: f32,
) {
    let (right, up, _) = orbit_basis(state.yaw, state.pitch);
    let origin = egui::pos2(
        rect.left() + AXIS_GIZMO_MARGIN_PX / pixels_per_point,
        rect.bottom() - AXIS_GIZMO_MARGIN_PX / pixels_per_point,
    );
    let length = AXIS_GIZMO_LENGTH_PX / pixels_per_point;
    let label_gap = AXIS_GIZMO_LABEL_GAP_PX / pixels_per_point;
    let pad = 8.0 / pixels_per_point;

    for (label, direction, color) in [
        (
            "X",
            Vec3::new(1.0, 0.0, 0.0),
            Color32::from_rgb(196, 57, 57),
        ),
        (
            "Y",
            Vec3::new(0.0, 1.0, 0.0),
            Color32::from_rgb(55, 132, 78),
        ),
        (
            "Z",
            Vec3::new(0.0, 0.0, 1.0),
            Color32::from_rgb(57, 99, 196),
        ),
    ] {
        let axis_x = direction.dot(right);
        let axis_y = direction.dot(up);
        let axis_len = (axis_x * axis_x + axis_y * axis_y).sqrt();
        if axis_len <= f32::EPSILON {
            continue;
        }

        let screen_dir = Vec2::new(axis_x / axis_len, -axis_y / axis_len);
        let mut pos = origin + screen_dir * (length + label_gap);
        pos.x = pos.x.clamp(rect.left() + pad, rect.right() - pad);
        pos.y = pos.y.clamp(rect.top() + pad, rect.bottom() - pad);
        painter.text(
            pos,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::monospace(11.0),
            color,
        );
    }
}

struct ViewportCallback {
    renderer: Arc<Mutex<Option<WgpuViewport>>>,
    request: RenderRequest,
}

impl egui_wgpu::CallbackTrait for ViewportCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        _callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Ok(mut guard) = self.renderer.lock()
            && let Some(renderer) = guard.as_mut()
        {
            renderer.prepare(device, queue, screen_descriptor, &self.request);
        }
        Vec::new()
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let viewport = info.viewport_in_pixels();
        if viewport.width_px == 0 || viewport.height_px == 0 {
            return;
        }
        if let Ok(guard) = self.renderer.lock()
            && let Some(renderer) = guard.as_ref()
        {
            renderer.paint(render_pass);
        }
    }
}

struct WgpuViewport {
    pipeline: wgpu::RenderPipeline,
    overlay_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    overlay_vertex_buffer: wgpu::Buffer,
    overlay_index_buffer: wgpu::Buffer,
    vertex_capacity_bytes: u64,
    index_capacity_bytes: u64,
    overlay_vertex_capacity_bytes: u64,
    overlay_index_capacity_bytes: u64,
    index_count: u32,
    overlay_index_count: u32,
    mesh_revision: Option<u64>,
    object_meshes: HashMap<String, CachedObjectMesh>,
}

impl WgpuViewport {
    fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gds3d_viewport_shader"),
            source: wgpu::ShaderSource::Wgsl(VIEWPORT_SHADER.into()),
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gds3d_viewport_uniform"),
            size: std::mem::size_of::<ViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gds3d_viewport_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gds3d_viewport_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gds3d_viewport_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = create_viewport_pipeline(
            device,
            &shader,
            &pipeline_layout,
            target_format,
            "gds3d_viewport_pipeline",
            true,
        );
        let overlay_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gds3d_viewport_overlay_shader"),
            source: wgpu::ShaderSource::Wgsl(OVERLAY_SHADER.into()),
        });
        let overlay_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("gds3d_viewport_overlay_pipeline_layout"),
                bind_group_layouts: &[],
                immediate_size: 0,
            });
        let overlay_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gds3d_viewport_overlay_pipeline"),
            layout: Some(&overlay_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &overlay_shader,
                entry_point: Some("vs_main"),
                buffers: &[OverlayVertex::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &overlay_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: RECOMMENDED_MSAA_SAMPLES as u32,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });

        let vertex_buffer = create_copy_buffer(
            device,
            "gds3d_viewport_vertex_buffer",
            wgpu::BufferUsages::VERTEX,
            BUFFER_SIZE_MIN,
        );
        let index_buffer = create_copy_buffer(
            device,
            "gds3d_viewport_index_buffer",
            wgpu::BufferUsages::INDEX,
            BUFFER_SIZE_MIN,
        );
        let overlay_vertex_buffer = create_copy_buffer(
            device,
            "gds3d_viewport_overlay_vertex_buffer",
            wgpu::BufferUsages::VERTEX,
            BUFFER_SIZE_MIN,
        );
        let overlay_index_buffer = create_copy_buffer(
            device,
            "gds3d_viewport_overlay_index_buffer",
            wgpu::BufferUsages::INDEX,
            BUFFER_SIZE_MIN,
        );

        Self {
            pipeline,
            overlay_pipeline,
            uniform_buffer,
            bind_group,
            vertex_buffer,
            index_buffer,
            overlay_vertex_buffer,
            overlay_index_buffer,
            vertex_capacity_bytes: BUFFER_SIZE_MIN,
            index_capacity_bytes: BUFFER_SIZE_MIN,
            overlay_vertex_capacity_bytes: BUFFER_SIZE_MIN,
            overlay_index_capacity_bytes: BUFFER_SIZE_MIN,
            index_count: 0,
            overlay_index_count: 0,
            mesh_revision: None,
            object_meshes: HashMap::new(),
        }
    }

    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_descriptor: &egui_wgpu::ScreenDescriptor,
        request: &RenderRequest,
    ) {
        let uniform = ViewUniform::from_request(request);
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniform));

        if self.mesh_revision != Some(request.scene_revision) {
            let mesh = request.mesh(&mut self.object_meshes);
            self.index_count = upload_viewport_mesh(
                device,
                queue,
                bytemuck::cast_slice(&mesh.vertices),
                bytemuck::cast_slice(&mesh.indices),
                mesh.indices.len() as u32,
                ViewportMeshBuffers {
                    vertex_buffer: &mut self.vertex_buffer,
                    vertex_capacity_bytes: &mut self.vertex_capacity_bytes,
                    index_buffer: &mut self.index_buffer,
                    index_capacity_bytes: &mut self.index_capacity_bytes,
                    vertex_label: "gds3d_viewport_vertex_buffer",
                    index_label: "gds3d_viewport_index_buffer",
                },
            );
            self.mesh_revision = Some(request.scene_revision);
        }

        let overlay = request.overlay_mesh(screen_descriptor.pixels_per_point);
        self.overlay_index_count = upload_viewport_mesh(
            device,
            queue,
            bytemuck::cast_slice(&overlay.vertices),
            bytemuck::cast_slice(&overlay.indices),
            overlay.indices.len() as u32,
            ViewportMeshBuffers {
                vertex_buffer: &mut self.overlay_vertex_buffer,
                vertex_capacity_bytes: &mut self.overlay_vertex_capacity_bytes,
                index_buffer: &mut self.overlay_index_buffer,
                index_capacity_bytes: &mut self.overlay_index_capacity_bytes,
                vertex_label: "gds3d_viewport_overlay_vertex_buffer",
                index_label: "gds3d_viewport_overlay_index_buffer",
            },
        );
    }

    fn paint(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        if self.index_count > 0 {
            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..self.index_count, 0, 0..1);
        }
        if self.overlay_index_count > 0 {
            render_pass.set_pipeline(&self.overlay_pipeline);
            render_pass.set_vertex_buffer(0, self.overlay_vertex_buffer.slice(..));
            render_pass.set_index_buffer(
                self.overlay_index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            render_pass.draw_indexed(0..self.overlay_index_count, 0, 0..1);
        }
    }
}

fn create_viewport_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    label: &'static str,
    depth_write_enabled: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[ViewportVertex::layout()],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth24Plus,
            depth_write_enabled: Some(depth_write_enabled),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: RECOMMENDED_MSAA_SAMPLES as u32,
            ..Default::default()
        },
        multiview_mask: None,
        cache: None,
    })
}

struct ViewportMeshBuffers<'a> {
    vertex_buffer: &'a mut wgpu::Buffer,
    vertex_capacity_bytes: &'a mut u64,
    index_buffer: &'a mut wgpu::Buffer,
    index_capacity_bytes: &'a mut u64,
    vertex_label: &'static str,
    index_label: &'static str,
}

fn upload_viewport_mesh(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    vertex_bytes: &[u8],
    index_bytes: &[u8],
    index_count: u32,
    buffers: ViewportMeshBuffers<'_>,
) -> u32 {
    if vertex_bytes.is_empty() || index_bytes.is_empty() {
        return 0;
    }

    let vertex_size = vertex_bytes.len() as u64;
    if vertex_size > *buffers.vertex_capacity_bytes {
        *buffers.vertex_capacity_bytes = next_buffer_size(vertex_size);
        *buffers.vertex_buffer = create_copy_buffer(
            device,
            buffers.vertex_label,
            wgpu::BufferUsages::VERTEX,
            *buffers.vertex_capacity_bytes,
        );
    }
    queue.write_buffer(buffers.vertex_buffer, 0, vertex_bytes);

    let index_size = index_bytes.len() as u64;
    if index_size > *buffers.index_capacity_bytes {
        *buffers.index_capacity_bytes = next_buffer_size(index_size);
        *buffers.index_buffer = create_copy_buffer(
            device,
            buffers.index_label,
            wgpu::BufferUsages::INDEX,
            *buffers.index_capacity_bytes,
        );
    }
    queue.write_buffer(buffers.index_buffer, 0, index_bytes);

    index_count
}

fn create_copy_buffer(
    device: &wgpu::Device,
    label: &'static str,
    usage: wgpu::BufferUsages,
    size: u64,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn next_buffer_size(size: u64) -> u64 {
    size.next_power_of_two().max(BUFFER_SIZE_MIN)
}

fn align_to(value: u32, alignment: u32) -> u32 {
    debug_assert!(alignment.is_power_of_two());
    let mask = alignment - 1;
    (value + mask) & !mask
}

fn push_texture_row_as_rgba(
    out: &mut Vec<u8>,
    row: &[u8],
    format: wgpu::TextureFormat,
) -> Result<(), String> {
    match format {
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => {
            out.extend_from_slice(row);
            Ok(())
        }
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
            for pixel in row.chunks_exact(4) {
                out.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
            }
            Ok(())
        }
        other => Err(format!("unsupported export texture format: {other:?}")),
    }
}

fn capture_size_for_canvas(canvas_width: u32, canvas_height: u32, view_size: Vec2) -> (u32, u32) {
    let viewport_width = view_size.x.max(1.0);
    let viewport_height = view_size.y.max(1.0);
    let viewport_ratio = viewport_width / viewport_height;
    let canvas_ratio = canvas_width as f32 / canvas_height as f32;

    if viewport_ratio >= canvas_ratio {
        let capture_width = canvas_width;
        let capture_height = ((capture_width as f32 / viewport_ratio).round() as u32).max(1);
        (capture_width, capture_height)
    } else {
        let capture_height = canvas_height;
        let capture_width = ((capture_height as f32 * viewport_ratio).round() as u32).max(1);
        (capture_width, capture_height)
    }
}

fn center_on_canvas(
    canvas_width: u32,
    canvas_height: u32,
    image_width: u32,
    image_height: u32,
    image_rgba: &[u8],
) -> Result<Vec<u8>, String> {
    let canvas_width_usize =
        usize::try_from(canvas_width).map_err(|_| "invalid canvas width".to_owned())?;
    let canvas_height_usize =
        usize::try_from(canvas_height).map_err(|_| "invalid canvas height".to_owned())?;
    let image_width_usize =
        usize::try_from(image_width).map_err(|_| "invalid image width".to_owned())?;
    let image_height_usize =
        usize::try_from(image_height).map_err(|_| "invalid image height".to_owned())?;
    let image_stride = image_width_usize
        .checked_mul(4)
        .ok_or_else(|| "image row is too large".to_owned())?;
    let image_size = image_stride
        .checked_mul(image_height_usize)
        .ok_or_else(|| "image is too large".to_owned())?;
    if image_rgba.len() != image_size {
        return Err("image buffer has invalid length".to_owned());
    }

    let canvas_stride = canvas_width_usize
        .checked_mul(4)
        .ok_or_else(|| "canvas row is too large".to_owned())?;
    let canvas_size = canvas_stride
        .checked_mul(canvas_height_usize)
        .ok_or_else(|| "canvas is too large".to_owned())?;
    let mut canvas = vec![255_u8; canvas_size];
    let x_offset = (canvas_width_usize.saturating_sub(image_width_usize)) / 2;
    let y_offset = (canvas_height_usize.saturating_sub(image_height_usize)) / 2;

    for row in 0..image_height_usize {
        let src_start = row * image_stride;
        let src_end = src_start + image_stride;
        let dst_start = (row + y_offset) * canvas_stride + x_offset * 4;
        let dst_end = dst_start + image_stride;
        if dst_end > canvas.len() {
            return Err("image does not fit on export canvas".to_owned());
        }
        canvas[dst_start..dst_end].copy_from_slice(&image_rgba[src_start..src_end]);
    }
    Ok(canvas)
}

fn push_xml_escaped(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);

        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let row_bytes = usize::try_from(width)
        .map_err(|_| "invalid png width".to_owned())?
        .checked_mul(4)
        .ok_or_else(|| "png row is too large".to_owned())?;
    let expected_len = row_bytes
        .checked_mul(usize::try_from(height).map_err(|_| "invalid png height".to_owned())?)
        .ok_or_else(|| "png image is too large".to_owned())?;
    if rgba.len() != expected_len {
        return Err("png pixel buffer has invalid length".to_owned());
    }

    let height_usize = usize::try_from(height).map_err(|_| "invalid png height".to_owned())?;
    let mut raw = Vec::with_capacity(expected_len + height_usize);
    for row in rgba.chunks_exact(row_bytes) {
        raw.push(PNG_FILTER_NONE);
        raw.extend_from_slice(row);
    }

    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut encoder, &raw).map_err(|err| err.to_string())?;
    let compressed = encoder.finish().map_err(|err| err.to_string())?;

    let mut png = Vec::new();
    png.extend_from_slice(PNG_SIGNATURE);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(PNG_BIT_DEPTH_8);
    ihdr.push(PNG_COLOR_TYPE_RGBA);
    ihdr.push(PNG_COMPRESSION_DEFLATE);
    ihdr.push(PNG_FILTER_NONE);
    ihdr.push(PNG_INTERLACE_NONE);
    write_png_chunk(&mut png, b"IHDR", &ihdr)?;
    write_png_chunk(&mut png, b"IDAT", &compressed)?;
    write_png_chunk(&mut png, b"IEND", &[])?;
    Ok(png)
}

fn write_png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) -> Result<(), String> {
    out.extend_from_slice(
        &u32::try_from(data.len())
            .map_err(|_| "png chunk is too large".to_owned())?
            .to_be_bytes(),
    );
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(kind);
    hasher.update(data);
    out.extend_from_slice(&hasher.finalize().to_be_bytes());
    Ok(())
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewUniform {
    eye: [f32; 4],
    center: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    forward: [f32; 4],
    params: [f32; 4],
}

impl ViewUniform {
    fn from_request(request: &RenderRequest) -> Self {
        let half_height = (request.bounds.span() / request.camera.zoom).max(1.0) * 0.5;
        let half_width = half_height * request.camera.aspect.max(0.1);
        let near = 0.1;
        let far = (request.bounds.span() * 8.0).max(1000.0);

        Self {
            eye: request.camera.eye.to_vec4(0.0),
            center: request.camera.target.to_vec4(0.0),
            right: request.camera.right.to_vec4(0.0),
            up: request.camera.up.to_vec4(0.0),
            forward: request.camera.forward.to_vec4(0.0),
            params: [half_width, half_height, near, far],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewportVertex {
    position: [f32; 3],
    color: [f32; 4],
    normal: [f32; 3],
    _pad: f32,
}

impl ViewportVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4, 2 => Float32x3];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }

    fn new(position: Vec3, normal: Vec3, color: [f32; 4]) -> Self {
        Self {
            position: position.to_array(),
            color,
            normal: normal.to_array(),
            _pad: 0.0,
        }
    }
}

struct ViewportMesh {
    vertices: Vec<ViewportVertex>,
    indices: Vec<u32>,
}

impl ViewportMesh {
    fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    fn append(&mut self, mesh: &MeshRequest, z_min: f32, z_max: f32) {
        let offset = self.vertices.len() as u32;
        let z0 = z_min.min(z_max);
        let z1 = z_min.max(z_max);
        let z_scale = z1 - z0;
        self.vertices.extend(mesh.vertices.iter().map(|vertex| {
            let mut vertex = *vertex;
            vertex.position[2] = z0 + vertex.position[2] * z_scale;
            vertex
        }));
        self.indices
            .extend(mesh.indices.iter().map(|index| offset + index));
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct OverlayVertex {
    position: [f32; 3],
    color: [f32; 4],
}

impl OverlayVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }

    fn new(x: f32, y: f32, z: f32, color: [f32; 4]) -> Self {
        Self {
            position: [x, y, z],
            color,
        }
    }
}

struct OverlayMesh {
    vertices: Vec<OverlayVertex>,
    indices: Vec<u32>,
    width_px: f32,
    height_px: f32,
}

impl OverlayMesh {
    fn new(width_px: f32, height_px: f32) -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            width_px,
            height_px,
        }
    }

    fn append_line(&mut self, projection: &Projection, line: &LineRequest) {
        let Some(from) = projection.project(line.from) else {
            return;
        };
        let Some(to) = projection.project(line.to) else {
            return;
        };

        self.append_projected_line(from, to, line.color, line.width_px);
    }

    fn append_axis_gizmo(&mut self, projection: &Projection) {
        let origin = ProjectedPoint {
            x: -1.0 + AXIS_GIZMO_MARGIN_PX * 2.0 / self.width_px,
            y: -1.0 + AXIS_GIZMO_MARGIN_PX * 2.0 / self.height_px,
        };
        for (direction, color) in [
            (Vec3::new(1.0, 0.0, 0.0), opaque_color(196, 57, 57)),
            (Vec3::new(0.0, 1.0, 0.0), opaque_color(55, 132, 78)),
            (Vec3::new(0.0, 0.0, 1.0), opaque_color(57, 99, 196)),
        ] {
            let x = direction.dot(projection.right);
            let y = direction.dot(projection.up);
            let length = (x * x + y * y).sqrt();
            if length <= f32::EPSILON {
                continue;
            }
            let end = ProjectedPoint {
                x: origin.x + x / length * AXIS_GIZMO_LENGTH_PX * 2.0 / self.width_px,
                y: origin.y + y / length * AXIS_GIZMO_LENGTH_PX * 2.0 / self.height_px,
            };
            self.append_projected_line(origin, end, color, AXIS_LINE_WIDTH_PX);
        }
    }

    fn append_projected_line(
        &mut self,
        from: ProjectedPoint,
        to: ProjectedPoint,
        color: [f32; 4],
        width_px: f32,
    ) {
        let dx_px = (to.x - from.x) * self.width_px * 0.5;
        let dy_px = (to.y - from.y) * self.height_px * 0.5;
        let length_px = (dx_px * dx_px + dy_px * dy_px).sqrt();
        if length_px <= 0.5 {
            return;
        }

        let half_width_px = width_px * 0.5;
        let offset_x = (-dy_px / length_px) * half_width_px * 2.0 / self.width_px;
        let offset_y = (dx_px / length_px) * half_width_px * 2.0 / self.height_px;
        let depth = 0.0;
        let base = self.vertices.len() as u32;
        self.vertices.push(OverlayVertex::new(
            from.x + offset_x,
            from.y + offset_y,
            depth,
            color,
        ));
        self.vertices.push(OverlayVertex::new(
            from.x - offset_x,
            from.y - offset_y,
            depth,
            color,
        ));
        self.vertices.push(OverlayVertex::new(
            to.x - offset_x,
            to.y - offset_y,
            depth,
            color,
        ));
        self.vertices.push(OverlayVertex::new(
            to.x + offset_x,
            to.y + offset_y,
            depth,
            color,
        ));
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

#[derive(Clone, Copy)]
struct LineRequest {
    from: Vec3,
    to: Vec3,
    color: [f32; 4],
    width_px: f32,
}

impl LineRequest {
    fn new(from: Vec3, to: Vec3, color: [f32; 4], width_px: f32) -> Self {
        Self {
            from,
            to,
            color,
            width_px,
        }
    }
}

#[derive(Clone)]
struct RenderRequest {
    scene_revision: u64,
    bounds: SceneBounds,
    camera: CameraRequest,
    show_axes: bool,
    rect: Rect,
    objects: Vec<ViewportObject>,
    selection: Vec<LineRequest>,
}

impl RenderRequest {
    fn from_scene(scene: &ViewportScene, state: &ViewportState, rect: Rect) -> Self {
        let bounds = scene_bounds(scene).unwrap_or_default();
        let mut selected_lines = Vec::new();
        for obj in &scene.objects {
            if obj.visible && scene.selected_id.as_deref() == Some(obj.id.as_str()) {
                selected_lines.extend(selection_lines(obj, &bounds));
            }
        }

        Self {
            scene_revision: scene.revision,
            bounds,
            camera: CameraRequest::new(&bounds, state, rect),
            show_axes: state.show_axes,
            rect,
            objects: scene.objects.clone(),
            selection: selected_lines,
        }
    }

    fn mesh(&self, cache: &mut HashMap<String, CachedObjectMesh>) -> ViewportMesh {
        cache.retain(|id, _mesh| {
            self.objects
                .iter()
                .any(|obj| obj.id.as_str() == id.as_str())
        });

        let mut mesh = ViewportMesh::new();
        for object in &self.objects {
            if !object.visible {
                continue;
            }
            append_object_mesh(cache, &mut mesh, object);
        }
        mesh
    }

    fn overlay_mesh(&self, pixels_per_point: f32) -> OverlayMesh {
        let width_px = (self.rect.width() * pixels_per_point).max(1.0);
        let height_px = (self.rect.height() * pixels_per_point).max(1.0);
        let projection = Projection::new(self);
        let mut mesh = OverlayMesh::new(width_px, height_px);
        for line in &self.selection {
            mesh.append_line(&projection, line);
        }
        if self.show_axes {
            mesh.append_axis_gizmo(&projection);
        }
        mesh
    }
}

fn append_object_mesh(
    cache: &mut HashMap<String, CachedObjectMesh>,
    mesh: &mut ViewportMesh,
    object: &ViewportObject,
) {
    if !object.visible {
        return;
    }

    let object_key = object_mesh_key(object);
    let color_key = object_color_key(object);
    let object_mesh = cache
        .entry(object.id.clone())
        .and_modify(|cached| {
            if cached.key != object_key {
                *cached = CachedObjectMesh {
                    key: object_key,
                    color_key,
                    mesh: MeshRequest::object(object),
                };
            } else if cached.color_key != color_key {
                cached.mesh.set_color(object_color(object));
                cached.color_key = color_key;
            }
        })
        .or_insert_with(|| CachedObjectMesh {
            key: object_key,
            color_key,
            mesh: MeshRequest::object(object),
        });
    mesh.append(&object_mesh.mesh, object.z_min, object.z_max);
}

#[derive(Clone, Copy)]
struct CameraRequest {
    eye: Vec3,
    target: Vec3,
    right: Vec3,
    up: Vec3,
    forward: Vec3,
    zoom: f32,
    aspect: f32,
}

impl CameraRequest {
    fn new(bounds: &SceneBounds, state: &ViewportState, rect: Rect) -> Self {
        let center = bounds.center();
        let span = bounds.span();
        let horizontal = state.pitch.cos();
        let direction = Vec3::new(
            state.yaw.cos() * horizontal,
            state.yaw.sin() * horizontal,
            state.pitch.sin(),
        );
        let (right, up, forward) = orbit_basis(state.yaw, state.pitch);
        let pan_x = state.pan.x / rect.width().max(1.0) * span / state.zoom;
        let pan_y = state.pan.y / rect.height().max(1.0) * span / state.zoom;
        let target = center - right * pan_x + up * pan_y;
        Self {
            eye: target + direction * span * CAMERA_DISTANCE_FACTOR,
            target,
            right,
            up,
            forward,
            zoom: state.zoom,
            aspect: rect.width().max(1.0) / rect.height().max(1.0),
        }
    }
}

struct Projection {
    center: Vec3,
    eye: Vec3,
    right: Vec3,
    up: Vec3,
    forward: Vec3,
    half_width: f32,
    half_height: f32,
    near: f32,
    far: f32,
}

impl Projection {
    fn new(request: &RenderRequest) -> Self {
        let half_height = (request.bounds.span() / request.camera.zoom).max(1.0) * 0.5;
        let half_width = half_height * request.camera.aspect.max(0.1);
        Self {
            center: request.camera.target,
            eye: request.camera.eye,
            right: request.camera.right,
            up: request.camera.up,
            forward: request.camera.forward,
            half_width,
            half_height,
            near: 0.1,
            far: (request.bounds.span() * 8.0).max(1000.0),
        }
    }

    fn project(&self, point: Vec3) -> Option<ProjectedPoint> {
        let rel = point - self.center;
        let eye_rel = point - self.eye;
        let depth = (eye_rel.dot(self.forward) - self.near) / (self.far - self.near);
        if !depth.is_finite() {
            return None;
        }
        Some(ProjectedPoint {
            x: rel.dot(self.right) / self.half_width.max(0.001),
            y: rel.dot(self.up) / self.half_height.max(0.001),
        })
    }
}

fn orbit_basis(yaw: f32, pitch: f32) -> (Vec3, Vec3, Vec3) {
    let sin_yaw = yaw.sin();
    let cos_yaw = yaw.cos();
    let sin_pitch = pitch.sin();
    let cos_pitch = pitch.cos();

    let forward = Vec3::new(-cos_yaw * cos_pitch, -sin_yaw * cos_pitch, -sin_pitch);
    let right = Vec3::new(-sin_yaw, cos_yaw, 0.0);
    let up = Vec3::new(-cos_yaw * sin_pitch, -sin_yaw * sin_pitch, cos_pitch);
    (right, up, forward)
}

#[derive(Clone, Copy)]
struct ProjectedPoint {
    x: f32,
    y: f32,
}

#[derive(Clone)]
struct MeshRequest {
    vertices: Vec<ViewportVertex>,
    indices: Vec<u32>,
}

struct CachedObjectMesh {
    key: u64,
    color_key: u64,
    mesh: MeshRequest,
}

impl MeshRequest {
    fn object(obj: &ViewportObject) -> Self {
        let color = object_color(obj);
        if obj.polygons.is_empty() {
            return box_mesh(&obj.bounds, 0.0, 1.0, color);
        }

        polygon_mesh(&obj.polygons, 0.0, 1.0, color)
    }

    fn set_color(&mut self, color: [f32; 4]) {
        for vertex in &mut self.vertices {
            vertex.color = color;
        }
    }
}

fn object_color(obj: &ViewportObject) -> [f32; 4] {
    parse_hex_color(&obj.color, obj.brightness)
}

fn object_mesh_key(obj: &ViewportObject) -> u64 {
    let mut hasher = DefaultHasher::new();
    obj.id.hash(&mut hasher);
    hash_f32(&mut hasher, obj.bounds.min_x);
    hash_f32(&mut hasher, obj.bounds.min_y);
    hash_f32(&mut hasher, obj.bounds.max_x);
    hash_f32(&mut hasher, obj.bounds.max_y);
    hash_f32(&mut hasher, obj.z_min);
    hash_f32(&mut hasher, obj.z_max);
    obj.polygons.len().hash(&mut hasher);
    for polygon in obj.polygons.iter() {
        polygon.points.len().hash(&mut hasher);
        for point in &polygon.points {
            hash_f32(&mut hasher, point[0]);
            hash_f32(&mut hasher, point[1]);
        }
    }
    hasher.finish()
}

fn object_color_key(obj: &ViewportObject) -> u64 {
    let mut hasher = DefaultHasher::new();
    obj.color.hash(&mut hasher);
    hash_f32(&mut hasher, obj.brightness);
    hasher.finish()
}

fn hash_f32(hasher: &mut DefaultHasher, value: f32) {
    value.to_bits().hash(hasher);
}

#[derive(Clone, Copy, Debug)]
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

impl Vec3 {
    fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    fn dot(self, rhs: Self) -> f32 {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z
    }

    fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    fn normalized(self) -> Self {
        let length = self.length();
        if length <= f32::EPSILON {
            return Self::new(1.0, 0.0, 0.0);
        }
        self * (1.0 / length)
    }

    fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }

    fn to_vec4(self, w: f32) -> [f32; 4] {
        [self.x, self.y, self.z, w]
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl std::ops::Mul<f32> for Vec3 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

#[derive(Clone, Copy)]
struct SceneBounds {
    min_x: f32,
    min_y: f32,
    min_z: f32,
    max_x: f32,
    max_y: f32,
    max_z: f32,
}

impl Default for SceneBounds {
    fn default() -> Self {
        Self {
            min_x: -500.0,
            min_y: -350.0,
            min_z: -20.0,
            max_x: 500.0,
            max_y: 350.0,
            max_z: 120.0,
        }
    }
}

impl SceneBounds {
    fn center(self) -> Vec3 {
        Vec3::new(
            (self.min_x + self.max_x) * 0.5,
            (self.min_y + self.max_y) * 0.5,
            (self.min_z + self.max_z) * 0.5,
        )
    }

    fn span(self) -> f32 {
        (self.max_x - self.min_x)
            .max(self.max_y - self.min_y)
            .max((self.max_z - self.min_z) * 3.0)
            .max(1.0)
    }
}

fn scene_bounds(scene: &ViewportScene) -> Option<SceneBounds> {
    let mut bounds = None::<SceneBounds>;
    for obj in &scene.objects {
        if !obj.visible {
            continue;
        }
        let object_bounds = object_scene_bounds(&obj.bounds, obj.z_min, obj.z_max);
        bounds = Some(match bounds {
            None => object_bounds,
            Some(current) => SceneBounds {
                min_x: current.min_x.min(object_bounds.min_x),
                min_y: current.min_y.min(object_bounds.min_y),
                min_z: current.min_z.min(object_bounds.min_z),
                max_x: current.max_x.max(object_bounds.max_x),
                max_y: current.max_y.max(object_bounds.max_y),
                max_z: current.max_z.max(object_bounds.max_z),
            },
        });
    }
    bounds
}

fn object_scene_bounds(bounds: &Bounds2d, z_min: f32, z_max: f32) -> SceneBounds {
    SceneBounds {
        min_x: bounds.min_x,
        min_y: bounds.min_y,
        min_z: z_min,
        max_x: bounds.max_x,
        max_y: bounds.max_y,
        max_z: z_max,
    }
}

fn box_mesh(bounds: &Bounds2d, z_min: f32, z_max: f32, color: [f32; 4]) -> MeshRequest {
    let x0 = bounds.min_x;
    let x1 = bounds.max_x;
    let y0 = bounds.min_y;
    let y1 = bounds.max_y;
    let z0 = z_min.min(z_max);
    let z1 = z_min.max(z_max);

    let corners = [
        Vec3::new(x0, y0, z0),
        Vec3::new(x1, y0, z0),
        Vec3::new(x1, y1, z0),
        Vec3::new(x0, y1, z0),
        Vec3::new(x0, y0, z1),
        Vec3::new(x1, y0, z1),
        Vec3::new(x1, y1, z1),
        Vec3::new(x0, y1, z1),
    ];
    let mut mesh = MeshRequest {
        vertices: Vec::with_capacity(24),
        indices: Vec::with_capacity(36),
    };
    push_quad(
        &mut mesh,
        [corners[0], corners[3], corners[2], corners[1]],
        Vec3::new(0.0, 0.0, -1.0),
        color,
    );
    push_quad(
        &mut mesh,
        [corners[4], corners[5], corners[6], corners[7]],
        Vec3::new(0.0, 0.0, 1.0),
        color,
    );
    push_quad(
        &mut mesh,
        [corners[0], corners[1], corners[5], corners[4]],
        Vec3::new(0.0, -1.0, 0.0),
        color,
    );
    push_quad(
        &mut mesh,
        [corners[1], corners[2], corners[6], corners[5]],
        Vec3::new(1.0, 0.0, 0.0),
        color,
    );
    push_quad(
        &mut mesh,
        [corners[2], corners[3], corners[7], corners[6]],
        Vec3::new(0.0, 1.0, 0.0),
        color,
    );
    push_quad(
        &mut mesh,
        [corners[3], corners[0], corners[4], corners[7]],
        Vec3::new(-1.0, 0.0, 0.0),
        color,
    );
    mesh
}

fn polygon_mesh(polygons: &[Polygon2d], z_min: f32, z_max: f32, color: [f32; 4]) -> MeshRequest {
    let mut mesh = MeshRequest {
        vertices: Vec::new(),
        indices: Vec::new(),
    };
    let z0 = z_min.min(z_max);
    let z1 = z_min.max(z_max);
    let edge_counts = polygon_edge_counts(polygons);

    for polygon in polygons {
        append_polygon(&mut mesh, polygon, z0, z1, color, &edge_counts);
    }
    mesh
}

fn append_polygon(
    mesh: &mut MeshRequest,
    polygon: &Polygon2d,
    z0: f32,
    z1: f32,
    color: [f32; 4],
    edge_counts: &HashMap<EdgeKey, u32>,
) {
    let points = normalized_polygon_points(polygon);
    if points.len() < 3 {
        return;
    }

    let mut coordinates = Vec::with_capacity(points.len() * 2);
    for [x, y] in &points {
        coordinates.push(*x);
        coordinates.push(*y);
    }

    let Ok(triangles) = earcutr::earcut(&coordinates, &[], 2) else {
        return;
    };
    if triangles.is_empty() {
        return;
    }

    for triangle in triangles.chunks_exact(3) {
        let a = points[triangle[0]];
        let b = points[triangle[1]];
        let c = points[triangle[2]];
        let top = [
            Vec3::new(a[0], a[1], z1),
            Vec3::new(b[0], b[1], z1),
            Vec3::new(c[0], c[1], z1),
        ];
        let bottom = [
            Vec3::new(c[0], c[1], z0),
            Vec3::new(b[0], b[1], z0),
            Vec3::new(a[0], a[1], z0),
        ];
        if !push_triangle(mesh, top, Vec3::new(0.0, 0.0, 1.0), color) {
            return;
        }
        if !push_triangle(mesh, bottom, Vec3::new(0.0, 0.0, -1.0), color) {
            return;
        }
    }

    if (z1 - z0).abs() <= f32::EPSILON {
        return;
    }
    for index in 0..points.len() {
        let next_index = (index + 1) % points.len();
        let a = points[index];
        let b = points[next_index];
        let Some(edge_key) = edge_key(a, b) else {
            continue;
        };
        if edge_counts.get(&edge_key).copied().unwrap_or(0) > 1 {
            continue;
        }
        let edge = Vec3::new(b[0] - a[0], b[1] - a[1], 0.0);
        if edge.length() <= f32::EPSILON {
            continue;
        }
        let normal = Vec3::new(edge.y, -edge.x, 0.0).normalized();
        push_quad(
            mesh,
            [
                Vec3::new(a[0], a[1], z0),
                Vec3::new(b[0], b[1], z0),
                Vec3::new(b[0], b[1], z1),
                Vec3::new(a[0], a[1], z1),
            ],
            normal,
            color,
        );
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct EdgeKey {
    a: PointKey,
    b: PointKey,
}

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct PointKey {
    x: i64,
    y: i64,
}

fn polygon_edge_counts(polygons: &[Polygon2d]) -> HashMap<EdgeKey, u32> {
    let mut counts = HashMap::new();
    for polygon in polygons {
        let points = normalized_polygon_points(polygon);
        if points.len() < 3 {
            continue;
        }
        for index in 0..points.len() {
            let next_index = (index + 1) % points.len();
            if let Some(key) = edge_key(points[index], points[next_index]) {
                let count = counts.entry(key).or_insert(0_u32);
                *count = count.saturating_add(1);
            }
        }
    }
    counts
}

fn edge_key(a: [f32; 2], b: [f32; 2]) -> Option<EdgeKey> {
    let a = point_key(a)?;
    let b = point_key(b)?;
    if a == b {
        return None;
    }
    if a <= b {
        Some(EdgeKey { a, b })
    } else {
        Some(EdgeKey { a: b, b: a })
    }
}

fn point_key(point: [f32; 2]) -> Option<PointKey> {
    if !point[0].is_finite() || !point[1].is_finite() {
        return None;
    }
    Some(PointKey {
        x: (point[0] * EDGE_KEY_SCALE).round() as i64,
        y: (point[1] * EDGE_KEY_SCALE).round() as i64,
    })
}

fn normalized_polygon_points(polygon: &Polygon2d) -> Vec<[f32; 2]> {
    let mut points = Vec::with_capacity(polygon.points.len());
    for point in &polygon.points {
        if !point[0].is_finite() || !point[1].is_finite() {
            return Vec::new();
        }
        if points.last().is_some_and(|last| last == point) {
            continue;
        }
        points.push(*point);
    }
    if points.len() >= 2 && points.first() == points.last() {
        points.pop();
    }
    points
}

fn push_triangle(mesh: &mut MeshRequest, points: [Vec3; 3], normal: Vec3, color: [f32; 4]) -> bool {
    if mesh.vertices.len() > u32::MAX as usize - 3 {
        return false;
    }

    let base = mesh.vertices.len() as u32;
    for point in points {
        mesh.vertices
            .push(ViewportVertex::new(point, normal, color));
    }
    mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
    true
}

fn push_quad(mesh: &mut MeshRequest, points: [Vec3; 4], normal: Vec3, color: [f32; 4]) {
    if mesh.vertices.len() > u32::MAX as usize - 4 {
        return;
    }

    let base = mesh.vertices.len() as u32;
    for point in points {
        mesh.vertices
            .push(ViewportVertex::new(point, normal, color));
    }
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn selection_lines(obj: &ViewportObject, scene_bounds: &SceneBounds) -> Vec<LineRequest> {
    let bounds = &obj.bounds;
    let z0 = obj.z_min.min(obj.z_max);
    let z1 = obj.z_min.max(obj.z_max);
    let pad = (scene_bounds.span() * SELECTION_PAD_FACTOR).max(0.5);
    let min = Vec3::new(bounds.min_x - pad, bounds.min_y - pad, z0 - pad);
    let max = Vec3::new(bounds.max_x + pad, bounds.max_y + pad, z1 + pad);
    let color = opaque_color(240, 180, 41);
    let corners = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ];
    let edges = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    edges
        .iter()
        .map(|(a, b)| LineRequest::new(corners[*a], corners[*b], color, SELECTION_LINE_WIDTH_PX))
        .collect()
}

fn parse_hex_color(value: &str, brightness: f32) -> [f32; 4] {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return opaque_color(45, 108, 223);
    }

    let Ok(rgb) = u32::from_str_radix(hex, 16) else {
        return opaque_color(45, 108, 223);
    };

    let scale = brightness.clamp(0.0, 2.0);
    [
        channel_to_float(((rgb >> 16) & 0xff) as f32 * scale),
        channel_to_float(((rgb >> 8) & 0xff) as f32 * scale),
        channel_to_float((rgb & 0xff) as f32 * scale),
        1.0,
    ]
}

fn opaque_color(r: u8, g: u8, b: u8) -> [f32; 4] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}

fn channel_to_float(value: f32) -> f32 {
    value.round().clamp(0.0, 255.0) / 255.0
}

const VIEWPORT_SHADER: &str = r#"
struct ViewUniform {
    eye: vec4<f32>,
    center: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    params: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> view: ViewUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) normal: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let rel = input.position - view.center.xyz;
    let eye_rel = input.position - view.eye.xyz;
    let half_width = max(view.params.x, 0.001);
    let half_height = max(view.params.y, 0.001);
    let near = view.params.z;
    let far = max(view.params.w, near + 1.0);
    let depth = (dot(eye_rel, view.forward.xyz) - near) / (far - near);

    let light = normalize(vec3<f32>(0.35, -0.45, 0.82));
    let intensity = 0.58 + max(dot(normalize(input.normal), light), 0.0) * 0.42;

    var out: VertexOutput;
    out.position = vec4<f32>(
        dot(rel, view.right.xyz) / half_width,
        dot(rel, view.up.xyz) / half_height,
        depth,
        1.0,
    );
    out.color = vec4<f32>(input.color.rgb * intensity, input.color.a);
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
"#;

const OVERLAY_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(input.position, 1.0);
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_side_wall_for_shared_polygon_edge() {
        let polygons = [
            Polygon2d {
                points: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            },
            Polygon2d {
                points: vec![[1.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0]],
            },
        ];

        let mesh = polygon_mesh(&polygons, 0.0, 1.0, opaque_color(10, 20, 30));

        assert_eq!(mesh.indices.len(), 60);
    }

    #[test]
    fn keeps_side_wall_for_single_polygon_edge() {
        let polygons = [Polygon2d {
            points: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        }];

        let mesh = polygon_mesh(&polygons, 0.0, 1.0, opaque_color(10, 20, 30));

        assert_eq!(mesh.indices.len(), 36);
    }
}
