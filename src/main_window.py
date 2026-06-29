from __future__ import annotations

from hashlib import sha1
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Callable

from PySide6.QtCore import QSettings, Qt
from PySide6.QtWidgets import (
    QFileDialog,
    QDockWidget,
    QMenu,
    QMainWindow,
    QMessageBox,
)

from component_tree import ComponentGroupInfo, ComponentTree
from gds_import_dialog import GdsImportDialog
from gds_loader import GdsLayerData, inspect_gds_file, load_gds_layers
from objects import BaseplateObject, Bounds2D, GdsLayerObject, SceneObject
from pdf_exporter import export_scene_pdf
from project_archive import ProjectArchiveObject, read_project_archive, write_project_archive
from property_panel import PropertyPanel
from scene import Scene
from ui_settings_dialog import (
    LEFT_PANEL_MIN_WIDTH_DEFAULT,
    RIGHT_PANEL_MIN_WIDTH_DEFAULT,
    UiSettings,
    UiSettingsDialog,
)
from viewport import Viewport


class MainWindow(QMainWindow):
    def __init__(self) -> None:
        super().__init__()
        self.setWindowTitle("GDS3D")
        self.resize(1280, 820)

        self.scene = Scene()
        self._gds_data: dict[str, GdsLayerData] = {}
        self._project_temp_dir: TemporaryDirectory[str] | None = None
        self._settings = QSettings("GDS3D", "GDS3D")
        self._ui_settings = self._load_ui_settings()

        self.viewport = Viewport(self)
        self.component_tree = ComponentTree(self)
        self.property_panel = PropertyPanel(self)

        self.setCentralWidget(self.viewport)
        self._create_docks()
        self._create_menu_bar()
        self._apply_ui_settings()
        self.statusBar().showMessage("Ready")

        self.component_tree.object_selected.connect(self._select_object)
        self.component_tree.visibility_changed.connect(self._set_visibility)
        self.property_panel.property_changed.connect(self._update_property)
        self.property_panel.reset_requested.connect(self._reset_property)
        self.property_panel.show_scene_summary(self.scene.count())

    def import_gds(self) -> None:
        file_name, _ = QFileDialog.getOpenFileName(
            self,
            "Import GDS",
            str(Path.cwd()),
            "GDS Files (*.gds);;All Files (*)",
        )
        if not file_name:
            return

        try:
            file_info = inspect_gds_file(Path(file_name))
            dialog = GdsImportDialog(file_info, self)
            if dialog.exec() != GdsImportDialog.DialogCode.Accepted:
                return

            layers = load_gds_layers(file_info.file_path, dialog.selected_layers())
            if not layers:
                return

            for data in layers:
                obj = GdsLayerObject(
                    name=f"L{data.layer}/{data.datatype}",
                    file_path=data.file_path,
                    source_path=data.file_path,
                    cell_name=data.cell_name,
                    layer=data.layer,
                    datatype=data.datatype,
                    bounds=data.bounds,
                    source_key=_gds_source_key(data.file_path),
                )
                self.scene.add(obj)
                self._gds_data[obj.id] = data
                self.viewport.add_or_update(obj, data.polygons, _gds_cache_key(data))
                self.component_tree.add_object(obj)
            self.viewport.reset_camera()
            self.statusBar().showMessage(
                f"Imported {len(layers)} layer(s) from {file_info.file_path.name}"
            )
        except Exception as exc:
            self._show_error("Import failed", str(exc))

    def open_project(self) -> None:
        file_name, _ = QFileDialog.getOpenFileName(
            self,
            "Open Project",
            str(Path.cwd()),
            "GDS3D Projects (*.gds3d);;All Files (*)",
        )
        if not file_name:
            return

        try:
            self._load_project(Path(file_name))
            self.statusBar().showMessage(f"Opened {Path(file_name).name}")
        except Exception as exc:
            self._show_error("Open project failed", str(exc))

    def export_view_as_png(self) -> None:
        self._export_view("Export View", "PNG Files (*.png)", "png", self.viewport.export_png)

    def export_view_as_svg(self) -> None:
        self._export_view("Export View", "SVG Files (*.svg)", "svg", self.viewport.export_svg)

    def export_view_as_pdf(self) -> None:
        self._export_view("Export View", "PDF Files (*.pdf)", "pdf", self._export_pdf)

    def export_scene_as_gltf(self) -> None:
        self._export_view(
            "Export Scene",
            "glTF Files (*.gltf)",
            "gltf",
            self.viewport.export_gltf,
        )

    def export_project_as_gds3d(self) -> None:
        file_name, _ = QFileDialog.getSaveFileName(
            self,
            "Export Project",
            str(Path.cwd() / "project.gds3d"),
            "GDS3D Projects (*.gds3d);;All Files (*)",
        )
        if not file_name:
            return

        path = self._ensure_suffix(Path(file_name), "gds3d")
        try:
            self._write_project(path)
            self.statusBar().showMessage(f"Exported {path.name}")
        except Exception as exc:
            self._show_error("Export failed", str(exc))

    def create_baseplate(self) -> None:
        bounds = self._default_baseplate_bounds()
        obj = BaseplateObject(name=self._next_baseplate_name(), bounds=bounds)
        try:
            should_reset_camera = self.scene.count() == 0
            self.scene.add(obj)
            self.viewport.add_or_update(obj)
            self.component_tree.add_object(obj)
            if should_reset_camera:
                self.viewport.reset_camera()
            self.statusBar().showMessage(f"Created {obj.name}")
        except Exception as exc:
            self._show_error("Create baseplate failed", str(exc))

    def delete_selected(self) -> None:
        current_item = self.component_tree.currentItem()
        group_info = self.component_tree.group_info_for_item(current_item)
        if group_info is not None:
            self._delete_objects(
                group_info.object_ids, f"Deleted cell {group_info.name}"
            )
            return

        object_id = self.component_tree.current_object_id()
        if object_id is None:
            return

        obj = self.scene.get(object_id)
        if obj is None:
            return

        self._delete_objects((object_id,), f"Deleted {obj.name}")

    def _delete_objects(self, object_ids: tuple[str, ...], status_message: str) -> None:
        deleted = False
        for object_id in object_ids:
            if self.scene.get(object_id) is None:
                continue
            self.scene.remove(object_id)
            self._gds_data.pop(object_id, None)
            self.viewport.remove_object(object_id)
            self.component_tree.remove_object(object_id)
            deleted = True

        if not deleted:
            return

        self.property_panel.show_scene_summary(self.scene.count())
        self.statusBar().showMessage(status_message)

    def _create_docks(self) -> None:
        self.left_dock = QDockWidget("Components", self)
        self.left_dock.setObjectName("componentsDock")
        self.left_dock.setWidget(self.component_tree)
        self.addDockWidget(Qt.DockWidgetArea.LeftDockWidgetArea, self.left_dock)

        self.right_dock = QDockWidget("Properties", self)
        self.right_dock.setObjectName("propertiesDock")
        self.right_dock.setWidget(self.property_panel)
        self.addDockWidget(Qt.DockWidgetArea.RightDockWidgetArea, self.right_dock)

    def _create_menu_bar(self) -> None:
        file_menu = self.menuBar().addMenu("&File")
        file_menu.addAction("Open Project", self.open_project)
        file_menu.addAction("Import GDS", self.import_gds)
        file_menu.addSeparator()

        file_menu.addAction("Export", self.export_project_as_gds3d)

        export_as_menu = QMenu("Export As", self)
        export_as_menu.addAction("PNG", self.export_view_as_png)
        export_as_menu.addAction("SVG", self.export_view_as_svg)
        export_as_menu.addAction("PDF", self.export_view_as_pdf)
        export_as_menu.addAction("glTF", self.export_scene_as_gltf)
        file_menu.addMenu(export_as_menu)

        edit_menu = self.menuBar().addMenu("&Edit")
        edit_menu.addAction("Create Baseplate", self.create_baseplate)
        edit_menu.addAction("Delete", self.delete_selected)
        edit_menu.addSeparator()
        edit_menu.addAction("Reset Camera", self.viewport.reset_camera)

        settings_menu = self.menuBar().addMenu("&Settings")
        settings_menu.addAction("UI Settings", self.open_ui_settings)

    def open_ui_settings(self) -> None:
        dialog = UiSettingsDialog(self._ui_settings, self)
        if dialog.exec() != UiSettingsDialog.DialogCode.Accepted:
            return

        self._ui_settings = dialog.settings()
        self._save_ui_settings(self._ui_settings)
        self._apply_ui_settings()
        self.statusBar().showMessage("Updated UI settings")

    def _load_ui_settings(self) -> UiSettings:
        left_width = _settings_int(
            self._settings,
            "ui/leftPanelMinWidth",
            LEFT_PANEL_MIN_WIDTH_DEFAULT,
            minimum=120,
            maximum=800,
        )
        right_width = _settings_int(
            self._settings,
            "ui/rightPanelMinWidth",
            RIGHT_PANEL_MIN_WIDTH_DEFAULT,
            minimum=120,
            maximum=800,
        )
        show_axes = self._settings.value("ui/showAxes", True, type=bool)
        return UiSettings(left_width, right_width, bool(show_axes))

    def _save_ui_settings(self, settings: UiSettings) -> None:
        self._settings.setValue("ui/leftPanelMinWidth", settings.left_panel_min_width)
        self._settings.setValue("ui/rightPanelMinWidth", settings.right_panel_min_width)
        self._settings.setValue("ui/showAxes", settings.show_axes)

    def _apply_ui_settings(self) -> None:
        left_width = self._ui_settings.left_panel_min_width
        right_width = self._ui_settings.right_panel_min_width
        self.left_dock.setMinimumWidth(left_width)
        self.right_dock.setMinimumWidth(right_width)
        self.resizeDocks(
            [self.left_dock, self.right_dock],
            [left_width, right_width],
            Qt.Orientation.Horizontal,
        )
        self.viewport.set_axes_visible(self._ui_settings.show_axes)

    def _select_object(self, object_id: object) -> None:
        if isinstance(object_id, ComponentGroupInfo):
            bounds = self._bounds_for_objects(object_id.object_ids)
            z_range = self._z_range_for_objects(object_id.object_ids)
            self.property_panel.show_cell_summary(
                object_id.name,
                object_id.file_path,
                object_id.object_count,
                bounds,
                z_range[0] if z_range is not None else None,
                z_range[1] if z_range is not None else None,
            )
            self.viewport.highlight_objects(list(object_id.object_ids))
            self.statusBar().showMessage(f"Selected cell {object_id.name}")
            return

        if not isinstance(object_id, str):
            self.property_panel.show_scene_summary(self.scene.count())
            self.viewport.highlight_object(None)
            self.statusBar().showMessage("Scene selected")
            return

        obj = self.scene.get(object_id)
        if obj is None:
            self.property_panel.show_scene_summary(self.scene.count())
            self.viewport.highlight_object(None)
            return

        self.property_panel.set_object(obj)
        self.viewport.highlight_object(obj.id)
        self.statusBar().showMessage(f"Selected {obj.name}")

    def _update_property(self, object_id: str, field: str, value: object) -> None:
        obj = self.scene.get(object_id)
        if obj is None:
            return

        try:
            self._apply_property(obj, field, value)
            self._sync_view_after_property(obj, field)
            self.component_tree.refresh_object(obj)
            self.component_tree.select_object(obj.id)
            self.statusBar().showMessage(f"Updated {obj.name}: {field}")
        except Exception as exc:
            self.property_panel.set_object(obj)
            self._show_error("Invalid property", str(exc))

    def _set_visibility(self, object_id: str, visible: bool) -> None:
        obj = self.scene.get(object_id)
        if obj is None:
            return

        obj.visible = visible
        self.viewport.update_actor(obj)
        self.component_tree.refresh_object(obj)
        state = "shown" if visible else "hidden"
        self.statusBar().showMessage(f"{obj.name} {state}")

    def _reset_property(self, object_id: str, field: str) -> None:
        obj = self.scene.get(object_id)
        if obj is None:
            return

        if field not in obj.defaults:
            return

        try:
            self._apply_property(obj, field, obj.defaults[field])
            self._sync_view_after_property(obj, field)
            self.component_tree.refresh_object(obj)
            self.property_panel.set_object(obj)
            self.component_tree.select_object(obj.id)
            self.statusBar().showMessage(f"Reset {obj.name}: {field}")
        except Exception as exc:
            self.property_panel.set_object(obj)
            self._show_error("Reset failed", str(exc))

    def _apply_property(self, obj: SceneObject, field: str, value: object) -> None:
        if field == "name":
            text = str(value).strip()
            if not text:
                raise ValueError("name cannot be empty")
            obj.name = text
            return
        if field == "visible":
            obj.visible = bool(value)
            return
        if field == "color":
            obj.color = str(value)
            return
        if field == "brightness":
            brightness = float(value)
            if not 0.0 <= brightness <= 2.0:
                raise ValueError("brightness must be between 0 and 2")
            obj.brightness = brightness
            return
        if field == "opacity":
            opacity = float(value)
            if not 0.0 <= opacity <= 1.0:
                raise ValueError("opacity must be between 0 and 1")
            obj.opacity = opacity
            return
        if field in {"z_min", "z_max"}:
            self._apply_z(obj, field, float(value))
            return
        if isinstance(obj, BaseplateObject) and field in {
            "min_x",
            "min_y",
            "max_x",
            "max_y",
        }:
            self._apply_baseplate_bound(obj, field, float(value))
            return
        raise ValueError(f"unsupported property: {field}")

    def _apply_z(self, obj: SceneObject, field: str, value: float) -> None:
        z_min = value if field == "z_min" else obj.z_min
        z_max = value if field == "z_max" else obj.z_max
        if z_min >= z_max:
            raise ValueError("z_min must be smaller than z_max")
        obj.z_min = z_min
        obj.z_max = z_max

    def _apply_baseplate_bound(
        self, obj: BaseplateObject, field: str, value: float
    ) -> None:
        current = obj.bounds
        values = {
            "min_x": current.min_x,
            "min_y": current.min_y,
            "max_x": current.max_x,
            "max_y": current.max_y,
        }
        values[field] = value
        obj.bounds = Bounds2D(**values)

    def _render_object(self, obj: SceneObject) -> None:
        if isinstance(obj, GdsLayerObject):
            data = self._gds_data.get(obj.id)
            if data is None:
                raise ValueError("missing GDS polygon data")
            self.viewport.rebuild_geometry(obj, data.polygons, _gds_cache_key(data))
        else:
            self.viewport.rebuild_geometry(obj)
        self.viewport.highlight_object(obj.id)

    def _sync_view_after_property(self, obj: SceneObject, field: str) -> None:
        if isinstance(obj, BaseplateObject) and field in {
            "min_x",
            "min_y",
            "max_x",
            "max_y",
        }:
            self._render_object(obj)
            return

        if field in {"color", "brightness", "opacity", "z_min", "z_max", "visible"}:
            self.viewport.update_actor(obj)
            self.viewport.highlight_object(obj.id)

    def _default_baseplate_bounds(self) -> Bounds2D:
        current = self.component_tree.currentItem()
        group_info = self.component_tree.group_info_for_item(current)
        if group_info is not None:
            bounds = self._bounds_for_objects(group_info.object_ids)
            if bounds is not None:
                return bounds

        gds_objects = [
            obj for obj in self.scene.objects() if isinstance(obj, GdsLayerObject)
        ]
        if gds_objects:
            return _merge_bounds([obj.bounds for obj in gds_objects])

        return Bounds2D(min_x=-100.0, min_y=-100.0, max_x=100.0, max_y=100.0)

    def _next_baseplate_name(self) -> str:
        used_indices: set[int] = set()
        prefix = "Baseplate "
        for obj in self.scene.objects():
            if not isinstance(obj, BaseplateObject):
                continue
            if not obj.name.startswith(prefix):
                continue
            suffix = obj.name[len(prefix) :]
            if suffix.isdecimal():
                used_indices.add(int(suffix))

        index = 1
        while index in used_indices:
            index += 1
        return f"{prefix}{index}"

    def _bounds_for_objects(self, object_ids: tuple[str, ...]) -> Bounds2D | None:
        objects = [self.scene.get(object_id) for object_id in object_ids]
        bounds = [obj.bounds for obj in objects if obj is not None]
        if not bounds:
            return None

        return _merge_bounds(bounds)

    def _z_range_for_objects(
        self, object_ids: tuple[str, ...]
    ) -> tuple[float, float] | None:
        objects = [self.scene.get(object_id) for object_id in object_ids]
        z_ranges = [(obj.z_min, obj.z_max) for obj in objects if obj is not None]
        if not z_ranges:
            return None
        return (
            min(z_min for z_min, _z_max in z_ranges),
            max(z_max for _z_min, z_max in z_ranges),
        )

    def _show_error(self, title: str, message: str) -> None:
        self.statusBar().showMessage(message)
        QMessageBox.warning(self, title, message)

    def _export_view(
        self,
        title: str,
        filter_text: str,
        suffix: str,
        export_func: Callable[[Path], None],
    ) -> None:
        default_name = f"project.{suffix}"
        file_name, _ = QFileDialog.getSaveFileName(
            self,
            title,
            str(Path.cwd() / default_name),
            filter_text,
        )
        if not file_name:
            return

        path = self._ensure_suffix(Path(file_name), suffix)
        try:
            export_func(path)
            self.statusBar().showMessage(f"Exported {path.name}")
        except Exception as exc:
            self._show_error("Export failed", str(exc))

    def _write_project(self, file_path: Path) -> None:
        gds_paths = [
            obj.source_path
            for obj in self.scene.objects()
            if isinstance(obj, GdsLayerObject)
        ]
        write_project_archive(file_path, self.scene.objects(), gds_paths)

    def _export_pdf(self, file_path: Path) -> None:
        export_scene_pdf(file_path, self.viewport, self.scene.objects())

    def _load_project(self, file_path: Path) -> None:
        archive_objects, gds_sources = read_project_archive(file_path)
        if self._project_temp_dir is not None:
            self._project_temp_dir.cleanup()
        self._project_temp_dir = TemporaryDirectory()
        temp_root = Path(self._project_temp_dir.name)

        self.scene = Scene()
        self._gds_data.clear()
        self.component_tree.clear()
        self.viewport.clear_scene()
        self.property_panel.show_scene_summary(0)

        gds_paths = self._materialize_gds_sources(gds_sources, temp_root)
        for archive_obj in archive_objects:
            obj = self._restore_object(archive_obj, file_path, gds_paths)
            self.scene.add(obj)
            if isinstance(obj, GdsLayerObject):
                data = self._gds_data[obj.id]
                self.viewport.add_or_update(obj, data.polygons, _gds_cache_key(data))
            else:
                self.viewport.add_or_update(obj)
            self.component_tree.add_object(obj)

        self.viewport.reset_camera()

    def closeEvent(self, event) -> None:  # noqa: N802
        if self._project_temp_dir is not None:
            self._project_temp_dir.cleanup()
            self._project_temp_dir = None
        super().closeEvent(event)

    def _restore_object(
        self,
        archive_obj: ProjectArchiveObject,
        file_path: Path,
        gds_paths: dict[str, Path],
    ) -> SceneObject:
        if archive_obj.kind == "gds_layer":
            payload = archive_obj.payload
            source_key = str(payload["source_key"])
            temp_path = gds_paths.get(source_key)
            if temp_path is None:
                raise ValueError(f"missing embedded GDS source: {source_key}")

            file_info = inspect_gds_file(temp_path)
            selection = next(
                (
                    layer.selection
                    for cell in file_info.cells
                    if cell.name == str(payload["cell_name"])
                    for layer in cell.layers
                    if layer.selection.layer == int(payload["layer"])
                    and layer.selection.datatype == int(payload["datatype"])
                ),
                None,
            )
            if selection is None:
                raise ValueError(f"unable to restore GDS layer: {source_key}")

            layers = load_gds_layers(file_info.file_path, [selection])
            if not layers:
                raise ValueError(f"unable to restore GDS layer: {source_key}")

            layer = layers[0]
            obj = GdsLayerObject(
                name=str(payload["name"]),
                file_path=file_path,
                source_path=temp_path,
                cell_name=str(payload["cell_name"]),
                layer=int(payload["layer"]),
                datatype=int(payload["datatype"]),
                bounds=layer.bounds,
                source_key=source_key,
                z_min=float(payload["z_min"]),
                z_max=float(payload["z_max"]),
                color=str(payload["color"]),
                brightness=float(payload["brightness"]),
                opacity=float(payload["opacity"]),
                visible=bool(payload["visible"]),
            )
            self._gds_data[obj.id] = layer
            return obj

        if archive_obj.kind == "baseplate":
            payload = archive_obj.payload
            bounds_dict = payload["bounds"]
            bounds = Bounds2D(
                min_x=float(bounds_dict["min_x"]),
                min_y=float(bounds_dict["min_y"]),
                max_x=float(bounds_dict["max_x"]),
                max_y=float(bounds_dict["max_y"]),
            )
            return BaseplateObject(
                name=str(payload["name"]),
                bounds=bounds,
                z_min=float(payload["z_min"]),
                z_max=float(payload["z_max"]),
                color=str(payload["color"]),
                brightness=float(payload["brightness"]),
                opacity=float(payload["opacity"]),
                visible=bool(payload["visible"]),
            )

        raise ValueError(f"unsupported archive object kind: {archive_obj.kind}")

    def _materialize_gds_sources(
        self, gds_sources: dict[str, bytes], temp_root: Path
    ) -> dict[str, Path]:
        gds_paths: dict[str, Path] = {}
        for source_name, raw in gds_sources.items():
            temp_path = temp_root / source_name
            temp_path.write_bytes(raw)
            gds_paths[source_name] = temp_path
        return gds_paths

    @staticmethod
    def _ensure_suffix(file_path: Path, suffix: str) -> Path:
        if file_path.suffix.lower() == f".{suffix}":
            return file_path
        return file_path.with_suffix(f".{suffix}")


def _gds_cache_key(data: GdsLayerData) -> tuple[str, str, int, int]:
    return (str(data.file_path), data.cell_name, data.layer, data.datatype)


def _gds_source_key(file_path: Path) -> str:
    resolved = file_path.expanduser().resolve()
    digest = sha1(str(resolved).encode("utf-8")).hexdigest()[:12]
    return f"{resolved.stem}-{digest}{resolved.suffix.lower()}"


def _settings_int(
    settings: QSettings,
    key: str,
    default: int,
    minimum: int,
    maximum: int,
) -> int:
    value = settings.value(key, default)
    try:
        number = int(value)
    except (TypeError, ValueError):
        return default
    return max(minimum, min(number, maximum))


def _merge_bounds(bounds: list[Bounds2D]) -> Bounds2D:
    if not bounds:
        raise ValueError("cannot merge empty bounds")
    return Bounds2D(
        min_x=min(bound.min_x for bound in bounds),
        min_y=min(bound.min_y for bound in bounds),
        max_x=max(bound.max_x for bound in bounds),
        max_y=max(bound.max_y for bound in bounds),
    )
