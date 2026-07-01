use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui_wgpu;
use gds3d_viewport::{ViewportObject, ViewportState};
use rust_i18n::t;

use crate::archive::{self, ArchiveObject};
use crate::export::{self, ExportFormat, ExportQuality, ExportSettings, ExportSizePreset};
use crate::model::{
    self, BaseplateObject, Bounds2d, CellKey, DisplayProperties, Scene, SceneObject, Selection,
};

#[path = "ui/mod.rs"]
mod ui;

const UNDO_STACK_MAX: usize = 100;
const LUCIDE_FONT_FAMILY: &str = "lucide";
const PROPERTY_LABEL_WIDTH: f32 = 88.0;
const PROPERTY_NUMBER_WIDTH: f32 = 58.0;
const PROPERTY_ROW_HEIGHT_MIN: f32 = 22.0;
const PROPERTY_VALUE_MIN_WIDTH: f32 = 64.0;
const PROPERTY_RESET_WIDTH: f32 = 20.0;
const MENU_POPUP_MIN_WIDTH: f32 = 112.0;
const SETTINGS_POPUP_MIN_WIDTH: f32 = 190.0;
const SETTINGS_COLUMN_GAP: f32 = 8.0;
const SETTINGS_CONTROL_WIDTH: f32 = 112.0;
const SETTINGS_DIR_NAME: &str = "GDS3D";
const SETTINGS_FILE_NAME: &str = "settings.json";
const UI_FONT_SIZE_DEFAULT: f32 = 14.0;
const UI_FONT_SIZE_MIN: f32 = 12.0;
const UI_FONT_SIZE_MAX: f32 = 20.0;
const STARTUP_WINDOW_Y_NUDGE: f32 = 24.0;

pub struct Gds3dApp {
    scene: Scene,
    selection: Selection,
    collapsed_cells: HashSet<CellKey>,
    viewport: ViewportState,
    render_state: Option<egui_wgpu::RenderState>,
    viewport_scene_cache: ViewportSceneCache,
    undo_stack: Vec<UndoCommand>,
    status: String,
    export_settings: ExportSettings,
    show_export_dialog: bool,
    import_dialog: Option<ImportDialogState>,
    export_task: Option<ExportTask>,
    left_panel_min_width: f32,
    right_panel_min_width: f32,
    locale: Locale,
    settings: AppSettings,
    archive_temp_dir: Option<PathBuf>,
    property_edit: PropertyEditState,
    startup_theme_pending: bool,
    startup_window_nudge_pending: bool,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct AppSettings {
    locale: String,
    show_axes: bool,
    left_panel_width: f32,
    right_panel_width: f32,
    #[serde(default = "default_ui_font_size")]
    ui_font_size: f32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            locale: "en".to_owned(),
            show_axes: true,
            left_panel_width: 240.0,
            right_panel_width: 280.0,
            ui_font_size: UI_FONT_SIZE_DEFAULT,
        }
    }
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

struct ExportTask {
    receiver: mpsc::Receiver<anyhow::Result<PathBuf>>,
}

#[derive(Default)]
struct PropertyEditState {
    object_id: Option<String>,
    name: String,
    color: String,
    brightness: f32,
    z_min: f32,
    z_max: f32,
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
}

impl PropertyEditState {
    fn sync(&mut self, object_id: &str, obj: &SceneObject) {
        if self.object_id.as_deref() == Some(object_id) {
            return;
        }

        let display = obj.display();
        let bounds = obj.bounds();
        self.object_id = Some(object_id.to_owned());
        self.name = display.name.clone();
        self.color = display.color.clone();
        self.brightness = display.brightness;
        self.z_min = display.z_min;
        self.z_max = display.z_max;
        self.min_x = bounds.min_x;
        self.max_x = bounds.max_x;
        self.min_y = bounds.min_y;
        self.max_y = bounds.max_y;
    }
}

struct ImportDialogState {
    info: model::GdsFileInfo,
    checked_layers: HashSet<model::GdsLayerSelection>,
    warning: Option<String>,
}

impl ImportDialogState {
    fn new(info: model::GdsFileInfo) -> Self {
        let mut checked_layers = HashSet::new();
        let layers = info
            .cells
            .iter()
            .flat_map(|cell| cell.layers.iter())
            .collect::<Vec<_>>();
        if let [layer] = layers.as_slice() {
            checked_layers.insert(layer.selection.clone());
        }

        Self {
            info,
            checked_layers,
            warning: None,
        }
    }

