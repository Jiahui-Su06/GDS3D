use std::path::PathBuf;

use eframe::egui::{self, FontId, TextStyle};

use super::super::{LUCIDE_FONT_FAMILY, clamp_ui_font_size};

#[cfg(all(unix, not(target_os = "macos")))]
const NOTO_CJK_SC_FACE_INDEX: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
struct FontCandidate {
    path: PathBuf,
    face_index: u32,
}

impl FontCandidate {
    fn new(path: impl Into<PathBuf>, face_index: u32) -> Self {
        Self {
            path: path.into(),
            face_index,
        }
    }
}

pub(in crate::app) fn configure_light_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Light));
}

pub(in crate::app) fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "lucide".to_owned(),
        egui::FontData::from_static(lucide_icons::LUCIDE_FONT_BYTES).into(),
    );
    fonts.families.insert(
        egui::FontFamily::Name(LUCIDE_FONT_FAMILY.into()),
        vec!["lucide".to_owned()],
    );

    if let Some(system_font) = load_font(system_ui_font_candidates()) {
        fonts
            .font_data
            .insert("system_ui".to_owned(), system_font.into());
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, "system_ui".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

fn load_font(candidates: Vec<FontCandidate>) -> Option<egui::FontData> {
    for candidate in candidates {
        let Ok(data) = std::fs::read(&candidate.path) else {
            continue;
        };
        let mut font = egui::FontData::from_owned(data);
        font.index = candidate.face_index;
        return Some(font);
    }
    None
}

#[cfg(target_os = "windows")]
fn system_ui_font_candidates() -> Vec<FontCandidate> {
    let font_dir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Windows"))
        .join("Fonts");
    [
        "msyh.ttc",
        "msyhl.ttc",
        "msyhbd.ttc",
        "simhei.ttf",
        "simsun.ttc",
    ]
    .into_iter()
    .map(|file_name| FontCandidate::new(font_dir.join(file_name), 0))
    .collect()
}

#[cfg(target_os = "macos")]
fn system_ui_font_candidates() -> Vec<FontCandidate> {
    path_candidates([
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
    ])
}

#[cfg(all(unix, not(target_os = "macos")))]
fn system_ui_font_candidates() -> Vec<FontCandidate> {
    let mut candidates = fontconfig_candidates(["sans:lang=zh-cn", "Noto Sans CJK SC"]);
    candidates.extend([
        FontCandidate::new(
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            NOTO_CJK_SC_FACE_INDEX,
        ),
        FontCandidate::new(
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            NOTO_CJK_SC_FACE_INDEX,
        ),
        FontCandidate::new(
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            NOTO_CJK_SC_FACE_INDEX,
        ),
        FontCandidate::new(
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            NOTO_CJK_SC_FACE_INDEX,
        ),
    ]);
    candidates
}

#[cfg(not(any(unix, target_os = "windows")))]
fn system_ui_font_candidates() -> Vec<FontCandidate> {
    Vec::new()
}

#[cfg(target_os = "macos")]
fn path_candidates(paths: impl IntoIterator<Item = &'static str>) -> Vec<FontCandidate> {
    paths
        .into_iter()
        .map(|path| FontCandidate::new(path, 0))
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn fontconfig_candidates(font_names: impl IntoIterator<Item = &'static str>) -> Vec<FontCandidate> {
    let mut candidates = Vec::new();
    for font_name in font_names {
        let Ok(output) = std::process::Command::new("fc-match")
            .args(["-f", "%{file}\t%{index}", font_name])
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let Some(candidate) = parse_fontconfig_candidate(&output.stdout) else {
            continue;
        };
        if candidates.iter().any(|existing: &FontCandidate| {
            existing.path == candidate.path && existing.face_index == candidate.face_index
        }) {
            continue;
        }
        candidates.push(candidate);
    }
    candidates
}

#[cfg(all(unix, not(target_os = "macos")))]
fn parse_fontconfig_candidate(output: &[u8]) -> Option<FontCandidate> {
    let output = std::str::from_utf8(output).ok()?.trim_end();
    let mut fields = output.split('\t');
    let path = fields.next()?;
    let face_index = fields.next()?.parse().ok()?;
    if path.is_empty() || fields.next().is_some() {
        return None;
    }
    Some(FontCandidate {
        path: PathBuf::from(path),
        face_index,
    })
}

pub(in crate::app) fn configure_industrial_style(ctx: &egui::Context, ui_font_size: f32) {
    let ui_font_size = clamp_ui_font_size(ui_font_size);
    let mut style = (*ctx.style_of(egui::Theme::Light)).clone();
    style.spacing.item_spacing = egui::vec2(6.0, 3.0);
    style.spacing.button_padding = egui::vec2(6.0, 2.0);
    style.spacing.indent = 14.0;
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(ui_font_size + 3.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(ui_font_size, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(ui_font_size, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(
            (ui_font_size - 2.0).max(10.0),
            egui::FontFamily::Proportional,
        ),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(ui_font_size, egui::FontFamily::Monospace),
    );
    style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(242, 245, 248);
    style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(214, 221, 229);
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(188, 199, 212);
    style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(96, 111, 128);
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(72, 89, 108);
    style.visuals.panel_fill = egui::Color32::from_rgb(238, 242, 246);
    style.visuals.window_fill = egui::Color32::from_rgb(246, 248, 250);
    style.visuals.extreme_bg_color = egui::Color32::from_rgb(224, 229, 235);
    style.visuals.indent_has_left_vline = false;
    ctx.set_style_of(egui::Theme::Light, style.clone());
    ctx.set_style_of(egui::Theme::Dark, style);
}

#[cfg(test)]
mod tests {
    #[cfg(all(unix, not(target_os = "macos")))]
    use super::{FontCandidate, parse_fontconfig_candidate};

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn parses_fontconfig_face_index() {
        let parsed =
            parse_fontconfig_candidate(b"/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc\t2");
        assert_eq!(
            parsed,
            Some(FontCandidate {
                path: "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc".into(),
                face_index: 2,
            })
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn rejects_invalid_fontconfig_output() {
        assert!(parse_fontconfig_candidate(b"/font.ttc\tnot-a-number").is_none());
        assert!(parse_fontconfig_candidate(b"/font.ttc").is_none());
        assert!(parse_fontconfig_candidate(b"\t0").is_none());
        assert!(parse_fontconfig_candidate(b"/font.ttc\t0\textra").is_none());
    }
}
