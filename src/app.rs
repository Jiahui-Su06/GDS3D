use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Sense, TextStyle, Vec2};
use gds3d_viewport::{
    self as viewport, Bounds2d as ViewportBounds2d, Polygon2d as ViewportPolygon2d, ViewportObject,
    ViewportScene, ViewportState,
};
use lucide_icons::Icon;
use rust_i18n::t;

use crate::archive::{self, ArchiveObject};
use crate::export::{ExportFormat, ExportQuality, ExportSettings, ExportSizePreset};
use crate::model::{
    self, BaseplateObject, Bounds2d, CellKey, DisplayProperties, Scene, SceneObject, Selection,
};

const UNDO_STACK_MAX: usize = 100;
const LUCIDE_FONT_FAMILY: &str = "lucide";

pub struct Gds3dApp {
    scene: Scene,
    selection: Selection,
    collapsed_cells: HashSet<CellKey>,
    viewport: ViewportState,
    viewport_scene_cache: ViewportSceneCache,
    undo_stack: Vec<UndoCommand>,
    status: String,
    export_settings: ExportSettings,
    show_export_dialog: bool,
    left_panel_min_width: f32,
    right_panel_min_width: f32,
    locale: Locale,
    archive_temp_dir: Option<PathBuf>,
}

#[derive(Clone)]
enum UndoCommand {
    AddObjects(Vec<String>),
    DeleteObjects(Vec<SceneObject>),
    ReplaceObject(Box<SceneObject>),
    SetGroupVisibility(Vec<(String, bool)>),
}

