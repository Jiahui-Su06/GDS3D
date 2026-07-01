use eframe::egui::{self, RichText, Sense, TextStyle};
use lucide_icons::Icon;
use rust_i18n::t;

use crate::model;

use super::super::{
    PROPERTY_LABEL_WIDTH, PROPERTY_NUMBER_WIDTH, PROPERTY_RESET_WIDTH, PROPERTY_ROW_HEIGHT_MIN,
    PROPERTY_VALUE_MIN_WIDTH, PropertyEditState,
};
use super::tree::paint_lucide_icon;
pub(super) fn editable_basic(
    ui: &mut egui::Ui,
    display: &mut model::DisplayProperties,
    edit: &mut PropertyEditState,
) {
    section_title(ui, t!("property.basic").as_ref());
    edit_text_row(
        ui,
        t!("property.name").as_ref(),
        &mut edit.name,
        &mut display.name,
        &display.defaults.name,
    );
}

pub(super) fn editable_display(
    ui: &mut egui::Ui,
    display: &mut model::DisplayProperties,
    edit: &mut PropertyEditState,
) {
    ui.separator();
    section_title(ui, t!("property.display").as_ref());
    edit_color_row(
        ui,
        t!("property.color").as_ref(),
        &mut edit.color,
        &mut display.color,
        &display.defaults.color,
    );
    edit_float_slider(
        ui,
        t!("property.brightness").as_ref(),
        &mut edit.brightness,
        &mut display.brightness,
        0.0..=2.0,
        0.05,
        display.defaults.brightness,
    );
}

pub(super) fn readonly_row(ui: &mut egui::Ui, label: &str, value: &str) {
    let value_width = property_value_width(ui);
    ui.horizontal(|ui| {
        property_label(ui, RichText::new(label));
        readonly_field(ui, value, value_width);
    });
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(
        RichText::new(text)
            .strong()
            .color(egui::Color32::from_rgb(28, 34, 40)),
    );
}

fn edit_text_row(
    ui: &mut egui::Ui,
    label: &str,
    edit_value: &mut String,
    target: &mut String,
    default_value: &str,
) {
    let value_width = property_value_width(ui);
    let row_height = property_row_height(ui);
    ui.horizontal(|ui| {
        property_label(ui, RichText::new(label));
        let field_width = value_width - reset_button_width(ui);
        let response = ui.add_sized(
            [field_width, row_height],
            egui::TextEdit::singleline(edit_value),
        );
        let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));
        if response.lost_focus() || (response.has_focus() && enter_pressed) {
            let value = edit_value.trim();
            if !value.is_empty() {
                *target = value.to_owned();
                *edit_value = target.clone();
            }
        }
        if reset_button(ui, target != default_value).clicked() {
            *target = default_value.to_owned();
            *edit_value = target.clone();
        }
    });
}

fn edit_color_row(
    ui: &mut egui::Ui,
    label: &str,
    edit_value: &mut String,
    target: &mut String,
    default_value: &str,
) {
    let value_width = property_value_width(ui);
    ui.horizontal(|ui| {
        property_label(ui, RichText::new(label));
        let field_width = value_width - reset_button_width(ui);

        let mut rgb = parse_hex_rgb(target)
            .or_else(|| parse_hex_rgb(edit_value))
            .unwrap_or([45, 108, 223]);
        let response = ui.color_edit_button_srgb(&mut rgb);
        if response.changed() {
            *target = format_hex_rgb(rgb);
            *edit_value = target.clone();
        }

        let spacing = ui.spacing().item_spacing.x;
        let hex_width = (field_width - response.rect.width() - spacing).max(48.0);
        readonly_field(ui, target, hex_width);
        if reset_button(ui, target != default_value).clicked() {
            *target = default_value.to_owned();
            *edit_value = target.clone();
        }
    });
}

fn edit_float_slider(
    ui: &mut egui::Ui,
    label: &str,
    edit_value: &mut f32,
    target: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f64,
    default_value: f32,
) {
    let min = *range.start();
    let max = *range.end();
    let value_width = property_value_width(ui);
    ui.horizontal(|ui| {
        property_label(ui, RichText::new(label));
        let response = slider_field(
            ui,
            edit_value,
            target,
            min,
            max,
            step,
            value_width - reset_button_width(ui),
        );
        if response.number_changed {
            *edit_value = *target;
        } else if response.slider_changed {
            *target = *edit_value;
        } else if !response.slider_dragged {
            *edit_value = *target;
        }
        if reset_button(ui, (*target - default_value).abs() > f32::EPSILON).clicked() {
            *target = default_value;
            *edit_value = default_value;
        }
    });
}

pub(super) fn edit_float_row(
    ui: &mut egui::Ui,
    label: &str,
    edit_value: &mut f32,
    target: &mut f32,
    options: FloatRowOptions,
) {
    let value_width = property_value_width(ui);
    let row_height = property_row_height(ui);
    ui.horizontal(|ui| {
        property_label(ui, RichText::new(label));
        let field_width = if options.default_value.is_some() {
            value_width - reset_button_width(ui)
        } else {
            value_width
        };
        let mut drag_value = egui::DragValue::new(edit_value)
            .speed(options.step)
            .range(*options.range.start()..=*options.range.end());
        if let Some(decimals) = options.decimals {
            drag_value = drag_value.max_decimals(decimals);
        }
        let response = ui.add_sized([field_width, row_height], drag_value);
        if response.changed() {
            *target = *edit_value;
        }
        if let Some(default_value) = options.default_value
            && reset_button(ui, (*target - default_value).abs() > f32::EPSILON).clicked()
        {
            *target = default_value;
            *edit_value = default_value;
        }
    });
    *target = target.clamp(*options.range.start(), *options.range.end());
    *edit_value = (*edit_value).clamp(*options.range.start(), *options.range.end());
}