    fn selected_layers(&self) -> Vec<model::GdsLayerSelection> {
        self.info
            .cells
            .iter()
            .flat_map(|cell| cell.layers.iter())
            .filter(|layer| self.checked_layers.contains(&layer.selection))
            .map(|layer| layer.selection.clone())
            .collect()
    }
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

    fn from_code(code: &str) -> Self {
        match code {
            "zh-CN" => Self::SimplifiedChinese,
            _ => Self::English,
        }
    }
}

impl Gds3dApp {
    fn poll_export_task(&mut self) {
        let Some(task) = self.export_task.as_ref() else {
            return;
        };
        match task.receiver.try_recv() {
            Ok(Ok(path)) => {
                self.status = t!("status.exported", name = file_name(&path)).to_string();
                self.export_task = None;
            }
            Ok(Err(err)) => {
                self.status = t!("status.export_failed", error = err).to_string();
                self.export_task = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status =
                    t!("status.export_failed", error = "export worker disconnected").to_string();
                self.export_task = None;
            }
        }
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::configure_light_theme(&cc.egui_ctx);
        ui::configure_fonts(&cc.egui_ctx);
        let settings = load_app_settings();
        ui::configure_industrial_style(&cc.egui_ctx, settings.ui_font_size);
        let locale = Locale::from_code(&settings.locale);
        rust_i18n::set_locale(locale.code());
        let mut viewport = ViewportState::new(cc.wgpu_render_state.as_ref());
        viewport.show_axes = settings.show_axes;
        Self {
            scene: Scene::default(),
            selection: Selection::Scene,
            collapsed_cells: HashSet::new(),
            viewport,
            render_state: cc.wgpu_render_state.clone(),
            viewport_scene_cache: ViewportSceneCache::default(),
            undo_stack: Vec::new(),
            status: t!("status.ready").to_string(),
            export_settings: ExportSettings::default(),
            show_export_dialog: false,
            import_dialog: None,
            export_task: None,
            left_panel_min_width: settings.left_panel_width,
            right_panel_min_width: settings.right_panel_width,
            locale,
            settings,
            archive_temp_dir: None,
            property_edit: PropertyEditState::default(),
            startup_theme_pending: true,
            startup_window_nudge_pending: true,
        }
    }

    fn import_gds(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("GDS", &["gds"])
            .pick_file()
        else {
            return;
        };

        match model::inspect_gds_file(&path) {
            Ok(info) => {
                self.import_dialog = Some(ImportDialogState::new(info));
            }
            Err(err) => {
                self.status = t!("status.import_failed", error = err).to_string();
            }
        }
    }