#[derive(Default)]
struct ViewportSceneCache {
    revision: Option<u64>,
    objects: Vec<ViewportObject>,
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
            Self::SimplifiedChinese => "简体中文",
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::SimplifiedChinese => "zh-CN",
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
            viewport: ViewportState::new(cc.wgpu_render_state.as_ref()),
            viewport_scene_cache: ViewportSceneCache::default(),
            undo_stack: Vec::new(),
            status: t!("status.ready").to_string(),
            export_settings: ExportSettings::default(),
            show_export_dialog: false,
            left_panel_min_width: 240.0,
            right_panel_min_width: 300.0,
            locale: Locale::English,
            archive_temp_dir: None,
        }
    }

    fn import_gds(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("GDS", &["gds"])
            .pick_file()
        else {
            return;
        };

        let objects = match model::import_gds_layers(&path) {
            Ok(objects) => objects,
            Err(err) => {
                self.status = t!("status.import_failed", error = err).to_string();
                return;
            }
        };

        let mut object_ids = Vec::new();
        for obj in objects {
            let object_id = obj.id().to_owned();
            if let Err(err) = self.scene.add(obj) {
                self.status = t!("status.import_failed", error = err).to_string();
                return;
            }
            object_ids.push(object_id);
        }
        if let Some(object_id) = object_ids.first() {
            self.selection = Selection::Object(object_id.clone());
        }
        let imported_count = object_ids.len();
        self.push_undo(UndoCommand::AddObjects(object_ids));
        self.viewport.reset_camera();
        self.status = t!(
            "status.imported_gds",
            name = file_name(&path),
            count = imported_count
        )
        .to_string();
    }

    fn open_project(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("GDS3D archive", &["gds3d"])
            .pick_file()
        else {
            return;
        };

        match read_archive_scene(&path) {
            Ok(scene) => {
                self.scene = scene;
                self.viewport_scene_cache = ViewportSceneCache::default();
                self.selection = Selection::Scene;
                self.undo_stack.clear();
                self.viewport.reset_camera();
                self.archive_temp_dir = Some(archive_temp_dir(&path));
                self.status = t!("status.opened", name = file_name(&path)).to_string();
            }
            Err(err) => {
                self.status = t!("status.open_failed", error = err).to_string();
            }
        }
    }

    fn export_project(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("GDS3D archive", &["gds3d"])
            .set_file_name("project.gds3d")
            .save_file()
        else {
            return;
        };

        let path = ensure_suffix(path, "gds3d");
        match write_archive_scene(&path, &self.scene) {
            Ok(()) => {
                self.status = t!("status.exported", name = file_name(&path)).to_string();
            }
            Err(err) => {
                self.status = t!("status.export_failed", error = err).to_string();
            }
        }
    }

    fn create_baseplate(&mut self) {
        let bounds = self.scene.default_baseplate_bounds();
        let obj = model::new_baseplate(self.scene.next_baseplate_name(), bounds);
        let object_id = obj.id().to_owned();
        if let Err(err) = self.scene.add(obj) {
            self.status = t!("status.create_baseplate_failed", error = err).to_string();
            return;
        }

        self.selection = Selection::Object(object_id.clone());
        self.push_undo(UndoCommand::AddObjects(vec![object_id]));
        self.status = t!("status.created_baseplate").to_string();
    }

    fn delete_selection(&mut self) {
        match self.selection.clone() {
            Selection::Scene => {}
            Selection::Object(object_id) => {
                if let Some(obj) = self.scene.remove(&object_id) {
                    self.selection = Selection::Scene;
                    self.push_undo(UndoCommand::DeleteObjects(vec![obj]));
                    self.status = t!("status.deleted_object").to_string();
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
                    self.status = t!("status.deleted_cell").to_string();
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
                    *current = *previous;
                    self.scene.touch();
                    self.selection = Selection::Object(id);
                }
            }
            UndoCommand::SetGroupVisibility(states) => {
                for (object_id, visible) in states {
                    if let Some(obj) = self.scene.get_mut(&object_id) {
                        obj.set_visible(visible);
                        self.scene.touch();
                    }
                }
            }
        }

        self.status = t!("status.undid").to_string();
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
                self.scene.touch();
            }
        }
        if !previous.is_empty() {
            self.push_undo(UndoCommand::SetGroupVisibility(previous));
            self.status = t!("status.changed_cell_visibility").to_string();
        }
    }

    fn replace_object_after_edit(&mut self, before: SceneObject) {
        let Some(after) = self.scene.get(before.id()) else {
            return;
        };
        if after != &before {
            self.push_undo(UndoCommand::ReplaceObject(Box::new(before)));
            self.scene.touch();
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
            let viewport_scene =
                viewport_scene(&self.scene, &self.selection, &mut self.viewport_scene_cache);
            viewport::show_viewport(
                ui,
                &viewport_scene,
                &mut self.viewport,
                t!("viewport.empty").as_ref(),
            );
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
                ui.menu_button(t!("menu.file").as_ref(), |ui| {
                    if ui.button(t!("action.open_project").as_ref()).clicked() {
                        ui.close();
                        self.open_project();
                    }
                    if ui.button(t!("action.import_gds").as_ref()).clicked() {
                        ui.close();
                        self.import_gds();
                    }
                    ui.separator();
                    if ui.button(t!("action.export_project").as_ref()).clicked() {
                        ui.close();
                        self.export_project();
                    }
                    if ui.button(t!("action.export_as").as_ref()).clicked() {
                        ui.close();
                        self.show_export_dialog = true;
                    }
                });

                ui.menu_button(t!("menu.edit").as_ref(), |ui| {
                    if ui
                        .add_enabled(
                            !self.undo_stack.is_empty(),
                            egui::Button::new(t!("action.undo").as_ref()),
                        )
                        .clicked()
                    {
                        ui.close();
                        self.undo();
                    }
                    if ui.button(t!("action.create_baseplate").as_ref()).clicked() {
                        ui.close();
                        self.create_baseplate();
                    }
                    if ui.button(t!("action.delete").as_ref()).clicked() {
                        ui.close();
                        self.delete_selection();
                    }
                    ui.separator();
                    if ui.button(t!("action.reset_camera").as_ref()).clicked() {
                        ui.close();
                        self.viewport.reset_camera();
                    }
                });

                ui.menu_button(t!("menu.settings").as_ref(), |ui| {
                    ui.menu_button(t!("menu.language").as_ref(), |ui| {
                        for locale in [Locale::English, Locale::SimplifiedChinese] {
                            if ui
                                .radio_value(&mut self.locale, locale, locale.label())
                                .clicked()
                            {
                                rust_i18n::set_locale(locale.code());
                                self.status = t!("status.language_changed").to_string();
                            }
                        }
                    });
                    ui.separator();
                    ui.checkbox(
                        &mut self.viewport.show_axes,
                        t!("setting.show_axes").as_ref(),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.left_panel_min_width, 160.0..=420.0)
                            .text(t!("setting.left_panel").as_ref()),
                    );
                    ui.add(
                        egui::Slider::new(&mut self.right_panel_min_width, 220.0..=520.0)
                            .text(t!("setting.right_panel").as_ref()),
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
                    ui.label(t!("status.object_count", count = self.scene.object_count()).as_ref());
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
                ui.label(RichText::new(t!("panel.scene").as_ref()).strong());
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
                self.scene.touch();
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
                ui.heading(t!("panel.properties").as_ref());
                ui.separator();
                match self.selection.clone() {
                    Selection::Scene => self.show_scene_properties(ui),
                    Selection::Cell(key) => self.show_cell_properties(ui, &key),
                    Selection::Object(object_id) => self.show_object_properties(ui, &object_id),
                }
            });
    }

    fn show_scene_properties(&self, ui: &mut egui::Ui) {
        readonly_row(
            ui,
            t!("property.selection").as_ref(),
            t!("property.selection_scene").as_ref(),
        );
        readonly_row(
            ui,
            t!("property.objects").as_ref(),
            &self.scene.object_count().to_string(),
        );
    }

    fn show_cell_properties(&self, ui: &mut egui::Ui, key: &CellKey) {
        let object_ids = self.object_ids_for_cell(key);
        readonly_row(
            ui,
            t!("property.selection").as_ref(),
            t!("property.selection_cell").as_ref(),
        );
        readonly_row(ui, t!("property.cell").as_ref(), &key.cell_name);
        readonly_row(
            ui,
            t!("property.file").as_ref(),
            &key.file_path.display().to_string(),
        );
        readonly_row(
            ui,
            t!("property.layers").as_ref(),
            &object_ids.len().to_string(),
        );
    }

    fn show_object_properties(&mut self, ui: &mut egui::Ui, object_id: &str) {
        let Some(before) = self.scene.get(object_id).cloned() else {
            ui.label(t!("property.no_component_selected").as_ref());
            return;
        };

        let Some(obj) = self.scene.get_mut(object_id) else {
            return;
        };

        match obj {
            SceneObject::GdsLayer(layer) => {
                editable_display(ui, &mut layer.display);
                readonly_row(ui, t!("property.layer").as_ref(), &layer.layer.to_string());
                readonly_row(
                    ui,
                    t!("property.datatype").as_ref(),
                    &layer.datatype.to_string(),
                );
                readonly_row(
                    ui,
                    t!("property.file").as_ref(),
                    &layer.file_path.display().to_string(),
                );
                readonly_row(ui, t!("property.cell").as_ref(), &layer.cell_name);
                readonly_bounds(ui, &layer.bounds);
            }
            SceneObject::Baseplate(baseplate) => {
                editable_display(ui, &mut baseplate.display);
                ui.separator();
                ui.label(RichText::new(t!("property.bounds").as_ref()).strong());
                ui.add(
                    egui::DragValue::new(&mut baseplate.bounds.min_x)
                        .prefix(t!("property.x_min_prefix").as_ref()),
                );
                ui.add(
                    egui::DragValue::new(&mut baseplate.bounds.max_x)
                        .prefix(t!("property.x_max_prefix").as_ref()),
                );
                ui.add(
                    egui::DragValue::new(&mut baseplate.bounds.min_y)
                        .prefix(t!("property.y_min_prefix").as_ref()),
                );
                ui.add(
                    egui::DragValue::new(&mut baseplate.bounds.max_y)
                        .prefix(t!("property.y_max_prefix").as_ref()),
                );
            }
        }

        self.replace_object_after_edit(before);
    }

    fn show_export_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_export_dialog;
        let mut should_close = false;
        egui::Window::new(t!("dialog.export_as").as_ref())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                egui::ComboBox::from_label(t!("export.format").as_ref())
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
                    egui::ComboBox::from_label(t!("export.size").as_ref())
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
                    egui::ComboBox::from_label(t!("export.quality").as_ref())
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
                    if ui.button(t!("action.export").as_ref()).clicked() {
                        self.status = t!(
                            "status.export_mapped",
                            format = self.export_settings.format.label()
                        )
                        .to_string();
                        should_close = true;
                    }
                    if ui.button(t!("action.cancel").as_ref()).clicked() {
                        should_close = true;
                    }
                });
            });
        self.show_export_dialog = open && !should_close;
    }
}

