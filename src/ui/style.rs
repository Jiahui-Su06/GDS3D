use eframe::egui::{self, FontId, TextStyle};

use super::super::{LUCIDE_FONT_FAMILY, clamp_ui_font_size};

pub(in crate::app) fn configure_light_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Light));
}

pub(in crate::app) fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "noto_sans".to_owned(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/NotoSans-Regular.ttf"))
            .into(),
    );
    fonts.font_data.insert(
        "noto_sans_cjk".to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/NotoSansCJKsc-Regular.otf"
        ))
        .into(),
    );
    fonts.font_data.insert(
        "lucide".to_owned(),
        egui::FontData::from_static(lucide_icons::LUCIDE_FONT_BYTES).into(),
    );
    fonts.families.insert(
        egui::FontFamily::Name(LUCIDE_FONT_FAMILY.into()),
        vec!["lucide".to_owned()],
    );

    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family.clone())
            .or_default()
            .insert(0, "noto_sans".to_owned());
        fonts
            .families
            .entry(family)
            .or_default()
            .push("noto_sans_cjk".to_owned());
    }

    ctx.set_fonts(fonts);
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
