use std::collections::HashSet;
use std::fs;
use std::path::Path;

use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Sense, TextStyle, Vec2};
use lucide_icons::Icon;

use crate::export::{ExportFormat, ExportQuality, ExportSettings, ExportSizePreset};
use crate::model::{self, CellKey, Scene, SceneObject, Selection};
use crate::viewport::{self, ViewportState};

const UNDO_STACK_MAX: usize = 100;
const LUCIDE_FONT_FAMILY: &str = "lucide";

pub struct Gds3dApp {
    scene: Scene,
    selection: Selection,
    collapsed_cells: HashSet<CellKey>,
    viewport: ViewportState,
    undo_stack: Vec<UndoCommand>,
    status: String,
    export_settings: ExportSettings,
    show_export_dialog: bool,
    left_panel_min_width: f32,
    right_panel_min_width: f32,
    locale: Locale,
}

#[derive(Clone)]
enum UndoCommand {
    AddObjects(Vec<String>),
    DeleteObjects(Vec<SceneObject>),
    ReplaceObject(SceneObject),
    SetGroupVisibility(Vec<(String, bool)>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Locale {
    English,
    SimplifiedChinese,
}

impl Locale {
    fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::SimplifiedChinese => "Simplified Chinese",
        }
    }
}

impl Gds3dApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_fonts(&cc.egui_ctx);
        configure_industrial_style(&cc.egui_ctx);
        Self {
            scene: Scene::default(),
            selection: Selection::Scene,
            collapsed_cells: HashSet::new(),
            viewport: ViewportState::default(),
            undo_stack: Vec::new(),
            status: "Ready".to_owned(),
            export_settings: ExportSettings::default(),
            show_export_dialog: false,
            left_panel_min_width: 240.0,
            right_panel_min_width: 300.0,
            locale: Locale::English,
        }
    }

    fn import_gds(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("GDS", &["gds"])
            .pick_file()
        else {
            return;
        };

        let obj = model::placeholder_gds_layer(path.clone());
        let object_id = obj.id().to_owned();
        if let Err(err) = self.scene.add(obj) {
            self.status = format!("Import failed: {err}");
            return;
        }

        self.selection = Selection::Object(object_id.clone());
        self.push_undo(UndoCommand::AddObjects(vec![object_id]));
        self.viewport.reset_camera();
        self.status = format!(
            "Registered {} as a migration placeholder; GDS parsing is the next Rust milestone",
            file_name(&path)
        );
    }

    fn open_project(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("GDS3D scene json", &["json"])
            .pick_file()
        else {
            return;
        };

        match read_scene_json(&path) {
            Ok(scene) => {
                self.scene = scene;
                self.selection = Selection::Scene;
                self.undo_stack.clear();
                self.viewport.reset_camera();
                self.status = format!("Opened {}", file_name(&path));
            }
            Err(err) => {
                self.status = format!("Open failed: {err}");
            }
        }
    }

    fn export_project(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("project.scene.json")
            .save_file()
        else {
            return;
        };

        match write_scene_json(&path, &self.scene) {
            Ok(()) => {
                self.status = format!("Exported {}", file_name(&path));
            }
            Err(err) => {
                self.status = format!("Export failed: {err}");
            }
        }
    }

    fn create_baseplate(&mut self) {
        let bounds = self.scene.default_baseplate_bounds();
        let obj = model::new_baseplate(self.scene.next_baseplate_name(), bounds);
        let object_id = obj.id().to_owned();
        if let Err(err) = self.scene.add(obj) {
            self.status = format!("Create baseplate failed: {err}");
            return;
        }

        self.selection = Selection::Object(object_id.clone());
        self.push_undo(UndoCommand::AddObjects(vec![object_id]));
        self.status = "Created baseplate".to_owned();
    }

    fn delete_selection(&mut self) {
        match self.selection.clone() {
            Selection::Scene => {}
            Selection::Object(object_id) => {
                if let Some(obj) = self.scene.remove(&object_id) {
                    self.selection = Selection::Scene;
                    self.push_undo(UndoCommand::DeleteObjects(vec![obj]));
                    self.status = "Deleted object".to_owned();
                }
            }
            Selection::Cell(key) => {
                let object_ids = self.object_ids_for_cell(&key);
                let mut removed = Vec::new();
                for object_id in object_ids {
                    if let Some(obj) = self.scene.remove(&object_id) {
                        removed.push(obj);
                    }
                }
                if !removed.is_empty() {
                    self.selection = Selection::Scene;
                    self.push_undo(UndoCommand::DeleteObjects(removed));
                    self.status = "Deleted cell".to_owned();
                }
            }
        }
    }

    fn undo(&mut self) {
        let Some(command) = self.undo_stack.pop() else {
            return;
        };

        match command {
            UndoCommand::AddObjects(object_ids) => {
                for object_id in object_ids {
                    self.scene.remove(&object_id);
                }
                self.selection = Selection::Scene;
            }
            UndoCommand::DeleteObjects(objects) => {
                for obj in objects {
                    let _ = self.scene.add(obj);
                }
            }
            UndoCommand::ReplaceObject(previous) => {
                let id = previous.id().to_owned();
                if let Some(current) = self.scene.get_mut(&id) {
                    *current = previous;
                    self.selection = Selection::Object(id);
                }
            }
            UndoCommand::SetGroupVisibility(states) => {
                for (object_id, visible) in states {
                    if let Some(obj) = self.scene.get_mut(&object_id) {
                        obj.set_visible(visible);
                    }
                }
            }
        }

        self.status = "Undid last action".to_owned();
    }

    fn push_undo(&mut self, command: UndoCommand) {
        self.undo_stack.push(command);
        if self.undo_stack.len() > UNDO_STACK_MAX {
            self.undo_stack.remove(0);
        }
    }

    fn object_ids_for_cell(&self, key: &CellKey) -> Vec<String> {
        self.scene
            .cell_groups()
            .into_iter()
            .find(|group| &group.key == key)
            .map(|group| group.object_ids)
            .unwrap_or_default()
    }

    fn set_group_visibility(&mut self, key: &CellKey, visible: bool) {
        let mut previous = Vec::new();
        for object_id in self.object_ids_for_cell(key) {
            if let Some(obj) = self.scene.get_mut(&object_id) {
                previous.push((object_id, obj.is_visible()));
                obj.set_visible(visible);
            }
        }
        if !previous.is_empty() {
            self.push_undo(UndoCommand::SetGroupVisibility(previous));
            self.status = "Changed cell visibility".to_owned();
        }
    }

    fn replace_object_after_edit(&mut self, before: SceneObject) {
        let Some(after) = self.scene.get(before.id()) else {
            return;
        };
        if after != &before {
            self.push_undo(UndoCommand::ReplaceObject(before));
        }
    }
}

