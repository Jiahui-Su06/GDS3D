use std::fs;
use std::path::Path;

use anyhow::bail;
use rust_i18n::t;
use serde::{Deserialize, Serialize};

use crate::model::{Bounds2d, Scene, SceneObject};

const EXPORT_MARGIN_FACTOR: f32 = 0.04;
const EXPORT_MARGIN_MIN_PX: f32 = 24.0;
const IMAGE_PIXELS_MAX: u64 = 64_000_000;
const PDF_POINTS_PER_PIXEL: f32 = 72.0 / 96.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    Png,
    Svg,
    Pdf,
    Gltf,
}

impl ExportFormat {
    pub const ALL: [Self; 3] = [Self::Png, Self::Svg, Self::Pdf];

    pub fn label(self) -> String {
        match self {
            Self::Png => t!("export.format_png").to_string(),
            Self::Svg => t!("export.format_svg").to_string(),
            Self::Pdf => t!("export.format_pdf").to_string(),
            Self::Gltf => t!("export.format_gltf").to_string(),
        }
    }

    pub fn needs_image_size(self) -> bool {
        matches!(self, Self::Png | Self::Svg | Self::Pdf)
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Svg => "svg",
            Self::Pdf => "pdf",
            Self::Gltf => "gltf",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportQuality {
    Low,
    Standard,
    High,
}

impl ExportQuality {
    pub const ALL: [Self; 3] = [Self::Low, Self::Standard, Self::High];

    pub fn label(self) -> String {
        match self {
            Self::Low => t!("export.quality_low").to_string(),
            Self::Standard => t!("export.quality_standard").to_string(),
            Self::High => t!("export.quality_high").to_string(),
        }
    }

    fn width(self) -> u32 {
        match self {
            Self::Low => 2000,
            Self::Standard => 4000,
            Self::High => 6000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportSizePreset {
    Figure4x3,
    Figure3x2,
    Wide16x9,
    Square1x1,
}

impl ExportSizePreset {
    pub const ALL: [Self; 4] = [
        Self::Figure4x3,
        Self::Figure3x2,
        Self::Wide16x9,
        Self::Square1x1,
    ];

    pub fn label(self) -> String {
        match self {
            Self::Figure4x3 => t!("export.size_figure_4_3").to_string(),
            Self::Figure3x2 => t!("export.size_figure_3_2").to_string(),
            Self::Wide16x9 => t!("export.size_wide_16_9").to_string(),
            Self::Square1x1 => t!("export.size_square_1_1").to_string(),
        }
    }

    fn ratio(self) -> (u32, u32) {
        match self {
            Self::Figure4x3 => (4, 3),
            Self::Figure3x2 => (3, 2),
            Self::Wide16x9 => (16, 9),
            Self::Square1x1 => (1, 1),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportSettings {
    pub format: ExportFormat,
    pub quality: ExportQuality,
    pub size_preset: ExportSizePreset,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            format: ExportFormat::Png,
            quality: ExportQuality::Standard,
            size_preset: ExportSizePreset::Figure4x3,
        }
    }
}

impl ExportSettings {
    pub fn image_size(self) -> Option<(u32, u32)> {
        if !self.format.needs_image_size() {
            return None;
        }

        let width = self.quality.width();
        let (ratio_w, ratio_h) = self.size_preset.ratio();
        let height = width * ratio_h / ratio_w;
        Some((width, height))
    }
}

pub fn write_scene_export(
    path: &Path,
    scene: &Scene,
    settings: ExportSettings,
) -> anyhow::Result<()> {
    let Some((width, height)) = settings.image_size() else {
        bail!("unsupported export format: {}", settings.format.label());
    };
    if u64::from(width) * u64::from(height) > IMAGE_PIXELS_MAX {
        bail!("export image is too large: {width} x {height}");
    }

    let objects = export_objects(scene)?;
    let frame = ExportFrame::new(width, height, &objects)?;
    match settings.format {
        ExportFormat::Png => bail!("PNG export is handled by the viewport renderer"),
        ExportFormat::Svg => write_svg(path, width, height, &objects, &frame),
        ExportFormat::Pdf => write_pdf(path, width, height, &objects, &frame),
        ExportFormat::Gltf => bail!("glTF export is not implemented"),
    }
}

fn export_objects(scene: &Scene) -> anyhow::Result<Vec<ExportObject<'_>>> {
    let mut objects = Vec::new();
    for obj in scene.objects() {
        if !obj.is_visible() {
            continue;
        }
        let polygons = match obj {
            SceneObject::GdsLayer(layer) => Some(layer.polygons.as_slice()),
            SceneObject::Baseplate(_) => None,
        };
        objects.push(ExportObject {
            bounds: obj.bounds(),
            color: export_color(obj.display().color.as_str(), obj.display().brightness),
            opacity: obj.display().opacity.clamp(0.0, 1.0),
            z_min: obj.display().z_min,
            polygons,
        });
    }

    if objects.is_empty() {
        bail!("no visible objects to export");
    }

    objects.sort_by(|a, b| {
        a.z_min
            .partial_cmp(&b.z_min)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(objects)
}

#[derive(Clone, Copy)]
struct ExportColor {
    r: u8,
    g: u8,
    b: u8,
}

struct ExportObject<'a> {
    bounds: &'a Bounds2d,
    color: ExportColor,
    opacity: f32,
    z_min: f32,
    polygons: Option<&'a [crate::model::Polygon2d]>,
}

struct ExportFrame {
    bounds: Bounds2d,
    height: u32,
    scale: f32,
    x_offset: f32,
    y_offset: f32,
}

impl ExportFrame {
    fn new(width: u32, height: u32, objects: &[ExportObject<'_>]) -> anyhow::Result<Self> {
        let mut bounds = None;
        for obj in objects {
            include_bounds(&mut bounds, obj.bounds);
        }
        let bounds = bounds.expect("export objects are non-empty");
        let world_width = bounds.max_x - bounds.min_x;
        let world_height = bounds.max_y - bounds.min_y;
        if world_width <= 0.0 || world_height <= 0.0 {
            bail!("scene bounds are empty");
        }

        let margin = ((width.min(height) as f32) * EXPORT_MARGIN_FACTOR).max(EXPORT_MARGIN_MIN_PX);
        let content_width = (width as f32 - margin * 2.0).max(1.0);
        let content_height = (height as f32 - margin * 2.0).max(1.0);
        let scale = (content_width / world_width).min(content_height / world_height);
        let drawn_width = world_width * scale;
        let drawn_height = world_height * scale;
        let x_offset = (width as f32 - drawn_width) * 0.5;
        let y_offset = (height as f32 - drawn_height) * 0.5;

        Ok(Self {
            bounds,
            height,
            scale,
            x_offset,
            y_offset,
        })
    }

    fn image_point(&self, point: [f32; 2]) -> (f32, f32) {
        let x = self.x_offset + (point[0] - self.bounds.min_x) * self.scale;
        let y = self.height as f32 - self.y_offset - (point[1] - self.bounds.min_y) * self.scale;
        (x, y)
    }

    fn pdf_point(&self, point: [f32; 2]) -> (f32, f32) {
        let x =
            (self.x_offset + (point[0] - self.bounds.min_x) * self.scale) * PDF_POINTS_PER_PIXEL;
        let y =
            (self.y_offset + (point[1] - self.bounds.min_y) * self.scale) * PDF_POINTS_PER_PIXEL;
        (x, y)
    }

    fn rectangle_points(&self, bounds: &Bounds2d) -> [[f32; 2]; 4] {
        [
            [bounds.min_x, bounds.min_y],
            [bounds.max_x, bounds.min_y],
            [bounds.max_x, bounds.max_y],
            [bounds.min_x, bounds.max_y],
        ]
    }
}

fn write_svg(
    path: &Path,
    width: u32,
    height: u32,
    objects: &[ExportObject<'_>],
    frame: &ExportFrame,
) -> anyhow::Result<()> {
    let mut svg = String::new();
    svg.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    svg.push('\n');
    svg.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"#
    ));
    svg.push('\n');
    svg.push_str(r##"<rect width="100%" height="100%" fill="#F8FAFC"/>"##);
    svg.push('\n');
    for obj in objects {
        if let Some(polygons) = obj.polygons {
            for polygon in polygons {
                push_svg_path(&mut svg, polygon.points.as_slice(), obj, frame);
            }
        } else {
            push_svg_path(&mut svg, &frame.rectangle_points(obj.bounds), obj, frame);
        }
    }
    svg.push_str("</svg>\n");
    fs::write(path, svg)?;
    Ok(())
}

fn push_svg_path(
    svg: &mut String,
    points: &[[f32; 2]],
    obj: &ExportObject<'_>,
    frame: &ExportFrame,
) {
    if points.len() < 3 {
        return;
    }

    svg.push_str(r#"<path d=""#);
    for (index, point) in points.iter().enumerate() {
        let (x, y) = frame.image_point(*point);
        if index == 0 {
            svg.push_str(&format!("M {:.3} {:.3}", x, y));
        } else {
            svg.push_str(&format!(" L {:.3} {:.3}", x, y));
        }
    }
    svg.push_str(" Z");
    svg.push_str(&format!(
        r#"" fill="{}" fill-opacity="{:.3}"/>"#,
        color_hex(obj.color),
        obj.opacity
    ));
    svg.push('\n');
}

fn write_pdf(
    path: &Path,
    width: u32,
    height: u32,
    objects: &[ExportObject<'_>],
    frame: &ExportFrame,
) -> anyhow::Result<()> {
    let page_width = width as f32 * PDF_POINTS_PER_PIXEL;
    let page_height = height as f32 * PDF_POINTS_PER_PIXEL;
    let mut content = String::new();
    content.push_str("q\n0.973 0.980 0.988 rg\n");
    content.push_str(&format!(
        "0 0 {:.3} {:.3} re f\nQ\n",
        page_width, page_height
    ));
    for obj in objects {
        if let Some(polygons) = obj.polygons {
            for polygon in polygons {
                push_pdf_path(&mut content, polygon.points.as_slice(), obj, frame);
            }
        } else {
            push_pdf_path(
                &mut content,
                &frame.rectangle_points(obj.bounds),
                obj,
                frame,
            );
        }
    }

    let mut pdf = PdfBuilder::new();
    let catalog_id = pdf.add_object("<< /Type /Catalog /Pages 2 0 R >>".to_owned());
    assert_eq!(catalog_id, 1);
    pdf.add_object("<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned());
    pdf.add_object(format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.3} {:.3}] /Contents 4 0 R >>",
        page_width, page_height
    ));
    pdf.add_stream(content.into_bytes());
    fs::write(path, pdf.finish())?;
    Ok(())
}

fn push_pdf_path(
    content: &mut String,
    points: &[[f32; 2]],
    obj: &ExportObject<'_>,
    frame: &ExportFrame,
) {
    if points.len() < 3 {
        return;
    }

    let r = obj.color.r as f32 / 255.0;
    let g = obj.color.g as f32 / 255.0;
    let b = obj.color.b as f32 / 255.0;
    content.push_str("q\n");
    content.push_str(&format!("{:.3} {:.3} {:.3} rg\n", r, g, b));
    for (index, point) in points.iter().enumerate() {
        let (x, y) = frame.pdf_point(*point);
        if index == 0 {
            content.push_str(&format!("{:.3} {:.3} m\n", x, y));
        } else {
            content.push_str(&format!("{:.3} {:.3} l\n", x, y));
        }
    }
    content.push_str("h f\nQ\n");
}

struct PdfBuilder {
    objects: Vec<Vec<u8>>,
}

impl PdfBuilder {
    fn new() -> Self {
        Self {
            objects: Vec::new(),
        }
    }

    fn add_object(&mut self, body: String) -> usize {
        self.objects.push(body.into_bytes());
        self.objects.len()
    }

    fn add_stream(&mut self, data: Vec<u8>) -> usize {
        let mut body = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
        body.extend_from_slice(&data);
        body.extend_from_slice(b"\nendstream");
        self.objects.push(body);
        self.objects.len()
    }

    fn finish(self) -> Vec<u8> {
        let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(self.objects.len());
        for (index, object) in self.objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(object);
            out.extend_from_slice(b"\nendobj\n");
        }

        let xref_offset = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", self.objects.len() + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                self.objects.len() + 1,
                xref_offset
            )
            .as_bytes(),
        );
        out
    }
}

fn include_bounds(target: &mut Option<Bounds2d>, bounds: &Bounds2d) {
    match target {
        Some(target) => {
            target.min_x = target.min_x.min(bounds.min_x);
            target.min_y = target.min_y.min(bounds.min_y);
            target.max_x = target.max_x.max(bounds.max_x);
            target.max_y = target.max_y.max(bounds.max_y);
        }
        None => *target = Some(bounds.clone()),
    }
}

fn export_color(value: &str, brightness: f32) -> ExportColor {
    let fallback = ExportColor {
        r: 0x2d,
        g: 0x6c,
        b: 0xdf,
    };
    let Some(hex) = value.strip_prefix('#') else {
        return fallback;
    };
    if hex.len() != 6 {
        return fallback;
    }
    let Ok(raw) = u32::from_str_radix(hex, 16) else {
        return fallback;
    };

    let brightness = brightness.clamp(0.0, 4.0);
    ExportColor {
        r: scale_channel(((raw >> 16) & 0xff) as u8, brightness),
        g: scale_channel(((raw >> 8) & 0xff) as u8, brightness),
        b: scale_channel((raw & 0xff) as u8, brightness),
    }
}

fn scale_channel(value: u8, brightness: f32) -> u8 {
    ((value as f32 * brightness).round()).clamp(0.0, 255.0) as u8
}

fn color_hex(color: ExportColor) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::export::{
        ExportFormat, ExportQuality, ExportSettings, ExportSizePreset, write_scene_export,
    };
    use crate::model::{
        Bounds2d, DisplayProperties, GdsLayerObject, Polygon2d, Scene, SceneObject, new_baseplate,
        new_object_id,
    };

    #[test]
    fn writes_svg_and_pdf_exports() {
        let temp_dir = std::env::temp_dir().join(format!("gds3d-export-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let scene = test_scene();

        for format in [ExportFormat::Svg, ExportFormat::Pdf] {
            let path = temp_dir.join(format!("scene.{}", format.extension()));
            write_scene_export(
                &path,
                &scene,
                ExportSettings {
                    format,
                    quality: ExportQuality::Low,
                    size_preset: ExportSizePreset::Square1x1,
                },
            )
            .expect("write scene export");
            let data = fs::read(&path).expect("read export");
            assert!(!data.is_empty());
            match format {
                ExportFormat::Svg => assert!(data.starts_with(b"<?xml")),
                ExportFormat::Pdf => assert!(data.starts_with(b"%PDF-1.4")),
                ExportFormat::Png => unreachable!(),
                ExportFormat::Gltf => unreachable!(),
            }
        }

        fs::remove_dir_all(&temp_dir).expect("remove temp dir");
    }

    fn test_scene() -> Scene {
        let mut scene = Scene::default();
        let bounds = Bounds2d {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 100.0,
            max_y: 100.0,
        };
        scene
            .add(new_baseplate("Baseplate 1", bounds.clone()))
            .expect("add baseplate");
        scene
            .add(SceneObject::GdsLayer(GdsLayerObject {
                id: new_object_id(),
                display: DisplayProperties::gds_layer("L4/1"),
                file_path: PathBuf::from("models/test.gds"),
                source_path: PathBuf::from("models/test.gds"),
                source_key: "test.gds".to_owned(),
                cell_name: "AWG".to_owned(),
                layer: 4,
                datatype: 1,
                bounds,
                polygons: vec![Polygon2d {
                    points: vec![[10.0, 10.0], [90.0, 10.0], [90.0, 90.0], [10.0, 90.0]],
                }],
            }))
            .expect("add gds layer");
        scene
    }
}