    fn import_selected_gds_layers(
        &mut self,
        path: PathBuf,
        selections: Vec<model::GdsLayerSelection>,
    ) {
        let objects = match model::import_gds_layer_selections(&path, &selections) {
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

    fn export_scene_as(&mut self) -> bool {
        let format = self.export_settings.format;
        let extension = format.extension();
        let default_file_name = format!("scene.{extension}");
        let Some(path) = rfd::FileDialog::new()
            .add_filter(format.label(), &[extension])
            .set_file_name(&default_file_name)
            .save_file()
        else {
            return false;
        };

        let path = ensure_suffix(path, extension);
        if format == ExportFormat::Png {
            match self.start_view_png_export(path) {
                Ok(()) => return true,
                Err(err) => {
                    self.status = t!("status.export_failed", error = err).to_string();
                    return false;
                }
            }
        }
        if format == ExportFormat::Svg {
            match self.start_view_svg_export(path) {
                Ok(()) => return true,
                Err(err) => {
                    self.status = t!("status.export_failed", error = err).to_string();
                    return false;
                }
            }
        }
        if format == ExportFormat::Pdf {
            match self.start_view_pdf_export(path) {
                Ok(()) => return true,
                Err(err) => {
                    self.status = t!("status.export_failed", error = err).to_string();
                    return false;
                }
            }
        }

        let result = match format {
            ExportFormat::Png => unreachable!("PNG export is handled above"),
            ExportFormat::Svg => unreachable!("SVG export is handled above"),
            ExportFormat::Pdf => unreachable!("PDF export is handled above"),
            ExportFormat::Gltf => {
                export::write_scene_export(&path, &self.scene, self.export_settings)
            }
        };

        match result {
            Ok(()) => {
                self.status = t!("status.exported", name = file_name(&path)).to_string();
                true
            }
            Err(err) => {
                self.status = t!("status.export_failed", error = err).to_string();
                false
            }
        }
    }

    fn start_view_png_export(&mut self, path: PathBuf) -> anyhow::Result<()> {
        let Some(render_state) = self.render_state.as_ref() else {
            anyhow::bail!("PNG export requires the WGPU renderer");
        };
        let Some((width, height)) = self.export_settings.image_size() else {
            anyhow::bail!("PNG export requires an image size");
        };
        let render_state = render_state.clone();
        let scene =
            ui::viewport_scene(&self.scene, &self.selection, &mut self.viewport_scene_cache);
        let scene = export_viewport_scene(scene);
        let viewport = self.viewport.clone();

        let (sender, receiver) = mpsc::channel();
        let path_for_worker = path.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<PathBuf> {
                let rgba = gds3d_viewport::render_view_rgba_canvas(
                    &render_state,
                    &scene,
                    &viewport,
                    width,
                    height,
                )
                .map_err(|err| anyhow::anyhow!(err))?;
                let png = gds3d_viewport::encode_rgba_png(width, height, &rgba)
                    .map_err(|err| anyhow::anyhow!(err))?;
                fs::write(&path_for_worker, png)?;
                Ok(path_for_worker)
            })();
            let _ = sender.send(result);
        });
        self.export_task = Some(ExportTask { receiver });
        self.status = t!("status.exporting", name = file_name(&path)).to_string();
        Ok(())
    }

    fn start_view_svg_export(&mut self, path: PathBuf) -> anyhow::Result<()> {
        let Some(render_state) = self.render_state.as_ref() else {
            anyhow::bail!("SVG export requires the WGPU renderer");
        };
        let Some((width, height)) = self.export_settings.image_size() else {
            anyhow::bail!("SVG export requires an image size");
        };
        let render_state = render_state.clone();
        let scene =
            ui::viewport_scene(&self.scene, &self.selection, &mut self.viewport_scene_cache);
        let scene = export_viewport_scene(scene);
        let viewport = self.viewport.clone();
        let title = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("scene")
            .to_owned();

        let (sender, receiver) = mpsc::channel();
        let path_for_worker = path.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<PathBuf> {
                let rgba = gds3d_viewport::render_view_rgba_canvas(
                    &render_state,
                    &scene,
                    &viewport,
                    width,
                    height,
                )
                .map_err(|err| anyhow::anyhow!(err))?;
                let png = gds3d_viewport::encode_rgba_png(width, height, &rgba)
                    .map_err(|err| anyhow::anyhow!(err))?;
                let svg = gds3d_viewport::embedded_png_svg(width, height, &title, &png)
                    .map_err(|err| anyhow::anyhow!(err))?;
                fs::write(&path_for_worker, svg)?;
                Ok(path_for_worker)
            })();
            let _ = sender.send(result);
        });
        self.export_task = Some(ExportTask { receiver });
        self.status = t!("status.exporting", name = file_name(&path)).to_string();
        Ok(())
    }

    fn start_view_pdf_export(&mut self, path: PathBuf) -> anyhow::Result<()> {
        let Some(render_state) = self.render_state.as_ref() else {
            anyhow::bail!("PDF export requires the WGPU renderer");
        };
        let Some((width, height)) = self.export_settings.image_size() else {
            anyhow::bail!("PDF export requires an image size");
        };
        let render_state = render_state.clone();
        let viewport_scene =
            ui::viewport_scene(&self.scene, &self.selection, &mut self.viewport_scene_cache);
        let viewport_scene = export_viewport_scene(viewport_scene);
        let viewport = self.viewport.clone();
        let scene = self.scene.clone();

        let (sender, receiver) = mpsc::channel();
        let path_for_worker = path.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<PathBuf> {
                let rgba = gds3d_viewport::render_view_rgba_canvas(
                    &render_state,
                    &viewport_scene,
                    &viewport,
                    width,
                    height,
                )
                .map_err(|err| anyhow::anyhow!(err))?;
                let png = gds3d_viewport::encode_rgba_png(width, height, &rgba)
                    .map_err(|err| anyhow::anyhow!(err))?;
                export::write_pdf_report(&path_for_worker, &scene, &png, width, height)?;
                Ok(path_for_worker)
            })();
            let _ = sender.send(result);
        });
        self.export_task = Some(ExportTask { receiver });
        self.status = t!("status.exporting", name = file_name(&path)).to_string();
        Ok(())
    }

    fn create_baseplate(&mut self) {
        let bounds = self.scene.default_baseplate_bounds(&self.selection);
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

    fn save_panel_widths(&mut self, left_width: f32, right_width: f32) {
        let left_width = left_width.clamp(160.0, 520.0);
        let right_width = right_width.clamp(180.0, 560.0);
        let changed = (self.settings.left_panel_width - left_width).abs() > 0.5
            || (self.settings.right_panel_width - right_width).abs() > 0.5;
        if !changed {
            return;
        }

        self.left_panel_min_width = left_width;
        self.right_panel_min_width = right_width;
        self.settings.left_panel_width = left_width;
        self.settings.right_panel_width = right_width;
        save_app_settings(&self.settings);
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

fn default_ui_font_size() -> f32 {
    UI_FONT_SIZE_DEFAULT
}

fn clamp_ui_font_size(ui_font_size: f32) -> f32 {
    if ui_font_size.is_finite() {
        ui_font_size.clamp(UI_FONT_SIZE_MIN, UI_FONT_SIZE_MAX)
    } else {
        UI_FONT_SIZE_DEFAULT
    }
}

fn export_viewport_scene(
    mut scene: gds3d_viewport::ViewportScene,
) -> gds3d_viewport::ViewportScene {
    scene.selected_id = None;
    scene
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<unnamed>")
        .to_owned()
}

fn load_app_settings() -> AppSettings {
    let Some(path) = settings_path() else {
        return AppSettings::default();
    };
    let Ok(data) = fs::read_to_string(path) else {
        return AppSettings::default();
    };
    serde_json::from_str(&data)
        .map(sanitize_app_settings)
        .unwrap_or_default()
}

fn sanitize_app_settings(mut settings: AppSettings) -> AppSettings {
    settings.left_panel_width = settings.left_panel_width.clamp(160.0, 520.0);
    settings.right_panel_width = settings.right_panel_width.clamp(180.0, 560.0);
    settings.ui_font_size = clamp_ui_font_size(settings.ui_font_size);
    settings
}

fn save_app_settings(settings: &AppSettings) {
    let Some(path) = settings_path() else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(data) = serde_json::to_vec_pretty(settings) else {
        return;
    };
    let _ = fs::write(path, data);
}

fn settings_path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(SETTINGS_DIR_NAME).join(SETTINGS_FILE_NAME))
}

