use std::path::{Path, PathBuf};

use eframe::egui::{self, FontId, TextStyle};

use super::super::{LUCIDE_FONT_FAMILY, clamp_ui_font_size};

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

fn load_font(paths: Vec<PathBuf>) -> Option<egui::FontData> {
    for path in paths {
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        return Some(egui::FontData::from_owned(data));
    }
    None
}

#[cfg(target_os = "windows")]
fn system_ui_font_candidates() -> Vec<PathBuf> {
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
    .map(|file_name| font_dir.join(file_name))
    .collect()
}

#[cfg(target_os = "macos")]
fn system_ui_font_candidates() -> Vec<PathBuf> {
    path_candidates([
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
    ])
}

#[cfg(all(unix, not(target_os = "macos")))]
fn system_ui_font_candidates() -> Vec<PathBuf> {
    let mut candidates = fontconfig_candidates(["sans:lang=zh-cn", "Noto Sans CJK SC"]);
    candidates.extend(path_candidates([
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    ]));
    candidates
}

#[cfg(not(any(unix, target_os = "windows")))]
fn system_ui_font_candidates() -> Vec<PathBuf> {
    Vec::new()
}

fn path_candidates(paths: impl IntoIterator<Item = &'static str>) -> Vec<PathBuf> {
    paths.into_iter().map(PathBuf::from).collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn fontconfig_candidates(font_names: impl IntoIterator<Item = &'static str>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for font_name in font_names {
        let Ok(output) = std::process::Command::new("fc-match")
            .args(["-f", "%{file}", font_name])
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if path.is_empty() {
            continue;
        }
        let path = PathBuf::from(path);
        if is_new_path(&candidates, &path) {
            candidates.push(path);
        }
    }
    candidates
}

#[cfg(all(unix, not(target_os = "macos")))]
fn is_new_path(paths: &[PathBuf], path: &Path) -> bool {
    !paths.iter().any(|candidate| candidate == path)
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