fn viewport_scene(
    scene: &Scene,
    selection: &Selection,
    cache: &mut ViewportSceneCache,
) -> ViewportScene {
    let revision = scene.revision();
    if cache.revision != Some(revision) {
        let previous_objects = cache.objects.clone();
        cache.objects = scene
            .objects()
            .map(|obj| {
                let previous = previous_objects
                    .iter()
                    .find(|previous| previous.id == obj.id());
                viewport_object(obj, previous)
            })
            .collect();
        cache.revision = Some(revision);
    }

    let selected_id = match selection {
        Selection::Object(id) => Some(id.clone()),
        Selection::Scene | Selection::Cell(_) => None,
    };
    ViewportScene {
        revision,
        objects: cache.objects.clone(),
        selected_id,
    }
}

fn viewport_object(obj: &SceneObject, previous: Option<&ViewportObject>) -> ViewportObject {
    let bounds = obj.bounds();
    let display = obj.display();
    let polygons = match obj {
        SceneObject::GdsLayer(layer) => previous
            .filter(|previous| !previous.polygons.is_empty())
            .map(|previous| Arc::clone(&previous.polygons))
            .unwrap_or_else(|| {
                layer
                    .polygons
                    .iter()
                    .map(|polygon| ViewportPolygon2d {
                        points: polygon.points.clone(),
                    })
                    .collect::<Vec<_>>()
                    .into()
            }),
        SceneObject::Baseplate(_) => Arc::from([]),
    };
    ViewportObject {
        id: obj.id().to_owned(),
        bounds: ViewportBounds2d {
            min_x: bounds.min_x,
            min_y: bounds.min_y,
            max_x: bounds.max_x,
            max_y: bounds.max_y,
        },
        visible: obj.is_visible(),
        color: display.color.clone(),
        brightness: display.brightness,
        opacity: display.opacity,
        z_min: display.z_min,
        z_max: display.z_max,
        polygons,
    }
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "noto_sans".to_owned(),
        egui::FontData::from_static(include_bytes!("assets/fonts/NotoSans-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        "noto_sans_cjk".to_owned(),
        egui::FontData::from_static(include_bytes!("assets/fonts/NotoSansCJKsc-Regular.otf"))
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
    ui.label(RichText::new(t!("property.display").as_ref()).strong());
    ui.horizontal(|ui| {
        ui.label(t!("property.name").as_ref());
        ui.text_edit_singleline(&mut display.name);
    });
    ui.checkbox(&mut display.visible, t!("property.visible").as_ref());
    ui.horizontal(|ui| {
        ui.label(t!("property.color").as_ref());
        ui.text_edit_singleline(&mut display.color);
    });
    ui.add(
        egui::Slider::new(&mut display.brightness, 0.0..=2.0)
            .text(t!("property.brightness").as_ref()),
    );
    ui.add(
        egui::Slider::new(&mut display.opacity, 0.0..=1.0).text(t!("property.opacity").as_ref()),
    );
    ui.add(egui::DragValue::new(&mut display.z_min).prefix(t!("property.z_min_prefix").as_ref()));
    ui.add(egui::DragValue::new(&mut display.z_max).prefix(t!("property.z_max_prefix").as_ref()));
}

fn readonly_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).strong());
        ui.label(value);
    });
}

