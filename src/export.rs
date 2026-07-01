use std::fs;
use std::path::Path;

use anyhow::bail;
use printpdf::{
    BuiltinFont, Color, Line, LinePoint, Mm, Op, PaintMode, PdfDocument, PdfPage, PdfSaveOptions,
    Point, Pt, RawImage, Rect, Rgb, TextItem, XObjectId, XObjectTransform, ops::PdfFontHandle,
};
use rust_i18n::t;
use serde::{Deserialize, Serialize};

use crate::model::{Bounds2d, Scene, SceneObject};

const EXPORT_MARGIN_FACTOR: f32 = 0.04;
const EXPORT_MARGIN_MIN_PX: f32 = 24.0;
const IMAGE_PIXELS_MAX: u64 = 64_000_000;
const PDF_PAGE_WIDTH_MM: f32 = 210.0;
const PDF_PAGE_HEIGHT_MM: f32 = 297.0;
const PDF_MARGIN_MM: f32 = 14.0;
const PDF_IMAGE_DPI: f32 = 300.0;
const PDF_TABLE_FONT_SIZE_PT: f32 = 7.0;
const PDF_TABLE_HEADER_FONT_SIZE_PT: f32 = 7.0;
const PDF_TABLE_ROW_HEIGHT_PT: f32 = 18.0;
const PDF_TABLE_HEADER_HEIGHT_PT: f32 = 20.0;
const PDF_CELL_PADDING_PT: f32 = 3.0;
const MM_TO_PT: f32 = 72.0 / 25.4;

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
        ExportFormat::Pdf => bail!("PDF export is handled by the viewport renderer"),
        ExportFormat::Gltf => bail!("glTF export is not implemented"),
    }
}

