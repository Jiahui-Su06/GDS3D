use std::sync::Arc;

use eframe::egui::{self, RichText, Sense, TextStyle};
use gds3d_viewport::{
    self as viewport, Bounds2d as ViewportBounds2d, Polygon2d as ViewportPolygon2d, ViewportObject,
    ViewportScene,
};
use rust_i18n::t;

use super::*;
mod properties;
mod style;
mod tree;

use properties::{
    FloatRowOptions, edit_float_row, editable_basic, editable_display, readonly_bounds,
    readonly_row,
};
pub(super) use style::{configure_fonts, configure_industrial_style, configure_light_theme};
use tree::tree_row;

impl Gds3dApp {
    fn nudge_startup_window(&mut self, ctx: &egui::Context) {
        if !self.startup_window_nudge_pending {
            return;
        }

        let Some(position) = ctx.input(|input| {
            let outer_rect = input.viewport().outer_rect?;
            Some(egui::pos2(
                outer_rect.min.x,
                (outer_rect.min.y - STARTUP_WINDOW_Y_NUDGE).max(0.0),
            ))
        }) else {
            ctx.request_repaint();
            return;
        };

        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
        self.startup_window_nudge_pending = false;
    }

    fn apply_startup_theme(&mut self, ctx: &egui::Context) {
        if !self.startup_theme_pending {
            return;
        }

        configure_light_theme(ctx);
        self.startup_theme_pending = false;
    }
}

impl eframe::App for Gds3dApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_export_task();
        if self.export_task.is_some() {
            ui.ctx().request_repaint();
        }
        self.apply_startup_theme(ui.ctx());
        self.nudge_startup_window(ui.ctx());
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
        if self.import_dialog.is_some() {
            self.show_import_window(ui.ctx());
        }
    }
}