fn readonly_bounds(ui: &mut egui::Ui, bounds: &model::Bounds2d) {
    ui.separator();
    ui.label(RichText::new(t!("property.bounds").as_ref()).strong());
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
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<unnamed>")
        .to_owned()
}

fn read_archive_scene(path: &Path) -> anyhow::Result<Scene> {
    let (objects, gds_sources) = archive::read_archive(path)?;
    let temp_dir = archive_temp_dir(path);
    fs::create_dir_all(&temp_dir)?;
    let source_paths = materialize_gds_sources(&temp_dir, gds_sources)?;

    let mut scene = Scene::default();
    for archive_obj in objects {
        let obj = restore_archive_object(&archive_obj, path, &source_paths)?;
        scene.add(obj)?;
    }
    Ok(scene)
}

fn write_archive_scene(path: &Path, scene: &Scene) -> anyhow::Result<()> {
    let objects: Vec<SceneObject> = scene.objects().cloned().collect();
    archive::write_archive(path, &objects)
}

fn restore_archive_object(
    archive_obj: &ArchiveObject,
    archive_path: &Path,
    source_paths: &HashMap<String, PathBuf>,
) -> anyhow::Result<SceneObject> {
    match archive_obj.kind.as_str() {
        "gds_layer" => restore_gds_layer(archive_obj, archive_path, source_paths),
        "baseplate" => restore_baseplate(archive_obj),
        kind => anyhow::bail!("unsupported archive object kind: {kind}"),
    }
}