pub(super) struct FloatRowOptions {
    range: std::ops::RangeInclusive<f32>,
    step: f64,
    decimals: Option<usize>,
    default_value: Option<f32>,
}

impl FloatRowOptions {
    pub(super) fn new(range: std::ops::RangeInclusive<f32>, step: f64) -> Self {
        Self {
            range,
            step,
            decimals: None,
            default_value: None,
        }
    }

    pub(super) fn with_decimals(mut self, decimals: usize) -> Self {
        self.decimals = Some(decimals);
        self
    }

    pub(super) fn with_default(mut self, default_value: Option<f32>) -> Self {
        self.default_value = default_value;
        self
    }
}

fn property_label(ui: &mut egui::Ui, text: RichText) {
    ui.allocate_ui_with_layout(
        egui::vec2(PROPERTY_LABEL_WIDTH, property_row_height(ui)),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.label(text);
        },
    );
}

fn property_value_width(ui: &egui::Ui) -> f32 {
    let spacing = ui.spacing().item_spacing.x;
    (ui.available_width() - PROPERTY_LABEL_WIDTH - spacing).max(PROPERTY_VALUE_MIN_WIDTH)
}

fn reset_button_width(ui: &egui::Ui) -> f32 {
    PROPERTY_RESET_WIDTH + ui.spacing().item_spacing.x
}

fn property_row_height(ui: &egui::Ui) -> f32 {
    (TextStyle::Body.resolve(ui.style()).size + 8.0).max(PROPERTY_ROW_HEIGHT_MIN)
}

fn reset_button(ui: &mut egui::Ui, enabled: bool) -> egui::Response {
    let row_height = property_row_height(ui);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(PROPERTY_RESET_WIDTH, row_height),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let response = response.on_hover_text(t!("property.reset").to_string());
    let color = if enabled {
        ui.visuals().widgets.inactive.fg_stroke.color
    } else {
        ui.visuals().widgets.noninteractive.fg_stroke.color
    };
    if enabled && (response.hovered() || response.clicked()) {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(2),
            ui.visuals().widgets.hovered.bg_fill,
        );
    }
    paint_lucide_icon(ui, rect, Icon::RotateCcw, 13.0, color);
    response
}

fn parse_hex_rgb(value: &str) -> Option<[u8; 3]> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }

    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some([
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
    ])
}

fn format_hex_rgb(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

fn readonly_field(ui: &mut egui::Ui, value: &str, width: f32) {
    let row_height = property_row_height(ui);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, row_height), Sense::hover());
    let fill = ui.visuals().widgets.noninteractive.bg_fill;
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(2), fill);
    ui.painter().text(
        rect.left_center() + egui::vec2(6.0, 0.0),
        egui::Align2::LEFT_CENTER,
        value,
        TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    response.on_hover_text(value);
}

struct SliderFieldResponse {
    slider_changed: bool,
    slider_dragged: bool,
    number_changed: bool,
}

fn slider_field(
    ui: &mut egui::Ui,
    edit_value: &mut f32,
    target: &mut f32,
    min: f32,
    max: f32,
    step: f64,
    width: f32,
) -> SliderFieldResponse {
    let spacing = ui.spacing().item_spacing.x;
    let slider_width = (width - PROPERTY_NUMBER_WIDTH - spacing).max(48.0);
    let row_height = property_row_height(ui);
    let slider_response = ui.add_sized(
        [slider_width, row_height],
        egui::Slider::new(edit_value, min..=max)
            .show_value(false)
            .step_by(step),
    );
    let number_response = ui.add_sized(
        [PROPERTY_NUMBER_WIDTH, row_height],
        egui::DragValue::new(target).speed(step).range(min..=max),
    );
    SliderFieldResponse {
        slider_changed: slider_response.changed(),
        slider_dragged: slider_response.dragged(),
        number_changed: number_response.changed(),
    }
}

pub(super) fn readonly_bounds(
    ui: &mut egui::Ui,
    bounds: &model::Bounds2d,
    display: &mut model::DisplayProperties,
    edit: &mut PropertyEditState,
) {
    ui.separator();
    section_title(ui, t!("property.bounds").as_ref());
    readonly_row(
        ui,
        t!("property.x_min").as_ref(),
        &format!("{:.4}", bounds.min_x),
    );
    readonly_row(
        ui,
        t!("property.x_max").as_ref(),
        &format!("{:.4}", bounds.max_x),
    );
    readonly_row(
        ui,
        t!("property.y_min").as_ref(),
        &format!("{:.4}", bounds.min_y),
    );
    readonly_row(
        ui,
        t!("property.y_max").as_ref(),
        &format!("{:.4}", bounds.max_y),
    );
    edit_float_row(
        ui,
        t!("property.z_min").as_ref(),
        &mut edit.z_min,
        &mut display.z_min,
        FloatRowOptions::new(-1_000_000.0..=1_000_000.0, 0.1)
            .with_decimals(1)
            .with_default(Some(display.defaults.z_min)),
    );
    edit_float_row(
        ui,
        t!("property.z_max").as_ref(),
        &mut edit.z_max,
        &mut display.z_max,
        FloatRowOptions::new(-1_000_000.0..=1_000_000.0, 0.1)
            .with_decimals(1)
            .with_default(Some(display.defaults.z_max)),
    );
}