impl Gds3dApp {
    fn show_menu(&mut self, parent_ui: &mut egui::Ui) {
        egui::Panel::top("menu_bar").show(parent_ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(t!("menu.file").as_ref(), |ui| {
                    ui.set_min_width(MENU_POPUP_MIN_WIDTH);
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
                    ui.set_min_width(MENU_POPUP_MIN_WIDTH);
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
                    let language_label = t!("menu.language");
                    let font_size_label = t!("setting.ui_font_size");
                    let show_axes_label = t!("setting.show_axes");
                    let label_width = settings_label_width(
                        ui,
                        [
                            language_label.as_ref(),
                            font_size_label.as_ref(),
                            show_axes_label.as_ref(),
                        ],
                    );
                    let popup_width = (label_width + SETTINGS_COLUMN_GAP + SETTINGS_CONTROL_WIDTH)
                        .max(SETTINGS_POPUP_MIN_WIDTH);
                    let control_width = popup_width - label_width - SETTINGS_COLUMN_GAP;
                    let row_height = ui.spacing().interact_size.y;
                    ui.set_width(popup_width);
                    ui.spacing_mut().item_spacing.y = 6.0;

                    ui.horizontal(|ui| {
                        settings_label(ui, language_label.as_ref(), label_width, row_height);
                        ui.add_space((SETTINGS_COLUMN_GAP - ui.spacing().item_spacing.x).max(0.0));
                        ui.allocate_ui_with_layout(
                            egui::vec2(control_width, row_height),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.menu_button(self.locale.label(), |ui| {
                                    ui.set_min_width(MENU_POPUP_MIN_WIDTH);
                                    for locale in [Locale::English, Locale::SimplifiedChinese] {
                                        if ui
                                            .radio_value(&mut self.locale, locale, locale.label())
                                            .clicked()
                                        {
                                            rust_i18n::set_locale(locale.code());
                                            self.settings.locale = locale.code().to_owned();
                                            save_app_settings(&self.settings);
                                            self.status = t!("status.language_changed").to_string();
                                        }
                                    }
                                });
                            },
                        );
                    });

                    ui.horizontal(|ui| {
                        settings_label(ui, font_size_label.as_ref(), label_width, row_height);
                        ui.add_space((SETTINGS_COLUMN_GAP - ui.spacing().item_spacing.x).max(0.0));
                        let mut ui_font_size = self.settings.ui_font_size;
                        let font_response = ui.add_sized(
                            [control_width, row_height],
                            egui::Slider::new(
                                &mut ui_font_size,
                                UI_FONT_SIZE_MIN..=UI_FONT_SIZE_MAX,
                            )
                            .show_value(true)
                            .step_by(1.0),
                        );
                        if font_response.changed() {
                            self.settings.ui_font_size = clamp_ui_font_size(ui_font_size);
                        }
                        let should_apply_font_size = font_response.drag_stopped()
                            || font_response.changed() && !font_response.dragged();
                        if should_apply_font_size {
                            configure_industrial_style(ui.ctx(), self.settings.ui_font_size);
                            save_app_settings(&self.settings);
                        }
                    });

                    ui.horizontal(|ui| {
                        settings_label(ui, show_axes_label.as_ref(), label_width, row_height);
                        ui.add_space((SETTINGS_COLUMN_GAP - ui.spacing().item_spacing.x).max(0.0));
                        ui.allocate_ui_with_layout(
                            egui::vec2(control_width, row_height),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let response = ui.checkbox(&mut self.viewport.show_axes, "");
                                if response.changed() {
                                    self.settings.show_axes = self.viewport.show_axes;
                                    save_app_settings(&self.settings);
                                }
                            },
                        );
                    });
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
                    if let Some(path) = self.selection_file_path() {
                        ui.separator();
                        let text = path.display().to_string();
                        ui.add(egui::Label::new(text).truncate())
                            .on_hover_text(path.display().to_string());
                    }
                });
            });
        });
    }

    fn show_left_panel(&mut self, parent_ui: &mut egui::Ui) {
        let panel = egui::Panel::left("component_tree")
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

                let empty_rect = ui.available_rect_before_wrap();
                let empty_response =
                    ui.interact(empty_rect, ui.id().with("empty_scene_area"), Sense::click());
                if empty_response.clicked() {
                    self.selection = Selection::Scene;
                }
            });
        self.save_panel_widths(panel.response.rect.width(), self.right_panel_min_width);
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
        let panel = egui::Panel::right("property_panel")
            .resizable(true)
            .default_size(self.right_panel_min_width)
            .size_range(180.0..=560.0)
            .show(parent_ui, |ui| {
                ui.set_min_width(0.0);
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.heading(t!("panel.properties").as_ref());
                ui.separator();
                match self.selection.clone() {
                    Selection::Scene => self.show_scene_properties(ui),
                    Selection::Cell(key) => self.show_cell_properties(ui, &key),
                    Selection::Object(object_id) => self.show_object_properties(ui, &object_id),
                }
            });
        self.save_panel_widths(self.left_panel_min_width, panel.response.rect.width());
    }

    fn show_scene_properties(&self, ui: &mut egui::Ui) {
        ui.label(t!("property.no_component_selected").as_ref());
    }

    fn show_cell_properties(&self, ui: &mut egui::Ui, key: &CellKey) {
        let summary = self.cell_bounds_summary(key);

        ui.label(RichText::new(t!("property.basic").as_ref()).strong());
        readonly_row(ui, t!("property.cell").as_ref(), &key.cell_name);
        if let Some(bounds) = summary.bounds {
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
        if let Some((z_min, z_max)) = summary.z_range {
            readonly_row(ui, t!("property.z_min").as_ref(), &format!("{z_min:.4}"));
            readonly_row(ui, t!("property.z_max").as_ref(), &format!("{z_max:.4}"));
        }
    }

    fn selection_file_path(&self) -> Option<&Path> {
        match &self.selection {
            Selection::Cell(key) => Some(key.file_path.as_path()),
            Selection::Object(object_id) => {
                let obj = self.scene.get(object_id)?;
                match obj {
                    SceneObject::GdsLayer(layer) => Some(layer.file_path.as_path()),
                    SceneObject::Baseplate(_) => None,
                }
            }
            Selection::Scene => None,
        }
    }

    fn show_object_properties(&mut self, ui: &mut egui::Ui, object_id: &str) {
        let Some(before) = self.scene.get(object_id).cloned() else {
            ui.label(t!("property.no_component_selected").as_ref());
            return;
        };

        self.property_edit.sync(object_id, &before);
        let Some(obj) = self.scene.get_mut(object_id) else {
            return;
        };

        match obj {
            SceneObject::GdsLayer(layer) => {
                editable_basic(ui, &mut layer.display, &mut self.property_edit);
                readonly_row(ui, t!("property.cell").as_ref(), &layer.cell_name);
                readonly_row(ui, t!("property.layer").as_ref(), &layer.layer.to_string());
                readonly_row(
                    ui,
                    t!("property.datatype").as_ref(),
                    &layer.datatype.to_string(),
                );
                editable_display(ui, &mut layer.display, &mut self.property_edit);
                readonly_bounds(
                    ui,
                    &layer.bounds,
                    &mut layer.display,
                    &mut self.property_edit,
                );
            }
            SceneObject::Baseplate(baseplate) => {
                editable_basic(ui, &mut baseplate.display, &mut self.property_edit);
                editable_display(ui, &mut baseplate.display, &mut self.property_edit);
                ui.separator();
                ui.label(RichText::new(t!("property.bounds").as_ref()).strong());
                edit_float_row(
                    ui,
                    t!("property.x_min").as_ref(),
                    &mut self.property_edit.min_x,
                    &mut baseplate.bounds.min_x,
                    FloatRowOptions::new(-1_000_000.0..=1_000_000.0, 1.0)
                        .with_default(baseplate.default_bounds.as_ref().map(|bounds| bounds.min_x)),
                );
                edit_float_row(
                    ui,
                    t!("property.x_max").as_ref(),
                    &mut self.property_edit.max_x,
                    &mut baseplate.bounds.max_x,
                    FloatRowOptions::new(-1_000_000.0..=1_000_000.0, 1.0)
                        .with_default(baseplate.default_bounds.as_ref().map(|bounds| bounds.max_x)),
                );
                edit_float_row(
                    ui,
                    t!("property.y_min").as_ref(),
                    &mut self.property_edit.min_y,
                    &mut baseplate.bounds.min_y,
                    FloatRowOptions::new(-1_000_000.0..=1_000_000.0, 1.0)
                        .with_default(baseplate.default_bounds.as_ref().map(|bounds| bounds.min_y)),
                );
                edit_float_row(
                    ui,
                    t!("property.y_max").as_ref(),
                    &mut self.property_edit.max_y,
                    &mut baseplate.bounds.max_y,
                    FloatRowOptions::new(-1_000_000.0..=1_000_000.0, 1.0)
                        .with_default(baseplate.default_bounds.as_ref().map(|bounds| bounds.max_y)),
                );
                edit_float_row(
                    ui,
                    t!("property.z_min").as_ref(),
                    &mut self.property_edit.z_min,
                    &mut baseplate.display.z_min,
                    FloatRowOptions::new(-1_000_000.0..=1_000_000.0, 0.1)
                        .with_decimals(1)
                        .with_default(Some(baseplate.display.defaults.z_min)),
                );
                edit_float_row(
                    ui,
                    t!("property.z_max").as_ref(),
                    &mut self.property_edit.z_max,
                    &mut baseplate.display.z_max,
                    FloatRowOptions::new(-1_000_000.0..=1_000_000.0, 0.1)
                        .with_decimals(1)
                        .with_default(Some(baseplate.display.defaults.z_max)),
                );
            }
        }

        self.replace_object_after_edit(before);
    }

    fn cell_bounds_summary(&self, key: &CellKey) -> CellBoundsSummary {
        let mut summary = CellBoundsSummary::default();
        for object_id in self.object_ids_for_cell(key) {
            let Some(SceneObject::GdsLayer(layer)) = self.scene.get(&object_id) else {
                continue;
            };

            include_bounds(&mut summary.bounds, &layer.bounds);
            summary.z_range = Some(
                summary
                    .z_range
                    .map(|(z_min, z_max)| {
                        (
                            z_min.min(layer.display.z_min),
                            z_max.max(layer.display.z_max),
                        )
                    })
                    .unwrap_or((layer.display.z_min, layer.display.z_max)),
            );
        }
        summary
    }

    fn show_import_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut should_cancel = false;
        let mut import_request = None;

        if let Some(state) = self.import_dialog.as_mut() {
            egui::Window::new(t!("dialog.import_gds").as_ref())
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .default_width(560.0)
                .max_width(640.0)
                .max_height(520.0)
                .resizable(true)
                .show(ctx, |ui| {
                    let path = state.info.file_path.display().to_string();
                    ui.add_sized(
                        [ui.available_width(), ui.spacing().interact_size.y],
                        egui::Label::new(path.clone()).truncate(),
                    )
                    .on_hover_text(path);
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .max_height(import_dialog_tree_height(&state.info))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for cell in &state.info.cells {
                                let layer_count = cell.layers.len();
                                let checked_count = cell
                                    .layers
                                    .iter()
                                    .filter(|layer| state.checked_layers.contains(&layer.selection))
                                    .count();
                                let mut cell_checked =
                                    layer_count > 0 && checked_count == layer_count;
                                let label = if checked_count > 0 && checked_count < layer_count {
                                    format!("{} ({checked_count}/{layer_count})", cell.name)
                                } else {
                                    cell.name.clone()
                                };

                                ui.horizontal(|ui| {
                                    let response = ui.checkbox(&mut cell_checked, label);
                                    if response.changed() {
                                        for layer in &cell.layers {
                                            if cell_checked {
                                                state
                                                    .checked_layers
                                                    .insert(layer.selection.clone());
                                            } else {
                                                state.checked_layers.remove(&layer.selection);
                                            }
                                        }
                                        state.warning = None;
                                    }
                                });

                                for layer in &cell.layers {
                                    ui.horizontal(|ui| {
                                        ui.add_space(24.0);
                                        let mut checked =
                                            state.checked_layers.contains(&layer.selection);
                                        let selection = &layer.selection;
                                        let response = ui.checkbox(
                                            &mut checked,
                                            t!(
                                                "gds_import.layer_datatype",
                                                layer = selection.layer,
                                                datatype = selection.datatype
                                            )
                                            .as_ref(),
                                        );
                                        if response.changed() {
                                            if checked {
                                                state.checked_layers.insert(selection.clone());
                                            } else {
                                                state.checked_layers.remove(selection);
                                            }
                                            state.warning = None;
                                        }
                                        ui.add_space(10.0);
                                        ui.label(
                                            t!(
                                                "gds_import.polygons_count",
                                                count = layer.polygon_count
                                            )
                                            .as_ref(),
                                        );
                                        ui.add_space(10.0);
                                        ui.label(format_bounds(&layer.bounds));
                                    });
                                }
                            }
                        });

                    if let Some(warning) = &state.warning {
                        ui.separator();
                        ui.colored_label(egui::Color32::from_rgb(180, 74, 60), warning);
                    }

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button(t!("action.import").as_ref()).clicked() {
                            let selections = state.selected_layers();
                            if selections.is_empty() {
                                state.warning =
                                    Some(t!("gds_import.select_layer_warning").to_string());
                            } else {
                                import_request = Some((state.info.file_path.clone(), selections));
                            }
                        }
                        if ui.button(t!("action.cancel").as_ref()).clicked() {
                            should_cancel = true;
                        }
                    });
                });
        }

        if let Some((path, selections)) = import_request {
            self.import_dialog = None;
            self.import_selected_gds_layers(path, selections);
        } else if should_cancel || !open {
            self.import_dialog = None;
        }
    }

    fn show_export_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_export_dialog;
        let mut should_close = false;
        egui::Window::new(t!("dialog.export_as").as_ref())
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
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
                    if ui
                        .add_enabled(
                            self.export_task.is_none(),
                            egui::Button::new(t!("action.export").as_ref()),
                        )
                        .clicked()
                    {
                        should_close = self.export_scene_as();
                    }
                    if ui.button(t!("action.cancel").as_ref()).clicked() {
                        should_close = true;
                    }
                });
            });
        self.show_export_dialog = open && !should_close;
    }
}