pub fn write_pdf_report(
    path: &Path,
    scene: &Scene,
    png: &[u8],
    image_width: u32,
    image_height: u32,
) -> anyhow::Result<()> {
    if image_width == 0 || image_height == 0 {
        bail!("PDF image size must be non-zero");
    }
    if u64::from(image_width) * u64::from(image_height) > IMAGE_PIXELS_MAX {
        bail!("PDF image is too large: {image_width} x {image_height}");
    }

    let rows = pdf_table_rows(scene);
    let mut doc = PdfDocument::new("GDS3D Export");
    let image = RawImage::decode_from_bytes(png, &mut Vec::new())
        .map_err(|err| anyhow::anyhow!("decode PDF preview image: {err}"))?;
    let image_id = doc.add_image(&image);
    let pages = vec![
        pdf_image_page(image_id, image_width, image_height),
        pdf_table_page(&rows),
    ];
    let bytes = doc
        .with_pages(pages)
        .save(&PdfSaveOptions::default(), &mut Vec::new());
    fs::write(path, bytes)?;
    Ok(())
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
    svg.push_str(&format!(r#"" fill="{}"/>"#, color_hex(obj.color)));
    svg.push('\n');
}

struct PdfTableRow {
    cells: [String; 8],
}

fn pdf_table_rows(scene: &Scene) -> Vec<PdfTableRow> {
    let mut rows = Vec::new();
    for obj in scene.objects() {
        if !obj.is_visible() {
            continue;
        }

        let bounds = obj.bounds();
        let display = obj.display();
        match obj {
            SceneObject::GdsLayer(layer) => rows.push(PdfTableRow {
                cells: [
                    display.name.clone(),
                    "GDS".to_owned(),
                    layer.cell_name.clone(),
                    layer.layer.to_string(),
                    layer.datatype.to_string(),
                    range_text(bounds.min_x, bounds.max_x),
                    range_text(bounds.min_y, bounds.max_y),
                    range_text(display.z_min, display.z_max),
                ],
            }),
            SceneObject::Baseplate(_) => rows.push(PdfTableRow {
                cells: [
                    display.name.clone(),
                    "Baseplate".to_owned(),
                    String::new(),
                    String::new(),
                    String::new(),
                    range_text(bounds.min_x, bounds.max_x),
                    range_text(bounds.min_y, bounds.max_y),
                    range_text(display.z_min, display.z_max),
                ],
            }),
        }
    }
    rows
}

fn pdf_image_page(image_id: XObjectId, image_width: u32, image_height: u32) -> PdfPage {
    let page_width_pt = mm_to_pt(PDF_PAGE_WIDTH_MM);
    let page_height_pt = mm_to_pt(PDF_PAGE_HEIGHT_MM);
    let margin_pt = mm_to_pt(PDF_MARGIN_MM);
    let content_width_pt = page_width_pt - margin_pt * 2.0;
    let content_height_pt = page_height_pt - margin_pt * 2.0;
    let image_limit_height_pt = (content_height_pt * 0.72).min(content_width_pt * 0.65);
    let base_width_pt = px_to_pt(image_width, PDF_IMAGE_DPI);
    let base_height_pt = px_to_pt(image_height, PDF_IMAGE_DPI);
    let scale = (content_width_pt / base_width_pt).min(image_limit_height_pt / base_height_pt);
    let drawn_width_pt = base_width_pt * scale;
    let drawn_height_pt = base_height_pt * scale;
    let x_pt = margin_pt + (content_width_pt - drawn_width_pt) * 0.5;
    let y_pt = page_height_pt - margin_pt - drawn_height_pt;

    let ops = vec![
        Op::SetFillColor {
            col: pdf_rgb(0xFF, 0xFF, 0xFF),
        },
        Op::DrawRectangle {
            rectangle: Rect {
                x: Pt(0.0),
                y: Pt(0.0),
                width: Pt(page_width_pt),
                height: Pt(page_height_pt),
                mode: Some(PaintMode::Fill),
                winding_order: None,
            },
        },
        Op::UseXobject {
            id: image_id,
            transform: XObjectTransform {
                translate_x: Some(Pt(x_pt)),
                translate_y: Some(Pt(y_pt)),
                rotate: None,
                scale_x: Some(scale),
                scale_y: Some(scale),
                dpi: Some(PDF_IMAGE_DPI),
            },
        },
    ];
    PdfPage::new(Mm(PDF_PAGE_WIDTH_MM), Mm(PDF_PAGE_HEIGHT_MM), ops)
}

fn pdf_table_page(rows: &[PdfTableRow]) -> PdfPage {
    let page_width_pt = mm_to_pt(PDF_PAGE_WIDTH_MM);
    let page_height_pt = mm_to_pt(PDF_PAGE_HEIGHT_MM);
    let margin_pt = mm_to_pt(PDF_MARGIN_MM);
    let table_width_pt = page_width_pt - margin_pt * 2.0;
    let column_ratios = [0.16, 0.11, 0.15, 0.08, 0.09, 0.14, 0.14, 0.13];
    let column_widths = column_widths(table_width_pt, column_ratios);
    let max_rows = ((page_height_pt - margin_pt * 2.0 - PDF_TABLE_HEADER_HEIGHT_PT)
        / PDF_TABLE_ROW_HEIGHT_PT)
        .floor()
        .max(0.0) as usize;
    let row_count = rows.len().min(max_rows);
    let table_height_pt = PDF_TABLE_HEADER_HEIGHT_PT + PDF_TABLE_ROW_HEIGHT_PT * row_count as f32;
    let top_y_pt = page_height_pt - margin_pt;
    let table_x_pt = margin_pt;
    let table_y_pt = top_y_pt - table_height_pt;

    let mut ops = Vec::new();
    ops.push(Op::SetFillColor {
        col: pdf_rgb(0xFF, 0xFF, 0xFF),
    });
    push_pdf_rect(
        &mut ops,
        table_x_pt,
        table_y_pt,
        table_width_pt,
        table_height_pt,
        PaintMode::Fill,
    );
    ops.push(Op::SetFillColor {
        col: pdf_rgb(0xDF, 0xE6, 0xEE),
    });
    push_pdf_rect(
        &mut ops,
        table_x_pt,
        top_y_pt - PDF_TABLE_HEADER_HEIGHT_PT,
        table_width_pt,
        PDF_TABLE_HEADER_HEIGHT_PT,
        PaintMode::Fill,
    );

    for row_index in 0..row_count {
        if row_index % 2 == 1 {
            let y_pt = top_y_pt
                - PDF_TABLE_HEADER_HEIGHT_PT
                - PDF_TABLE_ROW_HEIGHT_PT * (row_index + 1) as f32;
            ops.push(Op::SetFillColor {
                col: pdf_rgb(0xF5, 0xF7, 0xFA),
            });
            push_pdf_rect(
                &mut ops,
                table_x_pt,
                y_pt,
                table_width_pt,
                PDF_TABLE_ROW_HEIGHT_PT,
                PaintMode::Fill,
            );
        }
    }

    ops.push(Op::SetOutlineColor {
        col: pdf_rgb(0xAE, 0xB8, 0xC4),
    });
    ops.push(Op::SetOutlineThickness { pt: Pt(0.35) });
    push_table_grid(
        &mut ops,
        table_x_pt,
        table_y_pt,
        top_y_pt,
        &column_widths,
        row_count,
    );

    let headers = [
        "Name", "Kind", "Cell", "Layer", "Datatype", "X bounds", "Y bounds", "Z bounds",
    ];
    let mut cell_x_pt = table_x_pt;
    for (column_index, header) in headers.iter().enumerate() {
        push_pdf_text(
            &mut ops,
            header,
            cell_x_pt + PDF_CELL_PADDING_PT,
            top_y_pt - 12.0,
            PDF_TABLE_HEADER_FONT_SIZE_PT,
            true,
            column_widths[column_index] - PDF_CELL_PADDING_PT * 2.0,
        );
        cell_x_pt += column_widths[column_index];
    }

    for (row_index, row) in rows.iter().take(row_count).enumerate() {
        let text_y_pt = top_y_pt
            - PDF_TABLE_HEADER_HEIGHT_PT
            - PDF_TABLE_ROW_HEIGHT_PT * row_index as f32
            - 11.5;
        let mut cell_x_pt = table_x_pt;
        for (column_index, cell) in row.cells.iter().enumerate() {
            push_pdf_text(
                &mut ops,
                cell,
                cell_x_pt + PDF_CELL_PADDING_PT,
                text_y_pt,
                PDF_TABLE_FONT_SIZE_PT,
                false,
                column_widths[column_index] - PDF_CELL_PADDING_PT * 2.0,
            );
            cell_x_pt += column_widths[column_index];
        }
    }

    PdfPage::new(Mm(PDF_PAGE_WIDTH_MM), Mm(PDF_PAGE_HEIGHT_MM), ops)
}

fn push_table_grid(
    ops: &mut Vec<Op>,
    table_x_pt: f32,
    table_y_pt: f32,
    top_y_pt: f32,
    column_widths: &[f32; 8],
    row_count: usize,
) {
    let table_width_pt = column_widths.iter().sum::<f32>();
    let mut x_pt = table_x_pt;
    push_pdf_line(ops, x_pt, table_y_pt, x_pt, top_y_pt);
    for width_pt in column_widths {
        x_pt += width_pt;
        push_pdf_line(ops, x_pt, table_y_pt, x_pt, top_y_pt);
    }

    push_pdf_line(
        ops,
        table_x_pt,
        top_y_pt,
        table_x_pt + table_width_pt,
        top_y_pt,
    );
    let mut y_pt = top_y_pt - PDF_TABLE_HEADER_HEIGHT_PT;
    push_pdf_line(ops, table_x_pt, y_pt, table_x_pt + table_width_pt, y_pt);
    for _ in 0..row_count {
        y_pt -= PDF_TABLE_ROW_HEIGHT_PT;
        push_pdf_line(ops, table_x_pt, y_pt, table_x_pt + table_width_pt, y_pt);
    }
}

fn push_pdf_text(
    ops: &mut Vec<Op>,
    text: &str,
    x_pt: f32,
    y_pt: f32,
    size_pt: f32,
    bold: bool,
    max_width_pt: f32,
) {
    let text = truncate_pdf_text(text, size_pt, max_width_pt);
    let font = if bold {
        BuiltinFont::HelveticaBold
    } else {
        BuiltinFont::Helvetica
    };
    ops.push(Op::SetFillColor {
        col: pdf_rgb(0x1F, 0x23, 0x28),
    });
    ops.push(Op::SetFont {
        font: PdfFontHandle::Builtin(font),
        size: Pt(size_pt),
    });
    ops.push(Op::StartTextSection);
    ops.push(Op::SetTextCursor {
        pos: Point {
            x: Pt(x_pt),
            y: Pt(y_pt),
        },
    });
    ops.push(Op::ShowText {
        items: vec![TextItem::Text(text)],
    });
    ops.push(Op::EndTextSection);
}

fn push_pdf_line(ops: &mut Vec<Op>, x1_pt: f32, y1_pt: f32, x2_pt: f32, y2_pt: f32) {
    ops.push(Op::DrawLine {
        line: Line {
            points: vec![
                LinePoint {
                    p: Point {
                        x: Pt(x1_pt),
                        y: Pt(y1_pt),
                    },
                    bezier: false,
                },
                LinePoint {
                    p: Point {
                        x: Pt(x2_pt),
                        y: Pt(y2_pt),
                    },
                    bezier: false,
                },
            ],
            is_closed: false,
        },
    });
}

fn push_pdf_rect(
    ops: &mut Vec<Op>,
    x_pt: f32,
    y_pt: f32,
    width_pt: f32,
    height_pt: f32,
    mode: PaintMode,
) {
    ops.push(Op::DrawRectangle {
        rectangle: Rect {
            x: Pt(x_pt),
            y: Pt(y_pt),
            width: Pt(width_pt),
            height: Pt(height_pt),
            mode: Some(mode),
            winding_order: None,
        },
    });
}

fn column_widths(total_width_pt: f32, ratios: [f32; 8]) -> [f32; 8] {
    let mut widths = [0.0; 8];
    for (index, ratio) in ratios.iter().enumerate() {
        widths[index] = total_width_pt * ratio;
    }
    widths
}

fn truncate_pdf_text(text: &str, size_pt: f32, max_width_pt: f32) -> String {
    let char_width_pt = size_pt * 0.52;
    let max_chars = (max_width_pt / char_width_pt).floor().max(1.0) as usize;
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_owned();
    }
    if max_chars <= 1 {
        return ".".to_owned();
    }

    let mut out = text.chars().take(max_chars - 1).collect::<String>();
    out.push('.');
    out
}

fn pdf_rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(Rgb::new(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        None,
    ))
}

