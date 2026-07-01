use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, TextStyle, Vec2};
use lucide_icons::Icon;
use rust_i18n::t;

use super::super::LUCIDE_FONT_FAMILY;
pub(super) struct TreeRowResponse {
    pub(super) row: egui::Response,
    visibility: Option<egui::Response>,
    disclosure: Option<egui::Response>,
}

impl TreeRowResponse {
    pub(super) fn visibility_clicked(&self) -> bool {
        self.visibility
            .as_ref()
            .is_some_and(egui::Response::clicked)
    }

    pub(super) fn disclosure_clicked(&self) -> bool {
        self.disclosure
            .as_ref()
            .is_some_and(egui::Response::clicked)
    }
}

pub(super) fn tree_row(
    ui: &mut egui::Ui,
    depth: usize,
    selected: bool,
    text: &str,
    visible: Option<bool>,
    expanded: Option<bool>,
    tooltip: Option<String>,
) -> TreeRowResponse {
    let font_id = TextStyle::Body.resolve(ui.style());
    let font_size = font_id.size;
    let row_height = (font_size + 11.0).max(24.0);
    let (rect, row) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), row_height), Sense::click());
    let fill = if selected {
        Color32::from_rgb(53, 115, 220)
    } else if row.hovered() {
        Color32::from_rgb(224, 232, 241)
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 0.0, fill);
    }

    let text_color = if selected {
        Color32::WHITE
    } else {
        Color32::from_rgb(25, 30, 36)
    };
    let icon_color = if selected {
        Color32::from_rgb(241, 246, 252)
    } else {
        Color32::from_rgb(70, 82, 96)
    };

    let left = rect.left() + 6.0 + depth as f32 * 22.0;
    let center_y = rect.center().y;
    let disclosure_rect =
        Rect::from_min_size(Pos2::new(left, rect.top() + 4.0), Vec2::new(16.0, 16.0));
    let disclosure = expanded.map(|is_expanded| {
        let response = ui.interact(
            disclosure_rect,
            row.id.with(("disclosure", depth, text)),
            Sense::click(),
        );
        paint_disclosure(ui, disclosure_rect, is_expanded, icon_color);
        let tooltip = if is_expanded {
            t!("tooltip.collapse").to_string()
        } else {
            t!("tooltip.expand").to_string()
        };
        response.on_hover_text(tooltip)
    });

    let label_left = if expanded.is_some() {
        disclosure_rect.right() + 4.0
    } else {
        disclosure_rect.left() + 16.0
    };
    let eye_rect = visible.map(|_| {
        Rect::from_center_size(
            Pos2::new(rect.right() - 18.0, center_y),
            Vec2::new(22.0, 20.0),
        )
    });
    let label_right = eye_rect
        .map(|rect| rect.left() - 8.0)
        .unwrap_or(rect.right() - 6.0);
    let label_rect = Rect::from_min_max(
        Pos2::new(label_left, rect.top()),
        Pos2::new(label_right.max(label_left), rect.bottom()),
    );
    let label = elide_text(text, label_rect.width(), font_size);
    ui.painter().with_clip_rect(label_rect).text(
        Pos2::new(label_left, center_y),
        egui::Align2::LEFT_CENTER,
        label,
        font_id,
        text_color,
    );

    let visibility = visible.map(|is_visible| {
        let eye_rect = eye_rect.expect("eye rect exists when visible is Some");
        let response = ui.interact(eye_rect, row.id.with(("visibility", text)), Sense::click());
        paint_eye(ui, eye_rect, is_visible, icon_color);
        let tooltip = if is_visible {
            t!("tooltip.hide").to_string()
        } else {
            t!("tooltip.show").to_string()
        };
        response.on_hover_text(tooltip)
    });

    let row = if let Some(tooltip) = tooltip {
        row.on_hover_text(tooltip)
    } else {
        row
    };

    TreeRowResponse {
        row,
        visibility,
        disclosure,
    }
}

fn elide_text(text: &str, max_width: f32, font_size: f32) -> String {
    if max_width <= font_size * 1.8 {
        return "...".to_owned();
    }

    let approx_char_width = font_size * 0.56;
    let max_chars = (max_width / approx_char_width).floor() as usize;
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_owned();
    }
    if max_chars <= 3 {
        return "...".to_owned();
    }

    let keep = max_chars - 3;
    let mut value = text.chars().take(keep).collect::<String>();
    value.push_str("...");
    value
}

fn paint_disclosure(ui: &egui::Ui, rect: Rect, expanded: bool, color: Color32) {
    let icon = if expanded {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    paint_lucide_icon(ui, rect, icon, 15.0, color);
}

fn paint_eye(ui: &egui::Ui, rect: Rect, visible: bool, color: Color32) {
    let icon = if visible { Icon::Eye } else { Icon::EyeOff };
    paint_lucide_icon(ui, rect, icon, 16.0, color);
}

pub(super) fn paint_lucide_icon(ui: &egui::Ui, rect: Rect, icon: Icon, size: f32, color: Color32) {
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        char::from(icon).to_string(),
        FontId::new(size, egui::FontFamily::Name(LUCIDE_FONT_FAMILY.into())),
        color,
    );
}