fn restore_gds_layer(
    archive_obj: &ArchiveObject,
    archive_path: &Path,
    source_paths: &HashMap<String, PathBuf>,
) -> anyhow::Result<SceneObject> {
    let payload = archive_obj
        .payload
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("invalid GDS layer payload"))?;
    let source_key = string_field(payload, "source_key")?;
    let source_path = source_paths
        .get(&source_key)
        .ok_or_else(|| anyhow::anyhow!("missing embedded GDS source: {source_key}"))?;
    let cell_name = string_field(payload, "cell_name")?;
    let layer = i32_field(payload, "layer")?;
    let datatype = i32_field(payload, "datatype")?;

    let mut restored = model::import_gds_layers(source_path)?
        .into_iter()
        .find_map(|obj| match obj {
            SceneObject::GdsLayer(layer_obj)
                if layer_obj.cell_name == cell_name
                    && layer_obj.layer == layer
                    && layer_obj.datatype == datatype =>
            {
                Some(layer_obj)
            }
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("unable to restore GDS layer: {source_key}"))?;

    restored.display = display_from_payload(payload)?;
    restored.file_path = archive_path.to_path_buf();
    restored.source_path = source_path.clone();
    restored.source_key = source_key;
    Ok(SceneObject::GdsLayer(restored))
}

fn restore_baseplate(archive_obj: &ArchiveObject) -> anyhow::Result<SceneObject> {
    let payload = archive_obj
        .payload
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("invalid baseplate payload"))?;
    Ok(SceneObject::Baseplate(BaseplateObject {
        id: model::new_object_id(),
        display: display_from_payload(payload)?,
        bounds: bounds_from_payload(payload)?,
    }))
}

fn display_from_payload(
    payload: &serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<DisplayProperties> {
    Ok(DisplayProperties {
        name: string_field(payload, "name")?,
        visible: bool_field(payload, "visible")?,
        color: string_field(payload, "color")?,
        brightness: f32_field(payload, "brightness")?,
        opacity: f32_field(payload, "opacity")?,
        z_min: f32_field(payload, "z_min")?,
        z_max: f32_field(payload, "z_max")?,
    })
}

fn bounds_from_payload(
    payload: &serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<Bounds2d> {
    let bounds = payload
        .get("bounds")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("missing bounds"))?;
    Ok(Bounds2d {
        min_x: f32_field(bounds, "min_x")?,
        min_y: f32_field(bounds, "min_y")?,
        max_x: f32_field(bounds, "max_x")?,
        max_y: f32_field(bounds, "max_y")?,
    })
}

fn materialize_gds_sources(
    temp_dir: &Path,
    sources: HashMap<String, Vec<u8>>,
) -> anyhow::Result<HashMap<String, PathBuf>> {
    let mut paths = HashMap::new();
    for (source_name, data) in sources {
        let Some(file_name) = Path::new(&source_name).file_name() else {
            anyhow::bail!("invalid embedded GDS source name: {source_name}");
        };
        let path = temp_dir.join(file_name);
        fs::write(&path, data)?;
        paths.insert(source_name, path);
    }
    Ok(paths)
}

fn archive_temp_dir(path: &Path) -> PathBuf {
    std::env::temp_dir().join(format!(
        "gds3d-{}",
        archive::source_key_for_path(path).replace('.', "_")
    ))
}

fn ensure_suffix(path: PathBuf, suffix: &str) -> PathBuf {
    if path.extension().and_then(|value| value.to_str()) == Some(suffix) {
        path
    } else {
        path.with_extension(suffix)
    }
}

fn string_field(
    payload: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> anyhow::Result<String> {
    payload
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("missing string field: {field}"))
}

fn bool_field(
    payload: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> anyhow::Result<bool> {
    payload
        .get(field)
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| anyhow::anyhow!("missing bool field: {field}"))
}

fn i32_field(
    payload: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> anyhow::Result<i32> {
    let value = payload
        .get(field)
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| anyhow::anyhow!("missing integer field: {field}"))?;
    Ok(i32::try_from(value)?)
}

fn f32_field(
    payload: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> anyhow::Result<f32> {
    let value = payload
        .get(field)
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| anyhow::anyhow!("missing number field: {field}"))?;
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        anyhow::bail!("invalid number field: {field}");
    }
    Ok(value as f32)
}