impl eframe::App for Gds3dApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show_menu(ui);
        self.show_status_bar(ui);
        self.show_left_panel(ui);
        self.show_right_panel(ui);

        egui::CentralPanel::default_margins().show(ui, |ui| {
            viewport::show_viewport(ui, &self.scene, &self.selection, &mut self.viewport);
        });

        if self.show_export_dialog {
            self.show_export_window(ui.ctx());
        }
    }
}

impl Gds3dApp {
    fn show_menu(&mut self, parent_ui: &mut egui::Ui) {
        egui::Panel::top("menu_bar").show(parent_ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open Project").clicked() {
                        ui.close();
                        self.open_project();
                    }
                    if ui.button("Import GDS").clicked() {
                        ui.close();
                        self.import_gds();
                    }
                    ui.separator();
                    if ui.button("Export Project").clicked() {
                        ui.close();
                        self.export_project();
                    }
                    if ui.button("Export As").clicked() {
                        ui.close();
                        self.show_export_dialog = true;
                    }
                });

                ui.menu_button("Edit", |ui| {
                    if ui
                        .add_enabled(!self.undo_stack.is_empty(), egui::Button::new("Undo"))
                        .clicked()
                    {
                        ui.close();
                        self.undo();
                    }
                    if ui.button("Create Baseplate").clicked() {
                        ui.close();
                        self.create_baseplate();
                    }
                    if ui.button("Delete").clicked() {
                        ui.close();
                        self.delete_selection();
                    }
                    ui.separator();
                    if ui.button("Reset Camera").clicked() {
                        ui.close();
                        self.viewport.reset_camera();
                    }
                });

                ui.menu_button("Settings", |ui| {
                    ui.menu_button("Language", |ui| {
                        for locale in [Locale::English, Locale::SimplifiedChinese] {
                            ui.radio_value(&mut self.locale, locale, locale.label());
                        }
                    });
                    ui.separator();
                    ui.checkbox(&mut self.viewport.show_axes, "Show axes");
                    ui.add(
                        egui::Slider::new(&mut self.left_panel_min_width, 160.0..=420.0)
                            .text("Left panel"),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.right_panel_min_width, 220.0..=520.0)
                            .text("Right panel"),
                    );
                });
            });
        });
    }

    fn show_status_bar(&mut self, parent_ui: &mut egui::Ui) {
        egui::Panel::bottom("status_bar").show(parent_ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("Objects: {}", self.scene.object_count()));
                });
            });
        });
    }

    fn show_left_panel(&mut self, parent_ui: &mut egui::Ui) {
        egui::Panel::left("component_tree")
            .resizable(true)
            .default_size(self.left_panel_min_width)
            .size_range(160.0..=520.0)
            .show(parent_ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                ui.label(RichText::new("Scene").strong());
                ui.separator();

                let groups = self.scene.cell_groups();
                for group in groups {
                    let selected =
                        matches!(self.selection, Selection::Cell(ref key) if key == &group.key);
                    let expanded = !self.collapsed_cells.contains(&group.key);
                    let any_visible = group
                        .object_ids
                        .iter()
                        .any(|id| self.scene.get(id).is_some_and(SceneObject::is_visible));
                    let row = tree_row(
                        ui,
                        0,
                        selected,
                        &group.key.cell_name,
                        Some(any_visible),
                        Some(expanded),
                        Some(group.key.file_path.display().to_string()),
                    );
                    if row.disclosure_clicked() {
                        if expanded {
                            self.collapsed_cells.insert(group.key.clone());
                        } else {
                            self.collapsed_cells.remove(&group.key);
                        }
                    } else if row.visibility_clicked() {
                        self.set_group_visibility(&group.key, !any_visible);
                    } else if row.row.clicked() {
                        self.selection = Selection::Cell(group.key.clone());
                    }

                    if expanded {
                        for object_id in group.object_ids {
                            self.show_object_tree_row(ui, &object_id, 1);
                        }
                    }
                }

                let baseplate_ids: Vec<String> = self
                    .scene
                    .objects()
                    .filter(|obj| matches!(obj, SceneObject::Baseplate(_)))
                    .map(|obj| obj.id().to_owned())
                    .collect();
                for object_id in baseplate_ids {
                    self.show_object_tree_row(ui, &object_id, 0);
                }
            });
    }

    fn show_object_tree_row(&mut self, ui: &mut egui::Ui, object_id: &str, depth: usize) {
        let Some(obj) = self.scene.get(object_id) else {
            return;
        };
        let name = obj.display().name.clone();
        let visible = obj.is_visible();
        let selected = matches!(self.selection, Selection::Object(ref selected_id) if selected_id == object_id);

        let row = tree_row(ui, depth, selected, &name, Some(visible), None, None);
        if row.visibility_clicked() {
            let before = self.scene.get(object_id).cloned();
            if let Some(obj) = self.scene.get_mut(object_id) {
                obj.set_visible(!visible);
            }
            if let Some(before) = before {
                self.replace_object_after_edit(before);
            }
        } else if row.row.clicked() {
            self.selection = Selection::Object(object_id.to_owned());
        }
    }

    fn show_right_panel(&mut self, parent_ui: &mut egui::Ui) {
        egui::Panel::right("property_panel")
            .resizable(true)
            .default_size(self.right_panel_min_width)
            .size_range(220.0..=560.0)
            .show(parent_ui, |ui| {
                ui.heading("Properties");
                ui.separator();
                match self.selection.clone() {
                    Selection::Scene => self.show_scene_properties(ui),
                    Selection::Cell(key) => self.show_cell_properties(ui, &key),
                    Selection::Object(object_id) => self.show_object_properties(ui, &object_id),
                }
            });
    }

    fn show_scene_properties(&self, ui: &mut egui::Ui) {
        readonly_row(ui, "Selection", "Scene");
        readonly_row(ui, "Objects", &self.scene.object_count().to_string());
    }

    fn show_cell_properties(&self, ui: &mut egui::Ui, key: &CellKey) {
        let object_ids = self.object_ids_for_cell(key);
        readonly_row(ui, "Selection", "Cell");
        readonly_row(ui, "Cell", &key.cell_name);
        readonly_row(ui, "File", &key.file_path.display().to_string());
        readonly_row(ui, "Layers", &object_ids.len().to_string());
    }

    fn show_object_properties(&mut self, ui: &mut egui::Ui, object_id: &str) {
        let Some(before) = self.scene.get(object_id).cloned() else {
            ui.label("No component selected");
            return;
        };

        let Some(obj) = self.scene.get_mut(object_id) else {
            return;
        };

        match obj {
            SceneObject::GdsLayer(layer) => {
                editable_display(ui, &mut layer.display);
                readonly_row(ui, "Layer", &layer.layer.to_string());
                readonly_row(ui, "Datatype", &layer.datatype.to_string());
                readonly_row(ui, "File", &layer.file_path.display().to_string());
                readonly_row(ui, "Cell", &layer.cell_name);
                readonly_bounds(ui, &layer.bounds);
            }
            SceneObject::Baseplate(baseplate) => {
                editable_display(ui, &mut baseplate.display);
                ui.separator();
                ui.label(RichText::new("Bounds").strong());
                ui.add(egui::DragValue::new(&mut baseplate.bounds.min_x).prefix("X min "));
                ui.add(egui::DragValue::new(&mut baseplate.bounds.max_x).prefix("X max "));
                ui.add(egui::DragValue::new(&mut baseplate.bounds.min_y).prefix("Y min "));
                ui.add(egui::DragValue::new(&mut baseplate.bounds.max_y).prefix("Y max "));
            }
        }

        self.replace_object_after_edit(before);
    }

    fn show_export_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_export_dialog;
        let mut should_close = false;
        egui::Window::new("Export As")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                egui::ComboBox::from_label("Format")
                    .selected_text(self.export_settings.format.label())
                    .show_ui(ui, |ui| {
                        for format in ExportFormat::ALL {
                            ui.selectable_value(
                                &mut self.export_settings.format,
                                format,
                                format.label(),
                            );
                        }
                    });

                if self.export_settings.format.needs_image_size() {
                    egui::ComboBox::from_label("Size")
                        .selected_text(self.export_settings.size_preset.label())
                        .show_ui(ui, |ui| {
                            for preset in ExportSizePreset::ALL {
                                ui.selectable_value(
                                    &mut self.export_settings.size_preset,
                                    preset,
                                    preset.label(),
                                );
                            }
                        });
                    egui::ComboBox::from_label("Quality")
                        .selected_text(self.export_settings.quality.label())
                        .show_ui(ui, |ui| {
                            for quality in ExportQuality::ALL {
                                ui.selectable_value(
                                    &mut self.export_settings.quality,
                                    quality,
                                    quality.label(),
                                );
                            }
                        });
                    if let Some((width, height)) = self.export_settings.image_size() {
                        ui.label(format!("{width} x {height} px"));
                    }
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Export").clicked() {
                        self.status = format!(
                            "{} export is mapped; renderer-backed output comes next",
                            self.export_settings.format.label()
                        );
                        should_close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        should_close = true;
                    }
                });
            });
        self.show_export_dialog = open && !should_close;
    }
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "noto_sans".to_owned(),
        egui::FontData::from_static(include_bytes!("../../fonts/NotoSans-Regular.ttf")).into(),
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
            .entry(family)
            .or_default()
            .insert(0, "noto_sans".to_owned());
    }

    ctx.set_fonts(fonts);
}