fn range_text(min_value: f32, max_value: f32) -> String {
    format!("{min_value:.2}, {max_value:.2}")
}

fn mm_to_pt(value_mm: f32) -> f32 {
    value_mm * MM_TO_PT
}

fn px_to_pt(value_px: u32, dpi: f32) -> f32 {
    value_px as f32 * 72.0 / dpi
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
        ExportFormat, ExportQuality, ExportSettings, ExportSizePreset, write_pdf_report,
        write_scene_export,
    };
    use crate::model::{
        Bounds2d, DisplayProperties, GdsLayerObject, Polygon2d, Scene, SceneObject, new_baseplate,
        new_object_id,
    };

    #[test]
    fn writes_svg_export() {
        let temp_dir = std::env::temp_dir().join(format!("gds3d-export-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let scene = test_scene();

        let path = temp_dir.join("scene.svg");
        write_scene_export(
            &path,
            &scene,
            ExportSettings {
                format: ExportFormat::Svg,
                quality: ExportQuality::Low,
                size_preset: ExportSizePreset::Square1x1,
            },
        )
        .expect("write svg export");
        let data = fs::read(&path).expect("read svg export");
        assert!(data.starts_with(b"<?xml"));

        fs::remove_dir_all(&temp_dir).expect("remove temp dir");
    }

    #[test]
    fn writes_two_page_pdf_report() {
        let temp_dir = std::env::temp_dir().join(format!("gds3d-export-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let scene = test_scene();
        let png = gds3d_viewport::encode_rgba_png(1, 1, &[255, 255, 255, 255]).expect("encode png");

        let path = temp_dir.join("scene.pdf");
        write_pdf_report(&path, &scene, &png, 1, 1).expect("write pdf report");
        let data = fs::read(&path).expect("read pdf export");
        assert!(data.starts_with(b"%PDF"));

        let mut warnings = Vec::new();
        let doc = printpdf::PdfDocument::parse(
            &data,
            &printpdf::PdfParseOptions::default(),
            &mut warnings,
        )
        .expect("parse pdf export");
        assert_eq!(doc.pages.len(), 2);

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
