use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use eframe::{egui_wgpu, wgpu};

mod image;
mod math;
mod renderer;

use math::Vec3;

pub use image::{embedded_png_svg, encode_rgba_png};

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
    renderer: Arc<Mutex<Option<renderer::WgpuViewport>>>,
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
            let renderer =
                renderer::WgpuViewport::new(&render_state.device, render_state.target_format);
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

    let request = RenderRequest::from_scene(scene, state, rect, state.show_axes);
    let callback = egui_wgpu::Callback::new_paint_callback(
        rect,
        renderer::ViewportCallback {
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
    if u64::from(width) * u64::from(height) > image::RENDER_PIXELS_MAX {
        return Err(format!("export image is too large: {width} x {height}"));
    }

    let capture_size = image::capture_size_for_canvas(width, height, state.view_size);
    validate_capture_size(capture_size.0, capture_size.1)?;
    let capture_rgba = renderer::render_view_rgba(
        render_state,
        scene,
        state,
        capture_size.0,
        capture_size.1,
        false,
    )?;
    let rgba = image::fit_on_canvas(width, height, capture_size.0, capture_size.1, &capture_rgba)?;
    image::encode_png(width, height, &rgba).map_err(|err| format!("encode png: {err}"))
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
    if u64::from(width) * u64::from(height) > image::RENDER_PIXELS_MAX {
        return Err(format!("export image is too large: {width} x {height}"));
    }

    let capture_size = image::capture_size_for_canvas(width, height, state.view_size);
    validate_capture_size(capture_size.0, capture_size.1)?;
    let capture_rgba = renderer::render_view_rgba(
        render_state,
        scene,
        state,
        capture_size.0,
        capture_size.1,
        false,
    )?;
    image::fit_on_canvas(width, height, capture_size.0, capture_size.1, &capture_rgba)
}

fn validate_capture_size(width: u32, height: u32) -> Result<(), String> {
    if u64::from(width) * u64::from(height) > image::RENDER_PIXELS_MAX {
        return Err(format!("viewport capture is too large: {width} x {height}"));
    }
    Ok(())
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
    fn from_scene(
        scene: &ViewportScene,
        state: &ViewportState,
        rect: Rect,
        show_axes: bool,
    ) -> Self {
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
            camera: CameraRequest::new(&bounds, state, rect.size(), state.view_size),
            show_axes,
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
    fn new(bounds: &SceneBounds, state: &ViewportState, render_size: Vec2, pan_size: Vec2) -> Self {
        let center = bounds.center();
        let span = bounds.span();
        let horizontal = state.pitch.cos();
        let direction = Vec3::new(
            state.yaw.cos() * horizontal,
            state.yaw.sin() * horizontal,
            state.pitch.sin(),
        );
        let (right, up, forward) = orbit_basis(state.yaw, state.pitch);
        let pan_x = state.pan.x / pan_size.x.max(1.0) * span / state.zoom;
        let pan_y = state.pan.y / pan_size.y.max(1.0) * span / state.zoom;
        let target = center - right * pan_x + up * pan_y;
        Self {
            eye: target + direction * span * CAMERA_DISTANCE_FACTOR,
            target,
            right,
            up,
            forward,
            zoom: state.zoom,
            aspect: render_size.x.max(1.0) / render_size.y.max(1.0),
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
    if !obj.polygons.is_empty() {
        obj.polygons.len().hash(&mut hasher);
        obj.polygons.as_ptr().hash(&mut hasher);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_shared_side_wall() {
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
    fn keeps_single_side_wall() {
        let polygons = [Polygon2d {
            points: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        }];

        let mesh = polygon_mesh(&polygons, 0.0, 1.0, opaque_color(10, 20, 30));

        assert_eq!(mesh.indices.len(), 36);
    }

    #[test]
    fn orthographic_keeps_far_points_same_size() {
        let projection = Projection {
            center: Vec3::new(0.0, 0.0, 10.0),
            eye: Vec3::new(0.0, 0.0, 0.0),
            right: Vec3::new(1.0, 0.0, 0.0),
            up: Vec3::new(0.0, 1.0, 0.0),
            forward: Vec3::new(0.0, 0.0, 1.0),
            half_width: 5.0,
            half_height: 5.0,
            near: 0.1,
            far: 100.0,
        };

        let near = projection
            .project(Vec3::new(1.0, 0.0, 10.0))
            .expect("project near point");
        let far = projection
            .project(Vec3::new(1.0, 0.0, 20.0))
            .expect("project far point");

        assert!((near.x - 0.2).abs() < 0.0001);
        assert!((far.x - 0.2).abs() < 0.0001);
    }

    #[test]
    fn orthographic_projects_near_plane() {
        let projection = Projection {
            center: Vec3::new(0.0, 0.0, 10.0),
            eye: Vec3::new(0.0, 0.0, 0.0),
            right: Vec3::new(1.0, 0.0, 0.0),
            up: Vec3::new(0.0, 1.0, 0.0),
            forward: Vec3::new(0.0, 0.0, 1.0),
            half_width: 5.0,
            half_height: 5.0,
            near: 0.1,
            far: 100.0,
        };

        assert!(projection.project(Vec3::new(0.0, 0.0, 0.05)).is_some());
    }

    #[test]
    fn export_pan_is_stable() {
        let state = ViewportState {
            zoom: 2.0,
            pan: Vec2::new(120.0, -60.0),
            view_size: Vec2::new(800.0, 600.0),
            ..Default::default()
        };
        let bounds = SceneBounds::default();

        let viewport_camera = CameraRequest::new(&bounds, &state, state.view_size, state.view_size);
        let export_camera =
            CameraRequest::new(&bounds, &state, Vec2::new(1600.0, 1200.0), state.view_size);

        assert!((viewport_camera.target.x - export_camera.target.x).abs() < 0.0001);
        assert!((viewport_camera.target.y - export_camera.target.y).abs() < 0.0001);
        assert!((viewport_camera.target.z - export_camera.target.z).abs() < 0.0001);
        assert!((viewport_camera.aspect - export_camera.aspect).abs() < 0.0001);
    }

    #[test]
    fn key_ignores_z() {
        let polygons: Arc<[Polygon2d]> = vec![Polygon2d {
            points: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
        }]
        .into();
        let mut object = ViewportObject {
            id: "layer".to_owned(),
            bounds: Bounds2d {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 10.0,
                max_y: 10.0,
            },
            visible: true,
            color: "#2D6CDF".to_owned(),
            brightness: 1.0,
            z_min: 0.0,
            z_max: 10.0,
            polygons,
        };
        let before = object_mesh_key(&object);

        object.z_min = 20.0;
        object.z_max = 40.0;

        assert_eq!(object_mesh_key(&object), before);
    }
}
