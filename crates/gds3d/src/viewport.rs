use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, Vec2};

use crate::model::{Bounds2d, Scene, SceneObject, Selection};

#[derive(Clone, Debug)]
pub struct ViewportState {
    pub show_axes: bool,
    pub zoom: f32,
    pub pan: Vec2,
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            show_axes: true,
            zoom: 1.0,
            pan: Vec2::ZERO,
            yaw: -0.65,
            pitch: 0.72,
        }
    }
}

impl ViewportState {
    pub fn reset_camera(&mut self) {
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
        self.yaw = -0.65;
        self.pitch = 0.72;
    }
}

pub fn show_viewport(
    ui: &mut egui::Ui,
    scene: &Scene,
    selection: &Selection,
    state: &mut ViewportState,
) {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::drag());
    if response.dragged_by(egui::PointerButton::Primary) {
        let delta = response.drag_delta();
        state.yaw = (state.yaw + delta.x * 0.01).rem_euclid(std::f32::consts::TAU);
        state.pitch = (state.pitch + delta.y * 0.01).clamp(0.18, 1.35);
    }
    if response.dragged_by(egui::PointerButton::Secondary)
        || (response.dragged() && ui.input(|input| input.modifiers.shift))
    {
        state.pan += response.drag_delta();
    }
    if response.hovered() {
        let zoom_delta = ui.input(|input| input.zoom_delta());
        if (zoom_delta - 1.0).abs() > f32::EPSILON {
            state.zoom = (state.zoom * zoom_delta).clamp(0.12, 24.0);
        }
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_rgb(248, 250, 252));

    let camera = Camera2d::new(rect, state, scene_bounds(scene).as_ref());
    paint_floor_grid(&painter, &camera);

    let mut faces = scene
        .objects()
        .filter(|obj| obj.is_visible())
        .flat_map(|obj| object_faces(obj, selection, &camera))
        .collect::<Vec<_>>();
    faces.sort_by(|a, b| a.depth.total_cmp(&b.depth));

    for face in faces {
        painter.add(Shape::convex_polygon(
            face.points,
            face.fill,
            Stroke::new(face.stroke_width, face.stroke),
        ));
    }

    if state.show_axes {
        paint_axes(&painter, &camera);
    }

    if scene.object_count() == 0 {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Import a GDS file or create a baseplate",
            egui::FontId::proportional(14.0),
            Color32::from_rgb(91, 101, 112),
        );
    }
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
}

struct Camera2d {
    rect: Rect,
    center: Vec3,
    scale: f32,
    yaw_sin: f32,
    yaw_cos: f32,
    pitch_sin: f32,
    pitch_cos: f32,
    pan: Vec2,
}

impl Camera2d {
    fn new(rect: Rect, state: &ViewportState, bounds: Option<&SceneBounds>) -> Self {
        let (center, span) = bounds.map_or((Vec3::new(0.0, 0.0, 0.0), 1000.0), |bounds| {
            (bounds.center(), bounds.span().max(1.0))
        });
        let scale = rect.width().min(rect.height()) * 0.62 * state.zoom / span;
        let (yaw_sin, yaw_cos) = state.yaw.sin_cos();
        let (pitch_sin, pitch_cos) = state.pitch.sin_cos();

        Self {
            rect,
            center,
            scale,
            yaw_sin,
            yaw_cos,
            pitch_sin,
            pitch_cos,
            pan: state.pan,
        }
    }

    fn project(&self, point: Vec3) -> Pos2 {
        let x = point.x - self.center.x;
        let y = point.y - self.center.y;
        let z = point.z - self.center.z;

        let rx = x * self.yaw_cos - y * self.yaw_sin;
        let ry = x * self.yaw_sin + y * self.yaw_cos;
        let sy = ry * self.pitch_sin - z * self.pitch_cos;

        self.rect.center() + self.pan + Vec2::new(rx * self.scale, -sy * self.scale)
    }

    fn depth(&self, point: Vec3) -> f32 {
        let x = point.x - self.center.x;
        let y = point.y - self.center.y;
        let z = point.z - self.center.z;
        let ry = x * self.yaw_sin + y * self.yaw_cos;
        ry * self.pitch_cos + z * self.pitch_sin
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
    }
}

struct Face {
    points: Vec<Pos2>,
    fill: Color32,
    stroke: Color32,
    stroke_width: f32,
    depth: f32,
}