#[cfg(target_os = "windows")]
fn config_dir() -> Option<PathBuf> {
    env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn config_dir() -> Option<PathBuf> {
    home_dir().map(|home| home.join("Library").join("Application Support"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn config_dir() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".config")))
}

#[cfg(not(any(unix, target_os = "windows")))]
fn config_dir() -> Option<PathBuf> {
    home_dir().map(|home| home.join(".config"))
}

#[cfg(not(target_os = "windows"))]
fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
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
    restored.display.defaults = default_display(&restored.display);
    restored.id = optional_string_field(payload, "object_id").unwrap_or_else(model::new_object_id);
    restored.file_path =
        optional_path_field(payload, "display_path").unwrap_or_else(|| archive_path.to_path_buf());
    restored.source_path = source_path.clone();
    restored.source_key = source_key;
    Ok(SceneObject::GdsLayer(restored))
}

fn restore_baseplate(archive_obj: &ArchiveObject) -> anyhow::Result<SceneObject> {
    let payload = archive_obj
        .payload
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("invalid baseplate payload"))?;
    let bounds = bounds_from_payload(payload)?;
    Ok(SceneObject::Baseplate(BaseplateObject {
        id: optional_string_field(payload, "object_id").unwrap_or_else(model::new_object_id),
        display: {
            let mut display = display_from_payload(payload)?;
            display.defaults = default_display(&display);
            display
        },
        bounds: bounds.clone(),
        default_bounds: Some(bounds),
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
        z_min: f32_field(payload, "z_min")?,
        z_max: f32_field(payload, "z_max")?,
        defaults: model::DisplayDefaults::default(),
    })
}

fn default_display(display: &DisplayProperties) -> model::DisplayDefaults {
    model::DisplayDefaults {
        name: display.name.clone(),
        color: display.color.clone(),
        brightness: display.brightness,
        z_min: display.z_min,
        z_max: display.z_max,
    }
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

fn optional_string_field(
    payload: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<String> {
    payload
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

fn optional_path_field(
    payload: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<PathBuf> {
    optional_string_field(payload, field)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
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