fn configure_industrial_style(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Light)).clone();
    style.spacing.item_spacing = egui::vec2(6.0, 3.0);
    style.spacing.button_padding = egui::vec2(6.0, 2.0);
    style.spacing.indent = 14.0;
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(16.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(13.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(13.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0, egui::FontFamily::Proportional),
    );
    style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(242, 245, 248);
    style.visuals.panel_fill = egui::Color32::from_rgb(238, 242, 246);
    style.visuals.window_fill = egui::Color32::from_rgb(246, 248, 250);
    style.visuals.extreme_bg_color = egui::Color32::from_rgb(224, 229, 235);
    style.visuals.indent_has_left_vline = false;
    ctx.set_style_of(egui::Theme::Light, style.clone());
    ctx.set_style_of(egui::Theme::Dark, style);
}

struct TreeRowResponse {
    row: egui::Response,
    visibility: Option<egui::Response>,
    disclosure: Option<egui::Response>,
}

impl TreeRowResponse {
    fn visibility_clicked(&self) -> bool {
        self.visibility
            .as_ref()
            .is_some_and(egui::Response::clicked)
    }

    fn disclosure_clicked(&self) -> bool {
        self.disclosure
            .as_ref()
            .is_some_and(egui::Response::clicked)
    }
}

fn tree_row(
    ui: &mut egui::Ui,
    depth: usize,
    selected: bool,
    text: &str,
    visible: Option<bool>,
    expanded: Option<bool>,
    tooltip: Option<String>,
) -> TreeRowResponse {
    let row_height = 24.0;
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
        response.on_hover_text(if is_expanded { "Collapse" } else { "Expand" })
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
    let label = elide_text(text, label_rect.width(), 13.0);
    ui.painter().with_clip_rect(label_rect).text(
        Pos2::new(label_left, center_y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::new(13.0, egui::FontFamily::Proportional),
        text_color,
    );

    let visibility = visible.map(|is_visible| {
        let eye_rect = eye_rect.expect("eye rect exists when visible is Some");
        let response = ui.interact(eye_rect, row.id.with(("visibility", text)), Sense::click());
        paint_eye(ui, eye_rect, is_visible, icon_color);
        response.on_hover_text(if is_visible { "Hide" } else { "Show" })
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

fn paint_lucide_icon(ui: &egui::Ui, rect: Rect, icon: Icon, size: f32, color: Color32) {
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        char::from(icon).to_string(),
        FontId::new(size, egui::FontFamily::Name(LUCIDE_FONT_FAMILY.into())),
        color,
    );
}

fn editable_display(ui: &mut egui::Ui, display: &mut model::DisplayProperties) {
    ui.label(RichText::new("Display").strong());
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.text_edit_singleline(&mut display.name);
    });
    ui.checkbox(&mut display.visible, "Visible");
    ui.horizontal(|ui| {
        ui.label("Color");
        ui.text_edit_singleline(&mut display.color);
    });
    ui.add(egui::Slider::new(&mut display.brightness, 0.0..=2.0).text("Brightness"));
    ui.add(egui::Slider::new(&mut display.opacity, 0.0..=1.0).text("Opacity"));
    ui.add(egui::DragValue::new(&mut display.z_min).prefix("Z min "));
    ui.add(egui::DragValue::new(&mut display.z_max).prefix("Z max "));
}

fn readonly_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).strong());
        ui.label(value);
    });
}

fn readonly_bounds(ui: &mut egui::Ui, bounds: &model::Bounds2d) {
    ui.separator();
    ui.label(RichText::new("Bounds").strong());
    readonly_row(ui, "X min", &format!("{:.4}", bounds.min_x));
    readonly_row(ui, "X max", &format!("{:.4}", bounds.max_x));
    readonly_row(ui, "Y min", &format!("{:.4}", bounds.min_y));
    readonly_row(ui, "Y max", &format!("{:.4}", bounds.max_y));
}

fn read_scene_json(path: &Path) -> anyhow::Result<Scene> {
    let data = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&data)?)
}

fn write_scene_json(path: &Path, scene: &Scene) -> anyhow::Result<()> {
    let data = serde_json::to_string_pretty(scene)?;
    fs::write(path, data)?;
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<unnamed>")
        .to_owned()
}
