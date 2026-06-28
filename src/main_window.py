from __future__ import annotations

from pathlib import Path

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QFileDialog,
    QDockWidget,
    QMainWindow,
    QMessageBox,
    QToolBar,
)

from component_tree import ComponentTree
from gds_loader import GdsLayerData, load_default_gds_layer
from objects import BaseplateObject, Bounds2D, GdsLayerObject, SceneObject
from property_panel import PropertyPanel
from scene import Scene
from viewport import Viewport


class MainWindow(QMainWindow):
    def __init__(self) -> None:
        super().__init__()
        self.setWindowTitle("Draw3D")
        self.resize(1280, 820)

        self.scene = Scene()
        self._gds_data: dict[str, GdsLayerData] = {}

        self.viewport = Viewport(self)
        self.component_tree = ComponentTree(self)
        self.property_panel = PropertyPanel(self)

        self.setCentralWidget(self.viewport)
        self._create_docks()
        self._create_toolbar()
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
            data = load_default_gds_layer(Path(file_name))
            obj = GdsLayerObject(
                name=data.file_path.stem,
                file_path=data.file_path,
                cell_name=data.cell_name,
                layer=data.layer,
                datatype=data.datatype,
                bounds=data.bounds,
            )
            self.scene.add(obj)
            self._gds_data[obj.id] = data
            self.viewport.add_or_update(obj, data.polygons, _gds_cache_key(data))
            self.component_tree.add_object(obj)
            self.viewport.reset_camera()
            self.statusBar().showMessage(f"Imported {data.file_path.name}")
        except Exception as exc:
            self._show_error("Import failed", str(exc))

    def create_baseplate(self) -> None:
        bounds = self._default_baseplate_bounds()
        index = self.scene.count() + 1
        obj = BaseplateObject(name=f"Baseplate {index}", bounds=bounds)
        try:
            self.scene.add(obj)
            self.viewport.add_or_update(obj)
            self.component_tree.add_object(obj)
            self.statusBar().showMessage(f"Created {obj.name}")
        except Exception as exc:
            self._show_error("Create baseplate failed", str(exc))

    def delete_selected(self) -> None:
        object_id = self.component_tree.current_object_id()
        if object_id is None:
            return

        obj = self.scene.get(object_id)
        if obj is None:
            return

        self.scene.remove(object_id)
        self._gds_data.pop(object_id, None)
        self.viewport.remove_object(object_id)
        self.component_tree.remove_object(object_id)
        self.property_panel.show_scene_summary(self.scene.count())
        self.statusBar().showMessage(f"Deleted {obj.name}")

    def _create_docks(self) -> None:
        left_dock = QDockWidget("Components", self)
        left_dock.setObjectName("componentsDock")
        left_dock.setWidget(self.component_tree)
        self.addDockWidget(Qt.DockWidgetArea.LeftDockWidgetArea, left_dock)

        right_dock = QDockWidget("Properties", self)
        right_dock.setObjectName("propertiesDock")
        right_dock.setWidget(self.property_panel)
        right_dock.setMinimumWidth(360)
        self.addDockWidget(Qt.DockWidgetArea.RightDockWidgetArea, right_dock)
        self.resizeDocks([right_dock], [390], Qt.Orientation.Horizontal)

    def _create_toolbar(self) -> None:
        toolbar = QToolBar("Tools", self)
        toolbar.setMovable(False)
        toolbar.addAction("Import GDS", self.import_gds)
        toolbar.addAction("Create Baseplate", self.create_baseplate)
        toolbar.addAction("Delete", self.delete_selected)
        toolbar.addSeparator()
        toolbar.addAction("Reset Camera", self.viewport.reset_camera)
        self.addToolBar(Qt.ToolBarArea.TopToolBarArea, toolbar)

    def _select_object(self, object_id: object) -> None:
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
        selected_id = self.component_tree.current_object_id()
        selected = self.scene.get(selected_id) if selected_id is not None else None
        if selected is not None:
            return selected.bounds

        objects = self.scene.objects()
        if objects:
            return objects[-1].bounds

        return Bounds2D(min_x=-100.0, min_y=-100.0, max_x=100.0, max_y=100.0)

    def _show_error(self, title: str, message: str) -> None:
        self.statusBar().showMessage(message)
        QMessageBox.warning(self, title, message)


def _gds_cache_key(data: GdsLayerData) -> tuple[str, str, int, int]:
    return (str(data.file_path), data.cell_name, data.layer, data.datatype)