pub(super) fn viewport_scene(
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

#[derive(Default)]
struct CellBoundsSummary {
    bounds: Option<Bounds2d>,
    z_range: Option<(f32, f32)>,
}

fn include_bounds(target: &mut Option<Bounds2d>, bounds: &Bounds2d) {
    match target {
        Some(target) => {
            target.min_x = target.min_x.min(bounds.min_x);
            target.min_y = target.min_y.min(bounds.min_y);
            target.max_x = target.max_x.max(bounds.max_x);
            target.max_y = target.max_y.max(bounds.max_y);
        }
        None => {
            *target = Some(bounds.clone());
        }
    }
}

fn format_bounds(bounds: &Bounds2d) -> String {
    format!(
        "{:.2}, {:.2} - {:.2}, {:.2}",
        bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y
    )
}

fn import_dialog_tree_height(info: &model::GdsFileInfo) -> f32 {
    let row_count = info
        .cells
        .iter()
        .map(|cell| 1 + cell.layers.len())
        .sum::<usize>();
    (row_count as f32 * 28.0 + 8.0).clamp(96.0, 360.0)
}

fn settings_label_width<'a>(ui: &egui::Ui, labels: impl IntoIterator<Item = &'a str>) -> f32 {
    let font_size = TextStyle::Body.resolve(ui.style()).size;
    let max_chars = labels
        .into_iter()
        .map(|label| label.chars().count())
        .max()
        .unwrap_or(0);
    (max_chars as f32 * font_size * 0.78).clamp(42.0, 96.0)
}

fn settings_label(ui: &mut egui::Ui, text: &str, width: f32, height: f32) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(text);
        },
    );
}