fn object_faces(obj: &SceneObject, selection: &Selection, camera: &Camera2d) -> Vec<Face> {
    let bounds = obj.bounds();
    let display = obj.display();
    let base = parse_hex_color(&display.color)
        .gamma_multiply(display.brightness.clamp(0.0, 2.0))
        .linear_multiply(display.opacity.clamp(0.08, 1.0));
    let selected = matches!(selection, Selection::Object(id) if id == obj.id());
    let stroke = if selected {
        Color32::from_rgb(240, 180, 41)
    } else {
        Color32::from_rgb(73, 83, 95)
    };
    let stroke_width = if selected { 2.0 } else { 1.0 };

    let z0 = display.z_min;
    let z1 = display.z_max;
    let corners = [
        Vec3::new(bounds.min_x, bounds.min_y, z0),
        Vec3::new(bounds.max_x, bounds.min_y, z0),
        Vec3::new(bounds.max_x, bounds.max_y, z0),
        Vec3::new(bounds.min_x, bounds.max_y, z0),
        Vec3::new(bounds.min_x, bounds.min_y, z1),
        Vec3::new(bounds.max_x, bounds.min_y, z1),
        Vec3::new(bounds.max_x, bounds.max_y, z1),
        Vec3::new(bounds.min_x, bounds.max_y, z1),
    ];

    let mut faces = Vec::with_capacity(5);
    push_face(
        &mut faces,
        camera,
        [corners[0], corners[1], corners[2], corners[3]],
        shade(base, 0.72),
        stroke,
        stroke_width,
    );
    push_face(
        &mut faces,
        camera,
        [corners[0], corners[4], corners[5], corners[1]],
        shade(base, 0.82),
        stroke,
        stroke_width,
    );
    push_face(
        &mut faces,
        camera,
        [corners[1], corners[5], corners[6], corners[2]],
        shade(base, 0.68),
        stroke,
        stroke_width,
    );
    push_face(
        &mut faces,
        camera,
        [corners[2], corners[6], corners[7], corners[3]],
        shade(base, 0.78),
        stroke,
        stroke_width,
    );
    push_face(
        &mut faces,
        camera,
        [corners[4], corners[7], corners[6], corners[5]],
        shade(base, 1.05),
        stroke,
        stroke_width,
    );
    faces
}

fn push_face(
    faces: &mut Vec<Face>,
    camera: &Camera2d,
    points: [Vec3; 4],
    fill: Color32,
    stroke: Color32,
    stroke_width: f32,
) {
    faces.push(Face {
        points: points.iter().map(|point| camera.project(*point)).collect(),
        fill,
        stroke,
        stroke_width,
        depth: points.iter().map(|point| camera.depth(*point)).sum::<f32>() / 4.0,
    });
}

fn paint_floor_grid(painter: &egui::Painter, camera: &Camera2d) {
    let stroke = Stroke::new(1.0, Color32::from_rgb(221, 227, 234));
    let extent = 1200.0;
    let step = 100.0;
    let mut value = -extent;
    while value <= extent {
        painter.line_segment(
            [
                camera.project(Vec3::new(value, -extent, 0.0)),
                camera.project(Vec3::new(value, extent, 0.0)),
            ],
            stroke,
        );
        painter.line_segment(
            [
                camera.project(Vec3::new(-extent, value, 0.0)),
                camera.project(Vec3::new(extent, value, 0.0)),
            ],
            stroke,
        );
        value += step;
    }
}

fn paint_axes(painter: &egui::Painter, camera: &Camera2d) {
    let origin = Vec3::new(0.0, 0.0, 0.0);
    draw_axis(
        painter,
        camera,
        origin,
        Vec3::new(320.0, 0.0, 0.0),
        "X",
        Color32::from_rgb(196, 57, 57),
    );
    draw_axis(
        painter,
        camera,
        origin,
        Vec3::new(0.0, 320.0, 0.0),
        "Y",
        Color32::from_rgb(55, 132, 78),
    );
    draw_axis(
        painter,
        camera,
        origin,
        Vec3::new(0.0, 0.0, 160.0),
        "Z",
        Color32::from_rgb(57, 99, 196),
    );
}

fn draw_axis(
    painter: &egui::Painter,
    camera: &Camera2d,
    from: Vec3,
    to: Vec3,
    label: &str,
    color: Color32,
) {
    let from = camera.project(from);
    let to = camera.project(to);
    painter.line_segment([from, to], Stroke::new(2.0, color));
    painter.circle_filled(to, 3.0, color);
    painter.text(
        to + Vec2::new(6.0, -6.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::monospace(12.0),
        color,
    );
}

fn scene_bounds(scene: &Scene) -> Option<SceneBounds> {
    let mut bounds = None::<SceneBounds>;
    for obj in scene.objects() {
        let object_bounds =
            object_scene_bounds(obj.bounds(), obj.display().z_min, obj.display().z_max);
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

fn shade(color: Color32, factor: f32) -> Color32 {
    color.gamma_multiply(factor)
}

fn parse_hex_color(value: &str) -> Color32 {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return Color32::from_rgb(45, 108, 223);
    }

    let Ok(rgb) = u32::from_str_radix(hex, 16) else {
        return Color32::from_rgb(45, 108, 223);
    };

    Color32::from_rgb(
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
    )
}
